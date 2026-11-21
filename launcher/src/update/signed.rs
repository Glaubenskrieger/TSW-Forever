//! Launcher update metadata: parse, verify, decide. No platform code, no installing.
//!
//! **Product decision (2026-11): there is no automatic launcher update on Windows.** This whole module and `update::windows` are compiled only with the Cargo
//! feature `windows-self-update`, which no product build enables (see `update/mod.rs`). The code is kept for development; the Linux update is the Flatpak portal.
//!
//! This is the launcher's own update check. It has nothing to do with patching the original game files (`tsw_patch`, which has its own key and its own
//! code path): only primitives (minisign verification, SHA-256) are reused.
//!
//! The metadata is a small text file signed with minisign (Ed25519), fetched over https:
//!
//! ```text
//! tsw-forever-update 1
//! channel: stable
//! version: 0.3.5
//! issued: 2026-12-01T10:00:00Z
//! expires: 2027-03-01T00:00:00Z
//! windows-installer: tsw-forever-0.3.5-windows-setup.exe
//! windows-url: https://github.com/<owner>/<repo>/releases/download/v0.3.5/tsw-forever-0.3.5-windows-setup.exe
//! windows-size: 8199649
//! windows-sha256: <64 hex digits>
//! previous-version / previous-url / previous-size / previous-sha256: optional, all four or none (recovery target)
//! notes-url: optional
//! flatpak-ref: optional, informational
//! ```
//!
//! Every check is a refusal with a reason; nothing here can fail loudly for the user: offline, an unknown answer or a refused offer all end as
//! [`Outcome::Unknown`] / [`Outcome::Rejected`], and the launcher carries on as before.
//!
//! Order of checks (nothing in the metadata is trusted before the signature has been verified): size limit, signature with the key compiled into this
//! build, strict format, channel, channel rule of the build, version (numeric, strictly newer than the installed one), validity window, URLs, digest format.

use crate::channel::{use_of_offer, Channel, OfferUse};
use std::path::Path;
use std::process::{Command, Stdio};

#[path = "windows.rs"]
pub mod windows;

#[cfg(test)]
#[path = "fixtures.rs"]
mod fixtures;

/// The first line of the metadata; changing the grammar means a new number here.
pub const HEADER: &str = "tsw-forever-update 1";
/// Metadata above this size is not read at all.
pub const MAX_METADATA: usize = 16 * 1024;
/// Largest installer this launcher will download.
pub const MAX_INSTALLER: u64 = 512 * 1024 * 1024;
/// How far in the future `issued` may be (clock differences).
const SKEW_SECS: u64 = 24 * 3600;
/// Longest validity window a signed statement may have.
const MAX_VALIDITY_SECS: u64 = 400 * 24 * 3600;

/// Why an offer was refused. The text is for logs and diagnostics, never a pop-up.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UpdateError {
    /// No public key is compiled into this build: the update check is switched off (like the patch system without its key).
    NoTrustedKey,
    TooLarge,
    BadSignature(String),
    BadFormat(String),
    WrongChannel {
        expected: &'static str,
        found: String,
    },
    /// The build must not consult this channel at all.
    ChannelNotAllowed,
    /// Same or older version than the installed one.
    NotNewer {
        installed: String,
        offered: String,
    },
    NotYetValid,
    Expired,
    BadValidity(String),
    UrlNotAllowed(String),
    BadDigest(String),
    HashMismatch,
    SizeMismatch,
}

impl std::fmt::Display for UpdateError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            UpdateError::NoTrustedKey => write!(f, "no trusted update key is configured: the update check is off"),
            UpdateError::TooLarge => write!(f, "the update metadata is too large"),
            UpdateError::BadSignature(e) => write!(f, "the update metadata signature is not valid: {e}"),
            UpdateError::BadFormat(e) => write!(f, "the update metadata is malformed: {e}"),
            UpdateError::WrongChannel { expected, found } => write!(f, "the update metadata is for the channel {found}, expected {expected}"),
            UpdateError::ChannelNotAllowed => write!(f, "this build does not use that update channel"),
            UpdateError::NotNewer { installed, offered } => write!(f, "version {offered} is not newer than the installed {installed}"),
            UpdateError::NotYetValid => write!(f, "the update metadata is dated in the future"),
            UpdateError::Expired => write!(f, "the update metadata has expired"),
            UpdateError::BadValidity(e) => write!(f, "the validity window is not acceptable: {e}"),
            UpdateError::UrlNotAllowed(e) => write!(f, "a download address is not allowed: {e}"),
            UpdateError::BadDigest(e) => write!(f, "a checksum or size is malformed: {e}"),
            UpdateError::HashMismatch => write!(f, "the downloaded file does not match the signed checksum"),
            UpdateError::SizeMismatch => write!(f, "the downloaded file does not have the signed size"),
        }
    }
}

