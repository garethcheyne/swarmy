//! The download queue: jobs (one per NZB), a pool of connections per
//! server pulling articles off the queue, decoding and writing them into
//! place, then handing finished jobs to post-processing.
//!
//! Scheduling: jobs run in queue order (priority first). Each server has
//! `connections` workers. An article is first tried on the main servers
//! (lowest priority number); if they don't have it (430), the next priority
//! group of backup servers gets it, and so on. If no server has it, it's
//! missing and post-processing will need PAR2 to repair the gap.

use std::{
    collections::VecDeque,
    path::PathBuf,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    time::{Duration, Instant},
};

use anyhow::Context;
use serde::{Deserialize, Serialize};
use tokio::sync::{Notify, broadcast};

use crate::{
    nntp::{Connection, NntpError, ServerConfig},
    nzb::{Nzb, sanitise},
    post, yenc,
};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Server {
    pub id: String,
    pub name: String,
    pub enabled: bool,
    /// 0 = main server; higher numbers are backups, tried in order only for
    /// articles the lower ones don't have.
    pub priority: u8,
    #[serde(flatten)]
    pub config: ServerConfig,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
pub enum Priority {
    Low,
    Normal,
    High,
    Force,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum JobStatus {
    Queued,
    Downloading,
    Paused,
    /// Fetching PAR2 recovery blocks because something was damaged.
    Fetching,
    Verifying,
    Repairing,
    Extracting,
    Moving,
    Completed,
    Failed,
}

impl JobStatus {
    pub fn finished(self) -> bool {
        matches!(self, Self::Completed | Self::Failed)
    }
    fn downloading(self) -> bool {
        matches!(self, Self::Queued | Self::Downloading | Self::Fetching)
    }
}

/// What the UI and the SABnzbd API see.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JobView {
    pub id: String,
    pub name: String,
    pub category: String,
    pub priority: Priority,
    pub status: JobStatus,
    /// Decoded bytes, roughly 3% less than the encoded article sizes.
    pub size: u64,
    pub downloaded: u64,
    pub missing: u64,
    pub files: usize,
    pub added: i64,
    pub completed: Option<i64>,
    /// Where the finished files are.
    pub storage: Option<PathBuf>,
    pub fail_message: Option<String>,
    /// One-line status for post-processing steps ("Repaired 3 blocks").
    pub message: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ServerStatus {
    pub id: String,
    pub connections: usize,
    pub downloaded: u64,
    pub error: Option<String>,
}

#[derive(Debug, Clone)]
pub enum Event {
    Completed(JobView),
    Failed(JobView),
}

pub struct Settings {
    pub servers: Vec<Server>,
    pub incomplete_dir: PathBuf,
    pub complete_dir: PathBuf,
    /// Bytes per second; 0 is unlimited.
    pub speed_limit: u64,
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum Seg {
    Pending,
    InFlight,
    Done,
    Missing,
    /// PAR2 recovery data: only fetched if repair is needed.
    Held,
}

struct SegRt {
    file: usize,
    bytes: u64,
    message_id: String,
    state: Seg,
    /// Servers (by index) that said they don't have it.
    tried: u64,
}

struct Writer {
    dir: PathBuf,
    name: String,
    /// The subject didn't give a name; take the first yEnc header's.
    named: bool,
    file: Option<std::fs::File>,
    damaged: bool,
}

struct JobRt {
    view: JobView,
    password: Option<String>,
    segs: Vec<SegRt>,
    pending: VecDeque<usize>,
    retry: VecDeque<usize>,
    writers: Arc<Vec<Mutex<Writer>>>,
    dir: PathBuf,
    paused: bool,
}

struct ServerRt {
    server: Server,
    index: usize,
    connections: AtomicU64,
    downloaded: AtomicU64,
    error: Mutex<Option<String>>,
}

struct Inner {
    jobs: Mutex<Vec<JobRt>>,
    history: Mutex<Vec<JobView>>,
    servers: Mutex<Vec<Arc<ServerRt>>>,
    /// Bumped whenever servers change; old workers notice and exit.
    generation: AtomicU64,
    incomplete_dir: Mutex<PathBuf>,
    complete_dir: Mutex<PathBuf>,
    work: Notify,
    paused: AtomicBool,
    limiter: Limiter,
    downloaded: AtomicU64,
    events: broadcast::Sender<Event>,
    meter: Mutex<Meter>,
}

/// Shared handle to the downloader.
#[derive(Clone)]
pub struct Downloader {
    inner: Arc<Inner>,
}

fn now() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0)
}

fn new_id() -> String {
    // SABnzbd's format; Sonarr and Radarr treat it as opaque.
    let n: u64 = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos() as u64).unwrap_or(0);
    let mix = n ^ (n >> 29) ^ (std::process::id() as u64).rotate_left(17);
    format!("SABnzbd_nzo_{:012x}", mix & 0xffff_ffff_ffff)
}

