//! AST passes — token-aware transforms that run on the parsed comrak tree.
//!
//! Each pass takes the root node (and sometimes the arena) and mutates the
//! tree in place. They share a small set of walk / ancestry helpers so the
//! same "skip Code / CodeBlock / Link / Image" semantics applies everywhere.

use std::cell::RefCell;

use comrak::Arena;
use comrak::arena_tree::Node;
use comrak::nodes::{Ast, AstNode, LineColumn, NodeValue};

use super::preprocess::{
    self, Features, WikilinkResolver, apply_mermaid_escapes, is_cjk, split_wikilink_parts,
    text_contains_math,
};

// ---------------------------------------------------------------------------
// Walk helpers
// ---------------------------------------------------------------------------

/// Walk every node in document order (depth-first, pre-order).
pub(super) fn walk_ast<'a>(node: &'a AstNode<'a>, cb: &mut dyn FnMut(&'a AstNode<'a>)) {
    cb(node);
    for child in node.children() {
        walk_ast(child, cb);
    }
}

/// True if any ancestor up to `root` is a `Link` or `Image` — used to prevent
/// rewriting wikilinks inside already-linked text / image alt nodes.
pub(super) fn inside_link_or_image<'a>(node: &'a AstNode<'a>) -> bool {
    let mut cur = node.parent();
    while let Some(p) = cur {
        if matches!(
            &p.data.borrow().value,
            NodeValue::Link(_) | NodeValue::Image(_)
        ) {
            return true;
        }
        cur = p.parent();
    }
    false
}

pub(super) fn last_char_of_textlike<'a>(node: &'a AstNode<'a>) -> Option<char> {
    match &node.data.borrow().value {
        NodeValue::Text(t) => t.chars().last(),
        NodeValue::Code(c) => c.literal.chars().last(),
        _ => None,
    }
}

pub(super) fn first_char_of_textlike<'a>(node: &'a AstNode<'a>) -> Option<char> {
    match &node.data.borrow().value {
        NodeValue::Text(t) => t.chars().next(),
        NodeValue::Code(c) => c.literal.chars().next(),
        _ => None,
    }
}

// ---------------------------------------------------------------------------
// Passes
// ---------------------------------------------------------------------------

/// Rewrites `[[Page]]` / `[[Page|Alias]]` inside text nodes into a sequence of
/// AST nodes: `HtmlInline("<a …>") + Text(alias) + HtmlInline("</a>")`.
///
/// Splitting the visible text into its own `Text` node (rather than embedding
/// the whole `<a>…</a>` as a single `HtmlInline`) is what lets later AST
/// walkers — heading text collection in particular — pick up the alias as
/// part of the heading title and TOC entry.
///
/// Code, CodeBlock and HtmlBlock nodes are leaves whose content lives in
/// `literal`, so walking only Text nodes already skips them. Link/Image
/// children are skipped explicitly to avoid nesting `<a>` tags.
pub(super) fn transform_wikilinks<'a>(
    arena: &'a Arena<AstNode<'a>>,
    root: &'a AstNode<'a>,
    resolver: Option<&WikilinkResolver<'_>>,
) {
    let mut targets: Vec<&'a AstNode<'a>> = Vec::new();
    walk_ast(root, &mut |node| {
        if matches!(node.data.borrow().value, NodeValue::Text(_)) && !inside_link_or_image(node) {
            targets.push(node);
        }
    });

    for target in targets {
        let text = match &target.data.borrow().value {
            NodeValue::Text(t) => t.clone(),
            _ => continue,
        };
        if !text.contains("[[") {
            continue;
        }
        let parts = split_wikilink_parts(&text, resolver);
        if parts
            .iter()
            .all(|p| matches!(p, preprocess::WikiPart::Text(_)))
        {
            continue;
        }
        // Flatten each WikiPart into one or three AST node values, then
        // insert them after the target (in reverse so the final order matches
        // `parts`) and detach the original Text node.
        let pos = LineColumn { line: 0, column: 0 };
        let mut emitted: Vec<NodeValue> = Vec::with_capacity(parts.len() * 3);
        for part in parts {
            match part {
                preprocess::WikiPart::Text(s) => emitted.push(NodeValue::Text(s)),
                preprocess::WikiPart::Link { open, text, close } => {
                    emitted.push(NodeValue::HtmlInline(open));
                    emitted.push(NodeValue::Text(text));
                    emitted.push(NodeValue::HtmlInline(close));
                }
            }
        }
        for value in emitted.into_iter().rev() {
            let new_node = arena.alloc(Node::new(RefCell::new(Ast::new(value, pos))));
            target.insert_after(new_node);
        }
        target.detach();
    }
}

