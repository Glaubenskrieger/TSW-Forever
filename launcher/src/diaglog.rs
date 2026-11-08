//! Optional diagnostic logging (off by default).
//!
//! Off: nothing is written, no helper process exists, the game's output is discarded exactly as before. On (`diagnostics = basic|detailed` in the
//! configuration, or `--diagnostics`): one log file per start in the launcher's own folder (`logs/` next to the configuration), mode 0600 in a 0700
//! folder, every line passed through the central secret masker before it is written, environment variables listed by NAME only, and only the newest
//! [`KEEP_LAUNCHES`] starts are kept. Nothing is ever sent anywhere. Even masked logs can contain personal data (paths, hardware, character
//! context in game messages): the user decides whether to share them; [`bundle`] builds a reduced, re-masked text for bug reports.

use crate::secrets;
use std::fs::File;
use std::io::Write;
use std::path::{Path, PathBuf};

pub const KEEP_LAUNCHES: usize = 10;
const PREFIX: &str = "launch-";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum DiagLevel {
    #[default]
    Off,
    /// Launcher events: checks, shader-cache and compiler status, start parameters (masked), exit code and duration.
    Basic,
    /// Basic plus the game's own console output (masked line by line) with Wine error messages, and DXVK's info log.
    Detailed,
}

impl DiagLevel {
    pub fn parse(v: &str) -> Result<DiagLevel, String> {
        match v.trim().to_ascii_lowercase().as_str() {
            "off" | "false" | "0" | "no" => Ok(DiagLevel::Off),
            "basic" | "on" | "true" | "1" => Ok(DiagLevel::Basic),
            "detailed" | "detail" | "verbose" => Ok(DiagLevel::Detailed),
            _ => Err("diagnostics must be off, basic or detailed".into()),
        }
    }
    pub fn text(self) -> &'static str {
        match self {
            DiagLevel::Off => "off",
            DiagLevel::Basic => "basic",
            DiagLevel::Detailed => "detailed",
        }
    }
    pub fn is_on(self) -> bool {
        self != DiagLevel::Off
    }
}

/// `<folder of the configuration file>/logs`
pub fn log_dir(config_file: &Path) -> PathBuf {
    let beside = config_file.parent().map(|d| d.join("logs")).unwrap_or_else(|| PathBuf::from("logs"));
    // with an installation, the logs belong to it: `<base>/logs` (default ~/Games/The Secret World/logs), once the installation manager has created that folder
    let base = config_file
        .parent()
        .and_then(|d| crate::install::State::load(&d.join("install-state")).ok())
        .and_then(|s| s.base)
        .or_else(|| std::env::var_os("HOME").and_then(|h| crate::install::default_base(Some(Path::new(&h)))));
    match base {
        Some(b) if !crate::install::is_legacy_base(&b) && b.join("logs").is_dir() => b.join("logs"),
        _ => beside,
    }
}

/// Civil date and time (UTC) of a Unix time. No external crate: the days-to-date step is the standard proleptic-Gregorian algorithm.
pub fn civil(secs: u64) -> (i64, u32, u32, u32, u32, u32) {
    let days = (secs / 86_400) as i64;
    let rem = (secs % 86_400) as u32;
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    let y = yoe + era * 400 + if m <= 2 { 1 } else { 0 };
    (y, m, d, rem / 3600, rem % 3600 / 60, rem % 60)
}

pub fn iso(secs: u64) -> String {
    let (y, mo, d, h, mi, s) = civil(secs);
    format!("{y:04}-{mo:02}-{d:02} {h:02}:{mi:02}:{s:02}Z")
}

pub fn stamp(secs: u64) -> String {
    let (y, mo, d, h, mi, s) = civil(secs);
    format!("{y:04}{mo:02}{d:02}-{h:02}{mi:02}{s:02}")
}

#[cfg(unix)]
fn private_dir(dir: &Path) -> std::io::Result<()> {
    use std::os::unix::fs::{DirBuilderExt, PermissionsExt};
    std::fs::DirBuilder::new().recursive(true).mode(0o700).create(dir)?;
    std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700))
}
#[cfg(not(unix))]
fn private_dir(dir: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(dir)
}

#[cfg(unix)]
fn private_file(path: &Path) -> std::io::Result<File> {
    use std::os::unix::fs::OpenOptionsExt;
    std::fs::OpenOptions::new().write(true).create_new(true).mode(0o600).open(path)
}
#[cfg(not(unix))]
fn private_file(path: &Path) -> std::io::Result<File> {
    // create_new: an existing log is never truncated or overwritten (File::create would do exactly that)
    std::fs::OpenOptions::new().write(true).create_new(true).open(path)
}

