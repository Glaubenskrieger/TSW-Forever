//! Wine prefix manager: creates and verifies the prefix the original client runs in.
//!
//! What is installed, and why (each item verified with Wine `+loaddll` traces of the 32-bit DX11 client, see
//! documentation/third-party-components.md):
//!
//! * DXVK `d3d11.dll` and `dxgi.dll` (x86_64 -> `system32`, i386 -> `syswow64`): the client loads exactly these two.
//!   They only take effect together with the load-order override `d3d11,dxgi=n,b` ([`dll_overrides`]).
//! * vkd3d `libvkd3d-1.dll`, `libvkd3d-shader-1.dll`, `libvkd3d-utils-1.dll`: Wine's `wined3d.dll`, which the client's
//!   D3DX/DirectDraw side loads, imports them. Without them the client dies with `c0000135`.
//! * Nothing else by default: no `d3d9` (in every traced run only the `dxdiag` helper process loads it, never the client
//!   process itself; adding DXVK `d3d9`+`d3d10core`, which Proton installs by default, changed no measured value, see
//!   documentation/third-party-components.md), no `d3dcompiler`/`d3dx` replacements. That is the current state, not a promise: the retained
//!   ClientLog of a played session shows Wine's built-in HLSL compiler failing for the model shaders (documentation/third-party-components.md).
//!
//! The overrides the client runs with are data, not code: the prefix manifest lists them (`dll_overrides=`) together with
//! the MD5 of every installed component. [`dll_overrides`] hands out only a *validated* manifest value, and [`check`]
//! refuses a native override that has no matching, unchanged component. A future tested graphics profile therefore only
//! has to write such a manifest; nothing here needs a special case.
//!
//! A prefix must be created while the 32-bit FreeType/Fontconfig libraries exist: otherwise Wine stores nonsensical
//! window metrics (observed: `CaptionHeight=5434094`), which gives a window of millions of pixels. [`check_metrics`]
//! detects that and the manager refuses such a prefix instead of silently using it.

use std::path::{Path, PathBuf};
use std::process::Command;

use crate::doctor::{Finding, Level};
use crate::md5;
use crate::progress::{Outcome, Phase, Progress, RateMeter, Silent, Sink, Update};

pub const MANIFEST: &str = "tsw-launch-prefix.txt";
const OWNER: &str = ".tsw-launch-owner";
const PENDING_OWNER: &str = "tsw-launch prefix ownership v1\ncreation=pending\n";
const COMPLETE_OWNER: &str = "tsw-launch prefix ownership v1\ncreation=complete\n";
const DXVK: &[&str] = &["d3d11.dll", "dxgi.dll"];
const VKD3D: &[&str] = &["libvkd3d-1.dll", "libvkd3d-shader-1.dll", "libvkd3d-utils-1.dll"];
/// The only DLLs a manifest may override (graphics stack of the client). Anything else is rejected, not passed on.
const OVERRIDABLE: &[&str] = &["d3d11", "dxgi", "d3d10core", "d3d9", "d3dcompiler_43", "d3dcompiler_47", "d3dx9_43", "d3dx11_43"];
/// Plausible pixel range for caption/menu heights (healthy Wine prefixes: 25 / 18 / 17).
const METRIC_RANGE: std::ops::RangeInclusive<i64> = 8..=64;

/// One file to place into the prefix.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CopyOp {
    pub from: PathBuf,
    pub to: PathBuf,
}

/// The Wine/Proton runtime tree (`.../files`): its `bin/wine` decides the runtime directory.
pub fn runtime_files_dir(wine: &Path) -> Option<PathBuf> {
    let files = wine.parent()?.parent()?;
    files.join("lib").join("wine").is_dir().then(|| files.to_path_buf())
}

/// Files that must be copied for a 64-bit prefix with a 32-bit client. Pure function: no filesystem access.
pub fn plan_components(runtime_files: &Path, prefix: &Path) -> Vec<CopyOp> {
    let win = prefix.join("drive_c").join("windows");
    let mut ops = Vec::new();
    for (arch, dest) in [("x86_64-windows", "system32"), ("i386-windows", "syswow64")] {
        for name in DXVK {
            ops.push(CopyOp { from: runtime_files.join("lib/wine/dxvk").join(arch).join(name), to: win.join(dest).join(name) });
        }
        for name in VKD3D {
            ops.push(CopyOp { from: runtime_files.join("lib/vkd3d").join(arch).join(name), to: win.join(dest).join(name) });
        }
    }
    ops
}

/// Value of `key=` in the manifest text (first match).
fn manifest_value<'a>(text: &'a str, key: &str) -> Option<&'a str> {
    text.lines().find_map(|l| l.strip_prefix(key)?.strip_prefix('=')).map(str::trim)
}

/// `(relative path, md5)` of every `component=` line.
fn manifest_components(text: &str) -> Vec<(&str, &str)> {
    text.lines().filter_map(|l| l.strip_prefix("component=")?.rsplit_once(" md5=")).collect()
}

/// Recognise the complete historical manifest, not a file name or Wine's user.reg.
/// Component drift is handled by the existing integrity checks; it does not erase provenance.
fn owned_manifest(text: &str) -> bool {
    if text.lines().next() != Some("# written by tsw-launch prefix init; do not edit") {
        return false;
    }
    for (key, value) in [("arch", "win64+wow64 (32-bit client)"), ("dxvk", "installed"), ("vkd3d", "installed")] {
        if text.lines().filter(|l| l.starts_with(&format!("{key}="))).count() != 1 || manifest_value(text, key) != Some(value) {
            return false;
        }
    }
    if !["runtime", "runtime_dir"].iter().all(|k| manifest_value(text, k).is_some_and(|v| !v.is_empty())) || !manifest_value(text, "dll_overrides").is_some_and(|v| validate_overrides(v).is_ok()) {
        return false;
    }
    for key in ["runtime", "runtime_dir", "dll_overrides"] {
        if text.lines().filter(|l| l.starts_with(&format!("{key}="))).count() != 1 {
            return false;
        }
    }
    let components = manifest_components(text);
    if components.len() != text.lines().filter(|l| l.starts_with("component=")).count() {
        return false;
    }
    let mut paths = std::collections::HashSet::new();
    for (rel, hash) in &components {
        if !rel.starts_with("drive_c/windows/")
            || Path::new(rel).components().any(|c| !matches!(c, std::path::Component::Normal(_)))
            || !paths.insert(*rel)
            || hash.len() != 32
            || !hash.bytes().all(|b| b.is_ascii_hexdigit())
        {
            return false;
        }
    }
    ["system32", "syswow64"].iter().all(|arch| DXVK.iter().chain(VKD3D).all(|dll| paths.contains(format!("drive_c/windows/{arch}/{dll}").as_str())))
}

