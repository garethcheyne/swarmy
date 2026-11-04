//! Swarmy's version is `yyyy.mm.dd.build`: the release date, and which release
//! of that day it is. Cargo versions must be semver (three numbers, no leading
//! zeros), so the real version lives in one place, the newest entry in
//! CHANGELOG.md, and this bakes it in as `SWARMY_VERSION`.
//!
//! Release builds set `SWARMY_RELEASE` to the tag's version, and the build
//! fails if it doesn't match the changelog, so a release can't ship without
//! its notes.

fn main() {
    // Re-embed the web UI whenever `pnpm build` produces a new web/dist.
    println!("cargo:rerun-if-changed=../../web/dist");
    println!("cargo:rerun-if-changed=../../CHANGELOG.md");
    println!("cargo:rerun-if-env-changed=SWARMY_RELEASE");

    let changelog = std::fs::read_to_string("../../CHANGELOG.md").expect("reading CHANGELOG.md");
    let newest = changelog
        .lines()
        .find_map(|l| l.strip_prefix("## [")?.split_once(']').map(|(v, _)| v.trim().to_owned()))
        .expect("CHANGELOG.md has no `## [version]` entry");

    if let Ok(release) = std::env::var("SWARMY_RELEASE") {
        let release = release.trim().trim_start_matches('v');
        assert!(
            release == newest,
            "releasing {release}, but the newest CHANGELOG.md entry is {newest}. Add its release notes first."
        );
    }
    println!("cargo:rustc-env=SWARMY_VERSION={newest}");
}
