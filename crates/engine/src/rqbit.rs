use std::{
    collections::HashMap,
    net::{Ipv4Addr, Ipv6Addr, SocketAddr},
    num::NonZeroU32,
    path::PathBuf,
    str::FromStr,
    sync::{Arc, Mutex},
};

use anyhow::Context;
use async_trait::async_trait;
use librqbit::{
    AddTorrent, AddTorrentOptions, AddTorrentResponse, ListenerMode, ListenerOptions, Magnet,
    ManagedTorrent, Session, SessionOptions, SessionPersistenceConfig, TorrentStatsState,
    api::TorrentIdOrHash, dht::Id20,
};

use crate::{
    AddOptions, AddOutcome, AddSource, ContentLayout, Engine, FileEntry, NetworkInfo, PieceMap,
    TorrentState, TorrentStatus,
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

/// A magnet whose metadata is still being fetched. librqbit only returns
/// from `add_torrent` once metadata arrives, which can take minutes; clients
/// like Sonarr expect `add` to return at once, so we track it here meanwhile.
#[derive(Debug, Clone)]
struct Pending {
    name: Option<String>,
    save: PathBuf,
    paused: bool,
    cancelled: bool,
    error: Option<String>,
}

pub struct RqbitEngine {
    session: Arc<Session>,
    api: librqbit::Api,
    bind_interface: Option<String>,
    default_dir: PathBuf,
    pending: Arc<Mutex<HashMap<String, Pending>>>,
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
            default_dir: cfg.default_download_dir,
            pending: Default::default(),
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

/// Resolve metadata without starting a download, pick the folder for the
/// content layout, then add for real. Returns the info-hash.
async fn resolve_and_add(
    session: &Arc<Session>,
    add: AddTorrent<'static>,
    save: PathBuf,
    layout: ContentLayout,
    paused: impl Fn() -> bool,
) -> anyhow::Result<(String, bool)> {
    let listed = session
        .add_torrent(
            add,
            Some(AddTorrentOptions {
                list_only: true,
                ..Default::default()
            }),
        )
        .await
        .context("reading torrent metadata")?;
    let info = match listed {
        AddTorrentResponse::ListOnly(l) => l,
        AddTorrentResponse::AlreadyManaged(_, h) | AddTorrentResponse::Added(_, h) => {
            return Ok((h.info_hash().as_string(), true));
        }
    };
    let name = info.info.name().map(|n| n.into_owned()).unwrap_or_default();
    let files = info.info.iter_file_details().count();
    let folder = layout.folder(&save, &name, files);

    let resp = session
        .add_torrent(
            AddTorrent::from_bytes(info.torrent_bytes.clone()),
            Some(AddTorrentOptions {
                paused: paused(),
                // Re-use existing data (re-adding a torrent, or Sonarr retrying a grab).
                overwrite: true,
                output_folder: Some(folder.to_string_lossy().into_owned()),
                initial_peers: Some(info.seen_peers.clone()),
                ..Default::default()
            }),
        )
        .await
        .context("adding torrent")?;
    match resp {
        AddTorrentResponse::Added(_, h) => Ok((h.info_hash().as_string(), false)),
        AddTorrentResponse::AlreadyManaged(_, h) => Ok((h.info_hash().as_string(), true)),
        AddTorrentResponse::ListOnly(_) => anyhow::bail!("unexpected list-only response"),
    }
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

    let output_folder = t.output_folder().to_path_buf();
    // A single-file torrent's content is the file itself.
    let content_path = t
        .with_metadata(|m| {
            let real: Vec<_> = m.file_infos.iter().filter(|f| !f.attrs.padding).collect();
            match real.as_slice() {
                [only] => output_folder.join(&only.relative_filename),
                _ => output_folder.clone(),
            }
        })
        .unwrap_or_else(|_| output_folder.clone());

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
        output_folder,
        content_path,
        error: stats.error,
    }
}

fn pending_status(hash: &str, p: &Pending) -> TorrentStatus {
    TorrentStatus {
        info_hash: hash.to_owned(),
        name: p.name.clone(),
        state: if p.error.is_some() {
            TorrentState::Error
        } else {
            TorrentState::Initializing
        },
        finished: false,
        has_metadata: false,
        total_bytes: 0,
        downloaded_bytes: 0,
        uploaded_bytes: 0,
        download_speed: 0,
        upload_speed: 0,
        eta_secs: None,
        peers: 0,
        seeds: 0,
        output_folder: p.save.clone(),
        content_path: p.save.clone(),
        error: p.error.clone(),
    }
}

#[async_trait]
impl Engine for RqbitEngine {
    async fn add(&self, source: AddSource, opts: AddOptions) -> anyhow::Result<AddOutcome> {
        let save = opts.save_path.clone().unwrap_or_else(|| self.default_dir.clone());

        // Magnets: answer at once with the info-hash, fetch metadata in the background.
        if let AddSource::Url(url) = &source
            && url.starts_with("magnet:")
        {
            let magnet = Magnet::parse(url).context("that isn't a valid magnet link")?;
            let id = magnet.as_id20().context("the magnet link has no BitTorrent v1 info-hash")?;
            let hash = id.as_string();
            if self.session.get(TorrentIdOrHash::Hash(id)).is_some()
                || self.pending.lock().unwrap().contains_key(&hash)
            {
                return Ok(AddOutcome { info_hash: hash, already_existed: true });
            }
            self.pending.lock().unwrap().insert(
                hash.clone(),
                Pending {
                    name: magnet.name.clone(),
                    save: save.clone(),
                    paused: opts.paused,
                    cancelled: false,
                    error: None,
                },
            );

            let session = self.session.clone();
            let pending = self.pending.clone();
            let url = url.clone();
            let h = hash.clone();
            let layout = opts.layout;
            tokio::spawn(async move {
                let flag = |f: fn(&Pending) -> bool| {
                    let pending = pending.clone();
                    let h = h.clone();
                    move || pending.lock().unwrap().get(&h).is_some_and(f)
                };
                let result = resolve_and_add(&session, AddTorrent::from_url(url), save, layout, flag(|p| p.paused)).await;
                let mut p = pending.lock().unwrap();
                match result {
                    Ok(_) => {
                        let cancelled = p.get(&h).is_some_and(|x| x.cancelled);
                        p.remove(&h);
                        if cancelled {
                            // Removed while we were resolving: undo the add.
                            if let Ok(id) = parse_hash(&h) {
                                let session = session.clone();
                                tokio::spawn(async move {
                                    let _ = session.delete(TorrentIdOrHash::Hash(id), false).await;
                                });
                            }
                        }
                    }
                    Err(e) => {
                        tracing::warn!("magnet {h}: {e:#}");
                        if let Some(x) = p.get_mut(&h) {
                            x.error = Some(format!("{e:#}"));
                        }
                    }
                }
            });
            return Ok(AddOutcome { info_hash: hash, already_existed: false });
        }

        // .torrent files and URLs: metadata is in hand (or one HTTP fetch away).
        let add = match source {
            AddSource::Url(url) => AddTorrent::from_url(url),
            AddSource::TorrentFile(bytes) => AddTorrent::from_bytes(bytes),
        };
        let paused = opts.paused;
        let (info_hash, already_existed) =
            resolve_and_add(&self.session, add, save, opts.layout, move || paused).await?;
        Ok(AddOutcome { info_hash, already_existed })
    }

    fn list(&self) -> Vec<TorrentStatus> {
        let mut out: Vec<TorrentStatus> = self
            .session
            .with_torrents(|iter| iter.map(|(_, t)| status_of(t)).collect());
        let known: std::collections::HashSet<String> = out.iter().map(|t| t.info_hash.clone()).collect();
        let pending = self.pending.lock().unwrap();
        out.extend(
            pending
                .iter()
                .filter(|(h, p)| !p.cancelled && !known.contains(*h))
                .map(|(h, p)| pending_status(h, p)),
        );
        out
    }

    fn status(&self, hash: &str) -> Option<TorrentStatus> {
        if let Ok(t) = self.handle(hash) {
            return Some(status_of(&t));
        }
        let pending = self.pending.lock().unwrap();
        pending
            .get(hash)
            .filter(|p| !p.cancelled)
            .map(|p| pending_status(hash, p))
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
        if let Some(p) = self.pending.lock().unwrap().get_mut(hash) {
            p.paused = true;
            return Ok(());
        }
        let t = self.handle(hash)?;
        if t.is_paused() {
            return Ok(());
        }
        self.session.pause(&t).await
    }

    async fn resume(&self, hash: &str) -> anyhow::Result<()> {
        if let Some(p) = self.pending.lock().unwrap().get_mut(hash) {
            p.paused = false;
            return Ok(());
        }
        let t = self.handle(hash)?;
        if !t.is_paused() {
            return Ok(());
        }
        self.session.unpause(&t).await
    }

    async fn remove(&self, hash: &str, delete_files: bool) -> anyhow::Result<()> {
        {
            let mut pending = self.pending.lock().unwrap();
            if let Some(p) = pending.get_mut(hash) {
                // Still resolving: mark it, the resolver undoes the add if it lands.
                p.cancelled = true;
                if p.error.is_some() {
                    pending.remove(hash);
                }
                return Ok(());
            }
        }
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

#[cfg(test)]
mod tests {
    use crate::ContentLayout;
    use std::path::Path;

    #[test]
    fn layouts_match_qbittorrent() {
        let save = Path::new("/data/tv");
        assert_eq!(ContentLayout::Original.folder(save, "Show.S01", 3), save.join("Show.S01"));
        assert_eq!(ContentLayout::Original.folder(save, "ep.mkv", 1), save);
        assert_eq!(ContentLayout::Subfolder.folder(save, "ep.mkv", 1), save.join("ep"));
        assert_eq!(ContentLayout::NoSubfolder.folder(save, "Show.S01", 3), save);
        // Never let a hostile torrent name escape the save folder.
        assert_eq!(ContentLayout::Original.folder(save, "../../etc", 3), save);
    }
}