/// Read-only gate for EVERY prefix initialisation. An unfinished creation is resumable only
/// with the marker that we created in an empty directory before invoking Wine.
pub fn may_initialise(prefix: &Path) -> Result<(), String> {
    let refused = || {
        format!(
            "{} is not a proven TSW-Forever prefix (missing or damaged ownership/manifest); nothing was changed. Choose a new empty prefix folder; do not initialise an existing foreign prefix",
            prefix.display()
        )
    };
    if prefix.symlink_metadata().is_ok_and(|m| m.file_type().is_symlink() || !m.is_dir()) {
        return Err(refused());
    }
    for name in [MANIFEST, OWNER] {
        if prefix.join(name).symlink_metadata().is_ok_and(|m| !m.is_file()) {
            return Err(refused());
        }
    }
    match std::fs::read_to_string(prefix.join(MANIFEST)) {
        Ok(text) => {
            if !owned_manifest(&text) {
                return Err(refused());
            }
            match std::fs::read_to_string(prefix.join(OWNER)) {
                Ok(owner) if owner == PENDING_OWNER || owner == COMPLETE_OWNER => Ok(()),
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()), // verified legacy installation
                _ => Err(refused()),
            }
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            if let Ok(owner) = std::fs::read_to_string(prefix.join(OWNER)) {
                return if owner == PENDING_OWNER { Ok(()) } else { Err(refused()) };
            }
            match std::fs::read_dir(prefix) {
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
                Ok(mut entries) => {
                    if entries.next().is_none() {
                        Ok(())
                    } else {
                        Err(refused())
                    }
                }
                _ => Err(refused()),
            }
        }
        _ => Err(refused()),
    }
}

fn claim_prefix(prefix: &Path) -> Result<(), String> {
    may_initialise(prefix)?;
    std::fs::create_dir_all(prefix).map_err(|e| format!("cannot create prefix: {e}"))?;
    if !prefix.join(OWNER).exists() {
        use std::io::Write;
        let mut f = std::fs::OpenOptions::new().write(true).create_new(true).open(prefix.join(OWNER)).map_err(|e| format!("cannot record prefix provenance: {e}"))?;
        let owner = if prefix.join(MANIFEST).exists() { COMPLETE_OWNER } else { PENDING_OWNER };
        f.write_all(owner.as_bytes()).and_then(|()| f.sync_all()).map_err(|e| format!("cannot save prefix provenance: {e}"))?;
    }
    Ok(())
}

/// Validate a `WINEDLLOVERRIDES` value from a manifest: `dll[,dll]=mode[;...]`, DLLs from [`OVERRIDABLE`], mode `n`, `b`,
/// `n,b` or `b,n` (never `d`: disabling a DLL is not a graphics profile). Returns the DLLs that load a native file first.
pub fn validate_overrides(value: &str) -> Result<Vec<String>, String> {
    let mut native = Vec::new();
    for entry in value.split(';').filter(|e| !e.trim().is_empty()) {
        let (names, mode) = entry.split_once('=').ok_or_else(|| format!("override entry {entry:?} has no `=`"))?;
        if !matches!(mode.trim(), "n" | "b" | "n,b" | "b,n") {
            return Err(format!("override mode {mode:?} is not one of n, b, n,b, b,n"));
        }
        for name in names.split(',').map(str::trim) {
            if !OVERRIDABLE.contains(&name) {
                return Err(format!("{name:?} is not a DLL a prefix manifest may override"));
            }
            if mode.trim().starts_with('n') {
                native.push(name.to_string());
            }
        }
    }
    Ok(native)
}

/// The manifest's overrides, but only if the manifest enables DXVK and the value passes [`validate_overrides`].
/// `None` for a missing manifest, a manifest without DXVK, or an invalid value ([`check`] reports the reason).
pub fn dll_overrides(prefix: &Path) -> Option<String> {
    let text = std::fs::read_to_string(prefix.join(MANIFEST)).ok()?;
    text.lines().any(|l| l.trim() == "dxvk=installed").then_some(())?;
    let value = manifest_value(&text, "dll_overrides").unwrap_or("d3d11,dxgi=n,b");
    validate_overrides(value).ok().map(|_| value.to_string())
}

/// Findings about the manifest of a prefix: the override value, a verified component behind every native override, and
/// no component changed on disk since `prefix init` (for example overwritten by a Wine update).
pub fn check_manifest(prefix: &Path) -> Vec<Finding> {
    let Ok(text) = std::fs::read_to_string(prefix.join(MANIFEST)) else {
        return Vec::new();
    };
    let mut f = Vec::new();
    let components = manifest_components(&text);
    if let Some(value) = manifest_value(&text, "dll_overrides") {
        match validate_overrides(value) {
            Err(why) => f.push(
                Finding::bad(
                    Level::Fail,
                    format!("manifest override rejected: {why}"),
                    "only the documented graphics DLLs with mode n/b may be overridden; recreate the prefix with `tsw-launch prefix init`",
                )
                .coded("prefix.override_rejected", [why]),
            ),
            Ok(native) => {
                for dll in native {
                    let file = format!("{dll}.dll");
                    if !components.iter().any(|(rel, _)| Path::new(rel).file_name().is_some_and(|n| n == file.as_str())) {
                        f.push(
                            Finding::bad(
                                Level::Fail,
                                format!("native override for {dll} has no verified component in the manifest"),
                                "a native override is only valid together with the installed, hash-recorded file",
                            )
                            .coded("prefix.native_unverified", [dll.clone()]),
                        );
                    }
                }
            }
        }
    }
    let recorded = components.len();
    for (rel, md5) in components {
        match std::fs::read(prefix.join(rel)) {
            Ok(b) if md5::md5_hex(&b) == md5 => {}
            Ok(_) => f.push(
                Finding::bad(Level::Warn, format!("{rel} changed since the prefix was prepared"), "re-run `tsw-launch prefix init` to restore the recorded component")
                    .coded("prefix.component_changed", [rel]),
            ),
            Err(_) => f.push(Finding::bad(Level::Fail, format!("{rel} from the manifest is missing"), "run `tsw-launch prefix init`").coded("prefix.component_gone", [rel])),
        }
    }
    if f.is_empty() {
        let overrides = manifest_value(&text, "dll_overrides").unwrap_or("(default)");
        f.push(Finding::ok(format!("manifest: {recorded} components unchanged, overrides {overrides}")).coded("prefix.manifest_ok", [recorded.to_string(), overrides.to_string()]));
    }
    f
}

/// Merge our override with one the caller already has (theirs first; `;`-separated like WINEDLLOVERRIDES).
pub fn merge_overrides(existing: Option<&str>, ours: &str) -> String {
    match existing.filter(|e| !e.trim().is_empty()) {
        Some(e) => format!("{e};{ours}"),
        None => ours.to_string(),
    }
}

/// A native (non-Wine) graphics DLL of a [`NativeProfile`]: name without `.dll`, and the size and MD5 of the exact file.
/// The launcher never ships these files: they come from a directory (`--native-dir`, or the cache that [`crate::dxredist`] fills from Microsoft's
/// own download on request), and a file is only installed if size and MD5 match, so a wrong or modified DLL is refused instead of silently used.
#[derive(Debug, Clone, Copy)]
pub struct NativeFile {
    pub dll: &'static str,
    pub size: u64,
    pub md5: &'static str,
}

/// A named, reproducible set of native DLLs for the 32-bit client (installed into `syswow64`, enabled by `=n` overrides).
#[derive(Debug, Clone, Copy)]
pub struct NativeProfile {
    pub id: &'static str,
    pub files: &'static [NativeFile],
}

