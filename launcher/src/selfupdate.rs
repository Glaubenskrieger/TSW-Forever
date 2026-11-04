//! Launcher updates for the Flatpak version: DETECT only. Installing is done by the host\'s Flatpak (which verifies the OSTree/GPG
//! signature of the repository itself); the sandboxed launcher neither modifies itself nor talks to the host.
//!
//! How: an OSTree "archive" repository served over HTTPS exposes each branch as a plain file `refs/heads/<ref>` holding the commit
//! checksum. Inside a Flatpak the installed commit is in `/.flatpak-info` (`app-commit=`). Different checksums = an update exists.
//! This is a hint, not a security decision: a wrong answer can only show a wrong notice; the actual update is checked and signed by Flatpak.
//! A launcher update needs a restart of the launcher (the running instance keeps the old files until then).
//!
//! UNVERIFIED on a real Flatpak (the key name and the ref file layout follow the Flatpak/OSTree documentation, not a test installation).

use std::path::Path;
use std::process::{Command, Stdio};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Status {
    NotFlatpak,
    UpToDate,
    Available {
        installed: String,
        remote: String,
    },
    /// Could not find out (offline, repository not published yet, unexpected answer). Never an error for the user.
    Unknown(String),
}

fn is_commit(s: &str) -> bool {
    s.len() == 64 && s.bytes().all(|b| b.is_ascii_hexdigit())
}

/// `app-commit=<sha>` from the text of `/.flatpak-info`.
pub fn app_commit(flatpak_info: &str) -> Option<String> {
    flatpak_info.lines().find_map(|l| l.trim().strip_prefix("app-commit=")).map(|v| v.trim().to_ascii_lowercase()).filter(|v| is_commit(v))
}

/// `branch=<name>` from the `[Instance]` section of `/.flatpak-info`: the Flatpak branch this launcher was installed from (`stable`, `testing`, or
/// `local` for a bundle built on a developer machine). UNVERIFIED on a real Flatpak (the key follows the Flatpak documentation).
pub fn branch(flatpak_info: &str) -> Option<String> {
    let b = flatpak_info.lines().find_map(|l| l.trim().strip_prefix("branch="))?.trim();
    (!b.is_empty() && b.len() <= 32 && b.bytes().all(|c| c.is_ascii_alphanumeric() || matches!(c, b'.' | b'_' | b'-'))).then(|| b.to_string())
}

/// Only these branches are update channels of the project repository; any other branch (a local build) is never compared with the repository.
pub fn is_channel(branch: &str) -> bool {
    matches!(branch, "stable" | "testing")
}

/// Ask the repository for the commit of `refname` (for example `app/<id>/x86_64/stable`). https only.
pub fn remote_commit(curl: &Path, repo_url: &str, refname: &str) -> Result<String, String> {
    if !repo_url.starts_with("https://") || refname.contains("..") || refname.chars().any(|c| !(c.is_ascii_alphanumeric() || matches!(c, '/' | '.' | '_' | '-'))) {
        return Err("invalid repository address or ref".into());
    }
    let url = format!("{}/refs/heads/{refname}", repo_url.trim_end_matches('/'));
    let out = Command::new(curl)
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
            "256",
            "--",
        ])
        .arg(url)
        .stdin(Stdio::null())
        .output()
        .map_err(|e| format!("cannot start curl: {e}"))?;
    if !out.status.success() {
        return Err(String::from_utf8_lossy(&out.stderr).trim().to_string());
    }
    let t = String::from_utf8_lossy(&out.stdout).trim().to_ascii_lowercase();
    if is_commit(&t) {
        Ok(t)
    } else {
        Err("the repository answered with something that is not a commit checksum".into())
    }
}

pub fn check(flatpak_info: &Path, curl: &Path, repo_url: &str, refname: &str) -> Status {
    let Ok(info) = std::fs::read_to_string(flatpak_info) else {
        return Status::NotFlatpak;
    };
    let Some(installed) = app_commit(&info) else {
        return Status::Unknown("the installed version is not known".into());
    };
    match remote_commit(curl, repo_url, refname) {
        Ok(remote) if remote == installed => Status::UpToDate,
        Ok(remote) => Status::Available { installed, remote },
        Err(e) => Status::Unknown(e),
    }
}

/// What to tell the user (English id; the GUI translates).
pub fn instructions(app_id: &str) -> String {
    format!("A launcher update is available. Install it with your system\'s software center, or run: flatpak update {app_id}. Then restart the launcher.")
}

#[cfg(test)]
mod tests {
    use super::*;

    const A: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
    const B: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";

    #[test]
    fn the_installed_commit_is_read_from_the_flatpak_info() {
        let _exec = crate::test_exec_lock();
        assert_eq!(app_commit(&format!("[Application]\nname=x\n\n[Instance]\napp-commit={A}\napp-extensions=\n")), Some(A.into()));
        assert_eq!(app_commit("[Instance]\napp-commit=zzz\n"), None);
        assert_eq!(app_commit("[Application]\n"), None);
    }

    #[test]
    fn the_installed_branch_is_read_and_only_stable_and_testing_are_channels() {
        let _exec = crate::test_exec_lock();
        assert_eq!(branch("[Instance]\nbranch=testing\narch=x86_64\n"), Some("testing".into()));
        assert_eq!(branch("[Instance]\nbranch=local\n"), Some("local".into()));
        assert_eq!(branch("[Instance]\nbranch=../x\n"), None);
        assert_eq!(branch("[Instance]\nbranch=\n"), None);
        assert_eq!(branch("[Application]\nname=x\n"), None);
        assert!(is_channel("stable") && is_channel("testing") && !is_channel("local") && !is_channel("master"));
    }

    #[cfg(unix)]
    fn fake_curl(dir: &Path, body: &str, code: i32) -> std::path::PathBuf {
        use std::os::unix::fs::PermissionsExt;
        let p = dir.join("curl");
        std::fs::write(&p, format!("#!/bin/sh\necho '{body}'\nexit {code}\n")).unwrap();
        std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o755)).unwrap();
        p
    }

    #[cfg(unix)]
    #[test]
    fn up_to_date_available_unreachable_and_not_a_flatpak_are_told_apart() {
        let _exec = crate::test_exec_lock();
        let d = std::env::temp_dir().join(format!("tsw-selfupdate-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        let info = d.join("flatpak-info");
        std::fs::write(&info, format!("[Instance]\napp-commit={A}\n")).unwrap();
        let r = "app/org.example.App/x86_64/stable";
        let repo = "https://example.invalid/repo";
        assert_eq!(check(&info, &fake_curl(&d, A, 0), repo, r), Status::UpToDate);
        assert_eq!(check(&info, &fake_curl(&d, B, 0), repo, r), Status::Available { installed: A.into(), remote: B.into() });
        assert!(matches!(check(&info, &fake_curl(&d, "Could not resolve host", 6), repo, r), Status::Unknown(_)), "offline or not published yet: no error");
        assert!(matches!(check(&info, &fake_curl(&d, "<html>404</html>", 0), repo, r), Status::Unknown(_)), "garbage is not a commit");
        assert_eq!(check(&d.join("missing"), &fake_curl(&d, A, 0), repo, r), Status::NotFlatpak);
        assert!(matches!(check(&info, &fake_curl(&d, A, 0), "http://example.invalid/repo", r), Status::Unknown(_)), "https only");
        assert!(remote_commit(Path::new("curl"), repo, "../../etc/passwd").is_err());
        assert!(instructions("org.example.App").contains("flatpak update org.example.App") && instructions("x").contains("restart"));
        let _ = std::fs::remove_dir_all(&d);
    }
}
