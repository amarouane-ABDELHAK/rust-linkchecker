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
/// How long the set of links on a page must stop changing before it counts
/// as finished loading.
const SETTLE: Duration = Duration::from_millis(500);
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
        let page = self
            .browser
            .lock()
            .await
            .new_page(url.as_str())
            .await
            .map_err(render_error)?;
        let outcome = tokio::time::timeout(TIMEOUT, load(&page, url)).await;
        let _ = page.close().await;
        match outcome {
            Ok(rendered) => rendered,
            Err(_) => Err(Failure::Render(format!(
                "did not finish loading within {}s",
                TIMEOUT.as_secs()
            ))),
        }
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
    page.wait_for_navigation().await.map_err(render_error)?;

    // "Finished loading" is when the app has stopped adding links. Watching
    // the link count is framework-neutral and survives long-polling
    // connections that would keep a network-idle signal from ever firing.
    let mut last: Option<u64> = None;
    let mut stable_since = Instant::now();
    loop {
        let count: u64 = page
            .evaluate(
                "document.querySelectorAll('a[href], link[href], img[src], script[src]').length",
            )
            .await
            .map_err(render_error)?
            .into_value()
            .map_err(|error| Failure::Render(error.to_string()))?;
        if last == Some(count) {
            if stable_since.elapsed() >= SETTLE {
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
