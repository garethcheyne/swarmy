//! Single-user cookie auth, all in the daemon. The cookie is named `SID` on
//! purpose: it's what qBittorrent uses, and what Sonarr/Radarr expect back
//! from `/api/v2/auth/login`.
//!
//! - Passwords: Argon2id hashes only.
//! - Sessions: random 256-bit tokens; only their SHA-256 is persisted, so
//!   restarts keep people signed in and a leaked data file can't be replayed.
//! - Lockout: after N failed logins from one client address it's refused for
//!   a while. Behind a reverse proxy the client address comes from
//!   `X-Forwarded-For`, but only when the request arrives from a trusted proxy.

use std::{
    collections::HashMap,
    net::{IpAddr, SocketAddr},
    sync::Mutex,
    time::{Duration, Instant},
};

use argon2::{Argon2, PasswordHasher, PasswordVerifier};
use axum::{
    extract::{Request, State},
    http::{HeaderMap, StatusCode},
    middleware::Next,
    response::{IntoResponse, Response},
};
use axum_extra::extract::cookie::{Cookie, CookieJar, SameSite};
use sha2::{Digest, Sha256};

use crate::{
    state::Shared,
    store::{Account, Store, StoredSession, now_unix},
};

pub const COOKIE_NAME: &str = "SID";

#[derive(Default)]
pub struct Auth {
    failures: Mutex<HashMap<IpAddr, Failures>>,
}

struct Failures {
    count: u32,
    first: Instant,
    locked_until: Option<Instant>,
}

fn token_key(token: &str) -> String {
    hex(&Sha256::digest(token.as_bytes()))
}

impl Auth {
    /// Seconds left on a lockout for this address, if any.
    pub fn locked_for(&self, ip: IpAddr) -> Option<u64> {
        let f = self.failures.lock().unwrap();
        let until = f.get(&ip)?.locked_until?;
        let now = Instant::now();
        (until > now).then(|| (until - now).as_secs().max(1))
    }

    pub fn record_failure(&self, ip: IpAddr, max_attempts: u32, lockout: Duration) {
        if max_attempts == 0 {
            return;
        }
        let mut f = self.failures.lock().unwrap();
        let now = Instant::now();
        let e = f.entry(ip).or_insert(Failures { count: 0, first: now, locked_until: None });
        // Failures older than the lockout window don't count.
        if now.duration_since(e.first) > lockout {
            *e = Failures { count: 0, first: now, locked_until: None };
        }
        e.count += 1;
        if e.count >= max_attempts {
            e.locked_until = Some(now + lockout);
            tracing::warn!("locking out {ip} for {} min after {} failed logins", lockout.as_secs() / 60, e.count);
        }
    }

    pub fn clear_failures(&self, ip: IpAddr) {
        self.failures.lock().unwrap().remove(&ip);
    }
}

/// Create and persist a session; returns the raw token for the cookie.
pub fn create_session(store: &Store, ttl_days: u32, client: &str) -> anyhow::Result<String> {
    let token = hex(&rand::random::<[u8; 32]>());
    let now = now_unix();
    let expires = now + i64::from(ttl_days.max(1)) * 86400;
    store.update(|d| {
        d.sessions.retain(|_, s| s.expires > now);
        d.sessions.insert(
            token_key(&token),
            StoredSession { expires, created: now, client: client.to_owned() },
        );
    })?;
    Ok(token)
}

pub fn session_valid(store: &Store, token: &str) -> bool {
    let key = token_key(token);
    store.read(|d| d.sessions.get(&key).is_some_and(|s| s.expires > now_unix()))
}

pub fn revoke(store: &Store, token: &str) -> anyhow::Result<()> {
    let key = token_key(token);
    store.update(|d| {
        d.sessions.remove(&key);
    })
}

/// Sign out every session except `keep`. Returns how many were removed.
pub fn revoke_others(store: &Store, keep: Option<&str>) -> anyhow::Result<usize> {
    let keep = keep.map(token_key);
    store.update(|d| {
        let before = d.sessions.len();
        d.sessions.retain(|k, _| keep.as_deref() == Some(k.as_str()));
        before - d.sessions.len()
    })
}

pub fn session_cookie(token: String, ttl_days: u32, secure: bool) -> Cookie<'static> {
    Cookie::build((COOKIE_NAME, token))
        .path("/")
        .http_only(true)
        .secure(secure)
        .same_site(SameSite::Strict)
        .max_age(
            Duration::from_secs(u64::from(ttl_days.max(1)) * 86400)
                .try_into()
                .expect("session length fits a cookie max-age"),
        )
        .build()
}

/// Who is really on the other end, and did they come in over HTTPS?
pub struct Client {
    pub ip: IpAddr,
    pub https: bool,
}

fn trusted(ip: IpAddr, list: &[String]) -> bool {
    list.iter().any(|entry| {
        let entry = entry.trim();
        if let Ok(exact) = entry.parse::<IpAddr>() {
            return exact == ip;
        }
        // IPv4 CIDR, e.g. 10.0.0.0/8
        let Some((net, bits)) = entry.split_once('/') else { return false };
        let (Ok(IpAddr::V4(net)), Ok(bits), IpAddr::V4(ip)) = (net.parse::<IpAddr>(), bits.parse::<u32>(), ip) else {
            return false;
        };
        if bits > 32 {
            return false;
        }
        let mask = if bits == 0 { 0 } else { u32::MAX << (32 - bits) };
        (u32::from(net) & mask) == (u32::from(ip) & mask)
    })
}

