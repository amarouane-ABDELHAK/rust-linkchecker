# Intent: v1 link checker, shipped as a GitHub Action

Status: accepted
Agreed with: Abdelhak Marouane
Date: 2026-09-18

## Layer 1 — human-owned, code-free

### Motivation

When a site is deployed, nothing currently tells us that a link on it stopped
working. Broken links are found by a visitor, or never. We want a deployment to
be able to fail loudly on the same signal, in the same run that produced it,
without anyone remembering to check by hand.

### Task

A command-line tool takes one starting URL and crawls the site from there.

- It follows pages **underneath the starting path** and no further. A link
  outside that prefix — another site, or a different part of the same site — is
  checked for liveness but never crawled.
- On each page it checks anchor links **and asset references**: images,
  scripts, stylesheets. A missing image is a broken link.
- A link is **broken** if the response is in the 400 or 500 range, or if it
  never got a response at all: timeout, DNS failure, refused connection.
  Redirects are followed and judged on where they land, so a link that
  redirects to a live page passes and one that redirects into a 404 fails. A
  redirect loop, or too many hops, is broken.
- Output names each broken link with its status and **the page that linked to
  it**, so the developer knows what to edit without re-crawling. It prints the
  first 100 and states the true total.
- The tool exits non-zero when anything is broken, so the workflow goes red.
- Fixed behavior, not configurable: a 10-second request timeout, 16 requests in
  flight, non-HTTP schemes (`mailto:`, `tel:`, `javascript:`, `data:`) skipped
  silently, and fragments dropped when deciding whether two links are the same
  URL. `/page` and `/page#intro` are one request; `/page` and `/page/` are two.

The same repository ships the Action itself: an `action.yml` taking the
starting URL as an input, and a release workflow producing the Linux x86_64
binary it runs. Another repository consumes it with `uses:`.

### Completion criteria

- Pointed at a site with known broken links, the tool reports exactly those and
  exits non-zero; pointed at a clean site, it reports none and exits zero.
- A page linked only from a subpage several levels below the start is crawled;
  a page outside the start prefix is not.
- A broken image and a broken anchor are both reported.
- A URL that appears on many pages is requested once, not once per page.
- A run against a site with more than 100 broken links prints 100 and reports
  the real total.
- Another repository can add the Action with `uses:` and a `base-url:` input
  and get a red build on a broken link, using a tagged release.
- The binary runs the same way by hand: `linkchecker <url>`.

### Non-goals

Everything fenced off in `vision.md` still applies — no JavaScript rendering,
no authenticated crawling, no library API, no ignore list, no crates.io. For
this task specifically:

- No configuration beyond the starting URL. Not the 100-line cap, not the
  concurrency, not the timeout. Every input added to the Action is permanent.
- No output format other than the human-readable one above. No JSON, no
  annotations on the GitHub run, no PR comments.
- No caching or reuse of results between runs.
- No reporting of anything that is not a broken link. Slow pages, redirect
  chains, and mixed content are out of scope for v1.

## Layer 2 — implementation sketch (as of 828dfd1)

At that commit the tree holds only the playbook files — no Rust source, no
workflows. Everything below is new code, so there is nothing here to go stale
except the plan itself.

- Rough plan: one binary crate. A work queue seeded with the start URL; each
  page fetched, parsed for links, in-prefix HTML pages pushed back onto the
  queue, everything else checked once. A set of already-seen URLs guards both
  re-crawling and re-checking. Results collect into a list of failures carrying
  the URL, the reason, and the referring page; the exit code falls out of
  whether that list is empty.
- `action.yml` as a composite action that downloads the tagged binary and runs
  it, plus a release workflow that builds for `x86_64-unknown-linux-gnu` and
  attaches the binary to the tag.
- Gotchas worth knowing before starting:
  - "Under the starting path" is a string-prefix trap: `/start` must not match
    `/started`. Compare path segments, not characters.
  - Only HTML gets parsed for more links. A PNG under the prefix is checked and
    not opened — decide that from the response's content type, not the file
    extension, since a URL with no extension is the common case.
  - Cheap liveness checks (HEAD) are not universally supported; servers answer
    405 or lie. A fallback to a real GET is needed, or the report fills with
    false failures.
  - Redirect following is a client setting with a hop limit, and exhausting the
    limit has to surface as a broken link rather than an error that kills the
    run.
  - Relative URLs resolve against the page they were found on, including any
    redirect that page went through — resolving against the original request
    URL gives wrong absolute links.
  - A crawl failure must never take down the run: one unreachable page is a
    result to report, not a panic.
  - The binary runs on the runner's glibc. If the release is built in a
    different environment than it runs in, a static musl build avoids a class
    of loader failures that only appear in CI.
