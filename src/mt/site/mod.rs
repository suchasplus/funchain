//! Multi-file site builder. Mirror of Go `internal/site`.
//!
//! Pipeline (see `build`):
//!   1. [`scan`]      — recursive *.md walk
//!   2. Title pass    — render each entry once to capture frontmatter / first-H1
//!   3. [`tree`]      — folder-grouped nav HTML
//!   4. [`wikilinks`] — basename→outRel index
//!   5. Render pass   — re-render with wikilink resolver + write each HTML
//!   6. Static assets → OutDir/assets/

pub mod build;
pub mod scan;
pub mod tree;
pub mod url_path;
pub mod wikilinks;

pub use build::{
    BuildOptions, BuildReport, Context, PageRender, PageWarning, build, sanitize_root_name,
};
pub use scan::{Entry, index_of, landing_page, scan};
pub use tree::{TreeNode, build_tree, rel_path, render_tree};
pub use wikilinks::NameIndex;