/// Recovery volumes look like name.vol07+08.par2; the plain .par2 is the index.
fn is_recovery_volume(name: &str) -> bool {
    let l = name.to_ascii_lowercase();
    l.ends_with(".par2") && l.contains(".vol")
}

impl Downloader {
    pub fn new(settings: Settings) -> Self {
        let (events, _) = broadcast::channel(64);
        let d = Self {
            inner: Arc::new(Inner {
                jobs: Mutex::new(Vec::new()),
                history: Mutex::new(Vec::new()),
                servers: Mutex::new(Vec::new()),
                generation: AtomicU64::new(0),
                incomplete_dir: Mutex::new(settings.incomplete_dir),
                complete_dir: Mutex::new(settings.complete_dir),
                work: Notify::new(),
                paused: AtomicBool::new(false),
                limiter: Limiter::new(settings.speed_limit),
                downloaded: AtomicU64::new(0),
                events,
                meter: Mutex::new(Meter::default()),
            }),
        };
        d.set_servers(settings.servers);
        d
    }

    pub fn subscribe(&self) -> broadcast::Receiver<Event> {
        self.inner.events.subscribe()
    }

    /// Replace the server list and restart the connection workers.
    pub fn set_servers(&self, servers: Vec<Server>) {
        let generation = self.inner.generation.fetch_add(1, Ordering::SeqCst) + 1;
        let rts: Vec<Arc<ServerRt>> = servers
            .into_iter()
            .enumerate()
            .map(|(index, server)| {
                Arc::new(ServerRt {
                    server,
                    index,
                    connections: AtomicU64::new(0),
                    downloaded: AtomicU64::new(0),
                    error: Mutex::new(None),
                })
            })
            .collect();
        for s in &rts {
            if !s.server.enabled {
                continue;
            }
            for _ in 0..s.server.config.connections.clamp(1, 200) {
                tokio::spawn(worker(self.inner.clone(), s.clone(), generation));
            }
        }
        *self.inner.servers.lock().unwrap() = rts;
        // Articles some old server didn't have get another chance.
        for job in self.inner.jobs.lock().unwrap().iter_mut() {
            for (i, s) in job.segs.iter_mut().enumerate() {
                s.tried = 0;
                if s.state == Seg::Missing {
                    s.state = Seg::Pending;
                    job.view.missing = job.view.missing.saturating_sub(s.bytes);
                    job.pending.push_back(i);
                }
            }
        }
        self.inner.work.notify_waiters();
    }

    pub fn set_dirs(&self, incomplete: PathBuf, complete: PathBuf) {
        *self.inner.incomplete_dir.lock().unwrap() = incomplete;
        *self.inner.complete_dir.lock().unwrap() = complete;
    }

    pub fn set_speed_limit(&self, bytes_per_sec: u64) {
        self.inner.limiter.set_rate(bytes_per_sec);
    }

