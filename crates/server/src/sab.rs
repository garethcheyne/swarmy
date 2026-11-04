//! A SABnzbd-compatible API, so Sonarr, Radarr and friends can use Swarmy as
//! a "SABnzbd" download client. Served at `/api` and `/sabnzbd/api`.
//!
//! The contract (fields, types, status strings, error shapes) is recorded in
//! design/sabnzbd-compat.md, read from Sonarr's and Radarr's source.
//!
//! Auth: `apikey` (or `nzbkey`) is Swarmy's API key from Settings > Sonarr
//! and Radarr; `ma_username`/`ma_password` are the Swarmy account. Wrong keys
//! count towards the same per-address lockout as sign-in.

use std::{collections::HashMap, net::SocketAddr, time::Duration};

use axum::{
    extract::{ConnectInfo, FromRequest, Multipart, Request, State},
    http::{HeaderMap, Method, StatusCode, header},
    response::{IntoResponse, Response},
};
use serde_json::{Value, json};
use swarmy_usenet::{JobStatus, JobView, Priority};

use crate::{auth, state::Shared};

/// SABnzbd version we report; Sonarr needs >= 1.0 and parses x.y.z.
const VERSION: &str = "4.5.5";

fn reply(v: Value) -> Response {
    (StatusCode::OK, [(header::CONTENT_TYPE, "application/json; charset=utf-8")], v.to_string()).into_response()
}

fn error(msg: &str) -> Response {
    reply(json!({ "status": false, "error": msg }))
}

fn ok() -> Response {
    reply(json!({ "status": true }))
}

pub async fn api(State(state): State<Shared>, ConnectInfo(peer): ConnectInfo<SocketAddr>, req: Request) -> Response {
    let q: HashMap<String, String> = req
        .uri()
        .query()
        .map(|q| form_urlencoded::parse(q.as_bytes()).into_owned().collect())
        .unwrap_or_default();
    let mode = q.get("mode").map(String::as_str).unwrap_or("");

    // Real SABnzbd answers version without a key.
    if mode == "version" {
        return reply(json!({ "version": VERSION }));
    }
    if !state.store.read(|d| d.settings.usenet.sab_api) {
        return error("The SABnzbd API is turned off in Swarmy's settings");
    }

    // Authentication, with the sign-in lockout.
    let headers: HeaderMap = req.headers().clone();
    let web = state.store.read(|d| d.settings.webui.clone());
    let client = auth::client(peer, &headers, &web.trusted_proxies);
    if state.auth.locked_for(client.ip).is_some() {
        return error("API Key Incorrect");
    }
    let key = q.get("apikey").or_else(|| q.get("nzbkey")).filter(|k| !k.is_empty());
    let authed = if let Some(key) = key {
        crate::keys::verify(&state.store, key)
    } else if let (Some(u), Some(p)) = (q.get("ma_username"), q.get("ma_password")) {
        auth::check_credentials(&state, u, p).await
    } else {
        return error("API Key Required");
    };
    if !authed {
        state.auth.record_failure(client.ip, web.max_attempts, Duration::from_secs(u64::from(web.lockout_minutes.max(1)) * 60));
        return error("API Key Incorrect");
    }

    let is_post = req.method() == Method::POST;
    match mode {
        "get_config" => get_config(&state),
        "fullstatus" => reply(json!({ "status": { "completedir": complete_dir(&state) } })),
        "queue" => match q.get("name").map(String::as_str) {
            Some("delete") => delete(&state, &q, false),
            Some("pause") => job_pause(&state, &q, true),
            Some("resume") => job_pause(&state, &q, false),
            _ => queue(&state, &q),
        },
        "history" => match q.get("name").map(String::as_str) {
            Some("delete") => delete(&state, &q, true),
            _ => history(&state, &q),
        },
        "addfile" | "addlocalfile" if is_post => addfile(&state, &q, req).await,
        "addurl" => addurl(&state, &q).await,
        "pause" => {
            let _ = state.store.update(|d| d.settings.usenet.paused = true);
            state.usenet.set_paused(true);
            ok()
        }
        "resume" => {
            let _ = state.store.update(|d| d.settings.usenet.paused = false);
            state.usenet.set_paused(false);
            ok()
        }
        "get_cats" => reply(json!({ "categories": category_names(&state) })),
        "switch" => switch(&state, &q),
        "retry" => error("Retrying isn't supported; add the NZB again"),
        "" => error("No mode given"),
        _ => error("Not implemented"),
    }
}

fn complete_dir(state: &Shared) -> String {
    crate::usenet::dirs(state).1.to_string_lossy().into_owned()
}

