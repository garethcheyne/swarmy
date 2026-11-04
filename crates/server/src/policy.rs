//! Rules Swarmy enforces on top of the engine, run every tick:
//!
//! - speed limits, with alternative limits switched by hand or on a schedule,
//! - seeding limits (ratio / time → pause or remove),
//! - queueing (max active downloads, uploads and torrents).

use std::sync::Mutex;

use chrono::{Datelike, Local, Timelike};
use swarmy_engine::TorrentState;

use crate::{
    state::{Shared, TorrentView},
    store::{SeedAction, Settings, now_unix},
};

#[derive(Default)]
pub struct PolicyState {
    applied_limits: Mutex<Option<(Option<u32>, Option<u32>)>>,
    tick: Mutex<u64>,
}

/// Is the alternative-speed schedule active right now (server local time)?
pub fn schedule_active(s: &Settings) -> bool {
    let sch = &s.speed.schedule;
    if !sch.enabled {
        return false;
    }
    let now = Local::now();
    let minute = (now.hour() * 60 + now.minute()) as u16;
    let today = now.weekday().num_days_from_monday() as u8;
    let yesterday = (today + 6) % 7;
    if sch.from <= sch.to {
        sch.days.contains(&today) && minute >= sch.from && minute < sch.to
    } else {
        // Overnight window, e.g. 22:00-07:00: the evening part belongs to today,
        // the early-morning part to the day it started.
        (sch.days.contains(&today) && minute >= sch.from) || (sch.days.contains(&yesterday) && minute < sch.to)
    }
}

fn kib(v: u32) -> Option<u32> {
    (v > 0).then(|| v.saturating_mul(1024))
}

/// Apply all rules; returns whether alternative limits are in force.
pub async fn apply(state: &Shared, torrents: &[TorrentView]) -> bool {
    let settings = state.store.read(|d| d.settings.clone());

    // --- speed limits (every tick: cheap, and schedules flip on the minute)
    let alt = settings.speed.alt_enabled || schedule_active(&settings);
    let (down, up) = if alt {
        (kib(settings.speed.alt_download_limit), kib(settings.speed.alt_upload_limit))
    } else {
        (kib(settings.speed.download_limit), kib(settings.speed.upload_limit))
    };
    {
        let mut applied = state.policy.applied_limits.lock().unwrap();
        if *applied != Some((down, up)) {
            state.engine.set_rate_limits(down, up);
            *applied = Some((down, up));
        }
    }

    // Seeding limits and the queue change torrent state, so run them less often.
    let tick = {
        let mut t = state.policy.tick.lock().unwrap();
        *t += 1;
        *t
    };
    if tick % 2 != 0 {
        return alt;
    }
    if let Err(e) = seeding_limits(state, &settings, torrents).await {
        tracing::warn!("seeding limits: {e:#}");
    }
    if settings.queue.enabled
        && let Err(e) = queue(state, &settings, torrents).await
    {
        tracing::warn!("queue: {e:#}");
    }
    alt
}

async fn seeding_limits(state: &Shared, s: &Settings, torrents: &[TorrentView]) -> anyhow::Result<()> {
    let limits = &s.seeding;
    if limits.ratio_limit.is_none() && limits.time_limit.is_none() {
        return Ok(());
    }
    let now = now_unix();
    for t in torrents.iter().filter(|t| t.state == TorrentState::Seeding) {
        let ratio_hit = limits.ratio_limit.is_some_and(|r| t.ratio >= r);
        let time_hit = limits
            .time_limit
            .zip(t.completed_on)
            .is_some_and(|(mins, done)| now - done >= i64::from(mins) * 60);
        if !(ratio_hit || time_hit) {
            continue;
        }
        let why = if ratio_hit { format!("ratio {:.2}", t.ratio) } else { "seeding time".into() };
        match limits.action {
            SeedAction::Pause => {
                tracing::info!("seeding limit reached ({why}), pausing {}", t.name);
                state.engine.pause(&t.hash).await?;
            }
            SeedAction::Remove | SeedAction::RemoveWithFiles => {
                let files = limits.action == SeedAction::RemoveWithFiles;
                tracing::info!("seeding limit reached ({why}), removing {} (files: {files})", t.name);
                crate::api::delete_torrents(state, std::slice::from_ref(&t.hash), files)
                    .await
                    .map_err(|e| anyhow::anyhow!("{}", e.message()))?;
            }
        }
    }
    Ok(())
}

/// qBittorrent-style queueing. Torrents over a limit are paused and marked
/// `queued`; when a slot frees up the oldest queued one resumes. Torrents the
/// user paused (not queued) are left alone.
async fn queue(state: &Shared, s: &Settings, torrents: &[TorrentView]) -> anyhow::Result<()> {
    let q = &s.queue;
    let running = |t: &&TorrentView| matches!(t.state, TorrentState::Downloading | TorrentState::Initializing | TorrentState::Seeding);
    let is_dl = |t: &TorrentView| t.progress < 1.0;

    // Oldest first: they keep their slots, newer ones wait.
    let mut active: Vec<&TorrentView> = torrents.iter().filter(running).collect();
    active.sort_by_key(|t| t.added_on);
    let mut queued: Vec<&TorrentView> = torrents.iter().filter(|t| t.queued).collect();
    queued.sort_by_key(|t| t.added_on);

    let (mut dl, mut up, mut total) = (0u32, 0u32, 0u32);
    let mut to_queue = Vec::new();
    for t in &active {
        let over = if is_dl(t) { dl >= q.max_downloads } else { up >= q.max_uploads } || total >= q.max_active;
        if over {
            to_queue.push(t.hash.clone());
        } else {
            if is_dl(t) { dl += 1 } else { up += 1 }
            total += 1;
        }
    }

    let mut to_start = Vec::new();
    for t in &queued {
        let room = if is_dl(t) { dl < q.max_downloads } else { up < q.max_uploads } && total < q.max_active;
        if room {
            if is_dl(t) { dl += 1 } else { up += 1 }
            total += 1;
            to_start.push(t.hash.clone());
        }
    }

    if to_queue.is_empty() && to_start.is_empty() {
        return Ok(());
    }
    for h in &to_queue {
        state.engine.pause(h).await?;
    }
    for h in &to_start {
        state.engine.resume(h).await?;
    }
    state.store.update(|d| {
        for h in &to_queue {
            if let Some(m) = d.torrents.get_mut(h) {
                m.queued = true;
            }
        }
        for h in &to_start {
            if let Some(m) = d.torrents.get_mut(h) {
                m.queued = false;
            }
        }
    })?;
    Ok(())
}
