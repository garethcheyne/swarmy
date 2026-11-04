//! After downloading: repair (PAR2), unpack (RAR, 7z), then move the result
//! into the completed folder, `<complete>/<category>/<name>`.
//!
//! Archive entries are checked before anything is written: absolute paths
//! and `..` are refused, so an archive can't write outside its folder.

use std::path::{Component, Path, PathBuf};

use anyhow::{Context, bail};

use crate::{downloader::JobStatus, nzb::sanitise, par2};

pub struct Job {
    /// The job's incomplete folder.
    pub dir: PathBuf,
    pub name: String,
    pub category: String,
    pub password: Option<String>,
    pub files: Vec<PathBuf>,
    /// Articles were missing or failed their CRC.
    pub damaged: bool,
    pub complete_dir: PathBuf,
}

pub struct Output {
    pub storage: PathBuf,
    pub message: Option<String>,
}

pub fn run(job: &Job, progress: &dyn Fn(JobStatus, Option<String>)) -> anyhow::Result<Output> {
    let mut message = None;
    let par2_index = job
        .files
        .iter()
        .filter(|p| p.extension().is_some_and(|e| e.eq_ignore_ascii_case("par2")))
        .min_by_key(|p| std::fs::metadata(p).map(|m| m.len()).unwrap_or(u64::MAX))
        .cloned();

    if job.damaged {
        let Some(index) = &par2_index else {
            bail!("Some of the download is missing or damaged, and it has no repair (PAR2) files");
        };
        progress(JobStatus::Repairing, Some("Checking and repairing".into()));
        let report = par2::repair(index).context("Repair failed")?;
        message = Some(report);
    } else if let Some(index) = &par2_index {
        // Everything arrived intact; a full PAR2 verify would only re-read
        // every byte. Note it and move on (SABnzbd's "quick check").
        let _ = index;
    }

    // Archives to unpack (first volumes only).
    let entries: Vec<PathBuf> = std::fs::read_dir(&job.dir)?
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.is_file())
        .collect();
    let archives: Vec<&PathBuf> = entries.iter().filter(|p| first_volume(p).is_some()).collect();

    let out = unique_dir(&job.complete_dir.join(category_dir(&job.category)).join(sanitise(&job.name)));
    std::fs::create_dir_all(&out).with_context(|| format!("creating {}", out.display()))?;

    let mut consumed: Vec<PathBuf> = Vec::new();
    if !archives.is_empty() {
        progress(JobStatus::Extracting, Some("Unpacking".into()));
        for a in &archives {
            match first_volume(a) {
                Some(Kind::Rar) => unrar(a, &out, job.password.as_deref()).with_context(|| format!("Unpacking {}", file_name(a)))?,
                Some(Kind::SevenZip) => un7z(a, &out, job.password.as_deref()).with_context(|| format!("Unpacking {}", file_name(a)))?,
                None => {}
            }
        }
        // Every volume of every archive we unpacked is now redundant.
        consumed.extend(entries.iter().filter(|p| is_archive_part(p)).cloned());
    }

    progress(JobStatus::Moving, None);
    for p in &entries {
        let name = file_name(p);
        let skip = consumed.contains(p)
            || name.eq_ignore_ascii_case("job.nzb")
            || p.extension().is_some_and(|e| e.eq_ignore_ascii_case("par2"))
            || name.ends_with(".1") && p.with_extension("").exists(); // par2 repair backups
        if skip {
            continue;
        }
        move_file(p, &out.join(&name))?;
    }
    if std::fs::read_dir(&out)?.next().is_none() {
        bail!("Nothing was left after unpacking");
    }
    Ok(Output { storage: out, message })
}

enum Kind {
    Rar,
    SevenZip,
}

/// The volume number in `name.part07.rar`, if it's named that way.
fn part_number(lower: &str) -> Option<u32> {
    let stem = lower.strip_suffix(".rar")?;
    let i = stem.rfind(".part")?;
    let digits = &stem[i + 5..];
    (!digits.is_empty() && digits.chars().all(|c| c.is_ascii_digit())).then(|| digits.parse().ok()).flatten()
}

/// Is this the file to start unpacking from?
fn first_volume(p: &Path) -> Option<Kind> {
    let n = file_name(p).to_ascii_lowercase();
    if let Some(k) = part_number(&n) {
        return (k == 1).then_some(Kind::Rar);
    }
    if n.ends_with(".rar") {
        return Some(Kind::Rar);
    }
    if n.ends_with(".7z") || n.ends_with(".7z.001") {
        return Some(Kind::SevenZip);
    }
    None
}

fn is_archive_part(p: &Path) -> bool {
    let n = file_name(p).to_ascii_lowercase();
    if n.ends_with(".rar") || n.ends_with(".7z") {
        return true;
    }
    // Old-style .r00, .r01 … and 7z splits .7z.001 …
    let ext = n.rsplit('.').next().unwrap_or("");
    (ext.len() == 3 && ext.starts_with('r') && ext[1..].chars().all(|c| c.is_ascii_digit()))
        || (n.contains(".7z.") && ext.chars().all(|c| c.is_ascii_digit()))
}

