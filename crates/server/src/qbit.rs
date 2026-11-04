//! qBittorrent Web API v2 compatibility, mounted at `/api/v2`.
//!
//! Written against what Sonarr/Radarr's qBittorrent client actually sends and
//! reads (src/NzbDrone.Core/Download/Clients/QBittorrent), so they can use
//! Swarmy as "qBittorrent". The rules that matter:
//!
//! - Not signed in → `403 Forbidden` (Sonarr re-logs in on 403).
//! - `auth/login` answers `200 "Ok."` with a `SID` cookie, or `200 "Fails."`.
//! - `Authorization: Bearer <api key>` works instead of a login.
//! - State strings are qBittorrent 5's (`stoppedUP`, `stalledDL`, `metaDL`…).
//! - `content_path` / `root_path` / `save_path` must be exact: Sonarr imports from them.
//! - We report Web API 2.11.4 (qBittorrent 5.1), so clients use `stopped`,
//!   `inactiveSeedingTimeLimit`, share limits on add and `root_path`.
//! - A torrent waiting for or in the middle of a move reports `moving`, which
//!   Sonarr treats as still downloading, so it never imports half-moved files.

use std::{collections::HashMap, net::SocketAddr};

use axum::{
    Json, Router,
    body::Bytes,
    extract::{ConnectInfo, FromRequest, Multipart, Query, Request, State},
    http::{HeaderMap, StatusCode, header},
    middleware::{self, Next},
    response::{IntoResponse, Response},
    routing::{get, post},
};
use axum_extra::extract::cookie::CookieJar;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use swarmy_engine::TorrentState;

use crate::{
    api::{self, AddRequest},
    auth::{self, COOKIE_NAME},
    state::{Shared, TorrentView},
    store::{Category, SeedAction, now_unix},
};

pub const APP_VERSION: &str = "v5.1.2";
pub const WEBAPI_VERSION: &str = "2.11.4";
/// qBittorrent's "infinite" ETA.
const ETA_INF: u64 = 8_640_000;

pub fn router(state: Shared) -> Router<Shared> {
    let protected = Router::new()
        .route("/app/version", get(|| async { APP_VERSION }))
        .route("/app/webapiVersion", get(|| async { WEBAPI_VERSION }))
        .route("/app/buildInfo", get(build_info))
        .route("/app/preferences", get(preferences))
        .route("/app/setPreferences", post(set_preferences))
        .route("/app/defaultSavePath", get(default_save_path))
        .route("/torrents/info", get(torrents_info))
        .route("/torrents/properties", get(properties))
        .route("/torrents/files", get(files))
        .route("/torrents/trackers", get(|| async { Json(json!([])) }))
        .route("/torrents/add", post(add))
        .route("/torrents/delete", post(delete))
        .route("/torrents/stop", post(stop))
        .route("/torrents/pause", post(stop))
        .route("/torrents/start", post(start))
        .route("/torrents/resume", post(start))
        .route("/torrents/recheck", post(ok_for_known))
        .route("/torrents/reannounce", post(ok_for_known))
        .route("/torrents/setCategory", post(set_category))
        .route("/torrents/createCategory", post(create_category))
        .route("/torrents/editCategory", post(edit_category))
        .route("/torrents/removeCategories", post(remove_categories))
        .route("/torrents/categories", get(categories))
        .route("/torrents/addTags", post(add_tags))
        .route("/torrents/removeTags", post(remove_tags))
        .route("/torrents/createTags", post(create_tags))
        .route("/torrents/deleteTags", post(delete_tags))
        .route("/torrents/tags", get(tags))
        .route("/torrents/setShareLimits", post(set_share_limits))
        .route("/torrents/topPrio", post(top_prio))
        .route("/torrents/bottomPrio", post(bottom_prio))
        .route("/torrents/increasePrio", post(top_prio))
        .route("/torrents/decreasePrio", post(bottom_prio))
        .route("/torrents/setForceStart", post(set_force_start))
        .route("/torrents/setLocation", post(set_location))
        .route("/torrents/setAutoManagement", post(set_auto_management))
        .route("/transfer/info", get(transfer_info))
        .route("/transfer/speedLimitsMode", get(speed_limits_mode))
        .route("/transfer/toggleSpeedLimitsMode", post(toggle_speed_limits_mode))
        .route_layer(middleware::from_fn_with_state(state.clone(), require_auth));

    Router::new()
        .route("/auth/login", post(login))
        .route("/auth/logout", post(logout))
        .merge(protected)
        .route_layer(middleware::from_fn_with_state(state, require_enabled))
}

// ---------------------------------------------------------------- plumbing

fn text(status: StatusCode, body: &'static str) -> Response {
    (status, [(header::CONTENT_TYPE, "text/plain; charset=UTF-8")], body).into_response()
}

async fn require_enabled(State(state): State<Shared>, req: Request, next: Next) -> Response {
    if state.store.read(|d| d.settings.webui.qbit_api) {
        next.run(req).await
    } else {
        text(StatusCode::NOT_FOUND, "Not Found")
    }
}

pub fn api_key_hash(key: &str) -> String {
    Sha256::digest(key.as_bytes()).iter().map(|b| format!("{b:02x}")).collect()
}

