//! PageData + minijinja rendering. Mirror of Go `internal/assets/page.go`.

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD as BASE64;
use minijinja::{AutoEscape, Environment, Value};
use serde::Serialize;

use crate::mt::render::highlight_css;

/// Where MathJax / Mermaid live inside the static assets root.
pub const MERMAID_PATH: &str = "mermaid.min.js";
pub const MATHJAX_PATH: &str = "mathjax/tex-mml-chtml.js";

/// MathJax client-side config — must come *before* the MathJax library tag.
pub const MATHJAX_CONFIG_SCRIPT: &str = "<script>window.MathJax={tex:{inlineMath:[['$','$'],['\\\\(','\\\\)']],displayMath:[['$$','$$'],['\\\\[','\\\\]']]},options:{skipHtmlTags:['script','noscript','style','textarea','pre','code']}};</script>";

/// Bag of fields bound to the page template. Snake_case mirrors of Go's PageData.
#[derive(Default, Debug, Serialize)]
pub struct PageData {
    pub lang: String,
    pub title: String,
    pub description: String,
    pub theme: String,
    /// Rendered HTML body of the article (already trusted).
    pub body: String,
    /// Rendered `<ul>` tree for the right-side TOC.
    pub toc: String,
    /// Relative or absolute prefix for external assets (e.g. `"assets/"`, `"/assets/"`).
    pub assets_base: String,
    /// Extra `<script>` / `<style>` snippets injected into `<head>`.
    pub extra_head_tags: Vec<String>,

    // Multi-file mode (zero values disable the corresponding UI):
    pub site_name: String,
    pub site_tree: String,
    pub prev_href: String,
    pub prev_label: String,
    pub next_href: String,
    pub next_label: String,

    pub has_math: bool,
    pub has_mermaid: bool,

    pub mermaid_src: String,
    pub mathjax_src: String,

    // Inline modes — non-empty replaces the external `<link>`/`<script>` with an inlined block.
    pub inline_style: String,
    pub inline_app: String,
    pub inline_mermaid: String,
    pub inline_mathjax: String,

    /// Raw JS injected for the dev server's live-reload client.
    pub live_reload_js: String,
}

impl PageData {
    /// Fill in sensible defaults so callers can leave fields empty.
    pub fn with_defaults(mut self) -> Self {
        if self.lang.is_empty() {
            self.lang = "en".into();
        }
        if self.theme.is_empty() {
            self.theme = "auto".into();
        }
        self
    }
}

/// Renders the template with the supplied page data.
pub fn render(data: PageData) -> Result<String, RenderError> {
    let data = data.with_defaults();
    let mut env = Environment::new();
    env.set_auto_escape_callback(|_| AutoEscape::Html);
    env.add_template("page", super::template())?;
    let tpl = env.get_template("page")?;
    Ok(tpl.render(Value::from_serialize(&data))?)
}

/// Knobs callers pass into [`build_page`].
#[derive(Debug, Default, Clone)]
pub struct PageOptions {
    /// User-selected default theme override (`auto` | `light` | `dark`).
    pub theme: String,
    /// Path prefix for external resources (e.g. `"assets/"`, `"/assets/"`).
    /// Ignored when [`inline`] is true.
    pub assets_base: String,
    /// When true, embed CSS/JS/fonts inline so the resulting HTML is self-contained.
    pub inline: bool,
    /// Raw JS injected for the dev server's live-reload client.
    pub live_reload_js: String,
}

/// High-level helper that mirrors the Go `assets.BuildPage`. Caller passes the
/// already-rendered body/TOC HTML plus the feature flags; this fills in the
/// asset URLs / inline blobs / extra head tags consistently.
pub fn build_page(
    title: &str,
    description: &str,
    body: String,
    toc_html: String,
    has_math: bool,
    has_mermaid: bool,
    opts: PageOptions,
) -> Result<PageData, RenderError> {
    let mut data = PageData {
        title: title.to_string(),
        description: description.to_string(),
        theme: opts.theme.clone(),
        body,
        toc: toc_html,
        assets_base: opts.assets_base.clone(),
        has_math,
        has_mermaid,
        live_reload_js: opts.live_reload_js.clone(),
        ..Default::default()
    };

    // Syntax-highlight stylesheet is always injected — small enough and avoids
    // a second HTTP request in serve mode.
    data.extra_head_tags
        .push(format!("<style>{}</style>", highlight_css()));

    if has_math {
        data.extra_head_tags.push(MATHJAX_CONFIG_SCRIPT.to_string());
    }

    if opts.inline {
        let css =
            String::from_utf8_lossy(super::read_static("style.css").unwrap_or(&[])).into_owned();
        let fonts = inline_fonts_css();
        data.inline_style = format!("{css}\n{fonts}");
        data.inline_app =
            String::from_utf8_lossy(super::read_static("app.js").unwrap_or(&[])).into_owned();
        if has_mermaid && let Some(b) = super::read_static(MERMAID_PATH) {
            data.inline_mermaid = String::from_utf8_lossy(b).into_owned();
        }
        if has_math && let Some(b) = super::read_static(MATHJAX_PATH) {
            data.inline_mathjax = String::from_utf8_lossy(b).into_owned();
        }
    } else {
        if has_mermaid {
            data.mermaid_src = format!("{}{}", opts.assets_base, MERMAID_PATH);
        }
        if has_math {
            data.mathjax_src = format!("{}{}", opts.assets_base, MATHJAX_PATH);
        }
    }
    Ok(data)
}

