//! Usenet in Swarmy: settings for the downloader, saving the queue and
//! history across restarts, notifications, and the web UI's API under
//! `/api/usenet`. The SABnzbd-compatible API lives in `sab.rs`.

use std::{path::PathBuf, sync::LazyLock, time::Duration};

use axum::{
    Json, Router,
    extract::{Multipart, Path, Query, State},
    routing::{delete, get, post},
};
use serde::{Deserialize, Serialize};
use swarmy_usenet::{Event, JobStatus, JobView, Priority, Server, ServerStatus};

use crate::{
    api::{ApiError, ApiResult},
    state::Shared,
    store::{QueuedJob, UsenetSettings},
};

const HISTORY_LIMIT: usize = 1000;
/// NZBs are XML lists of message-ids; even huge ones are a few MB.
const MAX_NZB: usize = 50 * 1024 * 1024;

static HTTP: LazyLock<reqwest::Client> = LazyLock::new(|| {
    reqwest::Client::builder()
        .timeout(Duration::from_secs(60))
        .user_agent(concat!("Swarmy/", env!("CARGO_PKG_VERSION")))
        .build()
        .expect("HTTP client")
});

/// Where unfinished and finished jobs go: (incomplete, complete).
pub fn dirs(state: &Shared) -> (PathBuf, PathBuf) {
    let u = state.store.read(|d| d.settings.usenet.clone());
    // The demo keeps its downloads out of your real Downloads folder.
    let base = if state.demo { state.data_dir.join("usenet") } else { state.download_dir().join("usenet") };
    let pick = |s: &str, default: PathBuf| if s.trim().is_empty() { default } else { PathBuf::from(s.trim()) };
    let complete = pick(&u.complete_dir, base.clone());
    let incomplete = pick(&u.incomplete_dir, base.join("incomplete"));
    let abs = |p: PathBuf| std::path::absolute(&p).unwrap_or(p);
    (abs(incomplete), abs(complete))
}

/// Push the saved settings into the downloader.
pub fn apply(state: &Shared) {
    let u: UsenetSettings = state.store.read(|d| d.settings.usenet.clone());
    let (incomplete, complete) = dirs(state);
    state.usenet.set_dirs(incomplete, complete);
    // The demo is capped so its progress is watchable.
    let limit = if state.demo && u.speed_limit == 0 { 6 * 1024 } else { u.speed_limit };
    state.usenet.set_speed_limit(limit * 1024);
    state.usenet.set_paused(u.paused);
    let mut servers = u.servers;
    if state.demo {
        servers = state.demo_usenet.lock().unwrap().clone().into_iter().collect();
    }
    state.usenet.set_servers(servers);
}

/// Restore the queue and history, and start the background tasks.
pub fn start(state: &Shared) {
    apply(state);
    let (queue, history) = state.store.read(|d| (d.usenet_queue.clone(), d.usenet_history.clone()));
    state.usenet.restore_history(history);
    for j in queue {
        if let Err(e) = state.usenet.restore(&j.id, &j.name, &j.category, j.priority, j.paused) {
            tracing::warn!("couldn't restore Usenet job {}: {e:#}", j.name);
        }
    }
    // Notifications and history.
    let s = state.clone();
    tokio::spawn(async move {
        let mut rx = s.usenet.subscribe();
        loop {
            match rx.recv().await {
                Ok(Event::Completed(job)) => {
                    s.notify("completed", "Download finished", &job.name, "");
                    s.stats.event("completed", &job.id, &job.name, job.size, &job.category);
                    save_history(&s);
                    save_queue(&s);
                }
                Ok(Event::Failed(job)) => {
                    let why = job.fail_message.clone().unwrap_or_default();
                    s.notify("errored", "Download failed", &format!("{}: {why}", job.name), "");
                    save_history(&s);
                    save_queue(&s);
                }
                Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
                Err(_) => break,
            }
        }
    });
    if state.demo {
        tokio::spawn(run_demo(state.clone()));
    }
    // Keep the saved queue in step (pauses, reorders) without a write per change.
    let s = state.clone();
    tokio::spawn(async move {
        let mut tick = tokio::time::interval(Duration::from_secs(5));
        loop {
            tick.tick().await;
            save_queue(&s);
        }
    });
}

