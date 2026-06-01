//! Dev server with live-reload. Mirror of Go `internal/serve`.
//!
//! Synchronous all the way: tiny_http for HTTP, tungstenite for WebSocket
//! (upgrading via `Request::upgrade`), notify for file watching, and crossbeam
//! channels for the broadcast hub.

pub mod dir;
pub mod hub;
pub mod single;

pub use dir::{DirConfig, DirServer, serve_dir};
pub use hub::Hub;
pub use single::{SingleConfig, SingleServer, serve_single};

use std::io::{Read, Write};
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use crate::mt::render::RenderWarning;

/// Callback the dev servers invoke for every non-fatal warning surfaced by a
/// render pass. The library never writes to stderr directly — wire one of
/// these in to display diagnostics. The CLI installs an `eprintln!` closure;
/// tests typically leave it as `None` (warnings are then dropped silently).
pub type WarningSink = Arc<dyn Fn(&Path, &RenderWarning) + Send + Sync + 'static>;

use crossbeam_channel::{Receiver, after, select};
use tiny_http::{Header, Request, Response};
use tungstenite::Message;
use tungstenite::handshake::derive_accept_key;
use tungstenite::protocol::{Role, WebSocket};

/// JS injected into every served page in --serve mode.
pub const LIVE_RELOAD_JS: &str = "(function(){var p=location.protocol==='https:'?'wss:':'ws:';var ws=new WebSocket(p+'//'+location.host+'/__livereload');ws.onmessage=function(){location.reload();};ws.onclose=function(){setTimeout(function(){location.reload();},1500);};})();";

/// Errors produced by the serve subsystem.
#[derive(Debug)]
pub enum ServeError {
    Io(std::io::Error),
    Site(crate::mt::site::build::SiteError),
    Render(crate::mt::render::RenderError),
    Page(crate::mt::assets::page::RenderError),
    Watcher(notify::Error),
    Other(String),
}

impl std::fmt::Display for ServeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ServeError::Io(e) => write!(f, "io: {e}"),
            ServeError::Site(e) => write!(f, "site: {e}"),
            ServeError::Render(e) => write!(f, "render: {e}"),
            ServeError::Page(e) => write!(f, "page: {e}"),
            ServeError::Watcher(e) => write!(f, "watcher: {e}"),
            ServeError::Other(s) => write!(f, "{s}"),
        }
    }
}

impl std::error::Error for ServeError {}

impl From<std::io::Error> for ServeError {
    fn from(e: std::io::Error) -> Self {
        ServeError::Io(e)
    }
}
impl From<crate::mt::site::build::SiteError> for ServeError {
    fn from(e: crate::mt::site::build::SiteError) -> Self {
        ServeError::Site(e)
    }
}
impl From<crate::mt::render::RenderError> for ServeError {
    fn from(e: crate::mt::render::RenderError) -> Self {
        ServeError::Render(e)
    }
}
impl From<crate::mt::assets::page::RenderError> for ServeError {
    fn from(e: crate::mt::assets::page::RenderError) -> Self {
        ServeError::Page(e)
    }
}
impl From<notify::Error> for ServeError {
    fn from(e: notify::Error) -> Self {
        ServeError::Watcher(e)
    }
}

// ---------- WebSocket upgrade glue ----------

/// Returns true when the request looks like a WebSocket upgrade handshake.
pub fn is_ws_upgrade(req: &Request) -> bool {
    let mut has_upgrade = false;
    let mut has_connection = false;
    for h in req.headers() {
        let name = h.field.as_str().as_str().to_ascii_lowercase();
        let value = h.value.as_str().to_ascii_lowercase();
        if name == "upgrade" && value.contains("websocket") {
            has_upgrade = true;
        }
        if name == "connection" && value.contains("upgrade") {
            has_connection = true;
        }
    }
    has_upgrade && has_connection
}

/// Completes the WebSocket handshake on a tiny_http request and returns a
/// ready-to-use tungstenite session.
pub fn accept_websocket(req: Request) -> Result<WebSocket<Box<dyn ReadWriteSend>>, ServeError> {
    let client_key = req
        .headers()
        .iter()
        .find(|h| h.field.equiv("Sec-WebSocket-Key"))
        .map(|h| h.value.as_str().to_string())
        .ok_or_else(|| ServeError::Other("missing Sec-WebSocket-Key".into()))?;
    let accept = derive_accept_key(client_key.as_bytes());
    let accept_bytes: &[u8] = accept.as_bytes();

    let response = Response::empty(101)
        .with_header(Header::from_bytes(&b"Upgrade"[..], &b"websocket"[..]).unwrap())
        .with_header(Header::from_bytes(&b"Connection"[..], &b"Upgrade"[..]).unwrap())
        .with_header(Header::from_bytes(&b"Sec-WebSocket-Accept"[..], accept_bytes).unwrap());

    let stream = req.upgrade("websocket", response);
    let stream: Box<dyn ReadWriteSend> = Box::new(BoxedStream(stream));
    Ok(WebSocket::from_raw_socket(stream, Role::Server, None))
}