fn constant_eq(a: &str, b: &str) -> bool {
    a.len() == b.len() && a.bytes().zip(b.bytes()).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

/// Bearer API key or the shared `SID` session; anything else is a 403, which
/// is what makes Sonarr log in again.
async fn require_auth(State(state): State<Shared>, jar: CookieJar, req: Request, next: Next) -> Response {
    let bearer = req
        .headers()
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .map(str::trim);
    if let Some(key) = bearer {
        let stored = state.store.read(|d| d.settings.webui.api_key_hash.clone());
        if !stored.is_empty() && constant_eq(&api_key_hash(key), &stored) {
            return next.run(req).await;
        }
    }
    match jar.get(COOKIE_NAME) {
        Some(c) if auth::session_valid(&state.store, c.value()) => next.run(req).await,
        _ => text(StatusCode::FORBIDDEN, "Forbidden"),
    }
}

/// Form fields from either `application/x-www-form-urlencoded` or multipart
/// (qBittorrent accepts both; Sonarr uses urlencoded except for file uploads).
struct QForm {
    fields: Vec<(String, String)>,
    files: Vec<Vec<u8>>,
}

impl QForm {
    fn get(&self, k: &str) -> Option<&str> {
        self.fields.iter().find(|(n, _)| n == k).map(|(_, v)| v.as_str())
    }
    fn flag(&self, k: &str) -> bool {
        self.get(k).is_some_and(|v| v.eq_ignore_ascii_case("true"))
    }
}

impl FromRequest<Shared> for QForm {
    type Rejection = Response;
    async fn from_request(req: Request, state: &Shared) -> Result<Self, Response> {
        let is_multipart = req
            .headers()
            .get(header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .is_some_and(|ct| ct.starts_with("multipart/form-data"));
        if is_multipart {
            let mut mp = Multipart::from_request(req, state).await.map_err(IntoResponse::into_response)?;
            let mut form = QForm { fields: vec![], files: vec![] };
            while let Some(field) = mp
                .next_field()
                .await
                .map_err(|e| (StatusCode::BAD_REQUEST, e.to_string()).into_response())?
            {
                let name = field.name().unwrap_or_default().to_owned();
                if field.file_name().is_some() || name == "torrents" {
                    let bytes = field
                        .bytes()
                        .await
                        .map_err(|e| (StatusCode::BAD_REQUEST, e.to_string()).into_response())?;
                    form.files.push(bytes.to_vec());
                } else {
                    let v = field
                        .text()
                        .await
                        .map_err(|e| (StatusCode::BAD_REQUEST, e.to_string()).into_response())?;
                    form.fields.push((name, v));
                }
            }
            Ok(form)
        } else {
            let body = Bytes::from_request(req, state).await.map_err(IntoResponse::into_response)?;
            Ok(QForm {
                fields: form_urlencoded::parse(&body).into_owned().collect(),
                files: vec![],
            })
        }
    }
}

/// `hashes=a|b|c` or `hashes=all`, case-insensitive.
fn hashes(state: &Shared, raw: Option<&str>) -> Vec<String> {
    let raw = raw.unwrap_or_default();
    if raw.trim().eq_ignore_ascii_case("all") {
        return state.snapshot.borrow().torrents.iter().map(|t| t.hash.clone()).collect();
    }
    raw.split('|')
        .map(|h| h.trim().to_ascii_lowercase())
        .filter(|h| !h.is_empty())
        .collect()
}

/// A torrent by hash: from the latest snapshot, or straight from the engine
/// for one added since (Sonarr looks a torrent up within a second of adding it).
fn snapshot_torrent(state: &Shared, hash: &str) -> Option<TorrentView> {
    let hash = hash.to_ascii_lowercase();
    if let Some(t) = state.snapshot.borrow().torrents.iter().find(|t| t.hash == hash).cloned() {
        return Some(t);
    }
    let status = state.engine.status(&hash)?;
    let meta = state.store.read(|d| d.torrents.get(&hash).cloned()).unwrap_or_default();
    let moving = state.mover.busy().contains(&hash);
    Some(TorrentView::new(status, &meta, now_unix(), moving))
}

// ---------------------------------------------------------------- auth

async fn login(
    State(state): State<Shared>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    jar: CookieJar,
    form: QForm,
) -> Response {
    let web = state.store.read(|d| d.settings.webui.clone());
    let client = auth::client(peer, &headers, &web.trusted_proxies);
    if state.auth.locked_for(client.ip).is_some() {
        return text(
            StatusCode::FORBIDDEN,
            "Your IP address has been banned after too many failed authentication attempts.",
        );
    }
    let user = form.get("username").unwrap_or_default();
    let pass = form.get("password").unwrap_or_default();
    if auth::check_credentials(&state, user, pass).await {
        state.auth.clear_failures(client.ip);
        match auth::create_session(&state.store, web.session_days, &format!("{} (API)", client.ip)) {
            Ok(token) => (
                jar.add(auth::session_cookie(token, web.session_days, client.https)),
                [(header::CONTENT_TYPE, "text/plain; charset=UTF-8")],
                "Ok.",
            )
                .into_response(),
            Err(_) => text(StatusCode::INTERNAL_SERVER_ERROR, "Fails."),
        }
    } else {
        state.auth.record_failure(
            client.ip,
            web.max_attempts,
            std::time::Duration::from_secs(u64::from(web.lockout_minutes.max(1)) * 60),
        );
        text(StatusCode::OK, "Fails.")
    }
}

async fn logout(State(state): State<Shared>, jar: CookieJar) -> Response {
    if let Some(c) = jar.get(COOKIE_NAME) {
        let _ = auth::revoke(&state.store, c.value());
    }
    (jar.remove(COOKIE_NAME), StatusCode::OK).into_response()
}

// ---------------------------------------------------------------- app

async fn build_info() -> Json<Value> {
    Json(json!({
        "qt": "none",
        "libtorrent": "librqbit-9",
        "boost": "none",
        "openssl": "rustls",
        "zlib": "none",
        "bitness": 64,
        "platform": std::env::consts::OS,
    }))
}

async fn default_save_path(State(state): State<Shared>) -> String {
    state.download_dir().to_string_lossy().into_owned()
}

async fn preferences(State(state): State<Shared>) -> Json<Value> {
    let (s, username) = state
        .store
        .read(|d| (d.settings.clone(), d.account.as_ref().map(|a| a.username.clone()).unwrap_or_default()));
    let kib = |v: u32| i64::from(v) * 1024;
    let act = match s.seeding.action {
        SeedAction::Pause => 0,
        SeedAction::Remove => 1,
        SeedAction::RemoveWithFiles => 3,
    };
    let net = state.engine.network();
    let incomplete = state
        .incomplete_dir("")
        .unwrap_or_else(|| state.download_dir().join("incomplete"));
    Json(json!({
        "locale": "en",
        "save_path": state.download_dir(),
        "temp_path_enabled": s.downloads.use_incomplete_dir,
        "temp_path": incomplete,
        // Torrents added without a folder follow their category's.
        "auto_tmm_enabled": true,
        "torrent_changed_tmm_enabled": s.downloads.category_moves,
        "category_changed_tmm_enabled": s.downloads.category_moves,
        "save_path_changed_tmm_enabled": s.downloads.category_moves,
        "torrent_content_layout": "Original",
        "add_to_top_of_queue": false,
        "add_stopped_enabled": s.downloads.add_paused,
        "start_paused_enabled": s.downloads.add_paused,
        "create_subfolder_enabled": true,
        "preallocate_all": false,
        "incomplete_files_ext": false,
        "max_ratio_enabled": s.seeding.ratio_limit.is_some(),
        "max_ratio": s.seeding.ratio_limit.unwrap_or(-1.0),
        "max_seeding_time_enabled": s.seeding.time_limit.is_some(),
        "max_seeding_time": s.seeding.time_limit.map(i64::from).unwrap_or(-1),
        "max_inactive_seeding_time_enabled": s.seeding.inactive_time_limit.is_some(),
        "max_inactive_seeding_time": s.seeding.inactive_time_limit.map(i64::from).unwrap_or(-1),
        "max_ratio_act": act,
        "queueing_enabled": s.queue.enabled,
        "max_active_downloads": s.queue.max_downloads,
        "max_active_uploads": s.queue.max_uploads,
        "max_active_torrents": s.queue.max_active,
        "dont_count_slow_torrents": false,
        "dht": s.connection.dht,
        "pex": true,
        "lsd": s.connection.lsd,
        "encryption": 0,
        "anonymous_mode": false,
        "listen_port": net.listen_port.unwrap_or(s.connection.listen_port),
        "upnp": s.connection.upnp,
        "current_network_interface": s.connection.bind_interface,
        "max_connec_per_torrent": s.connection.peer_limit,
        "dl_limit": kib(s.speed.download_limit),
        "up_limit": kib(s.speed.upload_limit),
        "alt_dl_limit": kib(s.speed.alt_download_limit),
        "alt_up_limit": kib(s.speed.alt_upload_limit),
        "scheduler_enabled": s.speed.schedule.enabled,
        "web_ui_username": username,
        "web_ui_session_timeout": i64::from(s.webui.session_days) * 86400,
        "web_ui_max_auth_fail_count": s.webui.max_attempts,
        "web_ui_ban_duration": i64::from(s.webui.lockout_minutes) * 60,
        "web_ui_reverse_proxy_enabled": !s.webui.trusted_proxies.is_empty(),
        "web_ui_reverse_proxies_list": s.webui.trusted_proxies.join(";"),
        "bypass_local_auth": false,
    }))
}

/// `json={...}` with any subset of the keys from `preferences`. We apply the
/// ones that map onto Swarmy settings and ignore the rest, like qBittorrent
/// ignores unknown keys.
async fn set_preferences(State(state): State<Shared>, form: QForm) -> Response {
    let Some(Ok(p)) = form.get("json").map(serde_json::from_str::<Value>) else {
        return text(StatusCode::BAD_REQUEST, "Bad Request");
    };
    let b = |k: &str| p.get(k).and_then(Value::as_bool);
    let n = |k: &str| p.get(k).and_then(Value::as_f64);
    let res = state.store.update(|d| {
        let s = &mut d.settings;
        let to_kib = |v: f64| if v <= 0.0 { 0 } else { (v / 1024.0).round() as u32 };
        if let Some(v) = n("dl_limit") { s.speed.download_limit = to_kib(v) }
        if let Some(v) = n("up_limit") { s.speed.upload_limit = to_kib(v) }
        if let Some(v) = n("alt_dl_limit") { s.speed.alt_download_limit = to_kib(v) }
        if let Some(v) = n("alt_up_limit") { s.speed.alt_upload_limit = to_kib(v) }
        if let Some(v) = b("queueing_enabled") { s.queue.enabled = v }
        if let Some(v) = n("max_active_downloads") { s.queue.max_downloads = v.max(0.0) as u32 }
        if let Some(v) = n("max_active_uploads") { s.queue.max_uploads = v.max(0.0) as u32 }
        if let Some(v) = n("max_active_torrents") { s.queue.max_active = v.max(1.0) as u32 }
        if let Some(v) = b("add_stopped_enabled").or(b("start_paused_enabled")) { s.downloads.add_paused = v }
        if let Some(v) = b("temp_path_enabled") { s.downloads.use_incomplete_dir = v }
        if let Some(v) = p.get("temp_path").and_then(Value::as_str) { s.downloads.incomplete_dir = v.trim().to_owned() }
        if let Some(v) = b("category_changed_tmm_enabled").or(b("torrent_changed_tmm_enabled")) { s.downloads.category_moves = v }
        let ratio_on = b("max_ratio_enabled");
        if let Some(v) = n("max_ratio") { if v >= 0.0 { s.seeding.ratio_limit = Some(v) } }
        if ratio_on == Some(false) { s.seeding.ratio_limit = None }
        let time_on = b("max_seeding_time_enabled");
        if let Some(v) = n("max_seeding_time") { if v >= 0.0 { s.seeding.time_limit = Some(v as u32) } }
        if time_on == Some(false) { s.seeding.time_limit = None }
        let idle_on = b("max_inactive_seeding_time_enabled");
        if let Some(v) = n("max_inactive_seeding_time") { if v >= 0.0 { s.seeding.inactive_time_limit = Some(v as u32) } }
        if idle_on == Some(false) { s.seeding.inactive_time_limit = None }
        if let Some(v) = n("max_ratio_act") {
            s.seeding.action = match v as i64 { 1 => SeedAction::Remove, 3 => SeedAction::RemoveWithFiles, _ => SeedAction::Pause };
        }
    });
    match res {
        Ok(()) => StatusCode::OK.into_response(),
        Err(_) => text(StatusCode::INTERNAL_SERVER_ERROR, "Internal Server Error"),
    }
}

// ---------------------------------------------------------------- torrents

/// qBittorrent 5 state strings, as Sonarr/Radarr switch on them.
fn qbit_state(t: &TorrentView) -> &'static str {
    let done = t.progress >= 1.0;
    match t.state {
        _ if t.moving.is_some() => "moving",
        TorrentState::Moving => "moving",
        TorrentState::Error => "error",
        TorrentState::Initializing if !t.has_metadata => {
            if t.force_start { "forcedMetaDL" } else { "metaDL" }
        }
        TorrentState::Initializing => if done { "checkingUP" } else { "checkingDL" },
        TorrentState::Paused if t.queued => if done { "queuedUP" } else { "queuedDL" },
        TorrentState::Paused => if done { "stoppedUP" } else { "stoppedDL" },
        TorrentState::Downloading if t.force_start => "forcedDL",
        TorrentState::Downloading if t.download_speed > 0 => "downloading",
        TorrentState::Downloading => "stalledDL",
        TorrentState::Seeding if t.force_start => "forcedUP",
        TorrentState::Seeding if t.upload_speed > 0 => "uploading",
        TorrentState::Seeding => "stalledUP",
    }
}

fn seeding_time(t: &TorrentView, now: i64) -> i64 {
    match (t.state, t.completed_on) {
        (TorrentState::Seeding | TorrentState::Paused, Some(done)) => (now - done).max(0),
        _ => 0,
    }
}

fn torrent_json(t: &TorrentView, now: i64, global_ratio: Option<f64>, global_time: Option<u32>) -> Value {
    let left = t.size.saturating_sub(t.downloaded);
    let eta = match (t.state, t.eta) {
        (TorrentState::Downloading, Some(e)) => e,
        _ => ETA_INF,
    };
    let (name, hash) = (&t.name, &t.hash);
    let magnet = format!("magnet:?xt=urn:btih:{hash}&dn={}", form_urlencoded::byte_serialize(name.as_bytes()).collect::<String>());
    json!({
        "hash": hash,
        "infohash_v1": hash,
        "infohash_v2": "",
        "name": name,
        "magnet_uri": magnet,
        "size": t.size,
        "total_size": t.size,
        "progress": t.progress,
        "dlspeed": t.download_speed,
        "upspeed": t.upload_speed,
        "priority": if t.priority > 0 { 1 } else { 0 },
        "num_seeds": t.seeds,
        "num_complete": t.seeds,
        "num_leechs": t.peers.saturating_sub(t.seeds),
        "num_incomplete": t.peers.saturating_sub(t.seeds),
        "ratio": t.ratio,
        "popularity": 0,
        "eta": eta,
        "state": qbit_state(t),
        "seq_dl": false,
        "f_l_piece_prio": false,
        "category": t.category,
        "tags": t.tags.join(", "),
        "super_seeding": false,
        "force_start": t.force_start,
        "save_path": t.save_path,
        "download_path": t.download_path,
        "content_path": t.content_path,
        "root_path": t.root_path,
        "added_on": t.added_on,
        "completion_on": t.completed_on.unwrap_or(-1),
        "tracker": "",
        "trackers_count": 0,
        "dl_limit": -1,
        "up_limit": -1,
        "downloaded": t.downloaded,
        "uploaded": t.uploaded,
        "downloaded_session": t.downloaded,
        "uploaded_session": t.uploaded,
        "amount_left": left,
        "completed": t.downloaded,
        "max_ratio": global_ratio.unwrap_or(-1.0),
        "max_seeding_time": global_time.map(i64::from).unwrap_or(-1),
        "ratio_limit": t.ratio_limit,
        "seeding_time_limit": t.seeding_time_limit,
        "inactive_seeding_time_limit": t.inactive_seeding_time_limit,
        "seen_complete": t.completed_on.unwrap_or(-1),
        "last_activity": t.last_activity,
        "time_active": (now - t.added_on).max(0),
        "seeding_time": seeding_time(t, now),
        "auto_tmm": t.auto_managed,
        "availability": if t.has_metadata { 1.0 } else { -1.0 },
        "reannounce": 0,
        "private": false,
        "has_metadata": t.has_metadata,
    })
}

fn matches_filter(t: &TorrentView, filter: &str) -> bool {
    let s = qbit_state(t);
    match filter {
        "" | "all" => true,
        "downloading" => matches!(s, "downloading" | "stalledDL" | "metaDL" | "forcedMetaDL" | "forcedDL" | "queuedDL" | "checkingDL" | "stoppedDL"),
        "seeding" => matches!(s, "uploading" | "stalledUP" | "forcedUP" | "queuedUP" | "checkingUP"),
        "completed" => t.progress >= 1.0,
        "paused" | "stopped" => s.starts_with("stopped"),
        "resumed" | "running" => !s.starts_with("stopped"),
        "active" => t.download_speed > 0 || t.upload_speed > 0,
        "inactive" => t.download_speed == 0 && t.upload_speed == 0,
        "stalled" => s.starts_with("stalled"),
        "stalled_uploading" => s == "stalledUP",
        "stalled_downloading" => s == "stalledDL",
        "checking" => s.starts_with("checking"),
        "errored" => s == "error" || s == "missingFiles",
        _ => true,
    }
}

async fn torrents_info(State(state): State<Shared>, Query(q): Query<HashMap<String, String>>) -> Json<Value> {
    let now = now_unix();
    let (ratio, time) = state.store.read(|d| (d.settings.seeding.ratio_limit, d.settings.seeding.time_limit));
    let wanted: Option<Vec<String>> = q.get("hashes").map(|h| hashes(&state, Some(h)));
    let mut list: Vec<TorrentView> = state
        .snapshot
        .borrow()
        .torrents
        .iter()
        .filter(|t| matches_filter(t, q.get("filter").map(String::as_str).unwrap_or("all")))
        // `category=` (empty) means uncategorised, as in qBittorrent.
        .filter(|t| q.get("category").is_none_or(|c| &t.category == c))
        .filter(|t| q.get("tag").is_none_or(|tag| if tag.is_empty() { t.tags.is_empty() } else { t.tags.contains(tag) }))
        .filter(|t| wanted.as_ref().is_none_or(|w| w.contains(&t.hash)))
        .cloned()
        .collect();

    let mut items: Vec<Value> = list.drain(..).map(|t| torrent_json(&t, now, ratio, time)).collect();
    if let Some(key) = q.get("sort") {
        items.sort_by(|a, b| {
            let (x, y) = (&a[key], &b[key]);
            match (x.as_f64(), y.as_f64()) {
                (Some(x), Some(y)) => x.partial_cmp(&y).unwrap_or(std::cmp::Ordering::Equal),
                _ => x.to_string().cmp(&y.to_string()),
            }
        });
        if q.get("reverse").is_some_and(|r| r == "true") {
            items.reverse();
        }
    }
    let offset = q.get("offset").and_then(|o| o.parse::<i64>().ok()).unwrap_or(0);
    let offset = if offset < 0 { (items.len() as i64 + offset).max(0) as usize } else { offset as usize };
    let limit = q.get("limit").and_then(|l| l.parse::<usize>().ok()).filter(|l| *l > 0).unwrap_or(usize::MAX);
    Json(Value::Array(items.into_iter().skip(offset).take(limit).collect()))
}

async fn properties(State(state): State<Shared>, Query(q): Query<HashMap<String, String>>) -> Response {
    let Some(t) = q.get("hash").and_then(|h| snapshot_torrent(&state, h)) else {
        return text(StatusCode::NOT_FOUND, "Not Found");
    };
    let now = now_unix();
    let pieces = state.engine.pieces(&t.hash, 1).ok();
    let elapsed = (now - t.added_on).max(1);
    Json(json!({
        "hash": t.hash,
        "infohash_v1": t.hash,
        "infohash_v2": "",
        "name": t.name,
        "save_path": t.save_path,
        "download_path": t.download_path,
        "creation_date": -1,
        "piece_size": if let Some(p) = &pieces { if p.total > 0 { t.size / u64::from(p.total) } else { 0 } } else { 0 },
        "comment": "",
        "total_wasted": 0,
        "total_uploaded": t.uploaded,
        "total_uploaded_session": t.uploaded,
        "total_downloaded": t.downloaded,
        "total_downloaded_session": t.downloaded,
        "up_limit": -1,
        "dl_limit": -1,
        "time_elapsed": elapsed,
        "seeding_time": seeding_time(&t, now),
        "nb_connections": t.peers,
        "nb_connections_limit": -1,
        "share_ratio": t.ratio,
        "addition_date": t.added_on,
        "completion_date": t.completed_on.unwrap_or(-1),
        "created_by": "",
        "dl_speed_avg": t.downloaded / elapsed as u64,
        "dl_speed": t.download_speed,
        "eta": t.eta.unwrap_or(ETA_INF),
        "last_seen": t.last_activity,
        "peers": t.peers.saturating_sub(t.seeds),
        "peers_total": t.peers.saturating_sub(t.seeds),
        "pieces_have": pieces.as_ref().map(|p| p.have).unwrap_or(0),
        "pieces_num": pieces.as_ref().map(|p| p.total).unwrap_or(0),
        "reannounce": 0,
        "seeds": t.seeds,
        "seeds_total": t.seeds,
        "total_size": t.size,
        "up_speed_avg": t.uploaded / elapsed as u64,
        "up_speed": t.upload_speed,
        "isPrivate": false,
        "is_private": false,
        "has_metadata": t.has_metadata,
    }))
    .into_response()
}

async fn files(State(state): State<Shared>, Query(q): Query<HashMap<String, String>>) -> Response {
    let Some(t) = q.get("hash").and_then(|h| snapshot_torrent(&state, h)) else {
        return text(StatusCode::NOT_FOUND, "Not Found");
    };
    let Ok(list) = state.engine.files(&t.hash) else {
        return Json(json!([])).into_response();
    };
    // qBittorrent names files relative to the save path, including the
    // torrent's own folder ("Show.S01/ep1.mkv"), with `/` separators.
    let prefix = std::path::Path::new(&t.root_path)
        .strip_prefix(&t.save_path)
        .ok()
        .map(|p| p.components().map(|c| c.as_os_str().to_string_lossy()).collect::<Vec<_>>().join("/"))
        .filter(|p| !p.is_empty());
    Json(Value::Array(
        list.into_iter()
            .map(|f| {
                let name = match &prefix {
                    Some(p) => format!("{p}/{}", f.path),
                    None => f.path.clone(),
                };
                let progress = if f.size == 0 { 1.0 } else { f.downloaded_bytes as f64 / f.size as f64 };
                json!({
                    "index": f.index,
                    "name": name,
                    "size": f.size,
                    "progress": progress,
                    "priority": if f.selected { 1 } else { 0 },
                    "is_seed": progress >= 1.0,
                    "piece_range": [0, 0],
                    "availability": -1,
                })
            })
            .collect(),
    ))
    .into_response()
}

async fn add(State(state): State<Shared>, form: QForm) -> Response {
    let mut req = AddRequest::default();
    for (k, v) in &form.fields {
        req.field(k, v);
    }
    req.files = form.files;
    if req.urls.is_empty() && req.files.is_empty() {
        return text(StatusCode::BAD_REQUEST, "Fails.");
    }
    match api::add_all(&state, req).await {
        // qBittorrent 5 answers 409 when every torrent was already there;
        // Sonarr turns that into "release rejected" rather than a failure.
        Ok(added) if !added.is_empty() && added.iter().all(|a| a.already_existed) => {
            text(StatusCode::CONFLICT, "Torrent is already present")
        }
        Ok(_) => text(StatusCode::OK, "Ok."),
        Err(e) => {
            tracing::warn!("qBittorrent API add failed: {}", e.message());
            text(StatusCode::UNSUPPORTED_MEDIA_TYPE, "Fails.")
        }
    }
}

async fn delete(State(state): State<Shared>, form: QForm) -> Response {
    let list = hashes(&state, form.get("hashes"));
    let known: Vec<String> = list.into_iter().filter(|h| state.engine.status(h).is_some()).collect();
    match api::delete_torrents(&state, &known, form.flag("deleteFiles")).await {
        Ok(()) => StatusCode::OK.into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, e.message().to_owned()).into_response(),
    }
}

