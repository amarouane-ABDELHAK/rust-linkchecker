//! Rendering a page in a headless browser, for sites whose links exist only
//! after their script has run.
//!
//! Rendering is a fallback. The crawl asks for it only when a page's served
//! markup yields no pages to follow, and the browser is started the first time
//! that happens, so a plain site never pays for one. The browser is whatever
//! Chrome or Chromium is already on `PATH`; nothing is downloaded.

use std::path::PathBuf;
use std::time::{Duration, Instant};

use chromiumoxide::browser::{Browser, BrowserConfig};
use chromiumoxide::page::Page;
use futures::StreamExt;
use tokio::sync::{Mutex, Semaphore};
use tokio::task::JoinHandle;
use url::Url;

use crate::check::{Failure, TIMEOUT};

/// How many pages render at once. Tabs are far heavier than requests, and
/// the runners this runs on have two cores.
pub const RENDER_CONCURRENCY: usize = 4;
/// How long the browser may take to start. A cold first start on a fresh
/// GitHub runner takes around eleven seconds, so the per-request timeout is
/// far too short here. Paid once per run, and only when rendering is needed.
pub const LAUNCH_TIMEOUT: Duration = Duration::from_secs(60);
/// The most one page may take to render, opening and closing the tab
/// included. Chromium can hang on either of those, and an unbounded wait
/// there stalls the whole crawl on one URL.
pub const RENDER_TIMEOUT: Duration = Duration::from_secs(20);
/// How long a page must go without a new link or a newly finished network
/// request before it counts as finished loading, once its load event has
/// fired. Apps fetch data after load and only then render the links that
/// depend on it, so this has to outlast a typical fetch.
const SETTLE: Duration = Duration::from_millis(1500);
/// The same, while the load event has not fired: a script bundle that is
/// still downloading adds nothing for a while and then adds everything, so
/// a quiet spell proves less before load than after it.
const SETTLE_BEFORE_LOAD: Duration = Duration::from_millis(2500);
const POLL: Duration = Duration::from_millis(100);

/// Executable names tried on `PATH`, in order.
pub const BROWSER_NAMES: &[&str] = &[
    "google-chrome",
    "google-chrome-stable",
    "chromium",
    "chromium-browser",
    "chrome",
];

/// What the browser saw once the page had loaded.
pub struct Rendered {
    /// Where the tab ended up, after any redirects the app itself performed.
    pub final_url: Url,
    pub html: String,
}

/// A running headless browser, shared by the whole crawl.
pub struct Renderer {
    browser: Mutex<Browser>,
    events: JoinHandle<()>,
    tabs: Semaphore,
    /// This run's own profile directory. Chrome refuses to share one between
    /// processes, so each run gets a fresh one and removes it at the end.
    profile: PathBuf,
}

/// The first Chrome or Chromium on `PATH`, if any.
pub fn find_browser() -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    std::env::split_paths(&path)
        .flat_map(|dir| BROWSER_NAMES.iter().map(move |name| dir.join(name)))
        .find(|candidate| candidate.is_file())
}

impl Renderer {
    /// Start a browser. The error is a sentence for the report: what was
    /// missing, or why the launch failed.
    pub async fn launch() -> Result<Renderer, String> {
        let executable =
            find_browser().ok_or_else(|| "no Chrome or Chromium found on PATH".to_string())?;
        let profile = std::env::temp_dir().join(format!("linkchecker-{}", std::process::id()));
        std::fs::create_dir_all(&profile)
            .map_err(|error| format!("could not create {}: {error}", profile.display()))?;
        let config = BrowserConfig::builder()
            .chrome_executable(&executable)
            .user_data_dir(&profile)
            .new_headless_mode()
            // CI containers routinely lack the user namespaces the sandbox
            // needs; the pages we render are our own.
            .no_sandbox()
            .arg("--disable-gpu")
            .launch_timeout(LAUNCH_TIMEOUT)
            .request_timeout(TIMEOUT)
            .build()?;
        let (browser, mut handler) = Browser::launch(config)
            .await
            .map_err(|error| format!("{} failed to start: {error}", executable.display()))?;
        // The handler pumps protocol messages; nothing works until it runs.
        let events = tokio::spawn(async move {
            while let Some(event) = handler.next().await {
                if event.is_err() {
                    break;
                }
            }
        });
        Ok(Renderer {
            browser: Mutex::new(browser),
            events,
            tabs: Semaphore::new(RENDER_CONCURRENCY),
            profile,
        })
    }

