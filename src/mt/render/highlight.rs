//! Syntax highlighting — syntect adapter for comrak + dual-theme CSS generator.
//!
//! Mirrors the Go `internal/render/highlight.go`. We emit class-based HTML
//! (`<span class="syn-keyword">…`) and ship the matching stylesheet inlined
//! into the page so light/dark themes swap purely via CSS — no JS needed.

use std::collections::HashMap;
use std::io::Write;
use std::sync::OnceLock;

use comrak::adapters::SyntaxHighlighterAdapter;
use syntect::highlighting::ThemeSet;
use syntect::html::{ClassStyle, ClassedHTMLGenerator, css_for_theme_with_class_style};
use syntect::parsing::{SyntaxReference, SyntaxSet};
use syntect::util::LinesWithEndings;

const CLASS_PREFIX: &str = "syn-";

/// Class style used by both the HTML emitter and the CSS generator.
fn class_style() -> ClassStyle {
    ClassStyle::SpacedPrefixed {
        prefix: CLASS_PREFIX,
    }
}

/// Cached parser tables + bundled themes. Loading the defaults is cheap once.
pub struct SyntectBundle {
    pub syntaxes: SyntaxSet,
    pub themes: ThemeSet,
}

impl SyntectBundle {
    pub fn new() -> Self {
        Self {
            syntaxes: SyntaxSet::load_defaults_newlines(),
            themes: ThemeSet::load_defaults(),
        }
    }

    fn resolve<'a>(&'a self, lang: Option<&str>) -> &'a SyntaxReference {
        if let Some(l) = lang {
            if let Some(s) = self.syntaxes.find_syntax_by_token(l) {
                return s;
            }
            if let Some(s) = self.syntaxes.find_syntax_by_extension(l) {
                return s;
            }
        }
        self.syntaxes.find_syntax_plain_text()
    }
}

impl Default for SyntectBundle {
    fn default() -> Self {
        Self::new()
    }
}

/// Adapter glue for comrak.
pub struct SyntectAdapter<'a> {
    bundle: &'a SyntectBundle,
}

impl<'a> SyntectAdapter<'a> {
    pub fn new(bundle: &'a SyntectBundle) -> Self {
        Self { bundle }
    }
}

impl<'a> SyntaxHighlighterAdapter for SyntectAdapter<'a> {
    fn write_highlighted(
        &self,
        output: &mut dyn Write,
        lang: Option<&str>,
        code: &str,
    ) -> std::io::Result<()> {
        // Skip our own special blocks (mermaid handled separately downstream).
        if lang == Some("mermaid") {
            return output.write_all(code.as_bytes());
        }
        let syntax = self.bundle.resolve(lang);
        let mut hgen = ClassedHTMLGenerator::new_with_class_style(
            syntax,
            &self.bundle.syntaxes,
            class_style(),
        );
        for line in LinesWithEndings::from(code) {
            if let Err(e) = hgen.parse_html_for_line_which_includes_newline(line) {
                return Err(std::io::Error::other(format!("syntect: {e}")));
            }
        }
        output.write_all(hgen.finalize().as_bytes())
    }

    fn write_pre_tag(
        &self,
        output: &mut dyn Write,
        _attributes: HashMap<String, String>,
    ) -> std::io::Result<()> {
        output.write_all(b"<pre>")
    }

    fn write_code_tag(
        &self,
        output: &mut dyn Write,
        attributes: HashMap<String, String>,
    ) -> std::io::Result<()> {
        // comrak passes one of:
        //   "class"=language-X    (github_pre_lang=false)
        //   "lang"=X              (github_pre_lang=true on the *pre* tag, none here)
        // We normalise to <code class="language-X"> so client-side scanners
        // (e.g. our app.js mermaid bootstrap) can detect fence languages.
        let lang_class = attributes
            .get("class")
            .cloned()
            .or_else(|| attributes.get("lang").map(|l| format!("language-{l}")));
        if let Some(c) = lang_class {
            write!(output, r#"<code class="{c}">"#)
        } else {
            output.write_all(b"<code>")
        }
    }
}

// ---------- Two-theme stylesheet ----------

const LIGHT_THEME: &str = "InspiredGitHub";
const DARK_THEME: &str = "base16-ocean.dark";

/// Returns the combined CSS: light rules as default, dark scoped by
/// `[data-theme="dark"]` and `@media (prefers-color-scheme: dark)`.
pub fn highlight_css() -> &'static str {
    static CSS: OnceLock<String> = OnceLock::new();
    CSS.get_or_init(build_highlight_css)
}