/// Demo mode: a local mock news server and a steady supply of releases.
async fn run_demo(state: Shared) {
    let addr = match swarmy_usenet::demo::start().await {
        Ok(a) => a,
        Err(e) => return tracing::warn!("demo news server: {e}"),
    };
    *state.demo_usenet.lock().unwrap() = Some(Server {
        id: "demo".into(),
        name: "Swarmy demo news server".into(),
        enabled: true,
        priority: 0,
        config: swarmy_usenet::ServerConfig { host: addr.ip().to_string(), port: addr.port(), tls: false, username: String::new(), password: String::new(), connections: 8 },
    });
    apply(&state);
    let releases = swarmy_usenet::demo::RELEASES;
    let mut round = 0u64;
    let mut next = 0usize;
    loop {
        // Keep two or three jobs queued.
        while state.usenet.queue().len() < 3 {
            let r = &releases[next % releases.len()];
            let name = if round == 0 { r.name.to_string() } else { format!("{}.REPACK{round}", r.name) };
            let _ = add(&state, &swarmy_usenet::demo::nzb(r, round), Some(&name), r.category, Priority::Normal, false);
            next += 1;
            if next.is_multiple_of(releases.len()) {
                round += 1;
            }
        }
        // Finished demo files are deleted after a few minutes; history stays.
        let now = crate::store::now_unix();
        for j in state.usenet.history() {
            if let (Some(done), Some(p)) = (j.completed, &j.storage)
                && now - done > 300
                && p.starts_with(&state.data_dir)
                && p.exists()
            {
                let _ = std::fs::remove_dir_all(p);
            }
        }
        tokio::time::sleep(Duration::from_secs(10)).await;
    }
}

pub fn save_queue(state: &Shared) {
    let now: Vec<QueuedJob> = state
        .usenet
        .queue()
        .into_iter()
        .filter(|j| !j.status.finished())
        .map(|j| QueuedJob { id: j.id, name: j.name, category: j.category, priority: j.priority, paused: j.status == JobStatus::Paused })
        .collect();
    if state.store.read(|d| d.usenet_queue != now)
        && let Err(e) = state.store.update(|d| d.usenet_queue = now)
    {
        tracing::warn!("saving the Usenet queue: {e:#}");
    }
}

fn save_history(state: &Shared) {
    let mut history = state.usenet.history();
    history.truncate(HISTORY_LIMIT);
    if let Err(e) = state.store.update(|d| d.usenet_history = history) {
        tracing::warn!("saving Usenet history: {e:#}");
    }
}

/// Queue NZB bytes; the one place adds go through (UI, SABnzbd API).
pub fn add(state: &Shared, nzb: &[u8], name: Option<&str>, category: &str, priority: Priority, paused: bool) -> anyhow::Result<String> {
    let category = if category == "*" || category.eq_ignore_ascii_case("none") { "" } else { category };
    let id = state.usenet.add(nzb, name, category, priority)?;
    if paused {
        state.usenet.pause_job(&id, true);
    }
    if let Some(job) = state.usenet.job(&id) {
        state.notify("added", "Download added", &job.name, "");
        state.stats.event("added", &id, &job.name, job.size, category);
    }
    save_queue(state);
    Ok(id)
}

/// Fetch an NZB from a URL (indexer links), with a size cap.
pub async fn fetch_nzb(url: &str) -> anyhow::Result<(Vec<u8>, Option<String>)> {
    let parsed = reqwest::Url::parse(url)?;
    if !matches!(parsed.scheme(), "http" | "https") {
        anyhow::bail!("only http and https links can be added");
    }
    let mut resp = HTTP.get(parsed).send().await?.error_for_status()?;
    // Indexers name the file in Content-Disposition.
    let name = resp
        .headers()
        .get(reqwest::header::CONTENT_DISPOSITION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.split("filename=").nth(1))
        .map(|f| f.trim_matches(['"', '\'', ' ', ';']).trim_end_matches(".nzb").to_string())
        .filter(|f| !f.is_empty());
    let mut body = Vec::new();
    while let Some(chunk) = resp.chunk().await? {
        body.extend_from_slice(&chunk);
        if body.len() > MAX_NZB {
            anyhow::bail!("the NZB is too large");
        }
    }
    Ok((body, name))
}

// ---------------------------------------------------------------- web UI API

