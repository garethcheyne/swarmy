//! Torrent engine abstraction.
//!
//! The rest of Swarmy only talks to the [`Engine`] trait, so the underlying
//! BitTorrent implementation (librqbit today) can be swapped without touching
//! the HTTP layer or the qBittorrent compatibility shim.

mod rqbit;
mod sim;

use std::path::PathBuf;

use async_trait::async_trait;
use serde::Serialize;

pub use rqbit::{RqbitConfig, RqbitEngine};
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
    pub error: Option<String>,
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

#[derive(Debug, Clone, Default)]
pub struct AddOptions {
    pub save_path: Option<PathBuf>,
    pub paused: bool,
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
    fn network(&self) -> NetworkInfo;
    async fn pause(&self, hash: &str) -> anyhow::Result<()>;
    async fn resume(&self, hash: &str) -> anyhow::Result<()>;
    async fn remove(&self, hash: &str, delete_files: bool) -> anyhow::Result<()>;
    /// Global limits in bytes per second; `None` means unlimited.
    fn set_rate_limits(&self, download: Option<u32>, upload: Option<u32>);
    async fn shutdown(&self);
}
