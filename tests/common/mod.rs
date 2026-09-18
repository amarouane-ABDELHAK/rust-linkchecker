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
}

impl Reply {
    pub fn html(body: impl Into<String>) -> Reply {
        Reply {
            status: 200,
            content_type: "text/html; charset=utf-8",
            body: body.into(),
            location: None,
        }
    }

    pub fn png() -> Reply {
        Reply {
            status: 200,
            content_type: "image/png",
            body: "\u{89}PNG".to_string(),
            location: None,
        }
    }

    pub fn status(status: u16) -> Reply {
        Reply {
            status,
            content_type: "text/plain",
            body: format!("{status}"),
            location: None,
        }
    }

    pub fn redirect(to: impl Into<String>) -> Reply {
        Reply {
            status: 301,
            content_type: "text/plain",
            body: String::new(),
            location: Some(to.into()),
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

/// Serve `routes`, answering anything unlisted with a 404. Returns once the
/// socket is bound, so tests can use the address immediately.
pub fn serve(routes: HashMap<String, Reply>) -> Stub {
    serve_with(move |path| routes.get(path).cloned())
}

pub fn serve_with<F>(handler: F) -> Stub
where
    F: Fn(&str) -> Option<Reply> + Send + Sync + 'static,
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
    F: Fn(&str) -> Option<Reply> + Send + Sync + ?Sized,
{
    let mut buffer = [0_u8; 8192];
    let read = stream.read(&mut buffer)?;
    let request = String::from_utf8_lossy(&buffer[..read]);
    let mut parts = request.split_whitespace();
    let method = parts.next().unwrap_or("GET").to_string();
    let path = parts.next().unwrap_or("/").to_string();

    hits.lock().unwrap().push(path.clone());

    let reply = handler(&path).unwrap_or_else(|| Reply::status(404));
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