/// Swarmy's categories, plus Sonarr's and Radarr's defaults so their Test
/// button finds one out of the box.
fn category_names(state: &Shared) -> Vec<String> {
    let mut names: Vec<String> = state.store.read(|d| d.categories.keys().cloned().collect());
    for default in ["tv", "movies"] {
        if !names.iter().any(|n| n == default) {
            names.push(default.into());
        }
    }
    names.sort();
    names.insert(0, "*".into());
    names
}

fn get_config(state: &Shared) -> Response {
    let cats: Vec<Value> = category_names(state)
        .into_iter()
        .map(|name| {
            let dir = if name == "*" { String::new() } else { swarmy_usenet::nzb::sanitise(&name) };
            json!({ "name": name, "dir": dir, "priority": if name == "*" { 0 } else { -100 }, "pp": "3", "script": "None" })
        })
        .collect();
    reply(json!({
        "config": {
            "misc": {
                "complete_dir": complete_dir(state),
                "download_dir": crate::usenet::dirs(state).0.to_string_lossy(),
                "pre_check": 0,
                // Keep history, so Sonarr doesn't warn it'll vanish before import.
                "history_retention_option": "all",
                "history_retention_number": 1,
            },
            "categories": cats,
            "servers": [],
            "sorters": [],
        }
    }))
}

fn sab_status(s: JobStatus) -> &'static str {
    match s {
        JobStatus::Queued => "Queued",
        JobStatus::Downloading => "Downloading",
        JobStatus::Paused => "Paused",
        JobStatus::Fetching => "Fetching",
        JobStatus::Verifying => "Verifying",
        JobStatus::Repairing => "Repairing",
        JobStatus::Extracting => "Extracting",
        JobStatus::Moving => "Moving",
        JobStatus::Completed => "Completed",
        JobStatus::Failed => "Failed",
    }
}

fn sab_priority(p: Priority) -> &'static str {
    match p {
        Priority::Low => "Low",
        Priority::Normal => "Normal",
        Priority::High => "High",
        Priority::Force => "Force",
    }
}

/// SAB's "0:12:34" (or "1:02:03:04" past a day).
fn timeleft(secs: u64) -> String {
    let (d, h, m, s) = (secs / 86400, secs / 3600 % 24, secs / 60 % 60, secs % 60);
    if d > 0 { format!("{d}:{h:02}:{m:02}:{s:02}") } else { format!("{h}:{m:02}:{s:02}") }
}

fn cat(j: &JobView) -> String {
    if j.category.is_empty() { "*".into() } else { j.category.clone() }
}

fn wants(q: &HashMap<String, String>, j: &JobView) -> bool {
    q.get("category").filter(|c| !c.is_empty()).is_none_or(|c| *c == cat(j))
}

fn window<T>(items: Vec<T>, q: &HashMap<String, String>) -> Vec<T> {
    let start: usize = q.get("start").and_then(|s| s.parse().ok()).unwrap_or(0);
    let limit: usize = q.get("limit").and_then(|s| s.parse().ok()).unwrap_or(0);
    let it = items.into_iter().skip(start);
    if limit == 0 { it.collect() } else { it.take(limit).collect() }
}

fn queue(state: &Shared, q: &HashMap<String, String>) -> Response {
    let speed = state.usenet.speed();
    let paused = state.usenet.paused();
    let mut ahead: u64 = 0;
    let jobs: Vec<JobView> = state.usenet.queue().into_iter().filter(|j| wants(q, j)).collect();
    let slots: Vec<Value> = window(jobs, q)
        .into_iter()
        .enumerate()
        .map(|(index, j)| {
            let left = j.size.saturating_sub(j.downloaded);
            ahead += left;
            let mb = |b: u64| format!("{:.2}", b as f64 / 1_048_576.0);
            let pct = if j.size > 0 { j.downloaded * 100 / j.size } else { 0 };
            let eta = if speed > 0 { ahead / speed } else { 0 };
            json!({
                "index": index,
                "nzo_id": j.id,
                "filename": j.name,
                "cat": cat(&j),
                "priority": sab_priority(j.priority),
                "status": sab_status(j.status),
                "mb": mb(j.size),
                "mbleft": mb(left),
                "percentage": pct.to_string(),
                "timeleft": timeleft(eta),
            })
        })
        .collect();
    reply(json!({
        "queue": {
            "paused": paused,
            "slots": slots,
            "speed": format!("{:.1} K", speed as f64 / 1024.0),
            "kbpersec": format!("{:.2}", speed as f64 / 1024.0),
            "noofslots": slots.len(),
            "version": VERSION,
        }
    }))
}

