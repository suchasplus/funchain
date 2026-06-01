//! Markdown source preprocessors. Port of Go `internal/render/preprocess.go`.
//!
//! These run before the markdown parser sees the source. Each transform is a
//! pure str→String pipeline so they compose cleanly.

use regex::{Captures, Regex};
use std::sync::OnceLock;

// ---------- Admonition ----------
//
// Syntax (MkDocs-style):
//
//     !!! kind "Optional Title"
//         paragraph 1
//         paragraph 1 continued
//
//         paragraph 2 (blank line allowed within the block)
//
// Block ends at the first non-indented, non-blank line.

fn admonition_open_re() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| {
        Regex::new(r#"^!!!\s+([a-zA-Z][\w-]*)(?:\s+"([^"]*)")?\s*$"#).expect("admonition regex")
    })
}

fn title_case(s: &str) -> String {
    let mut chars = s.chars();
    match chars.next() {
        Some(c) => c.to_uppercase().collect::<String>() + chars.as_str(),
        None => String::new(),
    }
}

fn html_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            _ => out.push(c),
        }
    }
    out
}

/// Rewrites MkDocs-style admonitions into raw HTML `<div class="admonition admonition-KIND">`.
///
/// Fence-aware: top-level fenced code blocks (```` ``` ```` or `~~~`) are
/// passed through verbatim, so `!!! note` *inside* a code block is no longer
/// hijacked.
pub fn preprocess_admonition(src: &str) -> String {
    let mut out = String::with_capacity(src.len());
    let mut in_block = false;
    let mut pending_blank = false;
    // Top-level fence: (char, marker length). While Some, every line goes
    // through unchanged and admonition detection is suppressed.
    let mut in_fence: Option<(char, usize)> = None;

    let flush_close = |out: &mut String, in_block: &mut bool, pending_blank: &mut bool| {
        if *in_block {
            out.push_str("</div>\n");
            *in_block = false;
            *pending_blank = false;
        }
    };
    let open_block = |out: &mut String, kind: &str, title: &str| {
        let display = if title.is_empty() {
            title_case(kind)
        } else {
            title.to_string()
        };
        out.push_str(&format!(
            "<div class=\"admonition admonition-{}\" markdown=\"1\">\n",
            html_escape(kind)
        ));
        out.push_str(&format!(
            "<p class=\"admonition-title\">{}</p>\n\n",
            html_escape(&display)
        ));
    };

    let re = admonition_open_re();

    for line in src.lines() {
        // Inside a top-level fenced code block: pass through and only watch for
        // the matching close. Admonitions never start here.
        if let Some((fc, fn_)) = in_fence {
            if is_closing_fence(line, fc, fn_) {
                in_fence = None;
            }
            out.push_str(line);
            out.push('\n');
            continue;
        }
        if in_block {
            if line.trim().is_empty() {
                pending_blank = true;
                continue;
            }
            if let Some(stripped) = line
                .strip_prefix("    ")
                .or_else(|| line.strip_prefix('\t'))
            {
                if pending_blank {
                    out.push('\n');
                    pending_blank = false;
                }
                out.push_str(stripped);
                out.push('\n');
                continue;
            }
            // Block ends here. Fall through to re-evaluate this line.
            flush_close(&mut out, &mut in_block, &mut pending_blank);
        }
        // Outside any admonition.
        if let Some(caps) = re.captures(line) {
            let kind = caps
                .get(1)
                .map(|m| m.as_str())
                .unwrap_or("note")
                .to_lowercase();
            let title = caps.get(2).map(|m| m.as_str()).unwrap_or("");
            open_block(&mut out, &kind, title);
            in_block = true;
            pending_blank = false;
            continue;
        }
        // Open a fence if this line is a top-level fence start.
        if let Some(marker) = fence_open_marker(line) {
            in_fence = Some(marker);
        }
        out.push_str(line);
        out.push('\n');
    }
    flush_close(&mut out, &mut in_block, &mut pending_blank);
    out
}