async fn set_queued_flag(state: &Shared, list: &[String], f: impl Fn(&mut crate::store::TorrentMeta)) {
    let _ = state.store.update(|d| {
        for h in list {
            if let Some(m) = d.torrents.get_mut(h) {
                f(m);
            }
        }
    });
}

async fn stop(State(state): State<Shared>, form: QForm) -> Response {
    let list = hashes(&state, form.get("hashes"));
    for h in &list {
        let _ = state.engine.pause(h).await;
    }
    set_queued_flag(&state, &list, |m| {
        m.queued = false;
        m.force_start = false;
    })
    .await;
    StatusCode::OK.into_response()
}

async fn start(State(state): State<Shared>, form: QForm) -> Response {
    let list = hashes(&state, form.get("hashes"));
    for h in &list {
        let _ = state.engine.resume(h).await;
    }
    set_queued_flag(&state, &list, |m| m.queued = false).await;
    StatusCode::OK.into_response()
}

async fn ok_for_known(State(state): State<Shared>, form: QForm) -> Response {
    let _ = hashes(&state, form.get("hashes"));
    StatusCode::OK.into_response()
}

async fn set_category(State(state): State<Shared>, form: QForm) -> Response {
    let cat = form.get("category").unwrap_or_default().trim().to_owned();
    if !cat.is_empty() && !state.store.read(|d| d.categories.contains_key(&cat)) {
        return text(StatusCode::CONFLICT, "Incorrect category name");
    }
    let list = hashes(&state, form.get("hashes"));
    set_queued_flag(&state, &list, |m| m.category = cat.clone()).await;
    crate::mover::category_changed(&state, &list);
    StatusCode::OK.into_response()
}

