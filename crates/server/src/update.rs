//! Knowing about, and installing, new Swarmy releases.
//!
//! Swarmy checks GitHub's releases every few hours (Settings → About can turn
//! that off). Installing depends on how Swarmy was installed:
//!
//! - The Linux installer (`install/install.sh`) sets up `swarmy-update.path`,
//!   a systemd unit that watches for a request file, and tells Swarmy where
//!   that file is with `SWARMY_UPDATE_REQUEST`. Swarmy runs unprivileged, so
//!   it only *asks*: creating the file starts `swarmy-update.service`, which
//!   runs as root, finds the newest release itself and installs it with the
//!   checksum verified. Swarmy never chooses what gets installed. The
//!   updater's output goes to `SWARMY_UPDATE_LOG`.
//! - Docker: pull the new image. Swarmy says how.
//! - Anything else: the UI points at the release page.

use std::{
    path::PathBuf,
    sync::Mutex,
    time::{Duration, SystemTime},
};

use axum::{
    Json, Router,
    extract::State,
    http::StatusCode,
    routing::{get, post},
};
use serde::{Deserialize, Serialize};

use crate::{
    api::{ApiError, ApiResult},
    changelog::{self, Release},
    state::Shared,
    store::now_unix,
};

const REPO: &str = "garethcheyne/swarmy";
const EVERY: Duration = Duration::from_secs(6 * 3600);
/// The updater should pick a request up within a second or two.
const STUCK_AFTER: i64 = 60;

/// A version's numbers (`2026.11.05.4` → [2026, 11, 5, 4]), for ordering.
pub fn version_key(v: &str) -> Option<Vec<u32>> {
    let v = v.trim().trim_start_matches('v');
    let parts: Option<Vec<u32>> = v.split('.').map(|p| p.parse().ok()).collect();
    parts.filter(|p| !p.is_empty())
}

