//! Markdown → HTML pipeline. Mirror of Go `internal/render/render.go`.
//!
//! Composition (only the first two passes touch raw source — the rest run on
//! comrak's AST so they get correct fenced-code / blockquote / list scoping
//! for free):
//!
//! 1. split YAML frontmatter
//! 2. **Source pass** — only what comrak truly can't parse:
//!    - `preprocess_admonition` (MkDocs `!!! note "x"` → raw `<div>`)
//! 3. comrak `parse_document`
//! 4. **AST passes** — token-aware, fenced-code / link / image safe:
//!    - `transform_wikilinks` (`[[Page]]` → inline link, skipping Code /
//!      CodeBlock / inside Link/Image)
//!    - `normalize_mermaid_codeblocks` (escape rewrites only inside mermaid fences)
//!    - `strip_cjk_softbreaks_ast` (drop `SoftBreak` between CJK siblings — works
//!      in `<p>`, `<li>`, `<blockquote>`, …)
//!    - `process_headings` → [`HeadingRegistry`] (single source of truth for
//!      id / class / attrs / display text; consumed by both TOC and the
//!      `MtHeadingAdapter` strictly by ordinal — no source-line correlation)
//! 5. comrak `format_html_with_plugins` (+ syntect highlighter + heading adapter)

pub mod ast_passes;
pub mod diagnostics;
pub mod frontmatter;
pub mod heading;
pub mod highlight;
pub mod preprocess;
pub mod toc;

use std::io::Write;
use std::path::Path;
use std::sync::atomic::{AtomicUsize, Ordering};

use comrak::adapters::{HeadingAdapter, HeadingMeta};
use comrak::nodes::Sourcepos;
use comrak::{Arena, Options, Plugins, format_html_with_plugins, parse_document};

use ast_passes::{
    detect_features_ast, normalize_mermaid_codeblocks, strip_cjk_softbreaks, transform_wikilinks,
};

pub use diagnostics::{RenderWarning, WarningKind};
pub use frontmatter::{Frontmatter, FrontmatterError, split_frontmatter};
pub use heading::{HeadingRecord, HeadingRegistry, HeadingRenderInfo, process_headings};
pub use highlight::{SyntectAdapter, SyntectBundle, highlight_css};
pub use preprocess::{Features, WikilinkResolver, detect_features, preprocess_admonition};
pub use toc::{TocNode, build_toc, render_toc};

/// Outcome of one render pass.
#[derive(Debug, Default)]
pub struct RenderResult {
    pub title: String,
    pub description: String,
    pub tags: Vec<String>,
    pub theme: String,
    pub body: String,
    pub toc: Vec<TocNode>,
    pub toc_html: String,
    pub features: Features,
    pub frontmatter: Frontmatter,
    /// Non-fatal diagnostics surfaced by the pipeline (dropped unsafe
    /// heading attributes, duplicate explicit ids, …). The CLI prints these
    /// to stderr; rendering itself never fails because of them.
    pub warnings: Vec<RenderWarning>,
}

#[derive(Debug)]
pub enum RenderError {
    Frontmatter(FrontmatterError),
    Format(std::io::Error),
    Utf8(std::string::FromUtf8Error),
}

impl std::fmt::Display for RenderError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RenderError::Frontmatter(e) => write!(f, "frontmatter: {e}"),
            RenderError::Format(e) => write!(f, "format: {e}"),
            RenderError::Utf8(e) => write!(f, "utf8: {e}"),
        }
    }
}

impl std::error::Error for RenderError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            RenderError::Frontmatter(e) => Some(e),
            RenderError::Format(e) => Some(e),
            RenderError::Utf8(e) => Some(e),
        }
    }
}

impl From<FrontmatterError> for RenderError {
    fn from(e: FrontmatterError) -> Self {
        RenderError::Frontmatter(e)
    }
}
impl From<std::io::Error> for RenderError {
    fn from(e: std::io::Error) -> Self {
        RenderError::Format(e)
    }
}
impl From<std::string::FromUtf8Error> for RenderError {
    fn from(e: std::string::FromUtf8Error) -> Self {
        RenderError::Utf8(e)
    }
}

/// Cheap metadata extracted without running the full render pipeline. Used by
/// the site builder for the "first pass" where it only needs each page's
/// title + frontmatter for the navigation tree.
#[derive(Debug, Default, Clone)]
pub struct PageMeta {
    pub title: String,
    pub frontmatter: Frontmatter,
}

