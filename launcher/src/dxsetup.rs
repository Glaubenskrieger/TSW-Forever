//! Native Windows: help for missing Direct3D libraries of the DirectX End-User Runtimes (June 2010).
//!
//! The original 32-bit client loads `d3dcompiler_43.dll`, `d3dx9_43.dll` and `d3dx11_43.dll` (see [`crate::winnative::DIRECTX_LIBRARIES`]); Windows 10 and 11 do not
//! ship them. When they are missing, and only when the user asks for it, the launcher fetches **Microsoft's own web installer** `dxwebsetup.exe` from
//! `download.microsoft.com` over https, checks it and starts it. The launcher never extracts, copies, patches or redistributes a Microsoft library: Microsoft's
//! installer shows its own terms and installs the components itself (Windows asks for permission through its normal prompt).
//!
//! Checks before the installer is started, both must pass:
//! 1. the SHA-256 of the file equals the pinned value of the release that was examined (2026-11-08: version 9.29.1974.0, 295 320 bytes);
//! 2. the Authenticode signature is valid and the signer is Microsoft Corporation (asked from Windows through PowerShell).
//!
//! If Microsoft publishes a newer web installer the hash no longer matches: the download is then refused with a message that points to the official page,
//! and the pin is updated after the new file was examined.

use std::path::{Path, PathBuf};
use std::process::Command;

use crate::install::{self, InstallError, Layout};

/// The official download address (Microsoft Download Center, "DirectX End-User Runtime Web Installer", id 35).
pub const URL: &str = "https://download.microsoft.com/download/1/7/1/1718CCC4-6315-4D8E-9543-8E28A4E18C4C/dxwebsetup.exe";
/// The official page of the same package (shown to the user if the download cannot be used).
pub const PAGE: &str = "https://www.microsoft.com/download/details.aspx?id=35";
/// SHA-256 of `dxwebsetup.exe` version 9.29.1974.0 as served by `download.microsoft.com` on 2026-11-08 (trust on first use together with the valid Microsoft signature).
pub const SHA256: &str = "2cf71d098c608c56e07f4655855a886c3102553f648df88458df616b26fd612f";
pub const FILE_NAME: &str = "dxwebsetup.exe";

/// Where the downloaded installer is kept: the launcher's own download folder.
pub fn cache_file(layout: &Layout) -> PathBuf {
    layout.downloads().join(FILE_NAME)
}

/// Download Microsoft's installer (resumable, https only, certificates checked) and check its hash. Returns the verified file. The signature is checked
/// separately by [`verify_signature`] right before the start.
pub fn fetch(layout: &Layout, dl: &install::Downloader) -> Result<PathBuf, InstallError> {
    std::fs::create_dir_all(layout.downloads()).map_err(|e| InstallError::Failed(format!("cannot create {}: {e}", layout.downloads().display())))?;
    let dest = cache_file(layout);
    if dest.is_file() {
        match crate::sha256::file_sha256(&dest) {
            Ok((h, _)) if h.eq_ignore_ascii_case(SHA256) => return Ok(dest),
            _ => {
                // an old or damaged cache file is the launcher's own file: replace it
                let _ = std::fs::remove_file(&dest);
            }
        }
    }
    let part = layout.downloads().join(format!("{FILE_NAME}.part"));
    install::fetch_verified(dl, URL, &part, &dest, SHA256, "Downloading Microsoft's DirectX installer")
}

/// The PowerShell script that prints `status|subject` of the file's Authenticode signature. `None` if the path could end the quoted string.
pub fn signature_script(path: &Path) -> Option<String> {
    let p = path.to_string_lossy();
    if p.contains('\'') || p.contains('\n') || p.contains('\r') || p.contains('\0') {
        return None;
    }
    Some(format!("$s = Get-AuthenticodeSignature -LiteralPath '{p}'; '{{0}}|{{1}}' -f $s.Status, $s.SignerCertificate.Subject"))
}

/// Judge the output of [`signature_script`]: the signature must be valid and the signer Microsoft Corporation.
pub fn judge_signature(output: &str) -> Result<(), String> {
    let line = output.lines().map(str::trim).find(|l| !l.is_empty()).unwrap_or("");
    let (status, subject) = line.split_once('|').unwrap_or((line, ""));
    if !status.eq_ignore_ascii_case("Valid") {
        return Err(format!("the file's signature is not valid (Windows says: {})", if status.is_empty() { "nothing" } else { status }));
    }
    // the subject is a distinguished name such as `CN=Microsoft Corporation, O=Microsoft Corporation, L=Redmond, ...`
    let signer_ok = subject.split(',').map(str::trim).any(|part| part.eq_ignore_ascii_case("CN=Microsoft Corporation") || part.eq_ignore_ascii_case("O=Microsoft Corporation"));
    if signer_ok {
        Ok(())
    } else {
        Err(format!("the file is not signed by Microsoft Corporation (signer: {})", if subject.is_empty() { "unknown" } else { subject }))
    }
}

/// Ask Windows for the signature of `path` and judge it. Without PowerShell the answer is "not verified" (the installer is then not started).
#[cfg(windows)]
pub fn verify_signature(path: &Path) -> Result<(), String> {
    use std::os::windows::process::CommandExt;
    let script = signature_script(path).ok_or("the file name contains characters that cannot be checked safely")?;
    let mut cmd = Command::new("powershell");
    cmd.args(["-NoLogo", "-NoProfile", "-NonInteractive", "-Command", &script]).stdin(std::process::Stdio::null());
    cmd.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
    let out = cmd.output().map_err(|e| format!("the signature could not be checked ({e})"))?;
    judge_signature(&String::from_utf8_lossy(&out.stdout))
}

