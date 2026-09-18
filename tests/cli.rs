//! End-to-end: run the real binary against a controlled site and check what it
//! reports, what it requested, and what it exited with.

mod common;

use std::collections::HashMap;
use std::process::{Command, Output};

use common::{serve, serve_with, Reply, Stub};

fn run(url: &str) -> Output {
    Command::new(env!("CARGO_BIN_EXE_linkchecker"))
        .arg(url)
        .output()
        .expect("run linkchecker")
}

fn stdout(output: &Output) -> String {
    String::from_utf8_lossy(&output.stdout).to_string()
}

/// A site with one of everything: working links, dead links, a dead image, a
/// page several levels down, a page outside the starting path, links to
/// another origin, redirects, duplicates, and a mailto.
fn fixture() -> (Stub, Stub) {
    let external = serve({
        let mut routes = HashMap::new();
        routes.insert("/page".to_string(), Reply::html("<p>alive</p>"));
        routes
    });

    let away = format!("http://{}", external.addr);
    let main = serve({
        let mut routes = HashMap::new();
        routes.insert(
            "/start/".to_string(),
            Reply::html(format!(
                r#"<html><body>
                     <a href="about">about</a>
                     <a href="about">about again</a>
                     <a href="/start/dead">dead</a>
                     <a href="/start/shared">shared</a>
                     <a href="/start/redirect-ok">redirected</a>
                     <a href="/start/redirect-dead">redirected into a 404</a>
                     <a href="/outside">outside the prefix</a>
                     <a href="{away}/page">another origin</a>
                     <a href="{away}/gone">another origin, dead</a>
                     <a href="mailto:someone@example.com">mail</a>
                     <img src="logo.png">
                     <img src="missing.png">
                   </body></html>"#
            )),
        );
        routes.insert(
            "/start/about".to_string(),
            Reply::html(
                r#"<a href="deep/">deeper</a>
                   <a href="/start/dead">dead again</a>
                   <a href="/start/shared">shared again</a>"#,
            ),
        );
        routes.insert(
            "/start/deep/".to_string(),
            Reply::html(r#"<a href="leaf">leaf</a>"#),
        );
        routes.insert("/start/deep/leaf".to_string(), Reply::html("<p>bottom</p>"));
        routes.insert(
            "/start/shared".to_string(),
            Reply::html("<p>linked twice</p>"),
        );
        routes.insert("/start/logo.png".to_string(), Reply::png());
        routes.insert(
            "/start/redirect-ok".to_string(),
            Reply::redirect("/start/about"),
        );
        routes.insert(
            "/start/redirect-dead".to_string(),
            Reply::redirect("/start/dead"),
        );
        routes.insert(
            "/outside".to_string(),
            Reply::html(r#"<a href="/outside/never">never crawled</a>"#),
        );
        routes.insert(
            "/outside/never".to_string(),
            Reply::html("<p>unreachable</p>"),
        );
        // /start/dead, /start/missing.png and {away}/gone are absent: 404.
        routes
    });

    (main, external)
}

#[test]
fn reports_every_kind_of_broken_link_and_fails_the_run() {
    let (main, external) = fixture();
    let output = run(&main.url("/start/"));
    let report = stdout(&output);

    assert_eq!(
        output.status.code(),
        Some(1),
        "a broken link must fail the run:\n{report}"
    );
    assert!(report.contains("Error: 4 broken links"), "{report}");

    // a dead anchor, a dead image, a dead link on another origin, and a
    // redirect that lands on a 404
    assert!(report.contains(&main.url("/start/dead")), "{report}");
    assert!(report.contains(&main.url("/start/missing.png")), "{report}");
    assert!(report.contains(&external.url("/gone")), "{report}");
    assert!(
        report.contains(&main.url("/start/redirect-dead")),
        "{report}"
    );
}

#[test]
fn names_the_page_each_broken_link_was_found_on() {
    let (main, _external) = fixture();
    let report = stdout(&run(&main.url("/start/")));

    let dead = report
        .lines()
        .position(|line| line.contains(&main.url("/start/dead")))
        .expect("the dead link is listed");
    assert!(
        report
            .lines()
            .nth(dead + 1)
            .unwrap()
            .contains("linked from"),
        "{report}"
    );
}

#[test]
fn a_redirect_to_a_live_page_is_alive() {
    let (main, _external) = fixture();
    let report = stdout(&run(&main.url("/start/")));
    assert!(
        !report.contains(&main.url("/start/redirect-ok")),
        "{report}"
    );
}

#[test]
fn crawls_underneath_the_start_and_stops_at_the_fence() {
    let (main, _external) = fixture();
    run(&main.url("/start/"));

    // several levels below the start, reached only through another page
    assert!(main.was_hit("/start/deep/leaf"), "{:?}", main.hits());
    // outside the prefix: checked, but never read for more links
    assert!(main.was_hit("/outside"), "{:?}", main.hits());
    assert!(!main.was_hit("/outside/never"), "{:?}", main.hits());
}

#[test]
fn a_url_linked_from_several_pages_is_requested_once() {
    let (main, _external) = fixture();
    run(&main.url("/start/"));

    // linked from /start/ and again from /start/about
    assert_eq!(main.hit_count("/start/shared"), 1, "{:?}", main.hits());
    // /start/about is linked twice from /start/, and the crawler asks for it
    // once. It appears twice in the log only because /start/redirect-ok sends
    // the HTTP client there as well, which is the client following a redirect,
    // not the crawler re-fetching a link it already knows.
    assert_eq!(main.hit_count("/start/about"), 2, "{:?}", main.hits());
}

#[test]
fn non_http_schemes_are_skipped_silently() {
    let (main, _external) = fixture();
    let report = stdout(&run(&main.url("/start/")));
    assert!(!report.contains("mailto"), "{report}");
}

#[test]
fn a_clean_site_passes() {
    let site = serve({
        let mut routes = HashMap::new();
        routes.insert(
            "/ok/".to_string(),
            Reply::html(r#"<a href="/ok/next">next</a><img src="/ok/pic.png">"#),
        );
        routes.insert("/ok/next".to_string(), Reply::html("<p>fine</p>"));
        routes.insert("/ok/pic.png".to_string(), Reply::png());
        routes
    });

    let output = run(&site.url("/ok/"));
    let report = stdout(&output);
    assert_eq!(output.status.code(), Some(0), "{report}");
    assert!(report.contains("No broken links."), "{report}");
}

#[test]
fn long_lists_are_cut_at_a_hundred_but_the_total_is_true() {
    let site = serve_with(|path| match path {
        "/many/" => {
            let links: String = (0..127)
                .map(|i| format!(r#"<a href="/many/dead/{i}">{i}</a>"#))
                .collect();
            Some(Reply::html(format!("<html><body>{links}</body></html>")))
        }
        _ => None,
    });

    let output = run(&site.url("/many/"));
    let report = stdout(&output);
    assert_eq!(output.status.code(), Some(1), "{report}");
    assert!(
        report.contains("BROKEN (127 total, showing first 100):"),
        "{report}"
    );
    assert!(report.contains("Error: 127 broken links"), "{report}");
    assert_eq!(
        report.matches("linked from").count(),
        100,
        "exactly one hundred listed:\n{report}"
    );
}

#[test]
fn a_start_url_that_is_not_a_url_explains_itself() {
    let output = run("not-a-url");
    assert_eq!(output.status.code(), Some(2));
    let complaint = String::from_utf8_lossy(&output.stderr);
    assert!(
        complaint.contains("usage: linkchecker <url>"),
        "{complaint}"
    );
}

#[test]
fn an_unreachable_start_url_is_reported_not_panicked() {
    // nothing is listening on this port
    let output = run("http://127.0.0.1:1/start");
    let report = stdout(&output);
    assert_eq!(output.status.code(), Some(1), "{report}");
    assert!(report.contains("the starting URL"), "{report}");
}