/// Keep only the newest `keep` launches (`launch-*.log` and the `dxvk-*` folder of the same stamp). Only entries this module created are touched.
pub fn rotate(dir: &Path, keep: usize) {
    let mut stamps: Vec<String> =
        std::fs::read_dir(dir).into_iter().flatten().flatten().filter_map(|e| e.file_name().to_str().and_then(|n| n.strip_prefix(PREFIX)?.strip_suffix(".log").map(str::to_string))).collect();
    stamps.sort();
    let old = stamps.len().saturating_sub(keep);
    for s in &stamps[..old] {
        let _ = std::fs::remove_file(dir.join(format!("{PREFIX}{s}.log")));
        let _ = std::fs::remove_dir_all(dir.join(format!("dxvk-{s}")));
        let _ = std::fs::remove_file(dir.join(format!("game-{s}.log")));
    }
}

pub struct DiagLog {
    file: File,
    home: Option<String>,
    pub path: PathBuf,
}

impl DiagLog {
    /// Create the log of the start at `now` (rotating older ones first, keeping room for this one).
    pub fn create(dir: &Path, now: u64, home: Option<String>) -> std::io::Result<DiagLog> {
        private_dir(dir)?;
        rotate(dir, KEEP_LAUNCHES.saturating_sub(1));
        let path = dir.join(format!("{PREFIX}{}.log", stamp(now)));
        Ok(DiagLog { file: private_file(&path)?, home, path })
    }

    /// One timestamped event; every line is masked (credentials, tokens, names) and the home directory shortened. Failures to write are ignored.
    pub fn event(&mut self, now: u64, msg: &str) {
        for line in secrets::mask_text(msg).lines() {
            let line = match &self.home {
                Some(h) if h.len() > 1 => line.replace(h.as_str(), "~"),
                _ => line.to_string(),
            };
            let _ = writeln!(self.file, "{} {line}", iso(now));
        }
    }

    pub fn file_mut(&mut self) -> &mut File {
        &mut self.file
    }
}

/// The stamp part of a log path (`launch-<stamp>.log`), used to name the sibling DXVK folder and game log.
pub fn stamp_of(log: &Path) -> Option<String> {
    log.file_name()?.to_str()?.strip_prefix(PREFIX)?.strip_suffix(".log").map(str::to_string)
}

/// The newest log, if any.
pub fn newest(dir: &Path) -> Option<PathBuf> {
    let mut v: Vec<PathBuf> = std::fs::read_dir(dir).ok()?.flatten().map(|e| e.path()).filter(|p| stamp_of(p).is_some()).collect();
    v.sort();
    v.pop()
}

/// Exclude known saved environment values even if an old log or a check echoed one.
/// Short/common values may over-redact diagnostics; privacy takes precedence.
pub fn omit_values(parts: &mut [(&str, String)], values: &[&str]) {
    let mut values: Vec<_> = values.iter().copied().filter(|v| !v.is_empty()).collect();
    values.sort_by_key(|v| std::cmp::Reverse(v.len()));
    for (_, body) in parts {
        // One pass over the original text: replacement markers cannot themselves leak another value.
        let mut out = String::new();
        let mut rest = body.as_str();
        while !rest.is_empty() {
            if let Some(value) = values.iter().find(|v| rest.starts_with(**v)) {
                out.push_str("<omitted>");
                rest = &rest[value.len()..];
            } else {
                let c = rest.chars().next().unwrap();
                out.push(c);
                rest = &rest[c.len_utf8()..];
            }
        }
        *body = out;
    }
}

/// Text for a bug report: the pieces the caller gives (already text), each masked again, the home directory shortened, with a header that says
/// what it contains and what it does not. Returns the text and whether the final check still found something that looks like a credential.
pub fn bundle(parts: &[(&str, String)], home: Option<&str>, now: u64) -> (String, bool) {
    let mut out = format!(
        "tsw-launch diagnostic package, created {}\nContains: reduced configuration, launcher/start information, checks, shader/compiler status and (if present) the newest launcher log.\nSaved environment entries are omitted structurally; their known literal values are also removed from included records. No raw game log is included.\nOther personal data can remain: masking is heuristic. Please read it before you share it; nothing is uploaded.\n",
        iso(now)
    );
    for (title, body) in parts {
        out.push_str(&format!("\n===== {title} =====\n"));
        out.push_str(&secrets::mask_text(body));
        if !out.ends_with('\n') {
            out.push('\n');
        }
    }
    if let Some(h) = home.filter(|h| h.len() > 1) {
        out = out.replace(h, "~");
    }
    let leaked = out.lines().skip(5).any(|l| {
        let low = l.to_ascii_lowercase();
        (low.contains("password") || low.contains("passwd") || low.contains("loginkey") || low.contains("authorization")) && !low.contains("withheld")
    });
    (out, leaked)
}

