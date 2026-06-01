//! Recursive directory scanner. Mirror of Go `internal/site/scan.go`.

use std::cmp::Ordering;
use std::path::{Path, PathBuf};
use walkdir::WalkDir;

/// One markdown source file discovered under the site root.
#[derive(Debug, Clone)]
pub struct Entry {
    /// Absolute on-disk path.
    pub abs: PathBuf,
    /// Path relative to the site root, slash-separated (e.g. `"guide/intro.md"`).
    pub rel: String,
    /// Output HTML path (extension swapped to `.html`).
    pub out_rel: String,
    /// Basename without extension.
    pub stem: String,
    /// Title — populated post-render by the builder.
    pub title: String,
}

/// Walks `root` recursively, returns sorted `Entry`s for every `*.md` file.
/// Skips hidden directories and common build-output folders.
pub fn scan(root: &Path) -> std::io::Result<Vec<Entry>> {
    let root = root
        .canonicalize()
        .or_else(|_| Ok::<_, std::io::Error>(root.to_path_buf()))?;
    let mut out: Vec<Entry> = Vec::new();
    for entry in WalkDir::new(&root)
        .follow_links(false)
        .into_iter()
        .filter_entry(|e| !should_skip_dir(e))
    {
        let entry = match entry {
            Ok(e) => e,
            Err(err) => return Err(std::io::Error::other(err.to_string())),
        };
        if !entry.file_type().is_file() {
            continue;
        }
        let name = entry.file_name().to_string_lossy().to_lowercase();
        if !name.ends_with(".md") {
            continue;
        }
        let abs = entry.into_path();
        let rel = abs
            .strip_prefix(&root)
            .map(|p| p.to_path_buf())
            .unwrap_or_else(|_| abs.clone());
        let rel_slash = rel
            .components()
            .filter_map(|c| c.as_os_str().to_str())
            .collect::<Vec<_>>()
            .join("/");
        let stem = Path::new(&rel_slash)
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("")
            .to_string();
        // Replace the markdown extension case-insensitively. We already filter
        // to *.md (case-insensitively), so we expect to find one; fall back to
        // appending ".html" if we somehow don't.
        let out_rel = match Path::new(&rel_slash).extension().and_then(|e| e.to_str()) {
            Some(ext) if ext.eq_ignore_ascii_case("md") => {
                let cut = rel_slash.len() - ext.len() - 1; // also drop the leading dot
                format!("{}.html", &rel_slash[..cut])
            }
            _ => format!("{rel_slash}.html"),
        };
        out.push(Entry {
            abs,
            rel: rel_slash,
            out_rel,
            stem,
            title: String::new(),
        });
    }
    out.sort_by(|a, b| natural_cmp(&a.rel, &b.rel));
    Ok(out)
}

fn should_skip_dir(entry: &walkdir::DirEntry) -> bool {
    // Always allow the root itself through.
    if entry.depth() == 0 {
        return false;
    }
    if !entry.file_type().is_dir() {
        return false;
    }
    let name = entry.file_name().to_string_lossy();
    if name.starts_with('.') {
        return true;
    }
    matches!(
        name.as_ref(),
        "node_modules" | "vendor" | "dist" | "build" | "target"
    )
}

/// Total-order comparator that mirrors Go's `sort.Slice(entries[i].Rel < entries[j].Rel)` —
/// lexicographic by Unicode code points.
fn natural_cmp(a: &str, b: &str) -> Ordering {
    a.cmp(b)
}

/// Picks the landing page in priority: top-level README.md > top-level index.md > first entry.
pub fn landing_page(entries: &[Entry]) -> Option<&Entry> {
    let mut readme: Option<&Entry> = None;
    let mut index: Option<&Entry> = None;
    for e in entries {
        let base = e.rel.rsplit('/').next().unwrap_or(&e.rel).to_lowercase();
        let depth = e.rel.matches('/').count();
        match base.as_str() {
            "readme.md" if readme.is_none_or(|r| r.rel.matches('/').count() > depth) => {
                readme = Some(e);
            }
            "index.md" if index.is_none_or(|r| r.rel.matches('/').count() > depth) => {
                index = Some(e);
            }
            _ => {}
        }
    }
    readme.or(index).or_else(|| entries.first())
}

