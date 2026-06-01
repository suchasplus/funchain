//! Single-file dev server. Watches the parent directory (to survive editor
//! swap-writes), re-renders on changes, broadcasts reload pulses.

use std::ffi::OsString;
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::sync::{Arc, RwLock};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use crossbeam_channel::{Receiver, Sender, after, select, unbounded};
use notify::{RecursiveMode, Watcher};
use tiny_http::{Header, Response, Server};

use super::{Hub, LIVE_RELOAD_JS, ServeError, WarningSink, is_ws_upgrade, spawn_ws_worker};
use crate::mt::assets;
use crate::mt::assets::page::{PageOptions, build_page, render as render_page};
use crate::mt::render::Renderer;

const DEBOUNCE: Duration = Duration::from_millis(120);

#[derive(Clone)]
pub struct SingleConfig {
    pub file: PathBuf,
    pub port: u16,
    pub theme: String,
    /// Optional sink for non-fatal warnings produced by the render pipeline.
    /// `None` drops them silently (typical for tests); the CLI installs an
    /// `eprintln!` closure so user-facing builds still see diagnostics.
    pub on_warning: Option<WarningSink>,
}

impl std::fmt::Debug for SingleConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SingleConfig")
            .field("file", &self.file)
            .field("port", &self.port)
            .field("theme", &self.theme)
            .field("on_warning", &self.on_warning.as_ref().map(|_| "<sink>"))
            .finish()
    }
}

pub struct SingleServer {
    server: Arc<Server>,
    addr: SocketAddr,
    hub: Arc<Hub>,
    shutdown_tx: Sender<()>,
    workers: Vec<JoinHandle<()>>,
}

impl SingleServer {
    pub fn addr(&self) -> SocketAddr {
        self.addr
    }
    pub fn url(&self) -> String {
        format!("http://{}/", self.addr)
    }
    pub fn hub(&self) -> Arc<Hub> {
        self.hub.clone()
    }
    pub fn join(self) {
        // No external shutdown signal: the server runs until the binary process exits.
        for w in self.workers {
            let _ = w.join();
        }
    }
    pub fn shutdown(self) {
        let _ = self.shutdown_tx.send(());
        self.server.unblock();
        for w in self.workers {
            let _ = w.join();
        }
    }
}

/// Build the initial page, bind the listen socket, and spawn the HTTP +
/// watcher worker threads. Returns once both threads are running.
pub fn serve_single(cfg: SingleConfig) -> Result<SingleServer, ServeError> {
    let file = cfg
        .file
        .canonicalize()
        .or_else(|_| Ok::<_, std::io::Error>(cfg.file.clone()))?;
    // Build the syntax-highlighting tables once and share them across the
    // initial render and every subsequent watcher-driven re-render.
    let renderer = Arc::new(Renderer::new());
    let body = Arc::new(RwLock::new(render_once(
        &file,
        &cfg.theme,
        &renderer,
        cfg.on_warning.as_ref(),
    )?));

    let server = Server::http(format!("127.0.0.1:{}", cfg.port))
        .map_err(|e| ServeError::Other(e.to_string()))?;
    let addr = server
        .server_addr()
        .to_ip()
        .ok_or_else(|| ServeError::Other("non-ip listen address".into()))?;
    let server = Arc::new(server);
    let hub = Arc::new(Hub::new());
    let (shutdown_tx, shutdown_rx) = unbounded::<()>();

    let http_worker = {
        let server = server.clone();
        let body = body.clone();
        let hub = hub.clone();
        std::thread::Builder::new()
            .name("mt-rs-http".into())
            .spawn(move || http_loop(server, body, hub))
            .map_err(|e| ServeError::Other(format!("spawn http: {e}")))?
    };
    // Build the notify watcher synchronously *before* spawning the loop, so
    // serve_single only returns once the parent dir is actually being observed.
    // Otherwise tests (and fast manual edits) race with the watcher thread.
    let parent = file
        .parent()
        .ok_or_else(|| ServeError::Other("file has no parent directory".into()))?
        .to_path_buf();
    let target_name: OsString = file
        .file_name()
        .map(|n| n.to_os_string())
        .ok_or_else(|| ServeError::Other("file has no name".into()))?;
    let (notify_tx, notify_rx) = unbounded::<notify::Result<notify::Event>>();
    let mut notify_watcher: notify::RecommendedWatcher = notify::recommended_watcher(move |res| {
        let _ = notify_tx.send(res);
    })
    .map_err(|e| ServeError::Other(format!("watcher: {e}")))?;
    notify_watcher
        .watch(&parent, RecursiveMode::NonRecursive)
        .map_err(|e| ServeError::Other(format!("watch {}: {e}", parent.display())))?;

    let watcher_worker = {
        let body = body.clone();
        let hub = hub.clone();
        let theme = cfg.theme.clone();
        let renderer = renderer.clone();
        let file = file.clone();
        let on_warning = cfg.on_warning.clone();
        std::thread::Builder::new()
            .name("mt-rs-watcher".into())
            .spawn(move || {
                watcher_loop(
                    file,
                    theme,
                    body,
                    hub,
                    shutdown_rx,
                    renderer,
                    notify_rx,
                    notify_watcher,
                    target_name,
                    on_warning,
                )
            })
            .map_err(|e| ServeError::Other(format!("spawn watcher: {e}")))?
    };

    Ok(SingleServer {
        server,
        addr,
        hub,
        shutdown_tx,
        workers: vec![http_worker, watcher_worker],
    })
}

