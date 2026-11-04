//! Swarmy's own JSON API, used by the web UI. Mounted at `/api`.

use std::collections::BTreeMap;

use axum::{
    Json, Router,
    extract::{ConnectInfo, Multipart, Path, Query, State},
    http::{HeaderMap, StatusCode},
    middleware,
    response::{IntoResponse, Response},
    routing::{get, post, put},
};
use axum_extra::extract::cookie::CookieJar;
use std::net::SocketAddr;
use swarmy_engine::{AddOptions, AddSource, ContentLayout, FileEntry, PieceMap};
use serde::{Deserialize, Serialize};

use crate::{
    auth::{self, COOKIE_NAME},
    state::{Shared, Snapshot, TorrentView},
    store::{Category, TorrentMeta, now_unix},
    ws,
};

pub fn router(state: Shared) -> Router<Shared> {
    let protected = Router::new()
        .route("/auth/me", get(me))
        .route("/auth/password", post(change_password))
        .route("/auth/logout-others", post(logout_others))
        .route("/settings", get(get_settings).put(put_settings))
        .route("/speed/alt", post(set_alt_speed))
        .route("/apikey", post(create_api_key).delete(delete_api_key))
        .route("/tags", get(list_tags))
        .route("/tags/{name}", put(put_tag).delete(delete_tag))
        .route("/torrents", get(list_torrents).post(add_torrents))
        .route("/torrents/pause", post(pause))
        .route("/torrents/resume", post(resume))
        .route("/torrents/delete", post(delete))
        .route("/torrents/{hash}/files", get(files))
        .route("/torrents/{hash}/pieces", get(pieces))
        .route("/push", get(push_info))
        .route("/push/subscribe", post(push_subscribe))
        .route("/push/unsubscribe", post(push_unsubscribe))
        .route("/push/prefs", put(push_prefs))
        .route("/push/test", post(push_test))
        .route("/torrents/{hash}/meta", put(set_meta))
        .route("/categories", get(list_categories))
        .route("/categories/{name}", put(put_category).delete(delete_category))
        .route("/ws", get(ws::handler))
        .route("/stats/live", get(stats_live))
        .route("/stats/history", get(stats_history))
        .route("/stats/summary", get(stats_summary))
        .route("/stats/events", get(stats_events))
        .route("/info", get(info))
        .route("/changelog", get(changelog))
        .route("/peers", get(peer_map))
        .route("/geo", get(geo_status))
        .route("/geo/update", post(geo_update))
        // Merged before the auth layer: a layer only covers routes that
        // already exist when it's applied.
        .merge(crate::usenet::router())
        .route_layer(middleware::from_fn_with_state(state, auth::require_session));

    Router::new()
        // SABnzbd's API lives at /api itself; it checks its own key.
        .route("/", get(crate::sab::api).post(crate::sab::api))
        .route("/auth/login", post(login))
        .route("/auth/logout", post(logout))
        .merge(protected)
}

// ---------- errors ----------

pub struct ApiError(pub StatusCode, pub String);

impl ApiError {
    pub fn bad_request(msg: impl Into<String>) -> Self {
        Self(StatusCode::BAD_REQUEST, msg.into())
    }

    pub fn message(&self) -> &str {
        &self.1
    }
}

impl<E: Into<anyhow::Error>> From<E> for ApiError {
    fn from(e: E) -> Self {
        Self(StatusCode::INTERNAL_SERVER_ERROR, format!("{:#}", e.into()))
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        #[derive(Serialize)]
        struct Body {
            error: String,
        }
        (self.0, Json(Body { error: self.1 })).into_response()
    }
}

pub type ApiResult<T> = Result<T, ApiError>;

// ---------- auth ----------

#[derive(Deserialize)]
struct LoginReq {
    username: String,
    password: String,
}