// ---- versions and time ------------------------------------------------------------------------------------------------

/// A plain `MAJOR.MINOR.PATCH` version (project policy: no suffix, no build part).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Version(pub u32, pub u32, pub u32);

impl Version {
    pub fn parse(text: &str) -> Option<Version> {
        let mut it = text.split('.');
        let mut part = || {
            let p = it.next()?;
            let ok = !p.is_empty() && p.len() <= 6 && p.bytes().all(|b| b.is_ascii_digit()) && (p == "0" || !p.starts_with('0'));
            ok.then(|| p.parse::<u32>().ok()).flatten()
        };
        let v = Version(part()?, part()?, part()?);
        it.next().is_none().then_some(v)
    }
}

impl std::fmt::Display for Version {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}.{}.{}", self.0, self.1, self.2)
    }
}

/// `2026-12-01T10:00:00Z` (exactly this shape, UTC) as seconds since 1970. `None` for anything else, including impossible dates.
pub fn parse_utc(text: &str) -> Option<u64> {
    let b = text.as_bytes();
    if b.len() != 20 || b[4] != b'-' || b[7] != b'-' || b[10] != b'T' || b[13] != b':' || b[16] != b':' || b[19] != b'Z' {
        return None;
    }
    let num = |from: usize, to: usize| -> Option<u64> {
        let s = &text[from..to];
        s.bytes().all(|c| c.is_ascii_digit()).then(|| s.parse().ok()).flatten()
    };
    let (y, mo, d, h, mi, s) = (num(0, 4)?, num(5, 7)?, num(8, 10)?, num(11, 13)?, num(14, 16)?, num(17, 19)?);
    let leap = (y % 4 == 0 && y % 100 != 0) || y % 400 == 0;
    let dim = [31, if leap { 29 } else { 28 }, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31];
    if !(1970..=9999).contains(&y) || !(1..=12).contains(&mo) || d < 1 || d > dim[mo as usize - 1] || h > 23 || mi > 59 || s > 59 {
        return None;
    }
    let mut days: u64 = (1970..y).map(|yy| if (yy % 4 == 0 && yy % 100 != 0) || yy % 400 == 0 { 366 } else { 365 }).sum();
    days += dim[..mo as usize - 1].iter().sum::<u64>() + (d - 1);
    Some(days * 86_400 + h * 3_600 + mi * 60 + s)
}

// ---- the metadata -----------------------------------------------------------------------------------------------------

/// One downloadable installer, as signed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Installer {
    pub name: String,
    pub url: String,
    pub size: u64,
    pub sha256: String,
}

/// The parsed (not yet validated) metadata.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Metadata {
    pub channel: String,
    pub version: String,
    pub issued: u64,
    pub expires: u64,
    pub windows: Installer,
    pub previous: Option<(String, Installer)>,
    pub notes_url: Option<String>,
    pub flatpak_ref: Option<String>,
}

fn bad(s: impl Into<String>) -> UpdateError {
    UpdateError::BadFormat(s.into())
}

fn is_hex_digest(s: &str) -> bool {
    s.len() == 64 && s.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
}

