//! Moving a torrent's files to another folder without losing anything.
//!
//! A move is journalled before the first file is touched, so a crash or power
//! cut at any point leaves something [`recover`] can finish or undo on the next
//! start. The rules that make it safe:
//!
//! - Nothing at the destination is ever overwritten; a clash stops the move before it starts.
//! - Files are renamed when source and destination share a disk, which is
//!   instant. Otherwise they're copied and checked, and the originals are only
//!   deleted once the torrent is running from the new folder.
//! - Only directories that end up empty are removed.
//!
//! What's on disk says how far a move got: a file only at the destination was
//! renamed, one in both places was copied. That's all undo and cleanup need,
//! whether they run straight after a failure or after a restart.
//!
//! One exception: the destination can be the *same* folder under another name
//! (two mounts of one share, say `/mnt/a` and `/mnt/b`). Then every file is "in
//! both places" and deleting either copy deletes the only one, so such a move
//! (`Journal::same_folder`) never touches a file: the torrent is only pointed
//! at the new name.

use std::{
    fs,
    io::{Read, Write},
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};

use anyhow::Context;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Phase {
    /// Files are moving, or moved but the torrent still points at `from`.
    Files,
    /// The torrent runs from `to`; only the copied originals are left to delete.
    Cleanup,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct Journal {
    pub info_hash: String,
    pub from: PathBuf,
    pub to: PathBuf,
    /// The torrent's files, relative to its folder.
    pub files: Vec<PathBuf>,
    pub paused: bool,
    pub only_files: Option<Vec<usize>>,
    /// `from` is the torrent's own folder, so remove it once it's empty.
    pub prune_from: bool,
    pub phase: Phase,
    /// `from` and `to` are one folder under two names: switch, don't move.
    #[serde(default)]
    pub same_folder: bool,
}

/// Journals live in `<state dir>/moves`: `<hash>.json`, plus the torrent
/// itself and a copy of its resume data, so it can be restored whatever happens.
pub(crate) struct Journals {
    dir: PathBuf,
}

impl Journals {
    pub fn new(state_dir: &Path) -> Self {
        Self { dir: state_dir.join("moves") }
    }

    fn path(&self, hash: &str, ext: &str) -> PathBuf {
        self.dir.join(format!("{hash}.{ext}"))
    }

    pub fn torrent(&self, hash: &str) -> anyhow::Result<Vec<u8>> {
        fs::read(self.path(hash, "torrent")).context("reading the saved torrent")
    }

    /// Record a move before it starts. `resume` is librqbit's `.bitv` file.
    pub fn begin(&self, j: &Journal, torrent: &[u8], resume: &Path) -> anyhow::Result<()> {
        fs::create_dir_all(&self.dir).with_context(|| format!("creating {}", self.dir.display()))?;
        fs::write(self.path(&j.info_hash, "torrent"), torrent).context("saving the torrent")?;
        if resume.exists() {
            fs::copy(resume, self.path(&j.info_hash, "bitv")).context("saving resume data")?;
        }
        self.write(j)
    }

    pub fn write(&self, j: &Journal) -> anyhow::Result<()> {
        let tmp = self.path(&j.info_hash, "json.tmp");
        fs::write(&tmp, serde_json::to_vec_pretty(j)?).context("writing the move journal")?;
        fs::rename(&tmp, self.path(&j.info_hash, "json")).context("writing the move journal")
    }

    /// Put the saved resume data back (librqbit deletes it with the torrent).
    pub fn restore_resume(&self, hash: &str, resume: &Path) {
        let saved = self.path(hash, "bitv");
        if saved.exists()
            && let Err(e) = fs::copy(&saved, resume)
        {
            tracing::warn!("restoring resume data for {hash}: {e}");
        }
    }

    pub fn finish(&self, hash: &str) {
        for ext in ["json", "torrent", "bitv"] {
            let _ = fs::remove_file(self.path(hash, ext));
        }
    }

    pub fn list(&self) -> Vec<Journal> {
        let Ok(dir) = fs::read_dir(&self.dir) else { return Vec::new() };
        dir.flatten()
            .filter(|e| e.path().extension().is_some_and(|x| x == "json"))
            .filter_map(|e| {
                let j = fs::read(e.path()).ok().and_then(|b| serde_json::from_slice(&b).ok());
                if j.is_none() {
                    tracing::warn!("ignoring unreadable move journal {}", e.path().display());
                }
                j
            })
            .collect()
    }
}

/// Paths are the same folder (ignoring trailing slashes and `.` parts).
pub fn same_dir(a: &Path, b: &Path) -> bool {
    let norm = |p: &Path| p.components().collect::<PathBuf>();
    if norm(a) == norm(b) {
        return true;
    }
    matches!((fs::canonicalize(a), fs::canonicalize(b)), (Ok(x), Ok(y)) if x == y)
}

/// `a` and `b` are one folder reached by two paths that `canonicalize` can't
/// tie together, such as two mounts of the same share. Found out by leaving a
/// note in `a` and looking for it in `b`. False if either is missing or `a`
/// can't be written to.
pub fn same_folder_elsewhere(a: &Path, b: &Path) -> bool {
    if !a.is_dir() || !b.is_dir() || same_dir(a, b) {
        return false;
    }
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_nanos());
    let name = format!(".swarmy-same-folder-{}-{nanos}", std::process::id());
    let note = a.join(&name);
    if fs::OpenOptions::new().write(true).create_new(true).open(&note).is_err() {
        return false;
    }
    let seen = b.join(&name).exists();
    let _ = fs::remove_file(&note);
    seen
}

