use std::{
    net::{Ipv4Addr, Ipv6Addr, SocketAddr},
    num::NonZeroU32,
    path::PathBuf,
    str::FromStr,
    sync::Arc,
};

use anyhow::Context;
use async_trait::async_trait;
use librqbit::{
    AddTorrent, AddTorrentOptions, AddTorrentResponse, ListenerMode, ListenerOptions,
    ManagedTorrent, Session, SessionOptions, SessionPersistenceConfig, TorrentStatsState,
    api::TorrentIdOrHash, dht::Id20,
};

use crate::{
    AddOptions, AddOutcome, AddSource, Engine, FileEntry, NetworkInfo, PieceMap, TorrentState,
    TorrentStatus,
};

#[derive(Debug, Clone)]
pub struct RqbitConfig {
    /// Where torrents go when no save path is given.
    pub default_download_dir: PathBuf,
    /// Where librqbit keeps its session/fastresume state.
    pub state_dir: PathBuf,
    /// TCP + uTP listen port for incoming peers.
    pub listen_port: u16,
    pub enable_upnp: bool,
    /// Bind all torrent traffic to this interface (e.g. `wg0`, `tun0`).
    /// On Linux this uses SO_BINDTODEVICE, so traffic cannot leak if the VPN drops.
    pub bind_interface: Option<String>,
    pub ipv4_only: bool,
    pub dht: bool,
    pub lsd: bool,
    /// Max peers per torrent; None = engine default.
    pub peer_limit: Option<usize>,
    /// socks5://[user:pass@]host:port for outgoing peer connections.
    pub socks_proxy: Option<String>,
    /// URL of an IP blocklist (P2P/DAT format, optionally gzipped).
    pub blocklist_url: Option<String>,
}

pub struct RqbitEngine {
    session: Arc<Session>,
    api: librqbit::Api,
    bind_interface: Option<String>,
}

impl RqbitEngine {
    pub async fn start(cfg: RqbitConfig) -> anyhow::Result<Self> {
        std::fs::create_dir_all(&cfg.default_download_dir)
            .with_context(|| format!("creating {}", cfg.default_download_dir.display()))?;
        std::fs::create_dir_all(&cfg.state_dir)
            .with_context(|| format!("creating {}", cfg.state_dir.display()))?;

        let listen_addr: SocketAddr = if cfg.ipv4_only {
            (Ipv4Addr::UNSPECIFIED, cfg.listen_port).into()
        } else {
            (Ipv6Addr::UNSPECIFIED, cfg.listen_port).into()
        };

        let opts = SessionOptions {
            dht: cfg.dht.then(Default::default),
            disable_local_service_discovery: !cfg.lsd,
            peer_limit: cfg.peer_limit,
            blocklist_url: cfg.blocklist_url.clone(),
            connect: cfg.socks_proxy.clone().map(|proxy_url| librqbit::ConnectionOptions {
                proxy_url: Some(proxy_url),
                ..Default::default()
            }),
            bind_device_name: cfg.bind_interface.clone(),
            fastresume: true,
            persistence: Some(SessionPersistenceConfig::Json {
                folder: Some(cfg.state_dir.clone()),
            }),
            listen: Some(ListenerOptions {
                mode: ListenerMode::TcpAndUtp,
                listen_addr,
                enable_upnp_port_forwarding: cfg.enable_upnp,
                ipv4_only: cfg.ipv4_only,
                ..Default::default()
            }),
            ipv4_only: cfg.ipv4_only,
            client_name_and_version: Some(format!("Swarmy {}", env!("CARGO_PKG_VERSION"))),
            ..Default::default()
        };

        let session = Session::new_with_opts(cfg.default_download_dir.clone(), opts)
            .await
            .context("starting torrent session")?;
        let api = librqbit::Api::new(session.clone(), None);
        Ok(Self {
            session,
            api,
            bind_interface: cfg.bind_interface,
        })
    }

    fn handle(&self, hash: &str) -> anyhow::Result<Arc<ManagedTorrent>> {
        let id = parse_hash(hash)?;
        self.session
            .get(TorrentIdOrHash::Hash(id))
            .with_context(|| format!("torrent {hash} not found"))
    }
}

fn parse_hash(hash: &str) -> anyhow::Result<Id20> {
    Id20::from_str(hash).with_context(|| format!("invalid info-hash {hash:?}"))
}