fn build_highlight_css() -> String {
    let bundle = SyntectBundle::new();
    let light = bundle
        .themes
        .themes
        .get(LIGHT_THEME)
        .or_else(|| bundle.themes.themes.values().next())
        .expect("at least one theme bundled");
    let dark = bundle
        .themes
        .themes
        .get(DARK_THEME)
        .or_else(|| bundle.themes.themes.values().next())
        .expect("at least one theme bundled");

    let light_css =
        css_for_theme_with_class_style(light, class_style()).unwrap_or_else(|_| String::new());
    let dark_css =
        css_for_theme_with_class_style(dark, class_style()).unwrap_or_else(|_| String::new());

    let mut out = String::with_capacity(light_css.len() + dark_css.len() * 2 + 256);
    out.push_str("/* Syntect — light (default) */\n");
    out.push_str(&light_css);
    out.push_str("\n/* Syntect — dark explicit */\n");
    out.push_str(&prefix_selectors(&dark_css, "[data-theme=\"dark\"] "));
    out.push_str("\n/* Syntect — dark via system preference */\n");
    out.push_str("@media (prefers-color-scheme: dark) {\n");
    out.push_str(&prefix_selectors(
        &dark_css,
        ":root:not([data-theme=\"light\"]) ",
    ));
    out.push_str("\n}\n");
    out
}

/// Prefix every rule selector in a syntect-generated stylesheet with `scope`.
/// syntect emits rules with bare class selectors at the start of lines; we
/// inject the scope before each leading `.`.
fn prefix_selectors(css: &str, scope: &str) -> String {
    let mut out = String::with_capacity(css.len() + scope.len() * 64);
    for line in css.lines() {
        let trim = line.trim_start();
        if trim.starts_with('.') && line.contains('{') {
            // preserve any leading whitespace, then inject the scope
            let ws = &line[..line.len() - trim.len()];
            out.push_str(ws);
            out.push_str(scope);
            out.push_str(trim);
        } else {
            out.push_str(line);
        }
        out.push('\n');
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn css_contains_both_modes() {
        let css = highlight_css();
        assert!(css.contains("Syntect — light"));
        assert!(css.contains("[data-theme=\"dark\"]"), "missing dark scope");
        assert!(css.contains("prefers-color-scheme: dark"));
    }

    #[test]
    fn prefix_selectors_skips_non_rules() {
        let css = ".kw{color:red}\n/* comment */\nbody { ignored } /* not a chroma rule */\n";
        let scoped = prefix_selectors(css, "X ");
        assert!(scoped.contains("X .kw{color:red}"));
        // body { ... } also opens with non-dot — should pass through untouched
        assert!(scoped.contains("body { ignored }"));
    }

    #[test]
    fn adapter_highlights_known_language() {
        let bundle = SyntectBundle::new();
        let adapter = SyntectAdapter::new(&bundle);
        let mut buf: Vec<u8> = Vec::new();
        adapter
            .write_highlighted(&mut buf, Some("rust"), "fn main() {}\n")
            .unwrap();
        let html = String::from_utf8(buf).unwrap();
        assert!(html.contains(CLASS_PREFIX), "no class prefix: {html}");
    }

    #[test]
    fn adapter_falls_back_to_plain_text() {
        let bundle = SyntectBundle::new();
        let adapter = SyntectAdapter::new(&bundle);
        let mut buf: Vec<u8> = Vec::new();
        adapter
            .write_highlighted(&mut buf, Some("notalanguage"), "hello\n")
            .unwrap();
        // Plain-text path still produces some output (even if just escaped text).
        assert!(!buf.is_empty());
    }

    #[test]
    fn mermaid_blocks_pass_through_unchanged() {
        let bundle = SyntectBundle::new();
        let adapter = SyntectAdapter::new(&bundle);
        let mut buf: Vec<u8> = Vec::new();
        adapter
            .write_highlighted(&mut buf, Some("mermaid"), "flowchart LR\nA-->B")
            .unwrap();
        assert_eq!(String::from_utf8(buf).unwrap(), "flowchart LR\nA-->B");
    }
}