async fn login(
    State(state): State<Shared>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    jar: CookieJar,
    Json(req): Json<LoginReq>,
) -> Response {
    let web = state.store.read(|d| d.settings.webui.clone());
    let client = auth::client(peer, &headers, &web.trusted_proxies);
    if let Some(secs) = state.auth.locked_for(client.ip) {
        let mins = secs.div_ceil(60);
        let plural = if mins == 1 { "" } else { "s" };
        return ApiError(
            StatusCode::TOO_MANY_REQUESTS,
            format!("Too many failed sign-ins. Try again in {mins} minute{plural}."),
        )
        .into_response();
    }
    if auth::check_credentials(&state, &req.username, &req.password).await {
        state.auth.clear_failures(client.ip);
        match auth::create_session(&state.store, web.session_days, &client.ip.to_string()) {
            Ok(token) => (
                jar.add(auth::session_cookie(token, web.session_days, client.https)),
                StatusCode::NO_CONTENT,
            )
                .into_response(),
            Err(e) => ApiError::from(e).into_response(),
        }
    } else {
        state.auth.record_failure(
            client.ip,
            web.max_attempts,
            std::time::Duration::from_secs(u64::from(web.lockout_minutes.max(1)) * 60),
        );
        // Slow down guessing a little.
        tokio::time::sleep(std::time::Duration::from_millis(500)).await;
        ApiError(StatusCode::UNAUTHORIZED, "Invalid username or password".into()).into_response()
    }
}

async fn logout(State(state): State<Shared>, jar: CookieJar) -> impl IntoResponse {
    if let Some(c) = jar.get(COOKIE_NAME) {
        let _ = auth::revoke(&state.store, c.value());
    }
    (jar.remove(COOKIE_NAME), StatusCode::NO_CONTENT)
}

#[derive(Deserialize)]
struct PasswordReq {
    current_password: String,
    #[serde(default)]
    username: Option<String>,
    #[serde(default)]
    new_password: Option<String>,
}

async fn change_password(
    State(state): State<Shared>,
    jar: CookieJar,
    Json(req): Json<PasswordReq>,
) -> ApiResult<Json<serde_json::Value>> {
    let username = state
        .store
        .read(|d| d.account.as_ref().map(|a| a.username.clone()))
        .unwrap_or_default();
    if !auth::check_credentials(&state, &username, &req.current_password).await {
        return Err(ApiError(StatusCode::FORBIDDEN, "Your current password is wrong.".into()));
    }
    let new_name = req.username.map(|u| u.trim().to_owned()).filter(|u| !u.is_empty());
    let new_pw = req.new_password.filter(|p| !p.is_empty());
    if let Some(p) = &new_pw
        && p.chars().count() < 8
    {
        return Err(ApiError::bad_request("Use at least 8 characters for the new password."));
    }
    let hash = match &new_pw {
        Some(p) => Some(auth::hash_password(p)?),
        None => None,
    };
    state.store.update(|d| {
        if let Some(a) = d.account.as_mut() {
            if let Some(n) = new_name {
                a.username = n;
            }
            if let Some(h) = hash {
                a.password_hash = h;
            }
        }
    })?;
    // A new password signs out every other session.
    let signed_out = if new_pw.is_some() {
        auth::revoke_others(&state.store, jar.get(COOKIE_NAME).map(|c| c.value()))?
    } else {
        0
    };
    Ok(Json(serde_json::json!({ "signed_out": signed_out })))
}

async fn logout_others(State(state): State<Shared>, jar: CookieJar) -> ApiResult<Json<serde_json::Value>> {
    let n = auth::revoke_others(&state.store, jar.get(COOKIE_NAME).map(|c| c.value()))?;
    Ok(Json(serde_json::json!({ "signed_out": n })))
}

// ---------- settings ----------

