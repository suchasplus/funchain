//! mt — Markdown viewer port from Go. Module tree mirrors `madtool/internal/`:
//!
//!   * `assets`  — embedded static files + minijinja page template
//!   * `render`  — markdown → HTML pipeline
//!   * `site`    — multi-file site builder (M5)
//!   * `serve`   — dev server with live-reload (M6)
//!
//! The crate-level binary entrypoint is `src/bin/mt-rs.rs`, which calls
//! `funchain::mt::run`.

pub mod assets;
pub mod browser;
pub mod cli;
pub mod config;
pub mod render;
pub mod serve;
pub mod site;

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use crate::mt::assets::page::{PageOptions, build_page, render as render_page};
use crate::mt::render::Renderer;

/// Entry point invoked from `src/bin/mt-rs.rs`. Parses argv via argh, dispatches.
pub fn run() -> ExitCode {
    let cli: cli::Cli = argh::from_env();
    run_with(cli)
}

/// Same as [`run`] but takes a parsed CLI struct (used by tests).
pub fn run_with(cli: cli::Cli) -> ExitCode {
    if cli.version {
        println!("{}", crate::version::full());
        return ExitCode::SUCCESS;
    }
    let Some(target) = cli.target.clone() else {
        eprintln!("mt: missing FILE.md or DIR argument. Use --help.");
        return ExitCode::from(2);
    };
    let path = PathBuf::from(&target);
    let meta = match path.metadata() {
        Ok(m) => m,
        Err(e) => {
            eprintln!("mt: input: {e}");
            return ExitCode::FAILURE;
        }
    };
    let result = if meta.is_dir() {
        run_dir(&cli, &path)
    } else {
        run_file(&cli, &path)
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("mt: {e}");
            ExitCode::FAILURE
        }
    }
}

fn cli_warning_sink() -> serve::WarningSink {
    std::sync::Arc::new(|path: &Path, w: &render::RenderWarning| {
        eprintln!("mt: warning: {}: {w}", path.display());
    })
}

fn serve_single_mode(cli: &cli::Cli, file: &Path, theme: &str) -> Result<(), BoxedErr> {
    let server = serve::serve_single(serve::SingleConfig {
        file: file.to_path_buf(),
        port: cli.port,
        theme: theme.to_string(),
        on_warning: Some(cli_warning_sink()),
    })?;
    let url = server.url();
    eprintln!("mt: serving {} on {} (Ctrl-C to stop)", file.display(), url);
    if !cli.no_open
        && let Err(e) = browser::open_url(&url)
    {
        eprintln!("mt: browser open failed: {e}");
    }
    server.join();
    Ok(())
}

fn serve_dir_mode(cli: &cli::Cli, dir: &Path) -> Result<(), BoxedErr> {
    let cfg = config::load();
    let server = serve::serve_dir(serve::DirConfig {
        root: dir.to_path_buf(),
        port: cli.port,
        theme: cli.theme.clone(),
        on_warning: Some(cli_warning_sink()),
        exclude: effective_excludes(cli, &cfg),
        nav_filenames: cfg.nav_filenames,
    })?;
    let url = server.landing_url();
    eprintln!("mt: serving {} on {} (Ctrl-C to stop)", dir.display(), url);
    if !cli.no_open
        && let Err(e) = browser::open_url(&url)
    {
        eprintln!("mt: browser open failed: {e}");
    }
    server.join();
    Ok(())
}

/// Exclude list for directory scans: the global config's list, unless the
/// user passed `--all` to bypass it for this run.
fn effective_excludes(cli: &cli::Cli, cfg: &config::MtConfig) -> Vec<String> {
    if cli.all {
        Vec::new()
    } else {
        cfg.exclude.clone()
    }
}

fn run_dir(cli: &cli::Cli, dir: &Path) -> Result<(), BoxedErr> {
    if cli.print {
        return Err("--print not supported in directory mode".into());
    }
    if cli.output.is_some() {
        return Err("-o single-file output not supported in directory mode".into());
    }
    if cli.serve {
        return serve_dir_mode(cli, dir);
    }
    let cfg = config::load();
    let slug = site::sanitize_root_name(dir);
    let out_root = std::env::temp_dir().join("mt").join(slug);
    let renderer = Renderer::new();
    let report = site::build(
        site::BuildOptions {
            root: dir.to_path_buf(),
            out_dir: out_root.clone(),
            site_name: dir
                .file_name()
                .and_then(|s| s.to_str())
                .unwrap_or("site")
                .to_string(),
            theme: cli.theme.clone(),
            live_reload_js: String::new(),
            exclude: effective_excludes(cli, &cfg),
            nav_filenames: cfg.nav_filenames,
        },
        &renderer,
    )?;
    for pw in &report.warnings {
        eprintln!("mt: warning: {}: {}", pw.source.display(), pw.warning);
    }
    eprintln!("mt: rendered site → {}", out_root.display());
    if !cli.no_open {
        browser::open_file(&report.landing)?;
    }
    Ok(())
}

