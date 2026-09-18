# Vision

## What this project is

A Rust command-line tool that crawls a website from a given starting URL and
reports broken links. Given `https://example.com/start`, it walks every page
underneath that path — `/start/about`, `/start/visualization.png`, and so on —
and checks every link it finds on those pages, including links pointing off to
other sites.

## Who it is for

Developers in this organization who run it as a post-deployment check. They do
not invoke it by hand: they add this repository's Action to a workflow with
`uses: <org>/rust-linkchecker@<tag>` and pass the starting URL as an input.

## Goals

- After a deployment, the workflow run goes red when the deployed site has a
  broken link, and green when it does not.
- The developer reads the output and knows which links are broken without
  reproducing the crawl locally.
- Any response in the 400 or 500 range counts as a dead link, and so does a
  link that fails before it gets a status at all — a timeout, a DNS failure, a
  refused connection. If the tool could not confirm the link is alive, it is
  reported.
- Output prints the first 100 broken links along with the total number found,
  so a site with a thousand failures still produces a readable run.

## Non-goals

- **Crawling beyond the starting path prefix.** Off-site and out-of-prefix
  links are checked for liveness but never followed.
- **Judging a page by what it renders.** A page is alive or dead by its HTTP
  status, never by what its script painted. On a site that answers every path
  with a 200 shell, a wrong internal route cannot be caught by this tool.
- **Interacting with a page.** No clicking, no form filling, no scrolling to
  reveal more. Links are what the page exposes as links once it has loaded.
- **Anything beyond link health** — no SEO auditing, accessibility checks,
  spell-checking, or performance measurement.
- **Authenticated crawling.** No logins, cookies, or private sites.
- **A reusable library API.** This is a CLI. No public crate interface is
  maintained for other code to call.
- **Persisting state between runs.** No caches, no databases, no historical
  trend reports.
- **Fixing or rewriting** the broken links it finds. It reports; humans fix.
- **An ignore list or allowlist.** Every link found gets checked. There is no
  mechanism for exempting a known-flaky URL from the report.
- **Publishing to crates.io.** This stays an internal tool for now.

## Constraints

- The deliverable is a **GitHub Action** — an `action.yml` in this repository
  alongside the Rust source, plus the tagged release binary it runs. Consuming
  repositories reference it with `uses:`, so the Action's inputs and its
  release tags are a maintained public interface: renaming an input or moving a
  tag breaks other repositories' workflows.
- **Linux x86_64 only.** No macOS, no ARM, no Windows.
- **Rendering needs a browser on the runner.** The tool does not ship or
  download one. GitHub-hosted Ubuntu runners have Google Chrome preinstalled;
  a runner without it gets a clear message and rendering is skipped, not a
  crash.
- **Solo maintainer.** Design for one person to hold the whole thing in their
  head.
- **Earlier is better.** Shipping something that works beats shipping something
  complete.
- **No rate limiting required.** The tool crawls infrastructure we own, so
  politeness throttling and `robots.txt` handling are not requirements.
- **MIT licensed.**

## Direction

Version one takes a single starting path and checks the links on every subpath
beneath it. The output stays deliberately small: the first 100 broken links plus
a total count.

It stays internal to the organization — there is no plan to publish it, so no
external API or distribution surface needs protecting.

Some of the sites we deploy are single-page applications: the markup the server
sends is an empty shell, and every link on the site exists only after a browser
has run the script. For those sites the tool renders the page in a headless
browser and reads the links from the result. Rendering is a fallback, not the
default: a page whose served markup already yields pages to follow is never
rendered, so plain sites keep their speed and gain no new dependency.

Growth should come from making that one job more reliable — not from widening
what the tool does. A change that helps a red build point at the right URL
faster is with the grain. A change that adds a second kind of check, a second
output surface, or a second way to be configured needs a reason that traces back
to the goals above — and a new Action input is a permanent commitment, not a
convenience.