/// Walks `static/fonts/*.woff2`, encodes each as data: URI, emits `@font-face` rules.
/// Returns an empty string when no fonts are vendored.
fn inline_fonts_css() -> String {
    let Some(fonts_dir) = super::STATIC_DIR.get_dir("fonts") else {
        return String::new();
    };
    let mut out = String::new();
    for file in fonts_dir.files() {
        let name = file
            .path()
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("");
        if !name.ends_with(".woff2") {
            continue;
        }
        let (family, weight, italic) = parse_font_name(name);
        let b64 = BASE64.encode(file.contents());
        let style = if italic { "italic" } else { "normal" };
        out.push_str(&format!(
            "@font-face{{font-family:'{family}';font-style:{style};font-weight:{weight};font-display:swap;src:url(data:font/woff2;base64,{b64}) format('woff2');}}\n"
        ));
    }
    out
}

fn parse_font_name(name: &str) -> (String, String, bool) {
    let stem = name.trim_end_matches(".woff2");
    let parts: Vec<&str> = stem.split('-').collect();
    if parts.is_empty() {
        return (stem.to_string(), "400".to_string(), false);
    }
    let family = parts[0].to_string();
    let mut weight = "400".to_string();
    let mut italic = false;
    for p in &parts[1..] {
        let lower = p.to_ascii_lowercase();
        if lower == "italic" {
            italic = true;
        } else if !p.is_empty() && p.chars().all(|c| c.is_ascii_digit()) {
            weight = p.to_string();
        }
    }
    (family, weight, italic)
}

#[derive(Debug)]
pub enum RenderError {
    Template(minijinja::Error),
}

impl std::fmt::Display for RenderError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RenderError::Template(e) => write!(f, "template error: {e}"),
        }
    }
}

impl std::error::Error for RenderError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            RenderError::Template(e) => Some(e),
        }
    }
}

