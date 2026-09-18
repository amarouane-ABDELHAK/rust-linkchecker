//! Walking the site: what to fetch next, what has been seen, what broke.

use std::collections::{HashSet, VecDeque};

use futures::stream::{FuturesUnordered, StreamExt};
use reqwest::Client;
use url::Url;

use crate::check::{self, Failure};
use crate::extract;
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

struct Done {
    job: Job,
    outcome: Result<check::Fetched, Failure>,
}

/// Crawl from the scope's starting URL and check every link found beneath it.
///
/// One request per distinct URL, however many pages point at it. A link that
/// fails is recorded and the crawl carries on: an unreachable page is a result
/// to report, never a reason to stop.
pub async fn crawl(scope: &Scope, client: &Client) -> Report {
    let mut report = Report::default();
    let mut seen: HashSet<String> = HashSet::new();
    let mut queue: VecDeque<Job> = VecDeque::new();

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
            inflight.push(run(client, job));
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
            Ok(fetched) => {
                let Some(html) = fetched.html else { continue };
                report.pages_crawled += 1;
                for link in extract::links(&html, &fetched.final_url) {
                    if !seen.insert(dedup_key(&link)) {
                        continue;
                    }
                    let crawl = scope.contains(&link);
                    queue.push_back(Job {
                        url: link,
                        referrer: Some(fetched.final_url.clone()),
                        crawl,
                    });
                }
            }
        }
    }

    report
}

async fn run(client: &Client, job: Job) -> Done {
    // A redirect can carry an in-scope URL out of scope; judge what we were
    // actually served, not what we asked for.
    let outcome = match check::check(client, &job.url, job.crawl).await {
        Ok(fetched) if job.crawl => Ok(fetched),
        Ok(fetched) => Ok(check::Fetched {
            final_url: fetched.final_url,
            html: None,
        }),
        Err(failure) => Err(failure),
    };
    Done { job, outcome }
}