type BoxedErr = Box<dyn std::error::Error>;

fn run_file(cli: &cli::Cli, path: &Path) -> Result<(), BoxedErr> {
    let src = std::fs::read_to_string(path)?;

    // Priority order mirrors the Go side (cmd/mt/main.go): `--print` wins
    // over `--serve` so `mt-rs --print --serve file.md` always returns HTML
    // to stdout and exits.
    //
    // For `--serve` (without `--print`) the watcher will render the file
    // itself, and its warning sink emits diagnostics — so we skip the
    // up-front render entirely. We only need the frontmatter to resolve the
    // theme override, which is much cheaper.
    if cli.serve && !cli.print {
        let (fm, _body) = render::split_frontmatter(&src)?;
        let theme = if !fm.theme.is_empty() {
            fm.theme
        } else {
            cli.theme.clone()
        };
        return serve_single_mode(cli, path, &theme);
    }

    let renderer = Renderer::new();
    let res = renderer.render(&src, path.to_str().unwrap_or(""))?;
    report_warnings(path, &res.warnings);

    // Theme: frontmatter wins over CLI default.
    let theme = if !res.frontmatter.theme.is_empty() {
        res.frontmatter.theme.clone()
    } else {
        cli.theme.clone()
    };

    if cli.print {
        return print_html(&res, &theme);
    }
    if let Some(out) = &cli.output {
        return write_self_contained(out, &res, &theme, cli.no_open);
    }
    oneshot(path, &res, &theme, cli.no_open)
}

fn print_html(res: &render::RenderResult, theme: &str) -> Result<(), BoxedErr> {
    let data = build_page(
        &res.title,
        &res.description,
        res.body.clone(),
        res.toc_html.clone(),
        res.features.has_math,
        res.features.has_mermaid,
        PageOptions {
            theme: theme.to_string(),
            inline: true,
            ..Default::default()
        },
    )?;
    let html = render_page(data)?;
    print!("{html}");
    Ok(())
}