/// Refuse a move that would overwrite something, before anything is touched.
pub(crate) fn check_targets(j: &Journal) -> anyhow::Result<()> {
    anyhow::ensure!(!same_dir(&j.from, &j.to), "The torrent is already in that folder.");
    for rel in &j.files {
        anyhow::ensure!(
            rel.components().all(|c| matches!(c, std::path::Component::Normal(_))),
            "The torrent has an unsafe file name: {}",
            rel.display()
        );
        if j.same_folder {
            continue; // Its files are already "there": they're the same files.
        }
        let dst = j.to.join(rel);
        if j.from.join(rel).exists() && dst.exists() {
            anyhow::bail!("{} already exists. Swarmy won't overwrite it.", dst.display());
        }
    }
    Ok(())
}

/// Bytes of the torrent's files that are on disk (the move's 100%).
pub(crate) fn bytes_present(j: &Journal) -> u64 {
    j.files.iter().filter_map(|rel| fs::metadata(j.from.join(rel)).ok()).map(|m| m.len()).sum()
}

/// Move every file that exists. On failure everything is put back first.
pub(crate) fn move_files(j: &Journal, done: &AtomicU64) -> anyhow::Result<()> {
    if j.same_folder {
        done.store(bytes_present(j), Ordering::Relaxed);
        return Ok(());
    }
    move_files_with(j, done, |a, b| fs::rename(a, b))
}

/// [`move_files`] with the rename step swappable, so tests can force copies.
fn move_files_with(j: &Journal, done: &AtomicU64, rename: impl Fn(&Path, &Path) -> std::io::Result<()>) -> anyhow::Result<()> {
    let result = j.files.iter().try_for_each(|rel| {
        let (src, dst) = (j.from.join(rel), j.to.join(rel));
        let Ok(meta) = fs::metadata(&src) else {
            return Ok(()); // Not downloaded yet.
        };
        if let Some(parent) = dst.parent() {
            fs::create_dir_all(parent).with_context(|| format!("creating {}", parent.display()))?;
        }
        if rename(&src, &dst).is_ok() {
            done.fetch_add(meta.len(), Ordering::Relaxed);
            return Ok(());
        }
        // Another disk (or a rename the filesystem won't do): copy instead.
        copy_file(&src, &dst, done).with_context(|| format!("copying {} to {}", src.display(), dst.display()))
    });
    if let Err(e) = result {
        if let Err(undo_err) = undo(j) {
            tracing::error!("undoing a failed move of {}: {undo_err:#}", j.info_hash);
        }
        return Err(e);
    }
    Ok(())
}