async fn get_settings(State(state): State<Shared>) -> Json<serde_json::Value> {
    let (mut settings, sessions) = state.store.read(|d| (d.settings.clone(), d.sessions.len()));
    let restart_needed = connection_changed(&state, &settings);
    let api_key_set = !settings.webui.api_key_hash.is_empty();
    // The browser never needs the key's hash, or Usenet server passwords.
    settings.webui.api_key_hash.clear();
    crate::usenet::redact(&mut settings.usenet);
    Json(serde_json::json!({
        "settings": settings,
        "locked": state.locked,
        "restart_needed": restart_needed,
        "sessions": sessions,
        "effective_download_dir": state.download_dir(),
        "data_dir": state.data_dir,
        "api_key_set": api_key_set,
        "qbit_version": crate::qbit::APP_VERSION,
        "qbit_webapi": crate::qbit::WEBAPI_VERSION,
    }))
}

fn connection_changed(state: &Shared, s: &crate::store::Settings) -> bool {
    let a = &state.startup_connection;
    let b = &s.connection;
    // Locked fields can't change, so only compare what Settings controls.
    let lock = |f: &str| state.locked.contains(&f);
    (!lock("connection.listen_port") && a.listen_port != b.listen_port)
        || (!lock("connection.upnp") && a.upnp != b.upnp)
        || (!lock("connection.bind_interface") && a.bind_interface != b.bind_interface)
        || (!lock("connection.ipv4_only") && a.ipv4_only != b.ipv4_only)
        || a.dht != b.dht
        || a.lsd != b.lsd
        || a.peer_limit != b.peer_limit
        || a.socks_proxy != b.socks_proxy
        || a.blocklist_url != b.blocklist_url
}

async fn put_settings(
    State(state): State<Shared>,
    Json(mut new): Json<crate::store::Settings>,
) -> ApiResult<Json<serde_json::Value>> {
    let old_usenet = state.store.read(|d| d.settings.usenet.clone());
    crate::usenet::merge_and_validate(&mut new.usenet, &old_usenet).map_err(ApiError::bad_request)?;
    let usenet_changed = serde_json::to_string(&new.usenet).ok() != serde_json::to_string(&old_usenet).ok();
    let c = &new.connection;
    if c.listen_port < 1024 {
        return Err(ApiError::bad_request("Pick a listening port of 1024 or higher."));
    }
    if !c.socks_proxy.trim().is_empty() && !c.socks_proxy.trim().starts_with("socks5://") {
        return Err(ApiError::bad_request("The proxy must look like socks5://host:port."));
    }
    if let Some(r) = new.seeding.ratio_limit
        && !(0.0..=1000.0).contains(&r)
    {
        return Err(ApiError::bad_request("The ratio limit must be between 0 and 1000."));
    }
    if new.queue.enabled
        && (new.queue.max_active == 0 || (new.queue.max_downloads == 0 && new.queue.max_uploads == 0))
    {
        return Err(ApiError::bad_request("With queueing on, allow at least one active torrent."));
    }
    if new.speed.schedule.enabled && new.speed.schedule.from == new.speed.schedule.to {
        return Err(ApiError::bad_request("The schedule's start and end times are the same."));
    }
    if new.webui.session_days == 0 || new.webui.session_days > 365 {
        return Err(ApiError::bad_request("Keep people signed in for between 1 and 365 days."));
    }
    if let Some(h) = new.peer_map.home
        && !((-90.0..=90.0).contains(&h.lat) && (-180.0..=180.0).contains(&h.lon))
    {
        return Err(ApiError::bad_request("That location isn't on the map."));
    }
    let locations_changed = new.peer_map.locations != state.store.read(|d| d.settings.peer_map.locations);
    let queue_turned_off = !new.queue.enabled && state.store.read(|d| d.settings.queue.enabled);
    state.store.update(|d| {
        let key = std::mem::take(&mut d.settings.webui.api_key_hash);
        d.settings = new.clone();
        d.settings.webui.api_key_hash = key;
    })?;
    if usenet_changed {
        crate::usenet::apply(&state);
    }
    if locations_changed {
        let geo = state.geo.clone();
        if new.peer_map.locations {
            tokio::spawn(async move { geo.refresh().await });
        } else {
            geo.disable();
        }
    }
    // Turning the queue off releases everything it was holding.
    if queue_turned_off {
        let held: Vec<String> = state.store.read(|d| {
            d.torrents
                .iter()
                .filter(|(_, m)| m.queued)
                .map(|(h, _)| h.clone())
                .collect()
        });
        for h in &held {
            let _ = state.engine.resume(h).await;
        }
        state
            .store
            .update(|d| d.torrents.values_mut().for_each(|m| m.queued = false))?;
    }
    Ok(Json(serde_json::json!({ "restart_needed": connection_changed(&state, &new) })))
}