pub fn client(peer: SocketAddr, headers: &HeaderMap, trusted_proxies: &[String]) -> Client {
    let peer_ip = peer.ip().to_canonical();
    if !trusted(peer_ip, trusted_proxies) {
        return Client { ip: peer_ip, https: false };
    }
    // Walk X-Forwarded-For from the right, skipping our own trusted proxies.
    let forwarded = headers
        .get_all("x-forwarded-for")
        .iter()
        .filter_map(|v| v.to_str().ok())
        .flat_map(|v| v.split(','))
        .map(str::trim)
        .filter_map(|s| s.parse::<IpAddr>().ok())
        .collect::<Vec<_>>();
    let ip = forwarded
        .iter()
        .rev()
        .copied()
        .find(|ip| !trusted(*ip, trusted_proxies))
        .unwrap_or(peer_ip);
    let https = headers
        .get("x-forwarded-proto")
        .and_then(|v| v.to_str().ok())
        .is_some_and(|p| p.eq_ignore_ascii_case("https"));
    Client { ip, https }
}

pub fn hash_password(password: &str) -> anyhow::Result<String> {
    Argon2::default()
        .hash_password(password.as_bytes())
        .map(|h| h.to_string())
        .map_err(|e| anyhow::anyhow!("hashing password: {e}"))
}

/// Verifies credentials off the async runtime (argon2 is deliberately slow).
pub async fn check_credentials(state: &Shared, username: &str, password: &str) -> bool {
    let Some(account) = state.store.read(|d| d.account.clone()) else {
        return false;
    };
    if account.username != username {
        return false;
    }
    let password = password.to_owned();
    tokio::task::spawn_blocking(move || {
        PasswordVerifier::<str>::verify_password(
            &Argon2::default(),
            password.as_bytes(),
            account.password_hash.as_str(),
        )
        .is_ok()
    })
    .await
    .unwrap_or(false)
}

/// Ensure an account exists; apply `--password` if given. Logs a generated
/// password when creating the account for the first time.
pub fn ensure_account(store: &Store, username: &str, password: Option<&str>) -> anyhow::Result<()> {
    let existing = store.read(|d| d.account.clone());
    let (password, generated) = match (password, &existing) {
        (Some(p), _) => (p.to_owned(), false),
        (None, Some(_)) => return Ok(()),
        (None, None) => (hex(&rand::random::<[u8; 9]>()), true),
    };
    let account = Account {
        username: username.to_owned(),
        password_hash: hash_password(&password)?,
    };
    store.update(|d| d.account = Some(account))?;
    if generated {
        tracing::warn!(
            "No web UI account existed, created one.\n\n    username: {username}\n    password: {password}\n\n\
             Change it in Settings, or with --password / SWARMY_PASSWORD."
        );
    }
    Ok(())
}

/// Middleware: reject requests without a valid session cookie.
pub async fn require_session(
    State(state): State<Shared>,
    jar: CookieJar,
    req: Request,
    next: Next,
) -> Response {
    match jar.get(COOKIE_NAME) {
        Some(c) if session_valid(&state.store, c.value()) => next.run(req).await,
        _ => (StatusCode::UNAUTHORIZED, "Unauthorized").into_response(),
    }
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn trusted_matches_ips_and_cidrs() {
        let list = vec!["127.0.0.1".to_string(), "10.0.0.0/8".into(), "::1".into()];
        assert!(trusted("127.0.0.1".parse().unwrap(), &list));
        assert!(trusted("10.20.30.40".parse().unwrap(), &list));
        assert!(trusted("::1".parse().unwrap(), &list));
        assert!(!trusted("192.168.1.5".parse().unwrap(), &list));
    }

    #[test]
    fn forwarded_for_only_from_trusted_peer() {
        let list = vec!["127.0.0.1".to_string()];
        let mut h = HeaderMap::new();
        h.insert("x-forwarded-for", "203.0.113.9, 127.0.0.1".parse().unwrap());
        h.insert("x-forwarded-proto", "https".parse().unwrap());
        let via_proxy = client("127.0.0.1:5000".parse().unwrap(), &h, &list);
        assert_eq!(via_proxy.ip, "203.0.113.9".parse::<IpAddr>().unwrap());
        assert!(via_proxy.https);
        // A direct client can't spoof its address or claim HTTPS.
        let direct = client("198.51.100.7:5000".parse().unwrap(), &h, &list);
        assert_eq!(direct.ip, "198.51.100.7".parse::<IpAddr>().unwrap());
        assert!(!direct.https);
    }

    #[test]
    fn lockout_after_max_attempts() {
        let a = Auth::default();
        let ip: IpAddr = "203.0.113.9".parse().unwrap();
        for _ in 0..4 {
            a.record_failure(ip, 5, Duration::from_secs(60));
        }
        assert!(a.locked_for(ip).is_none());
        a.record_failure(ip, 5, Duration::from_secs(60));
        assert!(a.locked_for(ip).is_some());
        a.clear_failures(ip);
        assert!(a.locked_for(ip).is_none());
    }
}