    pub fn set_paused(&self, paused: bool) {
        self.inner.paused.store(paused, Ordering::SeqCst);
        self.inner.work.notify_waiters();
    }

    pub fn paused(&self) -> bool {
        self.inner.paused.load(Ordering::SeqCst)
    }

    /// Queue an NZB. Returns the job id.
    pub fn add(&self, nzb_bytes: &[u8], name: Option<&str>, category: &str, priority: Priority) -> anyhow::Result<String> {
        self.add_with_id(new_id(), nzb_bytes, name, category, priority, None)
    }

    /// Re-queue a job after a restart, keeping its id (Sonarr and Radarr
    /// track it) and its folder. Parts already on disk are fetched again.
    pub fn restore(&self, id: &str, name: &str, category: &str, priority: Priority, paused: bool) -> anyhow::Result<()> {
        let dir = self.inner.incomplete_dir.lock().unwrap().join(id);
        let nzb = std::fs::read(dir.join("job.nzb")).with_context(|| format!("reading the saved NZB for {name}"))?;
        self.add_with_id(id.to_string(), &nzb, Some(name), category, priority, Some(added_at(&dir)))?;
        if paused {
            self.pause_job(id, true);
        }
        Ok(())
    }

    fn add_with_id(&self, id: String, nzb_bytes: &[u8], name: Option<&str>, category: &str, priority: Priority, added: Option<i64>) -> anyhow::Result<String> {
        if !id.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
            anyhow::bail!("bad job id");
        }
        let nzb = Nzb::parse(nzb_bytes)?;
        let name = name
            .map(|n| n.trim_end_matches(".nzb").to_string())
            .or_else(|| nzb.meta.get("title").cloned())
            .unwrap_or_else(|| nzb.files[0].name_from_subject());
        let name = sanitise(&name);
        let dir = self.inner.incomplete_dir.lock().unwrap().join(&id);
        std::fs::create_dir_all(&dir)?;
        std::fs::write(dir.join("job.nzb"), nzb_bytes)?;

