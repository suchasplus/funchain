//! argh-based CLI definitions for the `mt-rs` binary.

use argh::FromArgs;
use std::path::PathBuf;

/// mt-rs — render Markdown and open in browser (Rust port of mt).
#[derive(FromArgs, Debug)]
pub struct Cli {
    /// write self-contained HTML to PATH (no browser open unless --no-open is omitted).
    #[argh(option, short = 'o')]
    pub output: Option<PathBuf>,

    /// start dev server with live-reload (single-file or directory mode).
    #[argh(switch)]
    pub serve: bool,

    /// port for --serve mode (default 7331).
    #[argh(option, default = "7331")]
    pub port: u16,

    /// don't auto-open the browser.
    #[argh(switch)]
    pub no_open: bool,

    /// default theme: auto | light | dark.
    #[argh(option, default = "String::from(\"auto\")")]
    pub theme: String,

    /// print rendered HTML to stdout and exit.
    #[argh(switch)]
    pub print: bool,

    /// include files hidden by the global config's exclude list
    /// (CLAUDE.md, AGENTS.md, … by default) in directory mode.
    #[argh(switch)]
    pub all: bool,

    /// write a zip archive of the rendered output (html + assets + md
    /// sources) to PATH, for distribution. Works on files and directories.
    #[argh(option)]
    pub archive: Option<PathBuf>,

    /// glob over root-relative paths (repeatable): only matching md files
    /// are rendered. `*` stops at `/`; use `**` to span directories.
    #[argh(option)]
    pub include: Vec<String>,

    /// glob over root-relative paths (repeatable): matching md files are
    /// dropped; wins over --include.
    #[argh(option)]
    pub exclude: Vec<String>,

    /// omit markdown sources from the --archive zip (html + assets only).
    #[argh(switch)]
    pub no_md: bool,

    /// print version metadata and exit.
    #[argh(switch, short = 'V')]
    pub version: bool,

    /// markdown file or directory.
    #[argh(positional)]
    pub target: Option<String>,
}
