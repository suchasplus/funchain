//! Single authoritative source for heading metadata.
//!
//! [`process_headings`] is an AST pass that, in one document-order walk:
//!   1. parses any trailing `{#id .class data-x="v"}` attribute block off each
//!      heading and removes it from the AST (so neither the rendered `<h*>`
//!      body nor the TOC leaks the syntax),
//!   2. collects the heading's plain-text content,
//!   3. picks the final slug (explicit `#id` wins; otherwise [`GoAnchorizer`]
//!      mirrors goldmark `parser.IDs.Generate`),
//!   4. records a [`HeadingRecord`] for the TOC + [`HeadingRenderInfo`] for
//!      the renderer adapter — both consumed by ordinal, never by source line.
//!
//! No source-line correlation is involved, so admonition rewrites, CJK
//! softbreak joining, or any future source-mutating pass can't desync the
//! heading registry from what the renderer sees.

use std::collections::HashMap;

use comrak::nodes::{AstNode, NodeValue};
use regex::Regex;

use super::ast_passes::walk_ast;
use super::diagnostics::{RenderWarning, WarningKind};

/// Final, canonical heading metadata for one heading. Lives in
/// [`HeadingRegistry`] in document order; `ordinal == registry.records[i]`'s
/// index.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct HeadingRecord {
    pub ordinal: usize,
    pub level: u8,
    pub text: String,
    pub id: String,
    pub classes: Vec<String>,
    pub attrs: Vec<(String, String)>,
}

/// All heading metadata for one rendered document, in source order. TOC
/// builder and `MtHeadingAdapter` both project off this single registry.
///
/// `warnings` collects non-fatal diagnostics raised while processing the
/// headings (dropped unsafe attributes, suffixed duplicate explicit ids, …).
/// The renderer hoists these to [`crate::mt::render::RenderResult::warnings`].
#[derive(Debug, Clone, Default)]
pub struct HeadingRegistry {
    pub records: Vec<HeadingRecord>,
    pub warnings: Vec<RenderWarning>,
}

impl HeadingRegistry {
    /// Project the slug / class / attr triple into the lean data the renderer
    /// adapter needs.
    pub fn render_infos(&self) -> Vec<HeadingRenderInfo> {
        self.records
            .iter()
            .map(|r| HeadingRenderInfo {
                id: r.id.clone(),
                classes: r.classes.clone(),
                attrs: r.attrs.clone(),
            })
            .collect()
    }
}

/// Slim subset of [`HeadingRecord`] used by `MtHeadingAdapter` at render time.
#[derive(Debug, Clone, Default)]
pub struct HeadingRenderInfo {
    pub id: String,
    pub classes: Vec<String>,
    pub attrs: Vec<(String, String)>,
}

// ---------------------------------------------------------------------------
// AST pass
// ---------------------------------------------------------------------------

/// Walks the AST in document order, processes each heading node, returns the
/// registry. The AST is mutated in place — trailing `{#…}` blocks are stripped
/// off the last text-bearing leaf of each heading.
pub fn process_headings<'a>(root: &'a AstNode<'a>) -> HeadingRegistry {
    // First collect heading nodes, then process. Doing both in the walk closure
    // would require mutably borrowing the closure's state while it's calling
    // itself; this two-step keeps the borrow story trivial.
    let mut headings: Vec<&'a AstNode<'a>> = Vec::new();
    walk_ast(root, &mut |node| {
        if matches!(node.data.borrow().value, NodeValue::Heading(_)) {
            headings.push(node);
        }
    });

    let mut records = Vec::with_capacity(headings.len());
    let mut warnings: Vec<RenderWarning> = Vec::new();
    let mut anchorizer = GoAnchorizer::default();

    for h_node in headings {
        let level = match &h_node.data.borrow().value {
            NodeValue::Heading(nh) => nh.level,
            _ => continue,
        };

        let parsed = strip_trailing_attr_block(h_node);
        let text = collect_heading_text(h_node);
        let ordinal = records.len();

        // Surface any rejected attribute names as warnings so the author sees
        // them in the CLI / tests.
        for name in &parsed.dropped_attr_names {
            warnings.push(RenderWarning::from_heading(
                WarningKind::DroppedHeadingAttr,
                name.clone(),
                ordinal,
                text.clone(),
            ));
        }

        let id = match parsed.id {
            Some(custom) => {
                resolve_explicit_id(&custom, ordinal, &text, &mut anchorizer, &mut warnings)
            }
            None => anchorizer.anchorize(&text),
        };

        records.push(HeadingRecord {
            ordinal,
            level,
            text,
            id,
            classes: parsed.classes,
            attrs: parsed.attrs,
        });
    }

    HeadingRegistry { records, warnings }
}