        let mut segs = Vec::new();
        let mut writers = Vec::new();
        let mut recovery_count = 0;
        let mut used = std::collections::HashSet::new();
        for (fi, f) in nzb.files.iter().enumerate() {
            let quoted = f.subject.contains('"');
            let mut fname = f.name_from_subject();
            while !used.insert(fname.clone()) {
                fname = format!("{fname}.{fi}");
            }
            let recovery = is_recovery_volume(&fname);
            if recovery {
                recovery_count += 1;
            }
            writers.push(Mutex::new(Writer { dir: dir.clone(), name: fname, named: quoted, file: None, damaged: false }));
            for s in &f.segments {
                segs.push(SegRt {
                    file: fi,
                    bytes: s.bytes,
                    message_id: s.message_id.clone(),
                    state: if recovery { Seg::Held } else { Seg::Pending },
                    tried: 0,
                });
            }
        }
        // Small PAR2 index files first (they help identify obfuscated
        // files), then everything else in NZB order.
        let mut order: Vec<usize> = (0..segs.len()).filter(|&i| segs[i].state == Seg::Pending).collect();
        order.sort_by_key(|&i| !nzb.files[segs[i].file].is_par2());
        let encoded_total: u64 = segs.iter().filter(|s| s.state != Seg::Held).map(|s| s.bytes).sum();
        let job = JobRt {
            view: JobView {
                id: id.clone(),
                name,
                category: category.to_string(),
                priority,
                status: JobStatus::Queued,
                size: encoded_total * 97 / 100,
                downloaded: 0,
                missing: 0,
                files: nzb.files.len() - recovery_count,
                added: added.unwrap_or_else(now),
                completed: None,
                storage: None,
                fail_message: None,
                message: None,
            },
            password: nzb.meta.get("password").cloned(),
            pending: order.into(),
            retry: VecDeque::new(),
            segs,
            writers: Arc::new(writers),
            dir,
            paused: false,
        };
        let mut jobs = self.inner.jobs.lock().unwrap();
        // Higher priority jumps ahead of lower; same priority keeps order.
        let at = jobs.iter().position(|j| j.view.priority < priority).unwrap_or(jobs.len());
        jobs.insert(at, job);
        drop(jobs);
        self.inner.work.notify_waiters();
        Ok(id)
    }

    pub fn queue(&self) -> Vec<JobView> {
        self.inner.jobs.lock().unwrap().iter().map(|j| j.view.clone()).collect()
    }

    pub fn history(&self) -> Vec<JobView> {
        self.inner.history.lock().unwrap().clone()
    }

    pub fn job(&self, id: &str) -> Option<JobView> {
        self.inner
            .jobs
            .lock()
            .unwrap()
            .iter()
            .map(|j| &j.view)
            .chain(self.inner.history.lock().unwrap().iter())
            .find(|v| v.id == id)
            .cloned()
    }

    pub fn pause_job(&self, id: &str, paused: bool) -> bool {
        let mut jobs = self.inner.jobs.lock().unwrap();
        let Some(j) = jobs.iter_mut().find(|j| j.view.id == id) else { return false };
        j.paused = paused;
        if j.view.status.downloading() || j.view.status == JobStatus::Paused {
            j.view.status = if paused { JobStatus::Paused } else { JobStatus::Queued };
        }
        drop(jobs);
        self.inner.work.notify_waiters();
        true
    }

    /// Remove from the queue (and its partial files) or from history
    /// (optionally deleting the finished files).
    pub fn remove(&self, id: &str, delete_files: bool) -> bool {
        let removed = {
            let mut jobs = self.inner.jobs.lock().unwrap();
            jobs.iter().position(|j| j.view.id == id).map(|i| jobs.remove(i))
        };
        if let Some(job) = removed {
            drop(job.writers);
            let _ = std::fs::remove_dir_all(&job.dir);
            return true;
        }
        let mut history = self.inner.history.lock().unwrap();
        if let Some(i) = history.iter().position(|v| v.id == id) {
            let v = history.remove(i);
            if delete_files && let Some(p) = &v.storage {
                let _ = if p.is_dir() { std::fs::remove_dir_all(p) } else { std::fs::remove_file(p) };
            }
            return true;
        }
        false
    }

    /// Move a queued job to `index` in the queue.
    pub fn move_job(&self, id: &str, index: usize) -> Option<usize> {
        let mut jobs = self.inner.jobs.lock().unwrap();
        let from = jobs.iter().position(|j| j.view.id == id)?;
        let job = jobs.remove(from);
        let to = index.min(jobs.len());
        jobs.insert(to, job);
        Some(to)
    }

    pub fn servers(&self) -> Vec<ServerStatus> {
        self.inner
            .servers
            .lock()
            .unwrap()
            .iter()
            .map(|s| ServerStatus {
                id: s.server.id.clone(),
                connections: s.connections.load(Ordering::Relaxed) as usize,
                downloaded: s.downloaded.load(Ordering::Relaxed),
                error: s.error.lock().unwrap().clone(),
            })
            .collect()
    }

    /// Bytes per second over the last few seconds.
    pub fn speed(&self) -> u64 {
        self.inner.meter.lock().unwrap().rate()
    }

    /// Decoded bytes downloaded since start.
    pub fn downloaded(&self) -> u64 {
        self.inner.downloaded.load(Ordering::Relaxed)
    }

    /// Bytes still to download in the queue.
    pub fn remaining(&self) -> u64 {
        self.inner.jobs.lock().unwrap().iter().map(|j| j.view.size.saturating_sub(j.view.downloaded)).sum()
    }

    /// Restore finished jobs saved by the app (history survives restarts).
    pub fn restore_history(&self, history: Vec<JobView>) {
        *self.inner.history.lock().unwrap() = history;
    }
}

