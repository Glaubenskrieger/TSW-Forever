//! Guards for the one place where the launcher reaches into the winit window (see `pin_window_limits_for_scale_override` in main.rs): Slint 1.18 leaves the fixed
//! window with stale limits when `SLINT_SCALE_FACTOR` overrides the scale. The workaround must stay small, Linux only and limited to the window's min/max size.
//! These are source-level checks, so a later change that widens the use of the unstable winit interface fails here first.

use std::sync::LazyLock;

/// The sources are compared with `\n` line ends: a Windows checkout with `core.autocrlf` has `\r\n`, which would hide every pattern that spans a line break.
fn lf(text: &str) -> String {
    text.replace("\r\n", "\n")
}

static MAIN: LazyLock<String> = LazyLock::new(|| lf(include_str!("../src/main.rs")));
static MANIFEST: LazyLock<String> = LazyLock::new(|| lf(include_str!("../Cargo.toml")));
static LOCK: LazyLock<String> = LazyLock::new(|| lf(include_str!("../Cargo.lock")));

fn the_workaround() -> String {
    let start = MAIN.find("fn pin_window_limits_for_scale_override").expect("the workaround exists");
    let rest = &MAIN[start..];
    rest[..rest.find("\n}\n").expect("the function ends") + 3].to_string()
}

#[test]
fn the_winit_access_is_used_only_by_the_workaround_and_only_for_the_size_limits() {
    let body = the_workaround();
    assert_eq!(MAIN.matches("with_winit_window").count(), 1, "no second use of the winit window anywhere in the launcher");
    assert_eq!(MAIN.matches("winit_030").count(), 1, "the unstable interface is imported once, inside the workaround");
    assert!(body.contains("set_min_inner_size") && body.contains("set_max_inner_size"));
    for forbidden in ["set_inner_size", "request_inner_size", "set_resizable", "set_fullscreen", "set_maximized", "set_outer_position", "set_decorations", "set_cursor"] {
        assert!(!body.contains(forbidden), "the workaround must not touch {forbidden}");
    }
    // minimum and maximum are the SAME size: the window stays fixed
    assert!(body.contains("let size = winit::dpi::PhysicalSize::new(w, h);") && body.contains("set_min_inner_size(Some(size))") && body.contains("set_max_inner_size(Some(size))"));
    // it does nothing unless the scale is overridden by the environment, so a normal start is not touched
    assert!(body.contains("model::scale_override(std::env::var(\"SLINT_SCALE_FACTOR\")"), "{body}");
}

#[test]
fn the_workaround_exists_and_is_called_on_linux_only() {
    for name in ["fn pin_window_limits_for_scale_override", "fn keep_window_limits_pinned"] {
        let at = MAIN.find(name).unwrap();
        assert!(MAIN[..at].trim_end().lines().last().is_some_and(|l| l.trim() == "#[cfg(target_os = \"linux\")]"), "{name} is Linux only");
    }
    // the only caller of the pin is the timer; the timer is started in one Linux-only statement and only when the scale is overridden
    assert_eq!(MAIN.matches("pin_window_limits_for_scale_override(&ui)").count(), 1, "called from the timer only");
    assert_eq!(MAIN.matches("keep_window_limits_pinned(&ui)").count(), 1, "started once");
    let call = MAIN.find("keep_window_limits_pinned(&ui)").unwrap();
    assert!(
        MAIN[..call].trim_end().ends_with("#[cfg(target_os = \"linux\")]\n    let _limits_timer = ") || MAIN[call - 120..call].contains("#[cfg(target_os = \"linux\")]"),
        "the start is Linux only"
    );
    let keeper = &MAIN[MAIN.find("fn keep_window_limits_pinned").unwrap()..];
    let keeper = &keeper[..keeper.find("\n}\n").unwrap()];
    assert!(keeper.contains("model::scale_override(std::env::var(\"SLINT_SCALE_FACTOR\")") && keeper.contains("?;"), "no timer at all without the override");
    assert!(keeper.contains("ticks.get() >= 50"), "the timer is bounded");
}

#[test]
fn the_unstable_slint_feature_is_a_linux_only_dependency_and_adds_no_package() {
    let linux = MANIFEST.split("[target.'cfg(target_os = \"linux\")'.dependencies]").nth(1).expect("a Linux-only dependency section");
    let section = linux.split("\n[").next().unwrap();
    assert!(section.contains("unstable-winit-030"), "{section}");
    assert!(!MANIFEST.split("[target.").next().unwrap().contains("unstable-winit-030"), "the common dependency list does not carry the feature");
    // winit and the Slint winit backend are in the lockfile already (the backend uses them); the feature only adds the edge from `slint`
    assert!(LOCK.contains("name = \"winit\"\nversion = \"0.30.") && LOCK.contains("name = \"i-slint-backend-winit\""));
}
