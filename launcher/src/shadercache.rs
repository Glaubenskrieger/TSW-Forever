//! The client's local shader cache (`shader.cache.dx11.local3`, magic `SHCA`): inspection, reversible quarantine, restore.
//!
//! Why this exists (measured, the original component measurements): the client compiles shader permutations it does not find
//! in its shipped caches and stores the result in this file, and it reuses the file in every later run. A vertex shader that Wine's
//! built-in compiler (vkd3d-shader) had produced stayed in the cache through every compiler change of the tests V1..V7 (black character clothing);
//! with a rebuilt cache (V8, Microsoft's compiler through the native `d3dcompiler_43`) the clothing was correct.
//!
//! What the launcher does and does NOT claim: it reports a *fact about the file* (how many blobs carry the `vkd3d-shader` signature, and whether the
//! shader-compiler configuration of the prefix changed since the cache was last used). It does not claim that any particular shader is wrong.
//! The remedy is a user-controlled rebuild: the file is renamed (never deleted, never overwritten), and can be restored.

use crate::doctor::{Finding, Level};
use crate::prefix;
use std::path::{Path, PathBuf};

pub const LOCAL_CACHE: &str = "shader.cache.dx11.local3";
const QUARANTINE_MARK: &str = ".quarantine-";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Blob {
    pub offset: usize,
    pub size: usize,
    /// Compiled by Wine's built-in compiler (signature `vkd3d-shader` inside the DXBC container).
    pub vkd3d: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Scan {
    pub bytes: usize,
    pub blobs: Vec<Blob>,
}

impl Scan {
    pub fn vkd3d_count(&self) -> usize {
        self.blobs.iter().filter(|b| b.vkd3d).count()
    }
}

fn u32le(d: &[u8], o: usize) -> Option<u32> {
    d.get(o..o + 4).map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
}

/// Find the DXBC containers inside the cache. A container is `DXBC`, a 16-byte checksum, version, total size, chunk count; only plausible ones count.
pub fn scan_bytes(data: &[u8]) -> Result<Scan, String> {
    if data.get(4..8) != Some(b"SHCA".as_slice()) {
        return Err("not a shader cache (no SHCA magic)".into());
    }
    let mut blobs = Vec::new();
    let mut i = 8;
    while i + 32 <= data.len() {
        if &data[i..i + 4] == b"DXBC" {
            if let (Some(total), Some(chunks)) = (u32le(data, i + 24), u32le(data, i + 28)) {
                let (total, chunks) = (total as usize, chunks as usize);
                if total > 32 && chunks <= 32 && i + total <= data.len() {
                    let blob = &data[i..i + total];
                    let vkd3d = blob.windows(12).any(|w| w == b"vkd3d-shader");
                    blobs.push(Blob { offset: i, size: total, vkd3d });
                    i += total;
                    continue;
                }
            }
        }
        i += 1;
    }
    Ok(Scan { bytes: data.len(), blobs })
}

pub fn scan_file(path: &Path) -> Result<Scan, String> {
    scan_bytes(&std::fs::read(path).map_err(|e| format!("{} is not readable: {e}", path.display()))?)
}

/// All quarantined copies in `game_dir`, oldest first.
pub fn quarantined(game_dir: &Path) -> Vec<PathBuf> {
    let mut v: Vec<PathBuf> = std::fs::read_dir(game_dir)
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.file_name().and_then(|n| n.to_str()).is_some_and(|n| n.starts_with(&format!("{LOCAL_CACHE}{QUARANTINE_MARK}"))))
        .collect();
    v.sort();
    v
}

