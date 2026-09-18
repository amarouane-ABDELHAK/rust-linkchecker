//! Turning a crawl into something a developer can act on.

use std::fmt::Write as _;

use crate::crawl::Report;

/// How many broken links are printed before the list is cut short.
pub const MAX_LISTED: usize = 100;

/// The run's human-readable summary.
///
/// Every broken link names its status and the page that linked to it, so the
/// developer knows what to edit without crawling again. Long lists are cut at
/// [`MAX_LISTED`], but the count is always the true one.
pub fn render(report: &Report) -> String {
    let mut out = String::new();
    let _ = writeln!(
        out,
        "Checked {} {} across {} {}.",
        report.links_checked,
        plural(report.links_checked, "link", "links"),
        report.pages_crawled,
        plural(report.pages_crawled, "page", "pages"),
    );

    if report.is_clean() {
        let _ = writeln!(out, "\nNo broken links.");
        return out;
    }

    let total = report.broken.len();
    let shown = total.min(MAX_LISTED);
    if total > shown {
        let _ = writeln!(out, "\nBROKEN ({total} total, showing first {shown}):\n");
    } else {
        let _ = writeln!(out, "\nBROKEN ({total}):\n");
    }

    for broken in report.broken.iter().take(shown) {
        let _ = writeln!(out, "  {:>9}  {}", broken.failure.label(), broken.url);
        if let Some(referrer) = &broken.referrer {
            let _ = writeln!(out, "             linked from {referrer}");
        } else {
            let _ = writeln!(out, "             the starting URL");
        }
        if let Some(detail) = broken.failure.detail() {
            let _ = writeln!(out, "             {detail}");
        }
    }

    let _ = writeln!(
        out,
        "\nError: {total} broken {}",
        plural(total, "link", "links")
    );
    out
}

fn plural<'a>(count: usize, one: &'a str, many: &'a str) -> &'a str {
    if count == 1 {
        one
    } else {
        many
    }
}

#[cfg(test)]
mod tests {
    use url::Url;

    use super::*;
    use crate::check::Failure;
    use crate::crawl::Broken;

    fn url(s: &str) -> Url {
        Url::parse(s).unwrap()
    }

    fn broken(n: usize) -> Vec<Broken> {
        (0..n)
            .map(|i| Broken {
                url: url(&format!("https://example.com/dead/{i}")),
                failure: Failure::Status(404),
                referrer: Some(url("https://example.com/start/")),
            })
            .collect()
    }

    #[test]
    fn a_clean_run_says_so() {
        let report = Report {
            broken: vec![],
            links_checked: 12,
            pages_crawled: 3,
        };
        let out = render(&report);
        assert!(out.contains("Checked 12 links across 3 pages."));
        assert!(out.contains("No broken links."));
        assert!(!out.contains("BROKEN"));
    }

    #[test]
    fn every_broken_link_names_its_source_page() {
        let report = Report {
            broken: broken(1),
            links_checked: 5,
            pages_crawled: 1,
        };
        let out = render(&report);
        assert!(out.contains("404  https://example.com/dead/0"));
        assert!(out.contains("linked from https://example.com/start/"));
        assert!(out.contains("Error: 1 broken link"));
    }

    #[test]
    fn long_lists_are_cut_but_the_total_is_not() {
        let report = Report {
            broken: broken(127),
            links_checked: 412,
            pages_crawled: 38,
        };
        let out = render(&report);
        assert!(out.contains("BROKEN (127 total, showing first 100):"));
        assert!(out.contains("/dead/99"));
        assert!(!out.contains("/dead/100"));
        assert!(out.contains("Error: 127 broken links"));
    }

    #[test]
    fn a_broken_starting_url_has_no_referrer_to_name() {
        let report = Report {
            broken: vec![Broken {
                url: url("https://example.com/start"),
                failure: Failure::Dns,
                referrer: None,
            }],
            links_checked: 1,
            pages_crawled: 0,
        };
        let out = render(&report);
        assert!(out.contains("DNS  https://example.com/start"));
        assert!(out.contains("the starting URL"));
    }
}
