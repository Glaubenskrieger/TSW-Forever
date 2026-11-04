//! Planning and applying a verified manifest to a game folder, as a transaction.
//!
//! Layout of the work folder (`patches/` below the installation folder, never inside the game folder):
//! `state` (installed patch version), `installed-manifest.txt`, `lock`, `journal`, `staging/`, `backup/`, `artifacts/`.
//! Order of events: lock -> recover an interrupted run -> plan -> download+verify artifacts -> build and verify every new file in
//! `staging/` -> back up the files to replace -> journal -> atomic swap per file -> verify all results again -> record the version.
//! Any failure after the journal exists rolls back from `backup/`; the recorded version changes only after full verification.

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;

use crate::fetch::Fetch;
use crate::manifest::{Artifact, FileEntry, Manifest};
use crate::path::{safe_target, validate_rel, TEMP_SUFFIX};
use crate::sha256::{file_sha256, sha256_hex};
use crate::{delta, io, PatchError};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    Unchanged,
    Delta,
    Full,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Plan {
    /// Per manifest file, in manifest order.
    pub actions: Vec<Action>,
}

impl Plan {
    pub fn changes(&self) -> usize {
        self.actions.iter().filter(|a| **a != Action::Unchanged).count()
    }
}

/// Installed patch version (0 = never patched). A damaged state file is an error, not "0", so it cannot cause a silent downgrade.
pub fn installed_version(work: &Path) -> Result<u64, PatchError> {
    match fs::read_to_string(work.join("state")) {
        Ok(t) => t.lines().find_map(|l| l.strip_prefix("version = ")).and_then(|v| v.trim().parse().ok()).ok_or_else(|| PatchError::State("the patch state file is damaged".into())),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(0),
        Err(e) => Err(io("cannot read the patch state", e)),
    }
}

fn hash_of(path: &Path) -> Result<Option<(String, u64)>, PatchError> {
    match fs::symlink_metadata(path) {
        Ok(_) => file_sha256(path).map(Some).map_err(|e| io(&format!("cannot read {}", path.display()), e)),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(io("cannot inspect a file", e)),
    }
}

/// Compare the game folder with the manifest. Read-only.
pub fn plan(root: &Path, m: &Manifest, installed: u64) -> Result<Plan, PatchError> {
    if m.version <= installed {
        return Err(PatchError::State(format!("manifest version {} is not newer than the installed patch {installed} (no downgrades)", m.version)));
    }
    if installed < m.requires {
        return Err(PatchError::State(format!("this manifest needs patch {} or newer to be installed first (installed: {installed})", m.requires)));
    }
    let mut actions = Vec::new();
    for f in &m.files {
        let rel = validate_rel(&f.path)?;
        let target = safe_target(root, &rel)?;
        let cur = hash_of(&target)?;
        actions.push(match cur {
            Some((h, _)) if h == f.new_sha256 => Action::Unchanged,
            Some((h, _)) if f.delta.is_some() && f.from_sha256.as_deref() == Some(h.as_str()) => Action::Delta,
            _ if f.full.is_some() => Action::Full,
            _ => return Err(PatchError::State(format!("{} differs from the expected original and the manifest has no full copy of it", f.path))),
        });
    }
    Ok(Plan { actions })
}

struct Lock(PathBuf);
impl Lock {
    fn take(work: &Path) -> Result<Lock, PatchError> {
        let p = work.join("lock");
        for _ in 0..2 {
            match fs::OpenOptions::new().write(true).create_new(true).open(&p) {
                Ok(mut f) => {
                    let _ = write!(f, "{}", std::process::id());
                    return Ok(Lock(p));
                }
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
                    let pid: Option<u32> = fs::read_to_string(&p).ok().and_then(|t| t.trim().parse().ok());
                    let alive = pid.is_some_and(|pid| Path::new(&format!("/proc/{pid}")).exists());
                    if alive {
                        return Err(PatchError::Busy("another update is running".into()));
                    }
                    let _ = fs::remove_file(&p); // stale lock of a crashed run
                }
                Err(e) => return Err(io("cannot create the update lock", e)),
            }
        }
        Err(PatchError::Busy("cannot take the update lock".into()))
    }
}
impl Drop for Lock {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.0);
    }
}

