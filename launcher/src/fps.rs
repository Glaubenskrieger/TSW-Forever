//! Frame-rate limit for the started game process.
//!
//! The runtime's DXVK (v3.x; checked: `dxgi.dll`/`d3d11.dll` of GE-Proton11-11 contain `dxvk.maxFrameRate`, `dxgi.maxFrameRate`,
//! `DXVK_CONFIG`, `DXVK_CONFIG_FILE`, and no `DXVK_FRAME_RATE`) no longer reads `DXVK_FRAME_RATE`. The limit is therefore handed over as DXVK's inline
//! configuration `DXVK_CONFIG="dxvk.maxFrameRate = N; dxgi.maxFrameRate = N"` in the environment of the game process only:
//! no file is written, no existing `dxvk.conf` is touched, the client executable is not touched. Both option names are set because
//! DXVK 3 renamed the option and an older name is still accepted; an unknown name is ignored by DXVK. An already present `DXVK_CONFIG`
//! of the user is kept; ours is appended and therefore wins for these two options only.
//! Whether the limit really takes effect is verified in the game (DXVK HUD, `fps_hud`), not assumed.

pub const MIN_FPS: u32 = 30;
pub const MAX_FPS: u32 = 240;
pub const DEFAULT_FPS: u32 = 60;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FpsLimit {
    Cap(u32),
    Unlimited,
}

impl Default for FpsLimit {
    /// 60 FPS is only the default; every value from [`MIN_FPS`] to [`MAX_FPS`] and "unlimited" can be chosen.
    fn default() -> Self {
        FpsLimit::Cap(DEFAULT_FPS)
    }
}

/// `unlimited` (also `off`, `0`) or a whole number from 30 to 240.
pub fn parse(v: &str) -> Result<FpsLimit, String> {
    let v = v.trim();
    if v.eq_ignore_ascii_case("unlimited") || v.eq_ignore_ascii_case("off") || v == "0" {
        return Ok(FpsLimit::Unlimited);
    }
    match v.parse::<u32>() {
        Ok(n) if (MIN_FPS..=MAX_FPS).contains(&n) => Ok(FpsLimit::Cap(n)),
        _ => Err(format!("fps_limit must be unlimited or a whole number from {MIN_FPS} to {MAX_FPS}")),
    }
}

impl FpsLimit {
    pub fn text(self) -> String {
        match self {
            FpsLimit::Cap(n) => n.to_string(),
            FpsLimit::Unlimited => "unlimited".into(),
        }
    }

    /// The two DXVK options. `0` is DXVK's "no limit"; it is set explicitly so that an explicit "unlimited" also beats a limit in a user's file.
    pub fn dxvk_options(self) -> String {
        let n = match self {
            FpsLimit::Cap(n) => n,
            FpsLimit::Unlimited => 0,
        };
        format!("dxvk.maxFrameRate = {n}; dxgi.maxFrameRate = {n}")
    }
}

/// `existing` (the user's own `DXVK_CONFIG`, if any) first, ours last.
pub fn merge_dxvk_config(existing: Option<&str>, ours: &str) -> String {
    match existing.map(str::trim).filter(|e| !e.is_empty()) {
        Some(e) => format!("{};{ours}", e.trim_end_matches(';')),
        None => ours.to_string(),
    }
}

/// DXVK HUD value with the `fps` item added (user's own items are kept).
pub fn merge_hud(existing: Option<&str>) -> String {
    match existing.map(str::trim).filter(|e| !e.is_empty()) {
        Some(e) if e.split(',').any(|i| i.trim() == "fps" || i.trim() == "full") => e.to_string(),
        Some(e) => format!("{e},fps"),
        None => "fps".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_default_is_60_and_the_range_is_free_up_to_240_plus_unlimited() {
        assert_eq!(FpsLimit::default(), FpsLimit::Cap(60));
        for ok in ["30", "60", "75", "144", "240", " 90 "] {
            assert!(matches!(parse(ok), Ok(FpsLimit::Cap(_))), "{ok}");
        }
        for u in ["unlimited", "UNLIMITED", "off", "0"] {
            assert_eq!(parse(u), Ok(FpsLimit::Unlimited));
        }
        for bad in ["", "29", "241", "-5", "60fps", "1e2", "sixty", "4294967296"] {
            assert!(parse(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn the_dxvk_options_use_the_v3_name_and_the_old_alias_and_never_the_removed_variable() {
        assert_eq!(FpsLimit::Cap(60).dxvk_options(), "dxvk.maxFrameRate = 60; dxgi.maxFrameRate = 60");
        assert_eq!(FpsLimit::Unlimited.dxvk_options(), "dxvk.maxFrameRate = 0; dxgi.maxFrameRate = 0");
        assert!(!FpsLimit::Cap(60).dxvk_options().contains("DXVK_FRAME_RATE"));
        assert_eq!(FpsLimit::Cap(144).text(), "144");
        assert_eq!(FpsLimit::Unlimited.text(), "unlimited");
    }

    #[test]
    fn a_users_own_dxvk_config_and_hud_items_are_kept() {
        assert_eq!(merge_dxvk_config(None, "a = 1"), "a = 1");
        assert_eq!(merge_dxvk_config(Some("  "), "a = 1"), "a = 1");
        assert_eq!(merge_dxvk_config(Some("dxgi.hideAmdGpu = True;"), "a = 1"), "dxgi.hideAmdGpu = True;a = 1");
        assert_eq!(merge_hud(None), "fps");
        assert_eq!(merge_hud(Some("devinfo,memory")), "devinfo,memory,fps");
        assert_eq!(merge_hud(Some("full")), "full");
        assert_eq!(merge_hud(Some("fps,memory")), "fps,memory");
    }
}
