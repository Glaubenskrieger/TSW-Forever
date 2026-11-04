//! Obtains the native Direct3D compiler/D3DX libraries of the shader profile [`crate::prefix::PROFILE_D3D_NATIVE_MS2010`] from Microsoft itself,
//! the same way Winetricks does, and without redistributing a single Microsoft file.
//!
//! Source: Microsoft's "DirectX End-User Runtimes (June 2010)" (`directx_Jun2010_redist.exe`, `download.microsoft.com`, https). It is pinned by
//! SHA-256 (the value Winetricks pins as well, and the one measured here on 2026-10-31) and used ONLY as an archive: nothing in it is executed.
//! The three 32-bit libraries are read out of its cabinets with `bsdtar` (libarchive, part of the Freedesktop runtime), and every extracted file is
//! checked again against the size and MD5 of the profile before it is stored below the launcher's data folder and installed into the managed
//! prefix. The user's consent is required for the download (about 100 MB); the archive is deleted afterwards, the three DLLs (4 MB) stay as a
//! cache so a second prefix needs no download.
//!
//! Licence: our artifacts contain no Microsoft payload. The verified package's original SDK/runtime terms are shown before new component
//! provisioning, with separate explicit acceptance bound to the reviewed package/text hashes. Mandatory local rights remain unaffected;
//! interoperability is not a blanket exception. See the Microsoft terms and limits summarized in README.md for the EU/German risk assessment and limits.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use crate::install::{fetch_verified, Downloader, InstallError};
use crate::prefix::NativeProfile;
use crate::progress::{Outcome, Phase, Progress, Update};
use crate::{md5, sha256};

pub const URL: &str = "https://download.microsoft.com/download/8/4/A/84A35BF1-DAFE-4AE8-82AF-AD2AE20B6B14/directx_Jun2010_redist.exe";
pub const SHA256: &str = "053f76dcbb28802e23341b6a787e3b0791c0fa5c8d4d011b1044172dbf89c73b";
pub const SIZE: u64 = 100_275_120;
/// Verified online copies of the original terms, not Microsoft-hosted pages.
pub const SDK_TERMS_URL: &str = "https://github.com/apitrace/dxsdk/blob/d964b66467aaa734edbc24326da8119f5f063dd3/Documentation/License%20Agreements/DirectX%20SDK%20EULA.txt";
pub const RUNTIME_TERMS_URL: &str = "https://store.steampowered.com/eula/17300_eula_1#:~:text=MICROSOFT%20DIRECTX%20END%20USER%20RUNTIME";
const ARCHIVE: &str = "directx_Jun2010_redist.exe";

/// `(cabinet inside the redistributable, member name inside the cabinet, profile dll name)`
const MEMBERS: &[(&str, &str, &str)] = &[
    ("Jun2010_D3DCompiler_43_x86.cab", "D3DCompiler_43.dll", "d3dcompiler_43"),
    ("Jun2010_d3dx9_43_x86.cab", "d3dx9_43.dll", "d3dx9_43"),
    ("Jun2010_d3dx11_43_x86.cab", "d3dx11_43.dll", "d3dx11_43"),
];

/// The legal text is obtained from the exact hash-verified package, never shipped in our artifacts.
const SDK_HEADER: &[u8] = b"MICROSOFT SOFTWARE LICENSE TERMS\r\nMICROSOFT DIRECTX SOFTWARE DEVELOPMENT KIT (SDK)";
const RUNTIME_HEADER: &[u8] = b"MICROSOFT SOFTWARE LICENSE TERMS\r\nMICROSOFT DIRECTX END USER RUNTIME\r\n";
const SDK_TEXT_SHA256: &str = "500b2c1e82c511e575fa443e3a686c7098097974e91f06a9f443e4c8f1bf10a4";
const RUNTIME_TEXT_SHA256: &str = "49385cb31461a0448f02053b17d73c42bbbcd27bf38d697a907eba4442d5f479";
const ACCEPTANCE: &str = "microsoft-terms-accepted-v1.txt";