/// Progress callback: (step, done, total).
pub type ProgressFn<'a> = dyn Fn(&str, usize, usize) + 'a;

/// Options of an apply run (`fail_after_swaps` is a test hook: fail after this many files were swapped, to exercise the rollback).
#[derive(Default)]
pub struct Options<'a> {
    pub game_running: Option<&'a dyn Fn() -> bool>,
    pub progress: Option<&'a ProgressFn<'a>>,
    #[doc(hidden)]
    pub fail_after_swaps: Option<usize>,
}

fn write_synced(path: &Path, bytes: &[u8]) -> Result<(), PatchError> {
    if let Some(d) = path.parent() {
        fs::create_dir_all(d).map_err(|e| io("cannot create a folder", e))?;
    }
    let mut f = fs::File::create(path).map_err(|e| io(&format!("cannot write {}", path.display()), e))?;
    f.write_all(bytes).and_then(|_| f.sync_all()).map_err(|e| io(&format!("cannot write {}", path.display()), e))
}

fn atomic_write(path: &Path, bytes: &[u8]) -> Result<(), PatchError> {
    let tmp = path.with_extension("tmp");
    write_synced(&tmp, bytes)?;
    fs::rename(&tmp, path).map_err(|e| io("cannot replace a state file", e))
}

/// Copy `from` next to `target` and rename it over `target` (same folder, so the replacement is atomic).
fn swap_in(from: &Path, target: &Path, like: Option<&Path>) -> Result<(), PatchError> {
    let name = target.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let tmp = target.with_file_name(format!("{name}{TEMP_SUFFIX}"));
    // never write through a link or foreign file that happens to carry the temporary name (remove_file removes a link itself)
    let _ = fs::remove_file(&tmp);
    fs::copy(from, &tmp).map_err(|e| io(&format!("cannot stage {}", target.display()), e))?;
    #[cfg(unix)]
    if let Some(perm) = like.and_then(|l| fs::metadata(l).ok()).map(|m| m.permissions()) {
        let _ = fs::set_permissions(&tmp, perm);
    }
    #[cfg(not(unix))]
    let _ = like;
    if let Ok(f) = fs::File::open(&tmp) {
        let _ = f.sync_all();
    }
    fs::rename(&tmp, target).map_err(|e| {
        let _ = fs::remove_file(&tmp);
        io(&format!("cannot replace {}", target.display()), e)
    })
}

fn journal_add(work: &Path, line: &str) -> Result<(), PatchError> {
    let mut f = fs::OpenOptions::new().append(true).create(true).open(work.join("journal")).map_err(|e| io("cannot write the journal", e))?;
    writeln!(f, "{line}").and_then(|_| f.sync_all()).map_err(|e| io("cannot write the journal", e))
}

/// Undo an interrupted or failed run from the journal and the backups. Idempotent; a missing journal means nothing to do.
pub fn recover(root: &Path, work: &Path) -> Result<bool, PatchError> {
    let Ok(text) = fs::read_to_string(work.join("journal")) else {
        return Ok(false);
    };
    for line in text.lines().rev() {
        let Some((kind, rel)) = line.split_once(' ') else {
            continue;
        };
        let Ok(relp) = validate_rel(rel) else {
            continue;
        };
        match kind {
            "replace" => swap_in(&work.join("backup").join(&relp), &safe_target(root, &relp)?, None)?,
            "create" => {
                let _ = fs::remove_file(safe_target(root, &relp)?);
            }
            // only removes a folder that is empty again (and never follows a link: remove_dir fails on one)
            "mkdir" => {
                let _ = fs::remove_dir(root.join(&relp));
            }
            _ => {}
        }
    }
    let _ = fs::remove_file(work.join("journal"));
    let _ = fs::remove_dir_all(work.join("backup"));
    let _ = fs::remove_dir_all(work.join("staging"));
    Ok(true)
}