/// Reserves an explicit `{#id}` slug against the shared anchorizer. On
/// collision: emits a [`WarningKind::DuplicateExplicitId`] and picks the
/// smallest `{custom}-N` that's still free, with `N` starting at 1 so the
/// numbering matches the auto-slug path (no `same → same-2` jumps).
///
/// `is_taken` spans both auto-generated and previously-recorded explicit
/// slugs, so the mixed case (`# Foo\n# Bar {#foo}`) is handled too.
fn resolve_explicit_id(
    custom: &str,
    ordinal: usize,
    heading_text: &str,
    anchorizer: &mut GoAnchorizer,
    warnings: &mut Vec<RenderWarning>,
) -> String {
    if !anchorizer.is_taken(custom) {
        anchorizer.record(custom);
        return custom.to_string();
    }
    warnings.push(RenderWarning::from_heading(
        WarningKind::DuplicateExplicitId,
        custom,
        ordinal,
        heading_text,
    ));
    let mut n: u32 = 1;
    loop {
        let candidate = format!("{custom}-{n}");
        if !anchorizer.is_taken(&candidate) {
            anchorizer.record(&candidate);
            return candidate;
        }
        n += 1;
    }
}

// ---------------------------------------------------------------------------
// Attribute extraction
// ---------------------------------------------------------------------------

#[derive(Debug, Default)]
struct ParsedAttrs {
    id: Option<String>,
    classes: Vec<String>,
    attrs: Vec<(String, String)>,
    /// Attribute names that were rejected by the safety whitelist. Surfaced as
    /// [`WarningKind::DroppedHeadingAttr`] warnings by `process_headings`.
    dropped_attr_names: Vec<String>,
}

fn trailing_attr_re() -> &'static Regex {
    static R: std::sync::OnceLock<Regex> = std::sync::OnceLock::new();
    R.get_or_init(|| Regex::new(r"\s+\{([^}]*)\}\s*$").expect("trailing attr regex"))
}

/// Finds the last text-bearing leaf inside `heading`, checks for a trailing
/// `{...}` attribute block, parses it, and strips it from the text node.
///
/// Returns the parsed attributes (or an empty `ParsedAttrs` if no block was
/// found — common for plain ATX/setext headings).
fn strip_trailing_attr_block<'a>(heading: &'a AstNode<'a>) -> ParsedAttrs {
    let Some(last_text) = find_last_text_node(heading) else {
        return ParsedAttrs::default();
    };

    let current: String = match &last_text.data.borrow().value {
        NodeValue::Text(t) => t.clone(),
        _ => return ParsedAttrs::default(),
    };

    let re = trailing_attr_re();
    let Some(caps) = re.captures(&current) else {
        return ParsedAttrs::default();
    };
    let block_content = caps[1].to_string();
    let match_start = caps.get(0).expect("regex match present").start();

    let parsed = parse_attr_block(&block_content);
    let stripped: String = current[..match_start].trim_end().to_string();
    last_text.data.borrow_mut().value = NodeValue::Text(stripped);
    parsed
}

fn find_last_text_node<'a>(heading: &'a AstNode<'a>) -> Option<&'a AstNode<'a>> {
    let mut last: Option<&'a AstNode<'a>> = None;
    walk_ast(heading, &mut |node| {
        if matches!(node.data.borrow().value, NodeValue::Text(_)) {
            last = Some(node);
        }
    });
    last
}

fn collect_heading_text<'a>(heading: &'a AstNode<'a>) -> String {
    let mut out = String::new();
    walk_ast(heading, &mut |node| match &node.data.borrow().value {
        NodeValue::Text(t) => out.push_str(t),
        NodeValue::Code(c) => out.push_str(&c.literal),
        _ => {}
    });
    out.trim().to_string()
}

