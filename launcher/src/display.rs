//! Native resolution of the active monitor, read from the kernel's DRM entries (`/sys/class/drm`), without any display
//! library. Used only by the legacy optional Gamescope path; normal starts do not overwrite the game's saved resolution.
//!
//! Limitation: with several active monitors the first one in directory order is used; the preferred (first listed)
//! mode of a connected, enabled connector is taken. A manual `resolution` in the configuration overrides this.

use std::path::Path;

/// `1920x1200` -> (1920, 1200)
fn parse_mode(line: &str) -> Option<(u32, u32)> {
    let (w, h) = line.trim().split_once('x')?;
    let h = h.trim_end_matches(|c: char| !c.is_ascii_digit());
    Some((w.parse().ok()?, h.parse().ok()?)).filter(|&(w, h)| w >= 640 && h >= 480)
}

/// Preferred mode of the first connected and enabled connector below `drm_root`.
pub fn primary_mode(drm_root: &Path) -> Option<(u32, u32)> {
    let mut entries: Vec<_> = std::fs::read_dir(drm_root).ok()?.flatten().map(|e| e.path()).collect();
    entries.sort();
    for dir in entries {
        let read = |f: &str| std::fs::read_to_string(dir.join(f)).unwrap_or_default();
        if read("status").trim() == "connected" && read("enabled").trim() == "enabled" {
            if let Some(mode) = read("modes").lines().find_map(parse_mode) {
                return Some(mode);
            }
        }
    }
    None
}