fn file_name(p: &Path) -> String {
    p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()
}

fn category_dir(c: &str) -> String {
    if c.trim().is_empty() { String::new() } else { sanitise(c) }
}

/// `dir`, or `dir (2)`, `dir (3)` … if it's taken.
fn unique_dir(dir: &Path) -> PathBuf {
    if !dir.exists() {
        return dir.to_path_buf();
    }
    let base = dir.as_os_str().to_string_lossy().into_owned();
    (2..).map(|i| PathBuf::from(format!("{base} ({i})"))).find(|p| !p.exists()).expect("a free name")
}

/// A path from an archive, made safe to join onto the output folder.
fn safe_relative(name: &Path) -> Option<PathBuf> {
    let mut out = PathBuf::new();
    for c in name.components() {
        match c {
            Component::Normal(part) => out.push(sanitise(&part.to_string_lossy())),
            Component::CurDir => {}
            // Absolute paths, drive letters and `..` are refused outright.
            _ => return None,
        }
    }
    (!out.as_os_str().is_empty()).then_some(out)
}

fn unrar(archive: &Path, out: &Path, password: Option<&str>) -> anyhow::Result<()> {
    let opened = match password {
        Some(pw) => unrar::Archive::with_password(archive, pw).open_for_processing(),
        None => unrar::Archive::new(archive).open_for_processing(),
    };
    let mut a = opened.map_err(|e| anyhow::anyhow!("{e}"))?;
    while let Some(header) = a.read_header().map_err(|e| anyhow::anyhow!(rar_error(&e.to_string(), password)))? {
        let entry = header.entry();
        if entry.is_file() {
            let Some(rel) = safe_relative(&entry.filename) else {
                bail!("the archive contains an unsafe path: {}", entry.filename.display());
            };
            let dest = out.join(rel);
            if let Some(parent) = dest.parent() {
                std::fs::create_dir_all(parent)?;
            }
            a = header.extract_to(&dest).map_err(|e| anyhow::anyhow!(rar_error(&e.to_string(), password)))?;
        } else {
            a = header.skip().map_err(|e| anyhow::anyhow!("{e}"))?;
        }
    }
    Ok(())
}

fn rar_error(e: &str, password: Option<&str>) -> String {
    if e.to_ascii_lowercase().contains("password") || e.contains("ERAR_BAD_PASSWORD") || e.contains("MissingPassword") {
        if password.is_some() { "the archive's password is wrong".into() } else { "the archive needs a password".into() }
    } else {
        e.to_string()
    }
}

fn un7z(archive: &Path, out: &Path, password: Option<&str>) -> anyhow::Result<()> {
    let check = |entry: &sevenz_rust2::ArchiveEntry, reader: &mut dyn std::io::Read, _dest: &PathBuf| {
        let Some(rel) = safe_relative(Path::new(entry.name())) else {
            return Err(sevenz_rust2::Error::Other(format!("unsafe path in archive: {}", entry.name()).into()));
        };
        sevenz_rust2::default_entry_extract_fn(entry, reader, &out.join(rel))
    };
    let file = std::fs::File::open(archive)?;
    let result = match password {
        Some(pw) => sevenz_rust2::decompress_with_extract_fn_and_password(file, out, pw.into(), check),
        None => sevenz_rust2::decompress_with_extract_fn(file, out, check),
    };
    result.map_err(|e| anyhow::anyhow!("{e}"))
}

/// Rename, or copy and delete when crossing filesystems.
fn move_file(from: &Path, to: &Path) -> anyhow::Result<()> {
    if std::fs::rename(from, to).is_ok() {
        return Ok(());
    }
    std::fs::copy(from, to).with_context(|| format!("moving {} to {}", from.display(), to.display()))?;
    std::fs::remove_file(from)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spots_first_volumes() {
        assert!(matches!(first_volume(Path::new("a.part01.rar")), Some(Kind::Rar)));
        assert!(matches!(first_volume(Path::new("a.part1.rar")), Some(Kind::Rar)));
        assert!(first_volume(Path::new("a.part02.rar")).is_none());
        assert!(matches!(first_volume(Path::new("a.rar")), Some(Kind::Rar)));
        assert!(first_volume(Path::new("a.r00")).is_none());
        assert!(matches!(first_volume(Path::new("a.7z.001")), Some(Kind::SevenZip)));
        assert!(first_volume(Path::new("a.mkv")).is_none());
        assert!(is_archive_part(Path::new("a.r07")));
        assert!(is_archive_part(Path::new("a.7z.002")));
        assert!(!is_archive_part(Path::new("a.mkv")));
    }

    #[test]
    fn refuses_unsafe_paths() {
        assert!(safe_relative(Path::new("../etc/passwd")).is_none());
        assert!(safe_relative(Path::new("/etc/passwd")).is_none());
        assert_eq!(safe_relative(Path::new("a/b/c.txt")), Some(PathBuf::from("a").join("b").join("c.txt")));
        assert_eq!(safe_relative(Path::new("./x.mkv")), Some(PathBuf::from("x.mkv")));
    }
}