/// Marker trait so WebSocket<...> can hold a boxed tiny_http stream.
pub trait ReadWriteSend: Read + Write + Send {}
impl<T: Read + Write + Send + ?Sized> ReadWriteSend for T {}

/// Newtype wrapper so we can implement ReadWriteSend for the boxed stream.
struct BoxedStream(Box<dyn tiny_http::ReadWrite + Send>);
impl Read for BoxedStream {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        self.0.read(buf)
    }
}
impl Write for BoxedStream {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.0.write(buf)
    }
    fn flush(&mut self) -> std::io::Result<()> {
        self.0.flush()
    }
}

/// WS session loop: forward broadcast pulses, send periodic pings, exit on disconnect.
pub fn ws_session(mut ws: WebSocket<Box<dyn ReadWriteSend>>, reload: Receiver<()>) {
    let ping = || -> Vec<u8> { Vec::new() };
    loop {
        let timeout = after(Duration::from_secs(20));
        select! {
            recv(reload) -> msg => match msg {
                Ok(()) => {
                    if ws.send(Message::Text("reload".into())).is_err() {
                        break;
                    }
                }
                Err(_) => break, // hub gone
            },
            recv(timeout) -> _ => {
                if ws.send(Message::Ping(ping())).is_err() {
                    break;
                }
            }
        }
    }
    let _ = ws.close(None);
}

/// Common helper used by both servers: spawn a worker thread to handle the
/// WebSocket session bound to a fresh `Hub` subscription.
pub fn spawn_ws_worker(req: Request, hub: Arc<Hub>) {
    std::thread::Builder::new()
        .name("mt-rs-ws".into())
        .spawn(move || {
            let ws = match accept_websocket(req) {
                Ok(w) => w,
                Err(_) => return,
            };
            ws_session(ws, hub.subscribe());
        })
        .expect("spawn ws worker");
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use std::thread;
    use std::time::Duration;
    use tiny_http::Server;

    #[test]
    fn live_reload_js_targets_correct_endpoint() {
        assert!(LIVE_RELOAD_JS.contains("/__livereload"));
        assert!(LIVE_RELOAD_JS.contains("WebSocket"));
    }

    // Helpers — spin up a tiny_http server that hands every incoming request to
    // the closure so we can inspect what `is_ws_upgrade` / `accept_websocket` see.
    fn capture_request<F>(handler: F)
    where
        F: FnOnce(Request) + Send + 'static,
    {
        let server = Server::http("127.0.0.1:0").expect("bind");
        let addr = server.server_addr().to_ip().unwrap();
        let server = Arc::new(server);
        let s = server.clone();
        let t = thread::spawn(move || {
            if let Some(req) = s.incoming_requests().next() {
                handler(req);
            }
        });
        // Client side — open TCP, write request, close.
        let mut client = std::net::TcpStream::connect(addr).unwrap();
        use std::io::Write as _;
        client
            .write_all(
                b"GET / HTTP/1.1\r\nHost: x\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Version: 13\r\nSec-WebSocket-Key: dGhlIHNhbXBsZSBub25jZQ==\r\n\r\n",
            )
            .unwrap();
        let _ = t.join();
        let _ = client; // dropped → TCP close
        server.unblock();
    }

    #[test]
    fn is_ws_upgrade_detects_real_handshake() {
        capture_request(|req| {
            assert!(is_ws_upgrade(&req), "should detect WS upgrade headers");
        });
    }

    #[test]
    fn accept_websocket_requires_key_header() {
        let server = Server::http("127.0.0.1:0").expect("bind");
        let addr = server.server_addr().to_ip().unwrap();
        let server = Arc::new(server);
        let s = server.clone();
        let t = thread::spawn(move || {
            if let Some(req) = s.incoming_requests().next() {
                let err = accept_websocket(req).err().expect("should fail");
                assert!(format!("{err}").contains("Sec-WebSocket-Key"));
            }
        });
        let mut client = std::net::TcpStream::connect(addr).unwrap();
        use std::io::Write as _;
        // No Sec-WebSocket-Key header at all.
        client
            .write_all(
                b"GET / HTTP/1.1\r\nHost: x\r\nUpgrade: websocket\r\nConnection: Upgrade\r\n\r\n",
            )
            .unwrap();
        let _ = t.join();
        let _ = client;
        server.unblock();
        // Tiny sleep to flush thread joining.
        thread::sleep(Duration::from_millis(20));
    }
}
