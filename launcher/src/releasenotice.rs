//! A notice that a newer stable version of TSW-Forever exists. **It is not an updater.**
//!
//! Product decision (2026-11): Windows has no automatic update. The launcher asks the official GitHub release page which version is the latest stable release and, only when
//! that version is really newer than the installed one, shows a quiet notice with a button that opens the official release page in the default browser.
//! Nothing is downloaded, installed or executed; no administrator rights are needed; offline, a refused or an unexpected answer end silently.
//!
//! How the question is asked: `HEAD <repo>/releases/latest` WITHOUT following the redirect. GitHub answers with a redirect to `<repo>/releases/tag/<tag>` of the
//! newest release that is neither a draft nor a pre-release, so pre-releases and testing builds are never offered. Only the `Location` header is read and it is
//! checked strictly: it must be exactly `<repo>/releases/tag/vMAJOR.MINOR.PATCH` (digits only: a tag with any suffix such as `-rc1` or `-testing` is ignored). The
//! address that the button opens is NOT taken from the answer; it is built from the repository constant and the validated version.
//!
//! The answer is not signed, so it is treated as untrusted text that can at worst show a wrong notice with a link to our own release page.

use crate::channel::Channel;
use std::process::{Command, Stdio};

/// Largest header block that is read.
const MAX_HEADERS: usize = 16 * 1024;

/// A version number with exactly three numeric parts.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Version(pub u32, pub u32, pub u32);

impl Version {
    /// `MAJOR.MINOR.PATCH`, digits only, no sign, no leading zeros (except a single `0`), each part at most nine digits.
    pub fn parse(text: &str) -> Option<Version> {
        let mut parts = text.split('.');
        let mut next = || {
            let p = parts.next()?;
            let ok = !p.is_empty() && p.len() <= 9 && p.bytes().all(|b| b.is_ascii_digit()) && (p == "0" || !p.starts_with('0'));
            if ok {
                p.parse::<u32>().ok()
            } else {
                None
            }
        };
        let v = Version(next()?, next()?, next()?);
        if parts.next().is_some() {
            return None;
        }
        Some(v)
    }
}

impl std::fmt::Display for Version {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}.{}.{}", self.0, self.1, self.2)
    }
}

/// What the window shows: the newer version and the official page to open.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Notice {
    pub version: String,
    pub url: String,
}

/// The address that is asked for the latest stable release.
pub fn latest_url(repo_url: &str) -> String {
    format!("{repo_url}/releases/latest")
}

/// The official page of one release, built from the repository constant (never copied from an answer).
pub fn release_page(repo_url: &str, version: Version) -> String {
    format!("{repo_url}/releases/tag/v{version}")
}

/// The version a header block names as the latest release: the last status line must be a redirect and its `Location` exactly `<repo>/releases/tag/vX.Y.Z`.
pub fn parse_latest(headers: &str, repo_url: &str) -> Option<Version> {
    let mut redirect = false;
    let mut location: Option<&str> = None;
    for line in headers.lines() {
        let line = line.trim_end_matches('\r');
        if line.len() >= 5 && line[..5].eq_ignore_ascii_case("HTTP/") {
            // a new status line starts a new block (for example the answer of a proxy before the real one)
            redirect = matches!(line.split_whitespace().nth(1), Some("301" | "302" | "303" | "307" | "308"));
            location = None;
        } else if let Some((name, value)) = line.split_once(':') {
            if name.eq_ignore_ascii_case("location") {
                location = Some(value.trim());
            }
        }
    }
    if !redirect {
        return None;
    }
    let tag = location?.strip_prefix(repo_url)?.strip_prefix("/releases/tag/v")?;
    Version::parse(tag)
}

/// How the header block is fetched. Production: [`CurlHead`]; tests: a fake.
pub trait HeadFetch {
    fn head(&self, url: &str) -> Result<String, String>;
}

/// The notice for a program of channel `build` and version `installed`, or `None` (also: offline, not stable, not newer, an answer that does not look right).
pub fn check(fetch: &dyn HeadFetch, build: Channel, repo_url: &str, installed: &str) -> Option<Notice> {
    // testing builds are private and never told about stable versions
    if build != Channel::Stable {
        return None;
    }
    let installed = Version::parse(installed)?;
    let headers = fetch.head(&latest_url(repo_url)).ok()?;
    let latest = parse_latest(&headers, repo_url)?;
    (latest > installed).then(|| Notice { version: latest.to_string(), url: release_page(repo_url, latest) })
}

/// `curl` as the fetcher: https only, no redirect is followed, short timeouts, a hard output limit, no console window.
pub struct CurlHead {
    pub curl: std::path::PathBuf,
}