/// Rewrites escape sequences inside fenced ```` ```mermaid ```` blocks to the
/// Mermaid-friendly equivalents (`\n` → `<br/>`, etc.). Other code blocks are
/// left alone, so `\n` inside a Python fence remains literal.
pub(super) fn normalize_mermaid_codeblocks<'a>(root: &'a AstNode<'a>) {
    walk_ast(root, &mut |node| {
        let mut data = node.data.borrow_mut();
        if let NodeValue::CodeBlock(cb) = &mut data.value
            && cb.info.trim() == "mermaid"
        {
            cb.literal = apply_mermaid_escapes(&cb.literal);
        }
    });
}

/// Removes `SoftBreak` nodes when both their immediate text-bearing siblings
/// end / begin with a CJK codepoint, mirroring goldmark's `extension.CJK`.
/// Works equally well inside `<p>`, `<li>`, `<blockquote>`, … because we walk
/// the tree (not raw lines).
pub(super) fn strip_cjk_softbreaks<'a>(root: &'a AstNode<'a>) {
    let mut targets: Vec<&'a AstNode<'a>> = Vec::new();
    walk_ast(root, &mut |node| {
        if matches!(node.data.borrow().value, NodeValue::SoftBreak) {
            targets.push(node);
        }
    });
    for node in targets {
        let prev_cjk = node
            .previous_sibling()
            .and_then(last_char_of_textlike)
            .is_some_and(is_cjk);
        let next_cjk = node
            .next_sibling()
            .and_then(first_char_of_textlike)
            .is_some_and(is_cjk);
        if prev_cjk && next_cjk {
            node.data.borrow_mut().value = NodeValue::Text(String::new());
        }
    }
}

/// AST-aware feature detector. Replaces the source-regex variant that used to
/// match `$...$` inside fenced code blocks (and so loaded MathJax for docs
/// that only mentioned the syntax in code).
///
/// Rules:
/// * `has_mermaid` ← there's at least one `CodeBlock` with `info == "mermaid"`
/// * `has_math`    ← at least one `Text` node matches the math regex.
///   `Code` and `CodeBlock` are leaves with content in `literal`, so walking
///   Text nodes already excludes them.
pub(super) fn detect_features_ast<'a>(root: &'a AstNode<'a>) -> Features {
    let mut features = Features::default();
    walk_ast(root, &mut |node| {
        let data = node.data.borrow();
        match &data.value {
            NodeValue::CodeBlock(cb) if cb.info.trim() == "mermaid" => {
                features.has_mermaid = true;
            }
            NodeValue::Text(t) if !features.has_math && text_contains_math(t) => {
                features.has_math = true;
            }
            _ => {}
        }
    });
    features
}

#[cfg(test)]
mod tests {
    use super::*;
    use comrak::{Options, parse_document};

    fn parse<'a>(arena: &'a Arena<AstNode<'a>>, src: &str) -> &'a AstNode<'a> {
        let mut opts = Options::default();
        opts.parse.smart = false;
        opts.render.unsafe_ = true;
        parse_document(arena, src, &opts)
    }

    #[test]
    fn detect_features_ignores_math_inside_fenced_code() {
        // The dollar-delimited math syntax shows up inside a python fence;
        // MathJax should NOT be flagged.
        let arena = Arena::new();
        let root = parse(
            &arena,
            "```python\nprice = $5 + $3\n```\n\nNormal paragraph with no math.\n",
        );
        let f = detect_features_ast(root);
        assert!(!f.has_math, "math falsely detected inside python fence");
        assert!(!f.has_mermaid);
    }

    #[test]
    fn detect_features_ignores_math_inside_inline_code() {
        // Inline `$x$` inside backticks should also not register.
        let arena = Arena::new();
        let root = parse(&arena, "Use `$x + $y` for arithmetic.\n");
        let f = detect_features_ast(root);
        assert!(!f.has_math, "inline-code math falsely detected");
    }

    #[test]
    fn detect_features_finds_real_inline_math() {
        let arena = Arena::new();
        let root = parse(&arena, "The energy is $E = mc^2$, classic.\n");
        let f = detect_features_ast(root);
        assert!(f.has_math);
    }

    #[test]
    fn detect_features_finds_display_math() {
        let arena = Arena::new();
        let root = parse(&arena, "Equation:\n\n$$ \\int x dx $$\n\nDone.\n");
        let f = detect_features_ast(root);
        assert!(f.has_math);
    }

    #[test]
    fn detect_features_finds_mermaid_fence() {
        let arena = Arena::new();
        let root = parse(&arena, "```mermaid\nflowchart LR\nA-->B\n```\n");
        let f = detect_features_ast(root);
        assert!(f.has_mermaid);
        // Mermaid block contains no `$...$` so math stays false.
        assert!(!f.has_math);
    }

    #[test]
    fn detect_features_does_not_match_non_mermaid_fence() {
        let arena = Arena::new();
        let root = parse(&arena, "```python\nprint('hi')\n```\n");
        let f = detect_features_ast(root);
        assert!(!f.has_mermaid);
    }
}
