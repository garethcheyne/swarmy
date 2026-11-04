//! Swarmy's own persistent state: account, categories and per-torrent metadata
//! (category, tags, timestamps) that the torrent engine doesn't track itself.
//!
//! Stored as a single JSON file, written atomically (temp file + rename).

use std::{
    collections::{BTreeMap, BTreeSet},
    path::{Path, PathBuf},
    sync::Mutex,
};

use anyhow::Context;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Account {
    pub username: String,
    /// Argon2 PHC string.
    pub password_hash: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Category {
    /// Empty means "<download dir>/<category name>".
    #[serde(default)]
    pub save_path: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TorrentMeta {
    #[serde(default)]
    pub category: String,
    #[serde(default)]
    pub tags: BTreeSet<String>,
    /// Unix seconds.
    pub added_on: i64,
    /// Unix seconds; set the first time the torrent is seen finished.
    #[serde(default)]
    pub completed_on: Option<i64>,
    /// Paused by the queue (not by the user); resumed automatically when a slot frees up.
    #[serde(default)]
    pub queued: bool,
}

/// Everything on the Settings page. Missing fields take defaults, so older
/// files keep working as settings are added.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub downloads: DownloadSettings,
    pub connection: ConnectionSettings,
    pub speed: SpeedSettings,
    pub queue: QueueSettings,
    pub seeding: SeedingSettings,
    pub webui: WebUiSettings,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            downloads: DownloadSettings::default(),
            connection: ConnectionSettings::default(),
            speed: SpeedSettings::default(),
            queue: QueueSettings::default(),
            seeding: SeedingSettings::default(),
            webui: WebUiSettings::default(),
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct DownloadSettings {
    /// Empty = the --download-dir / platform default.
    pub default_save_path: String,
    pub add_paused: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct ConnectionSettings {
    pub listen_port: u16,
    pub upnp: bool,
    /// Empty = don't bind.
    pub bind_interface: String,
    pub ipv4_only: bool,
    pub dht: bool,
    pub lsd: bool,
    /// 0 = engine default.
    pub peer_limit: u32,
    /// socks5://[user:pass@]host:port, empty = none.
    pub socks_proxy: String,
    /// URL of a P2P/DAT blocklist, empty = none.
    pub blocklist_url: String,
}

impl Default for ConnectionSettings {
    fn default() -> Self {
        Self {
            listen_port: 6881,
            upnp: false,
            bind_interface: String::new(),
            ipv4_only: false,
            dht: true,
            lsd: true,
            peer_limit: 0,
            socks_proxy: String::new(),
            blocklist_url: String::new(),
        }
    }
}

/// Speeds in KiB/s; 0 = unlimited.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct SpeedSettings {
    pub download_limit: u32,
    pub upload_limit: u32,
    pub alt_download_limit: u32,
    pub alt_upload_limit: u32,
    /// Alternative limits switched on by hand.
    pub alt_enabled: bool,
    pub schedule: Schedule,
}

impl Default for SpeedSettings {
    fn default() -> Self {
        Self {
            download_limit: 0,
            upload_limit: 0,
            alt_download_limit: 2048,
            alt_upload_limit: 512,
            alt_enabled: false,
            schedule: Schedule::default(),
        }
    }
}

/// When alternative limits switch on automatically (server local time).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Schedule {
    pub enabled: bool,
    /// Minutes after midnight.
    pub from: u16,
    pub to: u16,
    /// 0 = Monday … 6 = Sunday.
    pub days: Vec<u8>,
}

impl Default for Schedule {
    fn default() -> Self {
        Self {
            enabled: false,
            from: 8 * 60,
            to: 18 * 60,
            days: vec![0, 1, 2, 3, 4],
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct QueueSettings {
    pub enabled: bool,
    pub max_downloads: u32,
    pub max_uploads: u32,
    pub max_active: u32,
}

impl Default for QueueSettings {
    fn default() -> Self {
        Self {
            enabled: false,
            max_downloads: 3,
            max_uploads: 5,
            max_active: 8,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SeedAction {
    Pause,
    Remove,
    RemoveWithFiles,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct SeedingSettings {
    /// None = no ratio limit.
    pub ratio_limit: Option<f64>,
    /// Minutes; None = no time limit.
    pub time_limit: Option<u32>,
    pub action: SeedAction,
}

impl Default for SeedingSettings {
    fn default() -> Self {
        Self {
            ratio_limit: None,
            time_limit: None,
            action: SeedAction::Pause,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct WebUiSettings {
    pub session_days: u32,
    /// Failed logins from one address before it's locked out; 0 = never.
    pub max_attempts: u32,
    pub lockout_minutes: u32,
    /// Addresses (or IPv4 CIDRs) allowed to set X-Forwarded-For / -Proto.
    pub trusted_proxies: Vec<String>,
}

impl Default for WebUiSettings {
    fn default() -> Self {
        Self {
            session_days: 7,
            max_attempts: 5,
            lockout_minutes: 15,
            trusted_proxies: vec!["127.0.0.1".into(), "::1".into()],
        }
    }
}

/// A login session, persisted so restarts don't sign everyone out.
/// Only a SHA-256 of the token is stored.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StoredSession {
    pub expires: i64,
    pub created: i64,
    #[serde(default)]
    pub client: String,
}

/// A browser/device that asked for push notifications.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PushSubscription {
    pub endpoint: String,
    pub p256dh: String,
    pub auth: String,
    #[serde(default)]
    pub label: String,
    pub created: i64,
}

/// Which events send a push notification.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NotifyPrefs {
    #[serde(default = "yes")]
    pub completed: bool,
    #[serde(default = "yes")]
    pub errored: bool,
    #[serde(default)]
    pub added: bool,
}

fn yes() -> bool {
    true
}

impl Default for NotifyPrefs {
    fn default() -> Self {
        Self {
            completed: true,
            errored: true,
            added: false,
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PushData {
    /// VAPID private key: raw P-256 scalar, base64url without padding.
    #[serde(default)]
    pub vapid_private: String,
    #[serde(default)]
    pub subscriptions: Vec<PushSubscription>,
    #[serde(default)]
    pub prefs: NotifyPrefs,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct StoreData {
    #[serde(default)]
    pub settings: Settings,
    /// sha256(token) hex -> session.
    #[serde(default)]
    pub sessions: BTreeMap<String, StoredSession>,
    #[serde(default)]
    pub push: PushData,
    #[serde(default)]
    pub account: Option<Account>,
    #[serde(default)]
    pub categories: BTreeMap<String, Category>,
    #[serde(default)]
    pub tags: BTreeSet<String>,
    #[serde(default)]
    pub torrents: BTreeMap<String, TorrentMeta>,
}

pub struct Store {
    path: PathBuf,
    data: Mutex<StoreData>,
}

impl Store {
    pub fn open(path: PathBuf) -> anyhow::Result<Self> {
        let data = match std::fs::read(&path) {
            Ok(bytes) => serde_json::from_slice(&bytes)
                .with_context(|| format!("parsing {}", path.display()))?,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => StoreData::default(),
            Err(e) => return Err(e).with_context(|| format!("reading {}", path.display())),
        };
        Ok(Self {
            path,
            data: Mutex::new(data),
        })
    }

    /// Read-only access.
    pub fn read<R>(&self, f: impl FnOnce(&StoreData) -> R) -> R {
        f(&self.data.lock().unwrap())
    }

    /// Mutate and persist. The closure returns whether anything changed.
    pub fn update<R>(&self, f: impl FnOnce(&mut StoreData) -> R) -> anyhow::Result<R> {
        let mut data = self.data.lock().unwrap();
        let r = f(&mut data);
        write_atomic(&self.path, &serde_json::to_vec_pretty(&*data)?)?;
        Ok(r)
    }
}

fn write_atomic(path: &Path, bytes: &[u8]) -> anyhow::Result<()> {
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, bytes).with_context(|| format!("writing {}", tmp.display()))?;
    std::fs::rename(&tmp, path).with_context(|| format!("replacing {}", path.display()))?;
    Ok(())
}

pub fn now_unix() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}