/// Pulls just enough from the source to populate [`PageMeta`]: YAML
/// frontmatter (full) + the document title.
///
/// Title derivation runs the *same* AST pipeline as [`Renderer::render`] up
/// through heading collection — frontmatter → admonition preprocess → comrak
/// parse → wikilink transform → `process_headings` → `pick_title` — so the
/// nav / pager label always matches the page's own `<title>` and TOC.
///
/// What's skipped vs. a full render:
///   * `normalize_mermaid_codeblocks` + `strip_cjk_softbreaks` (don't affect
///     heading text)
///   * `SyntectAdapter` highlighting
///   * `format_html_with_plugins` + the minijinja page template
///
/// That keeps it cheap enough to call on every entry during the site
/// builder's "prepare" phase, where we just want titles for the nav.
pub fn extract_meta(src: &str, fallback_name: &str) -> Result<PageMeta, RenderError> {
    let (fm, body) = split_frontmatter(src)?;
    let pre = preprocess_admonition(&body);
    let arena = Arena::new();
    let opts = comrak_options();
    let root = parse_document(&arena, &pre, &opts);

    // Wikilinks: pass `None` for the resolver — prepare runs before the
    // name index is built, and the resolver only affects the `href`, never
    // the visible text node that `process_headings` walks for the title.
    transform_wikilinks(&arena, root, None);

    let registry = process_headings(root);
    let toc = build_toc(&registry.records, 4);
    let title = pick_title(&fm.title, &toc, fallback_name);
    Ok(PageMeta {
        title,
        frontmatter: fm,
    })
}

/// Shared comrak parser options — `render_with` and `extract_meta` MUST use
/// the same set so the AST they observe is identical (otherwise the title
/// extracted by the cheap meta path could diverge from the actual rendered
/// `<title>`).
fn comrak_options() -> Options {
    let mut opts = Options::default();
    // Extensions — match Go side (goldmark GFM + footnote + deflist + CJK).
    opts.extension.strikethrough = true;
    opts.extension.tagfilter = false; // we explicitly want raw HTML to pass through
    opts.extension.table = true;
    opts.extension.autolink = true;
    opts.extension.tasklist = true;
    opts.extension.superscript = false;
    // We emit `<h1 id="...">` via the HeadingAdapter ourselves, so leave
    // comrak's built-in `header_ids` disabled (it would also wrap the
    // heading text in an `<a id="...">` anchor that misplaces the id).
    opts.extension.header_ids = None;
    opts.extension.footnotes = true;
    opts.extension.description_lists = true;
    opts.extension.multiline_block_quotes = false;
    // Leave math parsing to MathJax on the client — comrak's math_dollars
    // would consume `$...$` into a `<span data-math-style>` wrapper, which
    // MathJax can't see (its template config scans for raw `$...$` /
    // `$$...$$` delimiters). The Go side (goldmark) does the same.
    opts.extension.math_dollars = false;
    opts.extension.math_code = false;
    opts.extension.wikilinks_title_after_pipe = false;
    opts.extension.wikilinks_title_before_pipe = false;
    opts.extension.underline = false;
    opts.extension.spoiler = false;
    opts.extension.greentext = false;
    // Match Go side's `extension.Typographer`: smart quotes, en/em dashes,
    // ellipses. comrak's `parse.smart` covers the same set.
    opts.parse.smart = true;
    opts.render.unsafe_ = true; // allow our admonition/wikilink raw HTML through
    opts.render.hardbreaks = false;
    opts.render.github_pre_lang = false; // `<code class="language-X">` for mermaid hooks
    opts.render.sourcepos = false; // we read sourcepos from AST nodes directly
    opts
}

/// Stateful renderer — caches the syntect syntax/theme tables across calls.
pub struct Renderer {
    bundle: SyntectBundle,
}

impl Default for Renderer {
    fn default() -> Self {
        Self::new()
    }
}

impl Renderer {
    pub fn new() -> Self {
        Self {
            bundle: SyntectBundle::new(),
        }
    }

    /// Render with default options (no wikilink resolver).
    pub fn render(&self, src: &str, fallback_name: &str) -> Result<RenderResult, RenderError> {
        self.render_with(src, fallback_name, None)
    }

