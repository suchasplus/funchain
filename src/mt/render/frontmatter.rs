//! YAML frontmatter splitter. Port of Go `internal/render/frontmatter.go`.

use serde::Deserialize;

#[derive(Debug, Default, Deserialize, Clone)]
#[serde(default)]
pub struct Frontmatter {
    pub title: String,
    pub description: String,
    pub tags: Vec<String>,
    pub theme: String,
}

#[derive(Debug)]
pub enum FrontmatterError {
    Yaml(serde_yml::Error),
}

impl std::fmt::Display for FrontmatterError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            FrontmatterError::Yaml(e) => write!(f, "yaml parse error: {e}"),
        }
    }
}

impl std::error::Error for FrontmatterError {}

/// Split the optional `---\n…\n---\n` YAML frontmatter from a markdown source.
///
/// Behaviour matches the Go side:
///   - No leading `---` → returns (default Frontmatter, full src, Ok)
///   - Unclosed fence → returns (default, full src, Ok)
///   - Malformed YAML inside a closed fence → returns the parse error
pub fn split_frontmatter(src: &str) -> Result<(Frontmatter, String), FrontmatterError> {
    if !src.starts_with("---") {
        return Ok((Frontmatter::default(), src.to_string()));
    }
    // Validate opening line ("---" optionally followed by spaces/tabs and a newline).
    let mut lines = src.split_inclusive('\n');
    let first = lines.next().unwrap_or("");
    if first.trim_end_matches(['\r', '\n', ' ', '\t']) != "---" {
        return Ok((Frontmatter::default(), src.to_string()));
    }

    // Collect lines until the closing fence.
    let mut yaml = String::new();
    let mut consumed_lines = 1usize; // already consumed opening fence
    let mut closed = false;
    for line in lines {
        consumed_lines += 1;
        let trimmed = line.trim_end_matches(['\r', '\n', ' ', '\t']);
        if trimmed == "---" {
            closed = true;
            break;
        }
        yaml.push_str(line);
    }
    if !closed {
        return Ok((Frontmatter::default(), src.to_string()));
    }

    let fm: Frontmatter = serde_yml::from_str(&yaml).map_err(FrontmatterError::Yaml)?;

    // Body = everything after the consumed lines.
    let body: String = src.split_inclusive('\n').skip(consumed_lines).collect();
    Ok((fm, body))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn none() {
        let (fm, body) = split_frontmatter("# Hello\nworld\n").unwrap();
        assert!(fm.title.is_empty());
        assert_eq!(body, "# Hello\nworld\n");
    }

    #[test]
    fn parsed() {
        let src = "---\ntitle: My Page\ndescription: A test\ntags: [a, b]\ntheme: dark\n---\n# Body\ncontent\n";
        let (fm, body) = split_frontmatter(src).unwrap();
        assert_eq!(fm.title, "My Page");
        assert_eq!(fm.description, "A test");
        assert_eq!(fm.tags, vec!["a", "b"]);
        assert_eq!(fm.theme, "dark");
        assert!(body.starts_with("# Body"), "body = {body:?}");
    }

    #[test]
    fn unclosed_fence_passthrough() {
        let src = "---\ntitle: x\n\n# no closing fence\n";
        let (fm, body) = split_frontmatter(src).unwrap();
        assert!(fm.title.is_empty());
        assert_eq!(body, src);
    }

    #[test]
    fn invalid_yaml_errors() {
        let src = "---\n: : :\n---\n# body\n";
        let err = split_frontmatter(src).unwrap_err();
        assert!(format!("{err}").contains("yaml"));
    }
}