fn render_once(
    file: &Path,
    theme: &str,
    renderer: &Renderer,
    on_warning: Option<&WarningSink>,
) -> Result<Vec<u8>, ServeError> {
    let src = std::fs::read_to_string(file)?;
    let res = renderer.render(&src, file.to_str().unwrap_or(""))?;
    if let Some(sink) = on_warning {
        for w in &res.warnings {
            sink(file, w);
        }
    }
    let final_theme = if !res.frontmatter.theme.is_empty() {
        res.frontmatter.theme.clone()
    } else {
        theme.to_string()
    };
    let page = build_page(
        &res.title,
        &res.description,
        res.body,
        res.toc_html,
        res.features.has_math,
        res.features.has_mermaid,
        PageOptions {
            theme: final_theme,
            assets_base: "/assets/".into(),
            inline: false,
            live_reload_js: LIVE_RELOAD_JS.into(),
        },
    )?;
    Ok(render_page(page)?.into_bytes())
}

fn http_loop(server: Arc<Server>, body: Arc<RwLock<Vec<u8>>>, hub: Arc<Hub>) {
    for req in server.incoming_requests() {
        if req.url() == "/__livereload" && is_ws_upgrade(&req) {
            spawn_ws_worker(req, hub.clone());
            continue;
        }
        handle_http(req, &body);
    }
}

fn handle_http(req: tiny_http::Request, body: &RwLock<Vec<u8>>) {
    let url = req.url();
    // Strip any query string when matching paths.
    let path = url.split('?').next().unwrap_or(url);
    if let Some(rest) = path.strip_prefix("/assets/") {
        if let Some(bytes) = assets::read_static(rest) {
            let resp =
                Response::from_data(bytes.to_vec()).with_header(content_type(guess_mime(rest)));
            let _ = req.respond(resp);
            return;
        }
        let _ = req.respond(Response::empty(404));
        return;
    }
    if path == "/" || path.is_empty() {
        let html = body.read().expect("body poisoned").clone();
        let resp = Response::from_data(html)
            .with_header(content_type("text/html; charset=utf-8"))
            .with_header(Header::from_bytes(&b"Cache-Control"[..], &b"no-store"[..]).unwrap());
        let _ = req.respond(resp);
        return;
    }
    let _ = req.respond(Response::empty(404));
}

fn content_type(value: &str) -> Header {
    Header::from_bytes(&b"Content-Type"[..], value.as_bytes()).unwrap()
}

#[allow(clippy::too_many_arguments)]
fn watcher_loop(
    file: PathBuf,
    theme: String,
    body: Arc<RwLock<Vec<u8>>>,
    hub: Arc<Hub>,
    shutdown: Receiver<()>,
    renderer: Arc<Renderer>,
    rx: Receiver<notify::Result<notify::Event>>,
    // Watcher kept alive for the lifetime of this thread; dropping it stops events.
    _watcher: notify::RecommendedWatcher,
    target_name: OsString,
    on_warning: Option<WarningSink>,
) {
    loop {
        // Wait for either a notify event or a shutdown signal.
        let first = select! {
            recv(rx) -> ev => match ev {
                Ok(Ok(e)) => e,
                Ok(Err(_)) => continue,
                Err(_) => return,
            },
            recv(shutdown) -> _ => return,
        };
        if !relevant(&first, &target_name) {
            continue;
        }
        // Debounce: keep extending the deadline as long as more relevant events arrive.
        let mut deadline = Instant::now() + DEBOUNCE;
        loop {
            let remaining = deadline.saturating_duration_since(Instant::now());
            let timeout = after(remaining);
            select! {
                recv(rx) -> ev => match ev {
                    Ok(Ok(e)) if relevant(&e, &target_name) => {
                        deadline = Instant::now() + DEBOUNCE;
                    }
                    Ok(_) => {}
                    Err(_) => return,
                },
                recv(timeout) -> _ => break,
                recv(shutdown) -> _ => return,
            }
        }
        if let Ok(new_body) = render_once(&file, &theme, &renderer, on_warning.as_ref()) {
            *body.write().expect("body poisoned") = new_body;
            hub.broadcast();
        }
    }
}

fn relevant(ev: &notify::Event, target_name: &OsString) -> bool {
    if !ev
        .paths
        .iter()
        .any(|p| p.file_name() == Some(target_name.as_os_str()))
    {
        return false;
    }
    matches!(
        ev.kind,
        notify::EventKind::Modify(_) | notify::EventKind::Create(_) | notify::EventKind::Remove(_)
    )
}

fn guess_mime(path: &str) -> &'static str {
    match path.rsplit('.').next().unwrap_or("") {
        "css" => "text/css; charset=utf-8",
        "js" => "application/javascript; charset=utf-8",
        "html" => "text/html; charset=utf-8",
        "woff2" => "font/woff2",
        "woff" => "font/woff",
        "json" => "application/json",
        "png" => "image/png",
        "svg" => "image/svg+xml",
        _ => "application/octet-stream",
    }
}
