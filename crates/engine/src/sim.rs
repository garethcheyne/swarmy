//! A simulated engine for demos and UI development (`--demo`).
//!
//! Nothing touches the network or disk: torrents "download" and "seed" with
//! plausible, noisy speeds, peers come and go, downloads stall and finish.

use std::{
    path::PathBuf,
    sync::{Arc, Mutex},
    time::Duration,
};

use async_trait::async_trait;

use crate::{
    AddOptions, AddOutcome, AddSource, Engine, FileEntry, NetworkInfo, PieceMap, TorrentState,
    TorrentStatus,
};

const MIB: f64 = 1024.0 * 1024.0;
const GIB: f64 = 1024.0 * MIB;

/// A torrent in the demo catalogue, plus the metadata the server should attach.
#[derive(Debug, Clone)]
pub struct DemoSeed {
    pub info_hash: String,
    pub category: &'static str,
    pub tags: &'static [&'static str],
    /// Seconds before "now" the torrent was added.
    pub added_ago: i64,
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum Phase {
    Metadata { left: u32 },
    Checking { left: u32 },
    Downloading,
    Seeding,
    Paused,
    Error,
}

#[derive(Debug)]
struct SimTorrent {
    hash: String,
    name: String,
    total: f64,
    downloaded: f64,
    uploaded: f64,
    phase: Phase,
    /// What this torrent's swarm can deliver when healthy (bytes/s).
    capacity: f64,
    /// How much other peers want what we seed, 0..1.
    popularity: f64,
    dl: f64,
    ul: f64,
    peers: u32,
    seeds: u32,
    /// Seconds remaining in a stall (no peers).
    stall: u32,
    output_folder: PathBuf,
    files: Vec<(String, f64)>,
    error: Option<String>,
}

impl SimTorrent {
    fn status(&self) -> TorrentStatus {
        let state = match self.phase {
            Phase::Metadata { .. } | Phase::Checking { .. } => TorrentState::Initializing,
            Phase::Downloading => TorrentState::Downloading,
            Phase::Seeding => TorrentState::Seeding,
            Phase::Paused => TorrentState::Paused,
            Phase::Error => TorrentState::Error,
        };
        let has_metadata = !matches!(self.phase, Phase::Metadata { .. });
        let dl = self.dl.max(0.0) as u64;
        let remaining = (self.total - self.downloaded).max(0.0) as u64;
        TorrentStatus {
            info_hash: self.hash.clone(),
            name: Some(self.name.clone()),
            state,
            finished: self.downloaded >= self.total,
            has_metadata,
            total_bytes: if has_metadata { self.total as u64 } else { 0 },
            downloaded_bytes: self.downloaded as u64,
            uploaded_bytes: self.uploaded as u64,
            download_speed: dl,
            upload_speed: self.ul.max(0.0) as u64,
            eta_secs: (state == TorrentState::Downloading && dl > 0).then(|| remaining / dl),
            peers: self.peers,
            seeds: self.seeds,
            output_folder: self.output_folder.clone(),
            content_path: match self.files.as_slice() {
                [(only, _)] => self.output_folder.join(only),
                _ => self.output_folder.join(&self.name),
            },
            error: self.error.clone(),
        }
    }

