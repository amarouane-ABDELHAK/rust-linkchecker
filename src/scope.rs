//! What counts as "underneath the starting URL", and when two links are the
//! same link.

use url::Url;

/// The starting point of a crawl: an origin plus the path segments every
/// crawlable page must sit beneath.
#[derive(Debug, Clone)]
pub struct Scope {
    start: Url,
    prefix: Vec<String>,
}

impl Scope {
    pub fn new(start: Url) -> Self {
        let prefix = path_segments(&start);
        Scope { start, prefix }
    }

    pub fn start(&self) -> &Url {
        &self.start
    }

    /// True when `url` sits on the same origin and underneath the starting
    /// path. Compares whole segments, so a start of `/start` does not swallow
    /// `/started`.
    pub fn contains(&self, url: &Url) -> bool {
        if !same_origin(&self.start, url) {
            return false;
        }
        let segments = path_segments(url);
        segments.len() >= self.prefix.len()
            && segments.iter().zip(self.prefix.iter()).all(|(a, b)| a == b)
    }
}

fn same_origin(a: &Url, b: &Url) -> bool {
    a.scheme() == b.scheme()
        && a.host_str() == b.host_str()
        && a.port_or_known_default() == b.port_or_known_default()
}

/// Non-empty path segments. The trailing empty segment of `/start/` is
/// dropped so it does not distinguish itself from `/start` for scoping.
fn path_segments(url: &Url) -> Vec<String> {
    url.path_segments()
        .map(|segments| {
            segments
                .filter(|segment| !segment.is_empty())
                .map(|segment| segment.to_string())
                .collect()
        })
        .unwrap_or_default()
}

/// The identity of a link for deduplication. Fragments never reach the server,
/// so they are dropped; a trailing slash can be a genuinely different resource,
/// so it is kept.
pub fn dedup_key(url: &Url) -> String {
    let mut url = url.clone();
    url.set_fragment(None);
    url.to_string()
}

/// True for schemes worth a request. `mailto:`, `tel:`, `javascript:` and
/// `data:` are skipped silently.
pub fn is_checkable(url: &Url) -> bool {
    matches!(url.scheme(), "http" | "https")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn url(s: &str) -> Url {
        Url::parse(s).unwrap()
    }

    #[test]
    fn prefix_matches_whole_segments_only() {
        let scope = Scope::new(url("https://example.com/start"));
        assert!(scope.contains(&url("https://example.com/start")));
        assert!(scope.contains(&url("https://example.com/start/")));
        assert!(scope.contains(&url("https://example.com/start/about")));
        assert!(scope.contains(&url("https://example.com/start/a/b/c.png")));
        // the trap: /started is not underneath /start
        assert!(!scope.contains(&url("https://example.com/started")));
        assert!(!scope.contains(&url("https://example.com/startled/page")));
        assert!(!scope.contains(&url("https://example.com/other")));
        assert!(!scope.contains(&url("https://example.com/")));
    }

    #[test]
    fn other_origins_are_never_in_scope() {
        let scope = Scope::new(url("https://example.com/start"));
        assert!(!scope.contains(&url("https://other.com/start/about")));
        assert!(!scope.contains(&url("http://example.com/start/about")));
        assert!(!scope.contains(&url("https://example.com:8443/start/about")));
    }

    #[test]
    fn a_root_start_contains_everything_on_the_origin() {
        let scope = Scope::new(url("https://example.com/"));
        assert!(scope.contains(&url("https://example.com/anything/at/all")));
        assert!(!scope.contains(&url("https://other.com/anything")));
    }

    #[test]
    fn fragments_collapse_but_trailing_slashes_do_not() {
        assert_eq!(
            dedup_key(&url("https://example.com/page#intro")),
            dedup_key(&url("https://example.com/page"))
        );
        assert_ne!(
            dedup_key(&url("https://example.com/page/")),
            dedup_key(&url("https://example.com/page"))
        );
        assert_ne!(
            dedup_key(&url("https://example.com/page?a=1")),
            dedup_key(&url("https://example.com/page"))
        );
    }

    #[test]
    fn only_http_schemes_are_checkable() {
        assert!(is_checkable(&url("https://example.com/")));
        assert!(is_checkable(&url("http://example.com/")));
        assert!(!is_checkable(&url("mailto:me@example.com")));
        assert!(!is_checkable(&url("tel:+15551234")));
        assert!(!is_checkable(&url("javascript:void(0)")));
        assert!(!is_checkable(&url("data:text/plain,hi")));
    }
}