fn newer(candidate: &str, than: &str) -> bool {
    match (version_key(candidate), version_key(than)) {
        (Some(a), Some(b)) => a > b,
        _ => false,
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct Available {
    pub version: String,
    pub url: String,
    pub published: Option<String>,
}

#[derive(Default)]
struct Known {
    latest: Option<Available>,
    /// Release notes of every version newer than this one, newest first.
    notes: Vec<Release>,
    checked_at: Option<i64>,
    error: Option<String>,
}

pub struct Updates {
    known: Mutex<Known>,
    http: reqwest::Client,
    /// Where to ask the systemd updater for an update (Linux installer only).
    request: Option<PathBuf>,
    log: Option<PathBuf>,
    in_docker: bool,
}

impl Updates {
    pub fn from_env() -> Self {
        let path = |name: &str| std::env::var_os(name).filter(|v| !v.is_empty()).map(PathBuf::from);
        Self {
            known: Mutex::default(),
            http: reqwest::Client::builder()
                .https_only(true)
                .connect_timeout(Duration::from_secs(15))
                .timeout(Duration::from_secs(30))
                .user_agent(concat!("Swarmy/", env!("SWARMY_VERSION")))
                .build()
                .expect("building the HTTP client"),
            request: path("SWARMY_UPDATE_REQUEST"),
            log: path("SWARMY_UPDATE_LOG"),
            in_docker: std::env::var_os("SWARMY_IN_CONTAINER").is_some(),
        }
    }

    /// The newest release, if it's newer than this one (for the status bar).
    pub fn available(&self) -> Option<String> {
        self.known.lock().unwrap().latest.as_ref().map(|a| a.version.clone())
    }

    async fn check(&self) {
        let result = self.fetch().await;
        let mut known = self.known.lock().unwrap();
        known.checked_at = Some(now_unix());
        match result {
            Ok((latest, notes)) => {
                known.latest = latest;
                known.notes = notes;
                known.error = None;
            }
            Err(e) => {
                tracing::warn!("checking for a Swarmy update: {e:#}");
                known.error = Some(format!("Couldn't reach GitHub to check: {e:#}"));
            }
        }
    }

    async fn fetch(&self) -> anyhow::Result<(Option<Available>, Vec<Release>)> {
        #[derive(Deserialize)]
        struct GhRelease {
            tag_name: String,
            html_url: String,
            #[serde(default)]
            body: Option<String>,
            #[serde(default)]
            published_at: Option<String>,
            #[serde(default)]
            draft: bool,
            #[serde(default)]
            prerelease: bool,
        }
        let list: Vec<GhRelease> = self
            .http
            .get(format!("https://api.github.com/repos/{REPO}/releases?per_page=30"))
            .header("accept", "application/vnd.github+json")
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;
        let current = env!("SWARMY_VERSION");
        let mut newer_ones: Vec<GhRelease> = list
            .into_iter()
            .filter(|r| !r.draft && !r.prerelease && newer(&r.tag_name, current))
            .collect();
        newer_ones.sort_by_key(|r| std::cmp::Reverse(version_key(&r.tag_name)));
        let notes = newer_ones
            .iter()
            .filter_map(|r| {
                let version = r.tag_name.trim_start_matches('v');
                let date = r.published_at.as_deref().and_then(|d| d.get(..10)).unwrap_or("");
                // The release body is that version's CHANGELOG.md entry, then an
                // "## Install" note; parse() stops the release at that heading.
                let src = format!("## [{version}] - {date}\n{}", r.body.as_deref().unwrap_or(""));
                changelog::parse(&src).into_iter().next()
            })
            .collect();
        let latest = newer_ones.first().map(|r| Available {
            version: r.tag_name.trim_start_matches('v').to_owned(),
            url: r.html_url.clone(),
            published: r.published_at.clone(),
        });
        Ok((latest, notes))
    }

    fn method(&self) -> &'static str {
        if self.request.is_some() {
            "installer"
        } else if self.in_docker {
            "docker"
        } else {
            "manual"
        }
    }

    /// Since when the request has been waiting for the updater, if it is.
    fn pending_since(&self) -> Option<i64> {
        let meta = std::fs::symlink_metadata(self.request.as_ref()?).ok()?;
        Some(unix(meta.modified().ok()?))
    }

    fn last_run(&self) -> Option<LastRun> {
        let path = self.log.as_ref()?;
        let meta = std::fs::metadata(path).ok()?;
        let text = std::fs::read_to_string(path).ok()?;
        if text.trim().is_empty() {
            return None;
        }
        let lines: Vec<&str> = text.lines().collect();
        let tail = lines[lines.len().saturating_sub(20)..].join("\n");
        // The updater's last line, written as it exits.
        let status = lines
            .iter()
            .rev()
            .find_map(|l| l.trim().strip_prefix("Updater finished: exit status "))
            .and_then(|s| s.trim().parse::<i32>().ok());
        Some(LastRun {
            at: unix(meta.modified().ok()?),
            running: status.is_none(),
            ok: status == Some(0),
            log: strip_ansi(&tail),
        })
    }

    fn view(&self, check_enabled: bool) -> UpdateView {
        let known = self.known.lock().unwrap();
        let pending = self.pending_since();
        UpdateView {
            current: env!("SWARMY_VERSION"),
            latest: known.latest.clone(),
            notes: known.notes.clone(),
            checked_at: known.checked_at,
            error: known.error.clone(),
            check: check_enabled,
            method: self.method(),
            pending: pending.is_some(),
            stuck: pending.is_some_and(|t| now_unix() - t > STUCK_AFTER),
            last_run: self.last_run(),
        }
    }
}

fn unix(t: SystemTime) -> i64 {
    t.duration_since(SystemTime::UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0)
}

/// install.sh colours its messages; the log is shown as plain text.
fn strip_ansi(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars();
    while let Some(c) = chars.next() {
        if c == '\u{1b}' {
            for c in chars.by_ref() {
                if c.is_ascii_alphabetic() {
                    break;
                }
            }
        } else {
            out.push(c);
        }
    }
    out
}

#[derive(Serialize)]
struct LastRun {
    at: i64,
    /// Still going: the updater hasn't written its last line yet.
    running: bool,
    ok: bool,
    log: String,
}

#[derive(Serialize)]
pub struct UpdateView {
    current: &'static str,
    /// The newest release, when it's newer than this one.
    latest: Option<Available>,
    notes: Vec<Release>,
    checked_at: Option<i64>,
    error: Option<String>,
    /// Automatic checks are on.
    check: bool,
    /// How an update gets installed: "installer" (one click), "docker" or "manual".
    method: &'static str,
    /// An install was asked for and the updater hasn't started it yet.
    pending: bool,
    /// ...for long enough that the updater probably isn't running.
    stuck: bool,
    last_run: Option<LastRun>,
}

/// Checks soon after starting, then every few hours, while checks are on.
pub async fn run(state: Shared) {
    tokio::time::sleep(Duration::from_secs(20)).await;
    loop {
        if state.store.read(|d| d.settings.updates.check) {
            state.updates.check().await;
        }
        tokio::time::sleep(EVERY).await;
    }
}

pub fn router() -> Router<Shared> {
    Router::new()
        .route("/update", get(status))
        .route("/update/check", post(check_now))
        .route("/update/install", post(install))
}

async fn status(State(state): State<Shared>) -> Json<UpdateView> {
    let check = state.store.read(|d| d.settings.updates.check);
    Json(state.updates.view(check))
}

async fn check_now(State(state): State<Shared>) -> Json<UpdateView> {
    state.updates.check().await;
    status(State(state)).await
}

async fn install(State(state): State<Shared>) -> ApiResult<(StatusCode, Json<UpdateView>)> {
    let updates = &state.updates;
    let Some(request) = &updates.request else {
        return Err(ApiError::bad_request(match updates.method() {
            "docker" => "Swarmy runs in Docker, so update it by pulling the new image.",
            _ => "This copy of Swarmy can't update itself. Run the installer again to update it.",
        }));
    };
    if updates.available().is_none() {
        return Err(ApiError::bad_request("You already have the newest Swarmy."));
    }
    // What's in it doesn't matter: the updater always installs the newest release.
    std::fs::write(request, format!("{}\n", now_unix()))
        .map_err(|e| ApiError::from(anyhow::anyhow!("asking the updater to start ({}): {e}", request.display())))?;
    tracing::info!("update requested from the web UI");
    let check = state.store.read(|d| d.settings.updates.check);
    Ok((StatusCode::ACCEPTED, Json(updates.view(check))))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn versions_order_by_date_then_build() {
        assert!(newer("v2026.11.05.10", "2026.11.05.9"));
        assert!(newer("2026.11.06.1", "2026.11.05.9"));
        assert!(newer("2027.01.01.1", "2026.12.31.4"));
        assert!(!newer("2026.11.05.4", "2026.11.05.4"));
        assert!(!newer("2026.11.05.3", "2026.11.05.4"));
        assert!(!newer("nightly", "2026.11.05.4"));
    }

    #[test]
    fn release_body_parses_as_notes() {
        let body = "Give each app its own API key.\n\n### New\n- Named keys.\n- Expiry.\n\n## Install\n\nSee the README.";
        let src = format!("## [2026.11.05.4] - 2026-11-05\n{body}");
        let r = changelog::parse(&src).into_iter().next().unwrap();
        assert_eq!(r.version, "2026.11.05.4");
        assert_eq!(r.summary, "Give each app its own API key.");
        assert_eq!(r.sections[0].items, vec!["Named keys.", "Expiry."]);
    }

    #[test]
    fn colours_are_stripped() {
        assert_eq!(strip_ansi("\u{1b}[1;32m==>\u{1b}[0m Upgraded"), "==> Upgraded");
    }
}