/// Copy with progress, then check the size. A partial copy is removed.
fn copy_file(src: &Path, dst: &Path, done: &AtomicU64) -> anyhow::Result<()> {
    let attempt = || -> anyhow::Result<()> {
        let mut from = fs::File::open(src)?;
        let meta = from.metadata()?;
        let mut to = fs::OpenOptions::new().write(true).create_new(true).open(dst)?;
        let mut buf = vec![0u8; 1 << 20];
        loop {
            let n = from.read(&mut buf)?;
            if n == 0 {
                break;
            }
            to.write_all(&buf[..n])?;
            done.fetch_add(n as u64, Ordering::Relaxed);
        }
        to.sync_all()?;
        anyhow::ensure!(to.metadata()?.len() == meta.len(), "the copy came out the wrong size");
        if let Ok(modified) = meta.modified() {
            let _ = to.set_modified(modified);
        }
        Ok(())
    };
    attempt().inspect_err(|_| {
        let _ = fs::remove_file(dst);
    })
}

/// Put a move back: renamed files return, copies are deleted.
pub(crate) fn undo(j: &Journal) -> anyhow::Result<()> {
    if j.same_folder {
        return Ok(()); // Nothing moved.
    }
    let mut first_error = None;
    for rel in &j.files {
        let (src, dst) = (j.from.join(rel), j.to.join(rel));
        let step = match (src.exists(), dst.exists()) {
            (false, true) => (|| {
                if let Some(parent) = src.parent() {
                    fs::create_dir_all(parent)?;
                }
                if fs::rename(&dst, &src).is_err() {
                    copy_file(&dst, &src, &AtomicU64::new(0))?;
                    fs::remove_file(&dst)?;
                }
                anyhow::Ok(())
            })(),
            (true, true) => fs::remove_file(&dst).map_err(Into::into),
            _ => Ok(()),
        };
        if let Err(e) = step {
            first_error.get_or_insert(e.context(format!("putting back {}", src.display())));
        }
    }
    prune(&j.to, &j.files, j.prune_from);
    first_error.map_or(Ok(()), Err)
}

/// After a successful move: delete the originals of copied files.
pub(crate) fn cleanup(j: &Journal) {
    if j.same_folder {
        return; // The "originals" are the files themselves.
    }
    for rel in &j.files {
        let (src, dst) = (j.from.join(rel), j.to.join(rel));
        if src.exists() && dst.exists()
            && let Err(e) = fs::remove_file(&src)
        {
            tracing::warn!("removing {} after moving it: {e}", src.display());
        }
    }
    prune(&j.from, &j.files, j.prune_from);
}

/// Remove the files' folders under `root` that are now empty (and `root`
/// itself if asked). Never touches a folder with anything left in it.
fn prune(root: &Path, files: &[PathBuf], include_root: bool) {
    for rel in files {
        let mut dir = root.join(rel);
        while dir.pop() && dir.starts_with(root) && dir != root {
            let _ = fs::remove_dir(&dir);
        }
    }
    if include_root {
        let _ = fs::remove_dir(root);
    }
}

/// librqbit's record of where a torrent's files are, from its `session.json`.
fn registered_folder(state_dir: &Path, hash: &str) -> Option<PathBuf> {
    let db: serde_json::Value = serde_json::from_slice(&fs::read(state_dir.join("session.json")).ok()?).ok()?;
    db.get("torrents")?.as_object()?.values().find_map(|t| {
        if t.get("info_hash")?.as_str()? != hash {
            return None;
        }
        Some(PathBuf::from(t.get("output_folder")?.as_str()?))
    })
}

/// Settle moves interrupted by a crash. Runs before the session starts, so no
/// torrent ever sees half-moved files. Moves that got as far as the new
/// folder are finished; the rest are undone. Returns the torrents the session
/// no longer has (it stopped between removing and re-adding them), to add back.
pub(crate) fn recover(journals: &Journals, state_dir: &Path) -> Vec<Journal> {
    let mut readd = Vec::new();
    for j in journals.list() {
        let at = registered_folder(state_dir, &j.info_hash);
        if j.phase == Phase::Cleanup || at.as_deref().is_some_and(|p| same_dir(p, &j.to)) {
            tracing::info!("finishing an interrupted move of {} to {}", j.info_hash, j.to.display());
            cleanup(&j);
            journals.finish(&j.info_hash);
            continue;
        }
        tracing::info!("undoing an interrupted move of {} to {}", j.info_hash, j.to.display());
        if let Err(e) = undo(&j) {
            tracing::error!("{e:#}");
        }
        journals.restore_resume(&j.info_hash, &state_dir.join(format!("{}.bitv", j.info_hash)));
        if at.is_none() {
            readd.push(j); // The journal goes once it's re-added.
        } else {
            journals.finish(&j.info_hash);
        }
    }
    readd
}