#[derive(Deserialize)]
struct AltReq {
    enabled: bool,
}

async fn set_alt_speed(State(state): State<Shared>, Json(req): Json<AltReq>) -> ApiResult<StatusCode> {
    state.store.update(|d| d.settings.speed.alt_enabled = req.enabled)?;
    Ok(StatusCode::NO_CONTENT)
}

// ---------- API key (qBittorrent-compatible API) ----------

/// Make a new key, store only its hash, and return the key this once.
async fn create_api_key(State(state): State<Shared>) -> ApiResult<Json<serde_json::Value>> {
    let key: String = rand::random::<[u8; 24]>().iter().map(|b| format!("{b:02x}")).collect();
    let key = format!("swarmy_{key}");
    let hash = crate::qbit::api_key_hash(&key);
    state.store.update(|d| d.settings.webui.api_key_hash = hash)?;
    Ok(Json(serde_json::json!({ "api_key": key })))
}

async fn delete_api_key(State(state): State<Shared>) -> ApiResult<StatusCode> {
    state.store.update(|d| d.settings.webui.api_key_hash.clear())?;
    Ok(StatusCode::NO_CONTENT)
}

// ---------- tags ----------

async fn list_tags(State(state): State<Shared>) -> Json<Vec<String>> {
    Json(state.store.read(|d| {
        let mut all = d.tags.clone();
        for m in d.torrents.values() {
            all.extend(m.tags.iter().cloned());
        }
        all.into_iter().collect()
    }))
}

async fn put_tag(State(state): State<Shared>, Path(name): Path<String>) -> ApiResult<StatusCode> {
    let name = name.trim().to_owned();
    if name.is_empty() || name.contains(',') {
        return Err(ApiError::bad_request("Tags can't be empty or contain commas."));
    }
    state.store.update(|d| {
        d.tags.insert(name);
    })?;
    Ok(StatusCode::NO_CONTENT)
}

async fn delete_tag(State(state): State<Shared>, Path(name): Path<String>) -> ApiResult<StatusCode> {
    state.store.update(|d| {
        d.tags.remove(&name);
        for m in d.torrents.values_mut() {
            m.tags.remove(&name);
        }
    })?;
    Ok(StatusCode::NO_CONTENT)
}


async fn me(State(state): State<Shared>) -> Json<serde_json::Value> {
    let username = state
        .store
        .read(|d| d.account.as_ref().map(|a| a.username.clone()));
    Json(serde_json::json!({ "username": username }))
}

// ---------- torrents ----------

async fn list_torrents(State(state): State<Shared>) -> Json<Vec<TorrentView>> {
    let snap: std::sync::Arc<Snapshot> = state.snapshot.borrow().clone();
    Json(snap.torrents.clone())
}

#[derive(Default)]
pub struct AddRequest {
    pub urls: Vec<String>,
    pub files: Vec<Vec<u8>>,
    pub category: String,
    pub tags: Vec<String>,
    pub save_path: Option<String>,
    /// None = follow Settings > Downloads > add paused.
    pub paused: Option<bool>,
    pub layout: ContentLayout,
    /// qBittorrent share limits (-2 global, -1 none); None = not given.
    pub ratio_limit: Option<f64>,
    pub seeding_time_limit: Option<i64>,
    pub inactive_seeding_time_limit: Option<i64>,
}