/// Parses a `{...}` attribute block content (without the braces).
///
/// Recognised tokens, separated by whitespace:
///   * `#id`            → `id` (last wins)
///   * `.class`         → appended to `classes`
///   * `key=value`      → appended to `attrs` if `key` is a safe name
///   * `key="quoted v"` → same; value may contain spaces
fn parse_attr_block(s: &str) -> ParsedAttrs {
    let mut out = ParsedAttrs::default();
    let chars: Vec<char> = s.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        while i < chars.len() && chars[i].is_whitespace() {
            i += 1;
        }
        if i >= chars.len() {
            break;
        }
        match chars[i] {
            '#' => {
                i += 1;
                let start = i;
                while i < chars.len() && !chars[i].is_whitespace() {
                    i += 1;
                }
                let id: String = chars[start..i].iter().collect();
                if !id.is_empty() {
                    out.id = Some(id);
                }
            }
            '.' => {
                i += 1;
                let start = i;
                while i < chars.len() && !chars[i].is_whitespace() {
                    i += 1;
                }
                let class: String = chars[start..i].iter().collect();
                if !class.is_empty() {
                    out.classes.push(class);
                }
            }
            _ => {
                let key_start = i;
                while i < chars.len() && chars[i] != '=' && !chars[i].is_whitespace() {
                    i += 1;
                }
                let key: String = chars[key_start..i].iter().collect();
                if i < chars.len() && chars[i] == '=' {
                    i += 1;
                    // comrak's `parse.smart` rewrites the ASCII `"` we'd see
                    // in the markdown source into curly `“` / `”` by the time
                    // process_headings runs. Treat all three as opening /
                    // closing quotes so `data-x="a b"` still parses cleanly.
                    let value: String = if i < chars.len() && is_attr_quote(chars[i]) {
                        i += 1;
                        let v_start = i;
                        while i < chars.len() && !is_attr_quote(chars[i]) {
                            i += 1;
                        }
                        let v: String = chars[v_start..i].iter().collect();
                        if i < chars.len() {
                            i += 1;
                        }
                        v
                    } else {
                        let v_start = i;
                        while i < chars.len() && !chars[i].is_whitespace() {
                            i += 1;
                        }
                        chars[v_start..i].iter().collect()
                    };
                    match attr_disposition(&key) {
                        AttrDecision::Allow => out.attrs.push((key, value)),
                        AttrDecision::Drop => out.dropped_attr_names.push(key),
                        AttrDecision::Silent => { /* malformed shape: silently ignore */ }
                    }
                }
            }
        }
    }
    out
}

fn is_attr_quote(c: char) -> bool {
    matches!(c, '"' | '\u{201C}' | '\u{201D}')
}

#[derive(Debug, PartialEq, Eq)]
enum AttrDecision {
    /// Name is in the safe whitelist and has a valid shape.
    Allow,
    /// Name has a valid shape but is rejected by the whitelist (e.g.
    /// `onclick`, `style`, `href`). Surfaced as a warning.
    Drop,
    /// Name doesn't have a valid identifier shape at all. Dropped silently —
    /// this is usually a parse-level mistake, not a security concern.
    Silent,
}

/// Heading attribute safety policy. The renderer wires arbitrary HTML
/// attributes onto user-controlled `<h*>` tags, so anything that can execute
/// script or load external resources must be rejected. We use a strict
/// whitelist:
///
///   * exact: `lang`, `dir`, `title`
///   * prefix: `data-*`, `aria-*`
///
/// `id` and `class` are intentionally *not* in this list — they have
/// dedicated `#id` / `.class` syntax in the attribute block, and routing them
/// through the kv path would emit duplicate / conflicting HTML attributes.
///
/// Everything else (most notably `on*` event handlers, `style`, `href`, `src`)
/// is dropped and reported via [`WarningKind::DroppedHeadingAttr`].
///
/// Names whose shape is outright invalid (start with a digit, contain unusual
/// characters) are silently rejected because they're almost always
/// parse-level mistakes, not attempted attribute injection.
fn attr_disposition(s: &str) -> AttrDecision {
    if s.is_empty() {
        return AttrDecision::Silent;
    }
    let mut iter = s.chars();
    let first = iter.next().unwrap();
    if !first.is_ascii_alphabetic() {
        return AttrDecision::Silent;
    }
    if !s
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
    {
        return AttrDecision::Silent;
    }
    if is_safe_attr_name(s) {
        AttrDecision::Allow
    } else {
        AttrDecision::Drop
    }
}

fn is_safe_attr_name(s: &str) -> bool {
    matches!(s, "lang" | "dir" | "title") || s.starts_with("data-") || s.starts_with("aria-")
}