/// When a job folder was created, as a Unix time (for restored jobs).
fn added_at(dir: &std::path::Path) -> i64 {
    std::fs::metadata(dir)
        .and_then(|m| m.created().or_else(|_| m.modified()))
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map_or_else(now, |d| d.as_secs() as i64)
}

/// Try a server's connection: connect, sign in, and quit.
pub async fn test_server(cfg: &ServerConfig) -> Result<(), NntpError> {
    let c = Connection::open(cfg).await?;
    c.quit().await;
    Ok(())
}

enum Pick {
    Work { job: String, seg: usize, message_id: String, bytes: u64, writers: Arc<Vec<Mutex<Writer>>>, file: usize },
    Idle,
}

/// Is `server` the right one to try this article next?
fn eligible(seg: &SegRt, server: &ServerRt, servers: &[Arc<ServerRt>]) -> bool {
    if seg.tried & (1 << server.index) != 0 {
        return false;
    }
    let lowest_untried = servers
        .iter()
        .filter(|s| s.server.enabled && seg.tried & (1 << s.index) == 0)
        .map(|s| s.server.priority)
        .min();
    lowest_untried == Some(server.server.priority)
}

fn pick(inner: &Inner, server: &ServerRt) -> Pick {
    if inner.paused.load(Ordering::Relaxed) {
        return Pick::Idle;
    }
    let servers = inner.servers.lock().unwrap().clone();
    let main = servers.iter().filter(|s| s.server.enabled).map(|s| s.server.priority).min() == Some(server.server.priority);
    let mut jobs = inner.jobs.lock().unwrap();
    for job in jobs.iter_mut() {
        if job.paused || !job.view.status.downloading() {
            continue;
        }
        // Retries (articles another server didn't have) first.
        let mut found = None;
        for k in 0..job.retry.len() {
            if eligible(&job.segs[job.retry[k]], server, &servers) {
                found = job.retry.remove(k);
                break;
            }
        }
        // Untried articles go to the main servers only; backups just get
        // what the main servers didn't have.
        if found.is_none() && main {
            while let Some(i) = job.pending.pop_front() {
                if job.segs[i].state == Seg::Pending {
                    found = Some(i);
                    break;
                }
            }
        }
        if let Some(i) = found {
            let s = &mut job.segs[i];
            s.state = Seg::InFlight;
            if job.view.status == JobStatus::Queued {
                job.view.status = JobStatus::Downloading;
            }
            return Pick::Work {
                job: job.view.id.clone(),
                seg: i,
                message_id: s.message_id.clone(),
                bytes: s.bytes,
                writers: job.writers.clone(),
                file: s.file,
            };
        }
    }
    Pick::Idle
}

enum Outcome {
    Done { decoded: u64, intact: bool },
    Missing,
    /// Back in the queue: connection trouble, try again.
    Retry,
}

