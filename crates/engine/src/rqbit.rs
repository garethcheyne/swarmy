use std::{
    collections::HashMap,
    net::{Ipv4Addr, Ipv6Addr, SocketAddr},
    num::NonZeroU32,
    path::PathBuf,
    str::FromStr,
    sync::{
        Arc, Mutex,
        atomic::{AtomicU64, Ordering},
    },
};

use anyhow::Context;
use async_trait::async_trait;
use librqbit::{
    AddTorrent, AddTorrentOptions, AddTorrentResponse, ListenerMode, ListenerOptions, Magnet,
    ManagedTorrent, Session, SessionOptions, SessionPersistenceConfig, TorrentStatsState,
    api::TorrentIdOrHash, dht::Id20,
};

use crate::{
    AddOptions, AddOutcome, AddSource, ContentLayout, Engine, FileEntry, MoveProgress, NetworkInfo, PeerEntry,
    PieceMap, TorrentState, TorrentStatus,
    relocate::{self, Journal, Journals, Phase},
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
    /// What peers see us as, e.g. "Swarmy 2026.11.05.1".
    pub client_name: String,
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

/// A torrent whose files are moving. While it's briefly out of the session
/// (between removing it and adding it back at the new folder) it's shown from
/// `last`; pause, resume and remove requests are held until the move ends.
struct Moving {
    last: TorrentStatus,
    done: Arc<AtomicU64>,
    total: u64,
    paused: bool,
    /// Remove once moved: `Some(delete_files)`.
    remove: Option<bool>,
}

fn moving_status(mut s: TorrentStatus, m: &Moving) -> TorrentStatus {
    s.state = TorrentState::Moving;
    s.download_speed = 0;
    s.upload_speed = 0;
    s.eta_secs = None;
    s.peers = 0;
    s.seeds = 0;
    s.moving = Some(MoveProgress {
        done_bytes: m.done.load(Ordering::Relaxed).min(m.total),
        total_bytes: m.total,
    });
    s
}

pub struct RqbitEngine {
    session: Arc<Session>,
    api: librqbit::Api,
    bind_interface: Option<String>,
    default_dir: PathBuf,
    state_dir: PathBuf,
    pending: Arc<Mutex<HashMap<String, Pending>>>,
    moving: Mutex<HashMap<String, Moving>>,
    journals: Journals,
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
            client_name_and_version: Some(cfg.client_name.clone()),
            ..Default::default()
        };

        // Settle any move a crash interrupted before the session sees the files.
        let journals = Journals::new(&cfg.state_dir);
        let readd = relocate::recover(&journals, &cfg.state_dir);

        let session = Session::new_with_opts(cfg.default_download_dir.clone(), opts)
            .await
            .context("starting torrent session")?;
        for j in readd {
            let added = match journals.torrent(&j.info_hash) {
                Ok(bytes) => add_at(&session, bytes.into(), &j.from, j.paused, j.only_files.clone()).await,
                Err(e) => Err(e),
            };
            match added {
                Ok(()) => journals.finish(&j.info_hash),
                Err(e) => tracing::error!("restoring {} after an interrupted move: {e:#}", j.info_hash),
            }
        }
        let api = librqbit::Api::new(session.clone(), None);
        Ok(Self {
            session,
            api,
            bind_interface: cfg.bind_interface,
            default_dir: cfg.default_download_dir,
            state_dir: cfg.state_dir,
            pending: Default::default(),
            moving: Default::default(),
            journals,
        })
    }

    fn resume_file(&self, hash: &str) -> PathBuf {
        self.state_dir.join(format!("{hash}.bitv"))
    }

    /// The pause state the user wants once a move ends.
    fn wants_paused(&self, hash: &str) -> bool {
        self.moving.lock().unwrap().get(hash).is_some_and(|m| m.paused)
    }

    async fn restart_unless_paused(&self, hash: &str, t: &Arc<ManagedTorrent>) {
        if !self.wants_paused(hash) {
            let _ = self.session.unpause(t).await;
        }
    }

    /// A move, once it's registered in `moving` and checked.
    async fn relocate_registered(&self, t: Arc<ManagedTorrent>, mut j: Journal, done: Arc<AtomicU64>) -> anyhow::Result<()> {
        let hash = j.info_hash.clone();
        let id = t.info_hash();
        let torrent = t.with_metadata(|m| m.torrent_bytes.clone())?;
        anyhow::ensure!(!torrent.is_empty(), "This torrent's details aren't available to move it with.");

        // Stop it touching the files, and record the move before moving anything.
        if !t.is_paused() {
            self.session.pause(&t).await.context("pausing the torrent")?;
        }
        if let Err(e) = self.journals.begin(&j, &torrent, &self.resume_file(&hash)) {
            self.journals.finish(&hash);
            self.restart_unless_paused(&hash, &t).await;
            return Err(e);
        }

        let (job, counter) = (j.clone(), done.clone());
        let moved = tokio::task::spawn_blocking(move || relocate::move_files(&job, &counter))
            .await
            .map_err(anyhow::Error::from)
            .and_then(|r| r);
        if let Err(e) = moved {
            // move_files has already put everything back.
            self.journals.finish(&hash);
            self.restart_unless_paused(&hash, &t).await;
            return Err(e);
        }

        // Point the torrent at the new folder: take it out of the session (which
        // deletes its resume data), restore that, and add it back.
        drop(t);
        let failed = match self.session.delete(TorrentIdOrHash::Hash(id), false).await {
            Err(e) => Some(e.context("switching the torrent to its new folder")),
            Ok(()) => {
                self.journals.restore_resume(&hash, &self.resume_file(&hash));
                add_at(&self.session, torrent.clone(), &j.to, self.wants_paused(&hash), j.only_files.clone())
                    .await
                    .err()
                    .map(|e| e.context("starting the torrent from its new folder"))
            }
        };
        if let Some(why) = failed {
            // Put the files back and the torrent where it was.
            let job = j.clone();
            let _ = tokio::task::spawn_blocking(move || relocate::undo(&job)).await;
            match self.session.get(TorrentIdOrHash::Hash(id)) {
                Some(t) => self.restart_unless_paused(&hash, &t).await,
                None => {
                    self.journals.restore_resume(&hash, &self.resume_file(&hash));
                    add_at(&self.session, torrent, &j.from, self.wants_paused(&hash), j.only_files.clone())
                        .await
                        .context("putting the torrent back after a failed move")?;
                }
            }
            self.journals.finish(&hash);
            return Err(why);
        }

        // It runs from the new folder. Delete copied originals and empty folders.
        j.phase = Phase::Cleanup;
        if let Err(e) = self.journals.write(&j) {
            tracing::warn!("{e:#}");
        }
        let job = j.clone();
        let _ = tokio::task::spawn_blocking(move || relocate::cleanup(&job)).await;
        self.journals.finish(&hash);
        Ok(())
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

/// Add a torrent whose files are already in `folder`.
async fn add_at(
    session: &Arc<Session>,
    torrent: bytes::Bytes,
    folder: &std::path::Path,
    paused: bool,
    only_files: Option<Vec<usize>>,
) -> anyhow::Result<()> {
    session
        .add_torrent(
            AddTorrent::from_bytes(torrent),
            Some(AddTorrentOptions {
                paused,
                overwrite: true,
                output_folder: Some(folder.to_string_lossy().into_owned()),
                only_files,
                ..Default::default()
            }),
        )
        .await?;
    Ok(())
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
        moving: None,
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
        moving: None,
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
        let moving = self.moving.lock().unwrap();
        let mut out: Vec<TorrentStatus> = self.session.with_torrents(|iter| {
            iter.map(|(_, t)| status_of(t))
                .filter_map(|s| match moving.get(&s.info_hash) {
                    Some(m) if m.remove.is_some() => None,
                    Some(m) => Some(moving_status(s, m)),
                    None => Some(s),
                })
                .collect()
        });
        let mut known: std::collections::HashSet<String> = out.iter().map(|t| t.info_hash.clone()).collect();
        // Mid-swap, a moving torrent is briefly out of the session.
        for (h, m) in moving.iter() {
            if m.remove.is_none() && known.insert(h.clone()) {
                out.push(moving_status(m.last.clone(), m));
            }
        }
        drop(moving);
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
        if let Some(m) = self.moving.lock().unwrap().get(hash) {
            if m.remove.is_some() {
                return None;
            }
            let live = self.handle(hash).map(|t| status_of(&t)).unwrap_or_else(|_| m.last.clone());
            return Some(moving_status(live, m));
        }
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

    fn peers(&self, hash: &str) -> anyhow::Result<Vec<PeerEntry>> {
        if self.pending.lock().unwrap().contains_key(hash) {
            return Ok(Vec::new());
        }
        let id = parse_hash(hash)?;
        // Paused and initialising torrents have no live peers; that's not an error.
        let Ok(snapshot) = self.api.api_peer_stats(TorrentIdOrHash::Hash(id), Default::default()) else {
            return Ok(Vec::new());
        };
        Ok(snapshot
            .peers
            .into_iter()
            .filter_map(|(addr, p)| {
                Some(PeerEntry {
                    addr: addr.parse().ok()?,
                    client: p.client_name,
                    connection: p.conn_kind.map(|k| format!("{k:?}").to_lowercase()),
                    downloaded_bytes: p.counters.fetched_bytes,
                    uploaded_bytes: p.counters.uploaded_bytes,
                    download_speed: None,
                    upload_speed: None,
                })
            })
            .collect())
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
        if let Some(m) = self.moving.lock().unwrap().get_mut(hash) {
            m.paused = true;
            return Ok(());
        }
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
        if let Some(m) = self.moving.lock().unwrap().get_mut(hash) {
            m.paused = false;
            return Ok(());
        }
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
        if let Some(m) = self.moving.lock().unwrap().get_mut(hash) {
            m.remove = Some(delete_files);
            return Ok(());
        }
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

    async fn relocate(&self, hash: &str, to: PathBuf) -> anyhow::Result<()> {
        anyhow::ensure!(
            !self.pending.lock().unwrap().contains_key(hash),
            "This torrent is still fetching its details. Move it once it has started."
        );
        let t = self.handle(hash)?;
        let from = t.output_folder().to_path_buf();
        if relocate::same_dir(&from, &to) {
            return Ok(());
        }
        let name = t.name();
        let files: Vec<PathBuf> = t.with_metadata(|m| {
            m.file_infos.iter().filter(|f| !f.attrs.padding).map(|f| f.relative_filename.clone()).collect()
        })?;
        // A multi-file torrent's folder is its own (named after it); a single
        // file sits straight in a shared folder that must stay.
        let prune_from = files.len() > 1 && from.file_name().map(|n| n.to_string_lossy().into_owned()) == name;
        let j = Journal {
            info_hash: hash.to_owned(),
            from,
            to,
            files,
            paused: t.is_paused(),
            only_files: t.only_files(),
            prune_from,
            phase: Phase::Files,
        };
        relocate::check_targets(&j)?;

        let done = Arc::new(AtomicU64::new(0));
        {
            let mut moving = self.moving.lock().unwrap();
            anyhow::ensure!(!moving.contains_key(hash), "This torrent is already being moved.");
            moving.insert(
                hash.to_owned(),
                Moving {
                    last: status_of(&t),
                    done: done.clone(),
                    total: relocate::bytes_present(&j),
                    paused: j.paused,
                    remove: None,
                },
            );
        }
        let result = self.relocate_registered(t, j, done).await;
        let ended = self.moving.lock().unwrap().remove(hash);
        if let Some(Moving { remove: Some(delete_files), .. }) = ended {
            self.remove(hash, delete_files).await?;
        }
        result
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