/// If `line` is a CommonMark-style fence opener (up to 3 leading spaces, then
/// 3+ backticks or tildes), returns `(fence_char, marker_length)`. Otherwise
/// `None`.
pub(crate) fn fence_open_marker(line: &str) -> Option<(char, usize)> {
    let leading = line.chars().take_while(|c| *c == ' ').count();
    if leading > 3 {
        return None;
    }
    let rest = &line[leading..];
    let first = rest.chars().next()?;
    if first != '`' && first != '~' {
        return None;
    }
    let n = rest.chars().take_while(|c| *c == first).count();
    if n < 3 {
        return None;
    }
    Some((first, n))
}

/// True if `line` closes a fence opened with `(open_char, open_len)`. The
/// closing marker must use the same character, be at least as long, and have
/// no info string trailing the marker.
pub(crate) fn is_closing_fence(line: &str, open_char: char, open_len: usize) -> bool {
    let leading = line.chars().take_while(|c| *c == ' ').count();
    if leading > 3 {
        return false;
    }
    let rest = &line[leading..];
    let n = rest.chars().take_while(|c| *c == open_char).count();
    if n < open_len {
        return false;
    }
    let after_marker: String = rest.chars().skip(n).collect();
    after_marker.trim().is_empty()
}

// ---------- Wikilinks ----------

fn wikilink_re() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| Regex::new(r"\[\[([^\[\]|]+)(?:\|([^\[\]]+))?\]\]").expect("wikilink regex"))
}

/// Resolver maps a wikilink target name to an href. Returning `None` falls back
/// to the default `<slug>.html` same-directory behavior.
pub type WikilinkResolver<'a> = dyn Fn(&str) -> Option<String> + 'a;

/// Rewrites `[[Page]]` / `[[Page|Alias]]` into `<a class="mt-wikilink" href=...>` anchors.
///
/// Note: source-level pass. The render pipeline now uses an AST-level pass
/// ([`split_wikilink_parts`]) so code blocks and link/image children are
/// skipped automatically. This function is kept for callers that want the
/// regex behavior on raw markdown — it does NOT respect fence boundaries.
pub fn preprocess_wikilinks(src: &str, resolver: Option<&WikilinkResolver<'_>>) -> String {
    wikilink_re()
        .replace_all(src, |caps: &Captures<'_>| {
            let page = caps.get(1).map(|m| m.as_str().trim()).unwrap_or("");
            let alias = caps
                .get(2)
                .map(|m| m.as_str().trim().to_string())
                .unwrap_or_else(|| page.to_string());
            let href = resolver
                .and_then(|r| r(page))
                .unwrap_or_else(|| format!("{}.html", slugify_path(page)));
            format!(
                "<a class=\"mt-wikilink\" href=\"{}\">{}</a>",
                html_escape(&href),
                html_escape(&alias)
            )
        })
        .into_owned()
}

/// One piece of text after splitting on `[[…]]` matches.
///
/// `Text` carries an untouched substring (verbatim source between matches).
/// `Link` carries the three pieces of a rewritten `[[Page|Alias]]`: the
/// opening anchor tag, the visible alias text (unescaped — the caller embeds
/// it as a `Text` AST node so the renderer html-escapes it), and the closing
/// tag. Splitting the link this way is what lets downstream AST passes (e.g.
/// heading text collection) walk into the alias.
#[derive(Debug, Clone)]
pub enum WikiPart {
    Text(String),
    Link {
        open: String,
        text: String,
        close: String,
    },
}

