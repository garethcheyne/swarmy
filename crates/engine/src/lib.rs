//! Torrent engine abstraction.
//!
//! The rest of Swarmy only talks to the [`Engine`] trait, so the underlying
//! BitTorrent implementation (librqbit today) can be swapped without touching
//! the HTTP layer or the qBittorrent compatibility shim.

mod relocate;
mod rqbit;
mod sim;

use std::{net::SocketAddr, path::PathBuf};

use async_trait::async_trait;
use serde::Serialize;

pub use rqbit::{RqbitConfig, RqbitEngine};
pub use relocate::same_dir;
pub use sim::{DemoSeed, SimEngine};

/// Lowercase 40-character hex v1 info-hash.
pub type InfoHash = String;

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TorrentState {
    /// Fetching metadata or checking existing files.
    Initializing,
    Downloading,
    Seeding,
    Paused,
    Error,
    /// Its files are moving to another folder.
    Moving,
}

#[derive(Debug, Clone, Serialize)]
pub struct TorrentStatus {
    pub info_hash: InfoHash,
    pub name: Option<String>,
    pub state: TorrentState,
    pub finished: bool,
    pub has_metadata: bool,
    pub total_bytes: u64,
    pub downloaded_bytes: u64,
    pub uploaded_bytes: u64,
    /// Bytes per second.
    pub download_speed: u64,
    /// Bytes per second.
    pub upload_speed: u64,
    pub eta_secs: Option<u64>,
    pub peers: u32,
    pub seeds: u32,
    pub output_folder: PathBuf,
    /// The torrent's content on disk: its folder, or the file for a
    /// single-file torrent (qBittorrent's `content_path`).
    pub content_path: PathBuf,
    pub error: Option<String>,
    /// Set while the torrent's files are being moved.
    pub moving: Option<MoveProgress>,
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
pub struct MoveProgress {
    pub done_bytes: u64,
    pub total_bytes: u64,
}

impl TorrentStatus {
    pub fn progress(&self) -> f64 {
        if self.total_bytes == 0 {
            return 0.0;
        }
        (self.downloaded_bytes as f64 / self.total_bytes as f64).clamp(0.0, 1.0)
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct PeerEntry {
    pub addr: SocketAddr,
    /// The peer's client, from its handshake, when it sent one.
    pub client: Option<String>,
    /// "tcp" or "utp" once connected.
    pub connection: Option<String>,
    /// Bytes we've had from this peer, and sent to it, this session.
    pub downloaded_bytes: u64,
    pub uploaded_bytes: u64,
    /// Engines that know instantaneous rates report them; otherwise the
    /// server derives rates from the byte counters.
    pub download_speed: Option<u64>,
    pub upload_speed: Option<u64>,
}

#[derive(Debug, Clone, Serialize)]
pub struct FileEntry {
    pub index: usize,
    /// Path relative to the torrent's output folder, `/`-separated.
    pub path: String,
    pub size: u64,
    pub downloaded_bytes: u64,
    pub selected: bool,
}

/// Which pieces a torrent has, squeezed into a fixed number of buckets for display.
#[derive(Debug, Clone, Serialize)]
pub struct PieceMap {
    pub total: u32,
    pub have: u32,
    /// Each bucket's fill, 0 (none of its pieces) to 100 (all of them).
    pub buckets: Vec<u8>,
}

impl PieceMap {
    pub fn from_bits(bits: impl ExactSizeIterator<Item = bool>, buckets: usize) -> Self {
        let total = bits.len();
        let n = buckets.clamp(1, total.max(1));
        let mut counts = vec![(0u32, 0u32); n];
        let mut have = 0;
        for (i, b) in bits.enumerate() {
            let slot = i * n / total.max(1);
            counts[slot].1 += 1;
            if b {
                counts[slot].0 += 1;
                have += 1;
            }
        }
        Self {
            total: total as u32,
            have,
            buckets: counts
                .into_iter()
                .map(|(h, t)| if t == 0 { 0 } else { (h * 100 / t) as u8 })
                .collect(),
        }
    }
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct NetworkInfo {
    pub listen_port: Option<u16>,
    pub dht_nodes: Option<usize>,
    pub bind_interface: Option<String>,
}

#[derive(Debug, Clone)]
pub enum AddSource {
    /// A magnet link or an http(s) URL pointing at a .torrent file.
    Url(String),
    /// Raw .torrent file contents.
    TorrentFile(Vec<u8>),
}

/// qBittorrent's content layouts.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum ContentLayout {
    /// Multi-file torrents get a folder named after the torrent; single files don't.
    #[default]
    Original,
    /// Always create a folder, even for a single file.
    Subfolder,
    /// Never create a folder.
    NoSubfolder,
}

impl ContentLayout {
    /// Where the torrent's files go, given the save path and its file count.
    pub fn folder(self, save: &std::path::Path, name: &str, files: usize) -> PathBuf {
        let safe = !name.is_empty()
            && std::path::Path::new(name)
                .components()
                .all(|c| matches!(c, std::path::Component::Normal(_)));
        match (self, files > 1, safe) {
            (_, _, false) | (Self::NoSubfolder, _, _) | (Self::Original, false, _) => save.to_path_buf(),
            (Self::Original, true, _) | (Self::Subfolder, true, _) => save.join(name),
            (Self::Subfolder, false, _) => {
                let stem = std::path::Path::new(name)
                    .file_stem()
                    .map(|s| s.to_string_lossy().into_owned())
                    .unwrap_or_else(|| name.to_owned());
                save.join(stem)
            }
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct AddOptions {
    pub save_path: Option<PathBuf>,
    pub paused: bool,
    pub layout: ContentLayout,
}

#[derive(Debug, Clone)]
pub struct AddOutcome {
    pub info_hash: InfoHash,
    pub already_existed: bool,
}

#[async_trait]
pub trait Engine: Send + Sync + 'static {
    async fn add(&self, source: AddSource, opts: AddOptions) -> anyhow::Result<AddOutcome>;
    fn list(&self) -> Vec<TorrentStatus>;
    fn status(&self, hash: &str) -> Option<TorrentStatus>;
    fn files(&self, hash: &str) -> anyhow::Result<Vec<FileEntry>>;
    fn pieces(&self, hash: &str, buckets: usize) -> anyhow::Result<PieceMap>;
    /// Connected peers. Empty for torrents that aren't running.
    fn peers(&self, hash: &str) -> anyhow::Result<Vec<PeerEntry>>;
    fn network(&self) -> NetworkInfo;
    async fn pause(&self, hash: &str) -> anyhow::Result<()>;
    async fn resume(&self, hash: &str) -> anyhow::Result<()>;
    /// Remove the torrent. During a move this takes effect once the move ends.
    async fn remove(&self, hash: &str, delete_files: bool) -> anyhow::Result<()>;
    /// Move the torrent's files so they live in `output_folder`, and carry on
    /// from there without re-downloading. Returns when the move is done; the
    /// torrent reports [`TorrentState::Moving`] meanwhile. Refuses to
    /// overwrite anything, and on failure leaves the torrent where it was.
    async fn relocate(&self, hash: &str, output_folder: PathBuf) -> anyhow::Result<()>;
    /// Global limits in bytes per second; `None` means unlimited.
    fn set_rate_limits(&self, download: Option<u32>, upload: Option<u32>);
    async fn shutdown(&self);
}
