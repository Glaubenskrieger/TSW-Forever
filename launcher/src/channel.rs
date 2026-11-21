//! Update channel identity and the rules between channels.
//!
//! The channel of a program is fixed when it is BUILT (see `build.rs`): `stable` is the default, `testing` needs the private overlay file. There is no
//! setting that changes it. The rules, as a table (build = the installed program, offer = the channel an update offer comes from):
//!
//! | build   | offer from stable                         | offer from testing |
//! |---------|-------------------------------------------|--------------------|
//! | stable  | update                                    | never considered   |
//! | testing | only as an explicit, user-approved switch | update             |
//!
//! A stable build never fetches or accepts anything from testing; a testing build never moves to stable by itself.

/// An update channel.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Channel {
    Stable,
    Testing,
}

impl Channel {
    pub fn name(self) -> &'static str {
        match self {
            Channel::Stable => "stable",
            Channel::Testing => "testing",
        }
    }

    pub fn parse(text: &str) -> Option<Channel> {
        match text {
            "stable" => Some(Channel::Stable),
            "testing" => Some(Channel::Testing),
            _ => None,
        }
    }
}

/// What a program may do with an offer from some channel.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OfferUse {
    /// A normal update inside the program's own channel.
    Update,
    /// A testing program was offered the stable version: shown only as a choice the user must confirm; never installed by itself.
    SwitchToStable,
}

/// The only place that decides which channels a program of channel `build` may consult. `None` = the offer must not even be looked at.
pub fn use_of_offer(build: Channel, offer: Channel) -> Option<OfferUse> {
    match (build, offer) {
        (Channel::Stable, Channel::Stable) | (Channel::Testing, Channel::Testing) => Some(OfferUse::Update),
        (Channel::Testing, Channel::Stable) => Some(OfferUse::SwitchToStable),
        (Channel::Stable, Channel::Testing) => None,
    }
}

/// The channel this program was built for.
pub fn build() -> Channel {
    Channel::parse(env!("TSW_CHANNEL")).expect("build.rs only accepts stable or testing")
}

/// The application ID of this build (the Flatpak ID): the stable ID, or the separate testing ID in a testing build.
pub const APP_ID: &str = env!("TSW_CHANNEL_APP_ID");

/// The base64 line of the stable update public key compiled into this build (empty = update check disabled, see `packaging/channels/stable.conf`).
pub const STABLE_UPDATE_KEY: &str = env!("TSW_UPDATE_KEY_STABLE");

/// A string that exists in testing builds only (empty in a stable build). The finished stable binary is scanned for it.
#[cfg(channel_testing)]
#[used]
pub static MARKER: &str = env!("TSW_CHANNEL_MARKER");

/// Marker and application ID of a testing build together, so that both are really in the finished program (a constant nobody uses is dropped).
#[cfg(channel_testing)]
#[used]
pub static IDENTITY: &str = concat!(env!("TSW_CHANNEL_MARKER"), " ", env!("TSW_CHANNEL_APP_ID"));

/// How this installation relates to the build, from `/.flatpak-info`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Identity {
    /// Application ID and branch fit this build.
    Consistent,
    /// Something does not fit (the text says what): updates must stay off and the user gets a warning.
    Mismatch(String),
}

fn flatpak_value<'a>(info: &'a str, section: &str, key: &str) -> Option<&'a str> {
    let mut in_section = false;
    for line in info.lines().map(str::trim) {
        if line.starts_with('[') {
            in_section = line == format!("[{section}]");
            continue;
        }
        if in_section {
            if let Some(v) = line.strip_prefix(key).and_then(|r| r.strip_prefix('=')) {
                return Some(v.trim());
            }
        }
    }
    None
}

/// Compare the facts Flatpak puts into `/.flatpak-info` (the sandbox cannot change them) with the channel the program was built for. `expected_app_id`
/// is this build's [`APP_ID`]. A branch other than the build's own channel name is accepted only if it is `local` (a package built on a developer machine,
/// which is never compared with any update source).
pub fn check_flatpak_identity(info: &str, build: Channel, expected_app_id: &str) -> Identity {
    match flatpak_value(info, "Application", "name") {
        Some(id) if id == expected_app_id => {}
        Some(id) => return Identity::Mismatch(format!("this program was built as {expected_app_id}, but runs as {id}")),
        None => return Identity::Mismatch("the application ID is not known".into()),
    }
    match flatpak_value(info, "Instance", "branch") {
        Some(b) if b == build.name() || b == "local" => Identity::Consistent,
        Some(b) => Identity::Mismatch(format!("this {} program is installed from the branch {b}", build.name())),
        None => Identity::Mismatch("the installed branch is not known".into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_channel_table_is_exactly_the_agreed_one() {
        use Channel::*;
        assert_eq!(use_of_offer(Stable, Stable), Some(OfferUse::Update));
        assert_eq!(use_of_offer(Stable, Testing), None, "stable never considers testing");
        assert_eq!(use_of_offer(Testing, Testing), Some(OfferUse::Update));
        assert_eq!(use_of_offer(Testing, Stable), Some(OfferUse::SwitchToStable), "testing only offers the switch, it never updates itself to stable");
    }

    #[test]
    fn names_round_trip_and_nothing_else_is_a_channel() {
        for c in [Channel::Stable, Channel::Testing] {
            assert_eq!(Channel::parse(c.name()), Some(c));
        }
        for bad in ["", "Stable", "STABLE", "beta", "stable ", "stable\n", "nightly", "testing2"] {
            assert_eq!(Channel::parse(bad), None, "{bad:?}");
        }
    }

    #[test]
    fn this_build_has_a_consistent_identity() {
        let c = build();
        #[cfg(channel_testing)]
        {
            assert_eq!(c, Channel::Testing);
            assert!(APP_ID.ends_with(".Testing") && !MARKER.is_empty());
        }
        #[cfg(not(channel_testing))]
        {
            assert_eq!(c, Channel::Stable);
            assert_eq!(APP_ID, crate::project::APP_ID, "a stable build uses the stable application ID of project.conf");
        }
    }

    fn info(app: &str, branch: &str) -> String {
        format!("[Application]\nname={app}\nruntime=runtime/org.freedesktop.Platform/x86_64/25.08\n\n[Instance]\nbranch={branch}\napp-commit={}\n", "a".repeat(64))
    }

    #[test]
    fn the_flatpak_identity_must_fit_the_build() {
        let id = "org.example.App";
        assert_eq!(check_flatpak_identity(&info(id, "stable"), Channel::Stable, id), Identity::Consistent);
        assert_eq!(check_flatpak_identity(&info(id, "local"), Channel::Stable, id), Identity::Consistent, "a local build is allowed");
        assert_eq!(check_flatpak_identity(&info(id, "testing"), Channel::Testing, id), Identity::Consistent);
        for (app, branch, build) in
            [("org.example.Other", "stable", Channel::Stable), (id, "testing", Channel::Stable), (id, "stable", Channel::Testing), (id, "beta", Channel::Stable), (id, "", Channel::Stable)]
        {
            assert!(matches!(check_flatpak_identity(&info(app, branch), build, id), Identity::Mismatch(_)), "{app} {branch}");
        }
        assert!(matches!(check_flatpak_identity("", Channel::Stable, id), Identity::Mismatch(_)));
        // the keys must come from their own sections: a value in the wrong section does not count
        assert!(matches!(check_flatpak_identity("[Other]\nname=org.example.App\nbranch=stable\n", Channel::Stable, id), Identity::Mismatch(_)));
    }
}