/// Move torrents' files to `location`; they stop following their category.
async fn set_location(State(state): State<Shared>, form: QForm) -> Response {
    let location = form.get("location").unwrap_or_default().trim().to_owned();
    if location.is_empty() {
        return text(StatusCode::BAD_REQUEST, "Save path cannot be empty");
    }
    let list = hashes(&state, form.get("hashes"));
    match crate::mover::move_by_hand(&state, &list, Some(std::path::Path::new(&location))) {
        Ok(()) => StatusCode::OK.into_response(),
        Err(e) => (StatusCode::CONFLICT, e.to_string()).into_response(),
    }
}

/// Turn following the category's folder on (which moves files there) or off.
async fn set_auto_management(State(state): State<Shared>, form: QForm) -> Response {
    let list = hashes(&state, form.get("hashes"));
    if form.flag("enable") {
        let _ = crate::mover::move_by_hand(&state, &list, None);
    } else {
        set_queued_flag(&state, &list, |m| m.auto_managed = false).await;
    }
    StatusCode::OK.into_response()
}

async fn create_category(State(state): State<Shared>, form: QForm) -> Response {
    let name = form.get("category").unwrap_or_default().trim().to_owned();
    if name.is_empty() || name.contains('\\') || name.starts_with('/') || name.ends_with('/') || name.contains("//") {
        return text(StatusCode::BAD_REQUEST, "Invalid category name");
    }
    let save_path = form.get("savePath").unwrap_or_default().trim().to_owned();
    let created = state.store.update(|d| {
        if d.categories.contains_key(&name) {
            false
        } else {
            d.categories.insert(name.clone(), Category { save_path });
            true
        }
    });
    match created {
        Ok(true) => StatusCode::OK.into_response(),
        Ok(false) => text(StatusCode::CONFLICT, "Category already exists"),
        Err(_) => text(StatusCode::INTERNAL_SERVER_ERROR, "Internal Server Error"),
    }
}

