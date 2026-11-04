//! Path safety for everything a manifest names. A manifest is signed, but a signed manifest can still contain a mistake (or a
//! key can leak), so paths are validated as if they were hostile: relative, normal components only, no Windows tricks,
//! no protected names, and no symbolic link anywhere on the way to the target.

use std::path::{Path, PathBuf};

use crate::PatchError;

/// File names that belong to the user (preferences, logs); a patch may never touch them.
const PROTECTED_NAMES: &[&str] = &["prefs_3.xml", "prefs_3.bxml", "clientlog.txt", "patcher.log", "last-launch.txt"];
/// First path components a patch may never touch: the original game data (RDB) is not ours to patch or redistribute.
const PROTECTED_ROOTS: &[&str] = &["rdb", "patches"];
/// Suffix of the temporary file next to a target; a manifest cannot name such a file.
pub const TEMP_SUFFIX: &str = ".tsw-new";

/// Validate a manifest path (always `/`-separated, relative to the game folder).
pub fn validate_rel(p: &str) -> Result<PathBuf, PatchError> {
    let bad = |why: &str| Err(PatchError::UnsafePath(format!("{p:?}: {why}")));
    if p.is_empty() || p.len() > 260 {
        return bad("empty or too long");
    }
    if p.chars().any(|c| c.is_control() || c == '\\' || c == ':' || c == '*' || c == '?' || c == '"' || c == '<' || c == '>' || c == '|') {
        return bad("contains a control, backslash, drive or wildcard character");
    }
    if p.starts_with('/') {
        return bad("absolute");
    }
    let mut out = PathBuf::new();
    let parts: Vec<&str> = p.split('/').collect();
    for (i, c) in parts.iter().enumerate() {
        if c.is_empty() || *c == "." || *c == ".." {
            return bad("empty, '.' or '..' component");
        }
        if c.len() > 255 || c.ends_with(' ') || c.ends_with('.') {
            return bad("component too long or ends with a space or dot");
        }
        let lower = c.to_ascii_lowercase();
        if i == 0 && PROTECTED_ROOTS.contains(&lower.as_str()) {
            return bad("protected folder");
        }
        if i == parts.len() - 1 && (PROTECTED_NAMES.contains(&lower.as_str()) || lower.ends_with(TEMP_SUFFIX)) {
            return bad("protected file name");
        }
        out.push(c);
    }
    Ok(out)
}

/// Resolve `rel` below `root` for writing: every existing component must be a real directory (no symlink), an existing target
/// must be a regular file. Does not create anything.
pub fn safe_target(root: &Path, rel: &Path) -> Result<PathBuf, PatchError> {
    let mut cur = root.to_path_buf();
    let comps: Vec<_> = rel.components().collect();
    for (i, c) in comps.iter().enumerate() {
        cur.push(c);
        match std::fs::symlink_metadata(&cur) {
            Ok(m) if m.file_type().is_symlink() => return Err(PatchError::UnsafePath(format!("{} is a symbolic link", cur.display()))),
            Ok(m) if i + 1 < comps.len() && !m.is_dir() => return Err(PatchError::UnsafePath(format!("{} is not a folder", cur.display()))),
            Ok(m) if i + 1 == comps.len() && !m.is_file() => return Err(PatchError::UnsafePath(format!("{} is not a regular file", cur.display()))),
            Ok(_) => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(PatchError::Io(format!("cannot inspect {}: {e}", cur.display()))),
        }
    }
    Ok(cur)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn good_paths_pass_and_hostile_ones_do_not() {
        assert_eq!(validate_rel("Data/Gui/Customized/Flash/a.swf").unwrap(), PathBuf::from("Data/Gui/Customized/Flash/a.swf"));
        assert!(validate_rel("my file (1).dll").is_ok());
        for bad in [
            "",
            "/etc/passwd",
            "../x",
            "a/../b",
            "a//b",
            "./a",
            "a/./b",
            "a\\b",
            "C:/x",
            "a:b",
            "a/b/",
            "a/..",
            "x\0y",
            "a\nb",
            "con*",
            "trailing /x",
            "dot./x",
            "RDB/0001.rdbdata",
            "rdb/x",
            "Patches/state",
            "Prefs_3.xml",
            "sub/prefs_3.BXML",
            "ClientLog.txt",
            "a/b.tsw-new",
            &"a".repeat(300),
            "a/<b>",
            "a|b",
        ] {
            assert!(validate_rel(bad).is_err(), "{bad:?}");
        }
    }

    #[cfg(unix)]
    #[test]
    fn symbolic_links_on_the_way_or_as_target_are_refused() {
        let d = std::env::temp_dir().join(format!("tsw-patch-path-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        let (root, outside) = (d.join("root"), d.join("outside"));
        std::fs::create_dir_all(root.join("real")).unwrap();
        std::fs::create_dir_all(&outside).unwrap();
        std::fs::write(outside.join("victim"), b"x").unwrap();
        std::os::unix::fs::symlink(&outside, root.join("linkdir")).unwrap();
        std::os::unix::fs::symlink(outside.join("victim"), root.join("real/linkfile")).unwrap();
        std::fs::write(root.join("real/ok.bin"), b"ok").unwrap();
        assert!(safe_target(&root, Path::new("linkdir/victim")).is_err());
        assert!(safe_target(&root, Path::new("real/linkfile")).is_err());
        assert!(safe_target(&root, Path::new("real/ok.bin")).is_ok());
        assert!(safe_target(&root, Path::new("real/new/deeper.bin")).is_ok(), "missing parts are fine");
        assert!(safe_target(&root, Path::new("real/ok.bin/x")).is_err(), "a file used as a folder");
        std::fs::create_dir_all(root.join("real/adir")).unwrap();
        assert!(safe_target(&root, Path::new("real/adir")).is_err(), "a folder as target");
        let _ = std::fs::remove_dir_all(&d);
    }
}