/// Returns the index of the entry whose `rel` matches, or None.
pub fn index_of(entries: &[Entry], rel: &str) -> Option<usize> {
    entries.iter().position(|e| e.rel == rel)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn tmp(label: &str) -> PathBuf {
        let pid = std::process::id();
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.subsec_nanos())
            .unwrap_or(0);
        let p = std::env::temp_dir().join(format!("mt-site-{label}-{pid}-{nanos}"));
        fs::create_dir_all(&p).unwrap();
        p
    }
    fn write(path: PathBuf, content: &str) {
        if let Some(p) = path.parent() {
            fs::create_dir_all(p).unwrap();
        }
        fs::write(path, content).unwrap();
    }

    #[test]
    fn mixed_case_md_extension_renames_correctly() {
        let dir = tmp("case");
        write(dir.join("Case.Md"), "# c");
        write(dir.join("README.MD"), "# r");
        let entries = scan(&dir).unwrap();
        let out_rels: Vec<_> = entries.iter().map(|e| e.out_rel.clone()).collect();
        assert!(
            out_rels.contains(&"Case.html".to_string()),
            "Case.html missing: {out_rels:?}"
        );
        assert!(
            out_rels.contains(&"README.html".to_string()),
            "README.html missing: {out_rels:?}"
        );
        // None of the entries should have the original .Md/.MD suffix.
        for r in &out_rels {
            assert!(!r.contains(".Md") && !r.contains(".MD"), "leaky case: {r}");
        }
    }

    #[test]
    fn scan_recurses_and_skips() {
        let dir = tmp("scan");
        write(dir.join("README.md"), "# r");
        write(dir.join("guide/intro.md"), "# i");
        write(dir.join("guide/deep/page.md"), "# d");
        write(dir.join(".git/HEAD"), "");
        write(dir.join("node_modules/pkg.md"), "skip");
        write(dir.join("image.png"), "");
        let entries = scan(&dir).unwrap();
        let rels: Vec<_> = entries.iter().map(|e| e.rel.clone()).collect();
        assert_eq!(
            rels,
            vec!["README.md", "guide/deep/page.md", "guide/intro.md"]
        );
        assert_eq!(entries[0].out_rel, "README.html");
    }

    #[test]
    fn landing_prefers_top_level_readme() {
        let entries = vec![
            Entry {
                abs: PathBuf::new(),
                rel: "guide/intro.md".into(),
                out_rel: "guide/intro.html".into(),
                stem: "intro".into(),
                title: String::new(),
            },
            Entry {
                abs: PathBuf::new(),
                rel: "README.md".into(),
                out_rel: "README.html".into(),
                stem: "README".into(),
                title: String::new(),
            },
        ];
        assert_eq!(landing_page(&entries).unwrap().rel, "README.md");
    }

    #[test]
    fn landing_falls_back_to_index_md() {
        let entries = vec![
            Entry {
                abs: PathBuf::new(),
                rel: "a.md".into(),
                out_rel: "a.html".into(),
                stem: "a".into(),
                title: String::new(),
            },
            Entry {
                abs: PathBuf::new(),
                rel: "index.md".into(),
                out_rel: "index.html".into(),
                stem: "index".into(),
                title: String::new(),
            },
        ];
        assert_eq!(landing_page(&entries).unwrap().rel, "index.md");
    }

    #[test]
    fn index_of_finds_match() {
        let entries = vec![
            Entry {
                abs: PathBuf::new(),
                rel: "a.md".into(),
                out_rel: "a.html".into(),
                stem: "a".into(),
                title: String::new(),
            },
            Entry {
                abs: PathBuf::new(),
                rel: "b.md".into(),
                out_rel: "b.html".into(),
                stem: "b".into(),
                title: String::new(),
            },
        ];
        assert_eq!(index_of(&entries, "b.md"), Some(1));
        assert_eq!(index_of(&entries, "missing.md"), None);
    }
}
