//! Guards for the two ways the launcher learns about an update on Linux. The Flatpak update portal (`flatpak_update.rs`) is the only way that installs
//! anything. The detect-only fallback (`launcher/src/selfupdate.rs`) shows a hint and must stay that: it may never start an installation, and it may only be
//! reached for a stable Flatpak that has an update source. These are source-level checks, so a later change that adds a second mechanism fails here first.

const SELFUPDATE: &str = include_str!("../../launcher/src/selfupdate.rs");
const MAIN: &str = include_str!("../src/main.rs");

fn non_test_part(src: &str) -> &str {
    src.split("#[cfg(test)]").next().unwrap()
}

#[test]
fn the_fallback_only_asks_for_a_commit_over_https_and_never_installs_or_runs_flatpak() {
    let code = non_test_part(SELFUPDATE);
    assert_eq!(code.matches("Command::new(").count(), 1, "the fallback may start exactly one program: curl, to read a published checksum");
    for forbidden in ["\"flatpak\"", "\"update\"", "\"install\"", "\"--commit", "org.freedesktop.portal", "std::fs::write", "File::create", "remove_file", "rename("] {
        assert!(!code.contains(forbidden), "selfupdate.rs must not contain {forbidden}: it only detects");
    }
    assert!(code.contains("\"=https\""), "the request is limited to https");
    assert!(code.contains("--proto-redir"), "redirects are limited to https as well");
    assert!(code.contains("--max-filesize"), "the answer is size-limited");
}

#[test]
fn the_fallback_is_reached_only_through_the_eligibility_check_and_only_when_the_portal_cannot_be_started() {
    let at = MAIN.find("tsw_launch::selfupdate::check(").expect("the fallback call exists");
    let function_start = MAIN[..at].rfind("fn start_launcher_update").expect("it lives in start_launcher_update");
    let before = &MAIN[function_start..at];
    // the function returns early unless the build and the installation are eligible (stable build, stable branch, own application id)
    assert!(before.contains("eligibility(info.as_deref(), tsw_launch::channel::build(), tsw_launch::channel::APP_ID)"), "{before}");
    assert!(before.contains("_ => return,"), "an installation that is not eligible must return before anything is asked");
    // and it is the error branch of starting the portal client: the portal remains the primary mechanism
    let portal = before.find("flatpak_update::start(").expect("the portal is tried first");
    let error_arm = before.find("Err(_) =>").expect("the fallback is the error arm of the portal start");
    assert!(portal < error_arm);
    // the fallback compares with the stable repository only, which is what an eligible build is
    assert!(MAIN[at..at + 300].contains("flatpak_ref_for(\"stable\")"));
    // it only sets a message; nothing else is done with the result
    let after = &MAIN[at..at + 900];
    assert!(after.contains("Status::Available"), "{after}");
    assert!(!after.contains("Command::new("), "the answer of the fallback is shown, not acted on");
}

#[test]
fn the_fallback_module_is_used_nowhere_else() {
    let uses = MAIN.matches("selfupdate::").count();
    assert_eq!(uses, 2, "check() and Status::Available in start_launcher_update only");
}
