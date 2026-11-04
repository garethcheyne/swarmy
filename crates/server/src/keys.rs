//! API keys, for Sonarr, Radarr, Prowlarr and scripts. Each has a name saying
//! what it's for and can expire. Only a SHA-256 of a key is kept; the key is
//! shown once, when it's made. Keys work on the qBittorrent API (as a Bearer
//! token) and the SABnzbd API (`apikey=`), not on Swarmy's own web UI API.

use axum::{
    Json, Router,
    extract::{Path, State},
    http::StatusCode,
    routing::{get, patch},
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::{
    api::{ApiError, ApiResult},
    state::Shared,
    store::{Store, now_unix},
};

/// How often "last used" is saved; Sonarr asks every few seconds.
const LAST_USED_EVERY: i64 = 300;
const MAX_NAME: usize = 60;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApiKey {
    pub id: String,
    pub name: String,
    /// SHA-256 (hex) of the key.
    pub hash: String,
    /// The key's last four characters, to match it with an app's settings.
    #[serde(default)]
    pub hint: String,
    pub created: i64,
    /// Unix time it stops working; None = never.
    #[serde(default)]
    pub expires: Option<i64>,
    #[serde(default)]
    pub last_used: Option<i64>,
}

pub fn router() -> Router<Shared> {
    Router::new()
        .route("/keys", get(list).post(create))
        .route("/keys/{id}", patch(rename).delete(revoke))
}

pub fn hash(key: &str) -> String {
    Sha256::digest(key.as_bytes()).iter().map(|b| format!("{b:02x}")).collect()
}

fn constant_eq(a: &str, b: &str) -> bool {
    a.len() == b.len() && a.bytes().zip(b.bytes()).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

/// Is this a current key? Notes when it was used.
pub fn verify(store: &Store, key: &str) -> bool {
    if key.is_empty() {
        return false;
    }
    let h = hash(key);
    let now = now_unix();
    let found = store.read(|d| {
        d.api_keys
            .iter()
            .find(|k| constant_eq(&k.hash, &h))
            .map(|k| (k.id.clone(), k.expires, k.last_used))
    });
    let Some((id, expires, last_used)) = found else { return false };
    if expires.is_some_and(|e| e <= now) {
        return false;
    }
    if last_used.is_none_or(|t| now - t >= LAST_USED_EVERY) {
        let _ = store.update(|d| {
            if let Some(k) = d.api_keys.iter_mut().find(|k| k.id == id) {
                k.last_used = Some(now);
            }
        });
    }
    true
}

/// The single unnamed key from before keys had names becomes a named one.
pub fn migrate(data: &mut crate::store::StoreData) -> bool {
    let old = std::mem::take(&mut data.settings.webui.api_key_hash);
    if old.is_empty() {
        return false;
    }
    if !data.api_keys.iter().any(|k| k.hash == old) {
        data.api_keys.push(ApiKey {
            id: new_id(),
            name: "API key".into(),
            hash: old,
            hint: String::new(),
            created: now_unix(),
            expires: None,
            last_used: None,
        });
    }
    true
}

fn new_id() -> String {
    rand::random::<[u8; 6]>().iter().map(|b| format!("{b:02x}")).collect()
}

#[derive(Serialize)]
pub struct KeyView {
    id: String,
    name: String,
    hint: String,
    created: i64,
    expires: Option<i64>,
    last_used: Option<i64>,
    expired: bool,
}

impl KeyView {
    fn of(k: &ApiKey, now: i64) -> Self {
        Self {
            id: k.id.clone(),
            name: k.name.clone(),
            hint: k.hint.clone(),
            created: k.created,
            expires: k.expires,
            last_used: k.last_used,
            expired: k.expires.is_some_and(|e| e <= now),
        }
    }
}

async fn list(State(state): State<Shared>) -> Json<Vec<KeyView>> {
    let now = now_unix();
    Json(state.store.read(|d| d.api_keys.iter().map(|k| KeyView::of(k, now)).collect()))
}

/// A trimmed name that no other key has.
fn check_name(state: &Shared, name: &str, except: Option<&str>) -> ApiResult<String> {
    let name = name.trim();
    if name.is_empty() {
        return Err(ApiError::bad_request("Give the key a name, like Sonarr, so you know what it's for."));
    }
    if name.chars().count() > MAX_NAME {
        return Err(ApiError::bad_request(format!("Keep the name to {MAX_NAME} characters or fewer.")));
    }
    let taken = state.store.read(|d| {
        d.api_keys
            .iter()
            .find(|k| Some(k.id.as_str()) != except && k.name.eq_ignore_ascii_case(name))
            .map(|k| k.name.clone())
    });
    if let Some(taken) = taken {
        return Err(ApiError::bad_request(format!("There's already a key named {taken}. Pick another name.")));
    }
    Ok(name.to_owned())
}

#[derive(Deserialize)]
struct NewKey {
    name: String,
    /// Unix time; absent or null = never expires.
    #[serde(default)]
    expires: Option<i64>,
}

#[derive(Serialize)]
struct Created {
    /// Shown this once; only its hash is kept.
    key: String,
    #[serde(flatten)]
    view: KeyView,
}

async fn create(State(state): State<Shared>, Json(req): Json<NewKey>) -> ApiResult<(StatusCode, Json<Created>)> {
    let name = check_name(&state, &req.name, None)?;
    let now = now_unix();
    if req.expires.is_some_and(|e| e <= now) {
        return Err(ApiError::bad_request("The expiry date has to be in the future."));
    }
    let secret: String = rand::random::<[u8; 24]>().iter().map(|b| format!("{b:02x}")).collect();
    let key = format!("swarmy_{secret}");
    let entry = ApiKey {
        id: new_id(),
        name,
        hash: hash(&key),
        hint: key[key.len() - 4..].to_owned(),
        created: now,
        expires: req.expires,
        last_used: None,
    };
    let view = KeyView::of(&entry, now);
    state.store.update(|d| d.api_keys.push(entry))?;
    Ok((StatusCode::CREATED, Json(Created { key, view })))
}

#[derive(Deserialize)]
struct Rename {
    name: String,
}

async fn rename(State(state): State<Shared>, Path(id): Path<String>, Json(req): Json<Rename>) -> ApiResult<Json<KeyView>> {
    let name = check_name(&state, &req.name, Some(&id))?;
    let now = now_unix();
    let view = state.store.update(|d| {
        d.api_keys.iter_mut().find(|k| k.id == id).map(|k| {
            k.name = name;
            KeyView::of(k, now)
        })
    })?;
    view.map(Json).ok_or_else(|| ApiError(StatusCode::NOT_FOUND, "That key isn't there any more.".into()))
}

async fn revoke(State(state): State<Shared>, Path(id): Path<String>) -> ApiResult<StatusCode> {
    let removed = state.store.update(|d| {
        let before = d.api_keys.len();
        d.api_keys.retain(|k| k.id != id);
        d.api_keys.len() != before
    })?;
    if removed { Ok(StatusCode::NO_CONTENT) } else { Err(ApiError(StatusCode::NOT_FOUND, "That key isn't there any more.".into())) }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::StoreData;

    fn store_with(keys: Vec<ApiKey>) -> (tempfile::TempDir, Store) {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(dir.path().join("store.json")).unwrap();
        store.update(|d| d.api_keys = keys).unwrap();
        (dir, store)
    }

    fn key(name: &str, secret: &str, expires: Option<i64>) -> ApiKey {
        ApiKey {
            id: new_id(),
            name: name.into(),
            hash: hash(secret),
            hint: String::new(),
            created: 0,
            expires,
            last_used: None,
        }
    }

    #[test]
    fn verify_checks_every_key_and_expiry() {
        let now = now_unix();
        let (_dir, store) = store_with(vec![
            key("Sonarr", "one", None),
            key("Radarr", "two", Some(now + 3600)),
            key("Old", "three", Some(now - 1)),
        ]);
        assert!(verify(&store, "one"));
        assert!(verify(&store, "two"));
        assert!(!verify(&store, "three"), "expired keys stop working");
        assert!(!verify(&store, "four"));
        assert!(!verify(&store, ""));
        assert!(store.read(|d| d.api_keys[0].last_used.is_some()), "last used is noted");
    }

    #[test]
    fn old_single_key_becomes_named() {
        let mut d = StoreData::default();
        d.settings.webui.api_key_hash = hash("legacy");
        assert!(migrate(&mut d));
        assert!(d.settings.webui.api_key_hash.is_empty());
        assert_eq!(d.api_keys.len(), 1);
        assert_eq!(d.api_keys[0].name, "API key");
        assert_eq!(d.api_keys[0].hash, hash("legacy"));
        assert!(!migrate(&mut d), "nothing more to do the second time");
    }

    #[test]
    fn compare() {
        let h = hash("secret");
        assert!(constant_eq(&h, &hash("secret")));
        assert!(!constant_eq(&h, &hash("other")));
    }
}
