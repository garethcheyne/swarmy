//! Statistics: how hard the daemon is working, now and over time.
//!
//! - Every second the ticker calls [`Stats::record`]; the last 10 minutes of
//!   1-second samples live in memory for live charts.
//! - Per-minute aggregates are written to SQLite (`stats.db`) and kept forever
//!   (~525k rows/year), so any range can be queried and re-bucketed in SQL.
//! - Torrent lifecycle events (added/completed/removed) go to an `events` table.

use std::{
    collections::{HashMap, VecDeque},
    path::{Path, PathBuf},
    sync::Mutex,
};

use anyhow::Context;
use rusqlite::{Connection, params};
use serde::Serialize;

use crate::{
    state::TorrentView,
    store::now_unix,
};

const LIVE_SECONDS: usize = 600;

#[derive(Debug, Clone, Copy, Default, Serialize)]
pub struct LiveSample {
    pub t: i64,
    pub dl: u64,
    pub ul: u64,
    pub peers: u32,
    pub active: u32,
    /// Daemon CPU, percent of the whole machine.
    pub cpu: f32,
    /// Daemon resident memory, bytes.
    pub mem: u64,
}

#[derive(Debug, Clone, Copy, Default, Serialize)]
pub struct SystemSample {
    pub cpu: f32,
    pub mem: u64,
    pub disk_free: u64,
    pub disk_total: u64,
}

#[derive(Default)]
struct MinuteAcc {
    minute: i64,
    dl_bytes: u64,
    ul_bytes: u64,
    dl_peak: u64,
    ul_peak: u64,
    peers_sum: u64,
    active_sum: u64,
    torrents: u32,
    cpu_sum: f64,
    mem_max: u64,
    n: u32,
}

struct Inner {
    db: Connection,
    live: VecDeque<LiveSample>,
    /// Last seen cumulative (downloaded, uploaded) per torrent.
    counters: HashMap<String, (u64, u64)>,
    acc: MinuteAcc,
    system: SystemSample,
    session_dl: u64,
    session_ul: u64,
}

pub struct Stats {
    inner: Mutex<Inner>,
    pub started_at: i64,
    download_dir: PathBuf,
}

#[derive(Debug, Serialize)]
pub struct Bucket {
    pub t: i64,
    pub dl_bytes: u64,
    pub ul_bytes: u64,
    /// Average over the bucket, bytes/s.
    pub dl_avg: f64,
    pub ul_avg: f64,
    pub dl_peak: u64,
    pub ul_peak: u64,
    pub peers: f64,
    pub active: f64,
    pub cpu: f64,
    pub mem: u64,
}

#[derive(Debug, Serialize)]
pub struct History {
    pub range: String,
    pub bucket_secs: i64,
    pub from: i64,
    pub to: i64,
    pub buckets: Vec<Bucket>,
}

#[derive(Debug, Serialize)]
pub struct Day {
    pub day: i64,
    pub dl_bytes: u64,
    pub ul_bytes: u64,
}

#[derive(Debug, Serialize)]
pub struct Event {
    pub t: i64,
    pub kind: String,
    pub hash: String,
    pub name: String,
    pub size: u64,
    pub category: String,
}

#[derive(Debug, Serialize)]
pub struct Totals {
    pub dl_bytes: u64,
    pub ul_bytes: u64,
}

#[derive(Debug, Serialize)]
pub struct Summary {
    pub session: Totals,
    pub today: Totals,
    pub all_time: Totals,
    pub recording_since: Option<i64>,
    pub uptime_secs: i64,
    pub peak_dl: u64,
    pub peak_ul: u64,
    pub completed_total: u64,
    pub completed_7d: u64,
    pub added_7d: u64,
    pub busiest_day: Option<Day>,
    pub daily: Vec<Day>,
    pub hourly_profile: Vec<f64>,
    pub system: SystemSample,
}

pub enum Range {
    Hour,
    Day,
    Week,
    Month,
    Year,
}

impl Range {
    pub fn parse(s: &str) -> Option<Self> {
        Some(match s {
            "1h" => Self::Hour,
            "24h" => Self::Day,
            "7d" => Self::Week,
            "30d" => Self::Month,
            "1y" => Self::Year,
            _ => return None,
        })
    }
    fn secs_and_bucket(&self) -> (i64, i64) {
        match self {
            Self::Hour => (3600, 60),
            Self::Day => (86400, 15 * 60),
            Self::Week => (7 * 86400, 3600),
            Self::Month => (30 * 86400, 6 * 3600),
            Self::Year => (365 * 86400, 86400),
        }
    }
    fn label(&self) -> &'static str {
        match self {
            Self::Hour => "1h",
            Self::Day => "24h",
            Self::Week => "7d",
            Self::Month => "30d",
            Self::Year => "1y",
        }
    }
}

