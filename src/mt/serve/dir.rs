//! Multi-file dev server. Watches the root directory recursively, re-renders
//! the whole site on any .md change, broadcasts reload pulses.

use std::collections::HashMap;
use std::ffi::OsStr;
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::sync::{Arc, RwLock};
use std::thread::JoinHandle;
use std::time::{Duration, Instant, SystemTime};

use crossbeam_channel::{Receiver, Sender, after, select, unbounded};
use notify::{EventKind, RecursiveMode, Watcher, event::CreateKind};
use tiny_http::{Header, Response, Server};
use walkdir::WalkDir;

use super::{Hub, LIVE_RELOAD_JS, ServeError, WarningSink, is_ws_upgrade, spawn_ws_worker};
use crate::mt::assets;
use crate::mt::render::Renderer;
use crate::mt::site::{BuildOptions, Context, landing_page};

const DEBOUNCE: Duration = Duration::from_millis(120);

#[derive(Clone)]
pub struct DirConfig {
    pub root: PathBuf,
    pub port: u16,
    pub theme: String,
    /// See [`crate::mt::serve::WarningSink`]. `None` drops warnings silently.
    pub on_warning: Option<WarningSink>,
    /// Basenames (case-insensitive) hidden from the scan — the global
    /// config's exclude list unless `--all` cleared it.
    pub exclude: Vec<String>,
    /// Show source filenames as small text in the nav tree.
    pub nav_filenames: bool,
}

impl std::fmt::Debug for DirConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DirConfig")
            .field("root", &self.root)
            .field("port", &self.port)
            .field("theme", &self.theme)
            .field("on_warning", &self.on_warning.as_ref().map(|_| "<sink>"))
            .finish()
    }
}

/// One cached page render — keyed by source absolute path. We invalidate when
/// either the on-disk mtime or size changes; that catches edits even when
/// the editor swap-writes (since the inode metadata still changes), and
/// avoids hashing every byte on each rebuild.
#[derive(Clone)]
struct CacheEntry {
    mtime: SystemTime,
    size: u64,
    html: Vec<u8>,
}

#[derive(Default)]
struct DirState {
    pages: HashMap<String, Vec<u8>>,
    landing: String,
    /// Per-source-file cache. Survives every rebuild — the next rebuild
    /// only re-renders files whose mtime/size changed.
    cache: HashMap<PathBuf, CacheEntry>,
}

pub struct DirServer {
    server: Arc<Server>,
    addr: SocketAddr,
    hub: Arc<Hub>,
    state: Arc<RwLock<DirState>>,
    shutdown_tx: Sender<()>,
    workers: Vec<JoinHandle<()>>,
}