/// Rename the local cache so that the game rebuilds it. Nothing is deleted; refuses if there is nothing to move or the target name exists.
pub fn quarantine(game_dir: &Path, now: u64) -> Result<PathBuf, String> {
    let from = game_dir.join(LOCAL_CACHE);
    if !from.is_file() {
        return Err(format!("there is no {LOCAL_CACHE} in {} (the game creates it; nothing to do)", game_dir.display()));
    }
    // a second set-aside within the same second gets a counter instead of replacing or refusing
    let mut to = game_dir.join(format!("{LOCAL_CACHE}{QUARANTINE_MARK}{now:012}"));
    let mut n = 1;
    while to.exists() {
        to = game_dir.join(format!("{LOCAL_CACHE}{QUARANTINE_MARK}{now:012}-{n:03}"));
        n += 1;
    }
    std::fs::rename(&from, &to).map_err(|e| format!("cannot move {}: {e}", from.display()))?;
    Ok(to)
}

/// Put the newest quarantined copy back. A cache the game has written since is itself quarantined first, so nothing is lost either way.
pub fn restore(game_dir: &Path, now: u64) -> Result<PathBuf, String> {
    let newest = quarantined(game_dir).pop().ok_or_else(|| "no quarantined shader cache found".to_string())?;
    if game_dir.join(LOCAL_CACHE).is_file() {
        quarantine(game_dir, now)?;
    }
    let to = game_dir.join(LOCAL_CACHE);
    std::fs::rename(&newest, &to).map_err(|e| format!("cannot restore {}: {e}", newest.display()))?;
    Ok(to)
}

/// The shader-compiler configuration of a prefix, as far as the manifest records it: `native:<profile>` or `builtin`.
pub fn compiler_context(prefix: &Path) -> String {
    let text = std::fs::read_to_string(prefix.join(prefix::MANIFEST)).unwrap_or_default();
    let get = |key: &str| text.lines().find_map(|l| l.strip_prefix(key)?.strip_prefix('=')).map(|v| v.trim().to_string());
    let overrides = get("dll_overrides").unwrap_or_default();
    let native43 = overrides.split(';').any(|g| g.ends_with("=n") && g.trim_end_matches("=n").split(',').any(|d| d == "d3dcompiler_43"));
    match (native43, get("native_profile")) {
        (true, Some(p)) => format!("native:{p}"),
        (true, None) => "native:unprofiled".to_string(),
        _ => "builtin".to_string(),
    }
}

/// Launcher state file that remembers the shader-compiler setup each game folder was last started with (next to the configuration).
pub fn default_state_file() -> Option<PathBuf> {
    crate::config::write_path().and_then(|p| p.parent().map(|d| d.join("shader-cache-context.txt")))
}

fn key_of(game_dir: &Path) -> String {
    game_dir.display().to_string().replace(['\t', '\n'], " ")
}

/// Context recorded for `game_dir` at the last start (`<game dir>\t<context>\t<unix>` lines in the launcher's state file).
pub fn recorded_context(state_file: &Path, game_dir: &Path) -> Option<String> {
    let text = std::fs::read_to_string(state_file).ok()?;
    let key = key_of(game_dir);
    text.lines()
        .filter_map(|l| {
            let mut p = l.split('\t');
            (p.next()? == key).then(|| p.next().map(str::to_string))?
        })
        .next_back()
}

/// Remember the context this start runs with (replaces the old line for the same game folder). Best effort: diagnostics never stop a start.
pub fn record_context(state_file: &Path, game_dir: &Path, context: &str, now: u64) {
    let key = key_of(game_dir);
    let old = std::fs::read_to_string(state_file).unwrap_or_default();
    let mut lines: Vec<String> = old.lines().filter(|l| l.split('\t').next() != Some(key.as_str())).map(str::to_string).collect();
    lines.push(format!("{key}\t{context}\t{now}"));
    if let Some(dir) = state_file.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let _ = std::fs::write(state_file, lines.join("\n") + "\n");
}