/// Write `text` as a private file in `dir` and return its path.
pub fn write_bundle(dir: &Path, now: u64, text: &str) -> std::io::Result<PathBuf> {
    private_dir(dir)?;
    let path = dir.join(format!("diagnostic-package-{}.txt", stamp(now)));
    let mut f = private_file(&path)?;
    f.write_all(text.as_bytes())?;
    Ok(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dir(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("tsw-diag-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        d
    }

    #[test]
    fn saved_environment_values_are_excluded_from_configuration_and_all_bundle_parts() {
        let cfg = crate::config::parse("env.MY_INTERNAL_PROJECT = redwood-internal\nenv.FOO = harmless-private-value\n").unwrap();
        let mut parts = vec![
            ("configuration", crate::config::diagnostic_text(&cfg)),
            ("old record", "old child echoed redwood-internal and harmless-private-value; path /home/fixture/game; password: invented".into()),
        ];
        assert!(!parts[0].1.contains("redwood-internal"));
        assert!(!parts[0].1.contains("harmless-private-value"));
        omit_values(&mut parts, &cfg.env.iter().map(|(_, v)| v.as_str()).collect::<Vec<_>>());
        let (text, leaked) = bundle(&parts, Some("/home/fixture"), 1);
        assert!(!leaked && !text.contains("redwood-internal") && !text.contains("harmless-private-value"));
        assert!(!text.contains("invented") && !text.contains("/home/fixture"));
        assert!(text.contains("Other personal data can remain"));
        assert_eq!(DiagLevel::default(), DiagLevel::Off);
    }

    #[test]
    fn levels_parse_and_default_to_off() {
        assert_eq!(DiagLevel::default(), DiagLevel::Off);
        assert_eq!(DiagLevel::parse("Basic"), Ok(DiagLevel::Basic));
        assert_eq!(DiagLevel::parse("detailed"), Ok(DiagLevel::Detailed));
        assert_eq!(DiagLevel::parse("off"), Ok(DiagLevel::Off));
        assert!(DiagLevel::parse("loud").is_err());
        assert!(!DiagLevel::Off.is_on() && DiagLevel::Basic.is_on());
    }

    #[test]
    fn dates_are_right() {
        assert_eq!(civil(0), (1970, 1, 1, 0, 0, 0));
        assert_eq!(civil(951_782_400), (2000, 2, 29, 0, 0, 0), "leap day");
        assert_eq!(civil(1_793_391_452), (2026, 10, 30, 20, 17, 32));
        assert_eq!(stamp(1_793_391_452), "20261030-201732");
        assert_eq!(iso(1_793_391_452), "2026-10-30 20:17:32Z");
    }

    #[test]
    fn a_log_is_private_masked_and_old_ones_are_rotated() {
        let d = dir("log");
        for i in 0..14u64 {
            let mut l = DiagLog::create(&d, 1_793_000_000 + i * 100, Some("/home/someone".into())).unwrap();
            l.event(1_793_000_000 + i * 100, "start in /home/someone/game");
        }
        let logs: Vec<_> = std::fs::read_dir(&d).unwrap().flatten().collect();
        assert_eq!(logs.len(), KEEP_LAUNCHES, "only the newest {KEEP_LAUNCHES} starts are kept");
        let mut l = DiagLog::create(&d, 1_793_100_000, Some("/home/someone".into())).unwrap();
        l.event(1_793_100_000, "args: -loginkey SECRETVALUE123 -statindex 7\nplayer nick: Somebody\nfine line /home/someone/x");
        let text = std::fs::read_to_string(&l.path).unwrap();
        assert!(!text.contains("SECRETVALUE123") && !text.contains("Somebody"), "{text}");
        assert!(text.contains("~/x") && text.contains("2026-"), "{text}");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(std::fs::metadata(&l.path).unwrap().permissions().mode() & 0o777, 0o600);
            assert_eq!(std::fs::metadata(&d).unwrap().permissions().mode() & 0o777, 0o700);
        }
        assert_eq!(newest(&d), Some(l.path.clone()));
        assert!(DiagLog::create(&d, 1_793_100_000, None).is_err(), "never overwrites an existing log");
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn rotation_touches_only_its_own_files() {
        let d = dir("rot");
        std::fs::create_dir_all(&d).unwrap();
        std::fs::write(d.join("notes.txt"), "mine").unwrap();
        for i in 0..5 {
            std::fs::write(d.join(format!("launch-2026010{i}-000000.log")), "x").unwrap();
        }
        std::fs::create_dir_all(d.join("dxvk-20260100-000000")).unwrap();
        rotate(&d, 3);
        assert!(d.join("notes.txt").exists());
        assert!(!d.join("launch-20260100-000000.log").exists() && !d.join("dxvk-20260100-000000").exists());
        assert!(d.join("launch-20260104-000000.log").exists() && d.join("launch-20260102-000000.log").exists());
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn the_bundle_masks_again_shortens_home_and_flags_leftovers() {
        let (t, leaked) = bundle(&[("start", "args: -loginkey ABC123SECRET\n/home/someone/game\nok line".into())], Some("/home/someone"), 1_793_000_000);
        assert!(!t.contains("ABC123SECRET") && t.contains("~/game") && t.contains("ok line") && !leaked, "{t}");
        assert!(t.contains("Please read it before you share it"));
        let d = dir("bundle");
        let p = write_bundle(&d, 1_793_000_000, &t).unwrap();
        assert!(p.is_file());
        let _ = std::fs::remove_dir_all(&d);
    }
}
