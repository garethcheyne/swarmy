//! Folders on the server: browsing them to pick one, and checking Swarmy can
//! write where it's been told to. Lists folder names only, never files, and
//! only to signed-in users (it sits behind the session check like the rest
//! of `/api`).
//!
//! The write check really writes (and removes) a small file rather than
//! reading permission bits: on NFS and SMB shares, and in unprivileged
//! containers, the bits often say yes when the server says no.

use std::path::{Path, PathBuf};

use axum::{
    Json, Router,
    extract::{Query, State},
    routing::{get, post},
};
use serde::{Deserialize, Serialize};

use crate::{
    api::{ApiError, ApiResult},
    state::Shared,
};

pub fn router() -> Router<Shared> {
    Router::new().route("/folders", get(list)).route("/folders/new", post(create))
}

/// Can Swarmy create files in `dir`? Creates the folder if it's missing.
/// The error says why not, in words someone can act on.
pub fn check_writable(dir: &Path) -> Result<(), String> {
    if !dir.is_absolute() {
        return Err(format!("{} isn't a full path. Give the whole path, like /data/downloads.", dir.display()));
    }
    let explain = |e: std::io::Error| match e.kind() {
        std::io::ErrorKind::PermissionDenied => {
            format!("Swarmy can't write to {}: permission denied{}.", dir.display(), running_as())
        }
        std::io::ErrorKind::ReadOnlyFilesystem => format!("{} is on a read-only disk.", dir.display()),
        std::io::ErrorKind::StorageFull => format!("{} is full.", dir.display()),
        _ => format!("Swarmy can't write to {}: {e}.", dir.display()),
    };
    std::fs::create_dir_all(dir).map_err(explain)?;
    let probe = dir.join(format!(".swarmy-write-test-{}", std::process::id()));
    std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&probe)
        .and_then(|mut f| std::io::Write::write_all(&mut f, b"ok"))
        .map_err(explain)?;
    let _ = std::fs::remove_file(&probe);
    Ok(())
}

/// Make sure every category's folder exists. Radarr and Sonarr check that the
/// folder a category downloads to is there, and warn about a missing path
/// mapping when it isn't, even before anything has downloaded into it.
pub fn make_category_folders(state: &Shared) {
    let names: Vec<String> = state.store.read(|d| d.categories.keys().cloned().collect());
    for name in names {
        let dir = state.resolve_save_path(None, &name);
        if let Err(e) = std::fs::create_dir_all(&dir) {
            tracing::warn!("can't create the folder for category {name}, {}: {e}", dir.display());
        }
    }
}

/// " (Swarmy runs as user 999)": the number to give a folder to.
fn running_as() -> String {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        if let Ok(m) = std::fs::metadata("/proc/self") {
            return format!(" (Swarmy runs as user {})", m.uid());
        }
    }
    String::new()
}

#[derive(Deserialize)]
struct ListQuery {
    path: Option<String>,
}

#[derive(Serialize)]
struct Entry {
    name: String,
    path: PathBuf,
}

#[derive(Serialize)]
struct Place {
    label: String,
    path: PathBuf,
}

#[derive(Serialize)]
struct Listing {
    path: PathBuf,
    parent: Option<PathBuf>,
    folders: Vec<Entry>,
    /// Whether Swarmy can write here, and if not, why.
    writable: bool,
    problem: Option<String>,
    /// Shortcuts: the download folder, mounted shares, the top of each disk.
    places: Vec<Place>,
}

const MAX_ENTRIES: usize = 2000;

async fn list(State(state): State<Shared>, Query(q): Query<ListQuery>) -> ApiResult<Json<Listing>> {
    let downloads = state.download_dir();
    let start = q.path.as_deref().map(str::trim).filter(|p| !p.is_empty()).map(PathBuf::from);
    let path = start.unwrap_or_else(|| nearest_existing(&downloads));
    if !path.is_absolute() {
        return Err(ApiError::bad_request("Give a full path, like /data."));
    }
    let listing = tokio::task::spawn_blocking(move || -> Result<Listing, String> {
        let mut folders: Vec<Entry> = std::fs::read_dir(&path)
            .map_err(|e| format!("Can't open {}: {e}", path.display()))?
            .flatten()
            .filter(|e| e.file_type().is_ok_and(|t| t.is_dir() || (t.is_symlink() && e.path().is_dir())))
            .filter_map(|e| {
                let name = e.file_name().to_string_lossy().into_owned();
                (!name.starts_with('.')).then(|| Entry { path: e.path(), name })
            })
            .take(MAX_ENTRIES)
            .collect();
        folders.sort_by_key(|e| e.name.to_lowercase());
        let (writable, problem) = match writable_without_creating(&path) {
            Ok(()) => (true, None),
            Err(e) => (false, Some(e)),
        };
        Ok(Listing {
            parent: path.parent().map(Path::to_path_buf),
            path,
            folders,
            writable,
            problem,
            places: places(&downloads),
        })
    })
    .await
    .map_err(|e| ApiError::bad_request(e.to_string()))?
    .map_err(ApiError::bad_request)?;
    Ok(Json(listing))
}