    fn tick(&mut self) {
        let jitter = |v: f64, amount: f64| v * (1.0 + rand::random_range(-amount..amount));
        match self.phase {
            Phase::Metadata { left } => {
                self.peers = rand::random_range(0..3);
                self.phase = if left == 0 {
                    Phase::Downloading
                } else {
                    Phase::Metadata { left: left - 1 }
                };
            }
            Phase::Checking { left } => {
                self.phase = if left == 0 {
                    if self.downloaded >= self.total {
                        Phase::Seeding
                    } else {
                        Phase::Downloading
                    }
                } else {
                    Phase::Checking { left: left - 1 }
                };
            }
            Phase::Downloading => {
                if self.stall > 0 {
                    self.stall -= 1;
                    self.dl *= 0.5;
                    self.peers = self.peers.saturating_sub(1);
                    self.seeds = 0;
                } else {
                    if rand::random_bool(0.004) {
                        self.stall = rand::random_range(15..90);
                    }
                    let target = jitter(self.capacity, 0.35);
                    self.dl += (target - self.dl) * 0.25;
                    let healthy = (self.capacity / MIB * 3.0).clamp(4.0, 60.0) as u32;
                    self.peers = drift(self.peers, healthy);
                    self.seeds = drift(self.seeds, healthy / 2);
                }
                // Leechers upload a little to each other.
                self.ul = jitter(self.dl * 0.08 * self.popularity, 0.5).max(0.0);
                self.downloaded = (self.downloaded + self.dl).min(self.total);
                if self.downloaded >= self.total {
                    self.phase = Phase::Seeding;
                    self.dl = 0.0;
                }
            }
            Phase::Seeding => {
                self.dl = 0.0;
                self.seeds = 0;
                let interest = if rand::random_bool(0.02) { 3.0 } else { 1.0 };
                let target = jitter(self.popularity * 1.6 * MIB * interest, 0.6).max(0.0);
                self.ul += (target - self.ul) * 0.2;
                self.peers = drift(self.peers, (self.popularity * 25.0) as u32);
            }
            Phase::Paused | Phase::Error => {
                self.dl = 0.0;
                self.ul = 0.0;
                self.peers = 0;
                self.seeds = 0;
            }
        }
        self.uploaded += self.ul;
    }
}

pub(crate) fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn drift(current: u32, target: u32) -> u32 {
    let step = rand::random_range(0..=2);
    if current < target {
        current + step
    } else {
        current.saturating_sub(step)
    }
}

/// Deterministic fake info-hash from a name (FNV-1a, stretched to 20 bytes).
fn fake_hash(name: &str) -> String {
    let mut out = String::with_capacity(40);
    for round in 0u64..3 {
        let mut h: u64 = 0xcbf29ce484222325 ^ round.wrapping_mul(0x9e3779b97f4a7c15);
        for b in name.bytes() {
            h ^= b as u64;
            h = h.wrapping_mul(0x100000001b3);
        }
        out.push_str(&format!("{h:016x}"));
    }
    out.truncate(40);
    out
}

fn files_for(name: &str, total: f64) -> Vec<(String, f64)> {
    let lower = name.to_lowercase();
    if lower.ends_with(".iso") || lower.ends_with(".img.xz") {
        return vec![(name.to_owned(), total)];
    }
    if lower.contains(".s0") || lower.contains("1080p") || lower.contains("720p") {
        return vec![
            (format!("{name}/{name}.mkv"), total * 0.985),
            (format!("{name}/{name}.en.srt"), total * 0.0004),
            (format!("{name}/Sample/{name}-sample.mkv"), total * 0.0141),
            (format!("{name}/{name}.nfo"), total * 0.0005),
        ];
    }
    let parts = 6;
    (1..=parts)
        .map(|i| (format!("{name}/part-{i:02}.tar.zst"), total / parts as f64))
        .collect()
}

struct Spec {
    name: &'static str,
    gib: f64,
    progress: f64,
    phase: Phase,
    capacity_mib: f64,
    popularity: f64,
    category: &'static str,
    tags: &'static [&'static str],
    added_hours_ago: f64,
}

const fn spec(
    name: &'static str,
    gib: f64,
    progress: f64,
    phase: Phase,
    capacity_mib: f64,
    popularity: f64,
    category: &'static str,
    tags: &'static [&'static str],
    added_hours_ago: f64,
) -> Spec {
    Spec {
        name,
        gib,
        progress,
        phase,
        capacity_mib,
        popularity,
        category,
        tags,
        added_hours_ago,
    }
}

use Phase::*;