fn open_db(path: &Path) -> anyhow::Result<Connection> {
    let db = Connection::open(path).with_context(|| format!("opening {}", path.display()))?;
    db.execute_batch(
        "PRAGMA journal_mode=WAL;
         PRAGMA synchronous=NORMAL;
         CREATE TABLE IF NOT EXISTS minute (
             ts INTEGER PRIMARY KEY,
             dl_bytes INTEGER NOT NULL, ul_bytes INTEGER NOT NULL,
             dl_peak INTEGER NOT NULL, ul_peak INTEGER NOT NULL,
             peers REAL NOT NULL, active REAL NOT NULL, torrents INTEGER NOT NULL,
             cpu REAL NOT NULL, mem INTEGER NOT NULL
         );
         CREATE TABLE IF NOT EXISTS events (
             id INTEGER PRIMARY KEY AUTOINCREMENT,
             ts INTEGER NOT NULL, kind TEXT NOT NULL, hash TEXT NOT NULL,
             name TEXT NOT NULL, size INTEGER NOT NULL, category TEXT NOT NULL
         );
         CREATE INDEX IF NOT EXISTS events_ts ON events(ts);",
    )?;
    Ok(db)
}

impl Stats {
    pub fn open(path: &Path, download_dir: PathBuf) -> anyhow::Result<Self> {
        Ok(Self {
            inner: Mutex::new(Inner {
                db: open_db(path)?,
                live: VecDeque::with_capacity(LIVE_SECONDS),
                counters: HashMap::new(),
                acc: MinuteAcc::default(),
                system: SystemSample::default(),
                session_dl: 0,
                session_ul: 0,
            }),
            started_at: now_unix(),
            download_dir,
        })
    }

    pub fn download_dir(&self) -> &Path {
        &self.download_dir
    }

    pub fn set_system(&self, s: SystemSample) {
        self.inner.lock().unwrap().system = s;
    }

    /// Called once per second with the current torrent list.
    pub fn record(&self, torrents: &[TorrentView]) {
        let now = now_unix();
        let mut g = self.inner.lock().unwrap();
        let inner = &mut *g;

        // Bytes moved since the last sample, from per-torrent cumulative counters.
        // A torrent seen for the first time contributes nothing (its history
        // predates this sample).
        let (mut dl_delta, mut ul_delta) = (0u64, 0u64);
        let mut seen = HashMap::with_capacity(torrents.len());
        for t in torrents {
            if let Some(&(pd, pu)) = inner.counters.get(&t.hash) {
                dl_delta += t.downloaded.saturating_sub(pd);
                ul_delta += t.uploaded.saturating_sub(pu);
            }
            seen.insert(t.hash.clone(), (t.downloaded, t.uploaded));
        }
        inner.counters = seen;
        inner.session_dl += dl_delta;
        inner.session_ul += ul_delta;

        let sample = LiveSample {
            t: now,
            dl: torrents.iter().map(|t| t.download_speed).sum(),
            ul: torrents.iter().map(|t| t.upload_speed).sum(),
            peers: torrents.iter().map(|t| t.peers).sum(),
            active: torrents
                .iter()
                .filter(|t| t.download_speed > 0 || t.upload_speed > 0)
                .count() as u32,
            cpu: inner.system.cpu,
            mem: inner.system.mem,
        };
        if inner.live.len() == LIVE_SECONDS {
            inner.live.pop_front();
        }
        inner.live.push_back(sample);

        let minute = now - now.rem_euclid(60);
        if inner.acc.minute != minute {
            if inner.acc.n > 0
                && let Err(e) = flush(&inner.db, &inner.acc)
            {
                tracing::warn!("writing stats: {e:#}");
            }
            inner.acc = MinuteAcc {
                minute,
                ..Default::default()
            };
        }
        let a = &mut inner.acc;
        a.dl_bytes += dl_delta;
        a.ul_bytes += ul_delta;
        a.dl_peak = a.dl_peak.max(sample.dl);
        a.ul_peak = a.ul_peak.max(sample.ul);
        a.peers_sum += sample.peers as u64;
        a.active_sum += sample.active as u64;
        a.torrents = torrents.len() as u32;
        a.cpu_sum += sample.cpu as f64;
        a.mem_max = a.mem_max.max(sample.mem);
        a.n += 1;
    }

