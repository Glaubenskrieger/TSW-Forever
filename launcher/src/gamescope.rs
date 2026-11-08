//! Optional Gamescope wrapper (no upscaling).
//!
//! Gamescope is an OPTIONAL extra. The normal start (Wine/Proton directly, no Gamescope) is the default and never needs it. With `gamescope = true` the
//! whole Wine command is wrapped in a plain Gamescope window whose internal and output resolution are identical (the monitor's native size, or an explicit
//! `gamescope_output`), so the picture is not scaled and the user interface keeps its normal size.
//!
//! FSR/upscaling was removed on purpose (decision of the owner, 2026-10-31): Gamescope's FSR scales the finished picture - 3D, menus, text and HUD alike -
//! so everything looks larger (measured: UI factor = output / render resolution, documentation/architecture.md). Old configuration
//! keys (`fsr`, `fsr_internal`, `fsr_output`, `fsr_sharpness`) are still read and ignored ([`crate::config`]).
//!
//! Everything here is pure (command lines are argument vectors, never shell text), so it is testable without Gamescope being installed.

use std::path::{Path, PathBuf};

use crate::doctor::{Finding, Level};

/// Where the Gamescope Vulkan-layer extension of the Freedesktop runtime mounts its binary inside a Flatpak sandbox.
/// UNVERIFIED on a real Flatpak build (see documentation/packaging.md).
pub const FLATPAK_EXTENSION_BIN: &str = "/usr/lib/extensions/vulkan/gamescope/bin/gamescope";

/// A resolved plan: the one resolution that is used inside and outside.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Plan {
    pub size: (u32, u32),
}

/// Resolve against the monitor size. `None` when Gamescope is off (the normal start path). An explicit `output` wins over the monitor.
pub fn plan(enabled: bool, output: Option<(u32, u32)>, monitor: Option<(u32, u32)>) -> Result<Option<Plan>, String> {
    if !enabled {
        return Ok(None);
    }
    let size = output.or(monitor).ok_or("Gamescope is on, but the screen resolution is unknown: set it explicitly (gamescope_output = 1920x1200)")?;
    Ok(Some(Plan { size }))
}

impl Plan {
    /// The Gamescope arguments, ending in the `--` separator; the Wine command line follows directly after it. Internal = output: no scaling, no filter.
    pub fn args(&self) -> Vec<String> {
        let n = |v: u32| v.to_string();
        ["-w", &n(self.size.0), "-h", &n(self.size.1), "-W", &n(self.size.0), "-H", &n(self.size.1), "-f", "--"].iter().map(|s| s.to_string()).collect()
    }

    /// Wrapper vector for [`crate::runner::Runner::command_full`]: Gamescope outermost, then the user's own wrapper (for example `gamemoderun`), then Wine.
    pub fn wrapper(&self, gamescope: &Path, user_wrapper: &[String]) -> Vec<String> {
        let mut w = vec![gamescope.to_string_lossy().into_owned()];
        w.extend(self.args());
        w.extend(user_wrapper.iter().cloned());
        w
    }
}

/// Find Gamescope: on `PATH`, and inside a Flatpak sandbox also at the runtime extension's fixed location. `extension_bin` is injectable for tests.
pub fn find_gamescope(path_var: Option<&std::ffi::OsStr>, in_flatpak: bool, extension_bin: &Path) -> Option<PathBuf> {
    crate::doctor::find_program(Path::new("gamescope"), path_var).or_else(|| (in_flatpak && extension_bin.is_file()).then(|| extension_bin.to_path_buf()))
}

/// Are we inside a Flatpak sandbox? (`/.flatpak-info` is the documented marker; `FLATPAK_ID` is set by the runtime.)
pub fn in_flatpak(flatpak_info: &Path, flatpak_id: Option<&std::ffi::OsStr>) -> bool {
    flatpak_info.is_file() || flatpak_id.is_some_and(|v| !v.is_empty())
}

/// Heuristic: is this session already a Gamescope session (SteamOS Game Mode)? Nesting works but is pointless there, so the user is told.
/// UNVERIFIED on a Steam Machine: the variables are the ones Gamescope sessions are known to set.
pub fn already_in_gamescope(var: impl Fn(&str) -> Option<String>) -> bool {
    var("GAMESCOPE_WAYLAND_DISPLAY").is_some_and(|v| !v.is_empty()) || var("XDG_CURRENT_DESKTOP").is_some_and(|v| v.eq_ignore_ascii_case("gamescope"))
}