impl From<minijinja::Error> for RenderError {
    fn from(e: minijinja::Error) -> Self {
        RenderError::Template(e)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn minimal_page_renders_title_and_body() {
        let html = render(PageData {
            title: "Hello".into(),
            body: "<p>world</p>".into(),
            assets_base: "assets/".into(),
            ..Default::default()
        })
        .unwrap();
        assert!(html.contains("<!doctype html>"));
        assert!(html.contains("<title>Hello</title>"));
        assert!(html.contains("<p>world</p>"));
        // Diagnostic for asset href + script src — be tolerant of html-escape variants
        // emitted by minijinja's autoescape.
        assert!(
            html.contains("style.css") && html.contains("assets"),
            "missing stylesheet link in:\n{html}"
        );
        assert!(
            html.contains("app.js") && html.contains("assets"),
            "missing app.js script in:\n{html}"
        );
    }

    #[test]
    fn inline_style_replaces_link_tag() {
        let html = render(PageData {
            title: "T".into(),
            inline_style: "body{color:red}".into(),
            inline_app: "/* app */".into(),
            ..Default::default()
        })
        .unwrap();
        assert!(html.contains("<style>body{color:red}</style>"));
        assert!(!html.contains(r#"<link rel="stylesheet""#));
    }

    #[test]
    fn site_tree_and_pager_render() {
        let html = render(PageData {
            title: "Doc".into(),
            site_name: "Manual".into(),
            site_tree: "<ul><li>x</li></ul>".into(),
            prev_href: "intro.html".into(),
            prev_label: "Intro".into(),
            next_href: "config.html".into(),
            next_label: "Config".into(),
            ..Default::default()
        })
        .unwrap();
        assert!(html.contains("Manual"));
        assert!(html.contains("<ul><li>x</li></ul>"));
        assert!(html.contains(r#"href="intro.html""#));
        assert!(html.contains(r#"href="config.html""#));
    }

    #[test]
    fn mermaid_and_math_blocks_only_when_flagged() {
        let bare = render(PageData {
            title: "T".into(),
            ..Default::default()
        })
        .unwrap();
        assert!(!bare.contains("mtMmdModal"));
        assert!(!bare.contains("MathJax"));

        let with = render(PageData {
            title: "T".into(),
            has_mermaid: true,
            has_math: true,
            mermaid_src: "mermaid.min.js".into(),
            mathjax_src: "mathjax.js".into(),
            extra_head_tags: vec!["<script>MathJaxConfig</script>".into()],
            ..Default::default()
        })
        .unwrap();
        assert!(with.contains("mtMmdModal"));
        assert!(with.contains("MathJaxConfig"));
        assert!(with.contains(r#"src="mathjax.js""#));
    }

    #[test]
    fn defaults_fill_lang_and_theme() {
        let html = render(PageData {
            title: "X".into(),
            ..Default::default()
        })
        .unwrap();
        assert!(html.contains(r#"lang="en""#));
        assert!(html.contains(r#"data-theme="auto""#));
    }

    #[test]
    fn build_page_external_assets_emit_src_attributes() {
        let data = build_page(
            "Doc",
            "",
            "<p>x</p>".into(),
            String::new(),
            true, // math
            true, // mermaid
            PageOptions {
                theme: "auto".into(),
                assets_base: "assets/".into(),
                inline: false,
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(data.mermaid_src, "assets/mermaid.min.js");
        assert_eq!(data.mathjax_src, "assets/mathjax/tex-mml-chtml.js");
        // MathJax config tag and highlight CSS both go into <head>.
        assert!(
            data.extra_head_tags
                .iter()
                .any(|t| t.contains("window.MathJax"))
        );
        assert!(
            data.extra_head_tags
                .iter()
                .any(|t| t.contains("<style>") && t.contains("Syntect"))
        );
    }

    #[test]
    fn build_page_inline_mode_pulls_in_assets() {
        let data = build_page(
            "Doc",
            "",
            "<p>x</p>".into(),
            String::new(),
            true,
            true,
            PageOptions {
                theme: "auto".into(),
                inline: true,
                ..Default::default()
            },
        )
        .unwrap();
        // inline_style holds CSS + font @font-face rules (which may be empty if no fonts).
        assert!(
            !data.inline_style.is_empty(),
            "inline css should not be empty"
        );
        assert!(
            !data.inline_app.is_empty(),
            "inline app.js should not be empty"
        );
        // mermaid + mathjax bundles should be inlined (non-empty).
        assert!(!data.inline_mermaid.is_empty(), "mermaid not inlined");
        assert!(!data.inline_mathjax.is_empty(), "mathjax not inlined");
    }

    #[test]
    fn build_page_no_extras_when_features_disabled() {
        let data = build_page(
            "Doc",
            "",
            "<p>x</p>".into(),
            String::new(),
            false,
            false,
            PageOptions {
                assets_base: "assets/".into(),
                ..Default::default()
            },
        )
        .unwrap();
        assert!(data.mermaid_src.is_empty());
        assert!(data.mathjax_src.is_empty());
        // The chroma stylesheet still goes in (syntax highlighting always-on), but no MathJax config.
        assert!(
            !data
                .extra_head_tags
                .iter()
                .any(|t| t.contains("window.MathJax"))
        );
    }

    #[test]
    fn pager_hrefs_get_html_escaped() {
        // Filenames are user-controlled in `mt-rs DIR/` mode, so the template
        // must autoescape `<`, `"`, `&` etc. before writing them into the
        // `href` attribute. (We dropped `| safe` on prev_href/next_href.)
        let html = render(PageData {
            title: "T".into(),
            prev_href: "evil<\"&.html".into(),
            prev_label: "Prev".into(),
            next_href: "ok.html".into(),
            next_label: "Next".into(),
            ..Default::default()
        })
        .unwrap();
        // Should NOT contain the raw special chars literally inside the href.
        assert!(
            !html.contains(r#"href="evil<""#),
            "raw quote leaked into href: {html}"
        );
        // Should contain the escaped forms.
        assert!(
            html.contains("&lt;") && html.contains("&amp;"),
            "expected escaping: {html}"
        );
    }

    #[test]
    fn parse_font_name_handles_variants() {
        assert_eq!(
            parse_font_name("Roboto-400.woff2"),
            ("Roboto".into(), "400".into(), false)
        );
        assert_eq!(
            parse_font_name("Roboto-700-italic.woff2"),
            ("Roboto".into(), "700".into(), true)
        );
        assert_eq!(
            parse_font_name("RobotoMono-400.woff2"),
            ("RobotoMono".into(), "400".into(), false)
        );
        // No weight → default 400, no italic
        assert_eq!(
            parse_font_name("Inter.woff2"),
            ("Inter".into(), "400".into(), false)
        );
    }
}
