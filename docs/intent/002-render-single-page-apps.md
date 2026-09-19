# Intent: render single-page apps so their links can be crawled

Status: agreed
Agreed with: Abdelhak Marouane
Date: 2026-09-18
Issue: https://github.com/amarouane-ABDELHAK/rust-linkchecker/issues/2

## Layer 1 — human-owned, code-free

### Motivation

The first site we pointed the Action at, explore.fm.odsi.io, is a single-page
application. The markup its server sends is a one-kilobyte shell: a script, a
stylesheet, a handful of icons, and not a single link to another page. Every
route the site has exists only after a browser has run that script. The
checker crawled exactly what the markup offered, reported 8 links on 1 page,
and went green, while the site's real pages and the links on them were never
looked at.

That is a green build that proves nothing, which is worse than no check at
all. The organization deploys more sites like this one, so the tool has to be
able to see the page a visitor sees, not only the page the server sends.

`vision.md` used to fence off JavaScript rendering. That fence has been moved
(see the amended Direction and Constraints); this note is the first task under
the new line.

### Task

When a crawled page's served markup yields no pages to follow, the checker
loads that page in a headless browser, waits for it to finish loading, and
reads the links from the rendered result instead. From there the crawl goes on
exactly as before: in-scope pages are followed, everything else is checked
once, and every broken link names the page it was found on.

- Rendering is a fallback. A page whose plain markup already contains at least
  one in-scope page link is never rendered. Plain sites behave exactly as
  today and never start a browser.
- A rendered page is judged alive or dead by its HTTP status, exactly like
  every other link. What the script painted does not change the verdict.
- The browser is the one already on the runner. The tool does not download or
  bundle one. When no browser can be found, the checker says so once, at the
  top of the output, and carries on with the markup-only behavior.
- A page that fails to render (the browser crashes, or loading never settles)
  is treated like a page that failed to load: reported as broken with the
  reason, and the crawl continues.
- Nothing becomes configurable. No new Action input, no flag.

### Completion criteria

- Pointed at explore.fm.odsi.io, the checker reports more than one page
  crawled and lists the rendered routes' assets and outbound links among the
  links checked.
- Pointed at a plain HTML site, the output is identical to today's and no
  browser process is started.
- A test site whose shell links to nothing, but whose script inserts anchors
  to three in-scope pages, one dead, produces a report naming the dead page
  and the page it was linked from, and exits non-zero.
- On a machine with no browser installed, the run against that same test site
  prints one line saying rendering is unavailable, reports 1 page as today,
  and does not crash.
- The release binary still runs on a GitHub-hosted Ubuntu runner with nothing
  installed beyond what the image ships.

### Non-goals

Everything in `vision.md` still applies. For this task specifically:

- No judging a route by its rendered content. A catch-all 200 shell keeps a
  wrong internal route invisible to this tool, and this note does not try to
  fix that.
- No clicking, scrolling, typing, or waiting for user-driven navigation.
  Links are anchors and asset references present in the DOM once the page has
  loaded.
- No logging in. Routes behind authentication render as whatever the app
  shows an anonymous visitor.
- No rendering of out-of-scope pages, and no rendering of pages that already
  had links in their markup.
- No configuration: not the render timeout, not a browser path, not an on/off
  switch.
- No downloading a browser at runtime. The Action's footprint stays one
  static binary.

### Open questions

- ~~How long to wait for a page to "finish loading" before reading its
  links.~~ Resolved during implementation: after the page's load event, the
  checker watches the number of links in the document and reads them once
  that number has held still for half a second, under the same 10-second cap
  every request has. This does not depend on the network going quiet, so a
  long-polling connection cannot stall it.

## Layer 2 — implementation sketch (as of e6d6eee)