/// UNCONFIRMED HYPOTHESIS (see documentation/third-party-components.md): V6 (native `d3dcompiler_47`, `d3dx9_43`,
/// `d3dx11_43`; reaches the world, outfit black) plus the one DLL that every correctly rendering run had native and every black run
/// had builtin: `d3dcompiler_43` (Direct3D reflection, and the compiler behind native `d3dx11_43`).
pub const PROFILE_D3D_NATIVE_V7: NativeProfile = NativeProfile {
    id: "d3d-native-v7",
    files: &[
        NativeFile { dll: "d3dcompiler_47", size: 3_681_592, md5: "eee83660394f290e3ea5faac41c23a70" },
        NativeFile { dll: "d3dx9_43", size: 1_998_168, md5: "86e39e9161c3d930d93822f1563c280d" },
        NativeFile { dll: "d3dx11_43", size: 248_672, md5: "8e0bb968ff41d80e5f2c747c04db79ae" },
        NativeFile { dll: "d3dcompiler_43", size: 2_106_216, md5: "1c9b45e87528b8bb8cfa884ea0099a85" },
    ],
};

/// The three native libraries that the Lutris installer script of The Secret World asks Winetricks for (`d3dcompiler_43 d3dx9_43 d3dx11_43`) and that
/// Lutris's own `d3d_extras` runtime links into its working prefix - byte-identical (same size and MD5) to the files of V7 and to those inside
/// Microsoft's DirectX June 2010 redistributable, from which [`crate::dxredist`] obtains them. Differs from V7 only by NOT overriding
/// `d3dcompiler_47` (the Lutris reference does not override it either; V7 itself documents it as optional). UNCONFIRMED as a replacement of V7
/// until a visual test on a fresh prefix shows correct character clothing (documentation/third-party-components.md).
pub const PROFILE_D3D_NATIVE_MS2010: NativeProfile = NativeProfile {
    id: "d3d-native-ms2010",
    files: &[
        NativeFile { dll: "d3dx9_43", size: 1_998_168, md5: "86e39e9161c3d930d93822f1563c280d" },
        NativeFile { dll: "d3dx11_43", size: 248_672, md5: "8e0bb968ff41d80e5f2c747c04db79ae" },
        NativeFile { dll: "d3dcompiler_43", size: 2_106_216, md5: "1c9b45e87528b8bb8cfa884ea0099a85" },
    ],
};

pub fn native_profile(id: &str) -> Option<&'static NativeProfile> {
    [&PROFILE_D3D_NATIVE_V7, &PROFILE_D3D_NATIVE_MS2010].into_iter().find(|p| p.id == id)
}

const NATIVE_BACKUP: &str = "tsw-launch-native-backup";
const NATIVE_KEY: &str = "native_profile";

fn native_group(p: &NativeProfile) -> String {
    format!("{}=n", p.files.iter().map(|f| f.dll).collect::<Vec<_>>().join(","))
}

fn syswow_rel(f: &NativeFile) -> String {
    format!("drive_c/windows/syswow64/{}.dll", f.dll)
}

/// The id of the native profile recorded in the prefix manifest, if one is installed (`None`: none, or no manifest).
pub fn installed_native(prefix: &Path) -> Option<String> {
    let text = std::fs::read_to_string(prefix.join(MANIFEST)).ok()?;
    manifest_value(&text, NATIVE_KEY).map(str::to_string)
}

/// Pure: the manifest with the profile enabled (override group, verified component lines, profile id).
pub fn manifest_with_native(text: &str, p: &NativeProfile) -> String {
    let mut out = Vec::new();
    for l in text.lines() {
        if l.starts_with("dll_overrides=") {
            out.push(format!("{l};{}", native_group(p)));
        } else {
            out.push(l.to_string());
        }
    }
    for f in p.files {
        out.push(format!("component={} md5={}", syswow_rel(f), f.md5));
    }
    out.push(format!("{NATIVE_KEY}={}", p.id));
    out.join("\n") + "\n"
}

/// Pure inverse of [`manifest_with_native`]: exactly what it added is removed, nothing else.
pub fn manifest_without_native(text: &str, p: &NativeProfile) -> String {
    let suffix = format!(";{}", native_group(p));
    let comps: Vec<String> = p.files.iter().map(|f| format!("component={} md5={}", syswow_rel(f), f.md5)).collect();
    let key = format!("{NATIVE_KEY}={}", p.id);
    let mut out = Vec::new();
    for l in text.lines() {
        if l == key || comps.iter().any(|c| c == l) {
            continue;
        }
        out.push(match l.strip_suffix(suffix.as_str()) {
            Some(rest) if l.starts_with("dll_overrides=") => rest.to_string(),
            _ => l.to_string(),
        });
    }
    out.join("\n") + "\n"
}

/// Read every file of the profile from `dir` and check size and MD5 BEFORE anything is changed. Errors name the file only.
fn verify_native_sources(p: &NativeProfile, dir: &Path) -> Result<Vec<Vec<u8>>, String> {
    let mut all = Vec::new();
    for f in p.files {
        let path = dir.join(format!("{}.dll", f.dll));
        let bytes = std::fs::read(&path).map_err(|e| format!("{}.dll is not readable in {}: {e}", f.dll, dir.display()))?;
        if bytes.len() as u64 != f.size || md5::md5_hex(&bytes) != f.md5 {
            return Err(format!("{}.dll in {} is not the file profile {} expects (size or MD5 differs); nothing was changed", f.dll, dir.display(), p.id));
        }
        all.push(bytes);
    }
    Ok(all)
}

/// Install a profile into an initialised prefix (one with the `prefix init` manifest). Replaced files are backed up first, so
/// [`remove_native`] restores the previous state. Idempotence is refused, not guessed: a prefix with a profile must be cleaned first.
pub fn install_native(p: &NativeProfile, dir: &Path, prefix: &Path) -> Result<(), String> {
    let man = prefix.join(MANIFEST);
    let text = std::fs::read_to_string(&man).map_err(|_| format!("{} has no {MANIFEST}: run `tsw-launch prefix init` first", prefix.display()))?;
    if manifest_value(&text, "dxvk") != Some("installed") {
        return Err("the prefix manifest does not enable DXVK; run `tsw-launch prefix init` first".into());
    }
    if let Some(have) = manifest_value(&text, NATIVE_KEY) {
        return Err(format!("profile {have} is already installed here; run `tsw-launch prefix native-remove` first"));
    }
    may_initialise(prefix)?;
    let sources = verify_native_sources(p, dir)?;
    let (win, backup) = (prefix.join("drive_c/windows/syswow64"), prefix.join(NATIVE_BACKUP));
    std::fs::create_dir_all(&backup).map_err(|e| format!("cannot create {}: {e}", backup.display()))?;
    for (f, bytes) in p.files.iter().zip(&sources) {
        let dest = win.join(format!("{}.dll", f.dll));
        if dest.symlink_metadata().is_ok() {
            std::fs::copy(&dest, backup.join(format!("{}.dll", f.dll))).map_err(|e| format!("cannot back up {}: {e}", dest.display()))?;
            std::fs::remove_file(&dest).map_err(|e| format!("cannot replace {}: {e}", dest.display()))?;
        }
        std::fs::write(&dest, bytes).map_err(|e| format!("cannot write {}: {e}", dest.display()))?;
    }
    std::fs::write(&man, manifest_with_native(&text, p)).map_err(|e| format!("cannot write {}: {e}", man.display()))
}

