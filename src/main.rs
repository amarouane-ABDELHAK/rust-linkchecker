//! A link checker for a deployed site: crawl from a starting URL, report every
//! link underneath it that is not alive, and fail the build if any are.

mod check;
mod crawl;
mod extract;
mod report;
mod scope;

use std::process::ExitCode;

use url::Url;

use scope::Scope;

const USAGE: &str = "usage: linkchecker <url>

Crawls every page underneath <url> and checks the links on them. Links
outside that path — other sites, other parts of this one — are checked but
never followed.

Exits 0 when every link is alive, 1 when any is broken, 2 when it could not
start.";

#[tokio::main]
async fn main() -> ExitCode {
    let start = match start_url() {
        Ok(url) => url,
        Err(message) => {
            eprintln!("{message}\n\n{USAGE}");
            return ExitCode::from(2);
        }
    };

    let client = match check::client() {
        Ok(client) => client,
        Err(error) => {
            eprintln!("could not start an HTTP client: {error}");
            return ExitCode::from(2);
        }
    };

    let scope = Scope::new(start);
    println!("Checking links under {}\n", scope.start());

    let report = crawl::crawl(&scope, &client).await;
    print!("{}", report::render(&report));

    if report.is_clean() {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}

/// The one thing this tool is configured with.
fn start_url() -> Result<Url, String> {
    let mut args = std::env::args().skip(1);
    let Some(argument) = args.next() else {
        return Err("missing the starting URL".to_string());
    };
    if argument == "-h" || argument == "--help" {
        println!("{USAGE}");
        std::process::exit(0);
    }
    if args.next().is_some() {
        return Err("expected exactly one starting URL".to_string());
    }

    let url =
        Url::parse(&argument).map_err(|error| format!("{argument:?} is not a URL: {error}"))?;
    if !scope::is_checkable(&url) {
        return Err(format!("{argument:?} is not an http or https URL"));
    }
    Ok(url)
}