fn write_self_contained(
    out: &Path,
    res: &render::RenderResult,
    theme: &str,
    no_open: bool,
) -> Result<(), BoxedErr> {
    let data = build_page(
        &res.title,
        &res.description,
        res.body.clone(),
        res.toc_html.clone(),
        res.features.has_math,
        res.features.has_mermaid,
        PageOptions {
            theme: theme.to_string(),
            inline: true,
            ..Default::default()
        },
    )?;
    let html = render_page(data)?;
    if let Some(parent) = out.parent()
        && !parent.as_os_str().is_empty()
    {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(out, html)?;
    eprintln!("mt: wrote {}", out.display());
    if !no_open {
        browser::open_file(out)?;
    }
    Ok(())
}

fn oneshot(
    src_file: &Path,
    res: &render::RenderResult,
    theme: &str,
    no_open: bool,
) -> Result<(), BoxedErr> {
    let root = std::env::temp_dir().join("mt");
    let assets_dir = root.join("assets");
    extract_assets_to(&assets_dir)?;
    let base = sanitize_basename(
        src_file
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("page"),
    );
    let out_path = root.join(format!("{base}.html"));

    let data = build_page(
        &res.title,
        &res.description,
        res.body.clone(),
        res.toc_html.clone(),
        res.features.has_math,
        res.features.has_mermaid,
        PageOptions {
            theme: theme.to_string(),
            assets_base: "assets/".into(),
            inline: false,
            ..Default::default()
        },
    )?;
    let html = render_page(data)?;
    std::fs::write(&out_path, html)?;
    eprintln!("mt: rendered → {}", out_path.display());
    if !no_open {
        browser::open_file(&out_path)?;
    }
    Ok(())
}

fn extract_assets_to(dst: &Path) -> std::io::Result<()> {
    assets::extract_to(dst)
}

fn report_warnings(path: &Path, warnings: &[render::RenderWarning]) {
    for w in warnings {
        eprintln!("mt: warning: {}: {w}", path.display());
    }
}

fn sanitize_basename(name: &str) -> String {
    // Match Go's `filepath.Ext`: a leading dot is part of the extension, so
    // a hidden filename like ".md" has an empty stem.
    let stem = match name.rfind('.') {
        Some(0) => "",
        Some(idx) => &name[..idx],
        None => name,
    };
    let mut out = String::with_capacity(stem.len());
    for c in stem.chars() {
        match c {
            'a'..='z' | 'A'..='Z' | '0'..='9' | '-' | '_' => out.push(c),
            _ => out.push('-'),
        }
    }
    if out.is_empty() { "page".into() } else { out }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sanitize_basename_examples() {
        assert_eq!(sanitize_basename("foo.md"), "foo");
        assert_eq!(sanitize_basename("path.md"), "path");
        assert_eq!(sanitize_basename("中文 docs.md"), "---docs");
        assert_eq!(sanitize_basename(".md"), "page");
    }

    #[test]
    fn print_writes_self_contained_html() {
        let dir = tempdir();
        let md = dir.join("hello.md");
        std::fs::write(&md, "# Hi\n\nHello **world**.\n").unwrap();
        let cli = cli::Cli {
            output: None,
            serve: false,
            port: 7331,
            no_open: true,
            theme: "auto".into(),
            print: true,
            all: false,
            version: false,
            target: Some(md.to_string_lossy().into_owned()),
        };
        // Capture stdout via a child process would be cleaner, but for the
        // smoke test we just run the dispatcher and observe absence of panic.
        // (assert_cmd integration test exercises stdout end-to-end.)
        let code = run_with(cli);
        assert_eq!(format!("{code:?}"), "ExitCode(unix_exit_status(0))");
    }

    #[test]
    fn output_mode_writes_self_contained() {
        let dir = tempdir();
        let md = dir.join("doc.md");
        let out = dir.join("doc.html");
        std::fs::write(&md, "# Title\nbody").unwrap();
        let cli = cli::Cli {
            output: Some(out.clone()),
            serve: false,
            port: 7331,
            no_open: true,
            theme: "auto".into(),
            print: false,
            all: false,
            version: false,
            target: Some(md.to_string_lossy().into_owned()),
        };
        let code = run_with(cli);
        assert_eq!(format!("{code:?}"), "ExitCode(unix_exit_status(0))");
        let raw = std::fs::read_to_string(&out).unwrap();
        assert!(raw.contains("<style>"), "inline style missing");
        assert!(raw.contains("<title>Title</title>"));
    }

    #[test]
    fn oneshot_writes_to_tmpdir() {
        let dir = tempdir();
        let md = dir.join("page.md");
        std::fs::write(&md, "# Page\n\nbody.\n").unwrap();
        // Redirect $TMPDIR to a scratch directory so we don't clobber the real one.
        let tmp_root = tempdir();
        // SAFETY: tests are single-threaded by default for this module + $TMPDIR
        // is only consulted by std::env::temp_dir at call time.
        unsafe {
            std::env::set_var("TMPDIR", &tmp_root);
        }
        let cli = cli::Cli {
            output: None,
            serve: false,
            port: 7331,
            no_open: true,
            theme: "auto".into(),
            print: false,
            all: false,
            version: false,
            target: Some(md.to_string_lossy().into_owned()),
        };
        let code = run_with(cli);
        assert_eq!(format!("{code:?}"), "ExitCode(unix_exit_status(0))");
        let out = tmp_root.join("mt").join("page.html");
        assert!(out.exists(), "expected oneshot output at {}", out.display());
        for name in ["style.css", "app.js"] {
            assert!(
                tmp_root.join("mt").join("assets").join(name).exists(),
                "missing asset {name}"
            );
        }
    }

    // ---- tiny tempdir helper to avoid pulling in tempfile as a dev-dep ----
    fn tempdir() -> PathBuf {
        let mut p = std::env::temp_dir();
        let pid = std::process::id();
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.subsec_nanos())
            .unwrap_or(0);
        p.push(format!("mt-rs-test-{pid}-{nanos}"));
        std::fs::create_dir_all(&p).unwrap();
        p
    }
}
