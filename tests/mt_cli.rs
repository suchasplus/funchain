//! End-to-end CLI tests for the `mt-rs` binary. Exercises the three single-file
//! output modes (--print, -o, default one-shot) without spawning a real browser.

use assert_cmd::Command;
use predicates::str::contains;
use std::fs;
use std::path::PathBuf;

fn tempdir(label: &str) -> PathBuf {
    let pid = std::process::id();
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.subsec_nanos())
        .unwrap_or(0);
    let p = std::env::temp_dir().join(format!("mt-rs-it-{label}-{pid}-{nanos}"));
    fs::create_dir_all(&p).unwrap();
    p
}

#[test]
fn mt_rs_version_prints_metadata() {
    Command::cargo_bin("mt-rs")
        .unwrap()
        .arg("--version")
        .assert()
        .success()
        .stdout(contains("0.1.0"));
}

#[test]
fn mt_rs_help_lists_flags() {
    Command::cargo_bin("mt-rs")
        .unwrap()
        .arg("--help")
        .assert()
        .success()
        .stdout(contains("--theme"))
        .stdout(contains("--serve"))
        .stdout(contains("--no-open"));
}

#[test]
fn mt_rs_print_outputs_html() {
    let dir = tempdir("print");
    let md = dir.join("hi.md");
    fs::write(&md, "# Hi\n\nbody **bold**.\n").unwrap();

    Command::cargo_bin("mt-rs")
        .unwrap()
        .args(["--print", "--no-open"])
        .arg(&md)
        .assert()
        .success()
        .stdout(contains("<!doctype html>"))
        .stdout(contains("<title>Hi</title>"))
        .stdout(contains("<strong>bold</strong>"));
}

#[test]
fn mt_rs_output_writes_self_contained() {
    let dir = tempdir("out");
    let md = dir.join("doc.md");
    let html = dir.join("doc.html");
    fs::write(&md, "# Title\n\nx\n").unwrap();

    Command::cargo_bin("mt-rs")
        .unwrap()
        .args(["-o"])
        .arg(&html)
        .args(["--no-open"])
        .arg(&md)
        .assert()
        .success();

    let raw = fs::read_to_string(&html).unwrap();
    assert!(raw.contains("<style>"), "self-contained must inline CSS");
    assert!(raw.contains("<title>Title</title>"));
}

