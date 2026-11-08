//! The original patcher's own verdict on the installation, read from its `Patcher.log`.
//!
//! Evidence (documentation/architecture.md): at the start of every run the patcher compares the local data with the
//! patch server and logs a summary. A run that reports `Old roothash == New roothash` and `0` new/changed/deleted resources and `Downloading
//! new/changed/patches: 0` says, by Funcom's own comparison against the server, that nothing is left to fetch. That is the only reliable "finished" mark:
//! the index (`RDBHashIndex.bin`) is complete from the first second, the containers are pre-sized, and a finished download writes no final line.
//!
//! Only these fixed lines are read. Nothing else of the log is stored, shown or interpreted (it can carry account data in other places). The log is
//! per run (the patcher truncates it when it starts), and the LAST run marker wins.

use std::io::{Read, Seek, SeekFrom};
use std::path::Path;
use std::time::SystemTime;

/// The log is read at most this far from its end (a long download run logs one progress line per 5 s plus one line per pack).
const MAX_READ: u64 = 8 * 1024 * 1024;
const RUN_MARKER: &str = "[Main] Patcher exe hash is";

/// What the latest run of the patcher reported.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Verdict {
    /// No log, no run marker, or a log that cannot be read: nothing is known.
    #[default]
    Unknown,
    /// A run has started but its summary is not complete yet (or only part of it is readable).
    Working,
    /// The run reported nothing left to fetch (roothash equal, every counter 0).
    Complete,
    /// The run reported data to fetch (or a different roothash).
    Downloading,
}

/// The number after `label` on a line of the patcher's summary (`"Downloading new:      5933   540.96 MB"` -> 5933).
fn count_after(line: &str, label: &str) -> Option<u64> {
    let rest = line.split_once(label)?.1;
    rest.split_whitespace().next()?.parse().ok()
}

fn value_after<'a>(line: &'a str, label: &str) -> Option<&'a str> {
    line.split_once(label).map(|(_, v)| v.trim()).filter(|v| !v.is_empty())
}

/// The verdict of the last run in `text`.
pub fn parse(text: &str) -> Verdict {
    let Some(start) = text.rfind(RUN_MARKER) else { return Verdict::Unknown };
    // from the beginning of the marker's line
    let from = text[..start].rfind('\n').map_or(0, |i| i + 1);
    let (mut old, mut new) = (None, None);
    let (mut source, mut download) = ([None; 4], [None; 3]);
    for line in text[from..].lines() {
        if line.contains("[LocalRDBSource]") {
            if let Some(v) = value_after(line, "Old roothash:") {
                old = Some(v);
            }
            if let Some(v) = value_after(line, "New roothash:") {
                new = Some(v);
            }
            for (i, label) in ["New resources:", "Changed resources:", "Changed source hash resources:", "Deleted resources:"].iter().enumerate() {
                if let Some(n) = count_after(line, label) {
                    source[i] = Some(n);
                }
            }
        } else if line.contains("[RDBManager]") {
            for (i, label) in ["Downloading new:", "Downloading changed:", "Downloading patches:"].iter().enumerate() {
                if let Some(n) = count_after(line, label) {
                    download[i] = Some(n);
                }
            }
        }
    }
    let (Some(old), Some(new)) = (old, new) else { return Verdict::Working };
    if source.iter().any(Option::is_none) || download.iter().any(Option::is_none) {
        return Verdict::Working;
    }
    let all_zero = source.iter().chain(download.iter()).all(|n| *n == Some(0));
    if old == new && all_zero {
        Verdict::Complete
    } else {
        Verdict::Downloading
    }
}

/// The verdict of the log file at `path` and its modification time. A missing or unreadable file is `Unknown`.
pub fn read(path: &Path) -> (Verdict, Option<SystemTime>) {
    let Ok(mut f) = std::fs::File::open(path) else { return (Verdict::Unknown, None) };
    let Ok(meta) = f.metadata() else { return (Verdict::Unknown, None) };
    let modified = meta.modified().ok();
    let skip = meta.len().saturating_sub(MAX_READ);
    if skip > 0 && f.seek(SeekFrom::Start(skip)).is_err() {
        return (Verdict::Unknown, modified);
    }
    let mut buf = Vec::new();
    if f.take(MAX_READ).read_to_end(&mut buf).is_err() {
        return (Verdict::Unknown, modified);
    }
    (parse(&String::from_utf8_lossy(&buf)), modified)
}

#[cfg(test)]
mod tests {
    use super::*;

    const START: &str =
        "[2026-11-01 17:11:30Z #0] [ID:0] [Main] Patcher exe hash is b36bba4f8606fe8fda4fec2a747703bf\n[2026-11-01 17:11:30Z #0] [ID:0] [Patcher] Patch server nslookup: error spawning a process.\n";
    const SOURCE_ZERO: &str = "[2026-11-01 17:11:41Z #0] [ID:0] [LocalRDBSource] New resources: 0 0.00 MB\n[2026-11-01 17:11:41Z #0] [ID:0] [LocalRDBSource] Changed resources: 0 0.00 MB\n[2026-11-01 17:11:41Z #0] [ID:0] [LocalRDBSource] Changed source hash resources: 0 0.00 MB\n[2026-11-01 17:11:41Z #0] [ID:0] [LocalRDBSource] Deleted resources: 0 0.00 MB\n[2026-11-01 17:11:41Z #0] [ID:0] [LocalRDBSource] Old roothash: 2018d1a9a0cff05d7f318cb68afffcc1\n[2026-11-01 17:11:41Z #0] [ID:0] [LocalRDBSource] New roothash: 2018d1a9a0cff05d7f318cb68afffcc1\n";
    const DOWNLOAD_ZERO: &str = "[2026-11-01 17:11:41Z #0] [ID:0] [RDBManager] Downloading new:          0     0.00 MB\n[2026-11-01 17:11:41Z #0] [ID:0] [RDBManager] Downloading changed:      0     0.00 MB\n[2026-11-01 17:11:41Z #0] [ID:0] [RDBManager] Downloading patches:      0     0.00 MB\n[2026-11-01 17:11:41Z #0] [ID:0] [RDBManager] Included localized:       0     0.00 MB\n";
    const DOWNLOAD_SOME: &str = "[2026-11-01 17:15:51Z #0] [ID:0] [RDBManager] Downloading new:       5933   540.96 MB\n[2026-11-01 17:15:51Z #0] [ID:0] [RDBManager] Downloading changed:      0     0.00 MB\n[2026-11-01 17:15:51Z #0] [ID:0] [RDBManager] Downloading patches:      0     0.00 MB\n";