/// Not Windows: nothing to verify and nothing to run.
#[cfg(not(windows))]
pub fn verify_signature(_path: &Path) -> Result<(), String> {
    Err("Microsoft's DirectX installer is only for Windows".into())
}

/// The command that starts the verified installer through the shell (so that Windows shows its permission prompt if needed). Microsoft's installer asks for the
/// licence itself; the launcher passes no switch that would hide or accept anything.
pub fn start_command(installer: &Path) -> Command {
    let dir = installer.parent().unwrap_or_else(|| Path::new("."));
    crate::winnative::start_command(installer, dir)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_source_is_the_official_microsoft_host_over_https_with_a_pinned_hash() {
        assert!(URL.starts_with("https://download.microsoft.com/download/") && URL.ends_with("/dxwebsetup.exe"));
        assert!(PAGE.starts_with("https://www.microsoft.com/"));
        assert!(crate::sha256::is_hex_digest(SHA256) && SHA256 == SHA256.to_ascii_lowercase());
        assert_eq!(FILE_NAME, "dxwebsetup.exe");
    }

    #[test]
    fn only_a_valid_microsoft_signature_passes() {
        let ms = "Valid|CN=Microsoft Corporation, O=Microsoft Corporation, L=Redmond, S=Washington, C=US";
        assert_eq!(judge_signature(ms), Ok(()));
        assert_eq!(judge_signature(&format!("\r\n{ms}\r\n")), Ok(()), "blank lines and line ends do not matter");
        assert_eq!(judge_signature("Valid|CN=MICROSOFT CORPORATION, OU=MOPR, O=Microsoft Corporation, L=Redmond"), Ok(()), "case does not matter");
        for bad in [
            "NotSigned|",
            "HashMismatch|CN=Microsoft Corporation, O=Microsoft Corporation",
            "UnknownError|CN=Microsoft Corporation",
            "Valid|CN=Funcom Oslo, O=Funcom Oslo, C=NO",
            "Valid|CN=Not Microsoft Corporation Ltd, O=Evil",
            "Valid|CN=Microsoft Corporation Fake, O=Evil",
            "Valid|",
            "",
            "garbage",
        ] {
            assert!(judge_signature(bad).is_err(), "{bad:?} must not pass");
        }
    }

    #[test]
    fn the_signature_script_refuses_paths_that_could_leave_the_quotes() {
        assert!(signature_script(Path::new("C:\\Users\\Anna\\AppData\\Local\\tsw-forever\\downloads\\dxwebsetup.exe")).is_some());
        assert!(signature_script(Path::new("C:\\Users\\O'Brien\\dxwebsetup.exe")).is_none());
        assert!(signature_script(Path::new("C:\\a\nb\\x.exe")).is_none());
        let s = signature_script(Path::new("C:\\Users\\Ünal Müller\\dxwebsetup.exe")).unwrap();
        assert!(s.contains("-LiteralPath 'C:\\Users\\Ünal Müller\\dxwebsetup.exe'") && s.contains("Get-AuthenticodeSignature"));
    }

    #[test]
    fn the_installer_is_started_without_any_switch_that_hides_or_accepts_something() {
        let c = start_command(Path::new("C:\\Users\\Anna\\AppData\\Local\\tsw-forever\\downloads\\dxwebsetup.exe"));
        let args: Vec<String> = c.get_args().map(|a| a.to_string_lossy().into_owned()).collect();
        let line = args.join(" ");
        assert!(!line.to_ascii_lowercase().contains("/q") && !line.to_ascii_lowercase().contains("/silent") && !line.to_ascii_lowercase().contains("/accept"), "{line}");
    }

    /// Network test (`cargo test -- --ignored`): the real download from download.microsoft.com equals the pin and carries Microsoft's valid signature. It only
    /// downloads into a temporary folder and starts nothing.
    #[cfg(windows)]
    #[test]
    #[ignore = "needs the network"]
    fn the_real_microsoft_download_matches_the_pin_and_the_signature() {
        let base = std::env::temp_dir().join(format!("tsw-dxsetup-net-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        let layout = Layout { base: base.clone() };
        let curl = crate::doctor::find_program(Path::new("curl"), std::env::var_os("PATH").as_deref()).expect("curl.exe is part of Windows 10/11");
        let cancel = std::sync::atomic::AtomicBool::new(false);
        let sink = crate::progress::Silent;
        let file = fetch(&layout, &install::Downloader { curl: &curl, cancel: &cancel, sink: &sink }).expect("download and hash check");
        assert!(file.ends_with("dxwebsetup.exe") && file.metadata().unwrap().len() < 2_000_000);
        assert_eq!(verify_signature(&file), Ok(()), "Microsoft's signature is valid");
        // a second call uses the cache and needs no network
        assert_eq!(fetch(&layout, &install::Downloader { curl: Path::new("does-not-exist"), cancel: &cancel, sink: &sink }).unwrap(), file);
        let _ = std::fs::remove_dir_all(&base);
    }

    #[cfg(windows)]
    #[test]
    fn windows_refuses_an_unsigned_file() {
        let _exec = crate::test_exec_lock(); // starts PowerShell
                                             // the running test program is not signed: it must not pass, and the answer is an explanation, not a panic
        let me = std::env::current_exe().unwrap();
        assert!(verify_signature(&me).is_err());
    }
}