/// Walks `text` and produces a sequence of `WikiPart`s. When no `[[…]]`
/// matches occur, the result is a single `Text` part. This is the building
/// block the AST-level wikilink pass uses to rewrite individual Text nodes.
pub fn split_wikilink_parts(text: &str, resolver: Option<&WikilinkResolver<'_>>) -> Vec<WikiPart> {
    let re = wikilink_re();
    let mut out: Vec<WikiPart> = Vec::new();
    let mut last_end = 0;
    for caps in re.captures_iter(text) {
        let m = caps.get(0).unwrap();
        if m.start() > last_end {
            out.push(WikiPart::Text(text[last_end..m.start()].to_string()));
        }
        let page = caps.get(1).map(|x| x.as_str().trim()).unwrap_or("");
        let alias = caps
            .get(2)
            .map(|x| x.as_str().trim().to_string())
            .unwrap_or_else(|| page.to_string());
        let href = resolver
            .and_then(|r| r(page))
            .unwrap_or_else(|| format!("{}.html", slugify_path(page)));
        out.push(WikiPart::Link {
            open: format!("<a class=\"mt-wikilink\" href=\"{}\">", html_escape(&href)),
            text: alias,
            close: "</a>".to_string(),
        });
        last_end = m.end();
    }
    if last_end < text.len() {
        out.push(WikiPart::Text(text[last_end..].to_string()));
    }
    if out.is_empty() {
        out.push(WikiPart::Text(text.to_string()));
    }
    out
}

fn slugify_path(s: &str) -> String {
    s.trim().replace(' ', "-")
}

// ---------- Mermaid escapes ----------

fn mermaid_block_re() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| {
        Regex::new(r"(?s)(```mermaid\b[^\n]*\n)(.*?)(\n[ \t]*```)").expect("mermaid block regex")
    })
}

fn escape_seq_re() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| Regex::new(r"\\.").expect("escape regex"))
}

/// Replaces literal escape sequences inside ```mermaid blocks so they render
/// correctly with `htmlLabels: true`.
///
///   \n, \r  → <br/>      (line break)
///   \t      → 4× &nbsp;  (tab)
///   \\      → \           (literal backslash)
///   \" / \' → " / '       (literal quote)
///   \X (other) → passthrough
pub fn preprocess_mermaid(src: &str) -> String {
    mermaid_block_re()
        .replace_all(src, |caps: &Captures<'_>| {
            let head = &caps[1];
            let body = &caps[2];
            let tail = &caps[3];
            let rewritten = apply_mermaid_escapes(body);
            format!("{head}{rewritten}{tail}")
        })
        .into_owned()
}

pub(crate) fn apply_mermaid_escapes(s: &str) -> String {
    escape_seq_re()
        .replace_all(s, |caps: &Captures<'_>| {
            let m = caps.get(0).map(|m| m.as_str()).unwrap_or("");
            if m.len() < 2 {
                return m.to_string();
            }
            // bytes[1] is the char after backslash (ascii guaranteed for the cases we map)
            match m.as_bytes()[1] {
                b'n' | b'r' => "<br/>".to_string(),
                b't' => "&nbsp;&nbsp;&nbsp;&nbsp;".to_string(),
                b'\\' => "\\".to_string(),
                b'"' => "\"".to_string(),
                b'\'' => "'".to_string(),
                _ => m.to_string(),
            }
        })
        .into_owned()
}

// (Heading attribute parsing now lives in `crate::mt::render::heading` as an
// AST pass.  The old source-level `extract_heading_ids` / `HeadingHint` API
// is gone — there were no external callers.)

// ---------- CJK softbreak ----------
//
// Goldmark's `extension.CJK` joins lines where the prev line ends with a CJK
// codepoint and the next starts with one — markdown's soft break would
// otherwise render as a space in HTML, which is wrong for CJK runs. We mimic
// the behavior by deleting the in-between newline pre-parse.