async fn worker(inner: Arc<Inner>, server: Arc<ServerRt>, generation: u64) {
    let mut conn: Option<Connection> = None;
    let mut backoff = Duration::from_secs(1);
    loop {
        if inner.generation.load(Ordering::SeqCst) != generation {
            break;
        }
        let Pick::Work { job, seg, message_id, bytes, writers, file } = pick(&inner, &server) else {
            // Nothing to do: drop idle connections after a while.
            let idle = tokio::time::timeout(Duration::from_secs(30), inner.work.notified()).await;
            if idle.is_err()
                && let Some(c) = conn.take()
            {
                c.quit().await;
                server.connections.fetch_sub(1, Ordering::Relaxed);
            }
            continue;
        };
        inner.limiter.acquire(bytes).await;

        if conn.is_none() {
            match Connection::open(&server.server.config).await {
                Ok(c) => {
                    conn = Some(c);
                    server.connections.fetch_add(1, Ordering::Relaxed);
                    *server.error.lock().unwrap() = None;
                    backoff = Duration::from_secs(1);
                }
                Err(e) => {
                    tracing::warn!(server = %server.server.name, "connection failed: {e}");
                    *server.error.lock().unwrap() = Some(e.to_string());
                    finish(&inner, &job, seg, Outcome::Retry, &server);
                    // Bad credentials won't fix themselves quickly.
                    let wait = if matches!(e, NntpError::AuthFailed(_)) { Duration::from_secs(120) } else { backoff };
                    backoff = (backoff * 2).min(Duration::from_secs(60));
                    tokio::time::sleep(wait).await;
                    continue;
                }
            }
        }
        let c = conn.as_mut().expect("connected");
        let outcome = match c.body(&message_id).await {
            Ok(body) => {
                server.downloaded.fetch_add(body.len() as u64, Ordering::Relaxed);
                let result = tokio::task::spawn_blocking(move || write_part(&writers, file, &body)).await;
                match result {
                    Ok(Ok((decoded, intact))) => Outcome::Done { decoded, intact },
                    Ok(Err(e)) => {
                        tracing::warn!("couldn't decode or write an article: {e:#}");
                        Outcome::Done { decoded: 0, intact: false }
                    }
                    Err(_) => Outcome::Retry,
                }
            }
            Err(NntpError::NoSuchArticle) => Outcome::Missing,
            Err(e) => {
                tracing::debug!(server = %server.server.name, "article fetch failed: {e}");
                *server.error.lock().unwrap() = Some(e.to_string());
                conn = None;
                server.connections.fetch_sub(1, Ordering::Relaxed);
                Outcome::Retry
            }
        };
        finish(&inner, &job, seg, outcome, &server);
    }
    if let Some(c) = conn.take() {
        c.quit().await;
        server.connections.fetch_sub(1, Ordering::Relaxed);
    }
}

/// Decode and write one article into its file. (decoded bytes, intact)
fn write_part(writers: &[Mutex<Writer>], file: usize, body: &[u8]) -> anyhow::Result<(u64, bool)> {
    let part = yenc::decode(body)?;
    let mut w = writers[file].lock().unwrap();
    if w.file.is_none() {
        if !w.named && !part.name.is_empty() {
            let n = sanitise(&part.name);
            let taken = writers.iter().enumerate().any(|(i, o)| i != file && o.try_lock().map(|o| o.name == n).unwrap_or(false));
            if !taken {
                w.name = n;
            }
            w.named = true;
        }
        let f = std::fs::OpenOptions::new().create(true).truncate(false).read(true).write(true).open(w.dir.join(&w.name))?;
        if f.metadata()?.len() < part.file_size {
            f.set_len(part.file_size)?;
        }
        w.file = Some(f);
    }
    if part.file_size > 0 && part.offset + part.data.len() as u64 > part.file_size {
        anyhow::bail!("part runs past the end of the file");
    }
    write_at(w.file.as_ref().expect("open"), part.offset, &part.data)?;
    if !part.intact {
        w.damaged = true;
    }
    Ok((part.data.len() as u64, part.intact))
}

#[cfg(unix)]
fn write_at(f: &std::fs::File, offset: u64, data: &[u8]) -> std::io::Result<()> {
    use std::os::unix::fs::FileExt;
    f.write_all_at(data, offset)
}

#[cfg(windows)]
fn write_at(f: &std::fs::File, mut offset: u64, mut data: &[u8]) -> std::io::Result<()> {
    use std::os::windows::fs::FileExt;
    while !data.is_empty() {
        let n = f.seek_write(data, offset)?;
        data = &data[n..];
        offset += n as u64;
    }
    Ok(())
}

