//! Non-fatal diagnostics surfaced from the render pipeline.
//!
//! These are problems the renderer can recover from (e.g. dropping an unsafe
//! attribute, suffixing a duplicate explicit id) but a human author probably
//! wants to know about. The library is silent about them — it just attaches
//! them to its return value. CLI / serve callers decide what to do (print to
//! stderr, surface in the UI, store in a build log, …).

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RenderWarning {
    pub kind: WarningKind,
    /// Short payload that identifies the offending value — the duplicate slug,
    /// the rejected attribute name, etc. Display formatters quote this.
    pub detail: String,
    /// Document-order ordinal of the heading the warning originated from, when
    /// applicable. `None` for warnings that don't come from a heading.
    pub heading_ordinal: Option<usize>,
    /// Plain-text content of that heading (with attribute block already
    /// stripped). Lets the CLI surface a human-readable pointer ("heading #2
    /// \"Foo Bar\"") rather than forcing the user to count by hand.
    pub heading_text: Option<String>,
}

impl RenderWarning {
    /// Construct a warning that originates from a specific heading.
    pub fn from_heading(
        kind: WarningKind,
        detail: impl Into<String>,
        ordinal: usize,
        text: impl Into<String>,
    ) -> Self {
        Self {
            kind,
            detail: detail.into(),
            heading_ordinal: Some(ordinal),
            heading_text: Some(text.into()),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WarningKind {
    /// Two or more headings share the same explicit `{#id}`. The renderer
    /// suffixes the later occurrences with `-1`, `-2`, … (matching auto-slug
    /// dedup) so anchor navigation still works.
    DuplicateExplicitId,
    /// A heading attribute was rejected by the safety whitelist (everything
    /// outside `lang`, `dir`, `title`, `data-*`, `aria-*`).
    DroppedHeadingAttr,
}

impl std::fmt::Display for RenderWarning {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if let (Some(ord), Some(text)) = (self.heading_ordinal, &self.heading_text) {
            write!(f, "heading #{ord} \"{text}\": ")?;
        }
        match self.kind {
            WarningKind::DuplicateExplicitId => {
                write!(f, "duplicate explicit heading id `{}`", self.detail)
            }
            WarningKind::DroppedHeadingAttr => {
                write!(f, "dropped unsafe heading attribute `{}`", self.detail)
            }
        }
    }
}