    pub fn event(&self, kind: &str, hash: &str, name: &str, size: u64, category: &str) {
        let g = self.inner.lock().unwrap();
        if let Err(e) = g.db.execute(
            "INSERT INTO events (ts, kind, hash, name, size, category) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![now_unix(), kind, hash, name, size as i64, category],
        ) {
            tracing::warn!("writing event: {e}");
        }
    }

    pub fn session_and_system(&self) -> anyhow::Result<(u64, u64, SystemSample)> {
        let g = self.inner.lock().unwrap();
        Ok((g.session_dl, g.session_ul, g.system))
    }

    pub fn live(&self) -> Vec<LiveSample> {
        self.inner.lock().unwrap().live.iter().copied().collect()
    }

    pub fn history(&self, range: Range) -> anyhow::Result<History> {
        let (span, bucket) = range.secs_and_bucket();
        let to = now_unix();
        let from = to - span;
        let g = self.inner.lock().unwrap();
        let mut stmt = g.db.prepare_cached(
            "SELECT (ts / ?1) * ?1 AS b,
                    SUM(dl_bytes), SUM(ul_bytes), MAX(dl_peak), MAX(ul_peak),
                    AVG(peers), AVG(active), AVG(cpu), MAX(mem)
             FROM minute WHERE ts >= ?2 GROUP BY b ORDER BY b",
        )?;
        let buckets = stmt
            .query_map(params![bucket, from - from.rem_euclid(bucket)], |r| {
                let dl: i64 = r.get(1)?;
                let ul: i64 = r.get(2)?;
                Ok(Bucket {
                    t: r.get(0)?,
                    dl_bytes: dl as u64,
                    ul_bytes: ul as u64,
                    dl_avg: dl as f64 / bucket as f64,
                    ul_avg: ul as f64 / bucket as f64,
                    dl_peak: r.get::<_, i64>(3)? as u64,
                    ul_peak: r.get::<_, i64>(4)? as u64,
                    peers: r.get(5)?,
                    active: r.get(6)?,
                    cpu: r.get(7)?,
                    mem: r.get::<_, i64>(8)? as u64,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(History {
            range: range.label().into(),
            bucket_secs: bucket,
            from,
            to,
            buckets,
        })
    }

    pub fn events(&self, limit: u32) -> anyhow::Result<Vec<Event>> {
        let g = self.inner.lock().unwrap();
        let mut stmt = g.db.prepare_cached(
            "SELECT ts, kind, hash, name, size, category FROM events ORDER BY ts DESC, id DESC LIMIT ?1",
        )?;
        let rows = stmt
            .query_map([limit], |r| {
                Ok(Event {
                    t: r.get(0)?,
                    kind: r.get(1)?,
                    hash: r.get(2)?,
                    name: r.get(3)?,
                    size: r.get::<_, i64>(4)? as u64,
                    category: r.get(5)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    pub fn summary(&self, utc_offset_secs: i64) -> anyhow::Result<Summary> {
        let now = now_unix();
        let local_now = now + utc_offset_secs;
        let today_start = local_now - local_now.rem_euclid(86400) - utc_offset_secs;
        let g = self.inner.lock().unwrap();
        let db = &g.db;

        // Include the in-progress minute so numbers move in real time.
        let (acc_dl, acc_ul) = (g.acc.dl_bytes, g.acc.ul_bytes);

        let sums = |from: i64| -> rusqlite::Result<(u64, u64)> {
            db.query_row(
                "SELECT COALESCE(SUM(dl_bytes),0), COALESCE(SUM(ul_bytes),0) FROM minute WHERE ts >= ?1",
                [from],
                |r| Ok((r.get::<_, i64>(0)? as u64, r.get::<_, i64>(1)? as u64)),
            )
        };
        let (all_dl, all_ul) = sums(0)?;
        let (today_dl, today_ul) = sums(today_start)?;
        let (peak_dl, peak_ul, since): (i64, i64, Option<i64>) = db.query_row(
            "SELECT COALESCE(MAX(dl_peak),0), COALESCE(MAX(ul_peak),0), MIN(ts) FROM minute",
            [],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )?;
        let count = |kind: &str, from: i64| -> rusqlite::Result<u64> {
            db.query_row(
                "SELECT COUNT(*) FROM events WHERE kind = ?1 AND ts >= ?2",
                params![kind, from],
                |r| r.get::<_, i64>(0).map(|v| v as u64),
            )
        };
        let week_ago = now - 7 * 86400;

        // Local calendar days for the last 30 days.
        let mut stmt = db.prepare_cached(
            "SELECT ((ts + ?1) / 86400) * 86400 - ?1 AS d, SUM(dl_bytes), SUM(ul_bytes)
             FROM minute WHERE ts >= ?2 GROUP BY d ORDER BY d",
        )?;
        let daily = stmt
            .query_map(params![utc_offset_secs, today_start - 29 * 86400], |r| {
                Ok(Day {
                    day: r.get(0)?,
                    dl_bytes: r.get::<_, i64>(1)? as u64,
                    ul_bytes: r.get::<_, i64>(2)? as u64,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;
        let busiest_day = db
            .query_row(
                "SELECT ((ts + ?1) / 86400) * 86400 - ?1 AS d, SUM(dl_bytes) AS s, SUM(ul_bytes)
                 FROM minute GROUP BY d ORDER BY s DESC LIMIT 1",
                [utc_offset_secs],
                |r| {
                    Ok(Day {
                        day: r.get(0)?,
                        dl_bytes: r.get::<_, i64>(1)? as u64,
                        ul_bytes: r.get::<_, i64>(2)? as u64,
                    })
                },
            )
            .ok();

        // Average download rate by local hour of day over the last 30 days.
        let mut hourly_profile = vec![0.0; 24];
        let mut stmt = db.prepare_cached(
            "SELECT ((ts + ?1) % 86400) / 3600 AS h, AVG(dl_bytes) / 60.0
             FROM minute WHERE ts >= ?2 GROUP BY h",
        )?;
        for row in stmt.query_map(params![utc_offset_secs, now - 30 * 86400], |r| {
            Ok((r.get::<_, i64>(0)?, r.get::<_, f64>(1)?))
        })? {
            let (h, v) = row?;
            if let Some(slot) = hourly_profile.get_mut(h as usize) {
                *slot = v;
            }
        }

        Ok(Summary {
            session: Totals {
                dl_bytes: g.session_dl,
                ul_bytes: g.session_ul,
            },
            today: Totals {
                dl_bytes: today_dl + acc_dl,
                ul_bytes: today_ul + acc_ul,
            },
            all_time: Totals {
                dl_bytes: all_dl + acc_dl,
                ul_bytes: all_ul + acc_ul,
            },
            recording_since: since,
            uptime_secs: now - self.started_at,
            peak_dl: peak_dl as u64,
            peak_ul: peak_ul as u64,
            completed_total: count("completed", 0)?,
            completed_7d: count("completed", week_ago)?,
            added_7d: count("added", week_ago)?,
            busiest_day,
            daily,
            hourly_profile,
            system: g.system,
        })
    }

    /// Fill an empty database with 30 days of plausible history (demo mode).
    pub fn backfill_demo(&self, names: &[(String, String, u64)]) -> anyhow::Result<()> {
        let mut g = self.inner.lock().unwrap();
        let empty: i64 = g.db.query_row("SELECT COUNT(*) FROM minute", [], |r| r.get(0))?;
        if empty > 0 {
            return Ok(());
        }
        let now = now_unix();
        let start = now - now.rem_euclid(60) - 30 * 86400;
        let tx = g.db.transaction()?;
        {
            let mut ins = tx.prepare(
                "INSERT INTO minute (ts, dl_bytes, ul_bytes, dl_peak, ul_peak, peers, active, torrents, cpu, mem)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
            )?;
            const MIB: f64 = 1024.0 * 1024.0;
            let mut burst = 0i64;
            let mut level = 0.0f64;
            let mut ul_level = 0.5 * MIB;
            let mut ts = start;
            while ts < now - 60 {
                let day = (ts - start) / 86400;
                let hour = ((ts % 86400) / 3600) as f64;
                // Evenings and overnight are busiest; weekends busier still.
                let diurnal = 0.55 + 0.45 * ((hour - 21.0) / 24.0 * std::f64::consts::TAU).cos();
                let weekend = if matches!(day % 7, 5 | 6) { 1.35 } else { 1.0 };
                if burst == 0 && rand::random_bool(0.0008) {
                    burst = rand::random_range(20..180);
                }
                // Roughly a heavy home user: ~50-150 GB/day with the odd big burst.
                let target = if burst > 0 {
                    burst -= 1;
                    rand::random_range(8.0..30.0) * MIB
                } else if rand::random_bool(0.35) {
                    rand::random_range(0.0..0.5) * MIB
                } else {
                    rand::random_range(0.5..5.0) * MIB * diurnal * weekend
                };
                level += (target - level) * 0.3;
                let dl = (level * 60.0) as i64;
                // Uploads wander on their own, loosely following how much is seeding.
                ul_level += (rand::random_range(0.05..1.6) * MIB * (0.5 + diurnal * 0.6) - ul_level) * 0.08;
                let ul_rate = (ul_level * rand::random_range(0.7..1.3)).max(0.0);
                let ul = (ul_rate * 60.0) as i64;
                let peers = level / MIB * 4.0 + rand::random_range(8.0..30.0);
                let active = (level / MIB / 3.0).clamp(1.0, 9.0) + rand::random_range(2.0..6.0);
                let cpu = (1.5 + level / MIB * 0.35 + rand::random_range(0.0..1.5)) as f32;
                let mem = (95.0 * MIB + level * 4.0 + rand::random_range(0.0..20.0) * MIB) as i64;
                ins.execute(params![
                    ts,
                    dl,
                    ul,
                    (level * rand::random_range(1.05..1.4)) as i64,
                    (ul_rate * rand::random_range(1.1..1.8)) as i64,
                    peers,
                    active,
                    24,
                    cpu,
                    mem
                ])?;
                ts += 60;
            }
            let mut ev = tx.prepare(
                "INSERT INTO events (ts, kind, hash, name, size, category) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            )?;
            for (i, (hash, name, size)) in names.iter().enumerate() {
                let added = now - rand::random_range(3600..30 * 86400);
                let cat = name.split('|').nth(1).unwrap_or("");
                let name = name.split('|').next().unwrap_or(name);
                ev.execute(params![added, "added", hash, name, *size as i64, cat])?;
                if i % 3 != 0 {
                    let done = (added + rand::random_range(600..6 * 3600)).min(now - 60);
                    ev.execute(params![done, "completed", hash, name, *size as i64, cat])?;
                }
            }
        }
        tx.commit()?;
        Ok(())
    }
}

fn flush(db: &Connection, a: &MinuteAcc) -> rusqlite::Result<()> {
    let n = a.n.max(1) as f64;
    db.execute(
        "INSERT INTO minute (ts, dl_bytes, ul_bytes, dl_peak, ul_peak, peers, active, torrents, cpu, mem)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)
         ON CONFLICT(ts) DO UPDATE SET
             dl_bytes = dl_bytes + excluded.dl_bytes, ul_bytes = ul_bytes + excluded.ul_bytes,
             dl_peak = MAX(dl_peak, excluded.dl_peak), ul_peak = MAX(ul_peak, excluded.ul_peak)",
        params![
            a.minute,
            a.dl_bytes as i64,
            a.ul_bytes as i64,
            a.dl_peak as i64,
            a.ul_peak as i64,
            a.peers_sum as f64 / n,
            a.active_sum as f64 / n,
            a.torrents,
            a.cpu_sum / n,
            a.mem_max as i64
        ],
    )?;
    Ok(())
}

/// Samples this process's CPU and memory, and free space on the download disk.
pub async fn run_system_sampler(stats: std::sync::Arc<Stats>) {
    use sysinfo::{Disks, ProcessesToUpdate, System};

    let Ok(pid) = sysinfo::get_current_pid() else {
        return;
    };
    let Ok(mut sys) = System::new() else { return };
    let Ok(mut disks) = Disks::new_with_refreshed_list() else {
        return;
    };
    let cores = std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(1) as f32;
    // Not canonicalize(): on Windows it yields a verbatim `\\?\C:\...` path, which
    // never prefix-matches a mount point like `C:\`.
    let dir = std::path::absolute(stats.download_dir())
        .unwrap_or_else(|_| stats.download_dir().to_path_buf());

    let mut iv = tokio::time::interval(std::time::Duration::from_secs(2));
    loop {
        iv.tick().await;
        sys.refresh_processes(ProcessesToUpdate::Some(&[pid]), false);
        disks.refresh(false);
        let (cpu, mem) = sys
            .process(pid)
            .map(|p| (p.cpu_usage() / cores, p.memory()))
            .unwrap_or_default();
        // The disk whose mount point is the longest prefix of the download dir.
        let disk = disks
            .iter()
            .filter(|d| dir.starts_with(d.mount_point()))
            .max_by_key(|d| d.mount_point().as_os_str().len());
        stats.set_system(SystemSample {
            cpu,
            mem,
            disk_free: disk.map(|d| d.available_space()).unwrap_or(0),
            disk_total: disk.map(|d| d.total_space()).unwrap_or(0),
        });
    }
}
