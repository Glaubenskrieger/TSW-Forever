//! Getting bytes onto disk. [`Fetch`] is the only place with network access; the rest of the component is offline.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use crate::manifest::Artifact;
use crate::PatchError;

pub trait Fetch {
    /// Put the artifact's bytes at `dest` (complete file). Integrity is verified by the caller against the signed manifest.
    fn fetch(&self, artifact: &Artifact, dest: &Path, cancel: &AtomicBool) -> Result<(), PatchError>;
}

/// Takes artifacts from a local directory (offline updates from a USB stick, and tests).
pub struct DirFetch(pub PathBuf);

impl Fetch for DirFetch {
    fn fetch(&self, a: &Artifact, dest: &Path, _: &AtomicBool) -> Result<(), PatchError> {
        std::fs::copy(self.0.join(&a.name), dest).map(|_| ()).map_err(|e| PatchError::Network(format!("{}: {e}", a.name)))
    }
}

/// HTTPS download with `curl` (argument vector, no shell, certificate checks on, https only even after redirects, resumable `.part`).
pub struct CurlFetch {
    pub curl: PathBuf,
    /// Base URL ending in `/`; artifact names are appended (they are restricted to `[A-Za-z0-9._-]`).
    pub base_url: String,
}

impl CurlFetch {
    pub fn get_text(&self, name: &str, max_bytes: usize) -> Result<String, PatchError> {
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
                "15",
                "--max-time",
                "30",
                "--max-filesize",
            ])
            .arg(max_bytes.to_string())
            .arg("--")
            .arg(format!("{}{name}", self.base_url))
            .stdin(Stdio::null())
            .output()
            .map_err(|e| PatchError::Network(format!("cannot start curl: {e}")))?;
        if !out.status.success() {
            return Err(PatchError::Network(format!("{name}: {}", String::from_utf8_lossy(&out.stderr).trim())));
        }
        String::from_utf8(out.stdout).map_err(|_| PatchError::Network(format!("{name} is not text")))
    }
}

impl Fetch for CurlFetch {
    fn fetch(&self, a: &Artifact, dest: &Path, cancel: &AtomicBool) -> Result<(), PatchError> {
        let part = dest.with_extension("part");
        for attempt in 0..2 {
            let mut child = Command::new(&self.curl)
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
                    "20",
                    "--speed-limit",
                    "1000",
                    "--speed-time",
                    "60",
                    "-C",
                    "-",
                    "--max-filesize",
                ])
                .arg(a.size.to_string())
                .arg("--output")
                .arg(&part)
                .arg("--")
                .arg(format!("{}{}", self.base_url, a.name))
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::piped())
                .spawn()
                .map_err(|e| PatchError::Network(format!("cannot start curl: {e}")))?;
            let status = loop {
                if cancel.load(Ordering::SeqCst) {
                    let _ = child.kill();
                    let _ = child.wait();
                    return Err(PatchError::Cancelled);
                }
                if let Some(s) = child.try_wait().map_err(|e| PatchError::Network(e.to_string()))? {
                    break s;
                }
                std::thread::sleep(Duration::from_millis(100));
            };
            if status.success() {
                return std::fs::rename(&part, dest).map_err(|e| PatchError::Io(format!("cannot finish {}: {e}", a.name)));
            }
            let mut msg = String::new();
            if let Some(mut s) = child.stderr.take() {
                let _ = std::io::Read::read_to_string(&mut s, &mut msg);
            }
            if status.code() == Some(33) && attempt == 0 {
                let _ = std::fs::remove_file(&part);
                continue;
            }
            return Err(PatchError::Network(format!("{}: {}", a.name, msg.trim())));
        }
        Err(PatchError::Network(format!("{}: download failed", a.name)))
    }
}