async fn edit_category(State(state): State<Shared>, form: QForm) -> Response {
    let name = form.get("category").unwrap_or_default().trim().to_owned();
    let save_path = form.get("savePath").unwrap_or_default().trim().to_owned();
    let found = state.store.update(|d| match d.categories.get_mut(&name) {
        Some(c) => {
            c.save_path = save_path;
            true
        }
        None => false,
    });
    match found {
        Ok(true) => {
            crate::mover::category_folder_changed(&state, &name);
            StatusCode::OK.into_response()
        }
        _ => text(StatusCode::CONFLICT, "Category does not exist"),
    }
}

async fn remove_categories(State(state): State<Shared>, form: QForm) -> Response {
    let names: Vec<String> = form
        .get("categories")
        .unwrap_or_default()
        .split(['\n', '\r'])
        .map(|s| s.trim().to_owned())
        .filter(|s| !s.is_empty())
        .collect();
    let _ = state.store.update(|d| {
        for n in &names {
            d.categories.remove(n);
            for m in d.torrents.values_mut() {
                if &m.category == n {
                    // Removing a category never moves files; they stay put.
                    m.category.clear();
                    m.auto_managed = false;
                }
            }
        }
    });
    StatusCode::OK.into_response()
}

async fn categories(State(state): State<Shared>) -> Json<Value> {
    let cats = state.store.read(|d| d.categories.clone());
    Json(Value::Object(
        cats.into_iter()
            .map(|(name, c)| {
                // qBittorrent reports the folder the category actually uses.
                let path = state.resolve_save_path(None, &name).to_string_lossy().into_owned();
                let shown = if c.save_path.is_empty() { path } else { c.save_path };
                (name.clone(), json!({ "name": name, "savePath": shown, "downloadPath": "" }))
            })
            .collect(),
    ))
}

