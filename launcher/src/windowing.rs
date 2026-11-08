//! Window-manager decorations of the game window: the Alt+Tab fix for exclusive full screen (2026-10-31).
//!
//! **Why this exists.** The client leaves exclusive full screen itself when it loses the focus (its `WM_ACTIVATEAPP(FALSE)` handler calls
//! `IDXGISwapChain::SetFullscreenState(FALSE)` and `ShowWindow(SW_MINIMIZE)`) and, when it is activated again, restores the window
//! and queues a request "back to full screen" in one shared settings block. The same block is also written by its `WM_SIZE` handler
//! (windowed resize). The window the client creates is 0x0, so after DXVK left full screen it is only 119x34 (client area 113x2). A window
//! manager that decorates it (KWin) cannot keep such a small client area, enlarges the window, Wine adopts the new size and the client gets
//! a second, asynchronous `WM_SIZE` after the activation: that overwrites the full-screen request with a windowed resize, the window stays tiny.
//! Without window-manager decorations the requested size is honoured, no second `WM_SIZE` arrives and the client re-enters full screen.
//! Measured with scripts/alttab_auto.py: 7 of 7 Alt+Tab cycles ended in the tiny window without the variable, 13 of 13 returned to full
//! screen with it (documentation/architecture.md).
//!
//! **What it does.** For Wine/Proton runners the launcher sets `WINE_NO_WM_DECORATION=1` in the game's environment (a GE-Proton switch:
//! `patches/proton/0001-win32u-add-env-switch-to-disable-wm-decorations.patch`, the same as the Wine setting `Decorated=N`). Only the
//! game process is affected; no prefix, registry or game file is changed.

use crate::prefs::DisplayMode;

pub const ENV: &str = "WINE_NO_WM_DECORATION";

/// Config key `wm_decorations`, option `--wm-decorations`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum WmDecorations {
    /// No decorations for the full-screen game (the fix), decorations stay for a windowed game, a virtual desktop and non-Wine runners.
    #[default]
    Auto,
    /// Never touch it: the window manager decorates the game windows.
    On,
    /// Always set the variable (also for a windowed game).
    Off,
}

impl WmDecorations {
    pub fn parse(v: &str) -> Result<WmDecorations, String> {
        match v.trim().to_ascii_lowercase().as_str() {
            "auto" => Ok(WmDecorations::Auto),
            "on" | "true" | "yes" => Ok(WmDecorations::On),
            "off" | "false" | "no" => Ok(WmDecorations::Off),
            _ => Err("wm_decorations must be auto, on or off".into()),
        }
    }
    pub fn text(self) -> &'static str {
        match self {
            WmDecorations::Auto => "auto",
            WmDecorations::On => "on",
            WmDecorations::Off => "off",
        }
    }
}

/// Facts the decision needs.
#[derive(Debug, Clone, Copy)]
pub struct Context {
    /// The game starts through a Wine/Proton runner (the variable means nothing for a native Windows start).
    pub wine: bool,
    /// Wine's virtual desktop is used (its windows are not decorated by the window manager anyway).
    pub virtual_desktop: bool,
    /// The display mode the client will use: the explicit `display_mode` option, else the client's saved preference; `None` = unknown.
    pub display_mode: Option<DisplayMode>,
    /// The user (configuration `env.`, `--env` or the process environment) already defines the variable: never overridden.
    pub user_defined: bool,
}

/// The variable to add to the game's environment, if any.
pub fn env_to_set(mode: WmDecorations, c: Context) -> Option<(&'static str, &'static str)> {
    if !c.wine || c.user_defined {
        return None;
    }
    let set = match mode {
        WmDecorations::On => false,
        WmDecorations::Off => true,
        WmDecorations::Auto => !c.virtual_desktop && c.display_mode != Some(DisplayMode::Windowed),
    };
    set.then_some((ENV, "1"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ctx() -> Context {
        Context { wine: true, virtual_desktop: false, display_mode: None, user_defined: false }
    }

    #[test]
    fn the_default_removes_the_decorations_of_a_full_screen_wine_game() {
        assert_eq!(env_to_set(WmDecorations::default(), ctx()), Some(("WINE_NO_WM_DECORATION", "1")));
        for m in [None, Some(DisplayMode::Fullscreen), Some(DisplayMode::Borderless)] {
            assert!(env_to_set(WmDecorations::Auto, Context { display_mode: m, ..ctx() }).is_some(), "{m:?}");
        }
    }

    #[test]
    fn auto_leaves_windowed_games_virtual_desktops_and_native_starts_alone() {
        assert_eq!(env_to_set(WmDecorations::Auto, Context { display_mode: Some(DisplayMode::Windowed), ..ctx() }), None);
        assert_eq!(env_to_set(WmDecorations::Auto, Context { virtual_desktop: true, ..ctx() }), None);
        assert_eq!(env_to_set(WmDecorations::Auto, Context { wine: false, ..ctx() }), None);
    }

    #[test]
    fn on_and_off_are_explicit_and_a_user_defined_variable_always_wins() {
        assert_eq!(env_to_set(WmDecorations::On, ctx()), None);
        assert!(env_to_set(WmDecorations::Off, Context { display_mode: Some(DisplayMode::Windowed), ..ctx() }).is_some());
        assert_eq!(env_to_set(WmDecorations::Off, Context { wine: false, ..ctx() }), None);
        for m in [WmDecorations::Auto, WmDecorations::On, WmDecorations::Off] {
            assert_eq!(env_to_set(m, Context { user_defined: true, ..ctx() }), None);
        }
    }

    #[test]
    fn the_option_text_round_trips_and_garbage_is_refused() {
        for m in [WmDecorations::Auto, WmDecorations::On, WmDecorations::Off] {
            assert_eq!(WmDecorations::parse(m.text()), Ok(m));
        }
        assert_eq!(WmDecorations::parse(" OFF "), Ok(WmDecorations::Off));
        assert!(WmDecorations::parse("maybe").is_err() && WmDecorations::parse("").is_err());
    }
}