pub fn router() -> Router<Shared> {
    Router::new()
        .route("/usenet", get(overview))
        .route("/usenet/add", post(add_nzbs))
        .route("/usenet/pause", post(pause_all))
        .route("/usenet/jobs/{id}/pause", post(pause_job))
        .route("/usenet/jobs/{id}/resume", post(resume_job))
        .route("/usenet/jobs/{id}/move", post(move_job))
        .route("/usenet/jobs/{id}", delete(remove_job))
        .route("/usenet/history/{id}", delete(remove_history))
        .route("/usenet/history", delete(clear_history))
        .route("/usenet/servers/test", post(test_server))
}

#[derive(Serialize)]
struct Overview {
    paused: bool,
    /// Bytes per second.
    speed: u64,
    remaining: u64,
    queue: Vec<JobView>,
    history: Vec<JobView>,
    servers: Vec<ServerStatus>,
    complete_dir: PathBuf,
}

#[derive(Deserialize)]
struct HistoryQuery {
    limit: Option<usize>,
}

async fn overview(State(state): State<Shared>, Query(q): Query<HistoryQuery>) -> Json<Overview> {
    let mut history = state.usenet.history();
    history.truncate(q.limit.unwrap_or(100).min(HISTORY_LIMIT));
    Json(Overview {
        paused: state.usenet.paused(),
        speed: state.usenet.speed(),
        remaining: state.usenet.remaining(),
        queue: state.usenet.queue(),
        history,
        servers: state.usenet.servers(),
        complete_dir: dirs(&state).1,
    })
}

fn parse_priority(s: &str) -> (Priority, bool) {
    match s.trim() {
        "-2" | "paused" => (Priority::Normal, true),
        "-1" | "low" => (Priority::Low, false),
        "1" | "high" => (Priority::High, false),
        "2" | "force" => (Priority::Force, false),
        _ => (Priority::Normal, false),
    }
}

/// Multipart: one or more `nzb` files and/or `url` fields, plus optional
/// `category`, `priority` and `name`.
async fn add_nzbs(State(state): State<Shared>, mut mp: Multipart) -> ApiResult<Json<Vec<String>>> {
    let mut files: Vec<(Vec<u8>, Option<String>)> = Vec::new();
    let mut urls = Vec::new();
    let mut category = String::new();
    let mut priority = String::new();
    let mut name = None;
    while let Some(field) = mp.next_field().await.map_err(|e| ApiError::bad_request(e.to_string()))? {
        match field.name().unwrap_or("") {
            "nzb" | "nzbfile" | "name" => {
                let fname = field.file_name().map(|f| f.trim_end_matches(".nzb").to_string());
                let bytes = field.bytes().await.map_err(|e| ApiError::bad_request(e.to_string()))?;
                if bytes.len() > MAX_NZB {
                    return Err(ApiError::bad_request("that NZB is too large"));
                }
                files.push((bytes.to_vec(), fname));
            }
            "url" | "urls" => {
                let text = field.text().await.unwrap_or_default();
                urls.extend(text.lines().map(str::trim).filter(|l| !l.is_empty()).map(String::from));
            }
            "category" => category = field.text().await.unwrap_or_default(),
            "priority" => priority = field.text().await.unwrap_or_default(),
            "title" => name = Some(field.text().await.unwrap_or_default()).filter(|n| !n.trim().is_empty()),
            _ => {}
        }
    }
    if files.is_empty() && urls.is_empty() {
        return Err(ApiError::bad_request("Pick an NZB file or paste a link."));
    }
    for url in urls {
        let (bytes, fname) = fetch_nzb(&url).await.map_err(|e| ApiError::bad_request(format!("Couldn't fetch {url}: {e:#}")))?;
        files.push((bytes, fname));
    }
    let (priority, paused) = parse_priority(&priority);
    let mut ids = Vec::new();
    let single = files.len() == 1;
    for (bytes, fname) in files {
        let n = if single { name.clone().or(fname) } else { fname };
        let id = add(&state, &bytes, n.as_deref(), &category, priority, paused).map_err(|e| ApiError::bad_request(format!("{e:#}")))?;
        ids.push(id);
    }
    Ok(Json(ids))
}

#[derive(Deserialize)]
struct PauseReq {
    paused: bool,
}

async fn pause_all(State(state): State<Shared>, Json(r): Json<PauseReq>) -> ApiResult<Json<bool>> {
    state.store.update(|d| d.settings.usenet.paused = r.paused)?;
    state.usenet.set_paused(r.paused);
    Ok(Json(r.paused))
}

