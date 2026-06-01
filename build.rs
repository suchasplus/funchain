//! Build metadata emitter — shells out to `git` and `date` so we stay free of
//! version-conflict-prone vergen variants. Read values via env!() in
//! src/version.rs. Falls back to "unknown" when the underlying command fails.

use std::process::Command;

fn main() {
    let sha = git_sha().unwrap_or_else(|| "unknown".into());
    let ts = build_ts().unwrap_or_else(|| "unknown".into());
    let rustc = rustc_version().unwrap_or_else(|| "unknown".into());

    println!("cargo:rustc-env=MT_GIT_SHA={sha}");
    println!("cargo:rustc-env=MT_BUILD_TIMESTAMP={ts}");
    println!("cargo:rustc-env=MT_RUSTC={rustc}");

    // Re-run when these change so version stays fresh.
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=.git/HEAD");
    if let Ok(head) = std::fs::read_to_string(".git/HEAD")
        && let Some(ref_path) = head
            .strip_prefix("ref: ")
            .and_then(|s| s.split_whitespace().next())
    {
        println!("cargo:rerun-if-changed=.git/{ref_path}");
    }
}

fn git_sha() -> Option<String> {
    cmd("git", &["rev-parse", "--short=12", "HEAD"])
}

fn build_ts() -> Option<String> {
    cmd("date", &["-u", "+%Y-%m-%dT%H:%M:%SZ"])
}

fn rustc_version() -> Option<String> {
    let raw = cmd("rustc", &["--version"])?;
    Some(raw.split_whitespace().nth(1).unwrap_or("?").to_string())
}

fn cmd(prog: &str, args: &[&str]) -> Option<String> {
    let out = Command::new(prog).args(args).output().ok()?;
    if !out.status.success() {
        return None;
    }
    let s = String::from_utf8(out.stdout).ok()?.trim().to_string();
    if s.is_empty() { None } else { Some(s) }
}
