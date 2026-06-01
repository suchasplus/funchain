//! Nav tree + relative-path helper. Mirror of Go `internal/site/tree.go`.

use std::fmt::Write;

use super::scan::Entry;

#[derive(Debug, Clone, Default)]
pub struct TreeNode {
    /// Display label (folder name or page title).
    pub name: String,
    /// Relative md path (empty for folders).
    pub rel: String,
    /// Relative html path (empty for folders).
    pub out_rel: String,
    pub is_dir: bool,
    pub children: Vec<TreeNode>,
}

/// Groups entries by their directory components, sort folders first.
pub fn build_tree(entries: &[Entry]) -> TreeNode {
    let mut root = TreeNode {
        is_dir: true,
        ..Default::default()
    };
    for e in entries {
        let parts: Vec<&str> = e.rel.split('/').collect();
        insert_path(&mut root, &parts, e);
    }
    sort_tree(&mut root);
    root
}

fn insert_path(parent: &mut TreeNode, parts: &[&str], e: &Entry) {
    let (head, tail) = match parts.split_first() {
        Some(s) => s,
        None => return,
    };
    if tail.is_empty() {
        let label = if !e.title.is_empty() {
            e.title.clone()
        } else {
            e.stem.clone()
        };
        parent.children.push(TreeNode {
            name: label,
            rel: e.rel.clone(),
            out_rel: e.out_rel.clone(),
            is_dir: false,
            children: Vec::new(),
        });
    } else {
        let dir = find_or_create_dir(parent, head);
        insert_path(dir, tail, e);
    }
}

fn find_or_create_dir<'a>(parent: &'a mut TreeNode, name: &str) -> &'a mut TreeNode {
    let pos = parent
        .children
        .iter()
        .position(|c| c.is_dir && c.name == name);
    if let Some(i) = pos {
        return &mut parent.children[i];
    }
    parent.children.push(TreeNode {
        name: name.to_string(),
        is_dir: true,
        ..Default::default()
    });
    parent.children.last_mut().unwrap()
}

fn sort_tree(node: &mut TreeNode) {
    node.children.sort_by(|a, b| match (a.is_dir, b.is_dir) {
        (true, false) => std::cmp::Ordering::Less,
        (false, true) => std::cmp::Ordering::Greater,
        _ => a.name.to_lowercase().cmp(&b.name.to_lowercase()),
    });
    for c in node.children.iter_mut() {
        if c.is_dir {
            sort_tree(c);
        }
    }
}

/// Renders the tree as a nested `<ul>` block. `current_rel` (a `rel` path,
/// not `out_rel`) decides which link gets `is-current`; `current_out_rel`
/// drives the [`rel_path`] computation for each link's href.
pub fn render_tree(root: &TreeNode, current_rel: &str, current_out_rel: &str) -> String {
    if root.children.is_empty() {
        return String::new();
    }
    let mut buf = String::with_capacity(512);
    render_list(&mut buf, root, current_rel, current_out_rel, 0);
    buf
}

fn render_list(
    buf: &mut String,
    node: &TreeNode,
    current_rel: &str,
    current_out_rel: &str,
    depth: u32,
) {
    if depth > 0 {
        let _ = write!(buf, r#"<ul class="mt-nav__list" data-depth="{depth}">"#);
    } else {
        buf.push_str(r#"<ul class="mt-nav__list mt-nav__list--root">"#);
    }
    for c in &node.children {
        if c.is_dir {
            let _ = write!(
                buf,
                r#"<li class="mt-nav__group"><div class="mt-nav__folder">{}</div>"#,
                html_escape(&c.name)
            );
            render_list(buf, c, current_rel, current_out_rel, depth + 1);
            buf.push_str("</li>");
        } else {
            let mut cls = String::from("mt-nav__link");
            if c.rel == current_rel {
                cls.push_str(" is-current");
            }
            // `rel_path` returns the decoded slash form. Percent-encode each
            // segment before emitting so browsers receive a valid URL — see
            // `crate::mt::site::url_path`.
            let href = super::url_path::encode_segments(&rel_path(current_out_rel, &c.out_rel));
            let _ = write!(
                buf,
                r#"<li><a class="{}" href="{}">{}</a></li>"#,
                cls,
                html_escape(&href),
                html_escape(&c.name)
            );
        }
    }
    buf.push_str("</ul>");
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

/// Returns the relative href from one html page to another, slash-separated.
/// Both arguments are slash-separated paths relative to the site root, e.g.
/// `rel_path("guide/intro.html", "ref/api.html") == "../ref/api.html"`.
pub fn rel_path(from_out: &str, to_out: &str) -> String {
    let from_dir = parent_of(from_out);
    if from_dir.is_empty() || from_dir == "." {
        return to_out.to_string();
    }
    let from_parts: Vec<&str> = split_clean(from_dir);
    let to_parts: Vec<&str> = split_clean(to_out);
    let mut i = 0;
    while i < from_parts.len() && i < to_parts.len() && from_parts[i] == to_parts[i] {
        i += 1;
    }
    let up = from_parts.len() - i;
    let mut parts: Vec<&str> = Vec::with_capacity(up + to_parts.len() - i);
    parts.extend(std::iter::repeat_n("..", up));
    parts.extend_from_slice(&to_parts[i..]);
    parts.join("/")
}

fn parent_of(p: &str) -> &str {
    match p.rfind('/') {
        Some(i) => &p[..i],
        None => "",
    }
}

fn split_clean(p: &str) -> Vec<&str> {
    if p.is_empty() || p == "." {
        return Vec::new();
    }
    p.split('/').collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn entry(rel: &str, title: &str) -> Entry {
        Entry {
            abs: PathBuf::new(),
            rel: rel.into(),
            out_rel: rel.replace(".md", ".html"),
            stem: rel
                .rsplit('/')
                .next()
                .unwrap_or("")
                .trim_end_matches(".md")
                .into(),
            title: title.into(),
        }
    }

    #[test]
    fn build_and_render_tree() {
        let entries = vec![
            entry("README.md", "Home"),
            entry("guide/intro.md", "Intro"),
            entry("guide/deep/page.md", ""),
        ];
        let root = build_tree(&entries);
        let html = render_tree(&root, "guide/intro.md", "guide/intro.html");
        assert!(
            html.contains(r#"mt-nav__folder">guide<"#),
            "missing folder: {html}"
        );
        assert!(
            html.contains(r#"class="mt-nav__link is-current" href="intro.html""#),
            "current link wrong: {html}"
        );
        assert!(
            html.contains(r#"href="../README.html""#),
            "README href wrong: {html}"
        );
    }

    #[test]
    fn rel_path_examples() {
        assert_eq!(
            rel_path("README.html", "guide/intro.html"),
            "guide/intro.html"
        );
        assert_eq!(
            rel_path("guide/intro.html", "README.html"),
            "../README.html"
        );
        assert_eq!(rel_path("guide/intro.html", "guide/api.html"), "api.html");
        assert_eq!(
            rel_path("guide/deep/x.html", "guide/api.html"),
            "../api.html"
        );
        assert_eq!(
            rel_path("guide/deep/x.html", "ref/api.html"),
            "../../ref/api.html"
        );
    }

    #[test]
    fn empty_tree_yields_empty() {
        let root = TreeNode {
            is_dir: true,
            ..Default::default()
        };
        assert!(render_tree(&root, "", "").is_empty());
    }
}
