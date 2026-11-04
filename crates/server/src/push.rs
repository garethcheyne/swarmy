//! Web Push notifications, implemented directly on pure-Rust crypto:
//!
//! - message encryption: RFC 8291 (`aes128gcm`, RFC 8188 framing),
//! - sender identity: VAPID, RFC 8292 (ES256-signed JWT).
//!
//! Subscriptions the push service reports as gone (404/410) are dropped.

use std::sync::Arc;

use aes_gcm::{Aes128Gcm, KeyInit, Nonce, aead::Aead};
use anyhow::Context;
use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD as B64};
use hkdf::Hkdf;
use p256::{
    PublicKey, SecretKey,
    ecdsa::{Signature, SigningKey, signature::Signer},
    elliptic_curve::sec1::ToEncodedPoint,
};
use serde::Serialize;
use sha2::Sha256;

use crate::store::{PushSubscription, Store, now_unix};

/// What the service worker turns into a system notification.
#[derive(Debug, Clone, Serialize)]
pub struct Notice {
    pub title: String,
    pub body: String,
    /// Notifications with the same tag replace each other.
    pub tag: String,
    /// Where tapping the notification should go.
    pub url: String,
}

pub struct Push {
    vapid: SecretKey,
    /// Uncompressed P-256 point, base64url: the browser's `applicationServerKey`.
    public_key: String,
    contact: String,
    http: reqwest::Client,
}

fn random_secret() -> SecretKey {
    // A random 32-byte string is a valid P-256 scalar with overwhelming probability.
    loop {
        if let Ok(k) = SecretKey::from_slice(&rand::random::<[u8; 32]>()) {
            return k;
        }
    }
}

fn uncompressed(key: &PublicKey) -> Vec<u8> {
    key.to_encoded_point(false).as_bytes().to_vec()
}

impl Push {
    /// Load the VAPID key from the store, generating one on first run.
    pub fn load_or_create(store: &Store, contact: String) -> anyhow::Result<Self> {
        let stored = store.read(|d| d.push.vapid_private.clone());
        let vapid = match B64.decode(&stored).ok().and_then(|b| SecretKey::from_slice(&b).ok()) {
            Some(k) => k,
            None => {
                let k = random_secret();
                let encoded = B64.encode(k.to_bytes());
                store.update(|d| d.push.vapid_private = encoded)?;
                k
            }
        };
        let public_key = B64.encode(uncompressed(&vapid.public_key()));
        let http = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(15))
            .build()
            .context("building push HTTP client")?;
        Ok(Self {
            vapid,
            public_key,
            contact,
            http,
        })
    }

    pub fn public_key(&self) -> &str {
        &self.public_key
    }

    /// RFC 8292: `Authorization: vapid t=<jwt>, k=<public key>`.
    fn vapid_header(&self, endpoint: &str) -> anyhow::Result<String> {
        let url = reqwest::Url::parse(endpoint).context("parsing push endpoint")?;
        let aud = url.origin().ascii_serialization();
        let header = B64.encode(br#"{"typ":"JWT","alg":"ES256"}"#);
        let claims = B64.encode(serde_json::to_vec(&serde_json::json!({
            "aud": aud,
            "exp": now_unix() + 12 * 3600,
            "sub": self.contact,
        }))?);
        let signing_input = format!("{header}.{claims}");
        let sig: Signature = SigningKey::from(&self.vapid).sign(signing_input.as_bytes());
        Ok(format!(
            "vapid t={signing_input}.{}, k={}",
            B64.encode(sig.to_bytes()),
            self.public_key
        ))
    }

    async fn send_one(&self, sub: &PushSubscription, payload: &[u8]) -> anyhow::Result<SendResult> {
        let body = encrypt(&sub.p256dh, &sub.auth, payload)?;
        let resp = self
            .http
            .post(&sub.endpoint)
            .header("Authorization", self.vapid_header(&sub.endpoint)?)
            .header("Content-Encoding", "aes128gcm")
            .header("Content-Type", "application/octet-stream")
            .header("TTL", (24 * 3600).to_string())
            .header("Urgency", "normal")
            .body(body)
            .send()
            .await
            .context("sending push")?;
        let status = resp.status().as_u16();
        Ok(match status {
            200..=299 => SendResult::Delivered,
            404 | 410 => SendResult::Gone,
            _ => {
                let text = resp.text().await.unwrap_or_default();
                anyhow::bail!("push service answered {status}: {}", text.trim())
            }
        })
    }

    /// Send to every subscription (or only `only`), pruning dead ones.
    /// Returns how many deliveries succeeded.
    pub async fn send(
        self: &Arc<Self>,
        store: &Store,
        notice: &Notice,
        only: Option<&str>,
    ) -> anyhow::Result<usize> {
        let subs: Vec<PushSubscription> = store.read(|d| {
            d.push
                .subscriptions
                .iter()
                .filter(|s| only.is_none_or(|e| s.endpoint == e))
                .cloned()
                .collect()
        });
        if subs.is_empty() && only.is_some() {
            anyhow::bail!("this device isn't subscribed");
        }
        let payload = serde_json::to_vec(notice)?;
        let mut delivered = 0;
        let mut gone = Vec::new();
        let mut last_err = None;
        for sub in &subs {
            match self.send_one(sub, &payload).await {
                Ok(SendResult::Delivered) => delivered += 1,
                Ok(SendResult::Gone) => gone.push(sub.endpoint.clone()),
                Err(e) => {
                    tracing::warn!("push to {} failed: {e:#}", host_of(&sub.endpoint));
                    last_err = Some(e);
                }
            }
        }
        if !gone.is_empty() {
            tracing::info!("removing {} expired push subscription(s)", gone.len());
            store.update(|d| d.push.subscriptions.retain(|s| !gone.contains(&s.endpoint)))?;
        }
        match (delivered, last_err) {
            (0, Some(e)) if only.is_some() => Err(e),
            _ => Ok(delivered),
        }
    }
}