fn contract(data: &[u8], header: &[u8], repeated: bool, expected: &str) -> Result<String, InstallError> {
    let start = data.windows(header.len()).position(|w| w == header).ok_or_else(|| InstallError::Failed("Microsoft licence text is missing; no components installed".into()))?;
    let rest = &data[start..];
    let end = if repeated { rest[header.len()..].windows(header.len()).position(|w| w == header).map(|n| n + header.len()) } else { rest.iter().position(|b| *b == 0) }
        .ok_or_else(|| InstallError::Failed("Microsoft licence text is incomplete".into()))?;
    let bytes = &rest[..end];
    if sha256::sha256_hex(bytes) != expected {
        return Err(InstallError::Failed("Microsoft licence text differs from the reviewed package; no components installed".into()));
    }
    // Exact Windows-1252 characters present in the pinned SDK text. Unknown control bytes fail closed.
    let mut text = String::new();
    for byte in bytes {
        text.push(match byte {
            0x92 => '\u{2019}',
            0x93 => '\u{201c}',
            0x94 => '\u{201d}',
            0x95 => '\u{2022}',
            0x80..=0x9f => return Err(InstallError::Failed("unsupported Microsoft licence encoding".into())),
            b => char::from(*b),
        });
    }
    Ok(text)
}

fn review_terms(archive: &Path, outer: &Path, bsdtar: &Path) -> Result<String, InstallError> {
    use std::io::{Read, Seek, SeekFrom};
    let mut file = std::fs::File::open(archive).map_err(|e| failed("cannot read licence container", e))?;
    let len = file.metadata().map_err(|e| failed("cannot stat licence container", e))?.len();
    file.seek(SeekFrom::Start(len.saturating_sub(65536))).map_err(|e| failed("cannot read licence resource", e))?;
    let mut tail = Vec::new();
    file.read_to_end(&mut tail).map_err(|e| failed("cannot read licence resource", e))?;
    let sdk = contract(&tail, SDK_HEADER, false, SDK_TEXT_SHA256)?;
    // Setup DLL is inspected only as a data container; no Microsoft executable is run.
    let out = Command::new(bsdtar).args(["-xOf"]).arg(outer).arg("dsetup32.dll").stdin(Stdio::null()).output().map_err(|e| failed("cannot read runtime licence resource", e))?;
    if !out.status.success() {
        return Err(InstallError::Failed("cannot extract runtime licence resource".into()));
    }
    let runtime = contract(&out.stdout, RUNTIME_HEADER, true, RUNTIME_TEXT_SHA256)?;
    Ok(format!("Microsoft DirectX June 2010 — original package terms\nSource: {URL}\nPackage SHA256: {SHA256}\n\n--- SDK terms (including separate-program clause) ---\n{sdk}\n\n--- Included end-user runtime terms ---\n{}\n", runtime.trim_end_matches("PAD")))
}

fn acceptance_fingerprint() -> String {
    sha256::sha256_hex(format!("{SHA256}\n{SDK_TEXT_SHA256}\n{RUNTIME_TEXT_SHA256}\n").as_bytes())
}

/// Local acknowledgement of this exact reviewed package and both original licence texts.
pub fn terms_accepted(cache: &Path) -> bool {
    std::fs::read_to_string(cache.join(ACCEPTANCE)).ok().as_deref() == Some(acceptance_fingerprint().as_str())
}

fn approve_terms(cache: &Path, text: &str, approval: &dyn Fn(&str) -> bool) -> Result<(), InstallError> {
    let fingerprint = acceptance_fingerprint();
    let record = cache.join(ACCEPTANCE);
    if std::fs::read_to_string(&record).ok().as_deref() == Some(&fingerprint) {
        return Ok(());
    }
    if !approval(text) {
        return Err(InstallError::Blocked("Microsoft terms were not accepted; no Direct3D components were installed. The verified download is retained for review/retry".into()));
    }
    std::fs::create_dir_all(cache).map_err(|e| failed("cannot create licence record folder", e))?;
    std::fs::write(cache.join("microsoft-terms.txt"), text).map_err(|e| failed("cannot retain licence text", e))?;
    std::fs::write(record, fingerprint).map_err(|e| failed("cannot record licence acceptance", e))
}

fn failed(what: &str, e: impl std::fmt::Display) -> InstallError {
    InstallError::Failed(format!("{what}: {e}"))
}

/// Folder that holds the verified DLLs of `profile`: `<native cache>/<profile id>` (`native cache` = [`crate::install::Layout::native_cache`]).
pub fn cache_dir(native_cache: &Path, profile: &NativeProfile) -> PathBuf {
    native_cache.join(profile.id)
}