impl AddRequest {
    /// Apply one qBittorrent-style form field (from multipart or urlencoded).
    pub fn field(&mut self, name: &str, text: &str) {
        let num = |t: &str| t.trim().parse::<f64>().ok();
        match name {
            "urls" => self.urls.extend(
                text.split(['\n', '\r'])
                    .map(str::trim)
                    .filter(|l| !l.is_empty())
                    .map(String::from),
            ),
            "category" => self.category = text.trim().to_owned(),
            "tags" => self.tags.extend(
                text.split(',')
                    .map(str::trim)
                    .filter(|t| !t.is_empty())
                    .map(String::from),
            ),
            "savepath" => self.save_path = Some(text.to_owned()),
            "paused" | "stopped" => self.paused = Some(text.trim() == "true"),
            "contentLayout" => {
                self.layout = match text.trim() {
                    "Subfolder" => ContentLayout::Subfolder,
                    "NoSubfolder" => ContentLayout::NoSubfolder,
                    _ => ContentLayout::Original,
                }
            }
            // Older qBittorrent: root_folder=true/false.
            "root_folder" => {
                self.layout = if text.trim() == "true" { ContentLayout::Subfolder } else { ContentLayout::NoSubfolder }
            }
            "ratioLimit" => self.ratio_limit = num(text),
            "seedingTimeLimit" => self.seeding_time_limit = num(text).map(|v| v as i64),
            "inactiveSeedingTimeLimit" => self.inactive_seeding_time_limit = num(text).map(|v| v as i64),
            _ => {}
        }
    }

    /// Parse qBittorrent-style multipart fields, so the same parser serves
    /// both our UI and `/api/v2/torrents/add`.
    pub async fn from_multipart(mut mp: Multipart) -> ApiResult<Self> {
        let mut req = AddRequest::default();
        while let Some(field) = mp
            .next_field()
            .await
            .map_err(|e| ApiError::bad_request(e.to_string()))?
        {
            let name = field.name().unwrap_or_default().to_owned();
            if name == "torrents" {
                let bytes = field
                    .bytes()
                    .await
                    .map_err(|e| ApiError::bad_request(e.to_string()))?;
                req.files.push(bytes.to_vec());
                continue;
            }
            let text = field
                .text()
                .await
                .map_err(|e| ApiError::bad_request(e.to_string()))?;
            req.field(&name, &text);
        }
        Ok(req)
    }
}

pub struct Added {
    pub hash: String,
    pub already_existed: bool,
}

/// Add every URL/file in the request.
pub async fn add_all(state: &Shared, req: AddRequest) -> ApiResult<Vec<Added>> {
    if req.urls.is_empty() && req.files.is_empty() {
        return Err(ApiError::bad_request("no torrents or URLs given"));
    }
    let save_path = state.resolve_save_path(req.save_path.as_deref(), &req.category);
    let paused = req
        .paused
        .unwrap_or_else(|| state.store.read(|d| d.settings.downloads.add_paused));

    let sources: Vec<AddSource> = req
        .urls
        .iter()
        .cloned()
        .map(AddSource::Url)
        .chain(req.files.iter().cloned().map(AddSource::TorrentFile))
        .collect();

    let mut added = Vec::new();
    for source in sources {
        let outcome = state
            .engine
            .add(
                source,
                AddOptions {
                    save_path: Some(save_path.clone()),
                    paused,
                    layout: req.layout,
                },
            )
            .await?;
        if !outcome.already_existed {
            let (name, size) = state
                .engine
                .status(&outcome.info_hash)
                .map(|s| (s.name.unwrap_or_default(), s.total_bytes))
                .unwrap_or_default();
            state
                .stats
                .event("added", &outcome.info_hash, &name, size, &req.category);
            state.notify("added", "Torrent added", &name, &outcome.info_hash);
            state.store.update(|d| {
                // qBittorrent creates unknown categories on the fly; so do we.
                if !req.category.is_empty() {
                    d.categories.entry(req.category.clone()).or_default();
                }
                d.tags.extend(req.tags.iter().cloned());
                d.torrents.insert(
                    outcome.info_hash.clone(),
                    TorrentMeta {
                        category: req.category.clone(),
                        tags: req.tags.iter().cloned().collect(),
                        added_on: now_unix(),
                        save_path: save_path.to_string_lossy().into_owned(),
                        ratio_limit: req.ratio_limit.unwrap_or(-2.0),
                        seeding_time_limit: req.seeding_time_limit.unwrap_or(-2),
                        inactive_seeding_time_limit: req.inactive_seeding_time_limit.unwrap_or(-2),
                        ..Default::default()
                    },
                );
            })?;
        }
        added.push(Added {
            hash: outcome.info_hash,
            already_existed: outcome.already_existed,
        });
    }
    Ok(added)
}