/// Parse the text strictly: the exact header, one `key: value` per line, printable ASCII only, no unknown and no repeated keys, no missing required keys.
pub fn parse(text: &str) -> Result<Metadata, UpdateError> {
    if text.len() > MAX_METADATA {
        return Err(UpdateError::TooLarge);
    }
    if !text.bytes().all(|b| b == b'\n' || (0x20..=0x7e).contains(&b)) {
        return Err(bad("only printable ASCII and line feeds are allowed"));
    }
    let mut lines = text.split('\n');
    if lines.next() != Some(HEADER) {
        return Err(bad("the first line is not the expected header"));
    }
    let mut map: Vec<(String, String)> = Vec::new();
    for line in lines {
        if line.is_empty() {
            continue;
        }
        let (k, v) = line.split_once(": ").ok_or_else(|| bad("a line is not `key: value`"))?;
        if v.is_empty() || v != v.trim() || k.is_empty() || !k.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-') {
            return Err(bad(format!("unusable line for {k:?}")));
        }
        if map.iter().any(|(e, _)| e == k) {
            return Err(bad(format!("{k} appears twice")));
        }
        map.push((k.to_string(), v.to_string()));
    }
    const KNOWN: &[&str] = &[
        "channel",
        "version",
        "issued",
        "expires",
        "windows-installer",
        "windows-url",
        "windows-size",
        "windows-sha256",
        "previous-version",
        "previous-installer",
        "previous-url",
        "previous-size",
        "previous-sha256",
        "notes-url",
        "flatpak-ref",
    ];
    if let Some((k, _)) = map.iter().find(|(k, _)| !KNOWN.contains(&k.as_str())) {
        return Err(bad(format!("unknown key {k}")));
    }
    let get = |k: &str| map.iter().find(|(e, _)| e == k).map(|(_, v)| v.clone());
    let need = |k: &str| get(k).ok_or_else(|| bad(format!("{k} is missing")));
    let size = |k: &str, v: String| -> Result<u64, UpdateError> {
        if v.is_empty() || v.len() > 12 || !v.bytes().all(|b| b.is_ascii_digit()) {
            return Err(UpdateError::BadDigest(format!("{k} is not a number")));
        }
        v.parse().map_err(|_| UpdateError::BadDigest(format!("{k} is not a number")))
    };
    let time = |k: &str| parse_utc(&need(k)?).ok_or_else(|| bad(format!("{k} is not a UTC time like 2026-12-01T10:00:00Z")));
    let windows = Installer { name: need("windows-installer")?, url: need("windows-url")?, size: size("windows-size", need("windows-size")?)?, sha256: need("windows-sha256")? };
    let prev_keys = ["previous-version", "previous-installer", "previous-url", "previous-size", "previous-sha256"];
    let present = prev_keys.iter().filter(|k| get(k).is_some()).count();
    let previous = match present {
        0 => None,
        5 => Some((
            need("previous-version")?,
            Installer { name: need("previous-installer")?, url: need("previous-url")?, size: size("previous-size", need("previous-size")?)?, sha256: need("previous-sha256")? },
        )),
        _ => return Err(bad("the previous-* keys come all together or not at all")),
    };
    Ok(Metadata {
        channel: need("channel")?,
        version: need("version")?,
        issued: time("issued")?,
        expires: time("expires")?,
        windows,
        previous,
        notes_url: get("notes-url"),
        flatpak_ref: get("flatpak-ref"),
    })
}

// ---- validation -------------------------------------------------------------------------------------------------------

/// Everything the decision depends on; no clock, no network, no global state, so every case is a plain test.
#[derive(Debug, Clone)]
pub struct Context<'a> {
    /// The channel of the installed program.
    pub build: Channel,
    /// The channel of the feed this metadata was fetched from.
    pub feed: Channel,
    /// Base64 line of the trusted minisign key of that feed (empty = off).
    pub key: &'a str,
    /// Repository address the release downloads must live under, without a trailing slash (`https://github.com/<owner>/<repo>`).
    pub repo_url: &'a str,
    /// The installed version (`0.3.1`).
    pub installed: &'a str,
    /// Seconds since 1970.
    pub now: u64,
}

/// A validated offer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Offer {
    pub channel: Channel,
    pub use_as: OfferUse,
    pub version: String,
    pub installer: Installer,
    pub previous: Option<(String, Installer)>,
    pub notes_url: Option<String>,
    pub expires: u64,
}

/// Is `url` exactly `<repo_url>/releases/download/v<version>/<name>` with nothing else in it? Strict on purpose: https only, no userinfo, port, query,
/// fragment, escape, backslash or dot segment, and the file name must be the signed one.
fn url_ok(url: &str, repo_url: &str, version: &str, name: &str) -> Result<(), UpdateError> {
    let no = |why: &str| Err(UpdateError::UrlNotAllowed(format!("{why}: {url}")));
    if !url.is_ascii() || !url.bytes().all(|b| b.is_ascii_alphanumeric() || matches!(b, b':' | b'/' | b'.' | b'-' | b'_')) {
        return no("unexpected character");
    }
    if !repo_url.starts_with("https://") || repo_url.ends_with('/') {
        return no("the repository address of this build is not usable");
    }
    let want = format!("{repo_url}/releases/download/v{version}/{name}");
    if url != want {
        return no("not the release file of the signed version in the official repository");
    }
    let name_ok = !name.is_empty() && name.len() <= 100 && !name.starts_with('.') && !name.contains("..") && !name.contains('/');
    if !name_ok {
        return no("unusable file name");
    }
    Ok(())
}

