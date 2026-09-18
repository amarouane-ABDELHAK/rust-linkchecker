//! Walking the site: what to fetch next, what has been seen, what broke.

use std::collections::{HashSet, VecDeque};

use futures::stream::{FuturesUnordered, StreamExt};
use reqwest::Client;
use tokio::sync::OnceCell;
use url::Url;

use crate::check::{self, Failure};
use crate::extract::{self, Kind, Link};
use crate::render::Renderer;
use crate::scope::{dedup_key, Scope};

/// How many requests are in flight at once.
pub const CONCURRENCY: usize = 16;

/// A link that failed, and the page that pointed at it.
#[derive(Debug, Clone)]
pub struct Broken {
    pub url: Url,
    pub failure: Failure,
    /// The page this link was found on. Absent only for the starting URL.
    pub referrer: Option<Url>,
}

/// What a whole crawl found.
#[derive(Debug, Default)]
pub struct Report {
    pub broken: Vec<Broken>,
    pub links_checked: usize,
    pub pages_crawled: usize,
    /// Set when a page needed rendering and no browser could be started:
    /// the sentence explaining why, printed once.
    pub render_unavailable: Option<String>,
}

impl Report {
    pub fn is_clean(&self) -> bool {
        self.broken.is_empty()
    }
}

struct Job {
    url: Url,
    referrer: Option<Url>,
    /// Whether this URL is underneath the starting path, and so worth reading
    /// for more links. Out-of-scope links are checked and never followed.
    crawl: bool,
}

/// A page that was read for links.
struct Page {
    /// Where it was finally served from; relative links resolve against this.
    final_url: Url,
    links: Vec<Link>,
}

struct Done {
    job: Job,
    /// `Ok(None)` is a live link that was not a page to read.
    outcome: Result<Option<Page>, Failure>,
}

/// The browser, started the first time a page turns out to need it. `Err`
/// means it was needed and could not be started.
type LazyRenderer = OnceCell<Result<Renderer, String>>;

/// Crawl from the scope's starting URL and check every link found beneath it.
///
/// One request per distinct URL, however many pages point at it. A link that
/// fails is recorded and the crawl carries on: an unreachable page is a result
/// to report, never a reason to stop.
pub async fn crawl(scope: &Scope, client: &Client) -> Report {
    let mut report = Report::default();
    let mut seen: HashSet<String> = HashSet::new();
    let mut queue: VecDeque<Job> = VecDeque::new();
    let renderer: LazyRenderer = OnceCell::new();

    seen.insert(dedup_key(scope.start()));
    queue.push_back(Job {
        url: scope.start().clone(),
        referrer: None,
        crawl: true,
    });

    let mut inflight = FuturesUnordered::new();
    loop {
        while inflight.len() < CONCURRENCY {
            let Some(job) = queue.pop_front() else { break };
            inflight.push(run(client, scope, &renderer, job));
        }

        let Some(done) = inflight.next().await else {
            break;
        };
        report.links_checked += 1;

        match done.outcome {
            Err(failure) => report.broken.push(Broken {
                url: done.job.url,
                failure,
                referrer: done.job.referrer,
            }),
            Ok(None) => {}
            Ok(Some(page)) => {
                report.pages_crawled += 1;
                for link in page.links {
                    if !seen.insert(dedup_key(&link.url)) {
                        continue;
                    }
                    let crawl = scope.contains(&link.url);
                    queue.push_back(Job {
                        url: link.url,
                        referrer: Some(page.final_url.clone()),
                        crawl,
                    });
                }
            }
        }
    }

    // Every borrow of the renderer lived in `inflight`; the queue is empty
    // and it is exhausted, so it can go.
    drop(inflight);
    match renderer.into_inner() {
        Some(Ok(renderer)) => renderer.shutdown().await,
        Some(Err(message)) => report.render_unavailable = Some(message),
        None => {}
    }
    report
}

async fn run(client: &Client, scope: &Scope, renderer: &LazyRenderer, job: Job) -> Done {
    let outcome = fetch(client, scope, renderer, &job).await;
    Done { job, outcome }
}

async fn fetch(
    client: &Client,
    scope: &Scope,
    renderer: &LazyRenderer,
    job: &Job,
) -> Result<Option<Page>, Failure> {
    let fetched = check::check(client, &job.url, job.crawl).await?;
    // A redirect can carry an in-scope URL out of scope; judge what we were
    // actually served, not what we asked for.
    let Some(html) = fetched.html.filter(|_| job.crawl) else {
        return Ok(None);
    };

    let mut page = Page {
        links: extract::links(&html, &fetched.final_url),
        final_url: fetched.final_url,
    };
    if leads_nowhere(&page, scope) && extract::runs_script(&html) {
        // The markup gave us nothing to follow but carries a script: this is
        // what a single-page app looks like as served. Let a browser run the
        // script and read the links it produces. (A script-free leaf page
        // cannot grow links, so it is never rendered.) No browser is a reason
        // to carry on with what the markup offered, not a failure of this
        // page.
        if let Ok(renderer) = renderer.get_or_init(Renderer::launch).await {
            let rendered = renderer.render(&page.final_url).await?;
            page.links
                .extend(extract::links(&rendered.html, &rendered.final_url));
            page.final_url = rendered.final_url;
        }
    }
    Ok(Some(page))
}

/// True when the page has no anchor to another in-scope page — nothing a
/// crawler could follow. A link back to the page itself does not count.
fn leads_nowhere(page: &Page, scope: &Scope) -> bool {
    let own = dedup_key(&page.final_url);
    !page.links.iter().any(|link| {
        link.kind == Kind::Anchor && scope.contains(&link.url) && dedup_key(&link.url) != own
    })
}