#[cfg(test)]
mod tests {
    use super::*;

    fn journal(root: &Path, files: &[&str]) -> Journal {
        Journal {
            info_hash: "ab".repeat(20),
            from: root.join("old/Show"),
            to: root.join("new/Show"),
            files: files.iter().map(PathBuf::from).collect(),
            paused: false,
            only_files: None,
            prune_from: true,
            phase: Phase::Files,
            same_folder: false,
        }
    }

    #[test]
    fn same_folder_switch_never_touches_files() {
        let tmp = tempfile::tempdir().unwrap();
        let mut j = journal(tmp.path(), &["S01/e1.mkv", "e2.mkv"]);
        // Stand-in for one share under two names: the files are "in both places".
        for root in [&j.from, &j.to] {
            write(&root.join("S01/e1.mkv"), "one");
            write(&root.join("e2.mkv"), "two");
        }
        assert!(check_targets(&j).is_err(), "an ordinary move must refuse to overwrite");
        j.same_folder = true;
        check_targets(&j).unwrap();
        let done = AtomicU64::new(0);
        move_files(&j, &done).unwrap();
        assert_eq!(done.load(Ordering::Relaxed), 6);
        cleanup(&j);
        undo(&j).unwrap();
        for root in [&j.from, &j.to] {
            assert_eq!(fs::read_to_string(root.join("S01/e1.mkv")).unwrap(), "one");
            assert_eq!(fs::read_to_string(root.join("e2.mkv")).unwrap(), "two");
        }
    }

    #[test]
    fn tells_different_folders_apart() {
        let tmp = tempfile::tempdir().unwrap();
        let (a, b) = (tmp.path().join("a"), tmp.path().join("b"));
        fs::create_dir_all(&a).unwrap();
        fs::create_dir_all(&b).unwrap();
        assert!(!same_folder_elsewhere(&a, &b));
        assert!(!same_folder_elsewhere(&a, &a), "the same path is same_dir, not elsewhere");
        assert!(!same_folder_elsewhere(&a, &tmp.path().join("missing")));
        assert_eq!(fs::read_dir(&a).unwrap().count(), 0, "the note is cleaned up");
    }