#[test]
fn mt_rs_oneshot_uses_tmpdir() {
    let dir = tempdir("oneshot");
    let md = dir.join("page.md");
    let scratch = tempdir("oneshot-tmp");
    fs::write(&md, "# Page\nbody\n").unwrap();

    Command::cargo_bin("mt-rs")
        .unwrap()
        .env("TMPDIR", &scratch)
        .arg("--no-open")
        .arg(&md)
        .assert()
        .success();

    let out = scratch.join("mt").join("page.html");
    assert!(out.exists(), "expected oneshot output at {}", out.display());
    let html = fs::read_to_string(&out).unwrap();
    assert!(html.contains("<title>Page</title>"));
    assert!(html.contains(r#"href="assets/style.css""#) || html.contains("style.css"));
}

#[test]
fn mt_rs_missing_file_fails_gracefully() {
    Command::cargo_bin("mt-rs")
        .unwrap()
        .args(["--no-open", "/nonexistent/path/to/file.md"])
        .assert()
        .failure()
        .stderr(contains("input"));
}

#[test]
fn mt_rs_dir_mode_renders_site() {
    let dir = tempdir("dir-src");
    let scratch = tempdir("dir-tmp");
    fs::create_dir_all(dir.join("guide")).unwrap();
    fs::write(dir.join("README.md"), "# Home\nlink to [[Intro]]\n").unwrap();
    fs::write(dir.join("guide/intro.md"), "# Intro\nbody\n").unwrap();
    fs::write(dir.join("guide/api.md"), "# API\nstuff\n").unwrap();

    Command::cargo_bin("mt-rs")
        .unwrap()
        .env("TMPDIR", &scratch)
        .arg("--no-open")
        .arg(&dir)
        .assert()
        .success();

    // Sanitised slug is the basename of `dir`. tempdir() uses a known pattern.
    let slug = dir
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap()
        .to_string();
    let out_root = scratch.join("mt").join(&slug);
    assert!(out_root.join("README.html").exists(), "README missing");
    assert!(out_root.join("guide/intro.html").exists(), "intro missing");
    assert!(out_root.join("guide/api.html").exists(), "api missing");
    assert!(out_root.join("assets/style.css").exists(), "assets missing");

    let home = fs::read_to_string(out_root.join("README.html")).unwrap();
    assert!(
        home.contains(r#"href="guide/intro.html""#),
        "wikilink not resolved across dirs: {home}"
    );

    // Pager between intro and api at the same depth → next href = api.html
    let intro = fs::read_to_string(out_root.join("guide/intro.html")).unwrap();
    assert!(
        intro.contains(r#"href="api.html""#),
        "next link missing in intro: {intro}"
    );
}

#[test]
fn mt_rs_print_wins_over_serve_flag() {
    // Mirror the Go side: `--print` short-circuits even when `--serve` is also
    // requested, so the test ends quickly and never starts a listening socket.
    let dir = tempdir("priority");
    let md = dir.join("doc.md");
    fs::write(&md, "# Doc\nbody\n").unwrap();

    Command::cargo_bin("mt-rs")
        .unwrap()
        .args(["--print", "--serve", "--no-open"])
        .arg(&md)
        .timeout(std::time::Duration::from_secs(3))
        .assert()
        .success()
        .stdout(contains("<title>Doc</title>"));
}

#[test]
fn mt_rs_renders_showcase_md_with_all_features() {
    // Go side's showcase.md exercises every feature: admonitions, mermaid,
    // math, footnotes, deflist, tasklist, wikilinks, frontmatter.
    let showcase = std::path::Path::new("../testdata/showcase.md");
    if !showcase.exists() {
        eprintln!("skipping — ../testdata/showcase.md not available in this checkout");
        return;
    }
    let out = Command::cargo_bin("mt-rs")
        .unwrap()
        .args(["--print", "--no-open"])
        .arg(showcase)
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let html = String::from_utf8(out).unwrap();

    // Frontmatter title (set explicitly to "mt Showcase").
    assert!(html.contains("<title>mt Showcase</title>"), "title missing");
    // Admonition rewriter ran end-to-end.
    for kind in [
        "admonition-note",
        "admonition-tip",
        "admonition-warning",
        "admonition-danger",
    ] {
        assert!(html.contains(kind), "missing {kind}");
    }
    // Wikilinks resolved.
    assert!(
        html.contains(r#"class="mt-wikilink""#),
        "wikilink class missing"
    );
    // Mermaid fence preserved + language class for client-side init.
    assert!(html.contains("language-mermaid"), "mermaid class missing");
    // MathJax config tag emitted.
    assert!(html.contains("window.MathJax"), "mathjax config missing");
    // Footnote rendered.
    assert!(
        html.contains("footnote") || html.contains("fn-"),
        "footnote missing"
    );
    // Syntax highlighting applied (syn- prefixed spans).
    assert!(html.contains("syn-"), "syntect spans missing");
    // Inline mode emitted CSS.
    assert!(html.contains("<style>"), "no inline style");
}

/// Directory fixture with AI-prompt files mixed in. Returns (src, scratch, xdg).
/// `xdg` is an isolated XDG_CONFIG_HOME so the invoking user's real
/// ~/.config/mt/config.toml can never leak into the test.
fn ai_site_fixture(label: &str) -> (PathBuf, PathBuf, PathBuf) {
    let src = tempdir(&format!("{label}-src"));
    let scratch = tempdir(&format!("{label}-tmp"));
    let xdg = tempdir(&format!("{label}-xdg"));
    fs::create_dir_all(src.join("appendix")).unwrap();
    fs::write(src.join("README.md"), "# Home\n").unwrap();
    fs::write(src.join("CLAUDE.md"), "# AI instructions\n").unwrap();
    fs::write(src.join("AGENTS.md"), "# Agent guide\n").unwrap();
    fs::write(src.join("appendix/measure.md"), "# 测量方案\n").unwrap();
    (src, scratch, xdg)
}

fn out_root(scratch: &PathBuf, src: &PathBuf) -> PathBuf {
    let slug = src.file_name().and_then(|s| s.to_str()).unwrap().to_string();
    scratch.join("mt").join(slug)
}

#[test]
fn mt_rs_dir_mode_hides_ai_prompt_files_by_default() {
    let (src, scratch, xdg) = ai_site_fixture("ai-default");
    Command::cargo_bin("mt-rs")
        .unwrap()
        .env("TMPDIR", &scratch)
        .env("XDG_CONFIG_HOME", &xdg)
        .arg("--no-open")
        .arg(&src)
        .assert()
        .success();

    let out = out_root(&scratch, &src);
    assert!(out.join("README.html").exists(), "README missing");
    assert!(out.join("appendix/measure.html").exists(), "appendix missing");
    assert!(
        !out.join("CLAUDE.html").exists(),
        "CLAUDE.md should be excluded by default"
    );
    assert!(
        !out.join("AGENTS.html").exists(),
        "AGENTS.md should be excluded by default"
    );

    let home = fs::read_to_string(out.join("README.html")).unwrap();
    assert!(!home.contains("CLAUDE"), "nav still lists CLAUDE: {home}");
    // Nav shows source filenames as small text by default.
    assert!(
        home.contains(r#"<span class="mt-nav__file">measure.md</span>"#),
        "nav filename span missing: {home}"
    );
}

#[test]
fn mt_rs_all_flag_includes_excluded_files() {
    let (src, scratch, xdg) = ai_site_fixture("ai-all");
    Command::cargo_bin("mt-rs")
        .unwrap()
        .env("TMPDIR", &scratch)
        .env("XDG_CONFIG_HOME", &xdg)
        .args(["--no-open", "--all"])
        .arg(&src)
        .assert()
        .success();

    let out = out_root(&scratch, &src);
    assert!(
        out.join("CLAUDE.html").exists(),
        "--all must include CLAUDE.md"
    );
    assert!(
        out.join("AGENTS.html").exists(),
        "--all must include AGENTS.md"
    );
}

#[test]
fn mt_rs_config_toml_overrides_defaults() {
    let (src, scratch, xdg) = ai_site_fixture("ai-cfg");
    fs::write(src.join("secret.md"), "# internal\n").unwrap();
    fs::create_dir_all(xdg.join("mt")).unwrap();
    fs::write(
        xdg.join("mt/config.toml"),
        "[site]\nexclude = [\"secret.md\"]\nnav_filenames = false\n",
    )
    .unwrap();

    Command::cargo_bin("mt-rs")
        .unwrap()
        .env("TMPDIR", &scratch)
        .env("XDG_CONFIG_HOME", &xdg)
        .arg("--no-open")
        .arg(&src)
        .assert()
        .success();

    let out = out_root(&scratch, &src);
    // Custom list replaces the default one entirely.
    assert!(
        out.join("CLAUDE.html").exists(),
        "custom exclude list should replace the default (CLAUDE.md kept)"
    );
    assert!(
        !out.join("secret.html").exists(),
        "secret.md should be excluded by config"
    );
    // nav_filenames = false disables the small-text filename line.
    let home = fs::read_to_string(out.join("README.html")).unwrap();
    assert!(
        !home.contains("mt-nav__file"),
        "nav filenames should be off: {home}"
    );
}

fn zip_names(path: &std::path::Path) -> Vec<String> {
    let f = fs::File::open(path).unwrap();
    let mut ar = zip::ZipArchive::new(f).unwrap();
    let mut names: Vec<String> = (0..ar.len())
        .map(|i| ar.by_index(i).unwrap().name().to_string())
        .collect();
    names.sort();
    names
}

#[test]
fn mt_rs_archive_dir_packs_html_assets_and_md() {
    let (src, scratch, xdg) = ai_site_fixture("ar-dir");
    let out = scratch.join("dist/docs.zip");
    Command::cargo_bin("mt-rs")
        .unwrap()
        .env("TMPDIR", &scratch)
        .env("XDG_CONFIG_HOME", &xdg)
        .args(["--no-open", "--archive"])
        .arg(&out)
        .arg(&src)
        .assert()
        .success()
        .stderr(contains("archived"));

    let names = zip_names(&out);
    let slug = src.file_name().and_then(|s| s.to_str()).unwrap().to_string();
    for want in [
        format!("{slug}/README.html"),
        format!("{slug}/README.md"),
        format!("{slug}/appendix/measure.html"),
        format!("{slug}/appendix/measure.md"),
        format!("{slug}/assets/style.css"),
    ] {
        assert!(names.contains(&want), "missing {want} in {names:?}");
    }
    // Config's AI excludes still apply inside archives.
    assert!(
        !names.iter().any(|n| n.contains("CLAUDE")),
        "CLAUDE leaked into archive: {names:?}"
    );
}

#[test]
fn mt_rs_archive_no_md_drops_sources() {
    let (src, scratch, xdg) = ai_site_fixture("ar-nomd");
    let out = scratch.join("docs.zip");
    Command::cargo_bin("mt-rs")
        .unwrap()
        .env("TMPDIR", &scratch)
        .env("XDG_CONFIG_HOME", &xdg)
        .args(["--no-open", "--no-md", "--archive"])
        .arg(&out)
        .arg(&src)
        .assert()
        .success();
    let names = zip_names(&out);
    assert!(
        !names.iter().any(|n| n.ends_with(".md")),
        "md sources leaked despite --no-md: {names:?}"
    );
    assert!(names.iter().any(|n| n.ends_with("README.html")));
}

#[test]
fn mt_rs_archive_include_exclude_filter_content_and_nav() {
    let (src, scratch, xdg) = ai_site_fixture("ar-filter");
    fs::write(src.join("appendix/wip-notes.md"), "# WIP\n").unwrap();
    let out = scratch.join("docs.zip");
    Command::cargo_bin("mt-rs")
        .unwrap()
        .env("TMPDIR", &scratch)
        .env("XDG_CONFIG_HOME", &xdg)
        .args([
            "--no-open",
            "--include",
            "README.md",
            "--include",
            "appendix/**",
            "--exclude",
            "**/wip-*.md",
            "--archive",
        ])
        .arg(&out)
        .arg(&src)
        .assert()
        .success();

    let names = zip_names(&out);
    let slug = src.file_name().and_then(|s| s.to_str()).unwrap().to_string();
    assert!(names.contains(&format!("{slug}/README.html")));
    assert!(names.contains(&format!("{slug}/appendix/measure.html")));
    assert!(
        !names.iter().any(|n| n.contains("wip-notes")),
        "--exclude ignored: {names:?}"
    );
    // A prior unfiltered build into the same TMPDIR must not leak stale
    // pages into a filtered archive — the archive build uses a fresh dir.
    Command::cargo_bin("mt-rs")
        .unwrap()
        .env("TMPDIR", &scratch)
        .env("XDG_CONFIG_HOME", &xdg)
        .arg("--no-open")
        .arg(&src)
        .assert()
        .success(); // renders EVERYTHING into $TMPDIR/mt/<slug>
    let out2 = scratch.join("filtered2.zip");
    Command::cargo_bin("mt-rs")
        .unwrap()
        .env("TMPDIR", &scratch)
        .env("XDG_CONFIG_HOME", &xdg)
        .args(["--no-open", "--include", "appendix/**", "--archive"])
        .arg(&out2)
        .arg(&src)
        .assert()
        .success();
    let names2 = zip_names(&out2);
    assert!(
        !names2.iter().any(|n| n.ends_with("README.html")),
        "stale unfiltered page leaked into filtered archive: {names2:?}"
    );
    assert!(names2.contains(&format!("{slug}/appendix/measure.html")));

    // Nav inside packed pages matches the packed set — no dead links.
    let f = fs::File::open(&out).unwrap();
    let mut ar = zip::ZipArchive::new(f).unwrap();
    let mut html = String::new();
    use std::io::Read as _;
    ar.by_name(&format!("{slug}/README.html"))
        .unwrap()
        .read_to_string(&mut html)
        .unwrap();
    assert!(
        !html.contains("wip-notes"),
        "nav still links excluded page: {html}"
    );
}

#[test]
fn mt_rs_archive_single_file_packs_self_contained_html_and_md() {
    let dir = tempdir("ar-single");
    let md = dir.join("方案.md");
    fs::write(&md, "# 方案\nbody\n").unwrap();
    let out = dir.join("方案.zip");
    Command::cargo_bin("mt-rs")
        .unwrap()
        .args(["--no-open", "--archive"])
        .arg(&out)
        .arg(&md)
        .assert()
        .success();

    let names = zip_names(&out);
    assert!(
        names.iter().any(|n| n.ends_with(".html")),
        "html missing: {names:?}"
    );
    assert!(
        names.iter().any(|n| n.ends_with("方案.md")),
        "md missing: {names:?}"
    );
    // Self-contained: inlined stylesheet, no external assets/ dir.
    let f = fs::File::open(&out).unwrap();
    let mut ar = zip::ZipArchive::new(f).unwrap();
    let html_name = zip_names(&out)
        .into_iter()
        .find(|n| n.ends_with(".html"))
        .unwrap();
    let mut html = String::new();
    use std::io::Read as _;
    ar.by_name(&html_name)
        .unwrap()
        .read_to_string(&mut html)
        .unwrap();
    assert!(html.contains("<style>"), "not self-contained");
    assert!(
        !names.iter().any(|n| n.contains("assets/")),
        "single-file archive should not carry assets dir: {names:?}"
    );
}

#[test]
fn mt_rs_archive_conflicts_with_print() {
    let dir = tempdir("ar-conflict");
    let md = dir.join("a.md");
    fs::write(&md, "# a\n").unwrap();
    Command::cargo_bin("mt-rs")
        .unwrap()
        .args(["--print", "--no-open", "--archive"])
        .arg(dir.join("x.zip"))
        .arg(&md)
        .assert()
        .failure()
        .stderr(contains("--archive"));
}

#[test]
fn mt_rs_dir_mode_rejects_print() {
    let dir = tempdir("dir-reject");
    fs::write(dir.join("a.md"), "# a").unwrap();
    Command::cargo_bin("mt-rs")
        .unwrap()
        .args(["--print", "--no-open"])
        .arg(&dir)
        .assert()
        .failure()
        .stderr(contains("--print not supported in directory mode"));
}