impl DirServer {
    pub fn addr(&self) -> SocketAddr {
        self.addr
    }
    /// URL to the landing page (`http://host:port/<landing>`).
    pub fn landing_url(&self) -> String {
        let landing = self.state.read().expect("state poisoned").landing.clone();
        format!("http://{}/{}", self.addr, landing)
    }
    pub fn hub(&self) -> Arc<Hub> {
        self.hub.clone()
    }
    pub fn join(self) {
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

pub fn serve_dir(cfg: DirConfig) -> Result<DirServer, ServeError> {
    let state = Arc::new(RwLock::new(DirState::default()));
    // Reused across the initial build and every subsequent watcher-driven rebuild
    // so syntect's bundled syntaxes / themes load exactly once.
    let renderer = Arc::new(Renderer::new());
    rebuild(&cfg, &state, &renderer)?;

    let server = Server::http(format!("127.0.0.1:{}", cfg.port))
        .map_err(|e| ServeError::Other(e.to_string()))?;
    let addr = server
        .server_addr()
        .to_ip()
        .ok_or_else(|| ServeError::Other("non-ip listen address".into()))?;
    let server = Arc::new(server);
    let hub = Arc::new(Hub::new());
    let (shutdown_tx, shutdown_rx) = unbounded::<()>();

    let http = {
        let server = server.clone();
        let state = state.clone();
        let hub = hub.clone();
        std::thread::Builder::new()
            .name("mt-rs-http".into())
            .spawn(move || http_loop(server, state, hub))
            .map_err(|e| ServeError::Other(format!("spawn http: {e}")))?
    };
    // Build the notify watcher synchronously *before* spawning the loop so
    // serve_dir only returns once every initial directory is being observed.
    // Otherwise tests (and fast manual edits) race with the watcher thread.
    let (notify_tx, notify_rx) = unbounded::<notify::Result<notify::Event>>();
    let mut notify_watcher: notify::RecommendedWatcher = notify::recommended_watcher(move |res| {
        let _ = notify_tx.send(res);
    })
    .map_err(|e| ServeError::Other(format!("watcher: {e}")))?;
    add_watches_with_skip(&mut notify_watcher, &cfg.root)
        .map_err(|e| ServeError::Other(format!("initial watch: {e}")))?;

    let watcher = {
        let cfg = cfg.clone();
        let state = state.clone();
        let hub = hub.clone();
        let renderer = renderer.clone();
        std::thread::Builder::new()
            .name("mt-rs-watcher".into())
            .spawn(move || {
                watcher_loop(
                    cfg,
                    state,
                    hub,
                    shutdown_rx,
                    renderer,
                    notify_rx,
                    notify_watcher,
                )
            })
            .map_err(|e| ServeError::Other(format!("spawn watcher: {e}")))?
    };

    Ok(DirServer {
        server,
        addr,
        hub,
        state,
        shutdown_tx,
        workers: vec![http, watcher],
    })
}

fn rebuild(
    cfg: &DirConfig,
    state: &RwLock<DirState>,
    renderer: &Renderer,
) -> Result<(), ServeError> {
    let ctx = Context::prepare(
        BuildOptions {
            root: cfg.root.clone(),
            out_dir: PathBuf::new(),
            site_name: cfg
                .root
                .file_name()
                .and_then(|s| s.to_str())
                .unwrap_or("site")
                .to_string(),
            theme: cfg.theme.clone(),
            live_reload_js: LIVE_RELOAD_JS.into(),
            exclude: cfg.exclude.clone(),
            nav_filenames: cfg.nav_filenames,
        },
        renderer,
    )?;

    // Snapshot the previous cache under a read lock; we'll build a new map
    // and swap at the end to keep the write window tiny.
    let prev_cache = {
        let guard = state.read().expect("state poisoned");
        guard.cache.clone()
    };
    let mut next_cache: HashMap<PathBuf, CacheEntry> = HashMap::with_capacity(ctx.entries.len());
    let mut pages: HashMap<String, Vec<u8>> = HashMap::with_capacity(ctx.entries.len());

    for i in 0..ctx.entries.len() {
        let e = &ctx.entries[i];
        let abs = e.abs.clone();
        let (mtime, size) = match std::fs::metadata(&abs) {
            Ok(m) => (m.modified().unwrap_or(SystemTime::UNIX_EPOCH), m.len()),
            Err(_) => (SystemTime::UNIX_EPOCH, 0),
        };

        let html = match prev_cache.get(&abs) {
            Some(cached) if cached.mtime == mtime && cached.size == size => cached.html.clone(),
            _ => {
                let page = ctx.render_page(i, renderer, Some("/assets/"))?;
                if let Some(sink) = cfg.on_warning.as_ref() {
                    for w in &page.warnings {
                        sink(&abs, w);
                    }
                }
                page.html
            }
        };
        next_cache.insert(
            abs,
            CacheEntry {
                mtime,
                size,
                html: html.clone(),
            },
        );
        pages.insert(e.out_rel.clone(), html);
    }
    let landing = landing_page(&ctx.entries)
        .map(|e| e.out_rel.clone())
        .unwrap_or_default();
    let mut guard = state.write().expect("state poisoned");
    guard.pages = pages;
    guard.landing = landing;
    guard.cache = next_cache;
    Ok(())
}

fn http_loop(server: Arc<Server>, state: Arc<RwLock<DirState>>, hub: Arc<Hub>) {
    for req in server.incoming_requests() {
        if req.url() == "/__livereload" && is_ws_upgrade(&req) {
            spawn_ws_worker(req, hub.clone());
            continue;
        }
        handle_http(req, &state);
    }
}

fn handle_http(req: tiny_http::Request, state: &RwLock<DirState>) {
    let url = req.url();
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

    let guard = state.read().expect("state poisoned");
    // The `pages` map is keyed by the *decoded* `out_rel` from the site
    // model (e.g. `guide/My Page.html`). Browsers send the percent-encoded
    // form (`guide/My%20Page.html`), so decode at the boundary before lookup.
    // See `crate::mt::site::url_path`.
    let key = if path == "/" || path.is_empty() {
        guard.landing.clone()
    } else {
        crate::mt::site::url_path::decode(path.trim_start_matches('/')).into_owned()
    };

    if let Some(body) = guard.pages.get(&key) {
        let resp = Response::from_data(body.clone())
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
    cfg: DirConfig,
    state: Arc<RwLock<DirState>>,
    hub: Arc<Hub>,
    shutdown: Receiver<()>,
    renderer: Arc<Renderer>,
    rx: Receiver<notify::Result<notify::Event>>,
    mut watcher: notify::RecommendedWatcher,
) {
    loop {
        let first = select! {
            recv(rx) -> ev => match ev {
                Ok(Ok(e)) => e,
                Ok(Err(_)) => continue,
                Err(_) => return,
            },
            recv(shutdown) -> _ => return,
        };
        // Newly-created subdirectory in the watched tree → add a watch for it
        // so subsequent .md files inside get noticed.
        handle_new_directories(&mut watcher, &first);
        if !relevant(&first, &cfg.root) {
            continue;
        }
        let mut deadline = Instant::now() + DEBOUNCE;
        loop {
            let remaining = deadline.saturating_duration_since(Instant::now());
            let timeout = after(remaining);
            select! {
                recv(rx) -> ev => match ev {
                    Ok(Ok(e)) => {
                        handle_new_directories(&mut watcher, &e);
                        if relevant(&e, &cfg.root) {
                            deadline = Instant::now() + DEBOUNCE;
                        }
                    }
                    Ok(Err(_)) => {}
                    Err(_) => return,
                },
                recv(timeout) -> _ => break,
                recv(shutdown) -> _ => return,
            }
        }
        if let Err(e) = rebuild(&cfg, &state, &renderer) {
            eprintln!("mt: rebuild failed: {e}");
            continue;
        }
        hub.broadcast();
    }
}

/// Walks `root` and adds every non-skipped directory to the watcher as
/// `NonRecursive`. Mirrors Go `internal/serve/dir.go::addWatcherRecursive`:
/// hidden / `node_modules` / `vendor` / `dist` / `build` / `target` are
/// pruned at the descent level so we never set up watches inside them.
fn add_watches_with_skip(
    watcher: &mut notify::RecommendedWatcher,
    root: &Path,
) -> notify::Result<()> {
    for entry in WalkDir::new(root)
        .follow_links(false)
        .into_iter()
        .filter_entry(|e| !should_skip_dir(e))
    {
        let entry = match entry {
            Ok(e) => e,
            Err(_) => continue,
        };
        if entry.file_type().is_dir() {
            // Don't error out the whole watcher loop if a single directory
            // can't be added — log and keep going.
            if let Err(e) = watcher.watch(entry.path(), RecursiveMode::NonRecursive) {
                eprintln!("mt: failed to watch {}: {e}", entry.path().display());
            }
        }
    }
    Ok(())
}

fn should_skip_dir(entry: &walkdir::DirEntry) -> bool {
    if entry.depth() == 0 {
        return false;
    }
    if !entry.file_type().is_dir() {
        return false;
    }
    let name = entry.file_name().to_string_lossy();
    if name.starts_with('.') {
        return true;
    }
    matches!(
        name.as_ref(),
        "node_modules" | "vendor" | "dist" | "build" | "target"
    )
}

/// If `ev` carries the creation of a new directory under our root, add a
/// watch for it. Skipped names (`.git`, `node_modules`, …) are filtered.
fn handle_new_directories(watcher: &mut notify::RecommendedWatcher, ev: &notify::Event) {
    if !matches!(
        ev.kind,
        EventKind::Create(CreateKind::Folder) | EventKind::Create(CreateKind::Any)
    ) {
        return;
    }
    for p in &ev.paths {
        if !p.is_dir() {
            continue;
        }
        if path_should_be_skipped(p) {
            continue;
        }
        let _ = watcher.watch(p, RecursiveMode::NonRecursive);
    }
}

fn path_should_be_skipped(p: &Path) -> bool {
    p.file_name()
        .and_then(OsStr::to_str)
        .map(|name| {
            name.starts_with('.')
                || matches!(
                    name,
                    "node_modules" | "vendor" | "dist" | "build" | "target"
                )
        })
        .unwrap_or(false)
}

fn relevant(ev: &notify::Event, root: &Path) -> bool {
    if !matches!(
        ev.kind,
        notify::EventKind::Modify(_) | notify::EventKind::Create(_) | notify::EventKind::Remove(_)
    ) {
        return false;
    }
    ev.paths.iter().any(|p| {
        // The skip check must only inspect the path *inside* the watched site
        // root. Otherwise a project nested under a parent path that happens to
        // contain `.cache` / `build` / `target` (e.g. `~/work/build/my-site/`)
        // would have every event filtered out.
        let rel = canonical_root(root)
            .ok()
            .and_then(|abs_root| {
                canonical_root(p)
                    .ok()
                    .and_then(|abs_p| abs_p.strip_prefix(&abs_root).ok().map(|s| s.to_path_buf()))
            })
            .unwrap_or_else(|| p.to_path_buf());
        if rel.components().any(|c| {
            c.as_os_str()
                .to_str()
                .map(|s| {
                    s.starts_with('.')
                        || matches!(s, "node_modules" | "vendor" | "dist" | "build" | "target")
                })
                .unwrap_or(false)
        }) {
            return false;
        }
        is_markdown_path(p)
    })
}

/// Best-effort canonicalisation: callers want stable prefix matching but the
/// path can have been removed (Remove events) by the time we look. Fall back
/// to the lexical form when the OS refuses.
fn canonical_root(p: &Path) -> std::io::Result<PathBuf> {
    match p.canonicalize() {
        Ok(c) => Ok(c),
        Err(_) => Ok(p.to_path_buf()),
    }
}

fn is_markdown_path(p: &Path) -> bool {
    p.extension()
        .and_then(OsStr::to_str)
        .map(|e| e.eq_ignore_ascii_case("md"))
        .unwrap_or(false)
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

#[cfg(test)]
mod tests {
    use super::*;
    use notify::Event;
    use notify::event::{EventKind, ModifyKind};
    use std::fs;

    fn tmp(label: &str) -> PathBuf {
        let pid = std::process::id();
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.subsec_nanos())
            .unwrap_or(0);
        let p = std::env::temp_dir().join(format!("mt-rs-dir-{label}-{pid}-{nanos}"));
        fs::create_dir_all(&p).unwrap();
        p
    }

    fn modify_event(p: PathBuf) -> Event {
        Event {
            kind: EventKind::Modify(ModifyKind::Any),
            paths: vec![p],
            attrs: notify::event::EventAttributes::new(),
        }
    }

    #[test]
    fn relevant_uses_relative_path_for_skip_check() {
        // Mimics the user's report: project root sits under a parent path
        // that contains `build` or `target` — events inside the *site* must
        // still register.
        let parent = tmp("under-build");
        let root = parent.join("build").join("my-site");
        fs::create_dir_all(&root).unwrap();
        let edited = root.join("page.md");
        fs::write(&edited, "# Hi\n").unwrap();
        let ev = modify_event(edited);
        assert!(
            relevant(&ev, &root),
            "edit inside site beneath a `build/` parent must still be relevant"
        );

        // But an event under an *internal* skipped directory is still ignored.
        let internal = root.join("build").join("ignored.md");
        fs::create_dir_all(internal.parent().unwrap()).unwrap();
        fs::write(&internal, "# noop").unwrap();
        let ev = modify_event(internal);
        assert!(
            !relevant(&ev, &root),
            "edit inside the site's own build/ directory must be skipped"
        );
    }
}