    fn write(path: &Path, body: &str) {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, body).unwrap();
    }

    #[test]
    fn moves_files_and_tidies_up() {
        let tmp = tempfile::tempdir().unwrap();
        let j = journal(tmp.path(), &["S01/e1.mkv", "S01/e2.mkv", "info.nfo", "not-downloaded.mkv"]);
        write(&j.from.join("S01/e1.mkv"), "one");
        write(&j.from.join("S01/e2.mkv"), "two");
        write(&j.from.join("info.nfo"), "nfo");
        check_targets(&j).unwrap();
        assert_eq!(bytes_present(&j), 9);

        let done = AtomicU64::new(0);
        move_files(&j, &done).unwrap();
        cleanup(&j);
        assert_eq!(done.load(Ordering::Relaxed), 9);
        assert_eq!(fs::read_to_string(j.to.join("S01/e2.mkv")).unwrap(), "two");
        assert!(!j.from.exists(), "the emptied torrent folder goes");
        assert!(tmp.path().join("old").exists(), "but not the folder above it");
    }

    /// As if the destination were another disk: every rename fails.
    fn no_rename(_: &Path, _: &Path) -> std::io::Result<()> {
        Err(std::io::Error::new(std::io::ErrorKind::CrossesDevices, "other disk"))
    }

    #[test]
    fn copies_across_disks_and_deletes_originals_only_at_the_end() {
        let tmp = tempfile::tempdir().unwrap();
        let j = journal(tmp.path(), &["S01/e1.mkv", "big.bin"]);
        write(&j.from.join("S01/e1.mkv"), "one");
        let big: Vec<u8> = (0..3_000_000u32).map(|i| i as u8).collect();
        fs::create_dir_all(&j.from).unwrap();
        fs::write(j.from.join("big.bin"), &big).unwrap();

        let done = AtomicU64::new(0);
        move_files_with(&j, &done, no_rename).unwrap();
        assert_eq!(done.load(Ordering::Relaxed), 3 + big.len() as u64);
        // Copied, but the originals stay until the torrent runs from the new folder.
        assert!(j.from.join("big.bin").exists());
        assert_eq!(fs::read(j.to.join("big.bin")).unwrap(), big);
        cleanup(&j);
        assert!(!j.from.exists());
        assert_eq!(fs::read_to_string(j.to.join("S01/e1.mkv")).unwrap(), "one");
    }

    #[test]
    fn a_failed_copy_puts_everything_back() {
        let tmp = tempfile::tempdir().unwrap();
        let j = journal(tmp.path(), &["a.mkv", "sub/b.mkv"]);
        write(&j.from.join("a.mkv"), "a");
        write(&j.from.join("sub/b.mkv"), "b");
        // b's destination folder can't be created: a file is in the way.
        write(&j.to.join("sub"), "in the way");

        let err = move_files_with(&j, &AtomicU64::new(0), no_rename).unwrap_err();
        assert!(format!("{err:#}").contains("creating"), "{err:#}");
        assert_eq!(fs::read_to_string(j.from.join("a.mkv")).unwrap(), "a");
        assert_eq!(fs::read_to_string(j.from.join("sub/b.mkv")).unwrap(), "b");
        assert!(!j.to.join("a.mkv").exists(), "the copy of a was removed");
        assert_eq!(fs::read_to_string(j.to.join("sub")).unwrap(), "in the way", "and nothing else was touched");
    }

    #[test]
    fn never_overwrites() {
        let tmp = tempfile::tempdir().unwrap();
        let j = journal(tmp.path(), &["a.mkv"]);
        write(&j.from.join("a.mkv"), "mine");
        write(&j.to.join("a.mkv"), "someone else's");
        assert!(check_targets(&j).is_err());
    }

    #[test]
    fn undo_restores_renamed_and_drops_copies() {
        let tmp = tempfile::tempdir().unwrap();
        let j = journal(tmp.path(), &["a.mkv", "b.mkv"]);
        // a was renamed, b was copied, as a crash mid-move would leave them.
        write(&j.to.join("a.mkv"), "a");
        write(&j.from.join("b.mkv"), "b");
        write(&j.to.join("b.mkv"), "b");
        undo(&j).unwrap();
        assert_eq!(fs::read_to_string(j.from.join("a.mkv")).unwrap(), "a");
        assert_eq!(fs::read_to_string(j.from.join("b.mkv")).unwrap(), "b");
        assert!(!j.to.exists());
    }

    #[test]
    fn keeps_folders_with_other_things_in_them() {
        let tmp = tempfile::tempdir().unwrap();
        let j = journal(tmp.path(), &["S01/e1.mkv"]);
        write(&j.from.join("S01/e1.mkv"), "one");
        write(&j.from.join("S01/subs.srt"), "not ours");
        move_files(&j, &AtomicU64::new(0)).unwrap();
        cleanup(&j);
        assert!(j.from.join("S01/subs.srt").exists());
    }

    #[test]
    fn recovery_finishes_or_undoes() {
        let tmp = tempfile::tempdir().unwrap();
        let state = tmp.path().join("state");
        let journals = Journals::new(&state);
        let j = journal(tmp.path(), &["a.mkv"]);
        write(&j.to.join("a.mkv"), "a");

        // The session says the torrent is in the new folder: finish.
        fs::create_dir_all(&state).unwrap();
        let db = serde_json::json!({ "torrents": { "0": { "info_hash": j.info_hash, "output_folder": j.to } } });
        fs::write(state.join("session.json"), db.to_string()).unwrap();
        write(&j.from.join("a.mkv"), "a"); // a copied original, not yet deleted
        journals.begin(&j, b"torrent", &state.join("none.bitv")).unwrap();
        assert!(recover(&journals, &state).is_empty());
        assert!(!j.from.join("a.mkv").exists() && j.to.join("a.mkv").exists());
        assert!(journals.list().is_empty());

        // The session lost it mid-swap: undo, and hand it back for re-adding.
        fs::write(state.join("session.json"), r#"{"torrents":{}}"#).unwrap();
        journals.begin(&j, b"torrent", &state.join("none.bitv")).unwrap();
        let readd = recover(&journals, &state);
        assert_eq!(readd.len(), 1);
        assert!(j.from.join("a.mkv").exists() && !j.to.join("a.mkv").exists());
    }
}
