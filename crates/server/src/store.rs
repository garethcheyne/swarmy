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

#[derive(Debug, Clone, Serialize, Deserialize)]
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
    /// The save path the torrent was added with (its content may be in a subfolder).
    #[serde(default)]
    pub save_path: String,
    /// qBittorrent share limits: -2 = use the global setting, -1 = no limit.
    #[serde(default = "use_global_f")]
    pub ratio_limit: f64,
    /// Minutes; -2 = global, -1 = no limit.
    #[serde(default = "use_global")]
    pub seeding_time_limit: i64,
    /// Minutes without activity; -2 = global, -1 = no limit.
    #[serde(default = "use_global")]
    pub inactive_seeding_time_limit: i64,
    /// Ignore queue limits for this torrent.
    #[serde(default)]
    pub force_start: bool,
    /// Queue order; lower starts first. 0 = by date added.
    #[serde(default)]
    pub priority: i64,
    /// While downloading in the incomplete folder: the folder it's in there.
    /// Empty once the torrent is in its save path.
    #[serde(default)]
    pub download_path: String,
    /// Follows its category's folder (qBittorrent's Automatic Torrent
    /// Management): set when added without a folder of its own.
    #[serde(default)]
    pub auto_managed: bool,
    /// Why the last move failed, until a move succeeds.
    #[serde(default)]
    pub move_error: Option<String>,
}

fn use_global() -> i64 {
    -2
}
fn use_global_f() -> f64 {
    -2.0
}

impl Default for TorrentMeta {
    fn default() -> Self {
        Self {
            category: String::new(),
            tags: Default::default(),
            added_on: 0,
            completed_on: None,
            queued: false,
            save_path: String::new(),
            ratio_limit: -2.0,
            seeding_time_limit: -2,
            inactive_seeding_time_limit: -2,
            force_start: false,
            priority: 0,
            download_path: String::new(),
            auto_managed: false,
            move_error: None,
        }
    }
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
    pub peer_map: PeerMapSettings,
    pub usenet: UsenetSettings,
    pub updates: UpdateSettings,
}

/// New Swarmy releases.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct UpdateSettings {
    /// Check GitHub for new releases every few hours.
    pub check: bool,
}

impl Default for UpdateSettings {
    fn default() -> Self {
        Self { check: true }
    }
}

/// Usenet downloading (NZB), SABnzbd-compatible.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct UsenetSettings {
    /// Passwords are blanked when settings are read; an empty password on
    /// save keeps the stored one for that server.
    pub servers: Vec<swarmy_usenet::Server>,
    /// Empty: `<download folder>/usenet`. Finished jobs go into
    /// `<this>/<category>/<name>`.
    pub complete_dir: String,
    /// Empty: `<download folder>/usenet/incomplete`.
    pub incomplete_dir: String,
    /// KiB/s; 0 = no limit.
    pub speed_limit: u64,
    pub paused: bool,
    /// Answer SABnzbd's API at /api and /sabnzbd/api (for Sonarr and Radarr).
    pub sab_api: bool,
}

impl Default for UsenetSettings {
    fn default() -> Self {
        Self {
            servers: Vec::new(),
            complete_dir: String::new(),
            incomplete_dir: String::new(),
            speed_limit: 0,
            paused: false,
            sab_api: true,
        }
    }
}

/// The swarm map.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct PeerMapSettings {
    /// Download and use the IP location database (and keep it up to date).
    pub locations: bool,
    /// Where this Swarmy is, for drawing flows; `None` lets the browser guess
    /// from its time zone.
    pub home: Option<Home>,
}

impl Default for PeerMapSettings {
    fn default() -> Self {
        Self { locations: true, home: None }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct Home {
    pub lat: f64,
    pub lon: f64,
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
            peer_map: PeerMapSettings::default(),
            usenet: UsenetSettings::default(),
            updates: UpdateSettings::default(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct DownloadSettings {
    /// Empty = the --download-dir / platform default.
    pub default_save_path: String,
    pub add_paused: bool,
    /// Download into a separate folder and move to the save path when complete.
    pub use_incomplete_dir: bool,
    /// Empty = "<download folder>/incomplete".
    pub incomplete_dir: String,
    /// Changing a torrent's category, or a category's folder, moves the files
    /// of torrents that follow their category.
    pub category_moves: bool,
}

impl Default for DownloadSettings {
    fn default() -> Self {
        Self {
            default_save_path: String::new(),
            add_paused: false,
            use_incomplete_dir: false,
            incomplete_dir: String::new(),
            category_moves: true,
        }
    }
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
    /// Minutes without any upload or download; None = no limit.
    pub inactive_time_limit: Option<u32>,
    pub action: SeedAction,
}

impl Default for SeedingSettings {
    fn default() -> Self {
        Self {
            ratio_limit: None,
            time_limit: None,
            inactive_time_limit: None,
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
    /// Serve the qBittorrent-compatible API at /api/v2 (Sonarr, Radarr, Prowlarr…).
    pub qbit_api: bool,
    /// The single API key from before keys had names (SHA-256 hex). Moved
    /// into `StoreData::api_keys` when Swarmy starts; read only for that.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub api_key_hash: String,
}

impl Default for WebUiSettings {
    fn default() -> Self {
        Self {
            session_days: 7,
            max_attempts: 5,
            lockout_minutes: 15,
            trusted_proxies: vec!["127.0.0.1".into(), "::1".into()],
            qbit_api: true,
            api_key_hash: String::new(),
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
    /// Usenet jobs still in the queue, restored on start (their NZBs are
    /// kept in each job's incomplete folder).
    #[serde(default)]
    pub usenet_queue: Vec<QueuedJob>,
    /// Finished Usenet jobs, newest first.
    #[serde(default)]
    pub usenet_history: Vec<swarmy_usenet::JobView>,
    /// API keys for Sonarr, Radarr and the like.
    #[serde(default)]
    pub api_keys: Vec<crate::keys::ApiKey>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct QueuedJob {
    pub id: String,
    pub name: String,
    pub category: String,
    pub priority: swarmy_usenet::Priority,
    pub paused: bool,
}

pub struct Store {
    path: PathBuf,
    data: Mutex<StoreData>,
}

impl Store {
    pub fn open(path: PathBuf) -> anyhow::Result<Self> {
        let mut data: StoreData = match std::fs::read(&path) {
            Ok(bytes) => serde_json::from_slice(&bytes)
                .with_context(|| format!("parsing {}", path.display()))?,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => StoreData::default(),
            Err(e) => return Err(e).with_context(|| format!("reading {}", path.display())),
        };
        if crate::keys::migrate(&mut data) {
            write_atomic(&path, &serde_json::to_vec_pretty(&data)?)?;
        }
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