    /// Render with an optional wikilink resolver. The resolver is called for
    /// every `[[Page]]` target; returning None falls back to `<slug>.html`.
    pub fn render_with(
        &self,
        src: &str,
        fallback_name: &str,
        resolver: Option<&WikilinkResolver<'_>>,
    ) -> Result<RenderResult, RenderError> {
        let (fm, body) = split_frontmatter(src)?;

        // ----- Source passes — anything comrak can't see otherwise -----
        // Admonitions aren't part of CommonMark/GFM, so we rewrite them in
        // the raw source before parsing. Heading attribute blocks
        // (`{#id .class data-x="v"}`) live on heading inline children and are
        // handled in the AST pass below — no source-line tracking involved.
        let pre = preprocess_admonition(&body);

        let arena = Arena::new();
        let opts = comrak_options();

        let root = parse_document(&arena, &pre, &opts);

        // ----- AST passes — same semantic intent as the old source regex
        // passes, but with proper token awareness (no rewriting inside fenced
        // code blocks, no misfires inside HTML / links).
        transform_wikilinks(&arena, root, resolver);
        normalize_mermaid_codeblocks(root);
        strip_cjk_softbreaks(root);

        // Feature flags now derive from the AST (so `$...$` inside a Python
        // fence no longer triggers MathJax loading). Must run *after* the
        // AST passes above so any normalization that changes node values is
        // already in effect.
        let features = detect_features_ast(root);

        // Single source of truth for heading metadata: ID, class, attrs, text.
        // Both the TOC and the renderer adapter project from this registry —
        // no source-line tracking anywhere downstream.
        let registry = process_headings(root);
        let toc_tree = build_toc(&registry.records, 4);
        let toc_html = render_toc(&toc_tree);

        let syntect = SyntectAdapter::new(&self.bundle);
        let heading_adapter = MtHeadingAdapter::new(registry.render_infos());
        let mut plugins = Plugins::default();
        plugins.render.codefence_syntax_highlighter = Some(&syntect);
        plugins.render.heading_adapter = Some(&heading_adapter);

        let mut html_buf: Vec<u8> = Vec::new();
        format_html_with_plugins(root, &opts, &mut html_buf, &plugins)?;
        let body_html = String::from_utf8(html_buf)?;

        let title = pick_title(&fm.title, &toc_tree, fallback_name);

        Ok(RenderResult {
            title,
            description: fm.description.clone(),
            tags: fm.tags.clone(),
            theme: fm.theme.clone(),
            body: body_html,
            toc_html,
            toc: toc_tree,
            features,
            frontmatter: fm,
            warnings: registry.warnings,
        })
    }
}

/// HeadingAdapter that emits `<h{level} id="…" class="…" key="…">` directly
/// on the heading element (vs comrak's default `<h1><a id="…" …>…</a></h1>`).
///
/// Looks up render info by **document-order ordinal**, not source line — the
/// `next` counter is incremented on each `enter` call. This relies on the
/// invariant that comrak renders headings in the same order as preorder AST
/// traversal, which the [`HeadingRegistry`] builder also uses.
struct MtHeadingAdapter {
    infos: Vec<HeadingRenderInfo>,
    next: AtomicUsize,
}

impl MtHeadingAdapter {
    fn new(infos: Vec<HeadingRenderInfo>) -> Self {
        Self {
            infos,
            next: AtomicUsize::new(0),
        }
    }
}

impl HeadingAdapter for MtHeadingAdapter {
    fn enter(
        &self,
        output: &mut dyn Write,
        meta: &HeadingMeta,
        _sourcepos: Option<Sourcepos>,
    ) -> std::io::Result<()> {
        let idx = self.next.fetch_add(1, Ordering::SeqCst);
        let info = self.infos.get(idx);
        let id = info.map(|i| i.id.as_str()).unwrap_or("");
        let classes = info.map(|i| i.classes.as_slice()).unwrap_or(&[]);
        let attrs = info.map(|i| i.attrs.as_slice()).unwrap_or(&[]);

        write!(output, "<h{}", meta.level)?;
        if !id.is_empty() {
            write!(output, " id=\"{}\"", toc::html_escape(id))?;
        }
        if !classes.is_empty() {
            write!(output, " class=\"{}\"", join_classes(classes))?;
        }
        for (k, v) in attrs {
            // `k` already passed `is_safe_attr_name` upstream, so it's pure
            // [A-Za-z_:][\w.:-]* — no escaping needed for the name.
            write!(output, " {}=\"{}\"", k, toc::html_escape(v))?;
        }
        write!(output, ">")
    }

    fn exit(&self, output: &mut dyn Write, meta: &HeadingMeta) -> std::io::Result<()> {
        write!(output, "</h{}>", meta.level)
    }
}

