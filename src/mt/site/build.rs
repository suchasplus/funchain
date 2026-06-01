//! Two-pass site builder. Mirror of Go `internal/site/build.go`.

use std::fs;
use std::path::{Path, PathBuf};

use super::scan::{Entry, landing_page, scan};
use super::tree::{TreeNode, build_tree, rel_path, render_tree};
use super::url_path::encode_segments;
use super::wikilinks::NameIndex;

use crate::mt::assets;
use crate::mt::assets::page::{PageOptions, build_page, render as render_page};
use crate::mt::render::{RenderWarning, Renderer, WikilinkResolver, extract_meta};

/// Knobs the caller (CLI / serve) passes in.
#[derive(Debug, Clone, Default)]
pub struct BuildOptions {
    pub root: PathBuf,
    pub out_dir: PathBuf,
    /// Sidebar label. Defaults to `Path::file_name(root)` when empty.
    pub site_name: String,
    /// User-supplied default theme override.
    pub theme: String,
    /// Optional live-reload JS injected into each page (serve mode).
    pub live_reload_js: String,
}

/// State needed to render any page in a site — built once by [`Context::prepare`].
pub struct Context {
    pub opts: BuildOptions,
    pub entries: Vec<Entry>,
    pub tree: TreeNode,
    pub name_index: NameIndex,
}

/// Outcome of one source file render: HTML bytes plus any non-fatal warnings
/// the render pipeline surfaced. Callers (CLI / serve) decide how to display
/// them — the library never prints to stderr.
#[derive(Debug)]
pub struct PageRender {
    pub html: Vec<u8>,
    pub warnings: Vec<RenderWarning>,
}

/// One warning anchored to its source file. Returned in batched form by
/// [`build`] so the CLI can report them with file context.
#[derive(Debug, Clone)]
pub struct PageWarning {
    pub source: PathBuf,
    pub warning: RenderWarning,
}

/// Result of a complete directory build.
#[derive(Debug)]
pub struct BuildReport {
    /// Absolute path of the landing page (`index.html` or first entry).
    pub landing: PathBuf,
    /// Per-page diagnostics, in source order.
    pub warnings: Vec<PageWarning>,
}

#[derive(Debug)]
pub enum SiteError {
    Io(std::io::Error),
    Render(crate::mt::render::RenderError),
    Page(crate::mt::assets::page::RenderError),
    Empty(PathBuf),
}

impl std::fmt::Display for SiteError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SiteError::Io(e) => write!(f, "io: {e}"),
            SiteError::Render(e) => write!(f, "render: {e}"),
            SiteError::Page(e) => write!(f, "page: {e}"),
            SiteError::Empty(p) => write!(f, "no markdown files found under {}", p.display()),
        }
    }
}

impl std::error::Error for SiteError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            SiteError::Io(e) => Some(e),
            SiteError::Render(e) => Some(e),
            SiteError::Page(e) => Some(e),
            SiteError::Empty(_) => None,
        }
    }
}

impl From<std::io::Error> for SiteError {
    fn from(e: std::io::Error) -> Self {
        SiteError::Io(e)
    }
}
impl From<crate::mt::render::RenderError> for SiteError {
    fn from(e: crate::mt::render::RenderError) -> Self {
        SiteError::Render(e)
    }
}
impl From<crate::mt::assets::page::RenderError> for SiteError {
    fn from(e: crate::mt::assets::page::RenderError) -> Self {
        SiteError::Page(e)
    }
}

