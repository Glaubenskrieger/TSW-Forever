//! Release tool: builds `manifest.txt` and the artifacts from an OLD tree (the files as they are before the patch) and a NEW tree
//! (the patched files). Only files that differ are listed. Signing is a separate step (minisign in the release pipeline).
//!
//!   tsw-patch-pack --old <dir> --new <dir> --out <dir> --version N [--requires M]
//!
//! The trees are read only; the output goes to a new/empty `--out` directory. Never run it over files that must not leave the
//! machine: `--out` contains full copies of every changed file.

use std::path::{Path, PathBuf};

use tsw_patch::delta;
use tsw_patch::manifest::{Artifact, FileEntry, Manifest};
use tsw_patch::path::validate_rel;
use tsw_patch::sha256::sha256_hex;

fn walk(root: &Path, dir: &Path, out: &mut Vec<String>) -> std::io::Result<()> {
    let mut entries: Vec<_> = std::fs::read_dir(dir)?.flatten().collect();
    entries.sort_by_key(|e| e.file_name());
    for e in entries {
        let ft = e.file_type()?;
        if ft.is_symlink() {
            return Err(std::io::Error::other(format!("{} is a symbolic link", e.path().display())));
        }
        if ft.is_dir() {
            walk(root, &e.path(), out)?;
        } else {
            out.push(e.path().strip_prefix(root).unwrap().components().map(|c| c.as_os_str().to_string_lossy().into_owned()).collect::<Vec<_>>().join("/"));
        }
    }
    Ok(())
}

fn run() -> Result<(), String> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let get = |k: &str| args.iter().position(|a| a == k).and_then(|i| args.get(i + 1)).cloned();
    let (old, new, out) = (get("--old").ok_or("--old missing")?, get("--new").ok_or("--new missing")?, get("--out").ok_or("--out missing")?);
    let version: u64 = get("--version").ok_or("--version missing")?.parse().map_err(|_| "--version must be an integer")?;
    let requires: u64 = get("--requires").map(|v| v.parse().map_err(|_| "--requires must be an integer")).transpose()?.unwrap_or(0);
    let (old, new, out) = (PathBuf::from(old), PathBuf::from(new), PathBuf::from(out));
    if out.exists() && std::fs::read_dir(&out).map_err(|e| e.to_string())?.next().is_some() {
        return Err("--out must be empty".into());
    }
    std::fs::create_dir_all(&out).map_err(|e| e.to_string())?;
    let mut rels = Vec::new();
    walk(&new, &new, &mut rels).map_err(|e| e.to_string())?;
    let (mut artifacts, mut files) = (Vec::new(), Vec::new());
    let mut add = |name: String, data: &[u8]| -> Result<String, String> {
        std::fs::write(out.join(&name), data).map_err(|e| e.to_string())?;
        artifacts.push(Artifact { name: name.clone(), sha256: sha256_hex(data), size: data.len() as u64 });
        Ok(name)
    };
    for rel in rels {
        validate_rel(&rel).map_err(|e| e.to_string())?;
        let nb = std::fs::read(new.join(&rel)).map_err(|e| format!("{rel}: {e}"))?;
        let ob = std::fs::read(old.join(&rel)).ok();
        if ob.as_deref() == Some(&nb[..]) {
            continue;
        }
        let new_sha = sha256_hex(&nb);
        let full = add(format!("f-{}", &new_sha[..24]), &nb)?;
        let (mut from, mut dname) = (None, None);
        if let Some(ob) = &ob {
            let d = delta::create(ob, &nb);
            if d.len() * 2 < nb.len() {
                from = Some(sha256_hex(ob));
                dname = Some(add(format!("d-{}", &sha256_hex(&d)[..24]), &d)?);
            }
        }
        files.push(FileEntry { path: rel, new_sha256: new_sha, new_size: nb.len() as u64, full: Some(full), from_sha256: from, delta: dname });
    }
    artifacts.dedup_by(|a, b| a.name == b.name);
    let m = Manifest { version, requires, artifacts, files };
    std::fs::write(out.join("manifest.txt"), m.to_text()).map_err(|e| e.to_string())?;
    println!("{} file(s) changed, {} artifact(s); sign manifest.txt with minisign", m.files.len(), m.artifacts.len());
    Ok(())
}

fn main() {
    if let Err(e) = run() {
        eprintln!("tsw-patch-pack: {e}");
        std::process::exit(1);
    }
}