#[rustfmt::skip]
const CATALOGUE: &[Spec] = &[
    spec("ubuntu-26.04-desktop-amd64.iso",          6.1, 0.42, Downloading, 11.0, 0.9, "linux",    &["iso"],           0.3),
    spec("Fedora-Workstation-Live-x86_64-45.iso",    2.3, 0.77, Downloading,  6.5, 0.6, "linux",    &["iso"],           1.2),
    spec("debian-13.2.0-amd64-DVD-1.iso",            3.9, 1.00, Seeding,      0.0, 0.7, "linux",    &["iso"],          26.0),
    spec("archlinux-2026.11.01-x86_64.iso",          1.2, 1.00, Seeding,      0.0, 0.8, "linux",    &["iso"],          70.0),
    spec("linuxmint-23-cinnamon-64bit.iso",          2.9, 0.08, Downloading,  4.2, 0.5, "linux",    &["iso"],           0.1),
    spec("NixOS-26.05-gnome-x86_64.iso",             3.4, 1.00, Paused,       0.0, 0.4, "linux",    &["iso"],         160.0),
    spec("raspios-trixie-arm64-full.img.xz",         1.6, 1.00, Seeding,      0.0, 0.3, "linux",    &["arm"],         300.0),
    spec("Night.of.the.Living.Dead.1968.1080p.BluRay", 7.8, 1.00, Seeding,   0.0, 0.5, "radarr",   &["public-domain"], 48.0),
    spec("Sintel.2010.2160p.WEB",                    4.6, 0.63, Downloading,  8.0, 0.6, "radarr",   &["blender"],       2.5),
    spec("Big.Buck.Bunny.2008.1080p.BluRay",         0.9, 1.00, Seeding,      0.0, 0.9, "radarr",   &["blender"],     500.0),
    spec("Tears.of.Steel.2012.2160p.WEB",            6.3, 0.00, Metadata { left: 40 }, 5.0, 0.4, "radarr", &["blender"], 0.02),
    spec("Cosmos.Laundromat.2015.1080p.WEB",         1.7, 0.91, Downloading,  3.0, 0.5, "radarr",   &["blender"],       5.0),
    spec("The.Cisco.Kid.1950.S01E03.720p.WEB",       0.7, 0.35, Downloading,  2.2, 0.3, "tv-sonarr", &["public-domain"], 0.6),
    spec("The.Cisco.Kid.1950.S01E02.720p.WEB",       0.7, 1.00, Seeding,      0.0, 0.3, "tv-sonarr", &["public-domain"], 30.0),
    spec("Flash.Gordon.1954.S01E07.720p.WEB",        0.6, 0.18, Downloading,  0.0, 0.2, "tv-sonarr", &["public-domain"], 3.0),
    spec("Dick.Tracy.1950.S01E01.720p.WEB",          0.6, 1.00, Seeding,      0.0, 0.2, "tv-sonarr", &["public-domain"], 96.0),
    spec("enwiki-20261101-pages-articles.xml",      24.5, 0.29, Downloading, 14.0, 0.7, "datasets", &["wikipedia"],     4.0),
    spec("openstreetmap-planet-261103.osm.pbf",     82.0, 0.05, Downloading, 16.0, 0.8, "datasets", &["osm"],           0.8),
    spec("common-voice-corpus-21.0-en",             71.3, 1.00, Seeding,      0.0, 0.4, "datasets", &["audio"],       220.0),
    spec("nasa-blue-marble-2026-mosaic",             9.2, 0.54, Paused,       0.0, 0.3, "datasets", &["nasa"],         12.0),
    spec("LibreOffice_26.2.0_Linux_x86-64_deb",      0.3, 1.00, Seeding,      0.0, 0.6, "software", &[],              10.0),
    spec("blender-5.1.0-linux-x64",                  0.4, 0.97, Downloading,  1.5, 0.7, "software", &[],               0.05),
    spec("Internet.Archive.Grateful.Dead.1977-05-08", 2.1, 0.66, Error,       0.0, 0.3, "music",    &["live"],          7.0),
    spec("Free.Music.Archive.fma_full",            106.0, 1.00, Seeding,      0.0, 0.2, "music",    &["dataset"],     400.0),
];

pub struct SimEngine {
    torrents: Arc<Mutex<Vec<SimTorrent>>>,
    /// Global (download, upload) caps in bytes/s, honoured like a real engine.
    limits: Arc<Mutex<(Option<u32>, Option<u32>)>>,
    download_dir: PathBuf,
}

/// Scale every torrent's rate down so the total stays under `cap`.
fn cap_total(ts: &mut [SimTorrent], cap: Option<u32>, get: fn(&mut SimTorrent) -> &mut f64) {
    let Some(cap) = cap else { return };
    let total: f64 = ts.iter_mut().map(|t| *get(t)).sum();
    if total > f64::from(cap) && total > 0.0 {
        let k = f64::from(cap) / total;
        for t in ts.iter_mut() {
            *get(t) *= k;
        }
    }
}

