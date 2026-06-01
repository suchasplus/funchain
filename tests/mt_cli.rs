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