fn status_of(t: &ManagedTorrent) -> TorrentStatus {
    let stats = t.stats();
    let live = stats.live.as_ref();

    let state = match stats.state {
        TorrentStatsState::Error => TorrentState::Error,
        TorrentStatsState::Paused => TorrentState::Paused,
        TorrentStatsState::Initializing { .. } => TorrentState::Initializing,
        TorrentStatsState::Live if stats.finished => TorrentState::Seeding,
        TorrentStatsState::Live => TorrentState::Downloading,
    };

    let to_bps = |mib_per_sec: f64| (mib_per_sec * 1024.0 * 1024.0).max(0.0) as u64;
    let download_speed = live.map(|l| to_bps(l.download_speed.mbps)).unwrap_or(0);
    let upload_speed = live.map(|l| to_bps(l.upload_speed.mbps)).unwrap_or(0);

    let remaining = stats.total_bytes.saturating_sub(stats.progress_bytes);
    let eta_secs = match (state, download_speed) {
        (TorrentState::Downloading, s) if s > 0 => Some(remaining / s),
        _ => None,
    };

    let (peers, seeds) = live
        .map(|l| {
            let p = &l.snapshot.peer_stats;
            (p.live, p.live_seeders)
        })
        .unwrap_or((0, 0));

    TorrentStatus {
        info_hash: t.info_hash().as_string(),
        name: t.name(),
        state,
        finished: stats.finished,
        has_metadata: t.with_metadata(|_| ()).is_ok(),
        total_bytes: stats.total_bytes,
        downloaded_bytes: stats.progress_bytes,
        uploaded_bytes: stats.uploaded_bytes,
        download_speed,
        upload_speed,
        eta_secs,
        peers,
        seeds,
        output_folder: t.output_folder().to_path_buf(),
        error: stats.error,
    }
}

#[async_trait]
impl Engine for RqbitEngine {
    async fn add(&self, source: AddSource, opts: AddOptions) -> anyhow::Result<AddOutcome> {
        let add = match source {
            AddSource::Url(url) => AddTorrent::from_url(url),
            AddSource::TorrentFile(bytes) => AddTorrent::from_bytes(bytes),
        };
        let add_opts = AddTorrentOptions {
            paused: opts.paused,
            // Re-use existing data (re-adding a torrent, or Sonarr retrying a grab).
            overwrite: true,
            output_folder: opts.save_path.map(|p| p.to_string_lossy().into_owned()),
            ..Default::default()
        };

        let resp = self
            .session
            .add_torrent(add, Some(add_opts))
            .await
            .context("adding torrent")?;

        let (handle, already_existed) = match resp {
            AddTorrentResponse::Added(_, h) => (h, false),
            AddTorrentResponse::AlreadyManaged(_, h) => (h, true),
            AddTorrentResponse::ListOnly(_) => anyhow::bail!("unexpected list-only response"),
        };
        Ok(AddOutcome {
            info_hash: handle.info_hash().as_string(),
            already_existed,
        })
    }

    fn list(&self) -> Vec<TorrentStatus> {
        self.session
            .with_torrents(|iter| iter.map(|(_, t)| status_of(t)).collect())
    }

    fn status(&self, hash: &str) -> Option<TorrentStatus> {
        self.handle(hash).ok().map(|t| status_of(&t))
    }

    fn files(&self, hash: &str) -> anyhow::Result<Vec<FileEntry>> {
        let t = self.handle(hash)?;
        let progress = t.stats().file_progress;
        let only = t.only_files();
        t.with_metadata(|m| {
            m.file_infos
                .iter()
                .enumerate()
                .filter(|(_, f)| !f.attrs.padding)
                .map(|(index, f)| FileEntry {
                    index,
                    path: f
                        .relative_filename
                        .components()
                        .map(|c| c.as_os_str().to_string_lossy())
                        .collect::<Vec<_>>()
                        .join("/"),
                    size: f.len,
                    downloaded_bytes: progress.get(index).copied().unwrap_or(0),
                    selected: only.as_ref().is_none_or(|o| o.contains(&index)),
                })
                .collect()
        })
    }

    fn pieces(&self, hash: &str, buckets: usize) -> anyhow::Result<PieceMap> {
        let id = parse_hash(hash)?;
        let (bf, len) = self
            .api
            .api_dump_haves(TorrentIdOrHash::Hash(id))
            .map_err(|e| anyhow::anyhow!("{e}"))?;
        let len = (len as usize).min(bf.len());
        Ok(PieceMap::from_bits((0..len).map(|i| bf[i]), buckets))
    }

    fn network(&self) -> NetworkInfo {
        NetworkInfo {
            listen_port: self.session.announce_port(),
            dht_nodes: self.session.get_dht().map(|d| {
                let s = d.stats();
                s.routing_table_size + s.routing_table_size_v6
            }),
            bind_interface: self.bind_interface.clone(),
        }
    }

    async fn pause(&self, hash: &str) -> anyhow::Result<()> {
        let t = self.handle(hash)?;
        if t.is_paused() {
            return Ok(());
        }
        self.session.pause(&t).await
    }

    async fn resume(&self, hash: &str) -> anyhow::Result<()> {
        let t = self.handle(hash)?;
        if !t.is_paused() {
            return Ok(());
        }
        self.session.unpause(&t).await
    }

    async fn remove(&self, hash: &str, delete_files: bool) -> anyhow::Result<()> {
        let id = parse_hash(hash)?;
        self.session
            .delete(TorrentIdOrHash::Hash(id), delete_files)
            .await
    }

    fn set_rate_limits(&self, download: Option<u32>, upload: Option<u32>) {
        let limits = &self.session.ratelimits;
        limits.set_download_bps(download.and_then(NonZeroU32::new));
        limits.set_upload_bps(upload.and_then(NonZeroU32::new));
    }

    async fn shutdown(&self) {
        self.session.stop().await;
    }
}
