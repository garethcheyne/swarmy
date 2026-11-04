//! Moving torrents between folders, one at a time.
//!
//! Moves come from four places: the user (or Sonarr's `setLocation`), a
//! torrent finishing in the incomplete folder, a torrent changing category,
//! and a category's folder changing. They queue here and run one after another,
//! so a batch doesn't have every disk thrashing at once. The engine does the
//! careful part (see `swarmy_engine::Engine::relocate`); this keeps Swarmy's
//! own records in step.

use std::{
    collections::{HashSet, VecDeque},
    path::{Path, PathBuf},
    sync::Mutex,
};

use tokio::sync::Notify;

use crate::state::Shared;

struct Job {
    hash: String,
    /// The new save path; `None` = its recorded save path (an incomplete
    /// download that just finished).
    to: Option<PathBuf>,
    /// Whether it follows its category afterwards; `None` = unchanged. Only
    /// applied once the move succeeds, so a failed move changes nothing.
    auto: Option<bool>,
}

#[derive(Default)]
pub struct Mover {
    queue: Mutex<VecDeque<Job>>,
    current: Mutex<Option<String>>,
    wake: Notify,
}

impl Mover {
    /// Queue a move. A newer request for the same torrent replaces a waiting one.
    pub fn request(&self, hash: &str, to: Option<PathBuf>) {
        self.request_with(hash, to, None);
    }

    fn request_with(&self, hash: &str, to: Option<PathBuf>, auto: Option<bool>) {
        let mut q = self.queue.lock().unwrap();
        match q.iter_mut().find(|j| j.hash == hash) {
            Some(j) => {
                j.to = to;
                j.auto = auto.or(j.auto);
            }
            None => q.push_back(Job { hash: hash.to_owned(), to, auto }),
        }
        drop(q);
        self.wake.notify_one();
    }

    /// Torrents waiting to move or moving now.
    pub fn busy(&self) -> HashSet<String> {
        let mut s: HashSet<String> = self.queue.lock().unwrap().iter().map(|j| j.hash.clone()).collect();
        s.extend(self.current.lock().unwrap().clone());
        s
    }
}

pub async fn run(state: Shared) {
    loop {
        let job = state.mover.queue.lock().unwrap().pop_front();
        let Some(job) = job else {
            state.mover.wake.notified().await;
            continue;
        };
        *state.mover.current.lock().unwrap() = Some(job.hash.clone());
        let result = perform(&state, &job).await;
        *state.mover.current.lock().unwrap() = None;
        if let Err(e) = result {
            let why = format!("{e:#}");
            tracing::warn!("moving {}: {why}", job.hash);
            let _ = state.store.update(|d| {
                if let Some(m) = d.torrents.get_mut(&job.hash) {
                    m.move_error = Some(why.clone());
                }
            });
            let name = state.engine.status(&job.hash).and_then(|s| s.name).unwrap_or_else(|| job.hash.clone());
            state.notify("errored", &format!("Couldn't move {name}"), &why, &job.hash);
        }
    }
}

async fn perform(state: &Shared, job: &Job) -> anyhow::Result<()> {
    let (Some(status), Some(meta)) = (
        state.engine.status(&job.hash),
        state.store.read(|d| d.torrents.get(&job.hash).cloned()),
    ) else {
        return Ok(()); // Removed while it waited.
    };
    let target = job.to.clone().unwrap_or_else(|| PathBuf::from(&meta.save_path));
    let in_incomplete = !meta.download_path.is_empty();

    // Still downloading in the incomplete folder: it goes to the new place when done.
    if in_incomplete && !status.finished {
        state.store.update(|d| {
            if let Some(m) = d.torrents.get_mut(&job.hash) {
                m.save_path = target.to_string_lossy().into_owned();
                m.auto_managed = job.auto.unwrap_or(m.auto_managed);
            }
        })?;
        return Ok(());
    }

    // The torrent's own folder (if it has one) keeps its place under the save path.
    let current = if in_incomplete {
        PathBuf::from(&meta.download_path)
    } else if meta.save_path.is_empty() {
        status.output_folder.clone()
    } else {
        PathBuf::from(&meta.save_path)
    };
    let output = new_output(&status.output_folder, &current, &target);
    if !swarmy_engine::same_dir(&status.output_folder, &output) {
        tracing::info!("moving {} to {}", status.name.as_deref().unwrap_or(&job.hash), output.display());
        state.engine.relocate(&job.hash, output).await?;
        let name = status.name.as_deref().unwrap_or(&job.hash);
        state.stats.event("moved", &job.hash, name, status.total_bytes, &meta.category);
    }
    state.store.update(|d| {
        if let Some(m) = d.torrents.get_mut(&job.hash) {
            m.save_path = target.to_string_lossy().into_owned();
            m.download_path.clear();
            m.move_error = None;
            m.auto_managed = job.auto.unwrap_or(m.auto_managed);
        }
    })?;
    Ok(())
}

/// Where the torrent's folder goes: the same place relative to the new save path.
fn new_output(output: &Path, save: &Path, target: &Path) -> PathBuf {
    match output.strip_prefix(save) {
        Ok(rel) => target.join(rel),
        Err(_) => target.to_path_buf(),
    }
}

/// After torrents change category: move the ones that follow their category.
pub fn category_changed(state: &Shared, hashes: &[String]) {
    if !state.store.read(|d| d.settings.downloads.category_moves) {
        return;
    }
    for (hash, category) in state.store.read(|d| {
        hashes
            .iter()
            .filter_map(|h| d.torrents.get(h).filter(|m| m.auto_managed).map(|m| (h.clone(), m.category.clone())))
            .collect::<Vec<_>>()
    }) {
        state.mover.request(&hash, Some(state.resolve_save_path(None, &category)));
    }
}

/// After a category's folder changes: move the torrents that follow it.
pub fn category_folder_changed(state: &Shared, category: &str) {
    let hashes: Vec<String> = state.store.read(|d| {
        d.torrents.iter().filter(|(_, m)| m.category == category).map(|(h, _)| h.clone()).collect()
    });
    category_changed(state, &hashes);
}

/// Move torrents to `location` by hand (they stop following their category),
/// or back under their category's folder with `location = None`.
pub fn move_by_hand(state: &Shared, hashes: &[String], location: Option<&Path>) -> anyhow::Result<()> {
    if let Some(dir) = location {
        anyhow::ensure!(dir.is_absolute(), "Give the full path of the folder, like /data/torrents.");
        std::fs::create_dir_all(dir).map_err(|e| anyhow::anyhow!("Swarmy can't create {}: {e}", dir.display()))?;
    }
    let known: Vec<(String, String)> = state.store.read(|d| {
        hashes.iter().filter_map(|h| Some((h.clone(), d.torrents.get(h)?.category.clone()))).collect()
    });
    for (hash, category) in known {
        let to = location.map(Path::to_path_buf).unwrap_or_else(|| state.resolve_save_path(None, &category));
        state.mover.request_with(&hash, Some(to), Some(location.is_none()));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::new_output;
    use std::path::Path;

    #[test]
    fn keeps_the_torrents_folder() {
        let out = new_output(Path::new("/dl/incomplete/tv/Show.S01"), Path::new("/dl/incomplete/tv"), Path::new("/data/tv"));
        assert_eq!(out, Path::new("/data/tv/Show.S01"));
        // A single file lives straight in the save path.
        let out = new_output(Path::new("/dl/tv"), Path::new("/dl/tv"), Path::new("/data/tv"));
        assert_eq!(out, Path::new("/data/tv"));
    }
}