impl Context {
    /// Scans the source root and grabs each entry's nav title via
    /// [`extract_meta`] — frontmatter + first-heading scan only, no syntect /
    /// template work. The full render (with wikilink resolution) happens
    /// later via [`render_page`], producing both HTML and warnings.
    pub fn prepare(mut opts: BuildOptions, _renderer: &Renderer) -> Result<Self, SiteError> {
        if opts.root.as_os_str().is_empty() {
            return Err(SiteError::Empty(opts.root.clone()));
        }
        if opts.site_name.is_empty() {
            opts.site_name = opts
                .root
                .file_name()
                .and_then(|s| s.to_str())
                .unwrap_or("site")
                .to_string();
        }
        let mut entries = scan(&opts.root)?;
        if entries.is_empty() {
            return Err(SiteError::Empty(opts.root.clone()));
        }
        for e in entries.iter_mut() {
            let src = fs::read_to_string(&e.abs)?;
            let meta = extract_meta(&src, e.abs.to_str().unwrap_or(""))?;
            e.title = meta.title;
        }
        let tree = build_tree(&entries);
        let name_index = NameIndex::build(&entries);
        Ok(Context {
            opts,
            entries,
            tree,
            name_index,
        })
    }

    /// Renders one entry (by index) to HTML bytes + diagnostics.
    ///
    /// `assets_base_override` lets the serve mode pin `/assets/` regardless of
    /// how deep the page lives; pass `None` to compute a relative path. The
    /// returned [`PageRender::warnings`] is forwarded to the caller for
    /// reporting — this function never writes to stderr itself.
    pub fn render_page(
        &self,
        i: usize,
        renderer: &Renderer,
        assets_base_override: Option<&str>,
    ) -> Result<PageRender, SiteError> {
        let e = self.entries.get(i).ok_or_else(|| {
            SiteError::Io(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                format!("entry index {i} out of range"),
            ))
        })?;
        let src = fs::read_to_string(&e.abs)?;

        let resolver_closure = self.name_index.resolver_for(&e.out_rel);
        let resolver_ref: &WikilinkResolver = &resolver_closure;
        let res = renderer.render_with(&src, e.abs.to_str().unwrap_or(""), Some(resolver_ref))?;
        let warnings = res.warnings.clone();

        let theme = if !res.frontmatter.theme.is_empty() {
            res.frontmatter.theme.clone()
        } else {
            self.opts.theme.clone()
        };

        let assets_base = match assets_base_override {
            Some(s) => s.to_string(),
            None => format!("{}/", rel_path(&e.out_rel, "assets")),
        };

        let mut page = build_page(
            &res.title,
            &res.description,
            res.body.clone(),
            res.toc_html.clone(),
            res.features.has_math,
            res.features.has_mermaid,
            PageOptions {
                theme,
                assets_base,
                inline: false,
                live_reload_js: self.opts.live_reload_js.clone(),
            },
        )?;
        page.site_name = self.opts.site_name.clone();
        page.site_tree = render_tree(&self.tree, &e.rel, &e.out_rel);

        if i > 0 {
            let prev = &self.entries[i - 1];
            // Pager hrefs go straight into HTML; encode each segment so file
            // names with spaces / non-ASCII produce valid URLs.
            page.prev_href = encode_segments(&rel_path(&e.out_rel, &prev.out_rel));
            page.prev_label = display_label(&prev.title, &prev.stem);
        }
        if i + 1 < self.entries.len() {
            let next = &self.entries[i + 1];
            page.next_href = encode_segments(&rel_path(&e.out_rel, &next.out_rel));
            page.next_label = display_label(&next.title, &next.stem);
        }

        let html = render_page(page)?;
        Ok(PageRender {
            html: html.into_bytes(),
            warnings,
        })
    }
}

fn display_label(title: &str, stem: &str) -> String {
    if !title.is_empty() {
        title.to_string()
    } else {
        stem.to_string()
    }
}

