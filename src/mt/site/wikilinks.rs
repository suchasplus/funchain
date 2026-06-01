//! Cross-file wikilink resolver. Mirror of Go `internal/site/wikilinks.go`.

use std::collections::HashMap;

use super::scan::Entry;
use super::tree::rel_path;
use super::url_path::encode_segments;

/// Maps `lowercase(basename)` → first matching `outRel`. Collisions are
/// recorded in `conflicts` so callers can warn.
#[derive(Debug, Default, Clone)]
pub struct NameIndex {
    by_name: HashMap<String, String>,
    pub conflicts: HashMap<String, Vec<String>>,
}

impl NameIndex {
    /// Build the index from a slice of entries.
    pub fn build(entries: &[Entry]) -> Self {
        let mut idx = NameIndex::default();
        for e in entries {
            let key = e.stem.to_lowercase();
            if let Some(existing) = idx.by_name.get(&key).cloned() {
                let list = idx.conflicts.entry(key.clone()).or_default();
                if list.is_empty() {
                    list.push(existing);
                }
                list.push(e.out_rel.clone());
                continue;
            }
            idx.by_name.insert(key, e.out_rel.clone());
        }
        idx
    }

    /// Returns a closure compatible with `crate::mt::render::WikilinkResolver`.
    /// `from_out` is the current page's html path (slash-separated, relative
    /// to the site root) — used to compute relative hrefs.
    pub fn resolver_for<'a>(&'a self, from_out: &'a str) -> impl Fn(&str) -> Option<String> + 'a {
        move |name: &str| {
            let key = name.trim().to_lowercase();
            self.by_name
                .get(&key)
                .map(|target| encode_segments(&rel_path(from_out, target)))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn entry(rel: &str) -> Entry {
        Entry {
            abs: PathBuf::new(),
            rel: rel.into(),
            out_rel: rel.replace(".md", ".html"),
            stem: rel
                .rsplit('/')
                .next()
                .unwrap_or("")
                .trim_end_matches(".md")
                .into(),
            title: String::new(),
        }
    }

    #[test]
    fn resolver_finds_pages() {
        let entries = vec![entry("README.md"), entry("guide/intro.md")];
        let idx = NameIndex::build(&entries);
        let from_root = idx.resolver_for("README.html");
        assert_eq!(from_root("intro"), Some("guide/intro.html".to_string()));
        let from_nested = idx.resolver_for("guide/intro.html");
        assert_eq!(from_nested("README"), Some("../README.html".to_string()));
        assert!(from_nested("unknown").is_none());
    }

    #[test]
    fn case_insensitive_match() {
        let entries = vec![entry("Notes.md")];
        let idx = NameIndex::build(&entries);
        let r = idx.resolver_for("a.html");
        assert_eq!(r("NOTES"), Some("Notes.html".into()));
        assert_eq!(r("notes"), Some("Notes.html".into()));
    }

    #[test]
    fn conflicts_are_recorded() {
        let entries = vec![entry("a/dup.md"), entry("b/dup.md")];
        let idx = NameIndex::build(&entries);
        let list = idx.conflicts.get("dup").expect("conflict not recorded");
        assert_eq!(list.len(), 2);
    }
}
