//! Integration tests for the dev server (`--serve` mode).
//!
//! These spin up real listening sockets on 127.0.0.1, talk to them via raw TCP
//! for HTTP and the `tungstenite` client for WebSocket. All servers bind to
//! port 0 so multiple tests can run in parallel without colliding.

use std::fs;
use std::io::{Read, Write};
use std::net::{SocketAddr, TcpStream};
use std::path::PathBuf;
use std::time::Duration;

use funchain::mt::serve::{DirConfig, SingleConfig, serve_dir, serve_single};
use tungstenite::Message;
use tungstenite::client::connect;
use tungstenite::stream::MaybeTlsStream;

/// Reads websocket frames until we see a `reload` text message, ignoring
/// pings/pongs. Sets a short socket-level read timeout so we can poll without
/// hanging forever. Panics if `total_timeout` elapses without a `reload`.
fn await_reload(
    ws: &mut tungstenite::WebSocket<MaybeTlsStream<TcpStream>>,
    total_timeout: Duration,
) {
    if let MaybeTlsStream::Plain(s) = ws.get_mut() {
        let _ = s.set_read_timeout(Some(Duration::from_millis(200)));
    }
    let deadline = std::time::Instant::now() + total_timeout;
    while std::time::Instant::now() < deadline {
        match ws.read() {
            Ok(Message::Text(t)) if t.as_str() == "reload" => return,
            Ok(Message::Text(t)) => panic!("unexpected text frame: {t}"),
            Ok(Message::Ping(_)) | Ok(Message::Pong(_)) => continue,
            Ok(other) => panic!("unexpected frame: {other:?}"),
            // WouldBlock / Timeout → keep polling
            Err(tungstenite::Error::Io(e))
                if e.kind() == std::io::ErrorKind::WouldBlock
                    || e.kind() == std::io::ErrorKind::TimedOut => {}
            Err(e) => panic!("ws read error: {e}"),
        }
    }
    panic!("did not receive `reload` within {total_timeout:?}");
}

// --------- helpers ---------

fn tempdir(label: &str) -> PathBuf {
    let pid = std::process::id();
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.subsec_nanos())
        .unwrap_or(0);
    let p = std::env::temp_dir().join(format!("mt-rs-serve-{label}-{pid}-{nanos}"));
    fs::create_dir_all(&p).unwrap();
    p
}

fn http_get(addr: SocketAddr, path: &str) -> (u16, String, String) {
    let mut sock = TcpStream::connect(addr).unwrap();
    sock.set_read_timeout(Some(Duration::from_secs(4))).unwrap();
    write!(sock, "GET {path} HTTP/1.0\r\nHost: localhost\r\n\r\n").unwrap();
    sock.flush().unwrap();
    let mut buf = String::new();
    sock.read_to_string(&mut buf).unwrap();
    let (head, body) = buf
        .split_once("\r\n\r\n")
        .map(|(h, b)| (h.to_string(), b.to_string()))
        .unwrap_or((buf.clone(), String::new()));
    let status: u16 = head
        .lines()
        .next()
        .unwrap_or("HTTP/1.0 0 ?")
        .split_whitespace()
        .nth(1)
        .and_then(|s| s.parse().ok())
        .unwrap_or(0);
    (status, head, body)
}

// --------- single-file mode ---------

#[test]
fn single_serves_page_and_assets() {
    let dir = tempdir("single-http");
    let md = dir.join("doc.md");
    fs::write(&md, "# Hello\nworld\n").unwrap();
    let server = serve_single(SingleConfig {
        file: md,
        port: 0,
        theme: "auto".into(),
        on_warning: None,
    })
    .unwrap();
    let addr = server.addr();

    let (status, _head, body) = http_get(addr, "/");
    assert_eq!(status, 200, "GET / status");
    assert!(body.contains("<title>Hello</title>"), "title missing");
    assert!(body.contains("/__livereload"), "live-reload JS missing");

    let (status, _head, css) = http_get(addr, "/assets/style.css");
    assert_eq!(status, 200);
    assert!(
        css.contains(":root") || css.contains("--mt-bg"),
        "css missing tokens"
    );

    server.shutdown();
}

#[test]
fn single_404_for_unknown_path() {
    let dir = tempdir("single-404");
    let md = dir.join("x.md");
    fs::write(&md, "# x").unwrap();
    let server = serve_single(SingleConfig {
        file: md,
        port: 0,
        theme: "auto".into(),
        on_warning: None,
    })
    .unwrap();

    let (status, _, _) = http_get(server.addr(), "/nope");
    assert_eq!(status, 404);
    server.shutdown();
}