fn tag_list(form: &QForm) -> Vec<String> {
    form.get("tags")
        .unwrap_or_default()
        .split(',')
        .map(|t| t.trim().to_owned())
        .filter(|t| !t.is_empty())
        .collect()
}

async fn add_tags(State(state): State<Shared>, form: QForm) -> Response {
    let (list, tags) = (hashes(&state, form.get("hashes")), tag_list(&form));
    let _ = state.store.update(|d| {
        d.tags.extend(tags.iter().cloned());
        for h in &list {
            if let Some(m) = d.torrents.get_mut(h) {
                m.tags.extend(tags.iter().cloned());
            }
        }
    });
    StatusCode::OK.into_response()
}

async fn remove_tags(State(state): State<Shared>, form: QForm) -> Response {
    let (list, tags) = (hashes(&state, form.get("hashes")), tag_list(&form));
    let _ = state.store.update(|d| {
        for h in &list {
            if let Some(m) = d.torrents.get_mut(h) {
                if tags.is_empty() {
                    m.tags.clear();
                } else {
                    m.tags.retain(|t| !tags.contains(t));
                }
            }
        }
    });
    StatusCode::OK.into_response()
}

async fn create_tags(State(state): State<Shared>, form: QForm) -> Response {
    let tags = tag_list(&form);
    let _ = state.store.update(|d| d.tags.extend(tags));
    StatusCode::OK.into_response()
}