    #[test]
    fn nothing_to_fetch_with_an_equal_roothash_is_the_only_complete_verdict() {
        assert_eq!(parse(&format!("{START}{SOURCE_ZERO}{DOWNLOAD_ZERO}")), Verdict::Complete);
    }

    #[test]
    fn data_to_fetch_or_a_different_roothash_is_not_complete() {
        assert_eq!(parse(&format!("{START}{SOURCE_ZERO}{DOWNLOAD_SOME}")), Verdict::Downloading);
        let changed = SOURCE_ZERO.replace("New roothash: 2018d1a9", "New roothash: ffffffff");
        assert_eq!(parse(&format!("{START}{changed}{DOWNLOAD_ZERO}")), Verdict::Downloading);
        let source_changed = SOURCE_ZERO.replace("Changed resources: 0", "Changed resources: 3");
        assert_eq!(parse(&format!("{START}{source_changed}{DOWNLOAD_ZERO}")), Verdict::Downloading);
        // the first run of a fresh installation: 559 409 resources, 20 GB
        let fresh = DOWNLOAD_SOME.replace("5933   540.96", "559409 20547.30");
        assert_eq!(parse(&format!("{START}{SOURCE_ZERO}{fresh}")), Verdict::Downloading);
    }

    #[test]
    fn a_summary_that_is_not_complete_yet_is_working_never_complete() {
        assert_eq!(parse(START), Verdict::Working, "run started, no summary yet");
        assert_eq!(parse(&format!("{START}{SOURCE_ZERO}")), Verdict::Working, "RDBManager lines missing");
        assert_eq!(parse(&format!("{START}{DOWNLOAD_ZERO}")), Verdict::Working, "roothash lines missing");
        let one_missing = DOWNLOAD_ZERO.replace("Downloading patches:      0     0.00 MB\n", "");
        assert_eq!(parse(&format!("{START}{SOURCE_ZERO}{one_missing}")), Verdict::Working, "one counter missing");
        let no_roothash_new = SOURCE_ZERO.lines().filter(|l| !l.contains("New roothash")).collect::<Vec<_>>().join("\n");
        assert_eq!(parse(&format!("{START}{no_roothash_new}\n{DOWNLOAD_ZERO}")), Verdict::Working, "one roothash missing");
    }

    #[test]
    fn nothing_known_without_a_run_marker_or_with_unreadable_text() {
        assert_eq!(parse(""), Verdict::Unknown);
        assert_eq!(parse(&format!("{SOURCE_ZERO}{DOWNLOAD_ZERO}")), Verdict::Unknown, "a summary without the marker of its run proves nothing");
        assert_eq!(parse("garbage \u{0} \u{fffd} [Main] Patcher exe hash is"), Verdict::Working, "a marker alone is a started run");
        assert_eq!(parse("Patcher exe hash is (not the real marker)"), Verdict::Unknown, "only the exact marker line starts a run");
        assert_eq!(read(Path::new("/nonexistent/Patcher.log")), (Verdict::Unknown, None));
    }

    #[test]
    fn the_last_run_decides() {
        let old_run = format!("{START}{SOURCE_ZERO}{DOWNLOAD_ZERO}");
        let new_run = format!("{START}{SOURCE_ZERO}{DOWNLOAD_SOME}");
        assert_eq!(parse(&format!("{old_run}{new_run}")), Verdict::Downloading, "a later run that downloads replaces an earlier complete one");
        assert_eq!(parse(&format!("{new_run}{old_run}")), Verdict::Complete);
        assert_eq!(parse(&format!("{old_run}{START}")), Verdict::Working, "a run that has just started has no summary yet");
    }

    #[test]
    fn progress_and_pack_lines_do_not_matter_and_the_file_is_read_from_disk() {
        let dir = std::env::temp_dir().join(format!("tsw-patchlog-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let log = dir.join("Patcher.log");
        let mut text = format!("{START}{SOURCE_ZERO}{DOWNLOAD_SOME}");
        for i in 0..2000 {
            text.push_str(&format!("[2026-11-01 17:17:15Z #0] [ID:0] [RDBManager] Downloading pack {i:032x} for type 1010516\n"));
        }
        text.push_str("[2026-11-01 17:17:40Z #0] [ID:0] [HTTPManager] Files:5845/5934 (526.7/541.4 MB)  Speed:4492.2 KB/s\n");
        std::fs::write(&log, &text).unwrap();
        let (verdict, modified) = read(&log);
        assert_eq!(verdict, Verdict::Downloading);
        assert!(modified.is_some());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