/// Inside paragraphs, joins consecutive lines whose break is flanked by CJK
/// characters. Skips fenced code blocks so source-preserving blocks aren't
/// touched.
pub fn preprocess_cjk_softbreak(src: &str) -> String {
    let mut out = String::with_capacity(src.len());
    let mut in_fence = false;
    let mut last_emitted_break: Option<usize> = None; // byte index in `out` where last \n landed
    let mut last_cjk_at_eol = false;
    for raw_line in src.split_inclusive('\n') {
        let trimmed = raw_line.trim_end_matches(['\n', '\r']);
        let opens_or_closes_fence = trimmed.trim_start().starts_with("```");
        if opens_or_closes_fence {
            in_fence = !in_fence;
            out.push_str(raw_line);
            last_emitted_break = None;
            last_cjk_at_eol = false;
            continue;
        }
        if in_fence {
            out.push_str(raw_line);
            last_emitted_break = None;
            last_cjk_at_eol = false;
            continue;
        }

        let first_cjk = trimmed.chars().next().is_some_and(is_cjk);
        // Should we strip the *previous* line break?
        if last_cjk_at_eol
            && first_cjk
            && !trimmed.is_empty()
            && let Some(idx) = last_emitted_break
        {
            // Drop the newline (and any preceding \r) we just appended.
            let mut new_len = idx;
            if new_len > 0 && out.as_bytes()[new_len - 1] == b'\r' {
                new_len -= 1;
            }
            out.truncate(new_len);
        }
        out.push_str(raw_line);
        // Note the position of the trailing newline (if any) so we can pop it later.
        last_emitted_break = if raw_line.ends_with('\n') {
            Some(out.len() - 1)
        } else {
            None
        };
        last_cjk_at_eol = trimmed.chars().last().is_some_and(is_cjk);
        if trimmed.is_empty() {
            // Paragraph break — reset.
            last_cjk_at_eol = false;
            last_emitted_break = None;
        }
    }
    out
}

pub(crate) fn is_cjk(c: char) -> bool {
    let n = c as u32;
    (0x3000..=0x303F).contains(&n)   // CJK Symbols and Punctuation
        || (0x3040..=0x309F).contains(&n) // Hiragana
        || (0x30A0..=0x30FF).contains(&n) // Katakana
        || (0x3400..=0x4DBF).contains(&n) // CJK Ext A
        || (0x4E00..=0x9FFF).contains(&n) // CJK Unified
        || (0xAC00..=0xD7AF).contains(&n) // Hangul
        || (0xF900..=0xFAFF).contains(&n) // CJK Compatibility Ideographs
        || (0xFF00..=0xFFEF).contains(&n) // Halfwidth and Fullwidth Forms
}

// ---------- Feature detection ----------

fn mermaid_fence_re() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| Regex::new(r"(?m)^[ \t]*```mermaid\b").expect("mermaid fence regex"))
}
fn math_re() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| Regex::new(r"\$\$[^$]+\$\$|\$[^$\n]+\$").expect("math regex"))
}

#[derive(Debug, Default, Clone, Copy)]
pub struct Features {
    pub has_math: bool,
    pub has_mermaid: bool,
}

/// Legacy source-level detector. Kept for fallback / tests. The render
/// pipeline now uses [`detect_features_ast`] so `$...$` inside fenced code
/// blocks no longer triggers MathJax loading.
pub fn detect_features(src: &str) -> Features {
    Features {
        has_mermaid: mermaid_fence_re().is_match(src),
        has_math: math_re().is_match(src),
    }
}

