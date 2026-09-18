# Intent: show progress while the crawl runs

Status: agreed
Agreed with: Abdelhak Marouane
Date: 2026-09-18

## Layer 1 — human-owned, code-free

### Motivation

Between "Checking links under …" and the final report the tool prints
nothing. On a small site that is a few seconds. On earth.gov/ghgcenter, with
hundreds of pages and many slow external links that each take up to ten
seconds to give up on, it is many minutes of silence. A developer watching a
Docker run cannot tell whether the crawl is working through a big site or
stuck on one host, and a CI log that has gone quiet for ten minutes looks the
same as a hung job. Nobody should have to guess whether to wait or to kill it.

### Task

While the crawl runs, the tool reports where it is, on the error stream so the
report on the output stream stays exactly what it is today.

- Each time an in-scope page has been read for links, one line names the
  page and how many links it held.
- Every few seconds, one summary line states how many pages have been
  crawled, how many links checked, how many links are still queued, and one
  URL currently in flight, so a stall points at the host causing it.
- Progress prints whether or not a person is watching: a Docker run, a
  terminal, and a GitHub Actions log all get it. No flag, no detection, no
  new Action input.
- The final report is unchanged, line for line. Anything that reads the
  tool's standard output today keeps working.

### Completion criteria

- A run against a multi-page site prints one progress line per crawled page
  and at least one summary line before the report, all on the error stream,
  and the standard output is identical to today's.
- On a site where one link stalls for its full timeout, the summary line
  during the stall names that link.
- The exit code and the report's wording and cut-off rules are unchanged.
- The end-to-end tests that assert on the report still pass without change.

### Non-goals

- No progress bar, no percentage. The crawl does not know how big the site
  is until it is done.
- No timing per link, no slow-link report. Slow pages are out of scope per
  the v1 note, and this note does not reopen that.
- No configuration of the interval, the verbosity, or whether progress prints
  at all.
- No change to what counts as broken or how it is reported.
- No progress annotations in the GitHub UI (`::group::`, `::notice::`); the
  log is plain text on both streams as it is today.

### Open questions

- The summary interval. Five seconds is the proposal: frequent enough that a
  ten-second stall shows up while it is happening, rare enough that a
  ten-minute run adds about a hundred lines to a CI log.

## Layer 2 — implementation sketch (as of 94772b8)

- Rough plan:
  - The crawl loop in `crawl::crawl` already has every number: the report's
    `pages_crawled` and `links_checked`, the queue length, and `inflight`.
    It gains a `tokio::time::interval` and a `tokio::select!` between the
    next finished job and the next tick. The tick prints the summary; a
    finished page prints its own line where `pages_crawled` is incremented.
  - "One URL in flight": the in-flight set is a `FuturesUnordered` of opaque
    futures, so the loop keeps a small side list of in-flight URLs (a
    `Vec<Url>` or `HashSet`, at most `CONCURRENCY` entries), inserting on
    push and removing on completion. The summary prints the oldest one, which
    is the likeliest stall.
  - Printing goes through `eprintln!`. `main` already owns stdout for the
    report; nothing there changes.
  - Test: the stub server gets a route that sleeps past the request timeout
    before answering (a `Reply` variant with a delay), and the end-to-end
    test captures stderr, asserts a `page` line per crawled page, a summary
    line, and that the stalled URL appears in a summary. Existing tests read
    `output.stdout` only and are untouched.
- Known gotchas discovered while scoping:
  - `tokio::select!` on `inflight.next()` when `inflight` is empty returns
    `None` immediately and would spin against the ticker; the loop's exit
    condition (queue empty and nothing in flight) has to be checked before
    selecting, not inferred from `None`.
  - The rendering path runs inside a job, so a page being rendered is "in
    flight" for the summary like any other, which is what we want: a slow
    render shows up as a stall on that URL.
  - Interleaving: per-page lines and summary lines share stderr; each is a
    whole line ending in a newline, printed from the single crawl task, so
    they cannot tear. Nothing else in the binary writes to stderr during a
    crawl except a browser-launch failure, which is a single line too.
  - Tests that count `linked from` occurrences or compare stdout are safe
    because progress never touches stdout. A test that compares stderr to
    empty would break; none does today.
  - `git grep eprintln` before starting: `main` prints usage errors to stderr
    before the crawl begins; keep that path as is.