/// Are all files of the profile already in `dir` with the right size and MD5?
pub fn cache_complete(profile: &NativeProfile, dir: &Path) -> bool {
    profile.files.iter().all(|f| std::fs::read(dir.join(format!("{}.dll", f.dll))).is_ok_and(|b| b.len() as u64 == f.size && md5::md5_hex(&b) == f.md5))
}

/// `(offset, length)` of the cabinet inside a self-extracting executable: the first `MSCF` signature whose reserved field is zero and whose declared
/// size fits the file. (The Freedesktop runtime's `bsdtar` cannot open the `.exe` itself, but reads the plain cabinet.)
pub fn find_cab(head: &[u8], file_len: u64) -> Option<(usize, u64)> {
    let mut from = 0;
    while let Some(rel) = head.get(from..)?.windows(4).position(|w| w == b"MSCF") {
        let i = from + rel;
        if let (Some(r), Some(sz)) = (head.get(i + 4..i + 8), head.get(i + 8..i + 12)) {
            let (r, sz) = (u32::from_le_bytes(r.try_into().ok()?), u32::from_le_bytes(sz.try_into().ok()?) as u64);
            if r == 0 && sz > 36 && i as u64 + sz <= file_len {
                return Some((i, sz));
            }
        }
        from = i + 1;
    }
    None
}

fn carve_cab(exe: &Path, out: &Path) -> Result<(), InstallError> {
    use std::io::{Read, Seek, SeekFrom};
    let mut f = std::fs::File::open(exe).map_err(|e| failed("cannot open the download", e))?;
    let len = f.metadata().map_err(|e| failed("cannot read the download", e))?.len();
    let mut head = vec![0u8; 4 << 20];
    let n = f.read(&mut head).map_err(|e| failed("cannot read the download", e))?;
    head.truncate(n);
    let (off, size) = find_cab(&head, len).ok_or_else(|| InstallError::Failed("no cabinet was found inside the download".into()))?;
    f.seek(SeekFrom::Start(off as u64)).map_err(|e| failed("cannot read the download", e))?;
    let mut o = std::fs::File::create(out).map_err(|e| failed("cannot write the cabinet", e))?;
    let copied = std::io::copy(&mut f.take(size), &mut o).map_err(|e| failed("cannot copy the cabinet", e))?;
    if copied != size {
        return Err(InstallError::Failed("the cabinet in the download is incomplete".into()));
    }
    Ok(())
}

fn bsdtar_extract(bsdtar: &Path, archive: &Path, into: &Path, members: &[&str]) -> Result<(), InstallError> {
    let out = Command::new(bsdtar).arg("-xf").arg(archive).arg("-C").arg(into).args(members).stdin(Stdio::null()).output().map_err(|e| failed(&format!("cannot start {}", bsdtar.display()), e))?;
    if out.status.success() {
        Ok(())
    } else {
        Err(InstallError::Failed(format!("reading {} failed: {}", archive.display(), String::from_utf8_lossy(&out.stderr).trim())))
    }
}

/// Make sure the verified DLLs of `profile` are in `cache_dir(data, profile)`: reuse them if complete, otherwise download the redistributable
/// (with consent), take the three libraries out of it and verify them. Returns the folder to give to [`crate::prefix::install_native`].
pub fn provide(native_cache: &Path, profile: &NativeProfile, consent: bool, approval: &dyn Fn(&str) -> bool, bsdtar: &Path, dl: &Downloader) -> Result<PathBuf, InstallError> {
    let result = run(native_cache, profile, consent, approval, bsdtar, dl);
    dl.sink.update(Update::Done(match &result {
        Ok(_) => Outcome::Finished("The Direct3D components were downloaded from Microsoft and verified.".into()),
        Err(e) => Outcome::Failed(e.to_string()),
    }));
    result
}

