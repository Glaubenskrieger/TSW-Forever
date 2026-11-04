//! The patch manifest: a small line-based text file. Grammar (anything else is an error; there is no way to express a command):
//!
//! ```text
//! tsw-forever-patch 1
//! version 3                      # patch version this manifest installs (integer, > installed version)
//! requires 2                     # lowest installed patch version this manifest can be applied on (0 = original client)
//! artifact <name> <sha256> <size>
//! file new=<sha256> size=<n> [full=<artifact>] [from=<sha256> delta=<artifact>] path=<relative/path with spaces allowed>
//! ```
//! Comments start with `#` at the beginning of a line. `file` lines must offer at least one source (`full`, or `from`+`delta`).

use std::collections::HashSet;

use crate::delta::MAX_FILE;
use crate::path::validate_rel;
use crate::sha256::is_hex_digest;
use crate::PatchError;

const HEADER_PREFIX: &str = "tsw-forever-patch ";
/// The first line this engine writes and reads.
pub const HEADER: &str = "tsw-forever-patch 1";
pub const MAX_MANIFEST_BYTES: usize = 1 << 20;
pub const MAX_FILES: usize = 10_000;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Artifact {
    pub name: String,
    pub sha256: String,
    pub size: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileEntry {
    pub path: String,
    pub new_sha256: String,
    pub new_size: u64,
    pub full: Option<String>,
    pub from_sha256: Option<String>,
    pub delta: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Manifest {
    pub version: u64,
    pub requires: u64,
    pub artifacts: Vec<Artifact>,
    pub files: Vec<FileEntry>,
}

fn artifact_name_ok(n: &str) -> bool {
    !n.is_empty() && n.len() <= 128 && !n.starts_with('.') && n.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'))
}

impl Manifest {
    pub fn artifact(&self, name: &str) -> Option<&Artifact> {
        self.artifacts.iter().find(|a| a.name == name)
    }

    pub fn parse(text: &str) -> Result<Manifest, PatchError> {
        if text.len() > MAX_MANIFEST_BYTES {
            return Err(PatchError::Manifest("manifest too large".into()));
        }
        let mut lines = text.lines().enumerate().map(|(i, l)| (i + 1, l)).filter(|(_, l)| !l.trim().is_empty() && !l.trim_start().starts_with('#'));
        let err = |n: usize, m: &str| Err(PatchError::Manifest(format!("line {n}: {m}")));
        match lines.next() {
            Some((_, h)) if h.trim() == HEADER => {}
            // a well-formed header with another number: tell "too new" from "garbage", so the user is told to update the launcher
            Some((n, h)) => {
                return match h.trim().strip_prefix(HEADER_PREFIX).and_then(|v| v.parse::<u32>().ok()) {
                    Some(v) if v > crate::MANIFEST_FORMAT => err(n, &format!("manifest format {v} needs a newer launcher (this one reads format {})", crate::MANIFEST_FORMAT)),
                    Some(v) => err(n, &format!("manifest format {v} is no longer supported (this launcher reads format {})", crate::MANIFEST_FORMAT)),
                    None => err(n, "unknown format header"),
                }
            }
            None => return Err(PatchError::Manifest("empty manifest".into())),
        }
        let (mut version, mut requires) = (None, None);
        let (mut artifacts, mut files): (Vec<Artifact>, Vec<FileEntry>) = (Vec::new(), Vec::new());
        let (mut paths, mut names) = (HashSet::new(), HashSet::new());
        for (n, line) in lines {
            let (kw, rest) = line.split_once(' ').unwrap_or((line, ""));
            match kw {
                "version" | "requires" => {
                    let v: u64 = rest.trim().parse().map_err(|_| PatchError::Manifest(format!("line {n}: {kw} needs an integer")))?;
                    let slot = if kw == "version" { &mut version } else { &mut requires };
                    if slot.replace(v).is_some() {
                        return err(n, &format!("duplicate {kw}"));
                    }
                }
                "artifact" => {
                    let f: Vec<&str> = rest.split_whitespace().collect();
                    let [name, sha, size] = f[..] else {
                        return err(n, "artifact needs <name> <sha256> <size>");
                    };
                    let size: u64 = size.parse().map_err(|_| PatchError::Manifest(format!("line {n}: bad size")))?;
                    if !artifact_name_ok(name) || !is_hex_digest(sha) || size > MAX_FILE {
                        return err(n, "bad artifact name, checksum or size");
                    }
                    if !names.insert(name.to_string()) {
                        return err(n, "duplicate artifact name");
                    }
                    artifacts.push(Artifact { name: name.into(), sha256: sha.to_ascii_lowercase(), size });
                }
                "file" => {
                    let (head, path) = rest.split_once(" path=").ok_or_else(|| PatchError::Manifest(format!("line {n}: file needs path= as its last field")))?;
                    validate_rel(path)?;
                    if !paths.insert(path.to_ascii_lowercase()) {
                        return err(n, "duplicate path (case-insensitive)");
                    }
                    let mut e = FileEntry { path: path.into(), new_sha256: String::new(), new_size: u64::MAX, full: None, from_sha256: None, delta: None };
                    for tok in head.split_whitespace() {
                        let (k, v) = tok.split_once('=').ok_or_else(|| PatchError::Manifest(format!("line {n}: expected key=value, got {tok:?}")))?;
                        match k {
                            "new" if is_hex_digest(v) => e.new_sha256 = v.to_ascii_lowercase(),
                            "from" if is_hex_digest(v) => e.from_sha256 = Some(v.to_ascii_lowercase()),
                            "size" => e.new_size = v.parse().map_err(|_| PatchError::Manifest(format!("line {n}: bad size")))?,
                            "full" if artifact_name_ok(v) => e.full = Some(v.into()),
                            "delta" if artifact_name_ok(v) => e.delta = Some(v.into()),
                            _ => return err(n, &format!("unknown or invalid field {k:?}")),
                        }
                    }
                    if e.new_sha256.is_empty() || e.new_size > MAX_FILE {
                        return err(n, "file needs new=<sha256> and a plausible size=");
                    }
                    if e.from_sha256.is_some() != e.delta.is_some() {
                        return err(n, "from= and delta= belong together");
                    }
                    if e.full.is_none() && e.delta.is_none() {
                        return err(n, "file offers no source (full= or from=+delta=)");
                    }
                    files.push(e);
                    if files.len() > MAX_FILES {
                        return err(n, "too many files");
                    }
                }
                _ => return err(n, &format!("unknown directive {kw:?}")),
            }
        }
        let version = version.ok_or_else(|| PatchError::Manifest("missing version".into()))?;
        let requires = requires.ok_or_else(|| PatchError::Manifest("missing requires".into()))?;
        if version == 0 || requires >= version {
            return Err(PatchError::Manifest("version must be > 0 and greater than requires".into()));
        }
        // a path may not be the folder of another path ("a" and "a/b"): that could only fail halfway through an update
        for f in &files {
            let dir = format!("{}/", f.path.to_ascii_lowercase());
            if paths.iter().any(|p| p.starts_with(&dir)) {
                return Err(PatchError::Manifest(format!("{}: is both a file and a folder of another entry", f.path)));
            }
        }
        for f in &files {
            for a in f.full.iter().chain(f.delta.iter()) {
                if !names.contains(a) {
                    return Err(PatchError::Manifest(format!("{}: unknown artifact {a:?}", f.path)));
                }
            }
        }
        Ok(Manifest { version, requires, artifacts, files })
    }

    /// Deterministic text form (used by the release tool and tests); `parse(to_text(m)) == m`.
    pub fn to_text(&self) -> String {
        let mut s = format!("{HEADER}\nversion {}\nrequires {}\n", self.version, self.requires);
        for a in &self.artifacts {
            s.push_str(&format!("artifact {} {} {}\n", a.name, a.sha256, a.size));
        }
        for f in &self.files {
            s.push_str(&format!("file new={} size={}", f.new_sha256, f.new_size));
            if let Some(x) = &f.full {
                s.push_str(&format!(" full={x}"));
            }
            if let (Some(from), Some(d)) = (&f.from_sha256, &f.delta) {
                s.push_str(&format!(" from={from} delta={d}"));
            }
            s.push_str(&format!(" path={}\n", f.path));
        }
        s
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const H: &str = "aa";
    fn h(c: char) -> String {
        std::iter::repeat_n(c, 64).collect()
    }
    fn sample() -> String {
        format!(
            "{HEADER}\n# comment\nversion 3\nrequires 2\nartifact full-a {} 10\nartifact delta-a {} 5\nfile new={} size=10 full=full-a from={} delta=delta-a path=bin/My Client.exe\nfile new={} size=10 full=full-a path=Data/x.dat\n",
            h('a'), h('b'), h('c'), h('d'), h('e')
        )
    }

    #[test]
    fn a_valid_manifest_parses_and_round_trips() {
        let m = Manifest::parse(&sample()).unwrap();
        assert_eq!((m.version, m.requires, m.artifacts.len(), m.files.len()), (3, 2, 2, 2));
        assert_eq!(m.files[0].path, "bin/My Client.exe", "spaces in paths are fine");
        assert_eq!(Manifest::parse(&m.to_text()).unwrap(), m);
        let _ = H;
    }

    #[test]
    fn the_manifest_format_is_not_the_patch_version_and_other_formats_are_told_apart() {
        assert_eq!(HEADER, format!("{HEADER_PREFIX}{}", crate::MANIFEST_FORMAT));
        let body = "version 2\nrequires 1\n";
        let msg = |h: &str| match Manifest::parse(&format!("{h}\n{body}")) {
            Err(PatchError::Manifest(m)) => m,
            other => panic!("{other:?}"),
        };
        assert!(msg("tsw-forever-patch 2").contains("needs a newer launcher"));
        assert!(msg("tsw-forever-patch 0").contains("no longer supported"));
        assert!(msg("tsw-forever-patch x").contains("unknown format header"));
        assert!(msg("tsw-forever-patch 1.1").contains("unknown format header"));
        assert!(msg("version 2").contains("unknown format header"), "the patch version line is not a format header");
    }

    #[test]
    fn a_file_may_not_also_be_the_folder_of_another_entry() {
        let t = format!("{HEADER}\nversion 2\nrequires 1\nartifact f {} 1\nfile new={} size=1 full=f path=Data\nfile new={} size=1 full=f path=data/x.dat\n", h('a'), h('b'), h('c'));
        assert!(matches!(Manifest::parse(&t), Err(PatchError::Manifest(m)) if m.contains("file and a folder")));
    }

    #[test]
    fn malformed_manifests_are_rejected_with_a_reason() {
        let ok = sample();
        let cases: Vec<(String, &str)> = vec![
            (ok.replace(HEADER, "tsw-forever-patch 2"), "header"),
            (ok.replace("version 3\n", ""), "missing version"),
            (ok.replace("requires 2", "requires 3"), "requires >= version"),
            (ok.replace("version 3", "version 3\nversion 4"), "duplicate version"),
            (ok.replace("full=full-a path=Data", "full=nope path=Data"), "unknown artifact"),
            (ok.replace(&h('e'), "short"), "bad checksum"),
            (ok.replace("path=Data/x.dat", "path=../evil"), "traversal"),
            (ok.replace("path=Data/x.dat", "path=bin/my client.exe"), "case-insensitive duplicate"),
            (ok.replace("path=Data/x.dat", "path=/abs"), "absolute"),
            (format!("{ok}run /bin/sh -c 'x'\n"), "unknown directive (no commands exist)"),
            (format!("{ok}exec rm -rf\n"), "exec"),
            (ok.replace("full=full-a path=Data", "full=full-a bogus=1 path=Data"), "unknown field"),
            (ok.replace(" from=", " fromx="), "unknown field 2"),
            (ok.replace("delta=delta-a ", ""), "from without delta"),
            (ok.replace("full=full-a path=Data", "path=Data"), "no source"),
            (ok.replace("artifact full-a", "artifact ../full-a"), "artifact name"),
            (ok.replace("artifact delta-a", "artifact full-a"), "duplicate artifact"),
            ("x".repeat(MAX_MANIFEST_BYTES + 1), "too large"),
            (String::new(), "empty"),
        ];
        for (text, why) in cases {
            assert!(Manifest::parse(&text).is_err(), "{why}");
        }
    }
}