async fn add_torrents(State(state): State<Shared>, mp: Multipart) -> ApiResult<Json<Vec<String>>> {
    let req = AddRequest::from_multipart(mp).await?;
    Ok(Json(add_all(&state, req).await?.into_iter().map(|a| a.hash).collect()))
}

#[derive(Deserialize)]
struct Hashes {
    hashes: Vec<String>,
    #[serde(default)]
    delete_files: bool,
}

/// A pause or resume by hand overrides the queue for those torrents.
fn clear_queued(state: &Shared, hashes: &[String]) -> anyhow::Result<()> {
    state.store.update(|d| {
        for h in hashes {
            if let Some(m) = d.torrents.get_mut(h) {
                m.queued = false;
            }
        }
    })
}

async fn pause(State(state): State<Shared>, Json(req): Json<Hashes>) -> ApiResult<StatusCode> {
    for h in &req.hashes {
        state.engine.pause(h).await?;
    }
    clear_queued(&state, &req.hashes)?;
    Ok(StatusCode::NO_CONTENT)
}

async fn resume(State(state): State<Shared>, Json(req): Json<Hashes>) -> ApiResult<StatusCode> {
    for h in &req.hashes {
        state.engine.resume(h).await?;
    }
    clear_queued(&state, &req.hashes)?;
    Ok(StatusCode::NO_CONTENT)
}

pub async fn delete_torrents(state: &Shared, hashes: &[String], delete_files: bool) -> ApiResult<()> {
    for h in hashes {
        let status = state.engine.status(h);
        state.engine.remove(h, delete_files).await?;
        if let Some(s) = status {
            let category = state
                .store
                .read(|d| d.torrents.get(h).map(|m| m.category.clone()))
                .unwrap_or_default();
            let name = s.name.as_deref().unwrap_or(h);
            state.stats.event("removed", h, name, s.total_bytes, &category);
        }
    }
    state.store.update(|d| {
        for h in hashes {
            d.torrents.remove(h);
        }
    })?;
    Ok(())
}

async fn delete(State(state): State<Shared>, Json(req): Json<Hashes>) -> ApiResult<StatusCode> {
    delete_torrents(&state, &req.hashes, req.delete_files).await?;
    Ok(StatusCode::NO_CONTENT)
}

async fn files(State(state): State<Shared>, Path(hash): Path<String>) -> ApiResult<Json<Vec<FileEntry>>> {
    Ok(Json(state.engine.files(&hash)?))
}

#[derive(Deserialize)]
struct MetaReq {
    category: Option<String>,
    tags: Option<Vec<String>>,
}