fn installer_ok(i: &Installer, repo_url: &str, version: &str) -> Result<(), UpdateError> {
    url_ok(&i.url, repo_url, version, &i.name)?;
    if !is_hex_digest(&i.sha256) {
        return Err(UpdateError::BadDigest("the SHA-256 is not 64 lower-case hex digits".into()));
    }
    if i.size == 0 || i.size > MAX_INSTALLER {
        return Err(UpdateError::BadDigest(format!("the size {} is not acceptable", i.size)));
    }
    Ok(())
}

/// Verify the signature, then validate the metadata against the build's rules. The order matters: nothing from the text is looked at before the
/// signature holds, except its size.
pub fn verify_and_validate(metadata: &[u8], signature: &str, ctx: &Context<'_>) -> Result<Offer, UpdateError> {
    if ctx.key.trim().is_empty() {
        return Err(UpdateError::NoTrustedKey);
    }
    if metadata.len() > MAX_METADATA {
        return Err(UpdateError::TooLarge);
    }
    tsw_patch::sig::verify(metadata, signature, ctx.key).map_err(|e| UpdateError::BadSignature(e.to_string()))?;
    let text = std::str::from_utf8(metadata).map_err(|_| bad("not valid text"))?;
    let m = parse(text)?;
    let use_as = use_of_offer(ctx.build, ctx.feed).ok_or(UpdateError::ChannelNotAllowed)?;
    if Channel::parse(&m.channel) != Some(ctx.feed) {
        return Err(UpdateError::WrongChannel { expected: ctx.feed.name(), found: m.channel });
    }
    let offered = Version::parse(&m.version).ok_or_else(|| bad("version is not MAJOR.MINOR.PATCH"))?;
    let installed = Version::parse(ctx.installed).ok_or_else(|| bad("the installed version is not MAJOR.MINOR.PATCH"))?;
    // validity window
    if m.issued > ctx.now + SKEW_SECS {
        return Err(UpdateError::NotYetValid);
    }
    if m.expires <= ctx.now {
        return Err(UpdateError::Expired);
    }
    if m.expires <= m.issued {
        return Err(UpdateError::BadValidity("it ends before it begins".into()));
    }
    if m.expires - m.issued > MAX_VALIDITY_SECS {
        return Err(UpdateError::BadValidity("it is longer than 400 days".into()));
    }
    // version rule: an update must be strictly newer; the stable offer to a testing program may be the same or newer (never an older one), and is only a choice
    let acceptable = match use_as {
        OfferUse::Update => offered > installed,
        OfferUse::SwitchToStable => offered >= installed,
    };
    if !acceptable {
        return Err(UpdateError::NotNewer { installed: installed.to_string(), offered: offered.to_string() });
    }
    let version = offered.to_string();
    if version != m.version {
        return Err(bad("the version is not in canonical form"));
    }
    installer_ok(&m.windows, ctx.repo_url, &version)?;
    let previous = match m.previous {
        Some((pv, inst)) => {
            let pver = Version::parse(&pv).ok_or_else(|| bad("previous-version is not MAJOR.MINOR.PATCH"))?;
            if pver >= offered || pver.to_string() != pv {
                return Err(bad("previous-version must be older than version"));
            }
            installer_ok(&inst, ctx.repo_url, &pv)?;
            Some((pv, inst))
        }
        None => None,
    };
    if let Some(n) = &m.notes_url {
        if !n.starts_with(&format!("{}/", ctx.repo_url)) || !n.bytes().all(|b| b.is_ascii_alphanumeric() || matches!(b, b':' | b'/' | b'.' | b'-' | b'_')) || n.contains("..") {
            return Err(UpdateError::UrlNotAllowed(format!("notes address: {n}")));
        }
    }
    Ok(Offer { channel: ctx.feed, use_as, version, installer: m.windows, previous, notes_url: m.notes_url, expires: m.expires })
}