fn finish(inner: &Arc<Inner>, job_id: &str, seg: usize, outcome: Outcome, server: &ServerRt) {
    let servers = inner.servers.lock().unwrap().clone();
    let mut jobs = inner.jobs.lock().unwrap();
    let Some(job) = jobs.iter_mut().find(|j| j.view.id == job_id) else { return };
    let s = &mut job.segs[seg];
    match outcome {
        Outcome::Done { decoded, intact } => {
            s.state = Seg::Done;
            job.view.downloaded += decoded;
            inner.downloaded.fetch_add(decoded, Ordering::Relaxed);
            inner.meter.lock().unwrap().add(decoded);
            if !intact {
                job.view.message = Some("Some articles were damaged; they'll be checked and repaired".into());
            }
        }
        Outcome::Missing => {
            s.tried |= 1 << server.index;
            let anyone_left = servers.iter().any(|o| o.server.enabled && s.tried & (1 << o.index) == 0);
            if anyone_left {
                s.state = Seg::Pending;
                job.retry.push_back(seg);
            } else {
                s.state = Seg::Missing;
                job.view.missing += s.bytes;
            }
        }
        Outcome::Retry => {
            s.state = Seg::Pending;
            // Articles some server already lacked stay on the retry list.
            if s.tried == 0 { job.pending.push_front(seg) } else { job.retry.push_front(seg) }
        }
    }
    // Everything fetched (or given up on)? Hand over to post-processing.
    let busy = job.segs.iter().any(|s| matches!(s.state, Seg::Pending | Seg::InFlight));
    if !busy && job.view.status.downloading() {
        job.view.status = JobStatus::Verifying;
        let id = job.view.id.clone();
        drop(jobs);
        let inner = inner.clone();
        tokio::spawn(async move { post_process(inner, id).await });
    } else {
        drop(jobs);
        inner.work.notify_one();
    }
}

fn set_status(inner: &Inner, id: &str, status: JobStatus, message: Option<String>) {
    if let Some(j) = inner.jobs.lock().unwrap().iter_mut().find(|j| j.view.id == id) {
        j.view.status = status;
        if message.is_some() {
            j.view.message = message;
        }
    }
}

/// Fetch the held PAR2 recovery volumes for a job. Returns false if it has none.
fn release_recovery(inner: &Inner, id: &str) -> bool {
    let mut jobs = inner.jobs.lock().unwrap();
    let Some(job) = jobs.iter_mut().find(|j| j.view.id == id) else { return false };
    let mut any = false;
    for i in 0..job.segs.len() {
        if job.segs[i].state == Seg::Held {
            job.segs[i].state = Seg::Pending;
            job.pending.push_back(i);
            any = true;
        }
    }
    if any {
        job.view.status = JobStatus::Fetching;
        job.view.message = Some("Damaged: fetching repair data".into());
    }
    any
}