- Rough plan:
  - A new `render` module owns the browser: find a Chrome or Chromium
    executable on `PATH` (and the usual Linux install paths), launch it
    headless once per run, open one tab per page to render, wait for load,
    and return the rendered DOM as a string. `chromiumoxide` (0.9, async,
    tokio) fits the existing runtime; `headless_chrome` (1.0) is the sync
    alternative if the async crate proves awkward.
  - `crawl::crawl` gains one decision after `extract::links`: if the page is
    in scope and none of the extracted links are in-scope HTML candidates,
    hand the final URL to the renderer and run `extract::links` again on the
    rendered DOM. Everything downstream (dedup, queue, referrer) is unchanged.
  - The browser is started lazily on first need, so plain sites never pay for
    it, and shut down when the crawl ends.
  - `Report` gains a flag for "rendering was needed but no browser was found",
    which `report::render` prints once at the top.
  - A new `Failure::Render(String)` variant covers browser-side failures, with
    label `render`.
  - End-to-end coverage: the stub server in `tests/common` gains a reply whose
    body is a shell plus an inline script that inserts anchors. Tests that
    need a browser skip themselves when none is on `PATH`, so the suite still
    passes on a laptop without Chrome; the release workflow runs on
    `ubuntu-latest`, which has Chrome preinstalled, so CI does exercise them.
- Found during implementation (the reviewer should know these):
  - "No pages to follow" alone is the wrong trigger: every script-free leaf
    page (`<p>bottom</p>`) matches it, so a plain site would start a browser
    for each leaf. The trigger is "no in-scope anchor to another page **and**
    the markup contains a `<script>`". A page without script cannot grow
    links, so nothing is lost. This tightens the Layer 1 rule without
    changing its route.
  - `chromiumoxide` defaults to one fixed profile directory for every
    process, and Chrome refuses to start when another instance holds its
    lock. Each run gets its own directory under the system temp dir and
    removes it on shutdown.
  - explore.fm.odsi.io itself, rendered, exposes exactly one anchor (to
    nasa.gov). Its navigation is a Sign In button and button-role elements,
    and its routes sit behind authentication. The checker now covers the
    rendered page's assets and that outbound link (17 links, up from 8), but
    the first completion criterion, more than one page crawled there, is not
    reachable without clicking or logging in, both non-goals. The stub-server
    tests prove the rendering path; that site does not exercise it further.
  - Found after the v0.2.0 release: a cold first start of Chrome on a fresh
    GitHub runner takes about eleven seconds (a warm one, about one). With
    the browser's launch timeout set to the 10-second request timeout, the
    consumer's first real run reported "Rendering unavailable: Timeout while
    resolving websocket URL" and fell back to the markup. The launch timeout
    is its own constant, 60 seconds, paid once per run and only when a page
    needs rendering. The per-request timeout is unchanged.
  - Found after v0.2.1, unrelated to rendering but recorded here so it is
    not lost: the HTTP client sent `Accept: */*`, its default, and a Rails
    site (earth.jpl.nasa.gov) answers 404 to that while answering 200 to a
    browser's `Accept: text/html,...`. The client now sends the browser
    value on every request. One-line fix, no intent note of its own.
  - Found after v0.3.0, thanks to the progress lines: a full crawl of
    earth.gov/ghgcenter sat for minutes on one URL with nothing else in
    flight. That URL serves the site's HTML shell, so it was being rendered,
    and only the load step of a render had a deadline; opening the tab and
    closing it did not, and Chromium can hang on either. The whole render is
    now under one 20-second deadline. The hang did not reproduce locally, in
    Docker or native, so this is a guarantee of progress rather than a fix
    for whatever Chromium was doing.
- Known gotchas discovered while scoping:
  - `Html::parse_document` in `extract` cannot run scripts, so "no in-scope
    links in the markup" is the only signal available before rendering.
    Deciding on "has a `<div id=root>`" or similar would be guessing at the
    framework; the link-count rule is framework-neutral.
  - The rendered DOM must be extracted against the tab's final URL, not the
    requested one, for the same redirect reason the plain path already
    handles.
  - Rendering re-requests the page's assets through the browser. Those
    requests are the browser's, not the checker's, and must not be counted as
    checked links or reported as failures; the checker still checks each
    asset once itself through `reqwest`.
  - Chrome needs `--no-sandbox` inside many CI containers and a writable
    temporary profile directory. Both are launch flags, not configuration.
  - The musl build is unaffected: the browser is a separate process spoken to
    over a socket, not a linked library.
  - Concurrency: `CONCURRENCY` is 16 in-flight requests. Sixteen tabs at once
    is heavy on a 2-core runner; rendering should run through a smaller
    semaphore (4) independent of the request limit.
  - `explore.fm.odsi.io` returns the shell for every path. Tests must not
    assume a rendered route can be found dead; the third criterion above uses
    a stub server where the dead page really returns 404.