fn join_classes(classes: &[String]) -> String {
    let escaped: Vec<String> = classes.iter().map(|c| toc::html_escape(c)).collect();
    escaped.join(" ")
}

fn pick_title(fm_title: &str, toc: &[TocNode], fallback_name: &str) -> String {
    if !fm_title.is_empty() {
        return fm_title.to_string();
    }
    if let Some(h1) = toc.iter().find(|n| n.level == 1) {
        return h1.text.clone();
    }
    if let Some(first) = toc.first() {
        return first.text.clone();
    }
    Path::new(fallback_name)
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("")
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extract_meta_returns_frontmatter_title() {
        let meta = extract_meta("---\ntitle: From FM\n---\n# H1\nbody\n", "fallback.md").unwrap();
        assert_eq!(meta.title, "From FM");
        assert_eq!(meta.frontmatter.title, "From FM");
    }

    #[test]
    fn extract_meta_matches_render_title_priority() {
        // `pick_title` (shared with render) prefers an H1 over earlier
        // lower-level headings, so the nav label tracks the page's own
        // <title>. The trailing `{#x}` attribute block is stripped.
        let src = "intro\n\n## Sub {#x}\n\n# Body\n";
        let meta = extract_meta(src, "x.md").unwrap();
        assert_eq!(meta.title, "Body");
        let full = Renderer::new().render(src, "x.md").unwrap();
        assert_eq!(meta.title, full.title);
    }

    #[test]
    fn extract_meta_uses_first_heading_when_no_h1() {
        let meta = extract_meta("## Only Sub\n", "x.md").unwrap();
        assert_eq!(meta.title, "Only Sub");
    }

    #[test]
    fn extract_meta_ignores_headings_inside_code_fence() {
        let meta = extract_meta("```\n# Fake\n```\n\n# Real\n", "x.md").unwrap();
        assert_eq!(meta.title, "Real");
    }

    #[test]
    fn extract_meta_falls_back_to_filename_stem() {
        let meta = extract_meta("no headings here.\n", "/path/to/My-Doc.md").unwrap();
        assert_eq!(meta.title, "My-Doc");
    }

    #[test]
    fn extract_meta_resolves_wikilink_alias_in_heading() {
        // Regression: the old raw-text scan returned `# Link [[Page|Alias]]`,
        // diverging from the actual rendered <title>. The AST pipeline now
        // walks Text nodes inside the wikilink, so the alias `Alias` ends up
        // in the heading text — matching what render_with produces.
        let meta = extract_meta("# Link [[Page|Alias]]\n", "x.md").unwrap();
        assert_eq!(meta.title, "Link Alias");

        // And the same input through full render gives the same title — the
        // whole point of routing extract_meta through the AST.
        let full = Renderer::new()
            .render("# Link [[Page|Alias]]\n", "x.md")
            .unwrap();
        assert_eq!(meta.title, full.title);
    }

    #[test]
    fn extract_meta_handles_setext_heading() {
        // Setext (`=====` underline) headings were silently skipped by the
        // raw line scan. comrak parses them as Heading nodes so the AST
        // pipeline picks them up.
        let meta = extract_meta("Setext Title\n============\nbody\n", "x.md").unwrap();
        assert_eq!(meta.title, "Setext Title");
    }

    #[test]
    fn extract_meta_unwraps_inline_markdown_in_heading() {
        // `# **Bold** & _emph_` previously made it through verbatim; the AST
        // walk collects the visible Text children, so the title is the
        // plain-text form — same as render_with's `pick_title`.
        let meta = extract_meta("# **Bold** rest\n", "x.md").unwrap();
        assert_eq!(meta.title, "Bold rest");
        let full = Renderer::new().render("# **Bold** rest\n", "x.md").unwrap();
        assert_eq!(meta.title, full.title);
    }

    #[test]
    fn basic_render() {
        let r = Renderer::new();
        let res = r
            .render("# Title\n\nHello **world**.\n", "fallback.md")
            .unwrap();
        assert_eq!(res.title, "Title");
        assert!(
            res.body.contains("<strong>world</strong>"),
            "body = {}",
            res.body
        );
        assert!(!res.toc.is_empty());
    }

    #[test]
    fn frontmatter_title_wins() {
        let r = Renderer::new();
        let res = r
            .render(
                "---\ntitle: From FM\n---\n# H1 Title\nbody\n",
                "fallback.md",
            )
            .unwrap();
        assert_eq!(res.title, "From FM");
    }

    #[test]
    fn fallback_from_filename() {
        let r = Renderer::new();
        let res = r.render("just text", "/path/to/My-Doc.md").unwrap();
        assert_eq!(res.title, "My-Doc");
    }

    #[test]
    fn features_detected() {
        let r = Renderer::new();
        let src = "text with $E=mc^2$.\n\n```mermaid\nflowchart LR\nA-->B\n```\n";
        let res = r.render(src, "").unwrap();
        assert!(res.features.has_math);
        assert!(res.features.has_mermaid);
    }

    #[test]
    fn admonition_inline() {
        let r = Renderer::new();
        let res = r
            .render("!!! note \"Hello\"\n    Inside **bold**.\n", "")
            .unwrap();
        assert!(res.body.contains("admonition-note"), "body = {}", res.body);
        assert!(res.body.contains("<strong>bold</strong>"));
    }

    #[test]
    fn admonition_inside_fenced_code_is_left_alone() {
        // Regression for issue #2: `!!! note` inside ``` was being hijacked
        // as an admonition. The source-level pass is now fence-aware.
        let r = Renderer::new();
        let res = r
            .render("```\n!!! note \"Bug\"\n    body line\n```\n\nafter\n", "")
            .unwrap();
        assert!(
            !res.body.contains("admonition-note"),
            "admonition fired inside fence: {}",
            res.body
        );
        // The literal `!!! note` survives in the rendered <pre><code>.
        assert!(
            res.body.contains("!!! note"),
            "literal admonition syntax not preserved: {}",
            res.body
        );
    }

    #[test]
    fn admonition_with_tilde_fence_also_skipped() {
        let r = Renderer::new();
        let res = r.render("~~~\n!!! warning\n~~~\n", "").unwrap();
        assert!(
            !res.body.contains("admonition-warning"),
            "tilde fence didn't shield admonition: {}",
            res.body
        );
    }

    #[test]
    fn wikilink_rendered() {
        let r = Renderer::new();
        let res = r.render("See [[Other]].\n", "").unwrap();
        assert!(res.body.contains(r#"class="mt-wikilink""#));
        assert!(res.body.contains(r#"href="Other.html""#));
    }

    #[test]
    fn wikilinks_in_code_block_left_alone() {
        // Before the AST migration, `[[X]]` inside ``` blocks (and `[[Y]]`
        // inside inline code) got rewritten. The AST-level pass scopes to
        // Text nodes only, which CodeBlock + Code don't expose.
        let r = Renderer::new();
        let res = r
            .render(
                "regular [[Page]]\n\n```\n[[Should not rewrite]]\n```\n\nInline `[[Also not]]` here.\n",
                "",
            )
            .unwrap();
        // The free-text wikilink IS rewritten.
        assert!(
            res.body.contains(r#"href="Page.html""#),
            "free-text wikilink missing: {}",
            res.body
        );
        // Fenced code body retains the original `[[...]]` literally.
        assert!(
            res.body.contains("[[Should not rewrite]]"),
            "fenced code wikilink got rewritten: {}",
            res.body
        );
        // Inline code retains the original `[[...]]` literally.
        assert!(
            res.body.contains("[[Also not]]"),
            "inline code wikilink got rewritten: {}",
            res.body
        );
    }

    #[test]
    fn wikilinks_inside_emphasis_still_rewritten() {
        // emphasis nodes wrap Text children, so the AST pass should still
        // descend into them. Verifies the walk isn't over-eager about skipping.
        let r = Renderer::new();
        let res = r.render("_see [[Detail]]_ here.\n", "").unwrap();
        assert!(
            res.body
                .contains(r#"<em>see <a class="mt-wikilink" href="Detail.html">Detail</a></em>"#),
            "emphasis-wrapped wikilink not rewritten: {}",
            res.body
        );
    }

    #[test]
    fn wikilink_in_heading_contributes_to_title_and_toc() {
        // The wikilink AST pass now splits the link into <a> / Text(alias) /
        // </a>, so heading text collection (which walks Text/Code nodes) picks
        // up the alias. Title and TOC must show "Link Alias", not just "Link".
        let r = Renderer::new();
        let res = r.render("# Link [[Page|Alias]]\n", "").unwrap();
        assert_eq!(res.title, "Link Alias", "title lost alias: {}", res.title);
        // TOC entry text mirrors the heading.
        assert!(
            res.toc_html.contains(">Link Alias</a>"),
            "TOC missing alias: {}",
            res.toc_html
        );
        // The rendered <h1> still has the wikilink anchor in place.
        assert!(
            res.body
                .contains(r#"<a class="mt-wikilink" href="Page.html">Alias</a>"#),
            "wikilink anchor missing inside heading: {}",
            res.body
        );
    }

    #[test]
    fn wikilink_uses_resolver() {
        let r = Renderer::new();
        let resolver: &WikilinkResolver = &|name: &str| {
            if name == "Other" {
                Some("custom/o.html".into())
            } else {
                None
            }
        };
        let res = r
            .render_with("See [[Other]].\n", "", Some(resolver))
            .unwrap();
        assert!(res.body.contains(r#"href="custom/o.html""#));
    }

    #[test]
    fn footnote_and_tasklist() {
        let r = Renderer::new();
        let src = "Note[^1].\n\n- [x] done\n- [ ] todo\n\n[^1]: footnote text\n";
        let res = r.render(src, "").unwrap();
        assert!(
            res.body.contains(r#"type="checkbox""#),
            "no checkbox: {}",
            res.body
        );
        assert!(
            res.body.contains("footnote-ref")
                || res.body.contains("href=\"#fn")
                || res.body.contains("footnote"),
            "no footnote: {}",
            res.body
        );
    }

    #[test]
    fn heading_id_lives_on_h_element() {
        let r = Renderer::new();
        let res = r.render("# Hello World\n", "").unwrap();
        // id attribute must be on the <h1>, not on a wrapped <a>.
        assert!(
            res.body.contains(r#"<h1 id="hello-world">"#),
            "expected <h1 id=…>, got: {}",
            res.body
        );
        assert!(
            !res.body.contains(r#"<a"#) || !res.body.contains(r#"aria-hidden"#),
            "comrak's anchor wrapper leaked: {}",
            res.body
        );
        // TOC anchors agree.
        assert!(res.toc_html.contains(r##"href="#hello-world""##));
    }

    #[test]
    fn unicode_heading_uses_go_style_slug() {
        let r = Renderer::new();
        let res = r.render("# Café Déjà\n## 你好 世界\n", "").unwrap();
        // Matches Go's autoheading-id rules: non-ASCII dropped, spaces → '-'.
        assert!(
            res.body.contains(r#"<h1 id="caf-dj">"#),
            "caf-dj missing: {}",
            res.body
        );
        assert!(
            res.body.contains(r#"<h2 id="-">"#),
            "dash-only slug missing: {}",
            res.body
        );
    }

    #[test]
    fn slug_empty_falls_back_to_literal_heading() {
        let r = Renderer::new();
        let res = r.render("# 你好\n", "").unwrap();
        assert!(
            res.body.contains(r#"<h1 id="heading">"#),
            "empty-slug fallback missing: {}",
            res.body
        );
    }

    #[test]
    fn slug_reflects_typographer_smart_dashes() {
        // The heading registry now derives slugs from the AST, which has
        // already gone through comrak's smart-punctuation pass, so `---`
        // becomes U+2014 (em dash) in the heading text. ASCII slugify drops
        // it and we get `foobar`. The `<h1>` body shows the same em-dash, so
        // body text and id remain mutually consistent — the cross-impl
        // parity with goldmark's raw-text slug is intentionally given up.
        let r = Renderer::new();
        let res = r.render("# Foo---Bar\n", "").unwrap();
        assert!(
            res.body.contains(r#"<h1 id="foobar">"#),
            "post-typographer slug missing: {}",
            res.body
        );
        // Body uses the em-dash.
        assert!(
            res.body.contains('\u{2014}'),
            "em-dash missing: {}",
            res.body
        );
    }

    #[test]
    fn slug_underscore_becomes_dash() {
        let r = Renderer::new();
        let res = r.render("# A_B\n", "").unwrap();
        assert!(
            res.body.contains(r#"<h1 id="a-b">"#),
            "underscore not converted: {}",
            res.body
        );
    }

    #[test]
    fn heading_attrs_passed_through_with_escaping() {
        let r = Renderer::new();
        let res = r
            .render("## KV {#id .foo data-x=\"a b\" lang=en}\n", "")
            .unwrap();
        assert!(
            res.body
                .contains(r#"<h2 id="id" class="foo" data-x="a b" lang="en">"#),
            "attrs missing or wrong order: {}",
            res.body
        );
    }

    #[test]
    fn heading_attrs_reject_invalid_name_shape() {
        // `1bad` starts with a digit → silently dropped (malformed shape).
        // `good` has a valid identifier shape but isn't in the safety
        // whitelist → dropped AND a RenderWarning is emitted.
        let r = Renderer::new();
        let res = r.render("# H {1bad=oops good=ok}\n", "").unwrap();
        assert!(
            !res.body.contains("1bad"),
            "malformed attr leaked: {}",
            res.body
        );
        assert!(
            !res.body.contains("good=") && !res.body.contains(r#"good="ok""#),
            "non-whitelisted attr leaked: {}",
            res.body
        );
        assert_eq!(res.warnings.len(), 1);
        assert_eq!(res.warnings[0].kind, WarningKind::DroppedHeadingAttr);
        assert_eq!(res.warnings[0].detail, "good");
    }

    #[test]
    fn heading_attrs_drops_event_handler_and_warns() {
        let r = Renderer::new();
        let res = r
            .render(r#"# H {onclick="alert(1)"}"#.to_string().as_str(), "")
            .unwrap();
        assert!(
            !res.body.contains("onclick"),
            "onclick leaked into HTML: {}",
            res.body
        );
        assert_eq!(res.warnings.len(), 1);
        assert_eq!(res.warnings[0].kind, WarningKind::DroppedHeadingAttr);
        assert_eq!(res.warnings[0].detail, "onclick");
    }

    #[test]
    fn heading_attrs_value_escapes_quotes_and_brackets() {
        // Pathological filename-like values must not break the HTML attribute.
        let r = Renderer::new();
        let res = r.render("# T {data-x=\"a\\\"<b>&c\"}\n", "").unwrap();
        // The literal quote is interpreted by our parser as end of value, so
        // value becomes `a\` — we only need to verify the *output* is well-formed.
        assert!(res.body.contains(r#"<h1"#));
        // No raw `<b>` should land inside the attribute.
        assert!(
            !res.body.contains(r#"data-x="a\"<b>"#),
            "raw < leaked: {}",
            res.body
        );
    }

    #[test]
    fn heading_class_attribute_emitted() {
        let r = Renderer::new();
        let res = r.render("# With Class {#id .foo .bar}\n", "").unwrap();
        assert!(
            res.body.contains(r#"<h1 id="id" class="foo bar">"#),
            "class attribute wrong: {}",
            res.body
        );
        // Inner text must not retain the `{...}` syntax.
        assert!(!res.body.contains("{#id"), "raw attr leaked: {}", res.body);
    }

    #[test]
    fn cjk_softbreak_joins_lines() {
        let r = Renderer::new();
        let res = r.render("这是\n中文\n", "").unwrap();
        assert!(
            res.body.contains("这是中文"),
            "CJK softbreak not joined: {}",
            res.body
        );
        assert!(
            !res.body.contains("这是\n中文"),
            "literal newline survived: {}",
            res.body
        );
    }

    #[test]
    fn cjk_softbreak_joins_inside_blockquote() {
        let r = Renderer::new();
        let res = r.render("> 这是\n> 中文\n", "").unwrap();
        // The literal newline between CJK chars must be gone in the blockquote.
        assert!(
            res.body.contains("这是中文"),
            "blockquote CJK softbreak not joined: {}",
            res.body
        );
    }

    #[test]
    fn cjk_softbreak_joins_inside_list_item() {
        let r = Renderer::new();
        let res = r.render("- 这是\n  中文\n", "").unwrap();
        assert!(
            res.body.contains("这是中文"),
            "list-item CJK softbreak not joined: {}",
            res.body
        );
    }

    #[test]
    fn cjk_softbreak_preserves_pre_block() {
        // Inside fenced code, the newline must survive — code semantics depend on it.
        let r = Renderer::new();
        let res = r.render("```\n这是\n中文\n```\n", "").unwrap();
        // Comrak escapes content inside <code>, so the chars stay verbatim
        // including the newline.
        assert!(
            res.body.contains("这是\n中文") || res.body.contains("这是\r\n中文"),
            "code-block newline got stripped: {}",
            res.body
        );
    }

    #[test]
    fn typographer_replaces_quotes_and_dashes() {
        let r = Renderer::new();
        let res = r.render("\"quote\" -- text...\n", "").unwrap();
        // Curly quotes (8220 / 8221) and en-dash (8211) and ellipsis (8230).
        assert!(
            res.body.contains('\u{201c}'),
            "no opening curly quote: {}",
            res.body
        );
        assert!(
            res.body.contains('\u{201d}'),
            "no closing curly quote: {}",
            res.body
        );
        assert!(res.body.contains('\u{2013}'), "no en-dash: {}", res.body);
        assert!(res.body.contains('\u{2026}'), "no ellipsis: {}", res.body);
    }

    #[test]
    fn duplicate_explicit_id_is_suffixed_and_warned() {
        // Three headings all pin `{#same}`. Suffix scheme matches the
        // auto-slug path so users don't see `same → same-2` jumps:
        // explicit collisions go `same → same-1 → same-2`.
        let r = Renderer::new();
        let res = r
            .render("# Alpha {#same}\n# Beta {#same}\n# Gamma {#same}\n", "")
            .unwrap();
        assert!(
            res.body.contains(r#"<h1 id="same">"#),
            "first explicit id missing: {}",
            res.body
        );
        assert!(
            res.body.contains(r#"<h1 id="same-1">"#),
            "second explicit id should be suffixed -1: {}",
            res.body
        );
        assert!(
            res.body.contains(r#"<h1 id="same-2">"#),
            "third explicit id should be suffixed -2: {}",
            res.body
        );
        assert!(res.toc_html.contains(r##"href="#same""##));
        assert!(res.toc_html.contains(r##"href="#same-1""##));
        assert!(res.toc_html.contains(r##"href="#same-2""##));
        let dup_warnings: Vec<&RenderWarning> = res
            .warnings
            .iter()
            .filter(|w| w.kind == WarningKind::DuplicateExplicitId)
            .collect();
        assert_eq!(dup_warnings.len(), 2);
        assert!(dup_warnings.iter().all(|w| w.detail == "same"));
        // Each warning carries the offending heading's ordinal + text so
        // the CLI can point at the source line without source-pos data.
        assert_eq!(dup_warnings[0].heading_ordinal, Some(1));
        assert_eq!(dup_warnings[0].heading_text.as_deref(), Some("Beta"));
        assert_eq!(dup_warnings[1].heading_ordinal, Some(2));
        assert_eq!(dup_warnings[1].heading_text.as_deref(), Some("Gamma"));
    }

    #[test]
    fn custom_heading_id_supported() {
        let r = Renderer::new();
        let res = r.render("# Real Title {#my-id}\n## Sub\n", "").unwrap();
        assert!(
            res.body.contains(r#"<h1 id="my-id">"#),
            "custom id missing on <h1>: {}",
            res.body
        );
        // The `{#my-id}` literal must NOT appear in the rendered text.
        assert!(
            !res.body.contains("{#my-id}"),
            "raw syntax leaked: {}",
            res.body
        );
        // Subheading without override falls back to anchorize.
        assert!(
            res.body.contains(r#"<h2 id="sub">"#),
            "h2 slug wrong: {}",
            res.body
        );
        // TOC tracks the custom id.
        assert!(
            res.toc_html.contains(r##"href="#my-id""##),
            "toc miss: {}",
            res.toc_html
        );
    }

    #[test]
    fn math_dollars_survive_for_mathjax() {
        let r = Renderer::new();
        let res = r
            .render(
                "Inline $E=mc^2$ and display:\n\n$$\\int_0^\\infty e^{-x^2}\\,dx$$\n",
                "",
            )
            .unwrap();
        // Inline $...$ must reach the browser untouched (no comrak math wrappers).
        assert!(
            res.body.contains("$E=mc^2$"),
            "inline math dollars consumed: {}",
            res.body
        );
        // Display math too.
        assert!(
            res.body.contains("$$"),
            "display math dollars stripped: {}",
            res.body
        );
        // Detection still fires so the page knows to load MathJax.
        assert!(res.features.has_math, "feature flag missed");
        // And no comrak math wrapper sneaks through.
        assert!(
            !res.body.contains("data-math-style"),
            "comrak math wrapper leaked: {}",
            res.body
        );
    }

    #[test]
    fn mermaid_passthrough_in_html() {
        let r = Renderer::new();
        let res = r
            .render("```mermaid\nflowchart LR\nA-->B\n```\n", "")
            .unwrap();
        assert!(res.body.contains("flowchart LR"));
        // Code fence class should be language-mermaid for our client-side init.
        assert!(
            res.body.contains("language-mermaid"),
            "no language-mermaid class: {}",
            res.body
        );
    }
}