async fn post_process(inner: Arc<Inner>, id: String) {
    let (dir, name, category, password, writers, missing, recovery_held) = {
        let jobs = inner.jobs.lock().unwrap();
        let Some(j) = jobs.iter().find(|j| j.view.id == id) else { return };
        let held = j.segs.iter().any(|s| s.state == Seg::Held);
        (j.dir.clone(), j.view.name.clone(), j.view.category.clone(), j.password.clone(), j.writers.clone(), j.view.missing, held)
    };
    // Close the files and gather their names.
    let mut files = Vec::new();
    let mut damaged = missing > 0;
    for w in writers.iter() {
        let mut w = w.lock().unwrap();
        w.file = None;
        damaged |= w.damaged;
        let p = w.dir.join(&w.name);
        if p.exists() {
            files.push(p);
        }
    }
    // Quick check: if every article arrived intact there's nothing to repair.
    // Otherwise fetch the recovery volumes first, if we held any back.
    if damaged && recovery_held && release_recovery(&inner, &id) {
        inner.work.notify_waiters();
        return; // finish() calls us again once they've arrived.
    }
    let complete_dir = inner.complete_dir.lock().unwrap().clone();
    let progress = {
        let inner = inner.clone();
        let id = id.clone();
        move |status: JobStatus, msg: Option<String>| set_status(&inner, &id, status, msg)
    };
    let job = post::Job { dir: dir.clone(), name, category, password, files, damaged, complete_dir };
    let result = tokio::task::spawn_blocking(move || post::run(&job, &progress)).await;
    let mut jobs = inner.jobs.lock().unwrap();
    let Some(pos) = jobs.iter().position(|j| j.view.id == id) else { return };
    let mut job = jobs.remove(pos);
    drop(jobs);
    job.view.completed = Some(now());
    let event = match result {
        Ok(Ok(out)) => {
            job.view.status = JobStatus::Completed;
            // The real size of what was delivered, not the estimate.
            job.view.size = dir_size(&out.storage);
            job.view.downloaded = job.view.size;
            job.view.storage = Some(out.storage);
            job.view.message = out.message;
            let _ = std::fs::remove_dir_all(&dir);
            Event::Completed(job.view.clone())
        }
        Ok(Err(e)) => {
            job.view.status = JobStatus::Failed;
            job.view.fail_message = Some(format!("{e:#}"));
            Event::Failed(job.view.clone())
        }
        Err(e) => {
            job.view.status = JobStatus::Failed;
            job.view.fail_message = Some(format!("post-processing crashed: {e}"));
            Event::Failed(job.view.clone())
        }
    };
    inner.history.lock().unwrap().insert(0, job.view);
    let _ = inner.events.send(event);
    inner.work.notify_waiters();
}

fn dir_size(p: &std::path::Path) -> u64 {
    match std::fs::metadata(p) {
        Ok(m) if m.is_file() => m.len(),
        Ok(_) => std::fs::read_dir(p).map(|d| d.flatten().map(|e| dir_size(&e.path())).sum()).unwrap_or(0),
        Err(_) => 0,
    }
}

/// A token bucket for the download speed limit.
struct Limiter {
    rate: AtomicU64,
    state: Mutex<(Instant, f64)>,
}

impl Limiter {
    fn new(rate: u64) -> Self {
        Self { rate: AtomicU64::new(rate), state: Mutex::new((Instant::now(), 0.0)) }
    }
    fn set_rate(&self, rate: u64) {
        self.rate.store(rate, Ordering::Relaxed);
    }
    async fn acquire(&self, bytes: u64) {
        loop {
            let rate = self.rate.load(Ordering::Relaxed);
            if rate == 0 {
                return;
            }
            let wait = {
                let mut st = self.state.lock().unwrap();
                let now = Instant::now();
                // Allow a burst of up to one second's worth.
                st.1 = (st.1 + now.duration_since(st.0).as_secs_f64() * rate as f64).min(rate as f64);
                st.0 = now;
                if st.1 >= bytes as f64 || st.1 >= rate as f64 {
                    st.1 -= bytes as f64;
                    return;
                }
                Duration::from_secs_f64(((bytes as f64 - st.1) / rate as f64).min(1.0))
            };
            tokio::time::sleep(wait).await;
        }
    }
}

/// A short moving window for the current speed.
#[derive(Default)]
struct Meter {
    samples: VecDeque<(Instant, u64)>,
}

impl Meter {
    fn add(&mut self, bytes: u64) {
        let now = Instant::now();
        self.samples.push_back((now, bytes));
        while self.samples.front().is_some_and(|(t, _)| now.duration_since(*t) > Duration::from_secs(5)) {
            self.samples.pop_front();
        }
    }
    fn rate(&self) -> u64 {
        let Some((first, _)) = self.samples.front() else { return 0 };
        let span = first.elapsed().as_secs_f64().max(1.0);
        if first.elapsed() > Duration::from_secs(6) {
            return 0;
        }
        (self.samples.iter().map(|(_, b)| *b).sum::<u64>() as f64 / span) as u64
    }
}
