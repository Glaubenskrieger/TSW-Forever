//! The experimental Windows launcher update in the window (signed installer: download, check, start, restore the previous version).
//!
//! Product decision (2026-11): Windows has NO automatic update. This module exists only with the Cargo feature `windows-self-update`, which no product build enables.
//! It is kept for development; see `tsw_launch::update` and `scripts/windows-update-spike/HANDOVER.md`.

use super::*;

/// Download, check and start the signed installer on a worker thread (see `tsw_launch::update::windows::apply`). The window shows each step. The worker waits for the
/// installer's exit code: normally Inno Setup closes this launcher during the installation and starts the new one (nobody reads a result then); if the launcher is still
/// here when the installer has ended, a success makes it leave and any other result is shown as a failure that changes nothing.
pub(crate) fn start_windows_update(weak: &slint::Weak<MainWindow>, state: &Shared, may_close: &Arc<AtomicBool>, st: &mut State, installer: tsw_launch::update::Installer) {
    use tsw_launch::update::windows::{Failure, Phase, Step};
    let Some(services) = st.update.win_services.clone() else {
        st.update.win = Phase::Failed(Failure::Storage("the update service is not available".into()));
        return;
    };
    st.update.win_dismissed = false;
    st.update.win = Phase::Working(Step::Downloading);
    let active = st.update.win_installer_active.clone();
    let (weak, state, may_close) = (weak.clone(), state.clone(), may_close.clone());
    std::thread::spawn(move || {
        let unique = (u64::from(std::process::id()) << 32) ^ std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0);
        let (w2, s2) = (weak.clone(), state.clone());
        let mut progress = move |step: Step| post(&w2, &s2, move |_, st| st.update.win = Phase::Working(step));
        let result = run_update(&*services.download, &*services.starter, &installer, &services.updates_dir, unique, &active, &mut progress);
        post(&weak, &state, move |ui, st| windows_update_finished(ui, st, &may_close, result));
    });
}

/// The job of the update worker, separate from the window so that its guarantees can be tested: `active` is true exactly while the installer process runs (set when
/// `apply` reports `Step::Installing`, i.e. after a successful start), and it is cleared on EVERY way out of `apply`: a result of any kind, and a panic, which is turned into a
/// failure instead of leaving the window in an "installing" state that closes without asking. Clearing happens here, on the worker, without waiting for the window.
pub(crate) fn run_update(
    download: &dyn tsw_launch::update::windows::Download,
    starter: &dyn tsw_launch::update::windows::Starter,
    installer: &tsw_launch::update::Installer,
    updates_dir: &Path,
    unique: u64,
    active: &AtomicBool,
    progress: &mut dyn FnMut(tsw_launch::update::windows::Step),
) -> Result<(), tsw_launch::update::windows::Failure> {
    use tsw_launch::update::windows::{apply, Failure, Step};
    let mut report = |step: Step| {
        if step == Step::Installing {
            // from here on the installer process exists and may ask this window to close
            active.store(true, Ordering::SeqCst);
        }
        progress(step);
    };
    let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| apply(download, starter, installer, updates_dir, unique, &mut report)));
    active.store(false, Ordering::SeqCst);
    outcome.unwrap_or_else(|_| Err(Failure::ResultUnknown("the update worker stopped unexpectedly".into())))
}

/// The end of a Windows update attempt, on the UI thread. This only runs when the launcher survived the installer (normally Inno Setup closes it, and the new launcher
/// has been started by the installer): an installer that ended with success means the launcher leaves; any other end is shown and the launcher stays as it is.
pub(crate) fn windows_update_finished(ui: &MainWindow, st: &mut State, may_close: &AtomicBool, result: Result<(), tsw_launch::update::windows::Failure>) {
    use tsw_launch::update::windows::Phase;
    // the installer has ended: the window asks before it closes again
    st.update.win_installer_active.store(false, Ordering::SeqCst);
    match result {
        Ok(()) => {
            st.update.win = Phase::Started;
            st.install_cancel.store(true, Ordering::SeqCst);
            leave(ui, may_close);
        }
        Err(f) => st.update.win = Phase::Failed(f),
    }
}

#[cfg(any(windows, test))]
/// The offer a check result gives for a normal update (a testing build is never moved to stable by itself, so a switch offer is not an update).
pub(crate) fn update_offer(outcome: tsw_launch::update::Outcome) -> Option<tsw_launch::update::Offer> {
    match outcome {
        tsw_launch::update::Outcome::Offer(o) if o.use_as == tsw_launch::channel::OfferUse::Update => Some(o),
        _ => None,
    }
}

/// Windows: the update check at start (stable builds only, at most every six hours, silent offline) and the services for the update itself.
#[cfg(windows)]
pub(crate) fn start_windows_update_check(ui: &MainWindow, state: &Shared) {
    use tsw_launch::update::{self, windows as w};
    if tsw_launch::channel::build() != tsw_launch::channel::Channel::Stable {
        return;
    }
    let Some(data) = tsw_launch::winnative::data_dir(&tsw_launch::winnative::real_env) else { return };
    let curl = doctor::find_program(Path::new("curl"), std::env::var_os("PATH").as_deref()).unwrap_or_else(|| PathBuf::from("curl"));
    let updates_dir = data.join("updates");
    let services = WinServices { download: Arc::new(w::CurlDownload { curl: curl.clone() }), starter: Arc::new(w::DirectStarter), updates_dir: updates_dir.clone() };
    let (weak, state) = (ui.as_weak(), state.clone());
    std::thread::spawn(move || {
        let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
        w::clean_leftovers(&updates_dir, now);
        post(&weak, &state, move |_, st| st.update.win_services = Some(services));
        let stamp = data.join("update-check");
        if !w::check_due(now, std::fs::read_to_string(&stamp).ok().as_deref().and_then(w::parse_last_check)) {
            return;
        }
        let ctx = update::Context {
            build: tsw_launch::channel::build(),
            feed: tsw_launch::channel::Channel::Stable,
            key: tsw_launch::channel::STABLE_UPDATE_KEY,
            repo_url: tsw_launch::project::REPO_URL,
            installed: tsw_launch::project::APP_VERSION,
            now,
        };
        let outcome = update::check(&update::CurlFeed { curl }, &ctx);
        // an answer (also "nothing new" or a refusal) counts as a check; being offline does not, so the next start tries again
        if !matches!(outcome, update::Outcome::Unknown(_) | update::Outcome::Inactive(_)) {
            let _ = std::fs::create_dir_all(&data);
            let _ = std::fs::write(&stamp, now.to_string());
        }
        if let Some(offer) = update_offer(outcome) {
            post(&weak, &state, move |_, st| {
                st.update.win_offer = Some(offer);
                st.update.win = w::Phase::Available;
            });
        }
    });
}
