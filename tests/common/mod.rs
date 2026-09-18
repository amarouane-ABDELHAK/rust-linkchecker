//! A tiny HTTP server the end-to-end tests crawl, plus a record of every path
//! it was asked for. Raw HTTP/1.1: the point is to control exactly what the
//! crawler sees, including the badly behaved cases.

#![allow(dead_code)]

use std::collections::HashMap;
use std::io::{Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::sync::{mpsc, Arc, Mutex};
use std::thread;

#[derive(Clone)]
pub struct Reply {
    pub status: u16,
    pub content_type: &'static str,
    pub body: String,
    pub location: Option<String>,
    /// How long to sit on the request before answering.
    pub delay: Option<std::time::Duration>,
}

impl Reply {
    pub fn html(body: impl Into<String>) -> Reply {
        Reply {
            status: 200,
            content_type: "text/html; charset=utf-8",
            body: body.into(),
            location: None,
            delay: None,
        }
    }

    pub fn png() -> Reply {
        Reply {
            status: 200,
            content_type: "image/png",
            body: "\u{89}PNG".to_string(),
            location: None,
            delay: None,
        }
    }

    pub fn status(status: u16) -> Reply {
        Reply {
            status,
            content_type: "text/plain",
            body: format!("{status}"),
            location: None,
            delay: None,
        }
    }

    pub fn redirect(to: impl Into<String>) -> Reply {
        Reply {
            status: 301,
            content_type: "text/plain",
            body: String::new(),
            location: Some(to.into()),
            delay: None,
        }
    }

    /// A page that never answers within the crawler's patience.
    pub fn stalled() -> Reply {
        Reply {
            delay: Some(std::time::Duration::from_secs(13)),
            ..Reply::html("<p>eventually</p>")
        }
    }
}

pub struct Stub {
    pub addr: SocketAddr,
    hits: Arc<Mutex<Vec<String>>>,
}

impl Stub {
    pub fn url(&self, path: &str) -> String {
        format!("http://{}{}", self.addr, path)
    }

    /// Every path requested, in order, including repeats.
    pub fn hits(&self) -> Vec<String> {
        self.hits.lock().unwrap().clone()
    }

    pub fn hit_count(&self, path: &str) -> usize {
        self.hits().iter().filter(|hit| *hit == path).count()
    }

    pub fn was_hit(&self, path: &str) -> bool {
        self.hit_count(path) > 0
    }
}

/// What the crawler asked for, as the server saw it.
pub struct Request {
    pub method: String,
    pub path: String,
    /// Header names lowercased.
    pub headers: HashMap<String, String>,
}

impl Request {
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .get(&name.to_ascii_lowercase())
            .map(String::as_str)
    }
}

/// Serve `routes`, answering anything unlisted with a 404. Returns once the
/// socket is bound, so tests can use the address immediately.
pub fn serve(routes: HashMap<String, Reply>) -> Stub {
    serve_with(move |path| routes.get(path).cloned())
}

pub fn serve_with<F>(handler: F) -> Stub
where
    F: Fn(&str) -> Option<Reply> + Send + Sync + 'static,
{
    serve_requests(move |request| handler(&request.path))
}

/// Like `serve_with`, for servers whose answer depends on more than the path.
pub fn serve_requests<F>(handler: F) -> Stub
where
    F: Fn(&Request) -> Option<Reply> + Send + Sync + 'static,
{
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
    let addr = listener.local_addr().expect("addr");
    let hits = Arc::new(Mutex::new(Vec::new()));

    let handler = Arc::new(handler);
    let (ready, started) = mpsc::channel();
    {
        let hits = Arc::clone(&hits);
        thread::spawn(move || {
            ready.send(()).ok();
            for stream in listener.incoming() {
                let Ok(stream) = stream else { continue };
                let hits = Arc::clone(&hits);
                let handler = Arc::clone(&handler);
                thread::spawn(move || {
                    let _ = answer(stream, &*handler, &hits);
                });
            }
        });
    }
    started.recv().expect("server thread");

    Stub { addr, hits }
}

fn answer<F>(mut stream: TcpStream, handler: &F, hits: &Mutex<Vec<String>>) -> std::io::Result<()>
where
    F: Fn(&Request) -> Option<Reply> + Send + Sync + ?Sized,
{
    let mut buffer = [0_u8; 8192];
    let read = stream.read(&mut buffer)?;
    let raw = String::from_utf8_lossy(&buffer[..read]);
    let mut lines = raw.lines();
    let mut parts = lines.next().unwrap_or("").split_whitespace();
    let method = parts.next().unwrap_or("GET").to_string();
    let path = parts.next().unwrap_or("/").to_string();
    let headers = lines
        .take_while(|line| !line.is_empty())
        .filter_map(|line| line.split_once(':'))
        .map(|(name, value)| (name.trim().to_ascii_lowercase(), value.trim().to_string()))
        .collect();
    let request = Request {
        method: method.clone(),
        path: path.clone(),
        headers,
    };

    hits.lock().unwrap().push(path.clone());

    let reply = handler(&request).unwrap_or_else(|| Reply::status(404));
    if let Some(delay) = reply.delay {
        thread::sleep(delay);
    }
    let body = if method == "HEAD" { "" } else { &reply.body };
    let mut response = format!(
        "HTTP/1.1 {} X\r\nContent-Type: {}\r\nContent-Length: {}\r\nConnection: close\r\n",
        reply.status,
        reply.content_type,
        reply.body.len(),
    );
    if let Some(location) = &reply.location {
        response.push_str(&format!("Location: {location}\r\n"));
    }
    response.push_str("\r\n");
    response.push_str(body);

    stream.write_all(response.as_bytes())?;
    stream.flush()
}

/// The names the checker looks for on `PATH`, in the same order. Tests that
/// need a real browser skip themselves when none is present.
pub const BROWSER_NAMES: &[&str] = &[
    "google-chrome",
    "google-chrome-stable",
    "chromium",
    "chromium-browser",
    "chrome",
];

pub fn browser_on_path() -> bool {
    let Some(path) = std::env::var_os("PATH") else {
        return false;
    };
    std::env::split_paths(&path)
        .any(|dir| BROWSER_NAMES.iter().any(|name| dir.join(name).is_file()))
}

/// A shell page whose links exist only after its script has run: one
/// in-scope anchor per route, inserted by JavaScript. This is what a
/// single-page app looks like to a crawler.
pub fn shell(routes: &[&str]) -> Reply {
    let inserts: String = routes
        .iter()
        .map(|route| {
            format!(
                r#"var a = document.createElement('a'); a.href = '{route}'; a.textContent = '{route}'; document.getElementById('root').appendChild(a);"#
            )
        })
        .collect();
    Reply::html(format!(
        r#"<!doctype html><html><head><title>app</title></head>
           <body><div id="root"></div>
           <script>window.setTimeout(function () {{ {inserts} }}, 50);</script>
           </body></html>"#
    ))
}
