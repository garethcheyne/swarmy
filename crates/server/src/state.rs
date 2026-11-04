use std::{
    collections::HashSet,
    path::PathBuf,
    sync::{Arc, Mutex},
    time::Duration,
};

use swarmy_engine::{Engine, TorrentState, TorrentStatus};
use serde::Serialize;
use tokio::sync::watch;

use crate::{
    auth::Auth,
    push::{Notice, Push},
    stats::{Stats, SystemSample},
    store::{Store, TorrentMeta, now_unix},
};

pub type Shared = Arc<AppState>;

pub struct AppState {
    pub engine: Arc<dyn Engine>,
    pub store: Store,
    pub auth: Auth,
    /// Used when Settings has no default save path.
    pub fallback_download_dir: PathBuf,
    pub data_dir: PathBuf,
    /// Settings fields fixed by command-line flags or env vars.
    pub locked: Vec<&'static str>,
    /// Connection settings the engine was started with (changes need a restart).
    pub startup_connection: crate::store::ConnectionSettings,
    pub policy: crate::policy::PolicyState,
    pub stats: Arc<Stats>,
    pub push: Arc<Push>,
    /// Torrents currently in the error state, so we notify once per failure.
    pub errored: Mutex<HashSet<String>>,
    /// Unix time each torrent last moved data (qBittorrent's `last_activity`).
    pub activity: Mutex<std::collections::HashMap<String, i64>>,
    pub demo: bool,
    /// Latest snapshot, refreshed every second by [`run_ticker`].
    pub snapshot: watch::Receiver<Arc<Snapshot>>,
}