async fn set_meta(
    State(state): State<Shared>,
    Path(hash): Path<String>,
    Json(req): Json<MetaReq>,
) -> ApiResult<StatusCode> {
    let found = state.store.update(|d| {
        if let Some(c) = &req.category
            && !c.is_empty()
        {
            d.categories.entry(c.clone()).or_default();
        }
        if let Some(t) = &req.tags {
            d.tags.extend(t.iter().cloned());
        }
        let Some(m) = d.torrents.get_mut(&hash) else {
            return false;
        };
        if let Some(c) = req.category {
            m.category = c;
        }
        if let Some(t) = req.tags {
            m.tags = t.into_iter().collect();
        }
        true
    })?;
    if found {
        Ok(StatusCode::NO_CONTENT)
    } else {
        Err(ApiError(StatusCode::NOT_FOUND, "torrent not found".into()))
    }
}

// ---------- categories ----------

async fn list_categories(State(state): State<Shared>) -> Json<BTreeMap<String, Category>> {
    Json(state.store.read(|d| d.categories.clone()))
}

async fn put_category(
    State(state): State<Shared>,
    Path(name): Path<String>,
    Json(cat): Json<Category>,
) -> ApiResult<StatusCode> {
    if name.trim().is_empty() {
        return Err(ApiError::bad_request("category name is empty"));
    }
    state.store.update(|d| d.categories.insert(name, cat))?;
    Ok(StatusCode::NO_CONTENT)
}

async fn delete_category(State(state): State<Shared>, Path(name): Path<String>) -> ApiResult<StatusCode> {
    state.store.update(|d| {
        d.categories.remove(&name);
        for m in d.torrents.values_mut() {
            if m.category == name {
                m.category.clear();
            }
        }
    })?;
    Ok(StatusCode::NO_CONTENT)
}

// ---------- stats ----------

async fn info(State(state): State<Shared>) -> Json<serde_json::Value> {
    Json(serde_json::json!({
        "name": "Swarmy",
        "version": env!("CARGO_PKG_VERSION"),
        "demo": state.demo,
        "download_dir": state.download_dir(),
        "network": state.engine.network(),
    }))
}

#[derive(Deserialize)]
struct PeerMapQuery {
    hash: Option<String>,
}

async fn peer_map(
    State(state): State<Shared>,
    Query(q): Query<PeerMapQuery>,
) -> Json<crate::peermap::PeerMap> {
    let hash = q.hash.filter(|h| !h.is_empty());
    Json(crate::peermap::build(&state, hash.as_deref()))
}

async fn geo_status(State(state): State<Shared>) -> Json<crate::geo::GeoStatus> {
    Json(state.geo.status())
}

/// "Check for an update now": runs in the background; poll /api/geo.
async fn geo_update(State(state): State<Shared>) -> ApiResult<StatusCode> {
    if !state.store.read(|d| d.settings.peer_map.locations) {
        return Err(ApiError::bad_request("Peer locations are switched off in Settings."));
    }
    let geo = state.geo.clone();
    tokio::spawn(async move { geo.refresh().await });
    Ok(StatusCode::ACCEPTED)
}

async fn changelog() -> Json<serde_json::Value> {
    Json(serde_json::json!({
        "version": env!("CARGO_PKG_VERSION"),
        "releases": crate::changelog::releases(),
    }))
}

#[derive(Deserialize)]
struct PiecesQuery {
    buckets: Option<usize>,
}

async fn pieces(
    State(state): State<Shared>,
    Path(hash): Path<String>,
    Query(q): Query<PiecesQuery>,
) -> ApiResult<Json<PieceMap>> {
    Ok(Json(state.engine.pieces(&hash, q.buckets.unwrap_or(256).clamp(16, 2048))?))
}

// ---------- push notifications ----------

async fn push_info(State(state): State<Shared>) -> Json<serde_json::Value> {
    let (prefs, devices) = state
        .store
        .read(|d| (d.push.prefs.clone(), d.push.subscriptions.len()));
    Json(serde_json::json!({
        "public_key": state.push.public_key(),
        "prefs": prefs,
        "devices": devices,
    }))
}

#[derive(Deserialize)]
struct SubscribeKeys {
    p256dh: String,
    auth: String,
}

