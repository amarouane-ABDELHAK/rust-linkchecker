//! Pulling links out of a page's markup.

use scraper::{Html, Selector};
use url::Url;

/// Every URL referenced by the page, resolved against `base`.
///
/// `base` must be the URL the page was finally served from — after any
/// redirects — or relative links resolve against the wrong place.
///
/// Anchors and asset references both count: a missing image is a broken link.
/// References that fail to resolve, or that use a scheme we cannot request,
/// are dropped here.
pub fn links(html: &str, base: &Url) -> Vec<Url> {
    let document = Html::parse_document(html);
    let base = base_href(&document, base);

    let mut found = Vec::new();
    for (selector, attribute) in [
        ("a[href]", "href"),
        ("link[href]", "href"),
        ("img[src]", "src"),
        ("script[src]", "src"),
    ] {
        let selector = Selector::parse(selector).expect("static selector");
        for element in document.select(&selector) {
            if let Some(value) = element.value().attr(attribute) {
                if let Ok(url) = base.join(value.trim()) {
                    if crate::scope::is_checkable(&url) {
                        found.push(url);
                    }
                }
            }
        }
    }
    found
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
        links(html, &base()).into_iter().map(String::from).collect()
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
    fn a_page_with_no_links_yields_none() {
        assert!(links_of("<html><body><p>nothing here</p></body></html>").is_empty());
    }
}