/// Undo [`install_native`]: restore the backed-up files (or remove files that did not exist before) and the manifest.
pub fn remove_native(prefix: &Path) -> Result<String, String> {
    let man = prefix.join(MANIFEST);
    let text = std::fs::read_to_string(&man).map_err(|_| format!("{} has no {MANIFEST}", prefix.display()))?;
    may_initialise(prefix)?;
    let id = manifest_value(&text, NATIVE_KEY).ok_or("no native profile is installed in this prefix")?.to_string();
    let p = native_profile(&id).ok_or_else(|| format!("unknown profile {id:?} in the manifest"))?;
    remove_native_with(p, prefix)?;
    Ok(id)
}

/// [`remove_native`] for a known profile (the registry lookup is the only difference; tests use fixture profiles).
fn remove_native_with(p: &NativeProfile, prefix: &Path) -> Result<(), String> {
    let man = prefix.join(MANIFEST);
    let text = std::fs::read_to_string(&man).map_err(|_| format!("{} has no {MANIFEST}", prefix.display()))?;
    let (win, backup) = (prefix.join("drive_c/windows/syswow64"), prefix.join(NATIVE_BACKUP));
    for f in p.files {
        let dest = win.join(format!("{}.dll", f.dll));
        let saved = backup.join(format!("{}.dll", f.dll));
        let _ = std::fs::remove_file(&dest);
        if saved.is_file() {
            std::fs::copy(&saved, &dest).map_err(|e| format!("cannot restore {}: {e}", dest.display()))?;
            let _ = std::fs::remove_file(&saved);
        }
    }
    let _ = std::fs::remove_dir(&backup);
    std::fs::write(&man, manifest_without_native(&text, p)).map_err(|e| format!("cannot write {}: {e}", man.display()))
}

/// Parse `[Control Panel\Desktop\WindowMetrics]` from `user.reg` text. Returns (name, value) pairs of the height keys.
pub fn read_metrics(user_reg: &str) -> Vec<(String, i64)> {
    let mut in_section = false;
    let mut out = Vec::new();
    for line in user_reg.lines() {
        if line.starts_with('[') {
            in_section = line.starts_with("[Control Panel\\\\Desktop\\\\WindowMetrics]");
            continue;
        }
        if !in_section {
            continue;
        }
        if let Some((k, v)) = line.split_once('=') {
            let k = k.trim().trim_matches('"');
            if matches!(k, "CaptionHeight" | "SmCaptionHeight" | "MenuHeight") {
                if let Ok(n) = v.trim().trim_matches('"').parse::<i64>() {
                    out.push((k.to_string(), n));
                }
            }
        }
    }
    out
}

/// Findings about the window metrics of a prefix (`user.reg` text).
pub fn check_metrics(user_reg: &str) -> Vec<Finding> {
    let metrics = read_metrics(user_reg);
    if metrics.is_empty() {
        return vec![
            Finding::bad(Level::Warn, "prefix has no WindowMetrics yet".into(), "run `tsw-launch prefix init` or start Wine once in this prefix").coded("prefix.metrics_none", Vec::<String>::new())
        ];
    }
    metrics
        .into_iter()
        .map(|(k, v)| {
            if METRIC_RANGE.contains(&v) {
                Finding::ok(format!("window metric {k}={v}")).coded("prefix.metric_ok", [k.clone(), v.to_string()])
            } else {
                Finding::bad(
                    Level::Fail,
                    format!("window metric {k}={v} is implausible (expected {}..={})", METRIC_RANGE.start(), METRIC_RANGE.end()),
                    "the prefix was created without the 32-bit FreeType/Fontconfig libraries; install them and create a NEW prefix (`tsw-launch prefix init`)",
                )
                .coded("prefix.metric_bad", [k.clone(), v.to_string(), METRIC_RANGE.start().to_string(), METRIC_RANGE.end().to_string()])
            }
        })
        .collect()
}

/// Is the compiler profile of the tested-good configuration installed? (Information about the prefix; nothing is changed.)
pub fn profile_finding(prefix: &Path) -> Option<Finding> {
    let text = std::fs::read_to_string(prefix.join(MANIFEST)).ok()?;
    if manifest_value(&text, "dxvk") != Some("installed") {
        return None;
    }
    Some(match manifest_value(&text, NATIVE_KEY) {
        Some(id) => Finding::ok(format!("compiler profile {id} installed")).coded("prefix.profile_ok", [id.to_string()]),
        None => Finding::bad(
            Level::Warn,
            "no native shader-compiler profile is installed in this prefix".into(),
            "the tested configuration uses Microsoft's d3dcompiler_43 and the D3DX libraries; with Wine's built-in compiler character clothing was black in our tests. Install with `tsw-launch prefix native-install --native-dir <folder with the 32-bit DLLs>` (advanced view)",
        )
        .coded("prefix.no_profile", Vec::<String>::new()),
    })
}

/// Check an existing prefix: metrics, components, manifest. Nothing is changed.
pub fn check(prefix: &Path, runtime_files: Option<&Path>) -> Vec<Finding> {
    let mut f = Vec::new();
    match std::fs::read_to_string(prefix.join("user.reg")) {
        Ok(t) => f.extend(check_metrics(&t)),
        Err(_) => {
            f.push(
                Finding::bad(Level::Fail, format!("{} is not an initialised Wine prefix (no user.reg)", prefix.display()), "run `tsw-launch prefix init --prefix <dir> --runner <wine>`")
                    .coded("prefix.not_initialised", [prefix.display().to_string()]),
            );
            return f;
        }
    }
    if let Some(rt) = runtime_files {
        for op in plan_components(rt, prefix) {
            match (std::fs::read(&op.from), std::fs::read(&op.to)) {
                (Ok(a), Ok(b)) if a == b => {}
                (Ok(_), Ok(_)) => f.push(
                    Finding::bad(Level::Warn, format!("{} differs from the runtime's copy", op.to.display()), "re-run `tsw-launch prefix init` to refresh the components")
                        .coded("prefix.runtime_differs", [op.to.display().to_string()]),
                ),
                (Ok(_), Err(_)) => {
                    f.push(Finding::bad(Level::Fail, format!("{} is missing", op.to.display()), "run `tsw-launch prefix init`").coded("prefix.file_missing", [op.to.display().to_string()]))
                }
                (Err(e), _) => f.push(
                    Finding::bad(Level::Fail, format!("runtime component {} unreadable: {e}", op.from.display()), "use a GE-Proton/Proton runtime that ships DXVK and vkd3d")
                        .coded("prefix.runtime_unreadable", [op.from.display().to_string(), e.to_string()]),
                ),
            }
        }
    }
    f.extend(check_manifest(prefix));
    f.extend(profile_finding(prefix));
    // A manifest that is missing or does not enable DXVK keeps the old warning; an invalid override is already a Fail above.
    if dll_overrides(prefix).is_none() && !f.iter().any(|x| x.code == "prefix.override_rejected") {
        f.push(Finding::bad(Level::Warn, "no component manifest: DXVK is not enabled for this prefix".into(), "run `tsw-launch prefix init`").coded("prefix.no_manifest", Vec::<String>::new()));
    }
    f
}