fn run(native_cache: &Path, profile: &NativeProfile, consent: bool, approval: &dyn Fn(&str) -> bool, bsdtar: &Path, dl: &Downloader) -> Result<PathBuf, InstallError> {
    let cache = cache_dir(native_cache, profile);
    if cache_complete(profile, &cache) && terms_accepted(native_cache) {
        return Ok(cache);
    }
    if !consent {
        return Err(InstallError::Blocked(format!("downloading the Direct3D components needs your explicit consent ({} MB from Microsoft: {URL})", SIZE / 1_000_000)));
    }
    let work = native_cache.join(".work");
    std::fs::create_dir_all(&work).map_err(|e| failed("cannot create the work folder", e))?;
    let (part, archive) = (work.join(format!("{ARCHIVE}.part")), work.join(ARCHIVE));
    if !archive.is_file() {
        fetch_verified(dl, URL, &part, &archive, SHA256, "Downloading the Direct3D components")?;
    } else {
        let (h, _) = crate::sha256::file_sha256(&archive).map_err(|e| failed("cannot read the cached download", e))?;
        if !h.eq_ignore_ascii_case(SHA256) {
            let _ = std::fs::remove_file(&archive);
            return Err(InstallError::Failed("the cached download does not match the pinned checksum and was removed; try again".into()));
        }
    }
    dl.sink.update(Update::Progress(Progress::indeterminate(Phase::Installing, "Reading the Direct3D components")));
    let (cabs, files) = (work.join("cabs"), work.join("files"));
    let _ = std::fs::remove_dir_all(&cabs);
    let _ = std::fs::remove_dir_all(&files);
    std::fs::create_dir_all(&cabs).and_then(|_| std::fs::create_dir_all(&files)).map_err(|e| failed("cannot create the work folders", e))?;
    let cab_names: Vec<&str> = MEMBERS.iter().map(|m| m.0).collect();
    let outer = work.join("outer.cab");
    carve_cab(&archive, &outer)?;
    let terms = review_terms(&archive, &outer, bsdtar)?;
    approve_terms(native_cache, &terms, approval)?;
    if cache_complete(profile, &cache) {
        return Ok(cache);
    }
    bsdtar_extract(bsdtar, &outer, &cabs, &cab_names)?;
    for (cab, member, _) in MEMBERS {
        bsdtar_extract(bsdtar, &cabs.join(cab), &files, &[member])?;
    }
    std::fs::create_dir_all(&cache).map_err(|e| failed("cannot create the cache folder", e))?;
    for (_, member, dll) in MEMBERS {
        let spec = profile.files.iter().find(|f| f.dll == *dll).ok_or_else(|| InstallError::Failed(format!("profile {} has no {dll}", profile.id)))?;
        let bytes = std::fs::read(files.join(member)).map_err(|e| failed(&format!("{member} was not found in the package"), e))?;
        if bytes.len() as u64 != spec.size || md5::md5_hex(&bytes) != spec.md5 {
            return Err(InstallError::Failed(format!("{member} from the package is not the file profile {} expects (size or MD5 differs); nothing was installed", profile.id)));
        }
        std::fs::write(cache.join(format!("{dll}.dll")), bytes).map_err(|e| failed("cannot store a verified component", e))?;
    }
    let _ = std::fs::remove_dir_all(&work);
    Ok(cache)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::prefix::{NativeFile, NativeProfile};
    use std::sync::atomic::AtomicBool;
    use std::sync::Mutex;

    fn tmp(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("tsw-dxredist-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    fn script(path: &Path, body: &str) {
        use std::os::unix::fs::PermissionsExt;
        std::fs::write(path, format!("#!/bin/sh\n{body}\n")).unwrap();
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755)).unwrap();
        let _ = std::fs::File::open(path).and_then(|f| f.sync_all());
    }

    /// A fake "redistributable": a tar holding three tar "cabinets" that hold the DLLs (bsdtar reads tar as well as cab/exe, and the real
    /// extraction commands are the same). Returns the payload path and a profile matching its content.
    fn fixture(root: &Path) -> (PathBuf, NativeProfile) {
        let mut files: Vec<NativeFile> = Vec::new();
        let (build, cabs) = (root.join("build"), root.join("cabs"));
        std::fs::create_dir_all(&build).unwrap();
        std::fs::create_dir_all(&cabs).unwrap();
        for (i, (cab, member, dll)) in MEMBERS.iter().enumerate() {
            let content = format!("fake {dll} #{i}").into_bytes();
            std::fs::write(build.join(member), &content).unwrap();
            let st = Command::new("tar").arg("-cf").arg(cabs.join(cab)).arg("-C").arg(&build).arg(member).status().unwrap();
            assert!(st.success());
            let (dll, md5s): (&'static str, &'static str) = (dll, md5::md5_hex(&content).leak());
            files.push(NativeFile { dll, size: content.len() as u64, md5: md5s });
        }
        let payload = root.join("redist.tar");
        let mut c = Command::new("tar");
        c.arg("-cf").arg(&payload).arg("-C").arg(&cabs);
        for m in MEMBERS {
            c.arg(m.0);
        }
        assert!(c.status().unwrap().success());
        (payload, NativeProfile { id: "test-profile", files: Box::leak(files.into_boxed_slice()) })
    }

    fn fake_curl(dir: &Path, payload: &Path) -> PathBuf {
        let curl = dir.join("curl");
        script(
            &curl,
            &format!(
                r#"case " $* " in *" --head "*) printf 'HTTP/1.1 200 OK\r\nContent-Length: %s\r\n\r\n' "$(wc -c < '{p}')"; exit 0;; esac
out=""; prev=""; for a in "$@"; do if [ "$prev" = "--output" ]; then out="$a"; fi; prev="$a"; done
cat '{p}' > "$out""#,
                p = payload.display()
            ),
        );
        curl
    }

    #[test]
    fn cached_components_require_current_acceptance_but_existing_accepted_cache_works_offline() {
        let root = tmp("cache-terms");
        let content = b"fixture dll";
        let profile = NativeProfile { id: "fixture-cache", files: Box::leak(vec![NativeFile { dll: "fixture", size: content.len() as u64, md5: md5::md5_hex(content).leak() }].into_boxed_slice()) };
        let data = root.join("data");
        let cache = cache_dir(&data, &profile);
        std::fs::create_dir_all(&cache).unwrap();
        std::fs::write(cache.join("fixture.dll"), content).unwrap();
        let before = sha256::file_sha256(&cache.join("fixture.dll")).unwrap();
        let cancel = AtomicBool::new(false);
        let dl = Downloader { curl: Path::new("/must-not-run"), cancel: &cancel, sink: &crate::progress::Silent };
        assert!(matches!(provide(&data, &profile, false, &|_| panic!("no prompt before download consent"), Path::new("/must-not-run"), &dl), Err(InstallError::Blocked(_))));
        approve_terms(&data, "Harmless fixture terms", &|_| true).unwrap();
        assert!(terms_accepted(&data));
        assert_eq!(provide(&data, &profile, false, &|_| panic!("no repeat prompt"), Path::new("/must-not-run"), &dl).unwrap(), cache);
        assert_eq!(sha256::file_sha256(&cache.join("fixture.dll")).unwrap(), before, "existing data unchanged");
        std::fs::write(data.join(ACCEPTANCE), "different terms").unwrap();
        assert!(!terms_accepted(&data));
        assert!(provide(&data, &profile, false, &|_| false, Path::new("/must-not-run"), &dl).is_err());
        assert_eq!(sha256::file_sha256(&cache.join("fixture.dll")).unwrap(), before);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn contract_text_must_be_complete_and_hash_pinned_without_executing_anything() {
        let header = b"Fixture terms:";
        let body = b"Fixture terms: personal fixture use\r\n";
        let mut bytes = body.to_vec();
        bytes.push(0);
        let hash = sha256::sha256_hex(body);
        assert_eq!(contract(&bytes, header, false, &hash).unwrap(), String::from_utf8(body.to_vec()).unwrap());
        assert!(contract(body, header, false, &hash).is_err(), "truncation is not accepted");
        assert!(contract(&bytes, header, false, &"0".repeat(64)).is_err(), "altered terms fail closed");
        let repeated = [body.as_slice(), body.as_slice()].concat();
        assert!(contract(&repeated, header, true, &hash).is_ok());
        assert!(contract(b"not a contract", header, false, &hash).is_err());
    }

    #[test]
    fn declining_leaves_no_acceptance_and_only_explicit_acceptance_is_cached() {
        let root = tmp("terms");
        let data = root.join("cache");
        assert!(approve_terms(&data, "Harmless fixture terms", &|_| false).is_err());
        assert!(!data.exists(), "no DLLs or acceptance record created on decline");
        let calls = std::cell::Cell::new(0);
        approve_terms(&data, "Harmless fixture terms", &|text| {
            assert_eq!(text, "Harmless fixture terms");
            calls.set(calls.get() + 1);
            true
        })
        .unwrap();
        assert_eq!(calls.get(), 1);
        assert_eq!(std::fs::read_to_string(data.join("microsoft-terms.txt")).unwrap(), "Harmless fixture terms");
        approve_terms(&data, "Harmless fixture terms", &|_| panic!("already accepted reviewed package")).unwrap();
        std::fs::write(data.join(ACCEPTANCE), "another package").unwrap();
        assert!(approve_terms(&data, "Harmless fixture terms", &|_| false).is_err(), "wrong package cannot inherit agreement");
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn the_cabinet_is_found_inside_an_executable_and_nonsense_is_not() {
        let _exec = crate::test_exec_lock();
        let mut exe = b"MZ....MSCF junk MSCF".to_vec(); // two false signatures (junk after them)
        exe.extend_from_slice(&[7u8; 40]);
        let at = exe.len();
        exe.extend_from_slice(b"MSCF");
        exe.extend_from_slice(&0u32.to_le_bytes());
        exe.extend_from_slice(&100u32.to_le_bytes());
        exe.resize(at + 100, 0);
        assert_eq!(find_cab(&exe, exe.len() as u64), Some((at, 100)));
        assert_eq!(find_cab(&exe, (at + 99) as u64), None, "a declared size beyond the file is refused");
        assert_eq!(find_cab(b"no cabinet here", 15), None);
    }

    #[test]
    fn the_pin_and_the_members_are_consistent_with_the_shipped_profile() {
        let _exec = crate::test_exec_lock();
        assert!(URL.starts_with("https://download.microsoft.com/") && crate::sha256::is_hex_digest(SHA256));
        let p = &crate::prefix::PROFILE_D3D_NATIVE_MS2010;
        assert_eq!(MEMBERS.len(), p.files.len());
        for (_, member, dll) in MEMBERS {
            assert!(member.to_ascii_lowercase() == format!("{dll}.dll") && p.files.iter().any(|f| f.dll == *dll), "{member}");
        }
        assert!(crate::prefix::native_profile(p.id).is_some());
    }

    #[test]
    fn a_missing_consent_downloads_nothing_and_a_verified_package_yields_verified_files_that_are_cached() {
        let _exec = crate::test_exec_lock();
        // the real pin cannot match a fixture, so this test drives `run` through the same steps with a fixture hash
        let root = tmp("ok");
        let (payload, profile) = fixture(&root);
        let data = root.join("data");
        let cancel = AtomicBool::new(false);
        let seen = Mutex::new(Vec::<Update>::new());
        let sink = |u: Update| seen.lock().unwrap().push(u);
        let dl = Downloader { curl: &fake_curl(&root, &payload), cancel: &cancel, sink: &sink };
        let r = provide(&data, &profile, false, &|_| panic!("no licence prompt without download consent"), Path::new("bsdtar"), &dl);
        assert!(matches!(r, Err(InstallError::Blocked(_))), "{r:?}");
        assert!(!data.join("tsw-launch").exists(), "no consent: nothing is created");
        // with consent the real SHA-256 pin rejects the fixture (it is not Microsoft's file) and nothing is installed
        let r = provide(&data, &profile, true, &|_| panic!("no licence prompt for an unverified fixture"), Path::new("bsdtar"), &dl);
        assert!(matches!(&r, Err(InstallError::Failed(m)) if m.contains("checksum mismatch")), "{r:?}");
        assert!(!cache_dir(&data, &profile).exists());
        assert!(!data.join("tsw-launch/native/.work").join(ARCHIVE).exists(), "a mismatching download is discarded");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn extraction_verifies_every_file_against_the_profile_and_refuses_a_deviating_one() {
        let _exec = crate::test_exec_lock();
        let root = tmp("extract");
        let (payload, profile) = fixture(&root);
        let work = root.join("w");
        let (cabs, files) = (work.join("cabs"), work.join("files"));
        std::fs::create_dir_all(&cabs).unwrap();
        std::fs::create_dir_all(&files).unwrap();
        let names: Vec<&str> = MEMBERS.iter().map(|m| m.0).collect();
        bsdtar_extract(Path::new("bsdtar"), &payload, &cabs, &names).unwrap();
        for (cab, member, _) in MEMBERS {
            bsdtar_extract(Path::new("bsdtar"), &cabs.join(cab), &files, &[member]).unwrap();
        }
        for (f, (_, member, _)) in profile.files.iter().zip(MEMBERS) {
            let b = std::fs::read(files.join(member)).unwrap();
            assert!(b.len() as u64 == f.size && md5::md5_hex(&b) == f.md5);
        }
        // a member that is not in the archive is an error, not a silent skip
        assert!(bsdtar_extract(Path::new("bsdtar"), &payload, &cabs, &["nope.cab"]).is_err());
        assert!(!cache_complete(&profile, &files), "the cache check wants lower-case names and the right content");
        let _ = std::fs::remove_dir_all(&root);
    }
}