fn fetch_verified(a: &Artifact, work: &Path, fetch: &dyn Fetch, cancel: &AtomicBool) -> Result<PathBuf, PatchError> {
    let dest = work.join("artifacts").join(&a.name);
    fs::create_dir_all(dest.parent().unwrap()).map_err(|e| io("cannot create the artifact folder", e))?;
    let check = |p: &Path| matches!(file_sha256(p), Ok((h, n)) if h == a.sha256 && n == a.size);
    if dest.is_file() && check(&dest) {
        return Ok(dest); // an earlier, interrupted run already fetched and it still verifies
    }
    let _ = fs::remove_file(&dest);
    fetch.fetch(a, &dest, cancel)?;
    if !check(&dest) {
        let _ = fs::remove_file(&dest);
        return Err(PatchError::Hash(format!("artifact {} does not match the signed manifest; it was discarded", a.name)));
    }
    Ok(dest)
}

fn build_new(f: &FileEntry, action: Action, target: &Path, m: &Manifest, work: &Path, fetch: &dyn Fetch, cancel: &AtomicBool) -> Result<Vec<u8>, PatchError> {
    let bytes = match action {
        Action::Delta => {
            let a = m.artifact(f.delta.as_deref().unwrap_or("")).ok_or_else(|| PatchError::Manifest("delta artifact missing".into()))?;
            let d = fs::read(fetch_verified(a, work, fetch, cancel)?).map_err(|e| io("cannot read a delta", e))?;
            let old = fs::read(target).map_err(|e| io(&format!("cannot read {}", target.display()), e))?;
            if old.len() as u64 > delta::MAX_FILE {
                return Err(PatchError::Delta("base file too large for a delta".into()));
            }
            delta::apply(&old, &d)?
        }
        _ => {
            let a = m.artifact(f.full.as_deref().unwrap_or("")).ok_or_else(|| PatchError::Manifest("full artifact missing".into()))?;
            fs::read(fetch_verified(a, work, fetch, cancel)?).map_err(|e| io("cannot read an artifact", e))?
        }
    };
    if bytes.len() as u64 != f.new_size || sha256_hex(&bytes) != f.new_sha256 {
        return Err(PatchError::Hash(format!("the result for {} does not match the signed manifest", f.path)));
    }
    Ok(bytes)
}