impl SimEngine {
    pub fn start(download_dir: PathBuf) -> (Self, Vec<DemoSeed>) {
        let mut torrents = Vec::new();
        let mut seeds = Vec::new();
        for s in CATALOGUE {
            let total = s.gib * GIB;
            let hash = fake_hash(s.name);
            let downloaded = total * s.progress;
            let ratio_so_far = if s.phase == Seeding || s.progress >= 1.0 {
                s.popularity * 2.5
            } else {
                s.progress * 0.1
            };
            torrents.push(SimTorrent {
                hash: hash.clone(),
                name: s.name.to_owned(),
                total,
                downloaded,
                uploaded: downloaded * ratio_so_far,
                phase: s.phase,
                // Keep the live demo in the same league as the backfilled history.
                capacity: s.capacity_mib * MIB * 0.55,
                popularity: s.popularity,
                dl: 0.0,
                ul: 0.0,
                peers: 0,
                seeds: 0,
                stall: if s.capacity_mib == 0.0 && s.phase == Downloading { u32::MAX } else { 0 },
                output_folder: download_dir.join(s.category),
                files: files_for(s.name, total),
                error: (s.phase == Error)
                    .then(|| "Disk write failed: no space left on device (os error 28)".into()),
            });
            seeds.push(DemoSeed {
                info_hash: hash,
                category: s.category,
                tags: s.tags,
                added_ago: (s.added_hours_ago * 3600.0) as i64,
            });
        }

        let torrents = Arc::new(Mutex::new(torrents));
        let limits = Arc::new(Mutex::new((None, None)));
        let weak = Arc::downgrade(&torrents);
        let lim = limits.clone();
        tokio::spawn(async move {
            let mut iv = tokio::time::interval(Duration::from_secs(1));
            loop {
                iv.tick().await;
                let Some(t) = weak.upgrade() else { return };
                let mut ts = t.lock().unwrap();
                for t in ts.iter_mut() {
                    t.tick();
                }
                // Apply global limits after the swarm "wanted" its rates, and
                // undo the bytes a capped torrent didn't really get.
                let before: Vec<(f64, f64)> = ts.iter().map(|t| (t.dl, t.ul)).collect();
                let (down, up) = *lim.lock().unwrap();
                cap_total(&mut ts, down, |t| &mut t.dl);
                cap_total(&mut ts, up, |t| &mut t.ul);
                for (t, (dl0, ul0)) in ts.iter_mut().zip(before) {
                    t.downloaded = (t.downloaded - (dl0 - t.dl)).max(0.0);
                    t.uploaded = (t.uploaded - (ul0 - t.ul)).max(0.0);
                }
            }
        });

        (
            Self {
                torrents,
                limits,
                download_dir,
            },
            seeds,
        )
    }

    fn with<R>(&self, hash: &str, f: impl FnOnce(&mut SimTorrent) -> R) -> anyhow::Result<R> {
        let mut ts = self.torrents.lock().unwrap();
        let t = ts
            .iter_mut()
            .find(|t| t.hash == hash)
            .ok_or_else(|| anyhow::anyhow!("torrent {hash} not found"))?;
        Ok(f(t))
    }
}

fn magnet_name(url: &str) -> Option<String> {
    let q = url.split_once('?')?.1;
    q.split('&').find_map(|kv| {
        let v = kv.strip_prefix("dn=")?;
        Some(v.replace('+', " ").replace("%20", " "))
    })
}

fn magnet_hash(url: &str) -> Option<String> {
    let i = url.find("urn:btih:")? + 9;
    let h: String = url[i..].chars().take_while(|c| c.is_ascii_alphanumeric()).collect();
    (h.len() == 40).then(|| h.to_lowercase())
}

