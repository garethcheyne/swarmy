//! The release notes, compiled in from CHANGELOG.md so every build knows what
//! it contains. Served at /api/changelog for Settings → About and the
//! "What's new" window.

use std::sync::LazyLock;

use serde::Serialize;

const SOURCE: &str = include_str!("../../../CHANGELOG.md");

#[derive(Debug, Clone, Serialize)]
pub struct Release {
    pub version: String,
    /// ISO date, or `None` while unreleased.
    pub date: Option<String>,
    pub summary: String,
    pub sections: Vec<Section>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Section {
    pub title: String,
    pub items: Vec<String>,
}

pub fn releases() -> &'static [Release] {
    static PARSED: LazyLock<Vec<Release>> = LazyLock::new(|| parse(SOURCE));
    &PARSED
}

/// Parses the subset of Keep a Changelog that CHANGELOG.md uses:
/// `## [1.2.0] - 2026-11-04`, an optional summary paragraph, `### Title`
/// sections and `- item` bullets (indented lines continue the bullet).
pub fn parse(src: &str) -> Vec<Release> {
    let mut out: Vec<Release> = Vec::new();
    for line in src.lines() {
        let trimmed = line.trim();
        if let Some(head) = trimmed.strip_prefix("## ") {
            let (version, date) = match head.split_once(" - ") {
                Some((v, d)) => (v, Some(d.trim())),
                None => (head, None),
            };
            let version = version.trim().trim_start_matches('[').trim_end_matches(']').trim();
            let date = date.filter(|d| !d.eq_ignore_ascii_case("unreleased")).map(str::to_string);
            out.push(Release { version: version.to_string(), date, summary: String::new(), sections: Vec::new() });
            continue;
        }
        let Some(release) = out.last_mut() else { continue };
        if let Some(title) = trimmed.strip_prefix("### ") {
            release.sections.push(Section { title: title.trim().to_string(), items: Vec::new() });
        } else if let Some(item) = trimmed.strip_prefix("- ").or_else(|| trimmed.strip_prefix("* ")) {
            if let Some(section) = release.sections.last_mut() {
                section.items.push(item.trim().to_string());
            }
        } else if !trimmed.is_empty() {
            if release.sections.is_empty() {
                if !release.summary.is_empty() {
                    release.summary.push(' ');
                }
                release.summary.push_str(trimmed);
            } else if line.starts_with(char::is_whitespace) {
                if let Some(item) = release.sections.last_mut().and_then(|s| s.items.last_mut()) {
                    item.push(' ');
                    item.push_str(trimmed);
                }
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn changelog_matches_this_version() {
        let all = releases();
        assert!(!all.is_empty(), "CHANGELOG.md has no releases");
        assert_eq!(
            all[0].version,
            env!("CARGO_PKG_VERSION"),
            "the newest CHANGELOG.md entry must be this version; add release notes before bumping"
        );
        for r in all {
            assert!(r.sections.iter().any(|s| !s.items.is_empty()), "{} has no entries", r.version);
        }
    }

    #[test]
    fn parses_releases_sections_and_continuations() {
        let src = "# Changelog\n\nIntro text.\n\n## [1.1.0] - 2026-12-01\n\nShort summary.\n\n### New\n- One\n- Two\n  continued\n\n### Fixed\n* Three\n\n## 1.0.0 - Unreleased\n### New\n- Zero\n";
        let r = parse(src);
        assert_eq!(r.len(), 2);
        assert_eq!(r[0].version, "1.1.0");
        assert_eq!(r[0].date.as_deref(), Some("2026-12-01"));
        assert_eq!(r[0].summary, "Short summary.");
        assert_eq!(r[0].sections[0].items, vec!["One", "Two continued"]);
        assert_eq!(r[0].sections[1].title, "Fixed");
        assert_eq!(r[1].date, None);
    }
}
