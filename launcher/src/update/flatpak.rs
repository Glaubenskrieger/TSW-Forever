//! The Flatpak side of the launcher update: when it applies, and what the update portal's signals mean. Pure: no D-Bus, no files, no network.
//!
//! The update itself is done by the HOST's Flatpak through the update portal (`org.freedesktop.portal.Flatpak`, `UpdateMonitor`): it checks the signature
//! of the repository, installs the new version atomically next to the running one, and the new version is used at the next start. The sandboxed launcher
//! only listens to the portal's signals and asks for the update after the user agreed; it never writes into `/app` and has no extra permission.

use crate::channel::{check_flatpak_identity, Channel, Identity};

/// Where this Flatpak is installed, from the `app-path=` of `/.flatpak-info`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Scope {
    /// `~/.local/share/flatpak` (also the usual SteamOS desktop case): the update needs no password.
    User,
    /// `/var/lib/flatpak`: the update may ask for the administrator's password (polkit).
    System,
    /// Another installation (a drive, a custom folder) or an unknown layout.
    Other,
}

/// `app-path=/var/lib/flatpak/app/<id>/x86_64/<branch>/<commit>/files` -> [`Scope`].
pub fn install_scope(info: &str) -> Scope {
    let path = info.lines().map(str::trim).find_map(|l| l.strip_prefix("app-path=")).unwrap_or("");
    let in_home = path.starts_with("/home/") && path.contains("/flatpak/app/");
    if path == "/var/lib/flatpak" || path.starts_with("/var/lib/flatpak/") {
        Scope::System
    } else if path.contains("/.local/share/flatpak/") || in_home {
        Scope::User
    } else {
        Scope::Other
    }
}

/// Whether the launcher offers updates through the portal at all.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Eligibility {
    /// Offer updates; the scope only changes the wording of a failure.
    Eligible(Scope),
    /// Not running in a Flatpak (a native build or `cargo run`): nothing to do.
    NotFlatpak,
    /// The installation does not fit this build (the text says why): updates stay off.
    WrongIdentity(String),
    /// A package without an update source: a local build (branch `local`, the former single-file bundles) or a testing package (handed out by hand).
    NoUpdateSource(String),
}

/// Decide from the text of `/.flatpak-info` (`None` = the file does not exist).
pub fn eligibility(info: Option<&str>, build: Channel, expected_app_id: &str) -> Eligibility {
    let Some(info) = info else { return Eligibility::NotFlatpak };
    if let Identity::Mismatch(why) = check_flatpak_identity(info, build, expected_app_id) {
        return Eligibility::WrongIdentity(why);
    }
    // Only the stable build has an update source (the public repository). A testing package is distributed by hand; its updates are new bundles.
    if build != Channel::Stable {
        return Eligibility::NoUpdateSource("this package is distributed by hand".into());
    }
    let branch = info.lines().map(str::trim).find_map(|l| l.strip_prefix("branch=")).unwrap_or("");
    if branch != "stable" {
        return Eligibility::NoUpdateSource(format!("the branch {branch:?} has no update source"));
    }
    Eligibility::Eligible(install_scope(info))
}

/// What the window shows about the launcher update.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum Phase {
    /// Nothing to say (also: offline, unknown).
    #[default]
    Idle,
    /// A newer version is available; waits for the user's decision.
    Available,
    /// The portal is installing (percent 0..=100).
    Working(u32),
    /// Installed; the running program is still the old one: restart to use the new version.
    RestartNeeded,
    /// The user asked, the portal found nothing to install (source not reachable, update not allowed, or already current).
    NothingToInstall,
    /// The portal reported an error.
    Failed(Failure),
}

/// Coarse reason of a failure (the technical text stays in the log; the window gives a hint).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Failure {
    /// Not allowed: the user denied it, or it needs authentication (a system-wide installation).
    NotAllowed,
    Network,
    Other(String),
}

/// What the portal tells us.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Event {
    /// `UpdateAvailable`: the commit that runs, the one that is installed and the one on the remote.
    Available { running: String, local: String, remote: String },
    /// `Progress`: status 0 = running, 1 = empty, 2 = done, 3 = failed.
    Progress { status: u32, percent: u32, error: String, message: String },
    /// The user pressed the button.
    Requested,
    /// The portal call itself could not be made.
    CallFailed(String),
}