async fn delete_tags(State(state): State<Shared>, form: QForm) -> Response {
    let tags = tag_list(&form);
    let _ = state.store.update(|d| {
        for t in &tags {
            d.tags.remove(t);
            for m in d.torrents.values_mut() {
                m.tags.remove(t);
            }
        }
    });
    StatusCode::OK.into_response()
}

async fn tags(State(state): State<Shared>) -> Json<Value> {
    let all = state.store.read(|d| {
        let mut t = d.tags.clone();
        for m in d.torrents.values() {
            t.extend(m.tags.iter().cloned());
        }
        t
    });
    Json(json!(all))
}

async fn set_share_limits(State(state): State<Shared>, form: QForm) -> Response {
    let list = hashes(&state, form.get("hashes"));
    let num = |k: &str| form.get(k).and_then(|v| v.trim().parse::<f64>().ok());
    let (ratio, time, idle) = (num("ratioLimit"), num("seedingTimeLimit"), num("inactiveSeedingTimeLimit"));
    set_queued_flag(&state, &list, |m| {
        if let Some(r) = ratio {
            m.ratio_limit = r;
        }
        if let Some(t) = time {
            m.seeding_time_limit = t as i64;
        }
        if let Some(i) = idle {
            m.inactive_seeding_time_limit = i as i64;
        }
    })
    .await;
    StatusCode::OK.into_response()
}

