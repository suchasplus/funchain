//! Build/version metadata shared by all funchain binaries.
//!
//! Compile-time values come from build.rs (which shells out to `git` and `date`).
//! When the underlying commands aren't available, fallback `"unknown"` is used.

/// SemVer from Cargo.toml.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Short git SHA (12 chars) when built from a git tree; `"unknown"` otherwise.
pub const GIT_SHA: &str = env!("MT_GIT_SHA");

/// UTC build timestamp, ISO-8601 (e.g. `2026-06-01T07:30:00.000000000Z`).
pub const BUILD_TIMESTAMP: &str = env!("MT_BUILD_TIMESTAMP");

/// Returns `"<version> (commit <sha>, built <date>)"`. Pieces that resolve to
/// `"unknown"` are elided so the output stays clean on non-git builds.
pub fn full() -> String {
    let mut parts: Vec<String> = vec![VERSION.to_string()];
    let mut details: Vec<String> = Vec::new();
    if GIT_SHA != "unknown" {
        details.push(format!("commit {GIT_SHA}"));
    }
    if BUILD_TIMESTAMP != "unknown" {
        details.push(format!("built {BUILD_TIMESTAMP}"));
    }
    if !details.is_empty() {
        parts.push(format!("({})", details.join(", ")));
    }
    parts.join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn version_pulls_from_cargo() {
        assert_eq!(VERSION, env!("CARGO_PKG_VERSION"));
    }

    #[test]
    fn full_starts_with_version() {
        assert!(full().starts_with(VERSION));
    }

    #[test]
    fn full_includes_commit_when_present() {
        // GIT_SHA is environment-dependent; just ensure no panic and shape is sane.
        let s = full();
        if GIT_SHA != "unknown" {
            assert!(s.contains("commit "));
        }
    }
}