/// Apply `m` to `root`. Returns the number of files changed. On any error the game folder is back to its previous state and
/// the recorded patch version is unchanged.
pub fn apply(root: &Path, work: &Path, m: &Manifest, fetch: &dyn Fetch, cancel: &AtomicBool, opt: &Options) -> Result<usize, PatchError> {
    if opt.game_running.is_some_and(|f| f()) {
        return Err(PatchError::Busy("the game is running; patches are never applied while it runs".into()));
    }
    if !root.is_dir() || fs::symlink_metadata(root).map(|m| m.file_type().is_symlink()).unwrap_or(true) {
        return Err(PatchError::UnsafePath("the game folder is missing or a symbolic link".into()));
    }
    fs::create_dir_all(work).map_err(|e| io("cannot create the patch work folder", e))?;
    let _lock = Lock::take(work)?;
    recover(root, work)?;
    let installed = installed_version(work)?;
    let plan = plan(root, m, installed)?;
    let step = |s: &str, i: usize, n: usize| {
        if let Some(p) = opt.progress {
            p(s, i, n)
        }
    };
    let todo: Vec<usize> = (0..m.files.len()).filter(|i| plan.actions[*i] != Action::Unchanged).collect();
    let staging = work.join("staging");
    let _ = fs::remove_dir_all(&staging);
    for (n, &i) in todo.iter().enumerate() {
        step("Preparing", n, todo.len());
        let f = &m.files[i];
        let target = safe_target(root, &validate_rel(&f.path)?)?;
        let bytes = build_new(f, plan.actions[i], &target, m, work, fetch, cancel)?;
        write_synced(&staging.join(i.to_string()), &bytes)?;
    }
    // point of no return: from here on a failure rolls back
    let result = (|| -> Result<(), PatchError> {
        for (n, &i) in todo.iter().enumerate() {
            step("Applying", n, todo.len());
            let f = &m.files[i];
            let rel = validate_rel(&f.path)?;
            let target = safe_target(root, &rel)?;
            let existed = target.is_file();
            if existed {
                let b = work.join("backup").join(&rel);
                fs::create_dir_all(b.parent().unwrap()).map_err(|e| io("cannot create the backup folder", e))?;
                fs::copy(&target, &b).map_err(|e| io(&format!("cannot back up {}", target.display()), e))?;
                if let Ok(h) = fs::File::open(&b) {
                    let _ = h.sync_all();
                }
            } else {
                let mut dir = PathBuf::new();
                let comps: Vec<_> = rel.components().collect();
                for c in &comps[..comps.len() - 1] {
                    dir.push(c);
                    if !root.join(&dir).exists() {
                        journal_add(work, &format!("mkdir {}", dir.to_string_lossy()))?;
                        fs::create_dir(root.join(&dir)).map_err(|e| io("cannot create a folder", e))?;
                    }
                }
            }
            journal_add(work, &format!("{} {}", if existed { "replace" } else { "create" }, f.path))?;
            if opt.fail_after_swaps == Some(n) {
                return Err(PatchError::Io("injected failure".into()));
            }
            swap_in(&staging.join(i.to_string()), &target, existed.then_some(target.as_path()))?;
        }
        step("Verifying", 0, todo.len());
        for &i in &todo {
            let f = &m.files[i];
            let target = safe_target(root, &validate_rel(&f.path)?)?;
            match hash_of(&target)? {
                Some((h, n)) if h == f.new_sha256 && n == f.new_size => {}
                _ => return Err(PatchError::Hash(format!("{} does not have the expected content after patching", f.path))),
            }
        }
        Ok(())
    })();
    if let Err(e) = result {
        recover(root, work)?; // restores the originals; if even this fails the journal stays for the next start
        return Err(e);
    }
    // the version is recorded last; if even that fails the files go back, so files and recorded version never disagree
    let record = atomic_write(&work.join("installed-manifest.txt"), m.to_text().as_bytes()).and_then(|_| atomic_write(&work.join("state"), format!("version = {}\n", m.version).as_bytes()));
    if let Err(e) = record {
        recover(root, work)?;
        return Err(e);
    }
    let _ = fs::remove_file(work.join("journal"));
    for d in ["backup", "staging", "artifacts"] {
        let _ = fs::remove_dir_all(work.join(d));
    }
    Ok(todo.len())
}

/// Files of the installed patch that no longer match (for a "repair" offer). Empty = intact.
pub fn drift(root: &Path, work: &Path) -> Result<Vec<String>, PatchError> {
    let Ok(text) = fs::read_to_string(work.join("installed-manifest.txt")) else {
        return Ok(Vec::new());
    };
    let m = Manifest::parse(&text)?;
    let mut bad = Vec::new();
    for f in &m.files {
        let t = safe_target(root, &validate_rel(&f.path)?)?;
        if !matches!(hash_of(&t)?, Some((h, _)) if h == f.new_sha256) {
            bad.push(f.path.clone());
        }
    }
    Ok(bad)
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;

    #[test]
    fn a_link_with_the_temporary_name_is_replaced_not_followed() {
        let d = std::env::temp_dir().join(format!("tsw-patch-swap-{}", std::process::id()));
        let _ = fs::remove_dir_all(&d);
        fs::create_dir_all(&d).unwrap();
        let (victim, from, target) = (d.join("victim"), d.join("new"), d.join("t.bin"));
        fs::write(&victim, b"precious").unwrap();
        fs::write(&from, b"new").unwrap();
        fs::write(&target, b"old").unwrap();
        std::os::unix::fs::symlink(&victim, d.join(format!("t.bin{TEMP_SUFFIX}"))).unwrap();
        swap_in(&from, &target, None).unwrap();
        assert_eq!(fs::read(&victim).unwrap(), b"precious", "the link target must stay untouched");
        assert_eq!(fs::read(&target).unwrap(), b"new");
        let _ = fs::remove_dir_all(&d);
    }
}