/// What the UI sees for a torrent: engine status merged with Swarmy metadata.
#[derive(Debug, Clone, Serialize)]
pub struct TorrentView {
    pub hash: String,
    pub name: String,
    pub state: TorrentState,
    pub progress: f64,
    pub size: u64,
    pub downloaded: u64,
    pub uploaded: u64,
    pub ratio: f64,
    pub download_speed: u64,
    pub upload_speed: u64,
    pub eta: Option<u64>,
    pub peers: u32,
    pub seeds: u32,
    pub save_path: String,
    pub category: String,
    pub tags: Vec<String>,
    pub added_on: i64,
    pub completed_on: Option<i64>,
    pub has_metadata: bool,
    /// Paused by the queue, waiting for a free slot.
    pub queued: bool,
    pub error: Option<String>,
    /// Content on disk: the torrent's folder, or the file itself.
    pub content_path: String,
    /// Folder holding all the content, if the torrent has one (else empty).
    pub root_path: String,
    pub ratio_limit: f64,
    pub seeding_time_limit: i64,
    pub inactive_seeding_time_limit: i64,
    pub force_start: bool,
    pub priority: i64,
    pub last_activity: i64,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct GlobalStats {
    /// Alternative speed limits are in force (by hand or by schedule).
    pub alt_speed: bool,
    pub download_speed: u64,
    pub upload_speed: u64,
    pub torrents: usize,
    pub downloading: usize,
    pub seeding: usize,
    pub peers: u32,
    pub session_downloaded: u64,
    pub session_uploaded: u64,
    pub system: SystemSample,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct Snapshot {
    pub torrents: Vec<TorrentView>,
    pub stats: GlobalStats,
}

impl TorrentView {
    pub fn new(s: TorrentStatus, meta: &TorrentMeta, last_activity: i64) -> Self {
        let save_path = if meta.save_path.is_empty() {
            s.output_folder.to_string_lossy().into_owned()
        } else {
            meta.save_path.clone()
        };
        let output = s.output_folder.to_string_lossy().into_owned();
        let root_path = if output != save_path { output } else { String::new() };
        let content_path = s.content_path.to_string_lossy().into_owned();
        let ratio = if s.downloaded_bytes > 0 {
            s.uploaded_bytes as f64 / s.downloaded_bytes as f64
        } else {
            0.0
        };
        Self {
            progress: s.progress(),
            name: s.name.clone().unwrap_or_else(|| s.info_hash.clone()),
            hash: s.info_hash,
            state: s.state,
            size: s.total_bytes,
            downloaded: s.downloaded_bytes,
            uploaded: s.uploaded_bytes,
            ratio,
            download_speed: s.download_speed,
            upload_speed: s.upload_speed,
            eta: s.eta_secs,
            peers: s.peers,
            seeds: s.seeds,
            save_path,
            content_path,
            root_path,
            ratio_limit: meta.ratio_limit,
            seeding_time_limit: meta.seeding_time_limit,
            inactive_seeding_time_limit: meta.inactive_seeding_time_limit,
            force_start: meta.force_start,
            priority: meta.priority,
            last_activity,
            category: meta.category.clone(),
            tags: meta.tags.iter().cloned().collect(),
            added_on: meta.added_on,
            completed_on: meta.completed_on,
            has_metadata: s.has_metadata,
            queued: meta.queued && s.state == TorrentState::Paused,
            error: s.error,
        }
    }
}

impl AppState {
    /// Build a fresh snapshot, recording completion times and adopting any
    /// torrents the engine restored that we have no metadata for.
    pub fn build_snapshot(self: &Arc<Self>) -> anyhow::Result<Snapshot> {
        let statuses = self.engine.list();
        let now = now_unix();

        let needs_write = self.store.read(|d| {
            statuses.iter().any(|s| match d.torrents.get(&s.info_hash) {
                None => true,
                Some(m) => s.finished && m.completed_on.is_none(),
            })
        });
        if needs_write {
            let mut completed = Vec::new();
            self.store.update(|d| {
                for s in &statuses {
                    let m = d
                        .torrents
                        .entry(s.info_hash.clone())
                        .or_insert_with(|| TorrentMeta {
                            added_on: now,
                            ..Default::default()
                        });
                    if s.finished && m.completed_on.is_none() {
                        m.completed_on = Some(now);
                        completed.push((s, m.category.clone()));
                    }
                }
            })?;
            for (s, category) in completed {
                let name = s.name.as_deref().unwrap_or(&s.info_hash);
                self.stats
                    .event("completed", &s.info_hash, name, s.total_bytes, &category);
                self.notify("completed", "Download finished", name, &s.info_hash);
            }
        }

        // Newly failed torrents: log and notify once per failure.
        let newly_failed: Vec<&TorrentStatus> = {
            let mut errored = self.errored.lock().unwrap();
            let now_failed: HashSet<&str> = statuses
                .iter()
                .filter(|s| s.state == TorrentState::Error)
                .map(|s| s.info_hash.as_str())
                .collect();
            errored.retain(|h| now_failed.contains(h.as_str()));
            statuses
                .iter()
                .filter(|s| s.state == TorrentState::Error && errored.insert(s.info_hash.clone()))
                .collect()
        };
        for s in newly_failed {
            let name = s.name.as_deref().unwrap_or(&s.info_hash);
            let category = self
                .store
                .read(|d| d.torrents.get(&s.info_hash).map(|m| m.category.clone()))
                .unwrap_or_default();
            self.stats.event("errored", &s.info_hash, name, s.total_bytes, &category);
            let why = s.error.as_deref().unwrap_or("Something went wrong");
            self.notify("errored", &format!("Problem with {name}"), why, &s.info_hash);
        }

        let activity = {
            let mut a = self.activity.lock().unwrap();
            for s in &statuses {
                if s.download_speed > 0 || s.upload_speed > 0 {
                    a.insert(s.info_hash.clone(), now);
                }
            }
            a.retain(|h, _| statuses.iter().any(|s| &s.info_hash == h));
            a.clone()
        };
        let torrents: Vec<TorrentView> = self.store.read(|d| {
            statuses
                .into_iter()
                .map(|s| {
                    let meta = d.torrents.get(&s.info_hash).cloned().unwrap_or_default();
                    let last = activity
                        .get(&s.info_hash)
                        .copied()
                        .unwrap_or_else(|| meta.completed_on.unwrap_or(meta.added_on));
                    TorrentView::new(s, &meta, last)
                })
                .collect()
        });

        let stats = GlobalStats {
            download_speed: torrents.iter().map(|t| t.download_speed).sum(),
            upload_speed: torrents.iter().map(|t| t.upload_speed).sum(),
            torrents: torrents.len(),
            downloading: torrents
                .iter()
                .filter(|t| t.state == TorrentState::Downloading)
                .count(),
            seeding: torrents
                .iter()
                .filter(|t| t.state == TorrentState::Seeding)
                .count(),
            peers: torrents.iter().map(|t| t.peers).sum(),
            ..Default::default()
        };
        Ok(Snapshot { torrents, stats })
    }

    /// Send a push notification for `kind` if the user enabled it. Fire and forget.
    pub fn notify(self: &Arc<Self>, kind: &str, title: &str, body: &str, hash: &str) {
        let (enabled, any) = self.store.read(|d| {
            let p = &d.push.prefs;
            let on = match kind {
                "completed" => p.completed,
                "errored" => p.errored,
                "added" => p.added,
                _ => false,
            };
            (on, !d.push.subscriptions.is_empty())
        });
        if !enabled || !any {
            return;
        }
        let notice = Notice {
            title: title.to_owned(),
            body: body.to_owned(),
            tag: format!("{kind}-{hash}"),
            url: format!("/#/torrents?hash={hash}"),
        };
        let state = self.clone();
        tokio::spawn(async move {
            if let Err(e) = state.push.send(&state.store, &notice, None).await {
                tracing::warn!("push notification failed: {e:#}");
            }
        });
    }

    /// Default save location: Settings > Downloads, else the startup default.
    pub fn download_dir(&self) -> PathBuf {
        let configured = self.store.read(|d| d.settings.downloads.default_save_path.clone());
        if self.locked.contains(&"downloads.default_save_path") || configured.trim().is_empty() {
            self.fallback_download_dir.clone()
        } else {
            PathBuf::from(configured.trim())
        }
    }

    /// Save path for a new torrent: explicit > category path > download dir.
    pub fn resolve_save_path(&self, explicit: Option<&str>, category: &str) -> PathBuf {
        if let Some(p) = explicit.filter(|p| !p.trim().is_empty()) {
            return PathBuf::from(p.trim());
        }
        let base = self.download_dir();
        if category.is_empty() {
            return base;
        }
        self.store.read(|d| match d.categories.get(category) {
            Some(c) if !c.save_path.is_empty() => PathBuf::from(&c.save_path),
            _ => base.join(category),
        })
    }
}

pub async fn run_ticker(state: Shared, tx: watch::Sender<Arc<Snapshot>>) {
    let mut interval = tokio::time::interval(Duration::from_secs(1));
    interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    loop {
        interval.tick().await;
        match state.build_snapshot() {
            Ok(mut s) => {
                state.stats.record(&s.torrents);
                s.stats.alt_speed = crate::policy::apply(&state, &s.torrents).await;
                if let Ok(sum) = state.stats.session_and_system() {
                    s.stats.session_downloaded = sum.0;
                    s.stats.session_uploaded = sum.1;
                    s.stats.system = sum.2;
                }
                let _ = tx.send(Arc::new(s));
            }
            Err(e) => tracing::warn!("snapshot failed: {e:#}"),
        }
    }
}
