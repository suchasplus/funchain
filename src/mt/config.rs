//! Global user configuration — `~/.config/mt/config.toml`.
//!
//! Controls directory/site mode behavior that shouldn't require flags on
//! every invocation. Today that is:
//!
//!   * `site.exclude` — basenames (case-insensitive) hidden from directory
//!     scans. Defaults to AI-assistant prompt files (`CLAUDE.md`,
//!     `AGENTS.md`, …) that are noise in a rendered documentation site.
//!     Setting `exclude = []` disables hiding entirely. The `--all` CLI
//!     switch bypasses the list for one run.
//!   * `site.nav_filenames` — show each page's source filename as small
//!     text under its title in the left nav (default `true`).
//!
//! A missing file yields [`MtConfig::default`]; a malformed file warns on
//! stderr and falls back to the defaults — rendering never fails because of
//! config problems. Explicit single-file targets are rendered regardless of
//! the exclude list (the list only applies to directory scans).

use std::path::PathBuf;

use serde::Deserialize;

/// Basenames hidden from directory scans when no config file overrides them:
/// AI-assistant instruction files that read as noise in a rendered site.
pub const DEFAULT_EXCLUDES: &[&str] = &["CLAUDE.md", "CLAUDE.local.md", "AGENTS.md", "GEMINI.md"];

/// Resolved configuration with all defaults applied.
#[derive(Debug, Clone, PartialEq)]
pub struct MtConfig {
    /// Basenames (case-insensitive) excluded from directory scans.
    pub exclude: Vec<String>,
    /// Show source filenames as small text in the site nav.
    pub nav_filenames: bool,
}

impl Default for MtConfig {
    fn default() -> Self {
        Self {
            exclude: DEFAULT_EXCLUDES.iter().map(|s| s.to_string()).collect(),
            nav_filenames: true,
        }
    }
}

impl MtConfig {
    /// Case-insensitive membership test against the exclude list.
    pub fn is_excluded(&self, basename: &str) -> bool {
        self.exclude
            .iter()
            .any(|e| e.eq_ignore_ascii_case(basename))
    }
}

/// On-disk TOML shape. All keys optional — absent keys keep their defaults.
#[derive(Debug, Deserialize)]
struct ConfigFile {
    site: Option<SiteSection>,
}

#[derive(Debug, Deserialize)]
struct SiteSection {
    exclude: Option<Vec<String>>,
    nav_filenames: Option<bool>,
}

/// Parses TOML text into a config, applying defaults for absent keys.
pub fn parse(text: &str) -> Result<MtConfig, toml::de::Error> {
    let file: ConfigFile = toml::from_str(text)?;
    let mut cfg = MtConfig::default();
    if let Some(site) = file.site {
        if let Some(exclude) = site.exclude {
            cfg.exclude = exclude;
        }
        if let Some(nav) = site.nav_filenames {
            cfg.nav_filenames = nav;
        }
    }
    Ok(cfg)
}

/// `$XDG_CONFIG_HOME/mt/config.toml`, falling back to `~/.config/mt/config.toml`.
pub fn config_path() -> Option<PathBuf> {
    config_path_from(
        std::env::var_os("XDG_CONFIG_HOME").map(PathBuf::from),
        std::env::var_os("HOME").map(PathBuf::from),
    )
}

fn config_path_from(xdg: Option<PathBuf>, home: Option<PathBuf>) -> Option<PathBuf> {
    let base = xdg.or_else(|| home.map(|h| h.join(".config")))?;
    Some(base.join("mt").join("config.toml"))
}

/// Loads the global config. Missing file → defaults; unreadable or malformed
/// file → warning on stderr + defaults.
pub fn load() -> MtConfig {
    let Some(path) = config_path() else {
        return MtConfig::default();
    };
    let Ok(text) = std::fs::read_to_string(&path) else {
        return MtConfig::default();
    };
    match parse(&text) {
        Ok(cfg) => cfg,
        Err(e) => {
            eprintln!("mt: warning: {}: {e}", path.display());
            MtConfig::default()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_hides_ai_prompt_files_and_shows_nav_filenames() {
        let cfg = MtConfig::default();
        for name in ["CLAUDE.md", "CLAUDE.local.md", "AGENTS.md", "GEMINI.md"] {
            assert!(
                cfg.exclude.iter().any(|e| e == name),
                "default exclude list missing {name}: {:?}",
                cfg.exclude
            );
        }
        assert!(cfg.nav_filenames, "nav filenames should default on");
    }

    #[test]
    fn is_excluded_matches_case_insensitively() {
        let cfg = MtConfig::default();
        assert!(cfg.is_excluded("CLAUDE.md"));
        assert!(cfg.is_excluded("claude.MD"));
        assert!(cfg.is_excluded("agents.md"));
        assert!(!cfg.is_excluded("README.md"));
        assert!(!cfg.is_excluded("notes.md"));
    }

    #[test]
    fn parse_replaces_exclude_list() {
        let cfg = parse("[site]\nexclude = [\"secret.md\"]\n").unwrap();
        assert_eq!(cfg.exclude, vec!["secret.md"]);
        // untouched key keeps its default
        assert!(cfg.nav_filenames);
    }

    #[test]
    fn parse_empty_exclude_hides_nothing() {
        let cfg = parse("[site]\nexclude = []\n").unwrap();
        assert!(cfg.exclude.is_empty());
        assert!(!cfg.is_excluded("CLAUDE.md"));
    }

    #[test]
    fn parse_empty_or_sectionless_text_gives_defaults() {
        assert_eq!(parse("").unwrap(), MtConfig::default());
        assert_eq!(parse("# just a comment\n").unwrap(), MtConfig::default());
    }

    #[test]
    fn parse_nav_filenames_off() {
        let cfg = parse("[site]\nnav_filenames = false\n").unwrap();
        assert!(!cfg.nav_filenames);
        // exclude untouched → default list
        assert!(cfg.is_excluded("CLAUDE.md"));
    }

    #[test]
    fn parse_rejects_malformed_toml() {
        assert!(parse("not = [toml").is_err());
    }

    #[test]
    fn config_path_prefers_xdg_over_home() {
        let p = config_path_from(Some(PathBuf::from("/xdg")), Some(PathBuf::from("/home/u")));
        assert_eq!(p, Some(PathBuf::from("/xdg/mt/config.toml")));
        let p = config_path_from(None, Some(PathBuf::from("/home/u")));
        assert_eq!(p, Some(PathBuf::from("/home/u/.config/mt/config.toml")));
        assert_eq!(config_path_from(None, None), None);
    }
}
