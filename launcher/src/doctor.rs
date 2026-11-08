//! Pre-flight checks for known problems (diagnosis only; nothing is installed or changed).
//!
//! Finding 1 (verified by trace): the 32-bit client process loads Wine's 32-bit font engine, which needs the 32-bit
//! FreeType library on the host. Without it `AddFontMemResourceEx` silently returns 0 and the client logs
//! "FontSystem_t failed AddFontMemResourceEx". The other checks cover what a start needs at all.

use std::path::{Path, PathBuf};

use crate::runner::Runner;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Level {
    Ok,
    /// The client can start, but something is likely to be wrong (a hint is given).
    Warn,
    /// A start cannot succeed.
    Fail,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Finding {
    pub level: Level,
    pub what: String,
    pub hint: String,
    /// Stable identifier of the message (for example `doctor.wine_missing`), `""` if none. A front end may translate by
    /// `code` + `args`; `what`/`hint` stay the English fallback and are what the CLI prints.
    pub code: &'static str,
    /// The values that were inserted into `what`/`hint`, in the order the message uses them.
    pub args: Vec<String>,
}

impl Finding {
    pub(crate) fn ok(what: String) -> Self {
        Finding { level: Level::Ok, what, hint: String::new(), code: "", args: Vec::new() }
    }
    pub(crate) fn bad(level: Level, what: String, hint: &str) -> Self {
        Finding { level, what, hint: hint.to_string(), code: "", args: Vec::new() }
    }
    /// Attach the stable message code and its arguments.
    pub(crate) fn coded<S: Into<String>>(mut self, code: &'static str, args: impl IntoIterator<Item = S>) -> Self {
        self.code = code;
        self.args = args.into_iter().map(Into::into).collect();
        self
    }
}

/// Native Windows: the Direct3D libraries the client loads that are missing from the system folders. A warning only: the original installer or the user can
/// add them, and nothing is downloaded by the launcher.
pub fn check_directx(missing: &[&str]) -> Vec<Finding> {
    match crate::winnative::directx_runtime(missing) {
        None => vec![Finding::ok("the Direct3D libraries of the DirectX June 2010 runtime are present".to_string()).coded("doctor.directx_ok", Vec::<String>::new())],
        Some(message) => {
            vec![Finding::bad(Level::Warn, message, "install Microsoft's DirectX End-User Runtimes (June 2010) yourself").coded("doctor.directx_missing", missing.iter().map(|m| m.to_string()))]
        }
    }
}

/// Standard multilib directories of common distributions.
/// (`/app/lib/i386-linux-gnu` is where a Flatpak mounts the 32-bit compatibility extension.)
pub const LIB32_DIRS: &[&str] = &["/usr/lib32", "/usr/lib/i386-linux-gnu", "/lib32", "/usr/lib/i686-linux-gnu", "/app/lib/i386-linux-gnu"];
const VULKAN_ICD_DIRS: &[&str] = &["/usr/share/vulkan/icd.d", "/etc/vulkan/icd.d", "/usr/local/share/vulkan/icd.d"];

fn has_lib(dirs: &[PathBuf], name_prefix: &str) -> Option<PathBuf> {
    for d in dirs {
        let Ok(rd) = std::fs::read_dir(d) else {
            continue;
        };
        for e in rd.flatten() {
            if e.file_name().to_string_lossy().starts_with(name_prefix) {
                return Some(e.path());
            }
        }
    }
    None
}

/// Check the 32-bit libraries the 32-bit Wine side needs for text. `dirs` is injectable for tests.
pub fn check_32bit_fonts(dirs: &[PathBuf]) -> Vec<Finding> {
    let mut out = Vec::new();
    for (prefix, pkg) in [("libfreetype.so.6", "lib32-freetype2 (Debian: libfreetype6:i386)"), ("libfontconfig.so.1", "lib32-fontconfig (Debian: libfontconfig1:i386)")] {
        match has_lib(dirs, prefix) {
            Some(p) => out.push(Finding::ok(format!("32-bit {prefix} found: {}", p.display())).coded("doctor.font_found", [prefix.to_string(), p.display().to_string()])),
            None => {
                let searched = dirs.iter().map(|d| d.display().to_string()).collect::<Vec<_>>().join(", ");
                out.push(
                    Finding::bad(
                        Level::Warn,
                        format!("32-bit {prefix} not found in {searched}"),
                        &format!(
                            "install {pkg}; without it the 32-bit client has no Wine font engine (AddFontMemResourceEx fails). \
                             A runtime that ships 32-bit libraries (Steam Linux Runtime) also avoids this."
                        ),
                    )
                    .coded("doctor.font_missing", [prefix.to_string(), searched, pkg.to_string()]),
                )
            }
        }
    }
    out
}

/// 32-bit X11 libraries that Wine's 32-bit `winex11.so` loads with `dlopen` (list read from the driver's strings).
/// Verified consequence of a missing `libXi`: `warn:cursor:grab_clipping_window XInput2 not supported, refusing to clip`
/// repeated ~15,000 times per second; for a full-screen window the client then spins in a message loop and never starts
/// the engine (documentation/architecture.md). The other libraries are needed for display modes, cursors, etc.
pub const X11_LIBS: &[(&str, &str, &str)] = &[
    ("libXi.so.6", "lib32-libxi (Debian: libxi6:i386)", "XInput2: without it Wine refuses cursor clipping and a full-screen client spins in a message loop"),
    ("libXrandr.so.2", "lib32-libxrandr (Debian: libxrandr2:i386)", "display mode changes (ChangeDisplaySettings) fail"),
    ("libXcursor.so.1", "lib32-libxcursor (Debian: libxcursor1:i386)", "custom mouse cursors cannot be shown"),
    ("libXfixes.so.3", "lib32-libxfixes (Debian: libxfixes3:i386)", "pointer barriers/cursor handling are unavailable"),
    ("libXcomposite.so.1", "lib32-libxcomposite (Debian: libxcomposite1:i386)", "window composition features are unavailable"),
    ("libXinerama.so.1", "lib32-libxinerama (Debian: libxinerama1:i386)", "multi-monitor geometry is unavailable"),
    ("libXrender.so.1", "lib32-libxrender (Debian: libxrender1:i386)", "rendering extensions are unavailable"),
];

/// Check the 32-bit X11 libraries. `dirs` is injectable for tests.
pub fn check_32bit_x11(dirs: &[PathBuf]) -> Vec<Finding> {
    X11_LIBS
        .iter()
        .map(|(lib, pkg, why)| match has_lib(dirs, lib) {
            Some(p) => Finding::ok(format!("32-bit {lib} found: {}", p.display())).coded("doctor.x11_found", [lib.to_string(), p.display().to_string()]),
            None => Finding::bad(Level::Warn, format!("32-bit {lib} not found"), &format!("install {pkg}: {why}")).coded("doctor.x11_missing", [lib.to_string(), pkg.to_string(), why.to_string()]),
        })
        .collect()
}

pub fn default_dirs() -> Vec<PathBuf> {
    LIB32_DIRS.iter().map(Path::new).map(Path::to_path_buf).collect()
}

/// Is `program` a runnable file (when it contains a path separator) or found on `path_var`?
pub fn find_program(program: &Path, path_var: Option<&std::ffi::OsStr>) -> Option<PathBuf> {
    if program.components().count() > 1 || program.is_absolute() {
        return program.is_file().then(|| program.to_path_buf());
    }
    std::env::split_paths(path_var?).find_map(|d| {
        let plain = d.join(program);
        if plain.is_file() {
            return Some(plain);
        }
        // Windows programs carry `.exe` (`curl` is `curl.exe`); elsewhere the name is exact
        let exe = (cfg!(windows) && program.extension().is_none()).then(|| d.join(format!("{}.exe", program.display())));
        exe.filter(|p| p.is_file())
    })
}

/// Everything a start needs. Pure inspection; `lib_dirs`/`path_var` are injectable for tests.
pub fn check_all(exe: &Path, runner: &Runner, prefix: Option<&Path>, lib_dirs: &[PathBuf], path_var: Option<&std::ffi::OsStr>, has_display: bool) -> Vec<Finding> {
    let mut out = Vec::new();
    match std::fs::read(exe) {
        Err(e) => out.push(
            Finding::bad(Level::Fail, format!("client executable {} is not readable: {e}", exe.display()), "give --exe or client_exe a path to TheSecretWorld[DX11].exe")
                .coded("doctor.exe_unreadable", [exe.display().to_string(), e.to_string()]),
        ),
        Ok(bytes) if !bytes.starts_with(b"MZ") => {
            out.push(Finding::bad(Level::Fail, format!("{} is not a Windows executable", exe.display()), "wrong file?").coded("doctor.exe_not_windows", [exe.display().to_string()]))
        }
        Ok(bytes) => {
            if crate::key::find_cipher_key(&bytes).is_some() {
                out.push(Finding::ok(format!("client executable recognised: {}", exe.display())).coded("doctor.exe_ok", [exe.display().to_string()]));
            } else {
                out.push(
                    Finding::bad(
                        Level::Fail,
                        format!("{} is not a recognised client build (patcher-key check not found)", exe.display()),
                        "only TheSecretWorld.exe / TheSecretWorldDX11.exe can be started",
                    )
                    .coded("doctor.exe_unknown_build", [exe.display().to_string()]),
                );
            }
        }
    }
    if let Runner::Wine { wine, .. } = runner {
        match find_program(wine, path_var) {
            Some(p) => out.push(Finding::ok(format!("wine program found: {}", p.display())).coded("doctor.wine_ok", [p.display().to_string()])),
            None => out.push(
                Finding::bad(Level::Fail, format!("wine program {} not found", wine.display()), "use --runner /path/to/wine (a Proton/GE-Proton 'files/bin/wine') or install wine")
                    .coded("doctor.wine_missing", [wine.display().to_string()]),
            ),
        }
        match prefix {
            Some(p) if p.is_dir() => out.push(Finding::ok(format!("prefix exists: {}", p.display())).coded("doctor.prefix_ok", [p.display().to_string()])),
            Some(p) => out.push(
                Finding::bad(
                    Level::Warn,
                    format!("prefix {} does not exist yet", p.display()),
                    "wine will create an empty prefix; the game needs the usual components (DXVK, d3dx, vcrun) installed in it",
                )
                .coded("doctor.prefix_new", [p.display().to_string()]),
            ),
            None => out.push(
                Finding::bad(Level::Warn, "no prefix set (wine would use ~/.wine)".to_string(), "pass --prefix or prefix = ... in the config").coded("doctor.prefix_unset", Vec::<String>::new()),
            ),
        }
        if cfg!(unix) {
            out.extend(check_32bit_fonts(lib_dirs));
            out.extend(check_32bit_x11(lib_dirs));
            if has_display {
                out.push(Finding::ok("a display is available (DISPLAY or WAYLAND_DISPLAY)".to_string()).coded("doctor.display_ok", Vec::<String>::new()));
            } else {
                out.push(
                    Finding::bad(Level::Warn, "no DISPLAY / WAYLAND_DISPLAY in the environment".to_string(), "start from a graphical session; the client needs a window")
                        .coded("doctor.display_missing", Vec::<String>::new()),
                );
            }
            let vulkan = VULKAN_ICD_DIRS.iter().any(|d| std::fs::read_dir(d).map(|rd| rd.flatten().any(|e| e.file_name().to_string_lossy().ends_with(".json"))).unwrap_or(false));
            if vulkan {
                out.push(Finding::ok("a Vulkan driver description (ICD) is installed".to_string()).coded("doctor.vulkan_ok", Vec::<String>::new()));
            } else {
                out.push(
                    Finding::bad(Level::Warn, "no Vulkan ICD found".to_string(), "DXVK needs a Vulkan driver (vulkan-radeon / nvidia-utils / vulkan-intel)")
                        .coded("doctor.vulkan_missing", Vec::<String>::new()),
                );
            }
        }
    }
    out
}

/// Process exit code for `--doctor`: 0 all fine, 4 warnings only, 3 something is missing that prevents a start.
pub fn exit_code(findings: &[Finding]) -> u8 {
    if findings.iter().any(|f| f.level == Level::Fail) {
        3
    } else if findings.iter().any(|f| f.level == Level::Warn) {
        4
    } else {
        0
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn a_program_name_finds_its_exe_on_windows_only() {
        use super::find_program;
        use std::path::Path;
        let dir = std::env::temp_dir().join(format!("tsw-findprog-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("curl.exe"), b"MZ").unwrap();
        std::fs::write(dir.join("exact"), b"x").unwrap();
        let path = std::env::join_paths([&dir]).unwrap();
        assert_eq!(find_program(Path::new("curl"), Some(&path)).is_some(), cfg!(windows), "`curl` finds `curl.exe` on Windows, never elsewhere");
        assert_eq!(find_program(Path::new("exact"), Some(&path)), Some(dir.join("exact")), "an exact name is always found");
        assert!(find_program(Path::new("missing"), Some(&path)).is_none());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn missing_direct3d_libraries_are_a_warning_never_a_failure() {
        let ok = check_directx(&[]);
        assert_eq!(ok.len(), 1);
        assert_eq!(ok[0].level, Level::Ok);
        let warn = check_directx(&["d3dx9_43.dll"]);
        assert_eq!(warn[0].level, Level::Warn);
        assert!(warn[0].what.contains("d3dx9_43.dll") && warn[0].what.contains("does not download"));
        assert_eq!(exit_code(&warn), 4, "a warning has its own exit code and is not the failure code");
        assert_ne!(exit_code(&warn), exit_code(&[Finding::bad(Level::Fail, "x".into(), "")]));
    }

    use super::*;

    fn tmp(name: &str) -> PathBuf {
        let t = std::env::temp_dir().join(format!("tsw-doctor-{name}-{}", std::process::id()));
        std::fs::create_dir_all(&t).unwrap();
        t
    }

    #[test]
    fn missing_libs_are_reported_with_package_hint() {
        let t = tmp("empty");
        let f = check_32bit_fonts(std::slice::from_ref(&t));
        assert!(f.iter().all(|x| x.level == Level::Warn) && f.len() == 2);
        assert!(f[0].hint.contains("lib32-freetype2"));
        std::fs::remove_dir_all(&t).ok();
    }

    #[test]
    fn missing_x11_libs_name_the_package_and_the_xinput_consequence() {
        let t = tmp("x11empty");
        let f = check_32bit_x11(std::slice::from_ref(&t));
        assert_eq!(f.len(), X11_LIBS.len());
        assert!(f.iter().all(|x| x.level == Level::Warn));
        assert!(f[0].hint.contains("lib32-libxi") && f[0].hint.contains("spins in a message loop"));
        for (lib, _, _) in X11_LIBS {
            std::fs::write(t.join(format!("{lib}.0")), b"").unwrap();
        }
        assert!(check_32bit_x11(std::slice::from_ref(&t)).iter().all(|x| x.level == Level::Ok));
        std::fs::remove_dir_all(&t).ok();
    }

    #[test]
    fn present_libs_pass() {
        let t = tmp("full");
        std::fs::write(t.join("libfreetype.so.6.20.1"), b"").unwrap();
        std::fs::write(t.join("libfontconfig.so.1.14.0"), b"").unwrap();
        assert!(check_32bit_fonts(std::slice::from_ref(&t)).iter().all(|x| x.level == Level::Ok));
        std::fs::remove_dir_all(&t).ok();
    }

    #[test]
    fn full_check_distinguishes_fail_warn_ok() {
        let t = tmp("all");
        let exe = t.join("tsw.exe");
        let mut bytes = b"MZ..Invalid patcher key arg \0\0\0".to_vec();
        bytes.extend_from_slice(&[7u8; 16]);
        bytes.extend_from_slice(b"Could not decode patcher key \0");
        std::fs::write(&exe, &bytes).unwrap();
        let fake_wine = t.join("wine");
        std::fs::write(&fake_wine, b"#!/bin/sh\n").unwrap();
        let r = Runner::Wine { wine: fake_wine.clone(), prefix: None };
        let f = check_all(&exe, &r, Some(&t), std::slice::from_ref(&t), None, true);
        assert_eq!(f.iter().filter(|x| x.level == Level::Fail).count(), 0, "{f:?}");
        assert!(exit_code(&f) == 4 || exit_code(&f) == 0, "libs/ICD may be missing in the test environment, never a failure");
        // not a client, no wine, unreadable
        let r2 = Runner::Wine { wine: PathBuf::from("definitely-not-installed-wine"), prefix: None };
        let other = t.join("other.exe");
        std::fs::write(&other, b"MZ plain").unwrap();
        let f2 = check_all(&other, &r2, None, &[], Some(std::ffi::OsStr::new("/nonexistent")), false);
        assert_eq!(exit_code(&f2), 3);
        assert!(f2.iter().filter(|x| x.level == Level::Fail).count() >= 2);
        assert_eq!(exit_code(&check_all(&t.join("missing.exe"), &Runner::Native, None, &[], None, true)), 3);
        std::fs::remove_dir_all(&t).ok();
    }

    #[test]
    fn find_program_by_name_and_by_path() {
        let t = tmp("prog");
        std::fs::write(t.join("wine"), b"").unwrap();
        assert!(find_program(Path::new("wine"), Some(t.as_os_str())).is_some());
        assert!(find_program(&t.join("wine"), None).is_some());
        assert!(find_program(Path::new("nope"), Some(t.as_os_str())).is_none());
        std::fs::remove_dir_all(&t).ok();
    }
}