async fn pause_job(State(state): State<Shared>, Path(id): Path<String>) -> ApiResult<()> {
    if !state.usenet.pause_job(&id, true) {
        return Err(ApiError::bad_request("No such download"));
    }
    save_queue(&state);
    Ok(())
}

async fn resume_job(State(state): State<Shared>, Path(id): Path<String>) -> ApiResult<()> {
    if !state.usenet.pause_job(&id, false) {
        return Err(ApiError::bad_request("No such download"));
    }
    save_queue(&state);
    Ok(())
}

#[derive(Deserialize)]
struct MoveReq {
    index: usize,
}

async fn move_job(State(state): State<Shared>, Path(id): Path<String>, Json(r): Json<MoveReq>) -> ApiResult<Json<usize>> {
    let at = state.usenet.move_job(&id, r.index).ok_or_else(|| ApiError::bad_request("No such download"))?;
    save_queue(&state);
    Ok(Json(at))
}

#[derive(Deserialize)]
struct DeleteQuery {
    #[serde(default)]
    delete_files: bool,
}

async fn remove_job(State(state): State<Shared>, Path(id): Path<String>) -> ApiResult<()> {
    if !state.usenet.remove(&id, true) {
        return Err(ApiError::bad_request("No such download"));
    }
    save_queue(&state);
    Ok(())
}

async fn remove_history(State(state): State<Shared>, Path(id): Path<String>, Query(q): Query<DeleteQuery>) -> ApiResult<()> {
    if !state.usenet.remove(&id, q.delete_files) {
        return Err(ApiError::bad_request("No such download"));
    }
    save_history(&state);
    Ok(())
}

async fn clear_history(State(state): State<Shared>) -> ApiResult<()> {
    for j in state.usenet.history() {
        state.usenet.remove(&j.id, false);
    }
    save_history(&state);
    Ok(())
}

#[derive(Serialize)]
struct TestResult {
    ok: bool,
    message: String,
}

/// Check a server's details before saving them. An empty password means
/// "the one already saved for this server".
async fn test_server(State(state): State<Shared>, Json(mut server): Json<Server>) -> Json<TestResult> {
    if server.config.password.is_empty() {
        let saved = state.store.read(|d| d.settings.usenet.servers.iter().find(|s| s.id == server.id).map(|s| s.config.password.clone()));
        if let Some(p) = saved {
            server.config.password = p;
        }
    }
    match tokio::time::timeout(Duration::from_secs(30), swarmy_usenet::downloader::test_server(&server.config)).await {
        Ok(Ok(())) => Json(TestResult {
            ok: true,
            message: if server.config.username.is_empty() { "Connected".into() } else { "Connected and signed in".into() },
        }),
        Ok(Err(e)) => Json(TestResult { ok: false, message: e.to_string() }),
        Err(_) => Json(TestResult { ok: false, message: "The server didn't answer in time".into() }),
    }
}

/// Settings as the UI sees them: server passwords never leave the server.
pub fn redact(u: &mut UsenetSettings) {
    for s in &mut u.servers {
        s.config.password.clear();
    }
}

/// On save: keep stored passwords where the UI sent none, and check the rest.
pub fn merge_and_validate(new: &mut UsenetSettings, old: &UsenetSettings) -> Result<(), String> {
    let mut ids = std::collections::HashSet::new();
    for s in &mut new.servers {
        if s.id.trim().is_empty() || !ids.insert(s.id.clone()) {
            return Err("Each Usenet server needs its own id.".into());
        }
        if s.config.host.trim().is_empty() {
            return Err("A Usenet server is missing its address.".into());
        }
        if s.config.port == 0 {
            return Err(format!("{}: pick a port (usually 563 for SSL, 119 without).", s.config.host));
        }
        if !(1..=200).contains(&s.config.connections) {
            return Err(format!("{}: use between 1 and 200 connections.", s.config.host));
        }
        s.config.host = s.config.host.trim().to_string();
        if s.config.password.is_empty()
            && let Some(o) = old.servers.iter().find(|o| o.id == s.id)
        {
            s.config.password = o.config.password.clone();
        }
    }
    if new.servers.len() > 64 {
        return Err("That's a lot of servers; keep it to 64.".into());
    }
    Ok(())
}