// ---------------------------------------------------------------------------
// Slug derivation — Go-style anchorizer
// ---------------------------------------------------------------------------

/// Slugifier + de-duplicator that mirrors goldmark's `parser.IDs.Generate`:
///   * ASCII letters / digits kept (uppercase lowered)
///   * space, hyphen, underscore → `-` (no collapsing)
///   * everything else dropped
///   * empty result → literal `"heading"`
///   * duplicates get `-N` (N counts from 1)
#[derive(Default)]
pub struct GoAnchorizer {
    counts: HashMap<String, u32>,
}

impl GoAnchorizer {
    pub fn anchorize(&mut self, text: &str) -> String {
        let base = slugify_go(text);
        let entry = self.counts.entry(base.clone()).or_insert(0);
        let n = *entry;
        *entry += 1;
        if n == 0 { base } else { format!("{base}-{n}") }
    }

    /// Marks `slug` as taken without producing a fresh one. Used when an
    /// explicit `#id` is given so subsequent collisions get suffixed.
    pub fn record(&mut self, slug: &str) {
        *self.counts.entry(slug.to_string()).or_insert(0) += 1;
    }

    /// True if `slug` has already been handed out (either by `anchorize` or
    /// `record`). Used to detect explicit-id collisions before issuing a
    /// suffix.
    pub fn is_taken(&self, slug: &str) -> bool {
        self.counts.get(slug).copied().unwrap_or(0) > 0
    }
}