/// Renders every Markdown file under `opts.root` to `opts.out_dir`, mirroring
/// the directory structure. Extracts static assets to `opts.out_dir/assets/`.
/// Returns the landing path plus a flat list of per-page warnings so the CLI
/// can present diagnostics (the library never writes to stderr).
pub fn build(opts: BuildOptions, renderer: &Renderer) -> Result<BuildReport, SiteError> {
    if opts.out_dir.as_os_str().is_empty() {
        return Err(SiteError::Io(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "out_dir is required",
        )));
    }
    fs::create_dir_all(&opts.out_dir)?;
    let ctx = Context::prepare(opts, renderer)?;
    assets::extract_to(&ctx.opts.out_dir.join("assets"))?;
    let mut warnings: Vec<PageWarning> = Vec::new();
    for i in 0..ctx.entries.len() {
        let e = &ctx.entries[i];
        let out_path = ctx.opts.out_dir.join(rel_to_os(&e.out_rel));
        if let Some(parent) = out_path.parent() {
            fs::create_dir_all(parent)?;
        }
        let page = ctx.render_page(i, renderer, None)?;
        fs::write(out_path, page.html)?;
        for w in page.warnings {
            warnings.push(PageWarning {
                source: e.abs.clone(),
                warning: w,
            });
        }
    }
    let landing = landing_page(&ctx.entries)
        .map(|e| ctx.opts.out_dir.join(rel_to_os(&e.out_rel)))
        .unwrap_or_else(|| ctx.opts.out_dir.clone());
    Ok(BuildReport { landing, warnings })
}

fn rel_to_os(rel: &str) -> PathBuf {
    let mut p = PathBuf::new();
    for part in rel.split('/') {
        p.push(part);
    }
    p
}