enum SendResult {
    Delivered,
    Gone,
}

fn host_of(url: &str) -> &str {
    url.split("://")
        .nth(1)
        .and_then(|r| r.split('/').next())
        .unwrap_or(url)
}

fn hkdf_expand<const N: usize>(salt: &[u8], ikm: &[u8], info: &[u8]) -> anyhow::Result<[u8; N]> {
    let mut out = [0u8; N];
    Hkdf::<Sha256>::new(Some(salt), ikm)
        .expand(info, &mut out)
        .map_err(|_| anyhow::anyhow!("hkdf output too long"))?;
    Ok(out)
}

/// RFC 8291 message encryption, a single `aes128gcm` record.
fn encrypt(p256dh_b64: &str, auth_b64: &str, plaintext: &[u8]) -> anyhow::Result<Vec<u8>> {
    let ua_public_bytes = B64
        .decode(p256dh_b64.trim_end_matches('='))
        .context("decoding subscription p256dh")?;
    let auth_secret = B64
        .decode(auth_b64.trim_end_matches('='))
        .context("decoding subscription auth")?;
    let ua_public = PublicKey::from_sec1_bytes(&ua_public_bytes).context("invalid p256dh key")?;

    let as_secret = random_secret();
    let as_public_bytes = uncompressed(&as_secret.public_key());
    let shared = p256::ecdh::diffie_hellman(as_secret.to_nonzero_scalar(), ua_public.as_affine());

    // IKM = HKDF(auth_secret, ecdh_secret, "WebPush: info" || 0 || ua_public || as_public)
    let mut key_info = b"WebPush: info\0".to_vec();
    key_info.extend_from_slice(&ua_public_bytes);
    key_info.extend_from_slice(&as_public_bytes);
    let ikm: [u8; 32] = hkdf_expand(&auth_secret, shared.raw_secret_bytes(), &key_info)?;

    let salt: [u8; 16] = rand::random();
    let cek: [u8; 16] = hkdf_expand(&salt, &ikm, b"Content-Encoding: aes128gcm\0")?;
    let nonce: [u8; 12] = hkdf_expand(&salt, &ikm, b"Content-Encoding: nonce\0")?;

    // One record: plaintext followed by the 0x02 "last record" delimiter.
    let mut record = plaintext.to_vec();
    record.push(0x02);
    let cipher = Aes128Gcm::new_from_slice(&cek).map_err(|_| anyhow::anyhow!("bad key length"))?;
    let sealed = cipher
        .encrypt(Nonce::from_slice(&nonce), record.as_ref())
        .map_err(|_| anyhow::anyhow!("encrypting push payload"))?;

    // Header: salt (16) || record size (4, BE) || key id length (1) || key id (sender public key).
    let mut body = Vec::with_capacity(86 + sealed.len());
    body.extend_from_slice(&salt);
    body.extend_from_slice(&4096u32.to_be_bytes());
    body.push(as_public_bytes.len() as u8);
    body.extend_from_slice(&as_public_bytes);
    body.extend_from_slice(&sealed);
    Ok(body)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Decrypt as the browser would, to prove the framing and key schedule round-trip.
    #[test]
    fn encrypt_round_trips() {
        let ua_secret = random_secret();
        let ua_public = uncompressed(&ua_secret.public_key());
        let auth: [u8; 16] = rand::random();
        let msg = br#"{"title":"Download finished","body":"Sintel"}"#;

        let body = encrypt(&B64.encode(&ua_public), &B64.encode(auth), msg).unwrap();

        let salt = &body[..16];
        assert_eq!(u32::from_be_bytes(body[16..20].try_into().unwrap()), 4096);
        let idlen = body[20] as usize;
        let as_public = PublicKey::from_sec1_bytes(&body[21..21 + idlen]).unwrap();
        let ciphertext = &body[21 + idlen..];

        let shared = p256::ecdh::diffie_hellman(ua_secret.to_nonzero_scalar(), as_public.as_affine());
        let mut key_info = b"WebPush: info\0".to_vec();
        key_info.extend_from_slice(&ua_public);
        key_info.extend_from_slice(&body[21..21 + idlen]);
        let ikm: [u8; 32] = hkdf_expand(&auth, shared.raw_secret_bytes(), &key_info).unwrap();
        let cek: [u8; 16] = hkdf_expand(salt, &ikm, b"Content-Encoding: aes128gcm\0").unwrap();
        let nonce: [u8; 12] = hkdf_expand(salt, &ikm, b"Content-Encoding: nonce\0").unwrap();
        let plain = Aes128Gcm::new_from_slice(&cek)
            .unwrap()
            .decrypt(Nonce::from_slice(&nonce), ciphertext)
            .unwrap();
        assert_eq!(plain.last(), Some(&0x02));
        assert_eq!(&plain[..plain.len() - 1], msg);
    }

    #[test]
    fn vapid_jwt_verifies() {
        use p256::ecdsa::{VerifyingKey, signature::Verifier};
        let store = Store::open(std::env::temp_dir().join(format!("swarmy-test-{}.json", rand::random::<u32>()))).unwrap();
        let push = Push::load_or_create(&store, "mailto:test@example.com".into()).unwrap();
        let h = push.vapid_header("https://fcm.googleapis.com/fcm/send/abc").unwrap();
        let jwt = h.strip_prefix("vapid t=").unwrap().split(", k=").next().unwrap();
        let mut parts = jwt.split('.');
        let (hdr, claims, sig) = (parts.next().unwrap(), parts.next().unwrap(), parts.next().unwrap());
        let claims: serde_json::Value = serde_json::from_slice(&B64.decode(claims).unwrap()).unwrap();
        assert_eq!(claims["aud"], "https://fcm.googleapis.com");
        let sig = Signature::from_slice(&B64.decode(sig).unwrap()).unwrap();
        VerifyingKey::from(&push.vapid.public_key())
            .verify(format!("{hdr}.{}", jwt.split('.').nth(1).unwrap()).as_bytes(), &sig)
            .unwrap();
        // Same key comes back on reload.
        let again = Push::load_or_create(&store, String::new()).unwrap();
        assert_eq!(again.public_key(), push.public_key());
    }
}
