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
            // bat's extended set (via two-face) — a superset of syntect's
            // bundled Sublime defaults; adds TypeScript/TSX, TOML,
            // Dockerfile, INI, … that fences commonly name.
            syntaxes: two_face::syntax::extra_newlines(),
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
            // Fence-name aliases for grammars absent from the set. The
            // fancy-regex build excludes "JavaScript (Babel)" (bat's JSX
            // grammar); TSX is a superset of JSX, so highlight with that.
            if let Some(alias) = match l {
                "jsx" => Some("tsx"),
                _ => None,
            } && let Some(s) = self.syntaxes.find_syntax_by_token(alias)
            {
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

/// Returns the combined CSS. Light and dark rules are mutually exclusive:
/// each theme's rules apply only under its explicit `[data-theme="…"]`
/// attribute or its `prefers-color-scheme` media block (when the attribute
/// is `auto`). Never rely on scoped rules *overriding* the other theme —
/// syntect themes emit selectors of wildly varying specificity, so a plain
/// `[data-theme="dark"] .syn-string` loses to a deep light-theme selector
/// chain and the light colors bleed into dark mode (unreadable on a dark
/// background).
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

    let mut out = String::with_capacity((light_css.len() + dark_css.len()) * 2 + 512);
    for (label, css, attr_scope, media, auto_scope) in [
        (
            "light",
            &light_css,
            "[data-theme=\"light\"] ",
            "light",
            ":root:not([data-theme=\"dark\"]) ",
        ),
        (
            "dark",
            &dark_css,
            "[data-theme=\"dark\"] ",
            "dark",
            ":root:not([data-theme=\"light\"]) ",
        ),
    ] {
        out.push_str(&format!("/* Syntect — {label} explicit */\n"));
        out.push_str(&prefix_selectors(css, attr_scope));
        out.push_str(&format!("\n/* Syntect — {label} via system preference */\n"));
        out.push_str(&format!("@media (prefers-color-scheme: {media}) {{\n"));
        out.push_str(&prefix_selectors(css, auto_scope));
        out.push_str("\n}\n");
    }
    out
}

/// Prefix every rule selector in a syntect-generated stylesheet with `scope`.
/// syntect emits each rule's full selector list on one line (comma-separated,
/// `{` at the end, bare class selectors only); every selector in the list gets
/// the scope, not just the first.
fn prefix_selectors(css: &str, scope: &str) -> String {
    let mut out = String::with_capacity(css.len() + scope.len() * 64);
    for line in css.lines() {
        let trim = line.trim_start();
        if let Some(brace) = trim.find('{')
            && trim.starts_with('.')
        {
            // preserve any leading whitespace, then scope each selector
            out.push_str(&line[..line.len() - trim.len()]);
            let scoped: Vec<String> = trim[..brace]
                .split(',')
                .map(|sel| format!("{scope}{}", sel.trim()))
                .collect();
            out.push_str(&scoped.join(", "));
            out.push(' ');
            out.push_str(&trim[brace..]);
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
    fn css_rules_are_all_theme_scoped() {
        // The light theme must never apply in dark contexts (and vice versa).
        // Scoped-by-prefix overriding is NOT enough: syntect themes contain
        // selectors of wildly varying specificity (InspiredGitHub has
        // 16-deep `.syn-meta` chains for JSON) that beat any scoped override.
        // The only robust scheme is mutual exclusion: every rule carries a
        // theme scope, so at most one theme's rules exist per context.
        let css = highlight_css();
        for (i, line) in css.lines().enumerate() {
            let trim = line.trim_start();
            assert!(
                !(trim.starts_with('.') && trim.contains('{')),
                "unscoped rule leaks across theme modes at line {}: {line}",
                i + 1
            );
        }
        // Light rules exist under both an explicit attribute scope and a
        // system-preference media block, mirroring the dark side.
        assert!(
            css.contains("[data-theme=\"light\"] .syn-"),
            "missing explicit light scope"
        );
        assert!(
            css.contains("@media (prefers-color-scheme: light)"),
            "missing light media block"
        );
    }

    #[test]
    fn prefix_selectors_prefixes_every_comma_separated_selector() {
        // syntect emits selector lists on one line: `.a, .b {`. Every
        // selector in the list must get the scope, otherwise the trailing
        // ones apply in ALL theme modes (dark colors leaking into light).
        let css = ".syn-comment, .syn-punctuation.syn-definition.syn-comment {\n color: #65737e;\n}\n";
        let scoped = prefix_selectors(css, "[data-theme=\"dark\"] ");
        assert!(
            scoped.contains("[data-theme=\"dark\"] .syn-comment"),
            "first selector unscoped: {scoped}"
        );
        assert!(
            scoped.contains("[data-theme=\"dark\"] .syn-punctuation.syn-definition.syn-comment"),
            "second selector unscoped: {scoped}"
        );
    }

    #[test]
    fn prefix_selectors_skips_non_rules() {
        let css = ".kw{color:red}\n/* comment */\nbody { ignored } /* not a chroma rule */\n";
        let scoped = prefix_selectors(css, "X ");
        assert!(scoped.contains("X .kw {color:red}"), "got: {scoped}");
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
    fn resolves_languages_beyond_sublime_defaults() {
        // Sublime's default packages (syntect's bundled set) lack several
        // everyday fence languages; the extended set must cover them so
        // ```typescript etc. don't silently fall back to plain text.
        let bundle = SyntectBundle::new();
        let plain = bundle.syntaxes.find_syntax_plain_text().name.clone();
        for lang in ["typescript", "ts", "tsx", "jsx", "toml", "dockerfile", "ini"] {
            let resolved = bundle.resolve(Some(lang)).name.clone();
            assert_ne!(
                resolved, plain,
                "`{lang}` fence falls back to plain text — grammar missing"
            );
        }
        // The defaults must still resolve after swapping syntax sets.
        for lang in ["json", "js", "rust", "python", "go", "yaml", "bash", "c"] {
            let resolved = bundle.resolve(Some(lang)).name.clone();
            assert_ne!(resolved, plain, "`{lang}` regressed to plain text");
        }
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