#[async_trait]
impl Engine for SimEngine {
    async fn add(&self, source: AddSource, opts: AddOptions) -> anyhow::Result<AddOutcome> {
        let (name, hash) = match &source {
            AddSource::Url(u) => {
                let name = magnet_name(u).unwrap_or_else(|| {
                    u.rsplit('/').next().unwrap_or("download").trim_end_matches(".torrent").to_owned()
                });
                let hash = magnet_hash(u).unwrap_or_else(|| fake_hash(u));
                (name, hash)
            }
            AddSource::TorrentFile(b) => {
                let n = format!("uploaded-{:x}", b.len());
                let h = fake_hash(&n);
                (n, h)
            }
        };
        let mut ts = self.torrents.lock().unwrap();
        if ts.iter().any(|t| t.hash == hash) {
            return Ok(AddOutcome { info_hash: hash, already_existed: true });
        }
        let total = rand::random_range(0.3..9.0) * GIB;
        ts.push(SimTorrent {
            hash: hash.clone(),
            files: files_for(&name, total),
            name,
            total,
            downloaded: 0.0,
            uploaded: 0.0,
            phase: if opts.paused { Phase::Paused } else { Phase::Metadata { left: 4 } },
            capacity: rand::random_range(2.0..12.0) * MIB,
            popularity: rand::random_range(0.2..0.9),
            dl: 0.0,
            ul: 0.0,
            peers: 0,
            seeds: 0,
            stall: 0,
            output_folder: opts.save_path.unwrap_or_else(|| self.download_dir.clone()),
            error: None,
        });
        Ok(AddOutcome { info_hash: hash, already_existed: false })
    }

    fn list(&self) -> Vec<TorrentStatus> {
        self.torrents.lock().unwrap().iter().map(SimTorrent::status).collect()
    }

    fn status(&self, hash: &str) -> Option<TorrentStatus> {
        self.with(hash, |t| t.status()).ok()
    }

    fn files(&self, hash: &str) -> anyhow::Result<Vec<FileEntry>> {
        self.with(hash, |t| {
            let mut offset = 0.0;
            t.files
                .iter()
                .enumerate()
                .map(|(index, (path, size))| {
                    let done = (t.downloaded - offset).clamp(0.0, *size);
                    offset += size;
                    FileEntry {
                        index,
                        path: path.clone(),
                        size: *size as u64,
                        downloaded_bytes: done as u64,
                        selected: true,
                    }
                })
                .collect()
        })
    }

    fn pieces(&self, hash: &str, buckets: usize) -> anyhow::Result<PieceMap> {
        self.with(hash, |t| {
            // 4 MiB pieces; which ones we "have" is a stable scatter that fills
            // up as progress rises, roughly like rarest-first in a real swarm.
            let total = ((t.total / (4.0 * MIB)).ceil() as usize).max(1);
            let p = (t.downloaded / t.total).clamp(0.0, 1.0);
            let seed = u64::from_str_radix(&t.hash[..12], 16).unwrap_or(1);
            let bits = (0..total).map(move |i| {
                let mut x = seed ^ (i as u64).wrapping_mul(0x9e3779b97f4a7c15);
                x ^= x >> 33;
                x = x.wrapping_mul(0xff51afd7ed558ccd);
                x ^= x >> 33;
                // Earlier pieces are a little more likely (sequential bias).
                let bias = 0.25 * (1.0 - i as f64 / total as f64);
                ((x % 10_000) as f64 / 10_000.0) < (p + bias * p * (1.0 - p)).min(1.0)
            });
            PieceMap::from_bits(bits, buckets)
        })
    }

    fn network(&self) -> NetworkInfo {
        NetworkInfo {
            listen_port: Some(6881),
            dht_nodes: Some(380 + (crate::sim::now_secs() % 60) as usize),
            bind_interface: Some("wg0".into()),
        }
    }

    async fn pause(&self, hash: &str) -> anyhow::Result<()> {
        self.with(hash, |t| {
            if t.phase != Phase::Error {
                t.phase = Phase::Paused;
            }
        })
    }

    async fn resume(&self, hash: &str) -> anyhow::Result<()> {
        self.with(hash, |t| {
            if matches!(t.phase, Phase::Paused | Phase::Error) {
                t.error = None;
                t.phase = Phase::Checking { left: 3 };
                if t.capacity == 0.0 {
                    t.capacity = 2.0 * MIB;
                    t.stall = 0;
                }
            }
        })
    }

    async fn remove(&self, hash: &str, _delete_files: bool) -> anyhow::Result<()> {
        let mut ts = self.torrents.lock().unwrap();
        let before = ts.len();
        ts.retain(|t| t.hash != hash);
        anyhow::ensure!(ts.len() < before, "torrent {hash} not found");
        Ok(())
    }

    fn set_rate_limits(&self, download: Option<u32>, upload: Option<u32>) {
        *self.limits.lock().unwrap() = (download, upload);
    }

    async fn shutdown(&self) {}
}