/// Manifest text: runtime version and the MD5 of every installed component (documents what the prefix contains).
pub fn manifest_text(runtime_version: &str, runtime_files: &Path, ops: &[CopyOp]) -> String {
    let mut s = format!("# written by tsw-launch prefix init; do not edit\nruntime={runtime_version}\nruntime_dir={}\narch=win64+wow64 (32-bit client)\ndxvk=installed\nvkd3d=installed\ndll_overrides=d3d11,dxgi=n,b\n", runtime_files.display());
    for op in ops {
        let digest = std::fs::read(&op.to).map(|b| md5::md5_hex(&b)).unwrap_or_else(|_| "unreadable".into());
        let rel = op.to.strip_prefix(op.to.ancestors().nth(4).unwrap_or(&op.to)).unwrap_or(&op.to);
        s.push_str(&format!("component={} md5={digest}\n", rel.display()));
    }
    s
}

fn wine_command(wine: &Path, runtime_files: &Path, prefix: &Path, args: &[&str]) -> Command {
    let lib = |d: &str| runtime_files.join(d);
    let mut ld = std::ffi::OsString::new();
    for d in [lib("lib"), lib("lib64")] {
        ld.push(d);
        ld.push(":");
    }
    if let Some(old) = std::env::var_os("LD_LIBRARY_PATH") {
        ld.push(old);
    }
    let mut c = Command::new(wine);
    c.args(args)
        .env("WINEPREFIX", prefix)
        .env("WINEDEBUG", "-all")
        .env("LD_LIBRARY_PATH", ld)
        // no Mono/Gecko download prompts and no desktop menu entries while creating the prefix
        .env("WINEDLLOVERRIDES", "mscoree=d;mshtml=d;winemenubuilder.exe=d");
    c
}

/// Create (if needed) and populate the prefix, without progress output.
pub fn init(prefix: &Path, wine: &Path) -> Result<PathBuf, String> {
    init_with(prefix, wine, &Silent)
}

/// Like [`init`], reporting real progress to `sink`: unmeasurable steps are reported as such, the component copy is
/// measured in bytes (with rate and ETA from real samples). Ends with exactly one `Update::Done`.
pub fn init_with(prefix: &Path, wine: &Path, sink: &dyn Sink) -> Result<PathBuf, String> {
    let result = run_init(prefix, wine, sink);
    sink.update(Update::Done(match &result {
        Ok(_) => Outcome::Finished("The Wine prefix is ready.".into()),
        Err(e) => Outcome::Failed(e.clone()),
    }));
    result
}

fn run_init(prefix: &Path, wine: &Path, sink: &dyn Sink) -> Result<PathBuf, String> {
    may_initialise(prefix)?;
    let previous_profile = std::fs::read_to_string(prefix.join(MANIFEST)).ok().and_then(|text| manifest_value(&text, NATIVE_KEY).map(str::to_string));
    let previous_profile = previous_profile.as_deref().map(|id| native_profile(id).ok_or_else(|| format!("unknown native profile {id}; nothing was changed"))).transpose()?;
    let step = |phase, text: &str| sink.update(Update::Progress(Progress::indeterminate(phase, text)));
    step(Phase::Checking, "Checking the Wine/Proton runtime");
    let rt = runtime_files_dir(wine).ok_or_else(|| format!("{} is not inside a Wine/Proton runtime tree (expected <runtime>/files/bin/wine)", wine.display()))?;
    claim_prefix(prefix)?;
    let version_out = Command::new(wine).arg("--version").env("LD_LIBRARY_PATH", rt.join("lib")).output().map_err(|e| format!("cannot run {}: {e}", wine.display()))?;
    let version = String::from_utf8_lossy(&version_out.stdout).trim().to_string();
    if !prefix.join("user.reg").is_file() {
        step(Phase::Installing, "Creating the Wine prefix");
        let status = wine_command(wine, &rt, prefix, &["wineboot", "-u"]).status().map_err(|e| format!("cannot run wineboot: {e}"))?;
        if !status.success() {
            return Err(format!("`wine wineboot -u` failed ({status}) for {}", prefix.display()));
        }
        // Wine writes user.reg/system.reg when the wineserver exits; wait for that instead of racing it.
        let server = wine.with_file_name("wineserver");
        let status = wine_command(&server, &rt, prefix, &["-w"]).status().map_err(|e| format!("cannot run {}: {e}", server.display()))?;
        if !status.success() {
            return Err(format!("`wineserver -w` failed ({status}) for {}", prefix.display()));
        }
    }
    step(Phase::Verifying, "Checking the window metrics");
    let metrics = check_metrics(&std::fs::read_to_string(prefix.join("user.reg")).map_err(|e| format!("no user.reg after wineboot: {e}"))?);
    if let Some(bad) = metrics.iter().find(|f| f.level == Level::Fail) {
        return Err(format!("{} - {}", bad.what, bad.hint));
    }
    let ops = plan_components(&rt, prefix);
    let total: u64 = ops.iter().filter_map(|op| std::fs::metadata(&op.from).ok()).map(|m| m.len()).sum();
    let (mut done, mut meter, start) = (0u64, RateMeter::default(), std::time::Instant::now());
    for op in &ops {
        let bytes = std::fs::read(&op.from).map_err(|e| format!("runtime component {} unreadable: {e} (does this runtime ship DXVK and vkd3d?)", op.from.display()))?;
        if let Some(dir) = op.to.parent() {
            std::fs::create_dir_all(dir).map_err(|e| format!("cannot create {}: {e}", dir.display()))?;
        }
        let _ = std::fs::remove_file(&op.to);
        std::fs::write(&op.to, &bytes).map_err(|e| format!("cannot write {}: {e}", op.to.display()))?;
        done += bytes.len() as u64;
        let (rate, eta_secs) = meter.sample(done, Some(total), start.elapsed());
        sink.update(Update::Progress(Progress { phase: Phase::Installing, step: "Installing the DXVK and vkd3d components".into(), done: Some(done), total: Some(total), rate, eta_secs }));
    }
    let path = prefix.join(MANIFEST);
    let text = manifest_text(&version, &rt, &ops);
    let text = previous_profile.map_or_else(|| text.clone(), |p| manifest_with_native(&text, p));
    std::fs::write(&path, text).map_err(|e| format!("cannot write {}: {e}", path.display()))?;
    std::fs::write(prefix.join(OWNER), COMPLETE_OWNER).map_err(|e| format!("cannot finish prefix provenance: {e}"))?;
    Ok(path)
}

// the Wine prefix manager is not used on Windows (no prefix there); its tests run on Linux
#[cfg(all(test, unix))]
mod tests {
    use super::*;