impl HeadFetch for CurlHead {
    fn head(&self, url: &str) -> Result<String, String> {
        if !url.starts_with("https://") || url.chars().any(|c| !(c.is_ascii_alphanumeric() || matches!(c, ':' | '/' | '.' | '-' | '_'))) {
            return Err("invalid address".into());
        }
        let mut cmd = Command::new(&self.curl);
        cmd.args(["--silent", "--head", "--max-redirs", "0", "--proto", "=https", "--connect-timeout", "8", "--max-time", "15", "--"]).arg(url).stdin(Stdio::null()).stderr(Stdio::null());
        crate::winnative::no_window(&mut cmd);
        let out = cmd.output().map_err(|e| format!("cannot start curl: {e}"))?;
        if !out.status.success() || out.stdout.len() > MAX_HEADERS {
            return Err("no usable answer".into());
        }
        Ok(String::from_utf8_lossy(&out.stdout).into_owned())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;

    const REPO: &str = "https://github.com/Glaubenskrieger/TSW-Forever";

    fn redirect(to: &str) -> String {
        format!("HTTP/2 302 \r\ncontent-type: text/html\r\nlocation: {to}\r\nserver: github.com\r\n\r\n")
    }

    struct Fake(Result<String, String>, RefCell<Vec<String>>);
    impl Fake {
        fn answer(h: &str) -> Fake {
            Fake(Ok(h.to_string()), RefCell::new(vec![]))
        }
    }
    impl HeadFetch for Fake {
        fn head(&self, url: &str) -> Result<String, String> {
            self.1.borrow_mut().push(url.to_string());
            self.0.clone()
        }
    }

    fn notice(headers: &str, build: Channel, installed: &str) -> Option<Notice> {
        check(&Fake::answer(headers), build, REPO, installed)
    }

    #[test]
    fn versions_are_strict() {
        assert_eq!(Version::parse("0.3.1"), Some(Version(0, 3, 1)));
        assert_eq!(Version::parse("10.20.30"), Some(Version(10, 20, 30)));
        for bad in ["", "1", "1.2", "1.2.3.4", "1.2.3-rc1", "1.2.3+b", "v1.2.3", "01.2.3", "1.02.3", "1.2.03", "1..3", "-1.2.3", "1.2.x", "1.2.3 ", " 1.2.3", "1.2.4294967296", "1.2.9999999999"] {
            assert_eq!(Version::parse(bad), None, "{bad:?}");
        }
        assert!(Version(0, 10, 0) > Version(0, 9, 9), "numeric, not textual");
    }

    #[test]
    fn a_newer_stable_release_gives_a_notice_with_the_official_page() {
        let n = notice(&redirect(&format!("{REPO}/releases/tag/v0.4.0")), Channel::Stable, "0.3.1").expect("newer");
        assert_eq!(n, Notice { version: "0.4.0".into(), url: format!("{REPO}/releases/tag/v0.4.0") });
    }

    #[test]
    fn the_same_or_an_older_version_gives_no_notice() {
        for installed in ["0.4.0", "0.4.1", "1.0.0"] {
            assert_eq!(notice(&redirect(&format!("{REPO}/releases/tag/v0.4.0")), Channel::Stable, installed), None, "{installed}");
        }
    }

    #[test]
    fn pre_releases_and_odd_tags_are_never_offered() {
        for tag in ["v0.4.0-rc1", "v0.4.0-testing", "v0.4.0-beta.2", "0.4.0", "v0.4", "v0.4.0.1", "vx", "v0.4.0/", "v0.4.0?x=1", "v0.4.0#a", "v0.4.0%20", "v00.4.0", "V0.4.0"] {
            assert_eq!(notice(&redirect(&format!("{REPO}/releases/tag/{tag}")), Channel::Stable, "0.3.1"), None, "{tag}");
        }
    }

    #[test]
    fn only_our_own_release_page_is_accepted_as_the_answer() {
        for to in [
            "https://github.com/someone-else/TSW-Forever/releases/tag/v9.9.9",
            "https://github.com/Glaubenskrieger/TSW-Forever-Evil/releases/tag/v9.9.9",
            "https://github.com/Glaubenskrieger/TSW-Forever/releases/download/v9.9.9/setup.exe",
            "https://evil.example/Glaubenskrieger/TSW-Forever/releases/tag/v9.9.9",
            "http://github.com/Glaubenskrieger/TSW-Forever/releases/tag/v9.9.9",
            "https://github.com/Glaubenskrieger/TSW-Forever/releases/tag/v9.9.9/../../x",
            "https://github.com/login?return_to=https%3A%2F%2Fgithub.com%2FGlaubenskrieger%2FTSW-Forever%2Freleases%2Flatest",
            "/Glaubenskrieger/TSW-Forever/releases/tag/v9.9.9",
        ] {
            assert_eq!(notice(&redirect(to), Channel::Stable, "0.3.1"), None, "{to}");
        }
    }

    #[test]
    fn the_button_address_is_built_from_the_validated_version_not_copied() {
        // the validated version is the only thing taken from the answer; the address is built from the constant
        let n = notice(&redirect(&format!("{REPO}/releases/tag/v1.2.3")), Channel::Stable, "0.3.1").unwrap();
        assert_eq!(n.url, release_page(REPO, Version(1, 2, 3)));
        assert!(n.url.starts_with(&format!("{REPO}/releases/tag/v")));
        assert!(n.url.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, ':' | '/' | '.' | '-')));
    }

    #[test]
    fn only_a_redirect_counts() {
        let ok = format!("location: {REPO}/releases/tag/v0.4.0\r\n");
        for status in ["HTTP/2 200", "HTTP/2 404", "HTTP/2 500", "HTTP/1.1 304", "HTTP/2 200 OK"] {
            assert_eq!(notice(&format!("{status}\r\n{ok}\r\n"), Channel::Stable, "0.3.1"), None, "{status}");
        }
        for status in ["HTTP/1.1 301 Moved Permanently", "HTTP/2 302", "HTTP/2 303", "HTTP/2 307", "HTTP/2 308"] {
            assert!(notice(&format!("{status}\r\n{ok}\r\n"), Channel::Stable, "0.3.1").is_some(), "{status}");
        }
        assert_eq!(notice("HTTP/2 302\r\n\r\n", Channel::Stable, "0.3.1"), None, "a redirect without a location");
        assert_eq!(notice("", Channel::Stable, "0.3.1"), None);
        assert_eq!(notice("garbage", Channel::Stable, "0.3.1"), None);
    }

    #[test]
    fn header_names_are_case_insensitive_and_the_last_block_counts() {
        let h = format!("HTTP/1.1 200 Connection established\r\n\r\nHTTP/2 302\r\nLocation: {REPO}/releases/tag/v0.4.0\r\n\r\n");
        assert!(notice(&h, Channel::Stable, "0.3.1").is_some());
        // the earlier block's location must not leak into a later non-redirect block
        let h = format!("HTTP/1.1 302\r\nlocation: {REPO}/releases/tag/v0.4.0\r\n\r\nHTTP/2 200\r\n\r\n");
        assert_eq!(notice(&h, Channel::Stable, "0.3.1"), None);
        // plain \n line ends work too
        assert!(notice(&format!("HTTP/2 302\nLOCATION: {REPO}/releases/tag/v0.4.0\n"), Channel::Stable, "0.3.1").is_some());
    }

    #[test]
    fn a_testing_build_is_never_told_about_stable_versions_and_asks_nothing() {
        let f = Fake::answer(&redirect(&format!("{REPO}/releases/tag/v9.9.9")));
        assert_eq!(check(&f, Channel::Testing, REPO, "0.3.1"), None);
        assert!(f.1.borrow().is_empty(), "no request is made at all");
    }

    #[test]
    fn offline_and_errors_end_silently() {
        let f = Fake(Err("could not resolve host".into()), RefCell::new(vec![]));
        assert_eq!(check(&f, Channel::Stable, REPO, "0.3.1"), None);
        assert_eq!(check(&Fake::answer(&redirect(&format!("{REPO}/releases/tag/v0.4.0"))), Channel::Stable, REPO, "not-a-version"), None, "an unreadable installed version offers nothing");
    }

    #[test]
    fn the_question_goes_to_the_latest_release_of_the_repository_only() {
        let f = Fake::answer(&redirect(&format!("{REPO}/releases/tag/v0.4.0")));
        check(&f, Channel::Stable, REPO, "0.3.1");
        assert_eq!(*f.1.borrow(), [format!("{REPO}/releases/latest")]);
    }

    #[test]
    fn the_curl_fetcher_refuses_anything_but_a_plain_https_address() {
        let c = CurlHead { curl: "curl-that-does-not-exist".into() };
        for bad in ["http://github.com/x", "ftp://x", "https://github.com/x y", "https://github.com/x?a=b", "https://github.com/x&y", "file:///c:/x"] {
            assert_eq!(c.head(bad), Err("invalid address".to_string()), "{bad}");
        }
    }

    /// Read-only check against the real public release page (needs the network and curl): `cargo test --lib releasenotice -- --ignored --nocapture`.
    #[test]
    #[ignore = "needs the network: one HEAD request to the public release page"]
    fn the_real_release_page_answers_in_the_expected_form() {
        let curl = CurlHead { curl: "curl".into() };
        let headers = curl.head(&latest_url(REPO)).expect("the real page answers");
        let latest = parse_latest(&headers, REPO).expect("a redirect to <repo>/releases/tag/vX.Y.Z");
        println!("latest stable release: {latest}");
        // an installed version that is older shows a notice for exactly that release, an equal or newer one shows none
        let older = Version(0, 0, 0).to_string();
        assert_eq!(check(&curl, Channel::Stable, REPO, &older).map(|n| n.url), Some(release_page(REPO, latest)));
        assert_eq!(check(&curl, Channel::Stable, REPO, &latest.to_string()), None);
    }

    #[test]
    fn the_repository_constant_is_the_official_one() {
        assert_eq!(crate::project::REPO_URL, REPO);
    }
}