    /// Load `url` in a tab, wait for its links to stop changing, and return
    /// the markup as the browser sees it.
    pub async fn render(&self, url: &Url) -> Result<Rendered, Failure> {
        let _tab = self
            .tabs
            .acquire()
            .await
            .map_err(|_| Failure::Render("browser closed".into()))?;
        // One deadline over everything, so a tab that will not open or will
        // not close cannot hold the crawl. A tab abandoned by the deadline
        // stays open in the browser until the run ends; that is cheaper than
        // waiting on it.
        match tokio::time::timeout(RENDER_TIMEOUT, self.open_load_close(url)).await {
            Ok(rendered) => rendered,
            Err(_) => Err(Failure::Render(format!(
                "did not finish rendering within {}s",
                RENDER_TIMEOUT.as_secs()
            ))),
        }
    }

    async fn open_load_close(&self, url: &Url) -> Result<Rendered, Failure> {
        // A blank tab first: opening a tab straight on `url` waits for the
        // page's load event, which one hanging image or beacon can hold for
        // as long as it likes. Navigating afterwards returns at once.
        let page = self
            .browser
            .lock()
            .await
            .new_page("about:blank")
            .await
            .map_err(render_error)?;
        let outcome = load(&page, url).await;
        let _ = page.close().await;
        outcome
    }

    pub async fn shutdown(self) {
        let mut browser = self.browser.into_inner();
        let _ = browser.close().await;
        let _ = browser.wait().await;
        self.events.abort();
        let _ = std::fs::remove_dir_all(&self.profile);
    }
}

async fn load(page: &Page, requested: &Url) -> Result<Rendered, Failure> {
    page.goto(requested.as_str()).await.map_err(render_error)?;

    // "Finished loading" is when the document has been parsed and the app
    // has stopped adding links. The load event is deliberately not awaited:
    // it waits for every image and frame, and one hanging beacon can hold
    // it for longer than the whole crawl. Watching the link count is
    // framework-neutral and survives long-polling connections too. Past the
    // deadline, whatever is on the page is what gets checked.
    // Two signals, both from inside the page: how many links it has, and how
    // many network requests have finished (resource timing entries). A fetch
    // in flight shows up as a new entry when it lands, which restarts the
    // clock before the links it feeds appear.
    let deadline = Instant::now() + TIMEOUT;
    let mut last: Option<(u64, u64)> = None;
    let mut stable_since = Instant::now();
    while Instant::now() < deadline {
        let (state, links, resources): (String, u64, u64) = page
            .evaluate(
                "[document.readyState, \
                  document.querySelectorAll('a[href], link[href], img[src], script[src]').length, \
                  performance.getEntriesByType('resource').length]",
            )
            .await
            .map_err(render_error)?
            .into_value()
            .map_err(|error| Failure::Render(error.to_string()))?;
        let parsed = state != "loading";
        let settle = if state == "complete" {
            SETTLE
        } else {
            SETTLE_BEFORE_LOAD
        };
        let count = (links, resources);
        if parsed && last == Some(count) {
            if stable_since.elapsed() >= settle {
                break;
            }
        } else {
            last = Some(count);
            stable_since = Instant::now();
        }
        tokio::time::sleep(POLL).await;
    }

    let final_url = page
        .url()
        .await
        .map_err(render_error)?
        .and_then(|current| Url::parse(&current).ok())
        .unwrap_or_else(|| requested.clone());
    let html = page.content().await.map_err(render_error)?;
    Ok(Rendered { final_url, html })
}

fn render_error(error: chromiumoxide::error::CdpError) -> Failure {
    Failure::Render(error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_empty_path_finds_no_browser() {
        // Runs in-process, so PATH is left alone and only the search is tested.
        let empty = std::env::temp_dir().join(format!("linkchecker-empty-{}", std::process::id()));
        std::fs::create_dir_all(&empty).unwrap();
        let path = std::env::join_paths([&empty]).unwrap();
        let found = std::env::split_paths(&path)
            .flat_map(|dir| BROWSER_NAMES.iter().map(move |name| dir.join(name)))
            .find(|candidate| candidate.is_file());
        assert!(found.is_none());
        let _ = std::fs::remove_dir_all(&empty);
    }
}