    fn snapshot(root: &Path) -> Vec<(std::path::PathBuf, String)> {
        fn walk(root: &Path, dir: &Path, files: &mut Vec<(std::path::PathBuf, String)>) {
            for entry in std::fs::read_dir(dir).unwrap() {
                let path = entry.unwrap().path();
                if path.is_dir() {
                    walk(root, &path, files);
                } else {
                    files.push((path.strip_prefix(root).unwrap().to_path_buf(), crate::sha256::file_sha256(&path).unwrap().0));
                }
            }
        }
        let mut files = Vec::new();
        walk(root, root, &mut files);
        files.sort();
        files
    }

    #[test]
    fn ownership_rejects_foreign_and_damaged_prefixes_without_any_write_or_execution() {
        let root = std::env::temp_dir().join(format!("tsw-ownership-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        for (case, manifest, owner) in [
            ("foreign-user", None, None),
            ("foreign-dlls", None, None),
            ("damaged", Some("dxvk=installed\n"), None),
            ("lost-manifest", None, Some(COMPLETE_OWNER)),
            ("damaged-owned", Some("broken"), Some(COMPLETE_OWNER)),
            ("bad-marker", None, Some("broken")),
        ] {
            let p = root.join(case);
            std::fs::create_dir_all(p.join("drive_c/windows/system32")).unwrap();
            std::fs::write(p.join("user.reg"), GOOD).unwrap();
            std::fs::write(p.join("drive_c/windows/system32/d3d11.dll"), b"foreign dxvk").unwrap();
            std::fs::write(p.join("drive_c/windows/system32/libvkd3d-1.dll"), b"foreign vkd3d").unwrap();
            if let Some(m) = manifest {
                std::fs::write(p.join(MANIFEST), m).unwrap();
            }
            if let Some(m) = owner {
                std::fs::write(p.join(OWNER), m).unwrap();
            }
            let before = snapshot(&p);
            assert!(may_initialise(&p).is_err(), "{case}");
            assert!(init(&p, Path::new("/nonexistent/must-not-execute")).unwrap_err().contains("nothing was changed"), "{case}");
            assert_eq!(snapshot(&p), before, "{case}: every file and SHA256 unchanged");
            let layout = crate::install::Layout { base: root.join(format!("layout-{case}")) };
            std::fs::create_dir_all(&layout.base).unwrap();
            std::fs::rename(&p, layout.prefix()).unwrap();
            assert!(crate::install::prepare(&layout).is_err());
            assert!(crate::install::create_prefix(&layout, Path::new("/nonexistent"), &Silent).is_err());
            assert_eq!(snapshot(&layout.prefix()), before);
        }
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn ownership_accepts_new_and_verified_legacy_and_resumes_only_our_interrupted_creation() {
        let p = std::env::temp_dir().join(format!("tsw-owned-resume-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&p);
        assert!(may_initialise(&p).is_ok());
        std::fs::create_dir_all(&p).unwrap();
        assert!(may_initialise(&p).is_ok());
        claim_prefix(&p).unwrap();
        std::fs::write(p.join("user.reg"), GOOD).unwrap();
        assert!(may_initialise(&p).is_ok(), "our interrupted wineboot may resume");
        let ops = plan_components(Path::new("/fixture/runtime"), &p);
        for op in &ops {
            std::fs::create_dir_all(op.to.parent().unwrap()).unwrap();
            std::fs::write(&op.to, b"fixture").unwrap();
        }
        std::fs::write(p.join(MANIFEST), manifest_text("fixture", Path::new("/fixture/runtime"), &ops)).unwrap();
        std::fs::remove_file(p.join(OWNER)).unwrap();
        assert!(may_initialise(&p).is_ok(), "complete historical manifest migrates");
        claim_prefix(&p).unwrap();
        let before = snapshot(&p);
        claim_prefix(&p).unwrap();
        assert_eq!(snapshot(&p), before, "repeated ownership claim is idempotent");
        std::fs::remove_file(p.join(MANIFEST)).unwrap();
        assert!(may_initialise(&p).is_err(), "a completed owner cannot hide a lost manifest");
        std::fs::remove_dir_all(p).unwrap();
    }

    const GOOD: &str = "[Control Panel\\\\Desktop\\\\WindowMetrics] 1\n\"CaptionHeight\"=\"25\"\n\"MenuHeight\"=\"18\"\n\"SmCaptionHeight\"=\"17\"\n\n[Other]\n\"CaptionHeight\"=\"99999\"\n";
    const BAD: &str = "[Control Panel\\\\Desktop\\\\WindowMetrics] 1\n\"CaptionHeight\"=\"5434094\"\n\"MenuHeight\"=\"10867998\"\n\"SmCaptionHeight\"=\"5434094\"\n";

    #[test]
    fn metrics_come_only_from_the_window_metrics_section() {
        assert_eq!(read_metrics(GOOD), vec![("CaptionHeight".into(), 25), ("MenuHeight".into(), 18), ("SmCaptionHeight".into(), 17)]);
    }

    #[test]
    fn healthy_metrics_pass_and_the_observed_corruption_is_a_failure_with_a_cause() {
        assert!(check_metrics(GOOD).iter().all(|f| f.level == Level::Ok));
        let bad = check_metrics(BAD);
        assert_eq!(bad.iter().filter(|f| f.level == Level::Fail).count(), 3);
        assert!(bad[0].hint.contains("FreeType"));
        assert_eq!(check_metrics("")[0].level, Level::Warn);
    }

    #[test]
    fn plan_covers_both_architectures_and_only_what_is_used() {
        let ops = plan_components(Path::new("/rt/files"), Path::new("/p"));
        assert_eq!(ops.len(), 10);
        assert!(ops.iter().any(|o| o.from == Path::new("/rt/files/lib/wine/dxvk/x86_64-windows/d3d11.dll") && o.to == Path::new("/p/drive_c/windows/system32/d3d11.dll")));
        assert!(ops.iter().any(|o| o.from == Path::new("/rt/files/lib/vkd3d/i386-windows/libvkd3d-1.dll") && o.to == Path::new("/p/drive_c/windows/syswow64/libvkd3d-1.dll")));
        assert!(!ops.iter().any(|o| o.to.to_string_lossy().contains("d3d9") || o.to.to_string_lossy().contains("d3d10core")));
    }

    #[test]
    fn runtime_dir_is_derived_from_the_wine_path() {
        let d = std::env::temp_dir().join(format!("tsw-prefix-rt-{}", std::process::id()));
        std::fs::create_dir_all(d.join("files/lib/wine")).unwrap();
        std::fs::create_dir_all(d.join("files/bin")).unwrap();
        assert_eq!(runtime_files_dir(&d.join("files/bin/wine")), Some(d.join("files")));
        assert_eq!(runtime_files_dir(Path::new("/usr/bin/wine")), None);
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn overrides_are_merged_not_replaced_and_need_the_manifest() {
        assert_eq!(merge_overrides(Some("a=d"), "d3d11,dxgi=n,b"), "a=d;d3d11,dxgi=n,b");
        assert_eq!(merge_overrides(None, "x"), "x");
        assert_eq!(merge_overrides(Some(" "), "x"), "x");
        let d = std::env::temp_dir().join(format!("tsw-prefix-man-{}", std::process::id()));
        std::fs::create_dir_all(&d).unwrap();
        assert_eq!(dll_overrides(&d), None);
        std::fs::write(d.join(MANIFEST), "dxvk=installed\n").unwrap();
        assert_eq!(dll_overrides(&d).as_deref(), Some("d3d11,dxgi=n,b"));
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn check_reports_missing_components_and_not_a_prefix() {
        let d = std::env::temp_dir().join(format!("tsw-prefix-chk-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        assert!(check(&d, None).iter().any(|f| f.level == Level::Fail && f.what.contains("not an initialised")));
        std::fs::write(d.join("user.reg"), GOOD).unwrap();
        let rt = d.join("rt");
        let f = check(&d, Some(&rt));
        assert!(f.iter().any(|f| f.level == Level::Fail && f.what.contains("unreadable")));
        assert!(f.iter().any(|f| f.what.contains("no component manifest")));
        let _ = std::fs::remove_dir_all(&d);
    }

    fn manifest_prefix(tag: &str, manifest: &str, files: &[(&str, &[u8])]) -> PathBuf {
        let d = std::env::temp_dir().join(format!("tsw-prefix-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(d.join("drive_c/windows/syswow64")).unwrap();
        for (rel, bytes) in files {
            std::fs::write(d.join(rel), bytes).unwrap();
        }
        std::fs::write(d.join(MANIFEST), manifest).unwrap();
        d
    }

    #[test]
    fn only_documented_graphics_dlls_and_load_orders_are_accepted_as_overrides() {
        assert_eq!(validate_overrides("d3d11,dxgi=n,b").unwrap(), vec!["d3d11", "dxgi"]);
        assert_eq!(validate_overrides("d3dcompiler_47=n;d3d9=b,n").unwrap(), vec!["d3dcompiler_47"]);
        assert!(validate_overrides("kernel32=n").is_err(), "unknown DLL");
        assert!(validate_overrides("d3d11=d").is_err(), "disabling is not a graphics profile");
        assert!(validate_overrides("d3d11").is_err(), "no mode");
        assert!(validate_overrides("d3d11=n,x").is_err(), "unknown mode");
        assert!(validate_overrides("d3d11=n;winemenubuilder.exe=d").is_err());
    }

    #[test]
    fn the_manifest_value_drives_the_override_and_an_invalid_value_is_never_passed_on() {
        let ok = manifest_prefix("ov-ok", "dxvk=installed\ndll_overrides=d3d11,dxgi=n,b;d3dcompiler_47=n\n", &[]);
        assert_eq!(dll_overrides(&ok).as_deref(), Some("d3d11,dxgi=n,b;d3dcompiler_47=n"));
        let bad = manifest_prefix("ov-bad", "dxvk=installed\ndll_overrides=kernel32=n\n", &[]);
        assert_eq!(dll_overrides(&bad), None);
        let _ = std::fs::remove_dir_all(&ok);
        let _ = std::fs::remove_dir_all(&bad);
    }

    #[test]
    fn a_native_override_needs_a_recorded_component_that_is_still_unchanged() {
        let md5 = md5::md5_hex(b"native");
        let man = format!("dxvk=installed\ndll_overrides=d3dcompiler_47=n\ncomponent=drive_c/windows/syswow64/d3dcompiler_47.dll md5={md5}\n");
        let good = manifest_prefix("nat-ok", &man, &[("drive_c/windows/syswow64/d3dcompiler_47.dll", b"native")]);
        assert!(check_manifest(&good).iter().all(|f| f.level == Level::Ok));
        // the file was replaced after `prefix init` (for example by Wine): drift is reported, not hidden
        std::fs::write(good.join("drive_c/windows/syswow64/d3dcompiler_47.dll"), b"builtin stub").unwrap();
        assert!(check_manifest(&good).iter().any(|f| f.level == Level::Warn && f.what.contains("changed since")));
        // override without any recorded component
        let none = manifest_prefix("nat-none", "dxvk=installed\ndll_overrides=d3dcompiler_47=n\n", &[]);
        assert!(check_manifest(&none).iter().any(|f| f.level == Level::Fail && f.what.contains("no verified component")));
        // recorded component missing on disk
        let gone = manifest_prefix("nat-gone", &man, &[]);
        assert!(check_manifest(&gone).iter().any(|f| f.level == Level::Fail && f.what.contains("is missing")));
        for d in [good, none, gone] {
            let _ = std::fs::remove_dir_all(&d);
        }
    }

    #[test]
    fn the_prefix_made_by_init_still_validates() {
        // the manifest text `manifest_text` writes must pass the new checks (no regression for existing managed prefixes)
        let text = "# x\nruntime=r\ndxvk=installed\nvkd3d=installed\ndll_overrides=d3d11,dxgi=n,b\ncomponent=drive_c/windows/system32/d3d11.dll md5=00\n";
        assert_eq!(manifest_value(text, "dll_overrides"), Some("d3d11,dxgi=n,b"));
        assert_eq!(manifest_components(text), vec![("drive_c/windows/system32/d3d11.dll", "00")]);
        assert!(validate_overrides("d3d11,dxgi=n,b").is_ok());
    }

    #[test]
    fn start_checks_refuse_a_rejected_override_and_still_warn_about_a_manifest_without_dxvk() {
        // `run_checks` (CLI) and `model::prepare` (GUI) both call `check`; a Fail there blocks the start / the Play button.
        let bad = manifest_prefix("chk-bad", "dxvk=installed\ndll_overrides=kernel32=n\n", &[]);
        std::fs::write(bad.join("user.reg"), GOOD).unwrap();
        let f = check(&bad, None);
        assert!(f.iter().any(|x| x.level == Level::Fail && x.what.contains("manifest override rejected")), "{f:?}");
        assert_eq!(dll_overrides(&bad), None, "and the rejected value is never handed to Wine");
        let nodxvk = manifest_prefix("chk-nodxvk", "runtime=x\n", &[]);
        std::fs::write(nodxvk.join("user.reg"), GOOD).unwrap();
        assert!(check(&nodxvk, None).iter().any(|x| x.level == Level::Warn && x.what.contains("DXVK is not enabled")));
        let none = manifest_prefix("chk-none", "x\n", &[]);
        std::fs::remove_file(none.join(MANIFEST)).unwrap();
        std::fs::write(none.join("user.reg"), GOOD).unwrap();
        assert!(check(&none, None).iter().any(|x| x.level == Level::Warn && x.what.contains("no component manifest")));
        for d in [bad, nodxvk, none] {
            let _ = std::fs::remove_dir_all(&d);
        }
    }

    // --- native profile (fixture bytes only; the real DLLs are never needed by the tests) ---
    fn fixture_profile(blobs: &[(&'static str, &[u8])]) -> &'static NativeProfile {
        let files: Vec<NativeFile> = blobs.iter().map(|(dll, b)| NativeFile { dll, size: b.len() as u64, md5: Box::leak(md5::md5_hex(b).into_boxed_str()) }).collect();
        Box::leak(Box::new(NativeProfile { id: "fixture", files: Box::leak(files.into_boxed_slice()) }))
    }

    fn native_source(tag: &str, blobs: &[(&str, &[u8])]) -> PathBuf {
        let d = std::env::temp_dir().join(format!("tsw-native-src-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        for (dll, b) in blobs {
            std::fs::write(d.join(format!("{dll}.dll")), b).unwrap();
        }
        d
    }

    const BASE_MANIFEST: &str = "dxvk=installed\ndll_overrides=d3d11,dxgi=n,b\ncomponent=drive_c/windows/system32/d3d11.dll md5=00\n";

    /// A prefix whose manifest is consistent (DXVK components present with their MD5), so `check_manifest` can be asserted clean.
    fn consistent_prefix(tag: &str, extra: &[(&str, &[u8])]) -> (PathBuf, String) {
        let (d11, dx) = (md5::md5_hex(b"dxvk-d3d11"), md5::md5_hex(b"dxvk-dxgi"));
        let man = format!("dxvk=installed\ndll_overrides=d3d11,dxgi=n,b\ncomponent=drive_c/windows/system32/d3d11.dll md5={d11}\ncomponent=drive_c/windows/system32/dxgi.dll md5={dx}\n");
        let pre = manifest_prefix(tag, &man, extra);
        std::fs::create_dir_all(pre.join("drive_c/windows/system32")).unwrap();
        std::fs::write(pre.join("drive_c/windows/system32/d3d11.dll"), b"dxvk-d3d11").unwrap();
        std::fs::write(pre.join("drive_c/windows/system32/dxgi.dll"), b"dxvk-dxgi").unwrap();
        std::fs::write(pre.join("user.reg"), GOOD).unwrap();
        let ops = plan_components(Path::new("/fixture/runtime"), &pre);
        for op in &ops {
            if !op.to.exists() {
                std::fs::create_dir_all(op.to.parent().unwrap()).unwrap();
                std::fs::write(&op.to, b"fixture").unwrap();
            }
        }
        let man = manifest_text("fixture", Path::new("/fixture/runtime"), &ops);
        std::fs::write(pre.join(MANIFEST), &man).unwrap();
        (pre, man)
    }

    #[test]
    fn the_v7_profile_is_the_v6_set_plus_exactly_d3dcompiler_43_and_is_a_valid_manifest() {
        let names: Vec<_> = PROFILE_D3D_NATIVE_V7.files.iter().map(|f| f.dll).collect();
        assert_eq!(names, ["d3dcompiler_47", "d3dx9_43", "d3dx11_43", "d3dcompiler_43"]);
        let m = manifest_with_native(BASE_MANIFEST, &PROFILE_D3D_NATIVE_V7);
        assert_eq!(manifest_value(&m, "dll_overrides"), Some("d3d11,dxgi=n,b;d3dcompiler_47,d3dx9_43,d3dx11_43,d3dcompiler_43=n"));
        assert!(validate_overrides(manifest_value(&m, "dll_overrides").unwrap()).is_ok(), "the existing validation accepts the profile");
        assert!(native_profile("d3d-native-v7").is_some() && native_profile("nope").is_none());
    }

    #[test]
    fn manifest_edit_roundtrips_exactly() {
        let p = fixture_profile(&[("d3dcompiler_43", b"aa"), ("d3dx9_43", b"bb")]);
        let with = manifest_with_native(BASE_MANIFEST, p);
        assert!(with.contains("native_profile=fixture") && with.contains("d3dcompiler_43,d3dx9_43=n"));
        assert_eq!(manifest_without_native(&with, p), BASE_MANIFEST);
    }

    #[test]
    fn install_verifies_backs_up_and_remove_restores_the_prefix() {
        let p = fixture_profile(&[("d3dcompiler_43", b"native-43"), ("d3dx9_43", b"native-x9")]);
        let src = native_source("ok", &[("d3dcompiler_43", b"native-43"), ("d3dx9_43", b"native-x9")]);
        let (pre, base) = consistent_prefix("nat-inst", &[("drive_c/windows/syswow64/d3dcompiler_43.dll", b"wine-builtin-43")]);
        install_native(p, &src, &pre).unwrap();
        let sys = pre.join("drive_c/windows/syswow64");
        assert_eq!(std::fs::read(sys.join("d3dcompiler_43.dll")).unwrap(), b"native-43");
        assert_eq!(std::fs::read(sys.join("d3dx9_43.dll")).unwrap(), b"native-x9");
        assert!(check_manifest(&pre).iter().all(|f| f.level == Level::Ok), "{:?}", check_manifest(&pre));
        assert_eq!(dll_overrides(&pre).as_deref(), Some("d3d11,dxgi=n,b;d3dcompiler_43,d3dx9_43=n"));
        // a second install is refused, not guessed
        assert!(install_native(p, &src, &pre).unwrap_err().contains("already installed"));
        remove_native_with(p, &pre).unwrap();
        assert_eq!(std::fs::read(sys.join("d3dcompiler_43.dll")).unwrap(), b"wine-builtin-43", "the replaced file is back");
        assert!(!sys.join("d3dx9_43.dll").exists(), "a file that did not exist before is gone again");
        assert_eq!(std::fs::read_to_string(pre.join(MANIFEST)).unwrap(), base);
        assert!(check_manifest(&pre).iter().all(|f| f.level == Level::Ok), "the restored prefix is clean again");
        assert!(!pre.join(NATIVE_BACKUP).exists());
        assert!(remove_native(&pre).unwrap_err().contains("no native profile"));
        for d in [src, pre] {
            let _ = std::fs::remove_dir_all(&d);
        }
    }

    #[test]
    fn a_wrong_file_or_a_missing_file_changes_nothing_and_names_only_the_file() {
        let p = fixture_profile(&[("d3dcompiler_43", b"right"), ("d3dx9_43", b"right2")]);
        let (pre, original_manifest) = consistent_prefix("nat-bad", &[("drive_c/windows/syswow64/d3dcompiler_43.dll", b"builtin")]);
        let wrong = native_source("wrong", &[("d3dcompiler_43", b"right"), ("d3dx9_43", b"TAMPERED")]);
        let e = install_native(p, &wrong, &pre).unwrap_err();
        assert!(e.contains("d3dx9_43.dll") && e.contains("nothing was changed") && !e.contains("TAMPERED"), "{e}");
        let missing = native_source("missing", &[("d3dcompiler_43", b"right")]);
        assert!(install_native(p, &missing, &pre).unwrap_err().contains("not readable"));
        assert_eq!(std::fs::read(pre.join("drive_c/windows/syswow64/d3dcompiler_43.dll")).unwrap(), b"builtin");
        assert_eq!(std::fs::read_to_string(pre.join(MANIFEST)).unwrap(), original_manifest);
        assert!(!pre.join(NATIVE_BACKUP).exists(), "no backup folder before verification passed");
        let nomanifest = std::env::temp_dir().join(format!("tsw-native-nom-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&nomanifest);
        assert!(install_native(p, &wrong, &nomanifest).unwrap_err().contains("prefix init"));
        for d in [pre, wrong, missing, nomanifest] {
            let _ = std::fs::remove_dir_all(&d);
        }
    }
}
