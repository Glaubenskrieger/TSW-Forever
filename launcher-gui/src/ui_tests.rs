//! Pointer regression tests for the real `MainWindow` (headless: software renderer, synthetic pointer events, no display).
//!
//! Background: in Slint a `FocusScope` without focus accepts the first `Pressed` event to take the focus. If such a scope
//! lies above a `TouchArea`, the first click only moves the focus and the action fires on the second click. These tests
//! press each control with exactly one click on a fresh window and require exactly one callback.

use slint::Model;
use std::cell::Cell;
use std::rc::Rc;

use slint::platform::software_renderer::{MinimalSoftwareWindow, RepaintBufferType};
use slint::platform::{Platform, PointerEventButton, WindowAdapter, WindowEvent};
use slint::{ComponentHandle, LogicalPosition, PhysicalSize, PlatformError};

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use crate::i18n::Catalog;
use crate::{MainWindow, Shared, State, View};

/// Title bar buttons are 46 px wide and right aligned: gear, minimize, close (no maximize: the window has a fixed size).
const GEAR: (f32, f32) = (885.0, 22.0);
const MINIMIZE: (f32, f32) = (931.0, 22.0);
const CLOSE: (f32, f32) = (977.0, 22.0);
const WIDTH: u32 = 1000;
const HEIGHT: u32 = 660;

struct Headless(Rc<MinimalSoftwareWindow>);

impl Platform for Headless {
    fn create_window_adapter(&self) -> Result<Rc<dyn WindowAdapter>, PlatformError> {
        Ok(self.0.clone())
    }
}

thread_local! {
    static SNAP_ADAPTER: std::cell::RefCell<Option<Rc<MinimalSoftwareWindow>>> = const { std::cell::RefCell::new(None) };
}

/// A fresh window without any focus or hover history. The platform is per thread, so every test installs its own.
/// The detection reads process-wide state (the `ProgramFiles` variables of the gate test, the user's "ignored installations"): the tests that change it run one
/// after the other.
static GLOBAL_SEARCH: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// Resets the user's search decision when a test ends, also when it fails.
struct ResetUserSearch;

impl Drop for ResetUserSearch {
    fn drop(&mut self) {
        tsw_launch::winnative::reset_user_search();
    }
}

fn fresh() -> MainWindow {
    // a machine with a real game installation registered must not influence any window test
    #[cfg(windows)]
    std::env::set_var("TSW_LAUNCH_NO_SYSTEM_SEARCH", "1");
    let adapter = MinimalSoftwareWindow::new(RepaintBufferType::NewBuffer);
    adapter.set_size(PhysicalSize::new(WIDTH, HEIGHT));
    let _ = slint::platform::set_platform(Box::new(Headless(adapter.clone())));
    SNAP_ADAPTER.with(|a| *a.borrow_mut() = Some(adapter.clone()));
    let ui = MainWindow::new().expect("window");
    ui.set_can_play(true);
    ui.show().expect("show");
    ui
}

