//! TOC builder.
//!
//! Pure projection from the canonical [`HeadingRegistry`](super::heading::HeadingRegistry)
//! into a nested `<ul>` tree. All slug / class / attribute decisions happen in
//! `heading.rs`; this module never re-anchorizes or otherwise re-derives heading
//! metadata, so the TOC and the rendered `<h*>` ids are guaranteed to agree.

use std::fmt::Write;

use crate::mt::render::heading::HeadingRecord;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TocNode {
    pub level: u8,
    pub id: String,
    pub text: String,
    pub children: Vec<TocNode>,
}

/// Builds a nested TOC from the heading registry, filtered to `max_level`.
pub fn build_toc(records: &[HeadingRecord], max_level: u8) -> Vec<TocNode> {
    let flat: Vec<TocNode> = records
        .iter()
        .filter(|r| r.level <= max_level)
        .map(|r| TocNode {
            level: r.level,
            id: r.id.clone(),
            text: r.text.trim().to_string(),
            children: Vec::new(),
        })
        .collect();
    nest_headings(flat)
}

fn nest_headings(flat: Vec<TocNode>) -> Vec<TocNode> {
    let mut roots: Vec<TocNode> = Vec::new();
    let mut path: Vec<Vec<usize>> = Vec::new();

    for node in flat {
        while let Some(idx_chain) = path.last() {
            let level_at = get_at(&roots, idx_chain).level;
            if level_at >= node.level {
                path.pop();
            } else {
                break;
            }
        }
        match path.last().cloned() {
            None => {
                roots.push(node);
                path.push(vec![roots.len() - 1]);
            }
            Some(parent_chain) => {
                let parent = get_at_mut(&mut roots, &parent_chain);
                parent.children.push(node);
                let mut new_chain = parent_chain;
                new_chain.push(get_at(&roots, &new_chain).children.len() - 1);
                path.push(new_chain);
            }
        }
    }
    roots
}

fn get_at<'a>(roots: &'a [TocNode], chain: &[usize]) -> &'a TocNode {
    let mut cur = &roots[chain[0]];
    for &i in &chain[1..] {
        cur = &cur.children[i];
    }
    cur
}

fn get_at_mut<'a>(roots: &'a mut [TocNode], chain: &[usize]) -> &'a mut TocNode {
    let mut cur = &mut roots[chain[0]];
    for &i in &chain[1..] {
        cur = &mut cur.children[i];
    }
    cur
}

/// Renders the TOC as a nested `<ul>` tree. Empty input → empty string.
pub fn render_toc(nodes: &[TocNode]) -> String {
    if nodes.is_empty() {
        return String::new();
    }
    let mut buf = String::with_capacity(256);
    render_list(&mut buf, nodes);
    buf
}

fn render_list(buf: &mut String, nodes: &[TocNode]) {
    buf.push_str("<ul>");
    for n in nodes {
        let _ = write!(
            buf,
            r##"<li class="lvl-{}"><a href="#{}">{}</a>"##,
            n.level,
            html_escape(&n.id),
            html_escape(&n.text)
        );
        if !n.children.is_empty() {
            render_list(buf, &n.children);
        }
        buf.push_str("</li>");
    }
    buf.push_str("</ul>");
}

pub(crate) fn html_escape(s: &str) -> String {
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

#[cfg(test)]
mod tests {
    use super::*;

    fn rec(level: u8, id: &str, text: &str) -> HeadingRecord {
        HeadingRecord {
            ordinal: 0,
            level,
            id: id.into(),
            text: text.into(),
            classes: vec![],
            attrs: vec![],
        }
    }

    #[test]
    fn render_toc_hierarchy() {
        let records = vec![
            rec(1, "intro", "Intro"),
            rec(2, "a", "A"),
            rec(3, "a1", "A1"),
            rec(2, "b", "B"),
        ];
        let toc = build_toc(&records, 4);
        let html = render_toc(&toc);
        for want in [
            r##"<a href="#intro">Intro</a>"##,
            r##"<a href="#a">A</a>"##,
            r##"<a href="#a1">A1</a>"##,
            r##"<a href="#b">B</a>"##,
        ] {
            assert!(html.contains(want), "missing {want} in {html}");
        }
    }

    #[test]
    fn build_toc_filters_by_max_level() {
        let records = vec![rec(1, "h1", "H1"), rec(2, "h2", "H2"), rec(5, "h5", "H5")];
        let toc = build_toc(&records, 4);
        // H5 is filtered out
        assert_eq!(toc.len(), 1);
        assert_eq!(toc[0].id, "h1");
        assert_eq!(toc[0].children.len(), 1);
        assert_eq!(toc[0].children[0].id, "h2");
    }

    #[test]
    fn empty_input_yields_empty_string() {
        assert!(render_toc(&[]).is_empty());
    }
}
