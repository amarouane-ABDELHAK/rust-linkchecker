//! Deciding whether one URL is alive.

use std::time::Duration;

use reqwest::{Client, Response, StatusCode};
use url::Url;

/// How long a single request may take before it counts as dead.
pub const TIMEOUT: Duration = Duration::from_secs(10);
/// How many redirect hops to follow before giving up on a link.
pub const MAX_REDIRECTS: usize = 10;

/// Why a link is considered broken.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Failure {
    /// The server answered, with a status in the 400 or 500 range.
    Status(u16),
    /// No answer arrived in time.
    Timeout,
    /// The host could not be resolved.
    Dns,
    /// The host refused or dropped the connection.
    Connect,
    /// The link redirected in circles, or too far.
    TooManyRedirects,
    /// The page answered, but the browser could not render it.
    Render(String),
    /// Anything else that stopped the request from completing.
    Other(String),
}

impl Failure {
    /// The short label the report puts in its left-hand column.
    pub fn label(&self) -> String {
        match self {
            Failure::Status(code) => code.to_string(),
            Failure::Timeout => "timeout".to_string(),
            Failure::Dns => "DNS".to_string(),
            Failure::Connect => "refused".to_string(),
            Failure::TooManyRedirects => "redirects".to_string(),
            Failure::Render(_) => "render".to_string(),
            Failure::Other(_) => "error".to_string(),
        }
    }

    /// The detail line, when there is more to say than the label.
    pub fn detail(&self) -> Option<&str> {
        match self {
            Failure::Other(message) | Failure::Render(message) => Some(message),
            _ => None,
        }
    }
}

/// A link that answered.
pub struct Fetched {
    /// Where the request ended up, after any redirects. Relative links on this
    /// page resolve against this, not against the URL we asked for.
    pub final_url: Url,
    /// The page's markup, if we asked for it and the server sent HTML.
    pub html: Option<String>,
}

/// What a browser asks for. Some servers pick the response by this header
/// and answer 404 to the `*/*` HTTP clients send by default, so the checker
/// asks the way a visitor's browser does.
pub const ACCEPT: &str = "text/html,application/xhtml+xml,application/xml;q=0.9,*/*;q=0.8";

pub fn client() -> Result<Client, reqwest::Error> {
    let mut headers = reqwest::header::HeaderMap::new();
    headers.insert(
        reqwest::header::ACCEPT,
        reqwest::header::HeaderValue::from_static(ACCEPT),
    );
    Client::builder()
        .default_headers(headers)
        .timeout(TIMEOUT)
        .redirect(reqwest::redirect::Policy::limited(MAX_REDIRECTS))
        .user_agent(concat!("rust-linkchecker/", env!("CARGO_PKG_VERSION")))
        .build()
}

/// Check one URL. `want_html` asks for the body, for pages we intend to crawl.
///
/// A link is alive when the response — after redirects — is not a 4xx or 5xx.
/// Cheap HEAD requests are tried first for links we only need to check, but
/// plenty of servers answer HEAD with 405 or worse while serving GET happily,
/// so any unsuccessful HEAD is retried as a GET before the link is condemned.
pub async fn check(client: &Client, url: &Url, want_html: bool) -> Result<Fetched, Failure> {
    if !want_html {
        match client.head(url.clone()).send().await {
            Ok(response) if response.status().is_success() => {
                return Ok(Fetched {
                    final_url: response.url().clone(),
                    html: None,
                });
            }
            // A failed HEAD proves nothing: fall through to GET.
            _ => {}
        }
    }

    let response = client.get(url.clone()).send().await.map_err(classify)?;
    let status = response.status();
    if !is_alive(status) {
        return Err(Failure::Status(status.as_u16()));
    }

    let final_url = response.url().clone();
    let html = if want_html && is_html(&response) {
        // A body we cannot read is a broken link, not a crash.
        Some(response.text().await.map_err(classify)?)
    } else {
        None
    };
    Ok(Fetched { final_url, html })
}

/// Whether to parse this response for more links. Decided from the content
/// type rather than the URL, because most page URLs carry no extension.
fn is_html(response: &Response) -> bool {
    response
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .map(|value| {
            let value = value.to_ascii_lowercase();
            value.starts_with("text/html") || value.starts_with("application/xhtml+xml")
        })
        .unwrap_or(false)
}

fn classify(error: reqwest::Error) -> Failure {
    if error.is_timeout() {
        return Failure::Timeout;
    }
    if error.is_redirect() {
        return Failure::TooManyRedirects;
    }
    if error.is_connect() {
        return if looks_like_dns(&error) {
            Failure::Dns
        } else {
            Failure::Connect
        };
    }
    if let Some(status) = error.status() {
        return Failure::Status(status.as_u16());
    }
    Failure::Other(root_cause(&error))
}

/// reqwest folds name resolution into connection errors, so the distinction
/// has to come from the underlying message.
fn looks_like_dns(error: &reqwest::Error) -> bool {
    let message = chain(error).to_ascii_lowercase();
    message.contains("dns")
        || message.contains("failed to lookup")
        || message.contains("name or service not known")
        || message.contains("nodename nor servname")
}

fn chain(error: &dyn std::error::Error) -> String {
    let mut message = error.to_string();
    let mut source = error.source();
    while let Some(cause) = source {
        message.push_str(": ");
        message.push_str(&cause.to_string());
        source = cause.source();
    }
    message
}

fn root_cause(error: &reqwest::Error) -> String {
    let mut deepest: &dyn std::error::Error = error;
    while let Some(cause) = deepest.source() {
        deepest = cause;
    }
    deepest.to_string()
}

/// The one definition of a dead link: 4xx and 5xx, nothing else. A redirect
/// is judged on where it lands, which the client has already resolved by the
/// time a status reaches here.
pub fn is_alive(status: StatusCode) -> bool {
    !(status.is_client_error() || status.is_server_error())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn labels_read_as_a_column() {
        assert_eq!(Failure::Status(404).label(), "404");
        assert_eq!(Failure::Status(500).label(), "500");
        assert_eq!(Failure::Timeout.label(), "timeout");
        assert_eq!(Failure::Dns.label(), "DNS");
        assert_eq!(Failure::TooManyRedirects.label(), "redirects");
        assert_eq!(Failure::Render("x".into()).label(), "render");
    }

    #[test]
    fn only_4xx_and_5xx_are_dead() {
        assert!(is_alive(StatusCode::OK));
        assert!(is_alive(StatusCode::NO_CONTENT));
        assert!(is_alive(StatusCode::MOVED_PERMANENTLY));
        assert!(!is_alive(StatusCode::NOT_FOUND));
        assert!(!is_alive(StatusCode::INTERNAL_SERVER_ERROR));
    }
}
