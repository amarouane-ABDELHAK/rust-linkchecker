# rust-linkchecker

Crawls a deployed site from a starting URL and fails the run if any link is
broken. Built to sit in a GitHub Actions workflow straight after a deployment.

## Use it in a workflow

```yaml
- uses: amarouane-ABDELHAK/rust-linkchecker@v0.2.0
  with:
    base-url: https://example.com/start
```

The step fails when anything is broken, and prints what and where:

```
Checked 412 links across 38 pages.

BROKEN (127 total, showing first 100):

        404  https://example.com/start/old-page
             linked from https://example.com/start/about
        500  https://api.other.com/v1/status
             linked from https://example.com/start/docs
        DNS  https://gone.example.net/
             linked from https://example.com/start/

Error: 127 broken links
```

## Use it by hand

The Action runs the same binary you can run yourself:

```
linkchecker https://example.com/start
```

Exit codes: `0` everything alive, `1` something broken, `2` it could not start.

## What it does

- Crawls every page **underneath the starting path**. Given
  `https://example.com/start`, it follows `/start/about` and
  `/start/guide/deep`, but not `/other` and not `/started`.
- Links **outside** that path — other sites, other parts of the same site —
  are checked for liveness and never followed.
- Checks anchors **and assets**: `<a href>`, `<img src>`, `<script src>`,
  `<link href>`. A missing image is a broken link.
- Treats as broken: any 4xx or 5xx, a timeout, a DNS failure, a refused
  connection, and a redirect loop. Redirects are followed and judged on where
  they land, so a link redirecting to a live page passes.
- Requests each distinct URL once, however many pages link to it.
- Renders single-page apps. A page whose markup has a script but no link to
  another page is loaded in the Chrome or Chromium found on `PATH`, and the
  links its script produced are crawled like any others. Plain sites never
  start a browser. Without a browser the run says so once and checks the
  markup as served.
- Prints the first 100 broken links and always states the true total.

Fixed, deliberately not configurable: a 10-second request timeout, 16 requests
in flight, 4 pages rendering at once, `mailto:`/`tel:`/`javascript:`/`data:`
links skipped, and `#fragments` dropped when deciding whether two links are
the same URL.

## What it does not do

No clicking or logging in to reach a page (a route only a button leads to is
never found), no judging a page by what it renders (a site that answers every
path with a 200 shell hides its dead routes from this tool), no authenticated
crawling, no ignore list, no JSON output, no caching between runs, and no
fixing of the links it finds. See
[vision.md](vision.md) for why, and `docs/intent/` for the note each change is
reviewed against.

## Development

```
cargo test     # unit tests, plus end-to-end tests against a stub server
cargo build --release
```

The end-to-end test that renders a single-page app skips itself when no
`google-chrome` or `chromium` is on `PATH`. On macOS, put a wrapper script on
`PATH` that execs `/Applications/Google Chrome.app/Contents/MacOS/Google Chrome`.
GitHub-hosted Ubuntu runners have Chrome preinstalled, so CI runs it.

Releases are cut by pushing a `v*` tag: CI builds a statically linked
`x86_64-unknown-linux-musl` binary and attaches it to the release, which is
what `action.yml` downloads. The tag a workflow references with `uses:` must
have a release of exactly that name — so pin an exact version rather than a
moving major tag, unless you also republish the asset when the tag moves.

## License

MIT
