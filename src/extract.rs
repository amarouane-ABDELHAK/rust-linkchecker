//! Pulling links out of a page's markup.

use scraper::{Html, Selector};
use url::Url;

/// What kind of reference a link is. Only anchors lead to more pages; the
/// distinction decides whether a page had anything to follow.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// `<a href>`: a page, or at least a thing that may be one.
    Anchor,
    /// `<img src>`, `<script src>`, `<link href>`: fetched, never followed.
    Asset,
}

#[derive(Debug, Clone)]
pub struct Link {
    pub url: Url,
    pub kind: Kind,
}

/// Every URL referenced by the page, resolved against `base`.
///
/// `base` must be the URL the page was finally served from — after any
/// redirects — or relative links resolve against the wrong place.
///
/// Anchors and asset references both count: a missing image is a broken link.
/// References that fail to resolve, or that use a scheme we cannot request,
/// are dropped here.
pub fn links(html: &str, base: &Url) -> Vec<Link> {
    let document = Html::parse_document(html);
    let base = base_href(&document, base);

    let mut found = Vec::new();
    for (selector, attribute, kind) in [
        ("a[href]", "href", Kind::Anchor),
        ("link[href]", "href", Kind::Asset),
        ("img[src]", "src", Kind::Asset),
        ("script[src]", "src", Kind::Asset),
    ] {
        let selector = Selector::parse(selector).expect("static selector");
        for element in document.select(&selector) {
            if is_connection_hint(element.value()) {
                continue;
            }
            if let Some(value) = element.value().attr(attribute) {
                if let Ok(url) = base.join(value.trim()) {
                    if crate::scope::is_checkable(&url) {
                        found.push(Link { url, kind });
                    }
                }
            }
        }
    }
    found
}

/// `<link rel="preconnect">` and `<link rel="dns-prefetch">` name an origin
/// the browser should warm a connection to. Nothing is fetched from the
/// `href`, whose path is meaningless and usually `/`, so it is not a link.
fn is_connection_hint(element: &scraper::node::Element) -> bool {
    element.name() == "link"
        && element
            .attr("rel")
            .map(|rel| {
                rel.split_ascii_whitespace().any(|token| {
                    token.eq_ignore_ascii_case("preconnect")
                        || token.eq_ignore_ascii_case("dns-prefetch")
                })
            })
            .unwrap_or(false)
}

/// Whether the page runs any script at all. A page without one cannot grow
/// links after it loads, so there is nothing a browser could add.
pub fn runs_script(html: &str) -> bool {
    let document = Html::parse_document(html);
    let selector = Selector::parse("script").expect("static selector");
    document.select(&selector).next().is_some()
}

/// A `<base href>` overrides the document's own URL for relative links.
fn base_href(document: &Html, fallback: &Url) -> Url {
    let selector = Selector::parse("base[href]").expect("static selector");
    document
        .select(&selector)
        .next()
        .and_then(|element| element.value().attr("href"))
        .and_then(|href| fallback.join(href).ok())
        .unwrap_or_else(|| fallback.clone())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn base() -> Url {
        Url::parse("https://example.com/start/index.html").unwrap()
    }

    fn links_of(html: &str) -> Vec<String> {
        links(html, &base())
            .into_iter()
            .map(|link| String::from(link.url))
            .collect()
    }

    #[test]
    fn finds_anchors_and_assets() {
        let found = links_of(
            r#"<html><head>
                 <link rel="stylesheet" href="/css/main.css">
                 <script src="app.js"></script>
               </head><body>
                 <a href="/start/about">about</a>
                 <a href="https://other.com/page">away</a>
                 <img src="viz.png">
               </body></html>"#,
        );
        assert!(found.contains(&"https://example.com/css/main.css".to_string()));
        assert!(found.contains(&"https://example.com/start/app.js".to_string()));
        assert!(found.contains(&"https://example.com/start/about".to_string()));
        assert!(found.contains(&"https://other.com/page".to_string()));
        assert!(found.contains(&"https://example.com/start/viz.png".to_string()));
        assert_eq!(found.len(), 5);
    }

    #[test]
    fn drops_unrequestable_schemes() {
        let found = links_of(
            r#"<a href="mailto:me@example.com">mail</a>
               <a href="tel:+15551234">call</a>
               <a href="javascript:void(0)">nothing</a>
               <img src="data:image/gif;base64,R0lGOD">
               <a href="/start/real">real</a>"#,
        );
        assert_eq!(found, vec!["https://example.com/start/real"]);
    }

    #[test]
    fn resolves_relative_links_against_the_page() {
        let found = links_of(r##"<a href="../sibling">up</a><a href="#section">self</a>"##);
        assert_eq!(
            found,
            vec![
                "https://example.com/sibling",
                "https://example.com/start/index.html#section"
            ]
        );
    }

    #[test]
    fn honours_a_base_href() {
        let found = links_of(
            r#"<head><base href="https://cdn.example.com/v2/"></head>
                                <body><a href="page">p</a></body>"#,
        );
        assert_eq!(found, vec!["https://cdn.example.com/v2/page"]);
    }

    #[test]
    fn anchors_and_assets_are_told_apart() {
        let found = links(
            r#"<a href="/start/about">a</a><img src="viz.png"><script src="app.js"></script>"#,
            &base(),
        );
        let kinds: Vec<Kind> = found.iter().map(|link| link.kind).collect();
        assert_eq!(kinds, vec![Kind::Anchor, Kind::Asset, Kind::Asset]);
    }

    #[test]
    fn only_pages_with_a_script_can_grow_links() {
        assert!(runs_script(
            r#"<div id="root"></div><script src="app.js"></script>"#
        ));
        assert!(runs_script("<script>document.write('x')</script>"));
        assert!(!runs_script("<html><body><p>bottom</p></body></html>"));
    }

    #[test]
    fn connection_hints_are_not_links() {
        // preconnect and dns-prefetch name an origin to warm up, not a
        // resource to fetch; their path is meaningless and often "/".
        let found = links_of(
            r#"<link rel="preconnect" href="/" crossorigin>
               <link rel="dns-prefetch" href="https://fonts.gstatic.com/">
               <link rel="preload" as="image" href="/start/logo.svg">
               <link rel="stylesheet" href="/start/main.css">"#,
        );
        assert_eq!(
            found,
            vec![
                "https://example.com/start/logo.svg",
                "https://example.com/start/main.css"
            ]
        );
    }

    #[test]
    fn a_page_with_no_links_yields_none() {
        assert!(links_of("<html><body><p>nothing here</p></body></html>").is_empty());
    }
}