#[derive(Deserialize)]
struct SubscribeReq {
    endpoint: String,
    keys: SubscribeKeys,
    #[serde(default)]
    label: String,
}

async fn push_subscribe(
    State(state): State<Shared>,
    Json(req): Json<SubscribeReq>,
) -> ApiResult<StatusCode> {
    if !req.endpoint.starts_with("https://") {
        return Err(ApiError::bad_request("push endpoint must be https"));
    }
    state.store.update(|d| {
        d.push.subscriptions.retain(|s| s.endpoint != req.endpoint);
        d.push.subscriptions.push(crate::store::PushSubscription {
            endpoint: req.endpoint,
            p256dh: req.keys.p256dh,
            auth: req.keys.auth,
            label: req.label,
            created: now_unix(),
        });
    })?;
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Deserialize)]
struct EndpointReq {
    endpoint: Option<String>,
}

async fn push_unsubscribe(
    State(state): State<Shared>,
    Json(req): Json<EndpointReq>,
) -> ApiResult<StatusCode> {
    if let Some(e) = req.endpoint {
        state
            .store
            .update(|d| d.push.subscriptions.retain(|s| s.endpoint != e))?;
    }
    Ok(StatusCode::NO_CONTENT)
}

async fn push_prefs(
    State(state): State<Shared>,
    Json(prefs): Json<crate::store::NotifyPrefs>,
) -> ApiResult<StatusCode> {
    state.store.update(|d| d.push.prefs = prefs)?;
    Ok(StatusCode::NO_CONTENT)
}

async fn push_test(
    State(state): State<Shared>,
    Json(req): Json<EndpointReq>,
) -> ApiResult<Json<serde_json::Value>> {
    let notice = crate::push::Notice {
        title: "Swarmy notifications are on".into(),
        body: "You'll hear from Swarmy when downloads finish or need attention.".into(),
        tag: "test".into(),
        url: "/".into(),
    };
    let sent = state
        .push
        .send(&state.store, &notice, req.endpoint.as_deref())
        .await
        .map_err(|e| ApiError(StatusCode::BAD_GATEWAY, format!("{e:#}")))?;
    Ok(Json(serde_json::json!({ "delivered": sent })))
}

async fn stats_live(State(state): State<Shared>) -> Json<Vec<crate::stats::LiveSample>> {
    Json(state.stats.live())
}

#[derive(Deserialize)]
struct HistoryQuery {
    range: Option<String>,
}

async fn stats_history(
    State(state): State<Shared>,
    Query(q): Query<HistoryQuery>,
) -> ApiResult<Json<crate::stats::History>> {
    let range = crate::stats::Range::parse(q.range.as_deref().unwrap_or("24h"))
        .ok_or_else(|| ApiError::bad_request("range must be one of 1h, 24h, 7d, 30d, 1y"))?;
    let stats = state.stats.clone();
    let h = tokio::task::spawn_blocking(move || stats.history(range)).await??;
    Ok(Json(h))
}

#[derive(Deserialize)]
struct SummaryQuery {
    /// Minutes east of UTC, so "today" and daily buckets follow the viewer's clock.
    tz: Option<i64>,
}

async fn stats_summary(
    State(state): State<Shared>,
    Query(q): Query<SummaryQuery>,
) -> ApiResult<Json<crate::stats::Summary>> {
    let offset = q.tz.unwrap_or(0).clamp(-14 * 60, 14 * 60) * 60;
    let stats = state.stats.clone();
    let s = tokio::task::spawn_blocking(move || stats.summary(offset)).await??;
    Ok(Json(s))
}

#[derive(Deserialize)]
struct EventsQuery {
    limit: Option<u32>,
}

async fn stats_events(
    State(state): State<Shared>,
    Query(q): Query<EventsQuery>,
) -> ApiResult<Json<Vec<crate::stats::Event>>> {
    Ok(Json(state.stats.events(q.limit.unwrap_or(30).min(500))?))
}