/// Findings for the start checks. Pure reading.
pub fn check(game_dir: &Path, prefix: Option<&Path>, state_file: Option<&Path>) -> Vec<Finding> {
    let path = game_dir.join(LOCAL_CACHE);
    if !path.is_file() {
        return vec![Finding::ok("no local shader cache yet (the game creates it on the first run)".into()).coded("cache.none", Vec::<String>::new())];
    }
    let scan = match scan_file(&path) {
        Ok(s) => s,
        Err(e) => {
            return vec![Finding::bad(Level::Warn, format!("local shader cache could not be inspected: {e}"), "the game ignores or rebuilds an unreadable cache; nothing was changed")
                .coded("cache.unreadable", [e])]
        }
    };
    let mut f = Vec::new();
    let bad = scan.vkd3d_count();
    if bad > 0 {
        f.push(
            Finding::bad(
                Level::Warn,
                format!("the local shader cache holds {bad} of {} shader(s) compiled by Wine's built-in compiler (vkd3d-shader)", scan.blobs.len()),
                "reused in every run; in our tests such an entry kept character clothing black until the cache was rebuilt. This is a fact about the file, not proof that a shader is wrong. Rebuild with `tsw-launch cache quarantine` (reversible: `cache restore`)",
            )
            .coded("cache.vkd3d_blobs", [bad.to_string(), scan.blobs.len().to_string()]),
        );
    }
    if let (Some(prefix), Some(state)) = (prefix, state_file) {
        let now = compiler_context(prefix);
        if let Some(then) = recorded_context(state, game_dir) {
            if then != now {
                f.push(
                    Finding::bad(
                        Level::Warn,
                        format!("the local shader cache was last used with the shader compiler setup {then}, this prefix has {now}"),
                        "entries compiled under the old setup stay in the cache; consider rebuilding it with `tsw-launch cache quarantine` (reversible)",
                    )
                    .coded("cache.context_changed", [then, now]),
                );
            }
        }
    }
    if f.is_empty() {
        f.push(Finding::ok(format!("local shader cache: {} shader(s), none compiled by Wine's built-in compiler", scan.blobs.len())).coded("cache.ok", [scan.blobs.len().to_string()]));
    }
    f
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dxbc(sig: &[u8]) -> Vec<u8> {
        let mut body = vec![0u8; 8];
        body.extend_from_slice(sig);
        let total = 32 + 4 + 8 + body.len();
        let mut b = b"DXBC".to_vec();
        b.extend_from_slice(&[0u8; 16]);
        b.extend_from_slice(&1u32.to_le_bytes());
        b.extend_from_slice(&(total as u32).to_le_bytes());
        b.extend_from_slice(&1u32.to_le_bytes());
        b.extend_from_slice(&36u32.to_le_bytes());
        b.extend_from_slice(b"SHEX");
        b.extend_from_slice(&(body.len() as u32).to_le_bytes());
        b.extend_from_slice(&body);
        b
    }
    fn cache(blobs: &[&[u8]]) -> Vec<u8> {
        let mut c = b"\0\0\0\0SHCA!\0\0\0RIMLIGHT\0".to_vec();
        for b in blobs {
            c.extend_from_slice(b"\0KEY_A\0");
            c.extend_from_slice(&dxbc(b));
        }
        c
    }
    fn dir(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("tsw-shc-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn blobs_are_found_and_only_the_vkd3d_ones_are_flagged() {
        let s = scan_bytes(&cache(&[b"", b"vkd3d-shader 1.x", b""])).unwrap();
        assert_eq!((s.blobs.len(), s.vkd3d_count()), (3, 1));
        assert_eq!(scan_bytes(&cache(&[b""])).unwrap().vkd3d_count(), 0);
        assert!(scan_bytes(b"\0\0\0\0NOPE....").is_err());
        assert_eq!(scan_bytes(b"\0\0\0\0SHCA").unwrap().blobs.len(), 0);
        // a truncated container at the end is ignored, not an out-of-range read
        let mut c = cache(&[b""]);
        c.extend_from_slice(&dxbc(b"x")[..30]);
        assert_eq!(scan_bytes(&c).unwrap().blobs.len(), 1);
    }

    #[test]
    fn quarantine_renames_never_deletes_and_restore_brings_it_back() {
        let d = dir("q");
        assert!(quarantine(&d, 1).unwrap_err().contains("nothing to do"));
        let data = cache(&[b"vkd3d-shader"]);
        std::fs::write(d.join(LOCAL_CACHE), &data).unwrap();
        let q = quarantine(&d, 100).unwrap();
        assert!(!d.join(LOCAL_CACHE).exists() && q.is_file());
        assert_eq!(std::fs::read(&q).unwrap(), data, "content untouched");
        // the game writes a new cache; restoring must keep that one too
        let fresh = cache(&[b""]);
        std::fs::write(d.join(LOCAL_CACHE), &fresh).unwrap();
        let back = restore(&d, 200).unwrap();
        assert_eq!(std::fs::read(&back).unwrap(), data);
        let q2 = quarantined(&d);
        assert_eq!(q2.len(), 1);
        assert_eq!(std::fs::read(&q2[0]).unwrap(), fresh, "the newer cache was set aside, not overwritten");
        assert!(restore(&dir("empty"), 1).unwrap_err().contains("no quarantined"));
        // two set-asides in the same second keep both files
        std::fs::write(d.join(LOCAL_CACHE), cache(&[b"a"])).unwrap();
        let a = quarantine(&d, 300).unwrap();
        std::fs::write(d.join(LOCAL_CACHE), cache(&[b"b"])).unwrap();
        let b = quarantine(&d, 300).unwrap();
        assert!(a != b && a.is_file() && b.is_file());
        assert_eq!(quarantined(&d).last(), Some(&b), "the newest sorts last");
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn the_findings_state_facts_and_notice_a_changed_compiler_setup() {
        let d = dir("chk");
        assert_eq!(check(&d, None, None)[0].code, "cache.none");
        std::fs::write(d.join(LOCAL_CACHE), cache(&[b"vkd3d-shader", b""])).unwrap();
        let f = check(&d, None, None);
        assert!(f[0].level == Level::Warn && f[0].code == "cache.vkd3d_blobs" && f[0].what.contains("1 of 2"), "{f:?}");
        assert!(f[0].hint.contains("not proof"), "no claim about a specific shader");
        std::fs::write(d.join(LOCAL_CACHE), cache(&[b"", b""])).unwrap();
        assert_eq!(check(&d, None, None)[0].code, "cache.ok");
        // compiler context: builtin vs native profile, recorded per game folder
        let pre = d.join("prefix");
        std::fs::create_dir_all(&pre).unwrap();
        std::fs::write(pre.join(prefix::MANIFEST), "dxvk=installed\ndll_overrides=d3d11,dxgi=n,b\n").unwrap();
        assert_eq!(compiler_context(&pre), "builtin");
        std::fs::write(pre.join(prefix::MANIFEST), "dxvk=installed\ndll_overrides=d3d11,dxgi=n,b;d3dcompiler_47,d3dx9_43,d3dx11_43,d3dcompiler_43=n\nnative_profile=d3d-native-v7\n").unwrap();
        assert_eq!(compiler_context(&pre), "native:d3d-native-v7");
        let state = d.join("state/ctx.txt");
        assert_eq!(recorded_context(&state, &d), None);
        record_context(&state, &d, "builtin", 1);
        record_context(&state, &d, "builtin", 2);
        assert_eq!(std::fs::read_to_string(&state).unwrap().lines().count(), 1, "one line per game folder");
        let f = check(&d, Some(&pre), Some(&state));
        assert!(f.iter().any(|x| x.code == "cache.context_changed" && x.level == Level::Warn), "{f:?}");
        record_context(&state, &d, "native:d3d-native-v7", 3);
        assert_eq!(check(&d, Some(&pre), Some(&state))[0].code, "cache.ok");
        let _ = std::fs::remove_dir_all(&d);
    }
}