/// Pre-flight findings for an enabled plan. `gamescope`: what [`find_gamescope`] returned.
pub fn check(plan: Option<&Plan>, gamescope: Option<&Path>, nested: bool, flatpak: bool) -> Vec<Finding> {
    let Some(plan) = plan else { return Vec::new() };
    let mut out = Vec::new();
    match gamescope {
        None => {
            let hint = if flatpak {
                "Install the Flatpak extension org.freedesktop.Platform.VulkanLayer.gamescope (matching the runtime branch), or switch Gamescope off: the normal start needs neither."
            } else {
                "Install the gamescope package of the distribution (SteamOS and the Steam Machine ship it), or switch Gamescope off: the normal start needs neither."
            };
            out.push(Finding::bad(Level::Fail, "Gamescope is switched on, but was not found".into(), hint).coded("gamescope.missing", [if flatpak { "flatpak" } else { "host" }]));
        }
        Some(p) => out.push(
            Finding::ok(format!("Gamescope found ({}), {}x{}", p.display(), plan.size.0, plan.size.1)).coded("gamescope.ready", [p.display().to_string(), format!("{}x{}", plan.size.0, plan.size.1)]),
        ),
    }
    if nested {
        out.push(
            Finding::bad(Level::Warn, "this session already runs inside gamescope (Game Mode)".into(), "Start the game without Gamescope here: Game Mode already provides it.")
                .coded("gamescope.nested", Vec::<String>::new()),
        );
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn off_is_the_default_and_produces_no_plan() {
        assert_eq!(plan(false, Some((1920, 1200)), Some((1920, 1200))).unwrap(), None);
    }

    #[test]
    fn internal_and_output_are_identical_so_the_picture_is_not_scaled() {
        let p = plan(true, None, Some((1920, 1200))).unwrap().unwrap();
        assert_eq!(p.args(), ["-w", "1920", "-h", "1200", "-W", "1920", "-H", "1200", "-f", "--"]);
        assert!(!p.args().iter().any(|a| a == "-F" || a == "fsr" || a == "-U" || a == "--sharpness"), "no filter, no FSR");
        assert_eq!(plan(true, Some((2560, 1440)), Some((1920, 1200))).unwrap().unwrap().size, (2560, 1440), "an explicit output wins");
        assert!(plan(true, None, None).is_err(), "unknown screen size: ask for it instead of guessing");
    }

    #[test]
    fn the_wrapper_puts_gamescope_first_then_the_users_wrapper() {
        let p = plan(true, None, Some((1920, 1200))).unwrap().unwrap();
        let w = p.wrapper(Path::new("/usr/bin/gamescope"), &["gamemoderun".to_string()]);
        assert_eq!(w[0], "/usr/bin/gamescope");
        assert_eq!(&w[w.len() - 2..], ["--", "gamemoderun"]);
    }

    #[test]
    fn findings_name_the_missing_program_and_say_that_the_normal_start_needs_neither() {
        let p = plan(true, None, Some((1920, 1200))).unwrap();
        let f = check(p.as_ref(), None, false, false);
        let txt = format!("{f:?}");
        assert!(f.len() == 1 && txt.contains("Gamescope is switched on") && !txt.contains("FSR"), "{txt}");
        assert!(check(p.as_ref(), Some(Path::new("/usr/bin/gamescope")), true, false).len() == 2, "found + nested warning");
        assert!(check(None, None, true, false).is_empty(), "off: no findings at all");
    }

    #[test]
    fn gamescope_is_found_on_the_path_or_at_the_flatpak_extension() {
        let d = std::env::temp_dir().join(format!("tsw-gs-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        let ext = d.join("gamescope-ext");
        std::fs::write(&ext, "x").unwrap();
        assert_eq!(find_gamescope(Some(d.as_os_str()), true, &ext), Some(ext.clone()));
        assert_eq!(find_gamescope(Some(d.as_os_str()), false, &ext), None);
        assert!(in_flatpak(&d.join("none"), Some("org.x".as_ref())) && !in_flatpak(&d.join("none"), None));
        assert!(already_in_gamescope(|k| (k == "XDG_CURRENT_DESKTOP").then(|| "gamescope".to_string())) && !already_in_gamescope(|_| None));
        let _ = std::fs::remove_dir_all(&d);
    }
}