/// After the download: size and SHA-256 of the file must be exactly the signed ones. Nothing is started before this holds.
pub fn verify_installer_file(path: &Path, expected: &Installer) -> Result<(), UpdateError> {
    let (hash, size) = tsw_patch::sha256::file_sha256(path).map_err(|e| UpdateError::BadDigest(format!("cannot read the download: {e}")))?;
    if size != expected.size {
        return Err(UpdateError::SizeMismatch);
    }
    if hash != expected.sha256 {
        return Err(UpdateError::HashMismatch);
    }
    Ok(())
}

// ---- fetching and the whole check ---------------------------------------------------------------------------------------

/// Where metadata comes from. Production: [`CurlFeed`]; tests: a fake.
pub trait Feed {
    fn fetch(&self, url: &str, max_bytes: usize) -> Result<Vec<u8>, String>;
}

/// The feed address of a channel: `<repo>/releases/latest/download/update-<channel>.txt` (and `.minisig`). GitHub's `latest` points at the newest
/// release that is not a pre-release, so a stable feed only ever moves with a published stable release.
pub fn feed_url(repo_url: &str, channel: Channel, signature: bool) -> String {
    format!("{repo_url}/releases/latest/download/update-{}.txt{}", channel.name(), if signature { ".minisig" } else { "" })
}

/// The result of one check. Offline and every refusal are ordinary results, never errors for the user.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    /// No key compiled in (or the channel is not consulted): nothing was fetched.
    Inactive(String),
    /// A valid, newer offer.
    Offer(Offer),
    /// The feed answered and nothing newer is offered (also: the offer is the installed version).
    UpToDate,
    /// Could not find out (offline, timeout, unexpected answer): the launcher carries on silently.
    Unknown(String),
    /// The metadata was fetched but refused (bad signature, wrong channel, expired, ...). Logged, never installed.
    Rejected(UpdateError),
}

/// Fetch the metadata and its signature of `ctx.feed` and decide. Makes no request at all if the key is missing or the build must not consult the channel.
pub fn check(feed: &dyn Feed, ctx: &Context<'_>) -> Outcome {
    if ctx.key.trim().is_empty() {
        return Outcome::Inactive(UpdateError::NoTrustedKey.to_string());
    }
    if use_of_offer(ctx.build, ctx.feed).is_none() {
        return Outcome::Inactive(UpdateError::ChannelNotAllowed.to_string());
    }
    let meta = match feed.fetch(&feed_url(ctx.repo_url, ctx.feed, false), MAX_METADATA) {
        Ok(b) => b,
        Err(e) => return Outcome::Unknown(e),
    };
    let sig = match feed.fetch(&feed_url(ctx.repo_url, ctx.feed, true), 4096) {
        Ok(b) => String::from_utf8_lossy(&b).into_owned(),
        Err(e) => return Outcome::Unknown(e),
    };
    match verify_and_validate(&meta, &sig, ctx) {
        Ok(offer) => Outcome::Offer(offer),
        Err(UpdateError::NotNewer { .. }) => Outcome::UpToDate,
        Err(e) => Outcome::Rejected(e),
    }
}

/// `curl` as the feed: https only (also for redirects), few redirects, timeouts, a hard size limit, nothing from the environment's proxy settings overridden.
pub struct CurlFeed {
    pub curl: std::path::PathBuf,
}

impl Feed for CurlFeed {
    fn fetch(&self, url: &str, max_bytes: usize) -> Result<Vec<u8>, String> {
        if !url.starts_with("https://") || url.chars().any(|c| !(c.is_ascii_alphanumeric() || matches!(c, ':' | '/' | '.' | '-' | '_'))) {
            return Err("invalid update address".into());
        }
        let out = Command::new(&self.curl)
            .args([
                "--fail",
                "--silent",
                "--show-error",
                "--location",
                "--max-redirs",
                "3",
                "--proto",
                "=https",
                "--proto-redir",
                "=https",
                "--connect-timeout",
                "10",
                "--max-time",
                "20",
                "--max-filesize",
            ])
            .arg(max_bytes.to_string())
            .args(["--"])
            .arg(url)
            .stdin(Stdio::null())
            .output()
            .map_err(|e| format!("cannot start curl: {e}"))?;
        if !out.status.success() {
            return Err(String::from_utf8_lossy(&out.stderr).trim().to_string());
        }
        if out.stdout.len() > max_bytes {
            return Err("the answer is larger than allowed".into());
        }
        Ok(out.stdout)
    }
}

#[cfg(test)]
#[path = "tests.rs"]
mod tests;