fn history(state: &Shared, q: &HashMap<String, String>) -> Response {
    let jobs: Vec<JobView> = state.usenet.history().into_iter().filter(|j| wants(q, j)).collect();
    let total = jobs.len();
    let slots: Vec<Value> = window(jobs, q)
        .into_iter()
        .map(|j| {
            let storage = j.storage.as_ref().map(|p| p.to_string_lossy().into_owned()).unwrap_or_default();
            json!({
                "nzo_id": j.id,
                "name": j.name,
                "nzb_name": format!("{}.nzb", j.name),
                "category": cat(&j),
                "bytes": j.size,
                "status": sab_status(j.status),
                "storage": storage,
                "fail_message": j.fail_message.clone().unwrap_or_default(),
                "download_time": j.completed.unwrap_or(j.added).saturating_sub(j.added),
                "completed": j.completed.unwrap_or(0),
            })
        })
        .collect();
    reply(json!({ "history": { "slots": slots, "noofslots": total, "paused": state.usenet.paused() } }))
}

fn ids(q: &HashMap<String, String>) -> Vec<String> {
    q.get("value").map(|v| v.split(',').map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect()).unwrap_or_default()
}

fn delete(state: &Shared, q: &HashMap<String, String>, from_history: bool) -> Response {
    let del_files = q.get("del_files").is_some_and(|v| v == "1");
    let mut done = Vec::new();
    for id in ids(q) {
        // "all" clears the history (SAB's API allows it).
        if from_history && id == "all" {
            for j in state.usenet.history() {
                state.usenet.remove(&j.id, del_files);
            }
            done.push(id);
            continue;
        }
        if state.usenet.remove(&id, del_files) {
            done.push(id);
        }
    }
    crate::usenet::save_queue(state);
    let history = state.usenet.history();
    let _ = state.store.update(|d| d.usenet_history = history);
    reply(json!({ "status": true, "nzo_ids": done }))
}

fn job_pause(state: &Shared, q: &HashMap<String, String>, paused: bool) -> Response {
    let done: Vec<String> = ids(q).into_iter().filter(|id| state.usenet.pause_job(id, paused)).collect();
    crate::usenet::save_queue(state);
    reply(json!({ "status": true, "nzo_ids": done }))
}

/// `mode=switch&value=A&value2=B`: move job A to B's position (or to index B).
fn switch(state: &Shared, q: &HashMap<String, String>) -> Response {
    let Some(id) = q.get("value") else { return error("No job given") };
    let queue = state.usenet.queue();
    let target = q.get("value2").and_then(|v| v.parse::<usize>().ok().or_else(|| queue.iter().position(|j| &j.id == v)));
    match target.and_then(|t| state.usenet.move_job(id, t)) {
        Some(pos) => reply(json!({ "result": { "position": pos, "priority": 0 } })),
        None => error("No such job"),
    }
}

fn priority_from(q: &HashMap<String, String>) -> (Priority, bool) {
    match q.get("priority").map(String::as_str) {
        Some("-2") => (Priority::Normal, true),
        Some("-1") => (Priority::Low, false),
        Some("1") => (Priority::High, false),
        Some("2") => (Priority::Force, false),
        _ => (Priority::Normal, false),
    }
}

fn added(state: &Shared, q: &HashMap<String, String>, nzb: &[u8], filename: Option<String>) -> Response {
    let (priority, paused) = priority_from(q);
    let category = q.get("cat").cloned().unwrap_or_default();
    let name = q.get("nzbname").filter(|n| !n.trim().is_empty()).cloned().or(filename);
    match crate::usenet::add(state, nzb, name.as_deref(), &category, priority, paused) {
        Ok(id) => reply(json!({ "status": true, "nzo_ids": [id] })),
        Err(e) => error(&format!("{e:#}")),
    }
}

async fn addfile(state: &Shared, q: &HashMap<String, String>, req: Request) -> Response {
    let Ok(mut mp) = Multipart::from_request(req, &()).await else {
        return error("Expected an NZB file upload");
    };
    // Sonarr sends one part named "name"; others use "nzbfile".
    while let Ok(Some(field)) = mp.next_field().await {
        if matches!(field.name(), Some("name" | "nzbfile" | "nzb")) {
            let filename = field.file_name().map(|f| f.trim_end_matches(".nzb").to_string());
            return match field.bytes().await {
                Ok(bytes) if bytes.len() <= 50 * 1024 * 1024 => added(state, q, &bytes, filename),
                Ok(_) => error("The NZB is too large"),
                Err(e) => error(&e.to_string()),
            };
        }
    }
    error("No NZB file in the upload")
}

async fn addurl(state: &Shared, q: &HashMap<String, String>) -> Response {
    let Some(url) = q.get("name").filter(|u| !u.is_empty()) else { return error("No URL given") };
    match crate::usenet::fetch_nzb(url).await {
        Ok((bytes, filename)) => added(state, q, &bytes, filename),
        Err(e) => error(&format!("Couldn't fetch the NZB: {e:#}")),
    }
}