/// Filesystem-friendly slug derived from a directory path.
pub fn sanitize_root_name(p: &Path) -> String {
    let base = p.file_name().and_then(|s| s.to_str()).unwrap_or("site");
    let mut out: String = base
        .chars()
        .map(|c| match c {
            'a'..='z' | 'A'..='Z' | '0'..='9' | '-' | '_' => c,
            _ => '-',
        })
        .collect();
    out = out.trim_matches('-').to_string();
    if out.is_empty() { "site".into() } else { out }
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
        let p = std::env::temp_dir().join(format!("mt-site-build-{label}-{pid}-{nanos}"));
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
    fn sanitize_root_name_examples() {
        assert_eq!(sanitize_root_name(Path::new("guide")), "guide");
        assert_eq!(sanitize_root_name(Path::new("my-docs")), "my-docs");
        assert_eq!(sanitize_root_name(Path::new("中文 docs")), "docs");
        assert_eq!(sanitize_root_name(Path::new("")), "site");
    }

    #[test]
    fn build_writes_all_pages_and_assets() {
        let src = tmp("src");
        let out = tmp("out");
        write(src.join("README.md"), "# Home\nHello [[Intro]]\n");
        write(src.join("guide/intro.md"), "# Intro\nbody\n");
        write(
            src.join("guide/deep/page.md"),
            "# Deep\n```mermaid\nflowchart LR\nA-->B\n```\n",
        );

        let renderer = Renderer::new();
        let report = build(
            BuildOptions {
                root: src.clone(),
                out_dir: out.clone(),
                site_name: "docs".into(),
                theme: "auto".into(),
                live_reload_js: String::new(),
            },
            &renderer,
        )
        .unwrap();

        // Files exist
        assert!(out.join("README.html").exists());
        assert!(out.join("guide/intro.html").exists());
        assert!(out.join("guide/deep/page.html").exists());
        // Assets exist
        assert!(out.join("assets/style.css").exists());
        assert!(out.join("assets/app.js").exists());
        // Landing prefers README
        assert!(report.landing.ends_with("README.html"));
        // No warnings for clean test sources.
        assert!(report.warnings.is_empty(), "{:?}", report.warnings);

        // Cross-file wikilink resolved: README.html should link to guide/intro.html.
        let home = fs::read_to_string(out.join("README.html")).unwrap();
        assert!(
            home.contains(r#"href="guide/intro.html""#),
            "wikilink not resolved across tree: {home}"
        );
        // Nested page should reference ../../assets/style.css.
        let deep = fs::read_to_string(out.join("guide/deep/page.html")).unwrap();
        assert!(
            deep.contains(r#"href="../../assets/style.css""#),
            "deep assets path wrong: {deep}"
        );
        // Mermaid feature on the deep page should pull in the mermaid script.
        assert!(deep.contains("mermaid.min.js"));
    }

    #[test]
    fn build_returns_warnings_anchored_to_source_path() {
        // A page with a duplicate explicit id surfaces one warning; that
        // warning must be attached to the abs path of its source file so the
        // CLI can render it with file context.
        let src = tmp("warns-src");
        let out = tmp("warns-out");
        write(src.join("dup.md"), "# Alpha {#same}\n# Beta {#same}\n");
        write(src.join("clean.md"), "# Clean page\n");
        let renderer = Renderer::new();
        let report = build(
            BuildOptions {
                root: src.clone(),
                out_dir: out.clone(),
                ..Default::default()
            },
            &renderer,
        )
        .unwrap();
        assert_eq!(report.warnings.len(), 1, "{:?}", report.warnings);
        let w = &report.warnings[0];
        // macOS canonicalises `src` through /private/var, so just verify the
        // source resolves to the same file as the input we wrote to.
        assert!(
            w.source.ends_with("dup.md"),
            "warning source not anchored to dup.md: {}",
            w.source.display()
        );
        assert_eq!(
            w.warning.kind,
            crate::mt::render::WarningKind::DuplicateExplicitId
        );
        assert_eq!(w.warning.detail, "same");
        assert_eq!(w.warning.heading_text.as_deref(), Some("Beta"));
    }

    #[test]
    fn nav_and_pager_labels_match_rendered_title() {
        // Regression for the extract_meta divergence: nav / pager labels are
        // derived from extract_meta during `prepare`, while the per-page
        // <title> comes from the full render. With the AST-based meta pass
        // they must agree even when the heading contains wikilinks, inline
        // markdown, or other comrak-level constructs.
        let src = tmp("title-parity-src");
        let out = tmp("title-parity-out");
        write(src.join("README.md"), "# Home\nintro\n");
        write(src.join("link.md"), "# Real [[Page|Alias]]\nbody\n");
        write(src.join("emph.md"), "# **Bold** rest\nbody\n");
        let renderer = Renderer::new();
        build(
            BuildOptions {
                root: src.clone(),
                out_dir: out.clone(),
                ..Default::default()
            },
            &renderer,
        )
        .unwrap();

        // Each page's nav block (rendered from the tree) names sibling links
        // by the title we extracted. Confirm those names match what the page
        // itself shows via <title>.
        let home = fs::read_to_string(out.join("README.html")).unwrap();
        let link = fs::read_to_string(out.join("link.html")).unwrap();
        let emph = fs::read_to_string(out.join("emph.html")).unwrap();

        assert!(
            link.contains("<title>Real Alias</title>"),
            "wikilink heading didn't flatten to title: {link}"
        );
        // Both prev/next labels and nav labels must use the same string.
        // home.html lists link.html in the nav — find the label cell.
        assert!(
            home.contains(">Real Alias<"),
            "nav label didn't pick up wikilink alias: {home}"
        );
        assert!(
            emph.contains("<title>Bold rest</title>") && home.contains(">Bold rest<"),
            "inline markdown title mismatch between page and nav"
        );
        // Raw `[[Page|Alias]]` must not leak into the nav.
        assert!(
            !home.contains("[[Page|Alias]]"),
            "raw wikilink syntax leaked into nav: {home}"
        );
    }

    #[test]
    fn build_orders_pager_alphabetically() {
        let src = tmp("pager-src");
        let out = tmp("pager-out");
        write(src.join("a.md"), "# A");
        write(src.join("b.md"), "# B");
        write(src.join("c.md"), "# C");
        let renderer = Renderer::new();
        build(
            BuildOptions {
                root: src.clone(),
                out_dir: out.clone(),
                ..Default::default()
            },
            &renderer,
        )
        .unwrap();
        let b = fs::read_to_string(out.join("b.html")).unwrap();
        // Prev → a.html, Next → c.html
        assert!(b.contains(r#"href="a.html""#), "prev wrong: {b}");
        assert!(b.contains(r#"href="c.html""#), "next wrong: {b}");
    }
}