pub fn slugify_go(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            'a'..='z' | '0'..='9' => out.push(c),
            'A'..='Z' => out.push(c.to_ascii_lowercase()),
            ' ' | '-' | '_' => out.push('-'),
            _ => { /* drop non-ASCII */ }
        }
    }
    if out.is_empty() {
        "heading".to_string()
    } else {
        out
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use comrak::{Arena, Options, parse_document};

    fn parse_and_process(src: &str) -> HeadingRegistry {
        let arena = Arena::new();
        let opts = Options::default();
        let root = parse_document(&arena, src, &opts);
        process_headings(root)
    }

    #[test]
    fn slugify_matches_go_for_unicode_inputs() {
        assert_eq!(slugify_go("Hello World"), "hello-world");
        assert_eq!(slugify_go("A_B"), "a-b");
        assert_eq!(slugify_go("Foo   Bar"), "foo---bar");
        assert_eq!(slugify_go("Foo---Bar"), "foo---bar");
        assert_eq!(slugify_go("Foo - Bar"), "foo---bar");
        assert_eq!(slugify_go("Café Déjà"), "caf-dj");
        assert_eq!(slugify_go("你好 世界"), "-");
        assert_eq!(slugify_go("你好"), "heading");
    }

    #[test]
    fn go_anchorizer_suffixes_duplicates_from_one() {
        let mut a = GoAnchorizer::default();
        assert_eq!(a.anchorize("Title"), "title");
        assert_eq!(a.anchorize("Title"), "title-1");
        assert_eq!(a.anchorize("Title"), "title-2");
        assert_eq!(a.anchorize("Other"), "other");
    }

    #[test]
    fn parse_attr_block_handles_id_class_kv() {
        let p = parse_attr_block(r#"#id .foo .bar data-x="a b" lang=en"#);
        assert_eq!(p.id.as_deref(), Some("id"));
        assert_eq!(p.classes, vec!["foo".to_string(), "bar".to_string()]);
        assert_eq!(
            p.attrs,
            vec![
                ("data-x".to_string(), "a b".to_string()),
                ("lang".to_string(), "en".to_string()),
            ]
        );
    }

    #[test]
    fn parse_attr_block_silently_drops_malformed_names() {
        // `1bad` starts with a digit → malformed shape → silently dropped.
        // `good` has a valid shape but isn't in the safety whitelist → dropped
        // and surfaced as a warning by `process_headings`.
        let p = parse_attr_block(r#"1bad=oops good=ok"#);
        assert!(p.attrs.is_empty(), "no kv attrs should pass the whitelist");
        // Only the whitelist-rejected name shows up in `dropped_attr_names` —
        // malformed shapes never make it that far.
        assert_eq!(p.dropped_attr_names, vec!["good".to_string()]);
    }

    #[test]
    fn parse_attr_block_allows_aria_and_data_prefixes() {
        let p = parse_attr_block(r#"data-x=1 aria-label=hi lang=zh"#);
        assert_eq!(
            p.attrs,
            vec![
                ("data-x".to_string(), "1".to_string()),
                ("aria-label".to_string(), "hi".to_string()),
                ("lang".to_string(), "zh".to_string()),
            ]
        );
        assert!(p.dropped_attr_names.is_empty());
    }

    #[test]
    fn parse_attr_block_drops_unsafe_event_handlers() {
        // onclick has a valid identifier shape but is not whitelisted →
        // dropped + recorded as a warning candidate.
        let p = parse_attr_block(r#"onclick="alert(1)" style=color:red"#);
        assert!(p.attrs.is_empty());
        assert_eq!(
            p.dropped_attr_names,
            vec!["onclick".to_string(), "style".to_string()]
        );
    }

    #[test]
    fn parse_attr_block_drops_id_and_class_via_kv_path() {
        // id / class have dedicated `#` / `.` syntax. Using the kv path is
        // ambiguous and would emit duplicate HTML attributes, so we drop and warn.
        let p = parse_attr_block(r#"id=foo class=bar"#);
        assert!(p.attrs.is_empty());
        assert_eq!(
            p.dropped_attr_names,
            vec!["id".to_string(), "class".to_string()]
        );
    }

    #[test]
    fn process_headings_strips_attr_block_from_text() {
        let reg = parse_and_process("# Title {#id .foo data-x=\"1\"}\n");
        assert_eq!(reg.records.len(), 1);
        let r = &reg.records[0];
        assert_eq!(r.level, 1);
        assert_eq!(r.id, "id");
        assert_eq!(r.classes, vec!["foo"]);
        assert_eq!(r.attrs, vec![("data-x".to_string(), "1".to_string())]);
        assert_eq!(r.text, "Title");
    }

    #[test]
    fn process_headings_ordinal_in_document_order() {
        let reg = parse_and_process("# A\n## B\n### C\n");
        assert_eq!(reg.records.len(), 3);
        assert_eq!(reg.records[0].ordinal, 0);
        assert_eq!(reg.records[1].ordinal, 1);
        assert_eq!(reg.records[2].ordinal, 2);
        assert_eq!(reg.records[0].id, "a");
        assert_eq!(reg.records[1].id, "b");
        assert_eq!(reg.records[2].id, "c");
    }

    #[test]
    fn process_headings_dedup_duplicates() {
        let reg = parse_and_process("# Same\n# Same\n# Same\n");
        let ids: Vec<&str> = reg.records.iter().map(|r| r.id.as_str()).collect();
        assert_eq!(ids, vec!["same", "same-1", "same-2"]);
    }

    #[test]
    fn process_headings_explicit_id_reserves_slug() {
        // Auto-slug for "Same" collides with explicit "#same" → auto gets suffixed.
        let reg = parse_and_process("# Same {#same}\n# Same\n");
        assert_eq!(reg.records[0].id, "same");
        assert_eq!(reg.records[1].id, "same-1");
    }

    #[test]
    fn process_headings_no_attr_block_keeps_text() {
        let reg = parse_and_process("# Plain heading\n");
        assert_eq!(reg.records[0].id, "plain-heading");
        assert_eq!(reg.records[0].text, "Plain heading");
    }

    #[test]
    fn process_headings_setext_handled_uniformly() {
        let reg = parse_and_process("Setext Heading {#setext-id}\n=================\n");
        assert_eq!(reg.records.len(), 1);
        assert_eq!(reg.records[0].id, "setext-id");
        assert_eq!(reg.records[0].text, "Setext Heading");
    }

    #[test]
    fn process_headings_nested_inline_stripping() {
        // The `{#id}` lives in the LAST text leaf of the heading, even if
        // earlier inline structure (emphasis) intervenes.
        let reg = parse_and_process("# **Bold** rest {#nested}\n");
        assert_eq!(reg.records[0].id, "nested");
        assert_eq!(reg.records[0].text, "Bold rest");
    }

    #[test]
    fn registry_render_infos_match_records() {
        let reg = parse_and_process("# T {#k .c}\n");
        let infos = reg.render_infos();
        assert_eq!(infos.len(), 1);
        assert_eq!(infos[0].id, "k");
        assert_eq!(infos[0].classes, vec!["c"]);
    }
}