/// Queue priority moves only make sense with queueing on; qBittorrent
/// answers 409 otherwise (and Sonarr expects that).
async fn move_prio(state: &Shared, form: &QForm, top: bool) -> Response {
    if !state.store.read(|d| d.settings.queue.enabled) {
        return text(StatusCode::CONFLICT, "Torrent queueing must be enabled");
    }
    let list = hashes(state, form.get("hashes"));
    let (min, max) = state.store.read(|d| {
        let ps = d.torrents.values().map(|m| if m.priority == 0 { m.added_on } else { m.priority });
        (ps.clone().min().unwrap_or(0), ps.max().unwrap_or(0))
    });
    let mut next = if top { min - list.len() as i64 } else { max + 1 };
    let _ = state.store.update(|d| {
        for h in &list {
            if let Some(m) = d.torrents.get_mut(h) {
                m.priority = next;
                next += 1;
            }
        }
    });
    StatusCode::OK.into_response()
}

async fn top_prio(State(state): State<Shared>, form: QForm) -> Response {
    move_prio(&state, &form, true).await
}

async fn bottom_prio(State(state): State<Shared>, form: QForm) -> Response {
    move_prio(&state, &form, false).await
}

async fn set_force_start(State(state): State<Shared>, form: QForm) -> Response {
    let list = hashes(&state, form.get("hashes"));
    let on = form.flag("value");
    set_queued_flag(&state, &list, |m| {
        m.force_start = on;
        if on {
            m.queued = false;
        }
    })
    .await;
    if on {
        for h in &list {
            let _ = state.engine.resume(h).await;
        }
    }
    StatusCode::OK.into_response()
}

// ---------------------------------------------------------------- transfer

async fn transfer_info(State(state): State<Shared>) -> Json<Value> {
    let snap = state.snapshot.borrow().clone();
    let s = state.store.read(|d| d.settings.speed.clone());
    let limit = |v: u32| i64::from(v) * 1024;
    let (dl, ul) = if snap.stats.alt_speed {
        (limit(s.alt_download_limit), limit(s.alt_upload_limit))
    } else {
        (limit(s.download_limit), limit(s.upload_limit))
    };
    Json(json!({
        "dl_info_speed": snap.stats.download_speed,
        "dl_info_data": snap.stats.session_downloaded,
        "up_info_speed": snap.stats.upload_speed,
        "up_info_data": snap.stats.session_uploaded,
        "dl_rate_limit": dl,
        "up_rate_limit": ul,
        "dht_nodes": state.engine.network().dht_nodes.unwrap_or(0),
        "connection_status": "connected",
    }))
}

async fn speed_limits_mode(State(state): State<Shared>) -> String {
    if state.snapshot.borrow().stats.alt_speed { "1".into() } else { "0".into() }
}

async fn toggle_speed_limits_mode(State(state): State<Shared>) -> Response {
    let _ = state.store.update(|d| d.settings.speed.alt_enabled = !d.settings.speed.alt_enabled);
    StatusCode::OK.into_response()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn view(state: TorrentState, progress: f64) -> TorrentView {
        TorrentView {
            hash: "a".repeat(40),
            name: "x".into(),
            state,
            progress,
            size: 100,
            downloaded: (progress * 100.0) as u64,
            uploaded: 0,
            ratio: 0.0,
            download_speed: 0,
            upload_speed: 0,
            eta: None,
            peers: 0,
            seeds: 0,
            save_path: "/data".into(),
            category: String::new(),
            tags: vec![],
            added_on: 0,
            completed_on: None,
            has_metadata: true,
            queued: false,
            error: None,
            content_path: "/data/x".into(),
            root_path: String::new(),
            ratio_limit: -2.0,
            seeding_time_limit: -2,
            inactive_seeding_time_limit: -2,
            force_start: false,
            priority: 0,
            last_activity: 0,
            download_path: String::new(),
            auto_managed: false,
            moving: None,
            move_error: None,
        }
    }

    #[test]
    fn states_match_qbittorrent_5() {
        assert_eq!(qbit_state(&view(TorrentState::Paused, 1.0)), "stoppedUP");
        assert_eq!(qbit_state(&view(TorrentState::Paused, 0.4)), "stoppedDL");
        assert_eq!(qbit_state(&view(TorrentState::Downloading, 0.4)), "stalledDL");
        assert_eq!(qbit_state(&view(TorrentState::Seeding, 1.0)), "stalledUP");
        assert_eq!(qbit_state(&view(TorrentState::Error, 0.4)), "error");
        assert_eq!(qbit_state(&view(TorrentState::Moving, 1.0)), "moving");
        let mut waiting = view(TorrentState::Seeding, 1.0);
        waiting.moving = Some(crate::state::MoveView { progress: 0.0, queued: true });
        assert_eq!(qbit_state(&waiting), "moving", "Sonarr must not import before the move");
        let mut meta = view(TorrentState::Initializing, 0.0);
        meta.has_metadata = false;
        assert_eq!(qbit_state(&meta), "metaDL");
        let mut q = view(TorrentState::Paused, 0.2);
        q.queued = true;
        assert_eq!(qbit_state(&q), "queuedDL");
        let mut up = view(TorrentState::Seeding, 1.0);
        up.upload_speed = 10;
        assert_eq!(qbit_state(&up), "uploading");
    }

    #[test]
    fn api_key_compare() {
        let h = api_key_hash("secret");
        assert!(constant_eq(&h, &api_key_hash("secret")));
        assert!(!constant_eq(&h, &api_key_hash("other")));
    }
}