/// Returns whether the inline math regex matches anywhere in `s`.
pub(crate) fn text_contains_math(s: &str) -> bool {
    math_re().is_match(s)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn admonition_basic() {
        let src = "before\n\n!!! note \"Heads up\"\n    body line 1\n    body line 2\n\nafter\n";
        let out = preprocess_admonition(src);
        assert!(
            out.contains(r#"<div class="admonition admonition-note""#),
            "{out}"
        );
        assert!(out.contains(r#"<p class="admonition-title">Heads up</p>"#));
        assert!(out.contains("body line 1"));
        assert!(out.contains("body line 2"));
        assert!(out.contains("</div>"));
        assert!(out.contains("after"));
    }

    #[test]
    fn admonition_default_title_from_kind() {
        let src = "!!! warning\n    danger\n";
        let out = preprocess_admonition(src);
        assert!(out.contains("admonition-warning"), "{out}");
        assert!(out.contains("Warning</p>"));
    }

    #[test]
    fn admonition_blank_line_within_block() {
        let src = "!!! tip\n    one\n\n    two\nnonindented\n";
        let out = preprocess_admonition(src);
        assert!(out.contains("one"));
        assert!(out.contains("two"));
        assert!(out.contains("nonindented"));
    }

    #[test]
    fn admonition_back_to_back_blocks() {
        let src = "!!! note\n    a\n\n!!! tip\n    b\n";
        let out = preprocess_admonition(src);
        assert!(out.contains("admonition-note"), "{out}");
        assert!(out.contains("admonition-tip"), "{out}");
    }

    #[test]
    fn wikilinks_basic_and_alias() {
        let out = preprocess_wikilinks("See [[Some Page]] and [[Foo|Bar Baz]].", None);
        assert!(out.contains(r#"href="Some-Page.html""#), "{out}");
        assert!(out.contains(">Some Page<"));
        assert!(out.contains(r#"href="Foo.html""#));
        assert!(out.contains(">Bar Baz<"));
    }

    #[test]
    fn wikilinks_resolver_overrides() {
        let r = |name: &str| {
            if name == "Page" {
                Some("custom/page.html".into())
            } else {
                None
            }
        };
        let out = preprocess_wikilinks("[[Page]]", Some(&r));
        assert!(out.contains(r#"href="custom/page.html""#));
    }

    #[test]
    fn mermaid_newline_escape() {
        let src = "```mermaid\nflowchart LR\n  A[\"Top\\nBottom\"] --> B\n```\n";
        let out = preprocess_mermaid(src);
        assert!(out.contains(r#"A["Top<br/>Bottom"]"#), "{out}");
        assert!(out.contains("flowchart LR\n"));
    }

    #[test]
    fn mermaid_outside_blocks_untouched() {
        let src = "prose with \\n stays\n\n```mermaid\nA[\"x\\ny\"]\n```\n";
        let out = preprocess_mermaid(src);
        assert!(out.contains(r#"prose with \n stays"#));
        assert!(out.contains(r#"A["x<br/>y"]"#));
    }

    #[test]
    fn mermaid_all_escape_kinds() {
        let src = "```mermaid\nA[\"line1\\nline2\\rline3\\tindent\\\\path\\\"q\"]\n```\n";
        let out = preprocess_mermaid(src);
        assert!(
            out.contains("line1<br/>line2<br/>line3&nbsp;&nbsp;&nbsp;&nbsp;indent"),
            "{out}"
        );
        assert!(out.contains(r#"\path"q"#), "{out}");
    }

    #[test]
    fn mermaid_unknown_escape_passthrough() {
        let out = preprocess_mermaid("```mermaid\nA[\"\\z\"]\n```\n");
        assert!(out.contains(r#"\z"#), "{out}");
    }

    // (Source-level heading-attr tests moved to `super::heading::tests` —
    // heading metadata extraction is now an AST pass.)

    #[test]
    fn cjk_softbreak_joins_cjk_lines() {
        assert_eq!(
            preprocess_cjk_softbreak("这是\n中文\n"),
            "这是中文\n",
            "consecutive CJK lines should join"
        );
    }

    #[test]
    fn cjk_softbreak_keeps_ascii_softbreak() {
        assert_eq!(
            preprocess_cjk_softbreak("hello\nworld\n"),
            "hello\nworld\n",
            "ASCII paragraphs untouched"
        );
    }

    #[test]
    fn cjk_softbreak_skips_fenced_code() {
        let src = "```\n这是\n中文\n```\n";
        // Inside a fenced code block the newline must be preserved.
        assert_eq!(preprocess_cjk_softbreak(src), src);
    }

    #[test]
    fn cjk_softbreak_respects_paragraph_break() {
        let src = "这是\n\n中文\n";
        // Blank line in the middle = paragraph break; do NOT join across it.
        assert_eq!(preprocess_cjk_softbreak(src), src);
    }

    #[test]
    fn detect_features_finds_both() {
        let f = detect_features("inline $a+b$ and\n```mermaid\nflowchart LR\nA-->B\n```\n");
        assert!(f.has_math);
        assert!(f.has_mermaid);
    }

    #[test]
    fn detect_features_none() {
        let f = detect_features("plain text");
        assert!(!f.has_math);
        assert!(!f.has_mermaid);
    }
}