#[test]
fn single_ws_reload_on_edit() {
    let dir = tempdir("single-ws");
    let md = dir.join("page.md");
    fs::write(&md, "# v1\n").unwrap();
    let server = serve_single(SingleConfig {
        file: md.clone(),
        port: 0,
        theme: "auto".into(),
        on_warning: None,
    })
    .unwrap();

    let url = format!("ws://{}/__livereload", server.addr());
    let (mut ws, _resp) = connect(&url).expect("ws connect");

    fs::write(&md, "# v2 edited\n").unwrap();

    await_reload(&mut ws, Duration::from_secs(6));
    let _ = ws.close(None);
    server.shutdown();
}

// --------- dir mode ---------

#[test]
fn dir_serves_pages_and_landing() {
    let dir = tempdir("dir-http");
    fs::create_dir_all(dir.join("guide")).unwrap();
    fs::write(dir.join("README.md"), "# Home\nlink to [[Intro]]\n").unwrap();
    fs::write(dir.join("guide/intro.md"), "# Intro\nbody\n").unwrap();
    let server = serve_dir(DirConfig {
        root: dir.clone(),
        port: 0,
        theme: "auto".into(),
        on_warning: None,
        exclude: Vec::new(),
        nav_filenames: true,
    })
    .unwrap();
    let addr = server.addr();

    let (status, _head, root_body) = http_get(addr, "/");
    assert_eq!(status, 200);
    assert!(root_body.contains("<title>Home</title>"), "landing wrong");
    assert!(
        root_body.contains(r#"href="guide/intro.html""#),
        "wikilink missing"
    );

    let (status, _head, nested) = http_get(addr, "/guide/intro.html");
    assert_eq!(status, 200);
    assert!(nested.contains("<title>Intro</title>"));

    let (status, _, _) = http_get(addr, "/assets/style.css");
    assert_eq!(status, 200);

    let (status, _, _) = http_get(addr, "/does-not-exist.html");
    assert_eq!(status, 404);
    server.shutdown();
}

#[test]
fn dir_watcher_skips_hidden_and_vendor_dirs() {
    let dir = tempdir("dir-skip");
    fs::create_dir_all(dir.join(".git")).unwrap();
    fs::create_dir_all(dir.join("node_modules/pkg")).unwrap();
    fs::write(dir.join("README.md"), "# r\n").unwrap();
    let server = serve_dir(DirConfig {
        root: dir.clone(),
        port: 0,
        theme: "auto".into(),
        on_warning: None,
        exclude: Vec::new(),
        nav_filenames: true,
    })
    .unwrap();

    let url = format!("ws://{}/__livereload", server.addr());
    let (mut ws, _resp) = connect(&url).expect("ws connect");
    std::thread::sleep(Duration::from_millis(150));

    // Writing a file inside .git or node_modules must NOT trigger reload.
    fs::write(dir.join(".git/HEAD"), "noise\n").unwrap();
    fs::write(dir.join("node_modules/pkg/foo.md"), "# noise\n").unwrap();
    if let tungstenite::stream::MaybeTlsStream::Plain(s) = ws.get_mut() {
        let _ = s.set_read_timeout(Some(Duration::from_millis(600)));
    }
    match ws.read() {
        Ok(Message::Text(t)) => panic!("unexpected reload from skipped path: {t}"),
        // ReadTimeout or any IO error after 600ms means no reload arrived — good.
        Err(_) => {}
        Ok(other) => panic!("unexpected ws message: {other:?}"),
    }
    server.shutdown();
}

#[test]
fn dir_serves_space_and_non_ascii_filenames() {
    // Regression for the percent-encoding boundary: filenames with spaces or
    // non-ASCII characters must be reachable both via the encoded URL the
    // browser sends and (still) via the raw decoded form.
    let dir = tempdir("dir-encoded");
    fs::write(dir.join("My Page.md"), "# Spaces\nbody\n").unwrap();
    fs::write(dir.join("你好.md"), "# Hello in CJK\nbody\n").unwrap();
    fs::write(
        dir.join("README.md"),
        "# Root\nsee [[My Page]] and [[你好]]\n",
    )
    .unwrap();
    let server = serve_dir(DirConfig {
        root: dir.clone(),
        port: 0,
        theme: "auto".into(),
        on_warning: None,
        exclude: Vec::new(),
        nav_filenames: true,
    })
    .unwrap();
    let addr = server.addr();

    // Browsers always send the encoded form — the server must decode it.
    let (status, _, body) = http_get(addr, "/My%20Page.html");
    assert_eq!(status, 200, "GET /My%20Page.html returned {status}");
    assert!(body.contains("<title>Spaces</title>"));

    let (status, _, body) = http_get(addr, "/%E4%BD%A0%E5%A5%BD.html");
    assert_eq!(status, 200, "GET CJK-encoded path returned {status}");
    assert!(body.contains("<title>Hello in CJK</title>"));

    // The landing page must emit encoded hrefs for the nav + wikilinks so a
    // user clicking through actually reaches a 200.
    let (_, _, landing) = http_get(addr, "/");
    assert!(
        landing.contains("My%20Page.html"),
        "landing should reference encoded space filename: {landing}"
    );
    assert!(
        landing.contains("%E4%BD%A0%E5%A5%BD.html"),
        "landing should reference encoded CJK filename: {landing}"
    );
    // No raw space in the href attribute would survive HTML attribute escaping.
    assert!(
        !landing.contains(r#"href="My Page.html""#),
        "landing emitted unencoded space-href: {landing}"
    );
    server.shutdown();
}

#[test]
fn dir_warning_sink_receives_diagnostics() {
    use std::sync::{Arc, Mutex};

    let dir = tempdir("dir-warn-sink");
    // Two duplicate explicit ids → one DuplicateExplicitId warning per render.
    fs::write(dir.join("page.md"), "# A {#dup}\n# B {#dup}\n").unwrap();

    let captured: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
    let captured_for_sink = captured.clone();
    let sink: funchain::mt::serve::WarningSink = Arc::new(move |p, w| {
        captured_for_sink
            .lock()
            .unwrap()
            .push(format!("{}: {w}", p.display()));
    });

    let server = serve_dir(DirConfig {
        root: dir.clone(),
        port: 0,
        theme: "auto".into(),
        on_warning: Some(sink),
        exclude: Vec::new(),
        nav_filenames: true,
    })
    .unwrap();
    // Hit the page so the initial render has definitely completed.
    let _ = http_get(server.addr(), "/page.html");
    let messages = captured.lock().unwrap().clone();
    assert!(
        messages
            .iter()
            .any(|m| m.contains("duplicate explicit heading id `dup`")),
        "sink did not capture duplicate-id warning: {messages:?}"
    );
    server.shutdown();
}

#[test]
fn dir_cache_only_rerenders_modified_files() {
    // Two pages. After the initial build, edit only one. The cache should
    // serve the untouched page from memory (byte-equal across rebuilds) and
    // only re-render the edited one.
    let dir = tempdir("dir-cache");
    fs::write(dir.join("alpha.md"), "# Alpha\nbody\n").unwrap();
    fs::write(dir.join("beta.md"), "# Beta\nbody\n").unwrap();
    let server = serve_dir(DirConfig {
        root: dir.clone(),
        port: 0,
        theme: "auto".into(),
        on_warning: None,
        exclude: Vec::new(),
        nav_filenames: true,
    })
    .unwrap();
    let addr = server.addr();

    let (_, _, alpha_v1) = http_get(addr, "/alpha.html");
    let (_, _, beta_v1) = http_get(addr, "/beta.html");

    // Bump mtime + content on beta only. Wait for the watcher to pick it up.
    let url = format!("ws://{}/__livereload", addr);
    let (mut ws, _resp) = connect(&url).expect("ws connect");
    fs::write(dir.join("beta.md"), "# Beta updated\n").unwrap();
    await_reload(&mut ws, Duration::from_secs(6));

    let (_, _, alpha_v2) = http_get(addr, "/alpha.html");
    let (_, _, beta_v2) = http_get(addr, "/beta.html");
    assert_eq!(alpha_v1, alpha_v2, "untouched page changed across rebuild");
    assert_ne!(beta_v1, beta_v2, "edited page should have changed");
    assert!(
        beta_v2.contains("Beta updated"),
        "beta did not pick up edit"
    );
    let _ = ws.close(None);
    server.shutdown();
}

#[test]
fn dir_ws_reload_on_edit() {
    let dir = tempdir("dir-ws");
    fs::create_dir_all(dir.join("guide")).unwrap();
    fs::write(dir.join("README.md"), "# v1\n").unwrap();
    fs::write(dir.join("guide/intro.md"), "# Intro\nbody\n").unwrap();
    let server = serve_dir(DirConfig {
        root: dir.clone(),
        port: 0,
        theme: "auto".into(),
        on_warning: None,
        exclude: Vec::new(),
        nav_filenames: true,
    })
    .unwrap();

    let url = format!("ws://{}/__livereload", server.addr());
    let (mut ws, _resp) = connect(&url).expect("ws connect");

    fs::write(dir.join("guide/intro.md"), "# Intro v2 edited\n").unwrap();
    await_reload(&mut ws, Duration::from_secs(6));
    let _ = ws.close(None);
    server.shutdown();
}