fn is_commit(s: &str) -> bool {
    s.len() == 64 && s.bytes().all(|b| b.is_ascii_hexdigit())
}

/// Classify the error text of a failed `Progress`. Only the coarse kind matters; the text itself is not shown.
pub fn classify_failure(error: &str, message: &str) -> Failure {
    let t = format!("{error} {message}").to_ascii_lowercase();
    if ["not allowed", "authoriz", "authentic", "polkit", "permission", "denied", "forbidden"].iter().any(|k| t.contains(k)) {
        Failure::NotAllowed
    } else if ["network", "resolve", "connect", "timed out", "timeout", "unreachable", "offline", "http", "download"].iter().any(|k| t.contains(k)) {
        Failure::Network
    } else {
        Failure::Other(message.chars().filter(|c| !c.is_control()).take(200).collect())
    }
}

/// The next phase after an event. A running install is not disturbed by availability signals; a finished install always wins.
pub fn reduce(current: &Phase, event: &Event) -> Phase {
    match event {
        Event::Available { running, local, remote } => {
            if !is_commit(running) || !is_commit(local) || !is_commit(remote) {
                return current.clone(); // an unusable signal changes nothing
            }
            if running != local {
                return Phase::RestartNeeded; // an update is installed and waits for the next start
            }
            match current {
                Phase::Working(_) | Phase::RestartNeeded => current.clone(),
                _ if remote != local => Phase::Available,
                _ => Phase::Idle,
            }
        }
        Event::Progress { status, percent, error, message } => match status {
            0 => Phase::Working((*percent).min(100)),
            1 => Phase::NothingToInstall,
            2 => Phase::RestartNeeded,
            3 => Phase::Failed(classify_failure(error, message)),
            _ => current.clone(),
        },
        Event::Requested => match current {
            Phase::Working(_) | Phase::RestartNeeded => current.clone(),
            _ => Phase::Working(0),
        },
        Event::CallFailed(e) => Phase::Failed(classify_failure("", e)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const A: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
    const B: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
    const ID: &str = "org.example.App";

    fn info(branch: &str, app_path: &str) -> String {
        format!("[Application]\nname={ID}\n\n[Instance]\nbranch={branch}\napp-path={app_path}\napp-commit={A}\n")
    }

    #[test]
    fn the_installation_scope_comes_from_the_app_path() {
        let user = format!("/home/deck/.local/share/flatpak/app/{ID}/x86_64/stable/{A}/files");
        assert_eq!(install_scope(&info("stable", &user)), Scope::User);
        assert_eq!(install_scope(&info("stable", &format!("/var/lib/flatpak/app/{ID}/x86_64/stable/{A}/files"))), Scope::System);
        assert_eq!(install_scope(&info("stable", &format!("/run/media/deck/SD/flatpak/app/{ID}/x86_64/stable/{A}/files"))), Scope::Other);
        assert_eq!(install_scope(&info("stable", "/var/lib/flatpakevil/app")), Scope::Other, "only the real system folder counts");
        assert_eq!(install_scope("[Instance]\nbranch=stable\n"), Scope::Other);
        assert_eq!(install_scope(""), Scope::Other);
    }

    #[test]
    fn only_a_matching_stable_package_offers_updates() {
        let user = "/home/u/.local/share/flatpak/app/org.example.App/x86_64/stable/c/files";
        assert_eq!(eligibility(Some(&info("stable", user)), Channel::Stable, ID), Eligibility::Eligible(Scope::User));
        assert_eq!(eligibility(None, Channel::Stable, ID), Eligibility::NotFlatpak);
        // a former single-file bundle (branch local) has no update source
        assert!(matches!(eligibility(Some(&info("local", user)), Channel::Stable, ID), Eligibility::NoUpdateSource(_)));
        // a branch that does not belong to the build, or another application
        assert!(matches!(eligibility(Some(&info("testing", user)), Channel::Stable, ID), Eligibility::WrongIdentity(_)));
        assert!(matches!(eligibility(Some(&info("stable", user)), Channel::Stable, "org.example.Other"), Eligibility::WrongIdentity(_)));
        // a testing package is handed out by hand: no portal update, whatever its branch says
        assert!(matches!(eligibility(Some(&info("testing", user)), Channel::Testing, ID), Eligibility::NoUpdateSource(_)));
        assert!(matches!(eligibility(Some(&info("local", user)), Channel::Testing, ID), Eligibility::NoUpdateSource(_)));
    }

    fn progress(status: u32, percent: u32) -> Event {
        Event::Progress { status, percent, error: String::new(), message: String::new() }
    }

    #[test]
    fn availability_means_a_newer_remote_and_restart_means_an_installed_one() {
        let idle = Phase::Idle;
        assert_eq!(reduce(&idle, &Event::Available { running: A.into(), local: A.into(), remote: B.into() }), Phase::Available);
        assert_eq!(reduce(&idle, &Event::Available { running: A.into(), local: A.into(), remote: A.into() }), Phase::Idle, "nothing new");
        // the portal reports an installed update that is not running yet
        assert_eq!(reduce(&idle, &Event::Available { running: A.into(), local: B.into(), remote: B.into() }), Phase::RestartNeeded);
        assert_eq!(reduce(&Phase::Available, &Event::Available { running: A.into(), local: B.into(), remote: B.into() }), Phase::RestartNeeded);
        // a running installation is not disturbed by a repeated signal
        assert_eq!(reduce(&Phase::Working(40), &Event::Available { running: A.into(), local: A.into(), remote: B.into() }), Phase::Working(40));
        // unusable signals change nothing
        for bad in ["", "x", &"g".repeat(64), &"a".repeat(63)] {
            assert_eq!(reduce(&Phase::Available, &Event::Available { running: A.into(), local: A.into(), remote: bad.into() }), Phase::Available, "{bad:?}");
        }
    }

    #[test]
    fn progress_follows_the_portal_statuses() {
        assert_eq!(reduce(&Phase::Available, &Event::Requested), Phase::Working(0));
        assert_eq!(reduce(&Phase::Working(0), &progress(0, 55)), Phase::Working(55));
        assert_eq!(reduce(&Phase::Working(0), &progress(0, 500)), Phase::Working(100), "percent never exceeds 100");
        assert_eq!(reduce(&Phase::Working(100), &progress(2, 100)), Phase::RestartNeeded);
        assert_eq!(reduce(&Phase::Working(0), &progress(1, 0)), Phase::NothingToInstall);
        assert_eq!(reduce(&Phase::Working(0), &progress(9, 0)), Phase::Working(0), "an unknown status changes nothing");
        // pressing the button twice, or after the install, does not restart anything
        assert_eq!(reduce(&Phase::Working(30), &Event::Requested), Phase::Working(30));
        assert_eq!(reduce(&Phase::RestartNeeded, &Event::Requested), Phase::RestartNeeded);
        // a second try after a refusal is possible
        assert_eq!(reduce(&Phase::NothingToInstall, &Event::Requested), Phase::Working(0));
        assert_eq!(reduce(&Phase::Failed(Failure::Network), &Event::Requested), Phase::Working(0));
    }

    #[test]
    fn failures_are_classified_coarsely_and_never_carry_control_characters() {
        let f = |e: &str, m: &str| reduce(&Phase::Working(0), &Event::Progress { status: 3, percent: 0, error: e.into(), message: m.into() });
        assert_eq!(f("org.freedesktop.DBus.Error.AccessDenied", "Authorization required"), Phase::Failed(Failure::NotAllowed));
        assert_eq!(f("", "Not allowed to update"), Phase::Failed(Failure::NotAllowed));
        // what the real portal reported when the update was denied (Flatpak 1.18.4, German locale): the error NAME decides, the localised text does not matter
        assert_eq!(f("org.freedesktop.DBus.Error.AccessDenied", "Anwendungsaktualisierung nicht erlaubt"), Phase::Failed(Failure::NotAllowed));
        assert_eq!(f("org.freedesktop.Flatpak.Error", "Unable to connect to host: Could not resolve host"), Phase::Failed(Failure::Network));
        match f("x", "weird\u{7}\nproblem") {
            Phase::Failed(Failure::Other(t)) => assert!(!t.chars().any(char::is_control) && t.contains("weird"), "{t:?}"),
            other => panic!("{other:?}"),
        }
        match f("x", &"y".repeat(1000)) {
            Phase::Failed(Failure::Other(t)) => assert_eq!(t.len(), 200),
            other => panic!("{other:?}"),
        }
        assert_eq!(reduce(&Phase::Available, &Event::CallFailed("no such interface".into())), Phase::Failed(Failure::Other("no such interface".into())));
    }
}