/// [`primary_mode`] for the running system.
pub fn system_primary_mode() -> Option<(u32, u32)> {
    primary_mode(Path::new("/sys/class/drm"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn connector(root: &Path, name: &str, status: &str, enabled: &str, modes: &str) {
        let d = root.join(name);
        std::fs::create_dir_all(&d).unwrap();
        std::fs::write(d.join("status"), format!("{status}\n")).unwrap();
        std::fs::write(d.join("enabled"), format!("{enabled}\n")).unwrap();
        std::fs::write(d.join("modes"), modes).unwrap();
    }

    #[test]
    fn the_first_connected_enabled_connector_decides_and_the_first_mode_is_preferred() {
        let d = std::env::temp_dir().join(format!("tsw-drm-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        connector(&d, "card1-DP-1", "disconnected", "disabled", "1024x768\n");
        connector(&d, "card1-DP-2", "connected", "disabled", "1280x720\n");
        connector(&d, "card1-eDP-1", "connected", "enabled", "1920x1200\n1920x1080\n");
        assert_eq!(primary_mode(&d), Some((1920, 1200)));
        assert_eq!(primary_mode(&d.join("missing")), None);
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn mode_lines_with_refresh_suffix_or_junk_are_handled() {
        assert_eq!(parse_mode("2560x1440\n"), Some((2560, 1440)));
        assert_eq!(parse_mode("1920x1080i"), Some((1920, 1080)));
        assert_eq!(parse_mode("tiny"), None);
        assert_eq!(parse_mode("320x200"), None);
    }
}

/// Valid display sizes, deduplicated and sorted by descending area, then width/height.
pub fn normalize(modes: impl IntoIterator<Item = (u32, u32)>) -> Vec<(u32, u32)> {
    let mut modes: Vec<_> = modes.into_iter().filter(|&(w, h)| (640..=16384).contains(&w) && (480..=16384).contains(&h)).collect();
    modes.sort_unstable_by_key(|&(w, h)| (std::cmp::Reverse(u64::from(w) * u64::from(h)), std::cmp::Reverse(w), std::cmp::Reverse(h)));
    modes.dedup();
    modes
}

/// Read-only query of the display server used by Wine (X11, XWayland or Steam's Gamescope).
/// No output configuration or mode-setting request is issued. No external tools or host escape are needed.
#[cfg(target_os = "linux")]
fn environment_modes() -> Option<Vec<(u32, u32)>> {
    use x11rb::connection::Connection;
    use x11rb::protocol::randr::{Connection as OutputConnection, ConnectionExt};
    let (conn, screen) = x11rb::connect(None).ok()?;
    let root = conn.setup().roots.get(screen)?.root;
    let resources = conn.randr_get_screen_resources_current(root).ok()?.reply().ok()?;
    let mut sizes = Vec::new();
    for output in &resources.outputs {
        let info = conn.randr_get_output_info(*output, resources.config_timestamp).ok()?.reply().ok()?;
        if info.connection != OutputConnection::CONNECTED || info.crtc == 0 {
            continue;
        }
        for id in info.modes {
            if let Some(mode) = resources.modes.iter().find(|m| m.id == id) {
                sizes.push((u32::from(mode.width), u32::from(mode.height)));
            }
        }
    }
    Some(normalize(sizes))
}

#[cfg(not(target_os = "linux"))]
fn environment_modes() -> Option<Vec<(u32, u32)>> {
    None
}

/// Bounded discovery: a missing/unresponsive display server must never block the UI or a launch indefinitely.
/// No DRM fallback: physical connector modes can differ from the modes exposed inside a nested compositor.
pub fn available_modes() -> Vec<(u32, u32)> {
    let (tx, rx) = std::sync::mpsc::channel();
    let _ = std::thread::Builder::new().name("display-modes".into()).spawn(move || {
        let _ = tx.send(environment_modes().unwrap_or_default());
    });
    rx.recv_timeout(std::time::Duration::from_secs(2)).unwrap_or_default()
}

/// Validate an explicitly saved choice on every launch. Missing modes never silently become another resolution.
pub fn game_desktop(selected: Option<(u32, u32)>, modes: &[(u32, u32)], wine: bool, gamescope: bool) -> Result<Option<(u32, u32)>, String> {
    let Some(size) = selected else {
        return Ok(None);
    };
    if !wine {
        return Err("Game resolution control requires a Wine/Proton runner".into());
    }
    if gamescope {
        return Err("Game resolution control cannot be combined with the legacy gamescope option; disable gamescope in the launcher configuration".into());
    }
    if !modes.contains(&size) {
        return Err(format!("Saved game resolution {}x{} is unavailable in the current display environment; choose an available mode or Managed by the game", size.0, size.1));
    }
    Ok(Some(size))
}

#[cfg(test)]
mod resolution_tests {
    use super::*;
    #[test]
    fn modes_are_valid_unique_and_sorted_without_inventing_resolutions() {
        assert_eq!(normalize([(1920, 1080), (0, 0), (1920, 1080), (3840, 2160), (1280, 720), (20000, 1080)]), [(3840, 2160), (1920, 1080), (1280, 720)]);
        assert!(normalize([]).is_empty());
    }
    #[test]
    fn default_never_requests_a_desktop_and_missing_choices_fail_explicitly() {
        assert_eq!(game_desktop(None, &[], true, false).unwrap(), None);
        assert_eq!(game_desktop(Some((1920, 1080)), &[(1920, 1080)], true, false).unwrap(), Some((1920, 1080)));
        assert!(game_desktop(Some((1920, 1080)), &[], true, false).is_err());
        assert!(game_desktop(Some((1920, 1080)), &[(1920, 1080)], false, false).is_err());
        assert!(game_desktop(Some((1920, 1080)), &[(1920, 1080)], true, true).is_err());
    }
}

/// Selection precedence without a monitor-size fallback. Legacy explicit options retain their documented semantics.
pub fn launch_resolution(game: Option<(u32, u32)>, gamescope: Option<(u32, u32)>, explicit: Option<(u32, u32)>, desktop: Option<(u32, u32)>) -> Option<(u32, u32)> {
    game.or(gamescope).or(explicit).or(desktop)
}

#[cfg(test)]
mod persistence_tests {
    use super::*;
    #[test]
    fn managed_default_does_not_overwrite_preferences_and_manual_choice_round_trips() {
        assert_eq!(launch_resolution(None, None, None, None), None);
        let c = crate::config::parse("game_resolution = 1920x1080\nfps_limit = 144\nenv.KEEP = yes\n").unwrap();
        let back = crate::config::parse(&crate::config::to_text(&c)).unwrap();
        assert_eq!(back, c);
        let size = game_desktop(back.game_resolution, &[(1920, 1080)], true, false).unwrap();
        assert_eq!(launch_resolution(size, None, Some((3840, 2160)), None), Some((1920, 1080)));
        let c = crate::runner::Runner::Wine { wine: "wine".into(), prefix: Some("/fixture/prefix".into()) }.command_full(
            &[],
            &["explorer".into(), "/desktop=TSW,1920x1080".into()],
            std::path::Path::new("/fixture/TheSecretWorldDX11.exe"),
            &[],
        );
        assert_eq!(c.get_program(), "wine");
        assert_eq!(c.get_args().map(|a| a.to_str().unwrap()).collect::<Vec<_>>(), ["explorer", "/desktop=TSW,1920x1080", "/fixture/TheSecretWorldDX11.exe"]);
    }
}
