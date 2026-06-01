//! Embedded static assets (CSS, JS, fonts, mermaid, mathjax) + page template.
//!
//! Mirror of the Go `internal/assets` package. Sync from Go side with
//! `make sync-mt-assets`.

use include_dir::{Dir, include_dir};

/// Snapshot of `src/mt/assets/static/` baked into the binary at compile time.
pub static STATIC_DIR: Dir<'static> = include_dir!("$CARGO_MANIFEST_DIR/src/mt/assets/static");

/// Raw HTML template source.
pub const TEMPLATE: &str = include_str!("template.html");

/// Returns the raw bytes of a file under static/ (e.g. "style.css", "mathjax/tex-mml-chtml.js").
pub fn read_static(path: &str) -> Option<&'static [u8]> {
    STATIC_DIR.get_file(path).map(|f| f.contents())
}

/// Returns the raw template string.
pub fn template() -> &'static str {
    TEMPLATE
}

/// Writes every file in [`STATIC_DIR`] to `dst`, preserving the relative tree.
/// Creates parent directories as needed.
pub fn extract_to(dst: &std::path::Path) -> std::io::Result<()> {
    std::fs::create_dir_all(dst)?;
    extract_dir_impl(&STATIC_DIR, dst)
}

fn extract_dir_impl(d: &Dir<'_>, base: &std::path::Path) -> std::io::Result<()> {
    for entry in d.entries() {
        match entry {
            include_dir::DirEntry::File(f) => {
                let target = base.join(f.path());
                if let Some(parent) = target.parent() {
                    std::fs::create_dir_all(parent)?;
                }
                std::fs::write(target, f.contents())?;
            }
            include_dir::DirEntry::Dir(sub) => {
                extract_dir_impl(sub, base)?;
            }
        }
    }
    Ok(())
}

pub mod page;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn template_has_expected_anchor() {
        assert!(TEMPLATE.contains("<title>"));
    }

    #[test]
    fn static_dir_has_known_files() {
        assert!(read_static("style.css").is_some(), "style.css missing");
        assert!(read_static("app.js").is_some(), "app.js missing");
    }
}