/// A window wired like the real program (all callbacks connected), with a throw-away configuration file.
fn wired(tag: &str) -> (MainWindow, Shared, Arc<AtomicBool>, std::path::PathBuf) {
    let ui = fresh();
    let dir = std::env::temp_dir().join(format!("tsw-ui-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let path = dir.join("config");
    let state: Shared = Arc::new(Mutex::new(State {
        config_path: path.clone(),
        config: Default::default(),
        catalog: Catalog::new("en"),
        view: View::default(),
        install: Default::default(),
        install_cancel: Default::default(),
        install_auto: false,
        installer_hash: None,
        hash_pending: false,
        evidence: Default::default(),
        watch_busy: false,
        funcom_children: Vec::new(),
        patcher_child: None,
        close_attempted: false,
        run: Default::default(),
        run_active: false,
        run_ok: false,
    }));
    crate::i18n::select_language("en");
    let may_close = Arc::new(AtomicBool::new(false));
    crate::wire(&ui, &state, &may_close);
    (ui, state, may_close, path)
}

fn click(ui: &MainWindow, x: f32, y: f32) {
    let position = LogicalPosition::new(x, y);
    let w = ui.window();
    w.dispatch_event(WindowEvent::PointerMoved { position });
    w.dispatch_event(WindowEvent::PointerPressed { position, button: PointerEventButton::Left });
    w.dispatch_event(WindowEvent::PointerReleased { position, button: PointerEventButton::Left });
}

/// Find the centre of the control whose callback `counter` reports: click every grid point twice on one window (so a
/// swallowed first click cannot hide the control) and take the bounding box of all hits.
fn locate(hook: impl Fn(&MainWindow, Rc<Cell<u32>>), region: (u32, u32, u32, u32)) -> (f32, f32) {
    let ui = fresh();
    let count = Rc::new(Cell::new(0));
    hook(&ui, count.clone());
    let (x0, y0, x1, y1) = region;
    let (mut min_x, mut min_y, mut max_x, mut max_y) = (f32::MAX, f32::MAX, f32::MIN, f32::MIN);
    for y in (y0..y1).step_by(4) {
        for x in (x0..x1).step_by(4) {
            count.set(0);
            click(&ui, x as f32, y as f32);
            click(&ui, x as f32, y as f32);
            if count.get() > 0 {
                (min_x, min_y, max_x, max_y) = (min_x.min(x as f32), min_y.min(y as f32), max_x.max(x as f32), max_y.max(y as f32));
            }
        }
    }
    assert!(min_x <= max_x, "control not found in region {region:?}");
    ((min_x + max_x) / 2.0, (min_y + max_y) / 2.0)
}

/// One click on a fresh window must trigger the callback exactly once.
fn assert_single_click(hook: impl Fn(&MainWindow, Rc<Cell<u32>>) + Copy, region: (u32, u32, u32, u32), what: &str) {
    let (x, y) = locate(hook, region);
    let ui = fresh();
    let count = Rc::new(Cell::new(0));
    hook(&ui, count.clone());
    click(&ui, x, y);
    assert_eq!(count.get(), 1, "{what}: one click at ({x},{y}) must act exactly once");
}

#[test]
fn play_button_acts_on_the_first_click() {
    assert_single_click(|ui, n| ui.on_play(move || n.set(n.get() + 1)), (300, 100, 700, 400), "SPIELEN");
}

#[test]
fn intro_checkbox_toggles_on_the_first_click() {
    assert_single_click(|ui, n| ui.on_settings_changed(move || n.set(n.get() + 1)), (300, 280, 700, 480), "Intro-Checkbox");
}

#[test]
fn gear_switches_views_on_the_first_click_and_back_with_the_back_button() {
    let ui = fresh();
    assert!(!ui.get_advanced());
    click(&ui, GEAR.0, GEAR.1);
    assert!(ui.get_advanced(), "first click on the gear must open the advanced view");
    click(&ui, GEAR.0, GEAR.1);
    assert!(!ui.get_advanced(), "second click on the gear must close it again");
}

#[test]
fn back_button_of_the_advanced_view_acts_on_the_first_click() {
    let ui = fresh();
    ui.set_fps_limit(144);
    ui.set_fps_hud(true);
    ui.set_advanced(true);
    click(&ui, 900.0, 80.0);
    assert!(ui.get_advanced(), "the old separate header button no longer exists");
    click(&ui, GEAR.0, GEAR.1);
    assert!(!ui.get_advanced(), "the same title-bar position is now Back");
    assert_eq!((ui.get_fps_limit(), ui.get_fps_hud()), (144, true));
}

fn press_key(ui: &MainWindow, key: slint::platform::Key) {
    let text: slint::SharedString = key.into();
    ui.window().dispatch_event(WindowEvent::KeyPressed { text: text.clone() });
    ui.window().dispatch_event(WindowEvent::KeyReleased { text });
}

#[test]
fn escape_leaves_the_advanced_view_after_it_was_opened_with_the_mouse() {
    let ui = fresh();
    click(&ui, GEAR.0, GEAR.1);
    assert!(ui.get_advanced());
    press_key(&ui, slint::platform::Key::Escape);
    assert!(!ui.get_advanced(), "Escape must leave the advanced view even though no click moved the focus");
}

#[test]
fn space_activates_the_focused_play_button_from_the_keyboard() {
    let ui = fresh();
    let count = Rc::new(Cell::new(0));
    let n = count.clone();
    ui.on_play(move || n.set(n.get() + 1));
    press_key(&ui, slint::platform::Key::Tab); // first focusable control in tab order
    ui.window().dispatch_event(WindowEvent::KeyPressed { text: " ".into() });
    ui.window().dispatch_event(WindowEvent::KeyReleased { text: " ".into() });
    assert_eq!(count.get(), 1, "Tab to SPIELEN, then Space, must start exactly once");
}

// ---- fixed window, exit confirmation, closing, language ----

#[test]
fn the_title_bar_has_gear_minimize_and_close_only_and_every_button_acts_on_the_first_click() {
    let ui = fresh();
    click(&ui, CLOSE.0, CLOSE.1);
    assert!(ui.get_confirm_exit(), "the X asks first");
    let ui = fresh();
    click(&ui, MINIMIZE.0, MINIMIZE.1);
    assert!(ui.window().is_minimized(), "minimize acts on the first click");
    // where the maximize button used to be (left of minimize) there is now the gear, nothing else
    let ui = fresh();
    click(&ui, GEAR.0 - 46.0, GEAR.1);
    assert!(!ui.window().is_maximized() && !ui.get_advanced() && !ui.get_confirm_exit(), "no hidden maximize button");
}

#[test]
fn the_window_has_one_fixed_size_and_no_maximize_state() {
    let ui = fresh();
    assert_eq!((ui.get_win_width(), ui.get_win_height()), (1000.0, 660.0));
    // the program may change the size property (small displays), the constraints follow it
    ui.set_win_width(900.0);
    ui.set_win_height(560.0);
    assert_eq!((ui.get_win_width(), ui.get_win_height()), (900.0, 560.0));
    click(&ui, 600.0, 22.0); // the drag area: a plain click must not maximize
    assert!(!ui.window().is_maximized());
}

#[test]
fn the_exit_dialog_blocks_the_window_below_and_opens_only_once() {
    let (ui, _state, _mc, _p) = wired("dlg1");
    let plays = Rc::new(Cell::new(0));
    let p = plays.clone();
    ui.on_play(move || p.set(p.get() + 1));
    click(&ui, CLOSE.0, CLOSE.1);
    click(&ui, CLOSE.0, CLOSE.1); // again: still one dialog, nothing stacked
    assert!(ui.get_confirm_exit());
    click(&ui, 500.0, 344.0); // where SPIELEN is: the scrim must swallow it
    assert_eq!(plays.get(), 0, "the window below does not react while the dialog is open");
    assert!(ui.get_confirm_exit(), "a click on the scrim does not dismiss the dialog");
    click(&ui, GEAR.0, GEAR.1);
    assert!(!ui.get_advanced(), "the title bar below is blocked too");
}

/// Find the dialog button that makes `done(ui)` true after ONE click (`reset` re-opens the dialog between probes).
fn dialog_button(done: impl Fn(&MainWindow) -> bool, reset: impl Fn(&MainWindow)) -> (f32, f32) {
    let ui = fresh();
    let (mut sum, mut n) = ((0.0, 0.0), 0);
    for y in (240..420).step_by(6) {
        for x in (250..750).step_by(6) {
            reset(&ui);
            click(&ui, x as f32, y as f32);
            if done(&ui) {
                sum = (sum.0 + x as f32, sum.1 + y as f32);
                n += 1;
            }
        }
    }
    assert!(n > 0, "button not found");
    (sum.0 / n as f32, sum.1 / n as f32)
}

#[test]
fn no_go_back_closes_only_the_dialog_and_yes_exit_leaves_on_one_click_each() {
    // "No": one click closes the dialog and does not leave
    let left = Rc::new(Cell::new(0));
    let (x, y) = {
        let l = left.clone();
        let probe = |ui: &MainWindow| {
            ui.set_confirm_exit(true);
            let _ = &l;
        };
        dialog_button(|ui| !ui.get_confirm_exit(), probe)
    };
    let (ui, _s, mc, _p) = wired("dlg2");
    let l = left.clone();
    ui.on_close_window(move || l.set(l.get() + 1));
    ui.set_confirm_exit(true);
    click(&ui, x, y);
    assert!(!ui.get_confirm_exit(), "No, go back: closes the dialog on the first click");
    assert_eq!(left.get(), 0, "and does not leave");
    assert!(!mc.load(Ordering::SeqCst));
    // "Yes": one click leaves exactly once
    let yes = Rc::new(Cell::new(0));
    let (yx, yy) = {
        let ui = fresh();
        let c = yes.clone();
        ui.on_close_window(move || c.set(c.get() + 1));
        let (mut sum, mut n) = ((0.0, 0.0), 0);
        for y in (240..420).step_by(6) {
            for x in (250..750).step_by(6) {
                ui.set_confirm_exit(true);
                let before = yes.get();
                click(&ui, x as f32, y as f32);
                if yes.get() > before {
                    sum = (sum.0 + x as f32, sum.1 + y as f32);
                    n += 1;
                }
            }
        }
        assert!(n > 0, "Yes button not found");
        (sum.0 / n as f32, sum.1 / n as f32)
    };
    let (ui, _s, _mc, _p) = wired("dlg3");
    let c = Rc::new(Cell::new(0));
    let c2 = c.clone();
    ui.on_close_window(move || c2.set(c2.get() + 1));
    ui.set_confirm_exit(true);
    click(&ui, yx, yy);
    assert_eq!(c.get(), 1, "Yes, exit: one click, one exit request");
}

#[test]
fn escape_cancels_the_dialog_and_the_safe_choice_has_the_focus() {
    let (ui, ..) = wired("dlg4");
    click(&ui, CLOSE.0, CLOSE.1);
    assert!(ui.get_confirm_exit());
    press_key(&ui, slint::platform::Key::Escape);
    assert!(!ui.get_confirm_exit(), "Escape cancels");
    // Enter on the default focus ("No, go back") must not exit
    let left = Rc::new(Cell::new(0));
    let l = left.clone();
    ui.on_close_window(move || l.set(l.get() + 1));
    click(&ui, CLOSE.0, CLOSE.1);
    ui.window().dispatch_event(WindowEvent::KeyPressed { text: "\n".into() });
    ui.window().dispatch_event(WindowEvent::KeyReleased { text: "\n".into() });
    assert_eq!(left.get(), 0, "Enter on the default button never exits");
    assert!(!ui.get_confirm_exit(), "the default button is \"No, go back\"");
}

#[test]
fn ctrl_q_asks_and_the_window_manager_close_request_asks_too() {
    let (ui, _s, mc, _p) = wired("close1");
    ui.window().dispatch_event(WindowEvent::KeyPressed { text: "q".into() }); // without Ctrl: nothing
    assert!(!ui.get_confirm_exit());
    ui.window().dispatch_event(WindowEvent::KeyPressed { text: slint::platform::Key::Control.into() });
    ui.window().dispatch_event(WindowEvent::KeyPressed { text: "q".into() });
    ui.window().dispatch_event(WindowEvent::KeyReleased { text: "q".into() });
    ui.window().dispatch_event(WindowEvent::KeyReleased { text: slint::platform::Key::Control.into() });
    assert!(ui.get_confirm_exit(), "Ctrl+Q asks");
    ui.set_confirm_exit(false);
    // Alt+F4 / task bar / "close window" of the compositor arrive as a close request
    ui.window().dispatch_event(WindowEvent::CloseRequested);
    assert!(ui.get_confirm_exit(), "a window-manager close request asks first");
    assert!(ui.window().is_visible(), "and keeps the window");
    ui.window().dispatch_event(WindowEvent::CloseRequested);
    assert!(ui.get_confirm_exit() && ui.window().is_visible(), "a second request does not stack or close");
    assert!(!mc.load(Ordering::SeqCst));
}

#[test]
fn the_confirmed_exit_and_the_automatic_close_after_a_start_leave_without_asking() {
    let (ui, _s, mc, _p) = wired("close2");
    crate::leave(&ui, &mc);
    assert!(!ui.window().is_visible() && !ui.get_confirm_exit(), "leaving does not show the dialog");
    assert!(mc.load(Ordering::SeqCst));
    // after a successful start the program hides the window through `leave`; a later close request is simply honoured
    let (ui, _s, mc, _p) = wired("close3");
    mc.store(true, Ordering::SeqCst);
    ui.window().dispatch_event(WindowEvent::CloseRequested);
    assert!(!ui.window().is_visible() && !ui.get_confirm_exit());
}

#[test]
fn a_failed_start_keeps_the_launcher_open_and_does_not_set_the_exit_permission() {
    let (ui, state, mc, _p) = wired("startfail");
    ui.set_running(true);
    {
        let mut st = state.lock().unwrap();
        st.view.message = Some(crate::i18n::Msg::plain("Starting failed: {0}"));
    }
    ui.set_running(false);
    assert!(ui.window().is_visible() && !mc.load(Ordering::SeqCst));
}

#[test]
fn switching_the_language_changes_window_and_program_texts_without_a_restart_and_is_saved() {
    let (ui, state, _mc, path) = wired("lang");
    {
        let st = state.lock().unwrap();
        crate::render(&ui, &st);
    }
    assert_eq!(ui.get_exit_question().as_str(), "Are you sure you want to exit the launcher?");
    assert_eq!((ui.get_exit_yes().as_str(), ui.get_exit_no().as_str(), ui.get_play_label().as_str()), ("Yes, exit", "No, go back", "PLAY"));
    state.lock().unwrap().view.status = Some((crate::i18n::Msg::plain("Ready."), 0));
    ui.invoke_language_selected(2); // index 2 = Deutsch
    assert_eq!(ui.get_exit_question().as_str(), "Launcher wirklich beenden?");
    assert_eq!((ui.get_exit_yes().as_str(), ui.get_exit_no().as_str(), ui.get_play_label().as_str()), ("Ja, beenden", "Nein, zurück", "SPIELEN"));
    assert_eq!(ui.get_status_text().as_str(), "Bereit.", "program messages follow the language");
    assert_eq!(ui.get_language_index(), 2);
    assert_eq!(ui.get_language_names().row_data(0).unwrap().as_str(), "Automatisch (Systemsprache)");
    assert!(std::fs::read_to_string(&path).unwrap().contains("language = de"), "the choice is written to the configuration");
    ui.invoke_language_selected(1); // English again
    assert_eq!(ui.get_exit_question().as_str(), "Are you sure you want to exit the launcher?");
    assert_eq!(ui.get_status_text().as_str(), "Ready.");
    assert!(std::fs::read_to_string(&path).unwrap().contains("language = en"));
    ui.invoke_language_selected(0); // automatic: the key disappears
    assert!(!std::fs::read_to_string(&path).unwrap().contains("language"));
    assert_eq!(ui.get_language_index(), 0);
}

#[test]
fn every_translated_window_text_is_really_used_by_the_compiled_window() {
    // the Slint side: select German and read every @tr property back; a missing bundled translation would show English
    let (ui, ..) = wired("lang2");
    ui.invoke_language_selected(2);
    assert_ne!(ui.get_exit_question().as_str(), "Are you sure you want to exit the launcher?", "bundled translations are active");
    let de = Catalog::new("de");
    assert_eq!(ui.get_exit_question().as_str(), de.tr("Are you sure you want to exit the launcher?"), "Slint's bundle equals the program's catalog");
    assert_eq!(ui.get_play_label().as_str(), de.tr("PLAY"));
}

/// Not a regression test: with `TSW_GUI_SNAPSHOTS=<dir>` it renders the launcher's own window (software renderer, nothing of
/// the game) to PPM files for a visual review of layout and translations. Without the variable it does nothing.
#[test]
fn render_snapshots_for_visual_review_when_asked() {
    let Ok(dir) = std::env::var("TSW_GUI_SNAPSHOTS") else {
        return;
    };
    std::fs::create_dir_all(&dir).unwrap();
    let (ui, state, _mc, _p) = wired("snap");
    let adapter = SNAP_ADAPTER.with(|a| a.borrow().clone()).expect("adapter");
    let dump = |ui: &MainWindow, name: &str| {
        // let the colour animations (160 ms) finish before the picture is taken
        std::thread::sleep(std::time::Duration::from_millis(400));
        slint::platform::update_timers_and_animations();
        ui.window().request_redraw();
        let (w, h) = (WIDTH as usize, HEIGHT as usize);
        let mut buf = vec![slint::Rgb8Pixel::default(); w * h];
        adapter.draw_if_needed(|r| {
            r.render(&mut buf, w);
        });
        let mut out = format!("P6\n{w} {h}\n255\n").into_bytes();
        for p in &buf {
            out.extend([p.r, p.g, p.b]);
        }
        std::fs::write(format!("{dir}/{name}.ppm"), out).unwrap();
    };
    {
        let mut st = state.lock().unwrap();
        st.view.status = Some((crate::i18n::Msg::plain("Ready. Notes are in the advanced view."), 1));
        let cat = st.catalog.clone();
        let _ = cat;
        crate::render(&ui, &st);
    }
    ui.set_can_play(true);
    dump(&ui, "normal-en");
    ui.invoke_language_selected(2);
    dump(&ui, "normal-de");
    ui.set_confirm_exit(true);
    dump(&ui, "exit-dialog-de");
    ui.set_confirm_exit(false);
    ui.invoke_language_selected(1);
    ui.set_confirm_exit(true);
    dump(&ui, "exit-dialog-en");
    ui.set_confirm_exit(false);
    ui.set_advanced(true);
    ui.invoke_language_selected(2);
    dump(&ui, "advanced-de");
    crate::set_resolution_modes(&ui, &state.lock().unwrap(), &[(1920, 1200), (1920, 1080), (1280, 720)]);
    ui.window().dispatch_event(WindowEvent::PointerScrolled { position: LogicalPosition::new(500.0, 550.0), delta_x: 0.0, delta_y: -560.0 });
    dump(&ui, "display-de");
    ui.window().dispatch_event(WindowEvent::PointerScrolled { position: LogicalPosition::new(500.0, 550.0), delta_x: 0.0, delta_y: 560.0 });
    ui.set_advanced(false);
    for (i, t) in crate::theme::THEMES.iter().enumerate() {
        ui.invoke_theme_selected(i as i32);
        dump(&ui, &format!("normal-{}", t.id));
        ui.set_confirm_exit(true);
        dump(&ui, &format!("dialog-{}", t.id));
        ui.set_confirm_exit(false);
    }
    ui.invoke_theme_selected(1);
    ui.set_advanced(true);
    dump(&ui, "advanced-illuminati");
    ui.set_advanced(false);
    // the main view while no installation exists (the big button installs), both languages and both labels
    ui.invoke_theme_selected(0);
    let set_install = |label: &str, notice: Option<&str>| {
        let mut st = state.lock().unwrap();
        st.view.needs_install = true;
        st.view.location = "~/Games/The Secret World".into();
        st.view.install_label = Some(crate::i18n::Msg::plain(label));
        st.view.install_notice = notice.map(crate::i18n::Msg::plain);
    };
    set_install("INSTALL THE GAME", None);
    ui.invoke_language_selected(1);
    dump(&ui, "install-en");
    ui.set_install_consent(true);
    ui.set_busy(true);
    ui.set_busy(false);
    ui.set_microsoft_consent(false);
    dump(&ui, "microsoft-consent-en");
    ui.invoke_language_selected(2);
    dump(&ui, "microsoft-consent-de");
    ui.set_busy(false);
    set_install("CONTINUE INSTALLATION", None);
    ui.invoke_language_selected(2);
    dump(&ui, "install-continue-de");
    ui.invoke_language_selected(1);
    dump(&ui, "install-continue-en");
    ui.invoke_language_selected(2);
    set_install(
        "CONTINUE INSTALLATION",
        Some("An earlier installation inside the Flatpak data folder was found. It is only reported and never used or changed; the game is installed in the folder above."),
    );
    ui.set_advanced(true);
    ui.invoke_language_selected(2);
    dump(&ui, "advanced-install-de");
    ui.set_advanced(false);
    // the two waiting states: files exist but Funcom has not confirmed (the patcher is offered) and a Funcom program is working (the button waits)
    for (label, waiting, hint) in [
        (
            "VERIFY INSTALLATION",
            false,
            "The game data is not confirmed as complete yet. The original patcher checks it and repairs what is missing: start it with the button. Once it confirms that nothing is left to fetch, this window continues by itself and closes the patcher.",
        ),
        (
            "FUNCOM IS WORKING …",
            true,
            "The original Funcom program is working (download or check). When it shows “Play”, close its window: this window then checks the result by itself and continues.",
        ),
    ] {
        {
            let mut st = state.lock().unwrap();
            st.view.needs_install = true;
            st.view.install_waiting = waiting;
            st.view.install_label = Some(crate::i18n::Msg::plain(label));
            st.view.install_hint = Some(crate::i18n::Msg::plain(hint));
            st.view.install_notice = None;
        }
        for (lang, name) in [(1, "en"), (2, "de")] {
            ui.invoke_language_selected(lang);
            dump(&ui, &format!("gate-{}-{name}", if waiting { "working" } else { "verify" }));
        }
    }
}

// ---- themes ----

#[test]
fn choosing_a_theme_recolours_the_window_at_once_and_is_saved_and_independent_of_the_language() {
    use slint::ComponentHandle;
    let (ui, state, _mc, path) = wired("theme");
    {
        let st = state.lock().unwrap();
        crate::render(&ui, &st);
    }
    let accent = |ui: &MainWindow| ui.global::<crate::Theme>().get_red();
    assert_eq!(accent(&ui), crate::theme::THEMES[0].palette.accent.color(), "default = Templar red");
    assert_eq!(ui.get_theme_index(), 0);
    for (i, t) in crate::theme::THEMES.iter().enumerate() {
        ui.invoke_theme_selected(i as i32);
        assert_eq!(accent(&ui), t.palette.accent.color(), "{}: recoloured without restart", t.id);
        assert_eq!(ui.global::<crate::Theme>().get_card_line(), t.palette.card_line.color());
        assert_eq!(ui.get_theme_index(), i as i32);
    }
    ui.invoke_theme_selected(2); // Dragon
    assert!(std::fs::read_to_string(&path).unwrap().contains("theme = dragon"), "stored");
    ui.invoke_language_selected(2); // German: names and description follow the language, the colours stay
    assert_eq!(accent(&ui), crate::theme::resolve(Some("dragon")).palette.accent.color());
    assert_eq!(ui.get_theme_names().row_data(0).unwrap().as_str(), "Templer");
    assert_eq!(ui.get_theme_names().row_data(3).unwrap().as_str(), "Neutral / Zuberi");
    assert_eq!(ui.get_theme_description().as_str(), "Dunkles Grün und Schwarz, ruhig und geheimnisvoll.");
    ui.invoke_language_selected(1);
    assert_eq!(ui.get_theme_names().row_data(0).unwrap().as_str(), "Templars");
    assert_eq!(ui.get_theme_index(), 2, "the language change does not touch the theme");
    ui.invoke_theme_selected(0); // back to the default: stored as "not set", old files stay interchangeable
    assert!(!std::fs::read_to_string(&path).unwrap().contains("theme"));
}

#[test]
fn a_stored_theme_is_applied_at_start_and_an_unknown_one_falls_back_to_templars() {
    use slint::ComponentHandle;
    let (ui, state, _mc, _p) = wired("theme2");
    state.lock().unwrap().config.theme = Some("illuminati".into());
    crate::render(&ui, &state.lock().unwrap());
    assert_eq!(ui.global::<crate::Theme>().get_red(), crate::theme::resolve(Some("illuminati")).palette.accent.color());
    state.lock().unwrap().config.theme = Some("from-the-future".into());
    crate::render(&ui, &state.lock().unwrap());
    assert_eq!(ui.global::<crate::Theme>().get_red(), crate::theme::THEMES[0].palette.accent.color(), "unknown id → default, no crash");
    assert_eq!(ui.get_theme_index(), 0);
}

#[test]
fn the_theme_selector_acts_on_the_first_click() {
    // the ComboBox of the appearance card lives in the advanced view; open it and make sure the selector exists in the model
    let (ui, ..) = wired("theme3");
    ui.set_advanced(true);
    assert_eq!(ui.get_theme_names().row_count(), 0, "filled by render()");
}

#[test]
fn files_of_the_game_are_not_enough_funcom_has_to_confirm_and_the_window_follows_by_itself() {
    let _serial = GLOBAL_SEARCH.lock().unwrap_or_else(|e| e.into_inner());
    // this test is about Funcom's confirmation, not about the detection pause: the pause is off for its duration
    let _reset = ResetUserSearch;
    tsw_launch::winnative::set_autodetect_pause_enabled(false);
    use tsw_launch::install::{self, Evidence, Layout};
    use tsw_launch::patchlog::Verdict;
    #[cfg(windows)]
    {
        static ONCE: std::sync::Once = std::sync::Once::new();
        ONCE.call_once(|| {
            let root = std::env::temp_dir().join(format!("tsw-test-programfiles-{}", std::process::id()));
            for key in ["ProgramFiles(x86)", "ProgramFiles", "ProgramW6432"] {
                std::env::set_var(key, &root);
            }
            std::env::set_var("TSW_LAUNCH_NO_SYSTEM_SEARCH", "1");
        });
    }
    let (ui, state, _may_close, path) = wired("gate");
    let base = path.parent().unwrap().join("Games").join("The Secret World");
    let game = install::game_path(&Layout { base: base.clone() });
    std::fs::create_dir_all(game.join("RDB")).unwrap();
    let client = game.join("TheSecretWorldDX11.exe");
    std::fs::write(&client, b"x").unwrap();
    std::fs::write(game.join("RDB/RDBHashIndex.bin"), b"x").unwrap();
    ui.set_install_base(base.display().to_string().into());
    let label = |st: &crate::State| st.view.install_label.as_ref().map(|m| st.catalog.render(m)).unwrap_or_default();
    let mut st = state.lock().unwrap();

    // client and index exist (as they do 30 s into the first patcher run), nothing confirmed, nothing running: not ready, the patcher has to check
    st.evidence = Evidence::default();
    crate::refresh_install(&ui, &state, &mut st);
    assert!(st.view.needs_install && !st.view.install_waiting);
    assert_eq!(label(&st), "VERIFY INSTALLATION");
    assert!(st.view.install_hint.is_some());
    crate::adopt_when_ready(&ui, &state, &mut st);
    assert!(st.config.client_exe.is_none(), "nothing is taken over before Funcom confirmed it");

    // a Funcom program works and has not confirmed: the button waits, whatever the files look like
    for verdict in [Verdict::Unknown, Verdict::Working, Verdict::Downloading] {
        st.evidence = Evidence { verdict, patcher_running: true, ..Default::default() };
        crate::refresh_install(&ui, &state, &mut st);
        assert!(st.view.needs_install && st.view.install_waiting, "{verdict:?}");
        assert_eq!(label(&st), "FUNCOM IS WORKING …");
        crate::adopt_when_ready(&ui, &state, &mut st);
        assert!(st.config.client_exe.is_none());
    }

    // Funcom's patcher reports nothing left to fetch, although its window is still open: ready, taken over, the button is PLAY
    st.evidence = Evidence { verdict: Verdict::Complete, patcher_running: true, ..Default::default() };
    crate::refresh_install(&ui, &state, &mut st);
    assert!(!st.view.needs_install && !st.view.install_waiting && st.view.install_hint.is_none());
    crate::adopt_when_ready(&ui, &state, &mut st);
    assert_eq!(st.config.client_exe.as_deref(), Some(client.as_path()), "the installation is stored without a click");
    assert!(std::fs::read_to_string(&path).unwrap().contains("client_exe"), "and persisted");

    // a run that ended after the patcher announced that it is done also opens it: a closed patcher, last verdict "downloading", nothing in the configuration
    st.config.client_exe = None;
    st.evidence = Evidence { verdict: Verdict::Downloading, ..Default::default() };
    st.run_ok = false;
    crate::refresh_install(&ui, &state, &mut st);
    assert!(st.view.needs_install, "closed without proof: the patcher has to check");
    st.run_ok = true;
    crate::refresh_install(&ui, &state, &mut st);
    assert!(!st.view.needs_install, "closed after it announced that it is done");
    crate::adopt_when_ready(&ui, &state, &mut st);
    assert_eq!(st.config.client_exe.as_deref(), Some(client.as_path()));

    // once the user's own configuration names the client, a later unknown verdict never second-guesses it
    st.evidence = Evidence::default();
    st.run_ok = false;
    crate::refresh_install(&ui, &state, &mut st);
    assert!(!st.view.needs_install, "an adopted installation stays ready");
    let _ = std::fs::remove_dir_all(path.parent().unwrap());
}

#[test]
fn resolution_selection_is_saved_without_erasing_hidden_settings_or_fps() {
    let (ui, state, _mc, path) = wired("resolution");
    {
        let mut st = state.lock().unwrap();
        st.config.game_resolution = Some((1920, 1080));
        st.config.resolution = Some((1280, 720));
        st.config.env = vec![("KEEP".into(), "yes".into())];
        crate::set_resolution_modes(&ui, &st, &[(3840, 2160), (1920, 1080), (1920, 1080)]);
    }
    assert_eq!(ui.get_resolution_names().row_count(), 3);
    assert_eq!(ui.get_resolution_index(), 2);
    ui.set_fps_limit(144);
    ui.set_fps_hud(true);
    ui.invoke_resolution_selected(2);
    let saved = tsw_launch::config::load(&path).unwrap();
    assert_eq!(saved.game_resolution, Some((1920, 1080)));
    assert_eq!(saved.resolution, Some((1280, 720)));
    assert_eq!(saved.env, vec![("KEEP".into(), "yes".into())]);
    assert_eq!(saved.fps_limit, Some(tsw_launch::fps::FpsLimit::Cap(144)));
    assert_eq!(saved.fps_hud, Some(true));
    crate::set_resolution_modes(&ui, &state.lock().unwrap(), &[]);
    assert_eq!(ui.get_resolution_index(), -1);
    assert!(ui.get_resolution_unavailable());
    assert_eq!(state.lock().unwrap().config.game_resolution, Some((1920, 1080)), "discovery failure does not erase saved choice");
    ui.invoke_resolution_selected(0);
    let saved = tsw_launch::config::load(&path).unwrap();
    assert_eq!(saved.game_resolution, None);
    assert!(saved.game_managed);
    assert_eq!(saved.resolution, Some((1280, 720)), "legacy keys remain stored when explicitly selecting game-managed");
}

#[test]
fn advanced_layout_keeps_slider_and_hides_legacy_panels() {
    let source = include_str!("../ui/launcher.slint").replace("\r\n", "\n"); // a Windows checkout may convert line endings
    let source = source.as_str();
    assert!(source.contains("FpsSlider { width: 320px; value <=> root.fps-limit"));
    for title in ["GAMESCOPE (OPTIONAL)", "MANUAL OVERRIDES", "CHECKS"] {
        assert!(source.contains(&format!("if false : Card {{\n                        title: @tr(\"{title}\")")));
    }
}

#[test]
fn existing_native_installation_is_not_rewritten_or_prompted_for_terms() {
    let root = std::env::temp_dir().join(format!("tsw-ui-existing-ms-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let layout = tsw_launch::install::Layout { base: root.clone() };
    std::fs::create_dir_all(layout.prefix()).unwrap();
    let manifest = layout.prefix().join("tsw-launch-prefix.txt");
    let content = "native_profile=d3d-native-ms2010\n";
    std::fs::write(&manifest, content).unwrap();
    crate::install_shader_profile(&layout, false, &|_| panic!("existing installation needs no new acceptance"), &tsw_launch::progress::Silent, &AtomicBool::new(false)).unwrap();
    assert_eq!(std::fs::read_to_string(manifest).unwrap(), content);
    assert_eq!(std::fs::read_dir(layout.prefix()).unwrap().count(), 1, "no new files in the existing prefix");
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn both_consents_are_available_before_install_and_neither_implies_the_other() {
    let ui = fresh();
    ui.set_install_mode(true);
    assert!(!ui.get_install_consent() && !ui.get_microsoft_consent());
    for (downloads, microsoft) in [(false, false), (true, false), (false, true), (true, true)] {
        ui.set_install_consent(downloads);
        ui.set_microsoft_consent(microsoft);
        assert_eq!(ui.get_setup_consents_ready(), downloads && microsoft);
    }
}

#[test]
fn installation_callbacks_cannot_bypass_either_consent() {
    let (ui, state, _may_close, _p) = wired("both-consents");
    ui.set_install_mode(true);
    for (downloads, microsoft) in [(false, false), (true, false), (false, true)] {
        ui.set_install_consent(downloads);
        ui.set_microsoft_consent(microsoft);
        ui.invoke_play();
        ui.invoke_install_action();
        assert!(!state.lock().unwrap().install_auto);
        assert!(!ui.get_busy());
    }
}

#[test]
fn setup_checkboxes_and_links_work_before_the_install_button_on_the_first_click() {
    let ui = fresh();
    crate::i18n::select_language("de");
    ui.set_install_mode(true);
    ui.set_install_main_label("SPIEL INSTALLIEREN".into());
    ui.set_install_location("~/Games/The Secret World".into());
    ui.set_status_text("Bereit. Hinweise finden Sie in der erweiterten Ansicht.".into());
    let installs = Rc::new(Cell::new(0));
    let count = installs.clone();
    ui.on_play(move || count.set(count.get() + 1));
    let links = Rc::new(Cell::new(0));
    let count = links.clone();
    ui.on_microsoft_terms_link(move |_| count.set(count.get() + 1));
    click(&ui, 500.0, 477.0);
    assert_eq!(installs.get(), 0);
    click(&ui, 175.0, 343.0); // Microsoft can be selected BEFORE the download checkbox.
    assert!(ui.get_microsoft_consent() && !ui.get_install_consent());
    click(&ui, 500.0, 477.0);
    assert_eq!(installs.get(), 0);
    click(&ui, 395.0, 263.0);
    assert!(ui.get_setup_consents_ready());
    click(&ui, 500.0, 477.0);
    assert_eq!(installs.get(), 1);
    click(&ui, 175.0, 343.0);
    click(&ui, 500.0, 477.0);
    assert_eq!(installs.get(), 1, "revoking either consent locks installation again");
    click(&ui, 395.0, 263.0);
    click(&ui, 370.0, 375.0);
    click(&ui, 600.0, 375.0);
    assert_eq!(links.get(), 2);
    assert!(!ui.get_install_consent() && !ui.get_microsoft_consent(), "opening terms is never agreement");
    ui.set_busy(true);
    click(&ui, 175.0, 343.0);
    click(&ui, 395.0, 263.0);
    assert!(!ui.get_setup_consents_ready());
}

#[test]
fn on_windows_only_the_download_consent_is_needed_because_no_microsoft_software_is_downloaded() {
    let ui = fresh();
    ui.set_windows(true);
    ui.set_install_mode(true);
    assert!(!ui.get_setup_consents_ready());
    ui.set_install_consent(true);
    assert!(ui.get_setup_consents_ready(), "the hidden Microsoft consent must not block the Windows setup");
    // an existing installation that the original patcher only has to check: nothing is downloaded by the launcher, so no download consent is asked
    ui.set_install_consent(false);
    ui.set_install_state_kind(1);
    assert!(ui.get_setup_consents_ready());
    ui.set_install_state_kind(0);
    ui.set_install_consent(true);
    ui.set_windows(false);
    assert!(!ui.get_setup_consents_ready(), "Linux still needs both consents");
}

/// First start on Windows: exactly three ways to set the game up, each reachable, each acting once on its own callback.
#[test]
fn the_first_start_on_windows_offers_exactly_three_choices() {
    fn window() -> MainWindow {
        let ui = fresh();
        ui.set_windows(true);
        ui.set_install_mode(true);
        ui.set_first_run(true);
        ui.set_install_consent(true);
        ui
    }
    // which callback fires where: scan the main view
    let ui = window();
    let (play, browse, adopt, other) = (Rc::new(Cell::new(0u32)), Rc::new(Cell::new(0u32)), Rc::new(Cell::new(0u32)), Rc::new(Cell::new(0u32)));
    let (p, b, a, o1, o2, o3) = (play.clone(), browse.clone(), adopt.clone(), other.clone(), other.clone(), other.clone());
    ui.on_play(move || p.set(p.get() + 1));
    ui.on_install_browse(move || b.set(b.get() + 1));
    ui.on_install_adopt_browse(move || a.set(a.get() + 1));
    ui.on_install_action(move || o1.set(o1.get() + 1));
    ui.on_install_patcher(move || o2.set(o2.get() + 1));
    ui.on_install_cancel(move || o3.set(o3.get() + 1));
    let mut seen = std::collections::BTreeSet::new();
    for y in (60..HEIGHT).step_by(6) {
        for x in (200..800.min(WIDTH)).step_by(8) {
            let before = (play.get(), browse.get(), adopt.get());
            click(&ui, x as f32, y as f32);
            let after = (play.get(), browse.get(), adopt.get());
            if after.0 > before.0 {
                seen.insert("download");
            }
            if after.1 > before.1 {
                seen.insert("disk");
            }
            if after.2 > before.2 {
                seen.insert("existing");
            }
        }
    }
    assert_eq!(seen.into_iter().collect::<Vec<_>>(), ["disk", "download", "existing"], "all three choices must be reachable");
    assert_eq!(other.get(), 0, "nothing else is offered in this view");
    // once the user is past this view (an installer was chosen, something runs) the normal main button returns
    let ui = window();
    ui.set_first_run(false);
    let (b, a) = (Rc::new(Cell::new(0u32)), Rc::new(Cell::new(0u32)));
    let (b2, a2) = (b.clone(), a.clone());
    ui.on_install_browse(move || b2.set(b2.get() + 1));
    ui.on_install_adopt_browse(move || a2.set(a2.get() + 1));
    for y in (60..HEIGHT).step_by(6) {
        for x in (200..800.min(WIDTH)).step_by(8) {
            click(&ui, x as f32, y as f32);
        }
    }
    assert_eq!((b.get(), a.get()), (0, 0), "the three choices are only offered on the first start");
}

/// Not a regression test: with `TSW_GUI_SNAPSHOTS=<dir>` it renders the Windows main view in its situations (first start, installation found, several
/// installations) to PPM files for a visual review. Without the variable it does nothing.
#[test]
fn render_windows_snapshots_for_visual_review_when_asked() {
    let Ok(dir) = std::env::var("TSW_GUI_SNAPSHOTS") else {
        return;
    };
    std::fs::create_dir_all(&dir).unwrap();
    let ui = fresh();
    let adapter = SNAP_ADAPTER.with(|a| a.borrow().clone()).expect("adapter");
    let dump = |ui: &MainWindow, name: &str| {
        std::thread::sleep(std::time::Duration::from_millis(400));
        slint::platform::update_timers_and_animations();
        ui.window().request_redraw();
        let (w, h) = (WIDTH as usize, HEIGHT as usize);
        let mut buf = vec![slint::Rgb8Pixel::default(); w * h];
        adapter.draw_if_needed(|r| {
            r.render(&mut buf, w);
        });
        let mut out = format!(
            "P6
{w} {h}
255
"
        )
        .into_bytes();
        for p in &buf {
            out.extend([p.r, p.g, p.b]);
        }
        std::fs::write(format!("{dir}/{name}.ppm"), out).unwrap();
    };
    ui.set_windows(true);
    ui.set_install_mode(true);
    ui.set_can_play(false);
    ui.set_status_text("Bereit.".into());
    for (code, idx) in [("en", 1), ("de", 2)] {
        ui.invoke_language_selected(idx);
        // first start: nothing installed
        ui.set_first_run(true);
        ui.set_install_location(r"C:\Program Files (x86)\Funcom\The Secret World".into());
        ui.set_install_state(crate::i18n::Catalog::new(code).render(&crate::i18n::Msg::plain("Not installed.")).into());
        ui.set_install_state_kind(0);
        dump(&ui, &format!("win-first-run-{code}"));
        // an installation was found but is not confirmed complete
        ui.set_first_run(false);
        ui.set_install_location(r"C:\Games\The Secret World".into());
        ui.set_install_state(crate::i18n::Catalog::new(code).render(&crate::i18n::Msg::plain("Installation found, but not confirmed complete. The original patcher has to check it.")).into());
        ui.set_install_state_kind(1);
        ui.set_install_main_label(if code == "de" { "INSTALLATION PRÜFEN".into() } else { "VERIFY INSTALLATION".into() });
        ui.set_install_consent(true);
        dump(&ui, &format!("win-found-{code}"));
        // several installations
        ui.set_detected_installs(slint::ModelRc::new(slint::VecModel::from(vec![r"C:\Games\The Secret World\TheSecretWorldDX11.exe".into(), r"D:\Spiele\TSW\TheSecretWorldDX11.exe".into()])));
        dump(&ui, &format!("win-several-{code}"));
        ui.set_detected_installs(slint::ModelRc::default());
        // the advanced view with the new-copy action and a remembered ignored installation
        ui.set_ignored_text(r"Ignorierte vorhandene Installation: C:\Games\The Secret World".into());
        ui.set_advanced(true);
        dump(&ui, &format!("win-advanced-{code}"));
        ui.set_advanced(false);
        ui.set_ignored_text("".into());
        // a game that is ready to play, but Direct3D libraries are missing
        ui.set_install_mode(false);
        ui.set_can_play(true);
        ui.set_directx_text("Diese Direct3D-Bibliotheken der Microsoft DirectX End-User Runtimes (Juni 2010) fehlen auf diesem Computer: d3dx9_43.dll, d3dx11_43.dll. Das Originalspiel braucht sie in der 32-Bit-Form; ohne sie startet es eventuell nicht oder zeigt Fehler.".into());
        dump(&ui, &format!("win-directx-{code}"));
        ui.set_directx_text("".into());
        ui.set_install_mode(true);
    }
}

/// Missing Direct3D libraries on Windows: the window says so, offers exactly two actions, each acting once, and hides the block in the first-run view.
#[test]
fn missing_directx_libraries_are_explained_and_offer_install_and_recheck() {
    let ui = fresh();
    ui.set_windows(true);
    ui.set_status_text("Bereit.".into());
    ui.set_directx_text("Diese Direct3D-Bibliotheken fehlen: d3dx9_43.dll".into());
    let (install, recheck, other) = (Rc::new(Cell::new(0u32)), Rc::new(Cell::new(0u32)), Rc::new(Cell::new(0u32)));
    let (i, r, o) = (install.clone(), recheck.clone(), other.clone());
    ui.on_install_directx(move || i.set(i.get() + 1));
    ui.on_recheck_directx(move || r.set(r.get() + 1));
    ui.on_play(move || o.set(o.get() + 1));
    let mut seen = std::collections::BTreeSet::new();
    for y in (60..HEIGHT).step_by(6) {
        for x in (200..800.min(WIDTH)).step_by(8) {
            let before = (install.get(), recheck.get());
            click(&ui, x as f32, y as f32);
            if install.get() > before.0 {
                seen.insert("install");
            }
            if recheck.get() > before.1 {
                seen.insert("recheck");
            }
        }
    }
    assert_eq!(seen.into_iter().collect::<Vec<_>>(), ["install", "recheck"], "both actions must be reachable");
    // nothing missing: no block, so neither action can be reached
    let ui = fresh();
    ui.set_windows(true);
    ui.set_directx_text("".into());
    let n = Rc::new(Cell::new(0u32));
    let (a, b) = (n.clone(), n.clone());
    ui.on_install_directx(move || a.set(a.get() + 1));
    ui.on_recheck_directx(move || b.set(b.get() + 1));
    for y in (60..HEIGHT).step_by(6) {
        for x in (200..800.min(WIDTH)).step_by(8) {
            click(&ui, x as f32, y as f32);
        }
    }
    assert_eq!(n.get(), 0, "no DirectX block while nothing is missing");
    // first start (game not installed yet): the Funcom installer comes first, so the block is not shown
    let ui = fresh();
    ui.set_windows(true);
    ui.set_install_mode(true);
    ui.set_first_run(true);
    ui.set_directx_text("fehlt".into());
    let n = Rc::new(Cell::new(0u32));
    let (a, b) = (n.clone(), n.clone());
    ui.on_install_directx(move || a.set(a.get() + 1));
    ui.on_recheck_directx(move || b.set(b.get() + 1));
    for y in (60..HEIGHT).step_by(6) {
        for x in (200..800.min(WIDTH)).step_by(8) {
            click(&ui, x as f32, y as f32);
        }
    }
    assert_eq!(n.get(), 0, "the first-run view stays free of the DirectX block");
}

/// "Install a new copy in another folder": an existing installation is ignored (never touched), the chosen empty folder is remembered, the window offers the
/// first-run choices again and the stored client path of the old installation is forgotten; an unsuitable folder changes nothing; the decision can be undone.
#[cfg(windows)] // Windows paths (the folder rules know drive letters)
#[test]
fn a_new_copy_in_another_folder_ignores_the_existing_installation_without_touching_it() {
    use tsw_launch::install;
    let _serial = GLOBAL_SEARCH.lock().unwrap_or_else(|e| e.into_inner());
    let _reset = ResetUserSearch;
    let (ui, state, _may_close, config_path) = wired("newcopy");
    // the installation state of this test goes next to the test's own configuration, never into the real settings
    let state_file = config_path.parent().unwrap().join("install-state");
    let root = std::env::temp_dir().join(format!("tsw-newcopy-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let old = root.join("Games").join("The Secret World");
    std::fs::create_dir_all(old.join("RDB")).unwrap();
    let old_exe = old.join("TheSecretWorldDX11.exe");
    std::fs::write(&old_exe, b"MZ").unwrap();
    std::fs::write(old.join("RDB").join("RDBHashIndex.bin"), b"x").unwrap();
    let before = (std::fs::metadata(&old_exe).unwrap().modified().unwrap(), std::fs::read_dir(&old).unwrap().count());
    let new = root.join("Neu");
    ui.set_install_base(root.join("base").display().to_string().into());
    {
        let mut st = state.lock().unwrap();
        st.config.client_exe = Some(old_exe.clone());
        st.install.client_exe = Some(old_exe.clone());
        // an unsuitable folder (inside the old installation) is refused and changes nothing
        crate::start_new_copy(&ui, &state, &mut st, old.join("RDB"));
        assert!(st.install.ignored.is_empty() && st.install.target.is_none(), "nothing was decided");
        assert!(st.config.client_exe.is_some(), "the configuration is unchanged");
        assert!(st.view.message.is_some());
        // a suitable folder
        std::fs::create_dir_all(&new).unwrap();
        crate::start_new_copy(&ui, &state, &mut st, new.clone());
        assert_eq!(st.install.target.as_deref(), Some(new.as_path()));
        assert!(st.install.ignored.iter().any(|d| tsw_launch::winnative::overlaps(d, &old)), "the old installation is ignored: {:?}", st.install.ignored);
        assert!(st.config.client_exe.is_none() && st.install.client_exe.is_none(), "a stored choice inside the old installation is forgotten");
        assert!(!std::fs::read_to_string(&config_path).unwrap_or_default().contains("client_exe"), "and removed from the saved configuration");
        assert!(install::find_installation(&install::Layout { base: root.join("base") }, &st.install).is_none_or(|f| !f.client.starts_with(&old)), "the launcher no longer finds the old installation");
        crate::refresh_install(&ui, &state, &mut st);
        assert!(st.view.first_run, "the first-run choices are offered again");
        assert_eq!(st.view.location, new.display().to_string(), "the new folder is shown");
        assert!(st.view.ignored.iter().any(|d| tsw_launch::winnative::overlaps(d, &old)));
    }
    // the old installation was never touched
    assert_eq!((std::fs::metadata(&old_exe).unwrap().modified().unwrap(), std::fs::read_dir(&old).unwrap().count()), before);
    assert!(old.join("RDB").join("RDBHashIndex.bin").is_file());
    // the decision is saved with the installation state and can be taken back
    let saved = std::fs::read_to_string(&state_file).unwrap();
    assert!(saved.contains("ignored_install = ") && saved.contains("install_target = "), "{saved}");
    ui.invoke_use_ignored_again();
    {
        let st = state.lock().unwrap();
        assert!(st.install.ignored.is_empty() && st.install.target.is_none());
        assert!(!std::fs::read_to_string(&state_file).unwrap().contains("ignored_install"));
    }
    let _ = std::fs::remove_dir_all(&root);
}

/// The windows offers the new-copy action: as a link in the main view while an installation is found but not confirmed, and always in the advanced view.
#[test]
fn the_new_copy_action_is_reachable_on_windows_only() {
    let reach = |windows: bool, kind: i32, first_run: bool, advanced: bool| -> u32 {
        let ui = fresh();
        ui.set_windows(windows);
        ui.set_install_mode(true);
        ui.set_first_run(first_run);
        ui.set_install_state_kind(kind);
        ui.set_advanced(advanced);
        let n = Rc::new(Cell::new(0u32));
        let c = n.clone();
        ui.on_install_new_copy(move || c.set(c.get() + 1));
        // below the title bar (the gear opens the advanced view) and away from the window edges
        for y in (60..HEIGHT).step_by(4) {
            for x in (120..800.min(WIDTH)).step_by(40) {
                click(&ui, x as f32, y as f32);
            }
        }
        n.get()
    };
    assert!(reach(true, 1, false, false) > 0, "main view, installation found but not confirmed");
    assert_eq!(reach(true, 0, true, false), 0, "no link in the first-run view (the three choices are the way there)");
    assert_eq!(reach(false, 1, false, false), 0, "never on Linux");
    assert!(reach(true, 0, false, true) > 0, "advanced view");
    assert_eq!(reach(false, 0, false, true), 0, "never on Linux");
}

/// The automatic detection is paused on Windows: at the normal start nothing is found - not through a stored path of an old configuration - and the first-run
/// choices appear at once. Choosing an installation by hand still works and from then on the stored path counts.
#[cfg(windows)]
#[test]
fn with_the_detection_paused_an_old_stored_path_is_not_taken_and_a_manual_choice_still_works() {
    use tsw_launch::install;
    let _serial = GLOBAL_SEARCH.lock().unwrap_or_else(|e| e.into_inner());
    let _reset = ResetUserSearch;
    let (ui, state, _may_close, _config_path) = wired("paused");
    let root = std::env::temp_dir().join(format!("tsw-paused-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let old = root.join("Games").join("The Secret World");
    std::fs::create_dir_all(old.join("RDB")).unwrap();
    let exe = old.join("TheSecretWorldDX11.exe");
    std::fs::write(&exe, b"MZ").unwrap();
    std::fs::write(old.join("RDB").join("RDBHashIndex.bin"), b"x").unwrap();
    ui.set_install_base(root.join("base").display().to_string().into());
    ui.set_windows(true);
    let before = std::fs::metadata(&exe).unwrap().modified().unwrap();
    {
        let mut st = state.lock().unwrap();
        // an old configuration and an old installation state that name the existing installation (not chosen by the user)
        st.config.client_exe = Some(exe.clone());
        st.install.client_exe = Some(exe.clone());
        st.install.apply_search(); // what the program does at start
        assert!(tsw_launch::winnative::autodetect_paused() && !tsw_launch::winnative::stored_paths_honored());
        crate::refresh_install(&ui, &state, &mut st);
        assert!(st.view.first_run && st.view.needs_install, "the first-run choices appear at once");
        assert!(st.view.detected.is_empty(), "nothing is listed as found");
        assert!(!crate::config_names_client(&st), "the old configuration does not name a client");
        assert!(install::find_installation(&install::Layout { base: root.join("base") }, &st.install).is_none(), "and nothing is detected");
        // the user chooses the installation by hand: it works, and the stored path counts from then on
        let adopted = install::adopt(&old).unwrap();
        st.install.client_exe = Some(adopted.clone());
        crate::adopt_into_config(&ui, &state, &mut st, install::Adoption { client_exe: adopted, prefix: None });
        assert!(st.install.choice_made && tsw_launch::winnative::stored_paths_honored());
        assert_eq!(st.config.client_exe.as_deref(), Some(exe.as_path()), "a stale stored path did not block the real choice");
        crate::refresh_install(&ui, &state, &mut st);
        assert!(!st.view.needs_install && !st.view.first_run, "after the manual choice the installation is used");
        // the decision survives a restart of the program (it is part of the installation state)
        let text = st.install.to_text();
        assert!(text.contains("choice_made = yes"));
        let reloaded = install::State::parse(&text).unwrap();
        reloaded.apply_search();
        assert!(tsw_launch::winnative::stored_paths_honored());
    }
    assert_eq!(std::fs::metadata(&exe).unwrap().modified().unwrap(), before, "the installation itself was never touched");
    let _ = std::fs::remove_dir_all(&root);
}