/// The write check for a folder that exists (browsing shouldn't create anything).
fn writable_without_creating(dir: &Path) -> Result<(), String> {
    if !dir.is_dir() {
        return Err(format!("{} doesn't exist.", dir.display()));
    }
    check_writable(dir)
}

fn nearest_existing(p: &Path) -> PathBuf {
    let mut p = p.to_path_buf();
    while !p.is_dir() {
        if !p.pop() {
            return PathBuf::from("/");
        }
    }
    p
}

fn places(downloads: &Path) -> Vec<Place> {
    let mut out = vec![Place { label: "Download folder".into(), path: downloads.to_path_buf() }];
    // Mounted shares (Proxmox maps them to /mnt/<name>; NAS mounts live here too).
    for base in ["/mnt", "/media", "/srv"] {
        if let Ok(dir) = std::fs::read_dir(base) {
            let mut found: Vec<PathBuf> = dir.flatten().map(|e| e.path()).filter(|p| p.is_dir()).collect();
            found.sort();
            out.extend(found.into_iter().map(|p| Place {
                label: p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default(),
                path: p,
            }));
        }
    }
    if cfg!(windows) {
        for letter in b'C'..=b'Z' {
            let root = PathBuf::from(format!("{}:\\", letter as char));
            if root.is_dir() {
                out.push(Place { label: format!("{}:", letter as char), path: root });
            }
        }
    } else {
        out.push(Place { label: "Top of the disk".into(), path: PathBuf::from("/") });
    }
    out
}

#[derive(Deserialize)]
struct NewFolder {
    parent: String,
    name: String,
}

async fn create(Json(req): Json<NewFolder>) -> ApiResult<Json<Entry>> {
    let name = req.name.trim();
    let ok = !name.is_empty()
        && name != "."
        && name != ".."
        && !name.contains(['/', '\\'])
        && !name.starts_with('.');
    if !ok {
        return Err(ApiError::bad_request("Pick a folder name without slashes."));
    }
    let parent = PathBuf::from(req.parent.trim());
    if !parent.is_absolute() || !parent.is_dir() {
        return Err(ApiError::bad_request("That folder isn't there any more."));
    }
    let path = parent.join(name);
    std::fs::create_dir(&path).map_err(|e| {
        ApiError::bad_request(match e.kind() {
            std::io::ErrorKind::AlreadyExists => format!("{name} already exists."),
            std::io::ErrorKind::PermissionDenied => {
                format!("Swarmy can't create folders in {}: permission denied{}.", parent.display(), running_as())
            }
            _ => format!("Couldn't create {name}: {e}"),
        })
    })?;
    Ok(Json(Entry { name: name.to_owned(), path }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn writable_folders_pass_and_leave_nothing_behind() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("new/deeper");
        check_writable(&dir).unwrap();
        assert!(dir.is_dir(), "missing folders are created");
        assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 0, "the test file is removed");
        assert!(check_writable(Path::new("relative/path")).is_err());
    }

    #[cfg(unix)]
    #[test]
    fn says_why_a_folder_is_read_only() {
        use std::os::unix::fs::PermissionsExt;
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("locked");
        std::fs::create_dir(&dir).unwrap();
        std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o555)).unwrap();
        // Root can write anywhere, so only check when the tests aren't run as root.
        if std::fs::metadata("/proc/self").map(|m| std::os::unix::fs::MetadataExt::uid(&m)).unwrap_or(0) != 0 {
            let e = check_writable(&dir).unwrap_err();
            assert!(e.contains("permission denied") && e.contains("runs as user"), "{e}");
        }
    }
}
