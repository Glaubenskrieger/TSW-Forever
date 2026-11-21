use super::fixtures::*;
use super::*;
use std::cell::RefCell;

const NOW: u64 = 1_796_169_600; // 2026-12-02T00:00:00Z, one day after the fixtures were issued

fn ctx<'a>(build: Channel, feed: Channel, key: &'a str, installed: &'a str, now: u64) -> Context<'a> {
    Context { build, feed, key, repo_url: REPO, installed, now }
}

fn stable<'a>(installed: &'a str) -> Context<'a> {
    ctx(Channel::Stable, Channel::Stable, PUBKEY, installed, NOW)
}

fn tmp(tag: &str) -> std::path::PathBuf {
    let d = std::env::temp_dir().join(format!("tsw-update-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

// ---- versions and time ----------------------------------------------------------------------------------------------

#[test]
fn only_plain_numeric_versions_exist_and_they_compare_numerically() {
    assert_eq!(Version::parse("0.3.5"), Some(Version(0, 3, 5)));
    assert!(Version::parse("0.10.0") > Version::parse("0.9.0"), "numeric, not textual");
    assert!(Version::parse("1.0.0") > Version::parse("0.99.99"));
    for bad in ["", "1", "1.2", "01.2.3", "1.02.3", "1.2.03", "1.2.3.4", "1.2.3-rc.1", "1.2.3+b", "v1.2.3", " 1.2.3", "1.2.3 ", "1..3", "a.b.c", "1.2.-3", "9999999.0.0", "1.2.3\n"] {
        assert_eq!(Version::parse(bad), None, "{bad:?}");
    }
    assert_eq!(Version(0, 3, 5).to_string(), "0.3.5");
}

#[test]
fn utc_times_are_exact_and_impossible_dates_are_refused() {
    assert_eq!(parse_utc("1970-01-01T00:00:00Z"), Some(0));
    assert_eq!(parse_utc("2000-02-29T23:59:59Z"), Some(951_868_799), "a leap day");
    assert_eq!(parse_utc("2026-12-01T10:00:00Z"), Some(1_796_119_200));
    assert_eq!(parse_utc("2027-03-01T00:00:00Z"), Some(1_803_859_200));
    for bad in [
        "",
        "2026-12-01",
        "2026-12-01 10:00:00Z",
        "2026-12-01T10:00:00",
        "2026-12-01T10:00:00+01:00",
        "2026-13-01T10:00:00Z",
        "2026-00-01T10:00:00Z",
        "2026-02-29T10:00:00Z",
        "2026-04-31T10:00:00Z",
        "2026-12-01T24:00:00Z",
        "2026-12-01T10:60:00Z",
        "2026-12-01T10:00:60Z",
        "1969-12-31T23:59:59Z",
        "2026-12-01T10:00:00z",
        "+026-12-01T10:00:00Z",
        "2026-12-01T10:00:00Z ",
        "２０２６-12-01T10:00:00Z",
    ] {
        assert_eq!(parse_utc(bad), None, "{bad:?}");
    }
}

// ---- the format -----------------------------------------------------------------------------------------------------

#[test]
fn the_good_metadata_parses_completely() {
    let m = parse(GOOD_META).unwrap();
    assert_eq!((m.channel.as_str(), m.version.as_str()), ("stable", "0.3.5"));
    assert_eq!((m.issued, m.expires), (1_796_119_200, 1_803_859_200));
    assert_eq!(m.windows.size, INSTALLER_BYTES.len() as u64);
    assert_eq!(m.windows.name, "tsw-forever-0.3.5-windows-setup.exe");
    let (pv, p) = m.previous.unwrap();
    assert_eq!((pv.as_str(), p.size), ("0.3.4", PREVIOUS_BYTES.len() as u64));
    assert!(m.notes_url.unwrap().ends_with("/releases/tag/v0.3.5") && m.flatpak_ref.unwrap().ends_with("/stable"));
}

#[test]
fn a_malformed_text_is_refused_in_every_way_it_can_be_malformed() {
    let good = GOOD_META.to_string();
    let cases: Vec<(&str, String)> = vec![
        ("wrong header", good.replacen("tsw-forever-update 1", "tsw-forever-update 2", 1)),
        ("no header", good.replacen("tsw-forever-update 1\n", "", 1)),
        ("header with a trailing space", good.replacen("update 1\n", "update 1 \n", 1)),
        ("CRLF", good.replace('\n', "\r\n")),
        ("duplicate key", format!("{good}channel: stable\n")),
        ("unknown key", format!("{good}extra: 1\n")),
        ("missing key", good.replace("expires: 2027-03-01T00:00:00Z\n", "")),
        ("no space after the colon", good.replacen("channel: stable", "channel:stable", 1)),
        ("two spaces", good.replacen("channel: stable", "channel:  stable", 1)),
        ("empty value", good.replacen("version: 0.3.5", "version: ", 1)),
        ("upper-case key", good.replacen("channel:", "Channel:", 1)),
        ("non-ASCII", good.replacen("stable", "stäble", 1)),
        ("control character", good.replacen("stable", "sta\u{7}ble", 1)),
        ("tab", good.replacen("channel: stable", "channel:\tstable", 1)),
        ("size not a number", good.replacen(&format!("windows-size: {}", INSTALLER_BYTES.len()), "windows-size: 12x", 1)),
        ("size with a sign", good.replacen(&format!("windows-size: {}", INSTALLER_BYTES.len()), "windows-size: -5", 1)),
        ("bad time", good.replacen("issued: 2026-12-01T10:00:00Z", "issued: yesterday", 1)),
        ("partial previous", good.replacen("previous-version: 0.3.4\n", "", 1)),
    ];
    for (what, text) in cases {
        assert!(matches!(parse(&text), Err(UpdateError::BadFormat(_) | UpdateError::BadDigest(_))), "{what} must be refused");
    }
    assert_eq!(parse(&"x".repeat(MAX_METADATA + 1)), Err(UpdateError::TooLarge));
    // a text without the optional parts is fine
    let minimal: String = GOOD_META.lines().filter(|l| !l.starts_with("previous-") && !l.starts_with("notes-") && !l.starts_with("flatpak-")).map(|l| format!("{l}\n")).collect();
    let m = parse(&minimal).unwrap();
    assert!(m.previous.is_none() && m.notes_url.is_none() && m.flatpak_ref.is_none());
}

// ---- signature and trust --------------------------------------------------------------------------------------------

#[test]
fn a_correctly_signed_stable_offer_for_an_older_installation_is_accepted() {
    let o = verify_and_validate(GOOD_META.as_bytes(), GOOD_SIG, &stable("0.3.1")).unwrap();
    assert_eq!((o.channel, o.use_as, o.version.as_str()), (Channel::Stable, OfferUse::Update, "0.3.5"));
    assert_eq!(o.installer.sha256, tsw_patch::sha256::sha256_hex(INSTALLER_BYTES));
    assert_eq!(o.previous.as_ref().unwrap().0, "0.3.4");
    assert_eq!(o.expires, 1_803_859_200);
}

#[test]
fn any_change_of_the_signed_text_breaks_the_signature() {
    let c = stable("0.3.1");
    let tampered: Vec<(&str, String)> = vec![
        ("version", GOOD_META.replacen("version: 0.3.5", "version: 0.9.9", 1)),
        ("channel", GOOD_META.replacen("channel: stable", "channel: testing", 1)),
        ("digest", GOOD_META.replacen("windows-sha256: ", "windows-sha256: 0", 1)),
        ("url", GOOD_META.replacen("github.com", "github.co", 1)),
        ("expiry", GOOD_META.replacen("2027-03-01", "2037-03-01", 1)),
        ("appended line", format!("{GOOD_META}notes-url: https://evil.example.com/\n")),
        ("trailing newline removed", GOOD_META.trim_end().to_string()),
        ("truncated", GOOD_META[..GOOD_META.len() / 2].to_string()),
        ("empty", String::new()),
    ];
    for (what, text) in tampered {
        assert!(matches!(verify_and_validate(text.as_bytes(), GOOD_SIG, &c), Err(UpdateError::BadSignature(_))), "{what}");
    }
}

#[test]
fn only_the_trusted_key_counts() {
    let c = stable("0.3.1");
    // signed by another key
    assert!(matches!(verify_and_validate(GOOD_META.as_bytes(), BY_OTHER_SIG, &c), Err(UpdateError::BadSignature(_))));
    // the right signature checked against another trusted key
    let other = ctx(Channel::Stable, Channel::Stable, OTHER_PUBKEY, "0.3.1", NOW);
    assert!(matches!(verify_and_validate(GOOD_META.as_bytes(), GOOD_SIG, &other), Err(UpdateError::BadSignature(_))));
    // garbage and empty signatures, a signature of another text
    for sig in ["", "x", "untrusted comment: x\nnot-base64\n", TESTING_SIG] {
        assert!(matches!(verify_and_validate(GOOD_META.as_bytes(), sig, &c), Err(UpdateError::BadSignature(_))), "{sig:?}");
    }
    // a malformed key is not "no key": it is refused as a bad key, and nothing is accepted
    let broken = ctx(Channel::Stable, Channel::Stable, "not-a-key", "0.3.1", NOW);
    assert!(matches!(verify_and_validate(GOOD_META.as_bytes(), GOOD_SIG, &broken), Err(UpdateError::BadSignature(_))));
}

#[test]
fn without_a_compiled_in_key_the_check_is_off() {
    let off = ctx(Channel::Stable, Channel::Stable, "", "0.3.1", NOW);
    assert_eq!(verify_and_validate(GOOD_META.as_bytes(), GOOD_SIG, &off), Err(UpdateError::NoTrustedKey));
    let spaces = ctx(Channel::Stable, Channel::Stable, "  \n", "0.3.1", NOW);
    assert_eq!(verify_and_validate(GOOD_META.as_bytes(), GOOD_SIG, &spaces), Err(UpdateError::NoTrustedKey));
    assert_eq!(crate::channel::STABLE_UPDATE_KEY, "", "the real key does not exist yet: packaging/channels/stable.conf is empty on purpose");
}

#[test]
fn oversized_metadata_is_not_even_looked_at() {
    let big = vec![b'a'; MAX_METADATA + 1];
    assert_eq!(verify_and_validate(&big, GOOD_SIG, &stable("0.3.1")), Err(UpdateError::TooLarge));
}

// ---- channels --------------------------------------------------------------------------------------------------------

#[test]
fn testing_metadata_never_works_for_a_stable_build() {
    // validly signed testing metadata fetched as if it were the stable feed: refused by the channel inside the signed text
    let e = verify_and_validate(TESTING_META.as_bytes(), TESTING_SIG, &stable("0.3.1")).unwrap_err();
    assert_eq!(e, UpdateError::WrongChannel { expected: "stable", found: "testing".into() });
    // and if the stable build were ever pointed at a testing feed, the build rule refuses it before anything else
    let as_testing_feed = ctx(Channel::Stable, Channel::Testing, PUBKEY, "0.3.1", NOW);
    assert_eq!(verify_and_validate(TESTING_META.as_bytes(), TESTING_SIG, &as_testing_feed), Err(UpdateError::ChannelNotAllowed));
    assert_eq!(verify_and_validate(GOOD_META.as_bytes(), GOOD_SIG, &as_testing_feed), Err(UpdateError::ChannelNotAllowed));
}

#[test]
fn a_testing_build_updates_inside_testing_and_is_only_offered_a_switch_to_stable() {
    let t = ctx(Channel::Testing, Channel::Testing, PUBKEY, "0.3.1", NOW);
    let o = verify_and_validate(TESTING_META.as_bytes(), TESTING_SIG, &t).unwrap();
    assert_eq!((o.channel, o.use_as), (Channel::Testing, OfferUse::Update));
    // the stable feed seen from a testing build is a choice, never an update
    let s = ctx(Channel::Testing, Channel::Stable, PUBKEY, "0.3.1", NOW);
    let o = verify_and_validate(GOOD_META.as_bytes(), GOOD_SIG, &s).unwrap();
    assert_eq!((o.channel, o.use_as), (Channel::Stable, OfferUse::SwitchToStable));
    // testing metadata read through the stable feed is still refused by its own channel field
    assert!(matches!(verify_and_validate(TESTING_META.as_bytes(), TESTING_SIG, &s), Err(UpdateError::WrongChannel { .. })));
}

#[test]
fn a_testing_build_is_never_pulled_back_to_an_older_stable() {
    let at = |installed: &'static str| ctx(Channel::Testing, Channel::Stable, PUBKEY, installed, NOW);
    assert!(verify_and_validate(GOOD_META.as_bytes(), GOOD_SIG, &at("0.3.5")).is_ok(), "the same version is a possible switch");
    assert!(matches!(verify_and_validate(GOOD_META.as_bytes(), GOOD_SIG, &at("0.3.6")), Err(UpdateError::NotNewer { .. })));
}

// ---- versions: downgrades and repeats ---------------------------------------------------------------------------------------

#[test]
fn only_a_strictly_newer_version_is_an_update() {
    assert!(verify_and_validate(GOOD_META.as_bytes(), GOOD_SIG, &stable("0.3.4")).is_ok());
    assert!(verify_and_validate(GOOD_META.as_bytes(), GOOD_SIG, &stable("0.2.9")).is_ok());
    for installed in ["0.3.5", "0.3.6", "0.4.0", "1.0.0"] {
        assert!(matches!(verify_and_validate(GOOD_META.as_bytes(), GOOD_SIG, &stable(installed)), Err(UpdateError::NotNewer { .. })), "{installed}");
    }
    // an installed version that is not a plain version is never compared by guessing
    for installed in ["", "0.3.5-rc.1", "dev", "0.3"] {
        assert!(matches!(verify_and_validate(GOOD_META.as_bytes(), GOOD_SIG, &stable(installed)), Err(UpdateError::BadFormat(_))), "{installed:?}");
    }
}

// ---- time ------------------------------------------------------------------------------------------------------------------

#[test]
fn expired_not_yet_valid_and_unreasonable_validity_windows_are_refused() {
    let at = |now: u64| ctx(Channel::Stable, Channel::Stable, PUBKEY, "0.3.1", now);
    assert_eq!(verify_and_validate(EXPIRED_META.as_bytes(), EXPIRED_SIG, &stable("0.3.1")), Err(UpdateError::Expired));
    let expires = 1_803_859_200;
    assert!(verify_and_validate(GOOD_META.as_bytes(), GOOD_SIG, &at(expires - 1)).is_ok(), "valid until the last second");
    assert_eq!(verify_and_validate(GOOD_META.as_bytes(), GOOD_SIG, &at(expires)), Err(UpdateError::Expired), "the expiry instant itself is already over");
    assert_eq!(verify_and_validate(GOOD_META.as_bytes(), GOOD_SIG, &at(expires + 86_400 * 365)), Err(UpdateError::Expired));
    let issued = 1_796_119_200;
    assert!(verify_and_validate(GOOD_META.as_bytes(), GOOD_SIG, &at(issued - 3_600)).is_ok(), "a clock that is an hour behind is tolerated");
    assert_eq!(verify_and_validate(GOOD_META.as_bytes(), GOOD_SIG, &at(issued - 3 * 86_400)), Err(UpdateError::NotYetValid), "three days behind is not");
    assert!(matches!(verify_and_validate(LONG_META.as_bytes(), LONG_SIG, &stable("0.3.1")), Err(UpdateError::BadValidity(_))), "a signed statement that lives for years");
}

// ---- addresses and checksums -------------------------------------------------------------------------------------------------

#[test]
fn only_the_official_release_file_of_the_signed_version_is_an_allowed_address() {
    let ok = format!("{REPO}/releases/download/v0.3.5/tsw-forever-0.3.5-windows-setup.exe");
    assert!(url_ok(&ok, REPO, "0.3.5", "tsw-forever-0.3.5-windows-setup.exe").is_ok());
    let name = "tsw-forever-0.3.5-windows-setup.exe";
    let bad: Vec<String> = vec![
        ok.replacen("https", "http", 1),
        ok.replacen("https://", "ftp://", 1),
        ok.replacen("https://", "https://user@", 1),
        ok.replacen("https://", &["https://", "user", ":", "word", "@"].concat(), 1), // userinfo with a password, assembled so that no source line looks like a credential
        ok.replacen("github.com", "github.com:8443", 1),
        ok.replacen("github.com", "evil.example.com", 1),
        ok.replacen("github.com", "github.com.evil.example.com", 1),
        ok.replacen("Glaubenskrieger", "someone-else", 1),
        ok.replacen("TSW-Forever", "TSW-Forever-Dev", 1),
        format!("{ok}?x=1"),
        format!("{ok}#frag"),
        format!("{ok}%2e"),
        ok.replacen("/releases/", "/releases/../", 1),
        ok.replacen("/v0.3.5/", "/v0.3.4/", 1),
        ok.replacen("/v0.3.5/", "/latest/", 1),
        ok.replace('/', "\\"),
        ok.replacen("setup.exe", "setup.exe ", 1),
        ok.replacen("download", "Download", 1),
        ok.replacen("/releases", "//releases", 1),
        String::new(),
        "https://github.com/Glaubenskrieger/TSW-Forever/releases/download/v0.3.5/tsw-forever-0.3.5-windows-setup.exe\n".into(),
        ok.replacen("tsw-forever", "ts\u{fffd}w-forever", 1),
    ];
    for u in &bad {
        assert!(matches!(url_ok(u, REPO, "0.3.5", name), Err(UpdateError::UrlNotAllowed(_))), "{u:?}");
    }
    // the file name in the metadata must be the one in the address, and plain
    assert!(url_ok(&ok, REPO, "0.3.5", "other.exe").is_err());
    assert!(url_ok(&ok, REPO, "0.3.6", name).is_err(), "the directory names the version");
    assert!(url_ok(&ok, "http://github.com/Glaubenskrieger/TSW-Forever", "0.3.5", name).is_err(), "an unusable repository address of the build");
    assert!(url_ok(&ok, &format!("{REPO}/"), "0.3.5", name).is_err());
}

#[test]
fn a_signed_offer_with_a_foreign_address_is_refused() {
    assert!(matches!(verify_and_validate(EVIL_URL_META.as_bytes(), EVIL_URL_SIG, &stable("0.3.1")), Err(UpdateError::UrlNotAllowed(_))));
    // the same text is fine when the build's own repository is the one in it
    let evil_repo = ctx(Channel::Stable, Channel::Stable, PUBKEY, "0.3.1", NOW);
    let evil_repo = Context { repo_url: "https://evil.example.com/Glaubenskrieger/TSW-Forever", ..evil_repo };
    assert!(verify_and_validate(EVIL_URL_META.as_bytes(), EVIL_URL_SIG, &evil_repo).is_ok(), "the allowed address comes from the build, not from the metadata");
}

#[test]
fn the_recovery_version_must_be_older_and_equally_trusted() {
    assert!(matches!(verify_and_validate(BAD_PREV_META.as_bytes(), BAD_PREV_SIG, &stable("0.3.1")), Err(UpdateError::BadFormat(_))), "previous newer than version");
}

#[test]
fn digests_and_sizes_must_have_the_signed_shape() {
    let i = |sha: &str, size: u64| Installer {
        name: "tsw-forever-0.3.5-windows-setup.exe".into(),
        url: format!("{REPO}/releases/download/v0.3.5/tsw-forever-0.3.5-windows-setup.exe"),
        size,
        sha256: sha.into(),
    };
    let good = "a".repeat(64);
    assert!(installer_ok(&i(&good, 100), REPO, "0.3.5").is_ok());
    for sha in ["", "a", &"A".repeat(64), &"g".repeat(64), &"a".repeat(63), &"a".repeat(65), &format!("{} ", "a".repeat(63))] {
        assert!(matches!(installer_ok(&i(sha, 100), REPO, "0.3.5"), Err(UpdateError::BadDigest(_))), "{sha:?}");
    }
    assert!(installer_ok(&i(&good, 0), REPO, "0.3.5").is_err());
    assert!(installer_ok(&i(&good, MAX_INSTALLER + 1), REPO, "0.3.5").is_err());
    assert!(installer_ok(&i(&good, MAX_INSTALLER), REPO, "0.3.5").is_ok());
}

// ---- the downloaded file ------------------------------------------------------------------------------------------------------

#[test]
fn the_download_must_have_exactly_the_signed_size_and_digest() {
    let o = verify_and_validate(GOOD_META.as_bytes(), GOOD_SIG, &stable("0.3.1")).unwrap();
    let d = tmp("download");
    let f = d.join("setup.exe");
    std::fs::write(&f, INSTALLER_BYTES).unwrap();
    assert_eq!(verify_installer_file(&f, &o.installer), Ok(()));
    // one byte different, same size
    let mut changed = INSTALLER_BYTES.to_vec();
    changed[0] ^= 1;
    std::fs::write(&f, &changed).unwrap();
    assert_eq!(verify_installer_file(&f, &o.installer), Err(UpdateError::HashMismatch));
    // truncated and extended
    std::fs::write(&f, &INSTALLER_BYTES[..10]).unwrap();
    assert_eq!(verify_installer_file(&f, &o.installer), Err(UpdateError::SizeMismatch));
    std::fs::write(&f, [INSTALLER_BYTES, b"x"].concat()).unwrap();
    assert_eq!(verify_installer_file(&f, &o.installer), Err(UpdateError::SizeMismatch));
    // missing and empty
    assert!(matches!(verify_installer_file(&d.join("missing.exe"), &o.installer), Err(UpdateError::BadDigest(_))));
    std::fs::write(&f, b"").unwrap();
    assert_eq!(verify_installer_file(&f, &o.installer), Err(UpdateError::SizeMismatch));
    // the recovery installer is checked against its own signed values
    let (_, prev) = o.previous.unwrap();
    std::fs::write(&f, PREVIOUS_BYTES).unwrap();
    assert_eq!(verify_installer_file(&f, &prev), Ok(()));
    assert_eq!(verify_installer_file(&f, &o.installer), Err(UpdateError::SizeMismatch));
    let _ = std::fs::remove_dir_all(&d);
}

// ---- the whole check with a fake feed -----------------------------------------------------------------------------------------

struct Fake {
    calls: RefCell<Vec<String>>,
    meta: Result<Vec<u8>, String>,
    sig: Result<Vec<u8>, String>,
}

impl Fake {
    fn ok(meta: &str, sig: &str) -> Fake {
        Fake { calls: RefCell::new(vec![]), meta: Ok(meta.as_bytes().to_vec()), sig: Ok(sig.as_bytes().to_vec()) }
    }
}

impl Feed for Fake {
    fn fetch(&self, url: &str, max: usize) -> Result<Vec<u8>, String> {
        self.calls.borrow_mut().push(url.to_string());
        let r = if url.ends_with(".minisig") { &self.sig } else { &self.meta };
        match r {
            Ok(b) if b.len() > max => Err("the answer is larger than allowed".into()),
            other => other.clone(),
        }
    }
}

#[test]
fn the_feed_addresses_are_fixed_by_the_build_and_the_channel() {
    assert_eq!(feed_url(REPO, Channel::Stable, false), format!("{REPO}/releases/latest/download/update-stable.txt"));
    assert_eq!(feed_url(REPO, Channel::Stable, true), format!("{REPO}/releases/latest/download/update-stable.txt.minisig"));
    let f = Fake::ok(GOOD_META, GOOD_SIG);
    check(&f, &stable("0.3.1"));
    assert_eq!(*f.calls.borrow(), vec![feed_url(REPO, Channel::Stable, false), feed_url(REPO, Channel::Stable, true)]);
}

#[test]
fn a_valid_newer_offer_is_found_and_the_rest_is_ordinary() {
    assert!(matches!(check(&Fake::ok(GOOD_META, GOOD_SIG), &stable("0.3.1")), Outcome::Offer(o) if o.version == "0.3.5"));
    assert_eq!(check(&Fake::ok(GOOD_META, GOOD_SIG), &stable("0.3.5")), Outcome::UpToDate);
    assert_eq!(check(&Fake::ok(GOOD_META, GOOD_SIG), &stable("0.4.0")), Outcome::UpToDate, "a newer installation than the feed is not an error");
}

#[test]
fn being_offline_or_getting_garbage_changes_nothing_for_the_user() {
    let offline = Fake { calls: RefCell::new(vec![]), meta: Err("Could not resolve host".into()), sig: Err("Could not resolve host".into()) };
    assert!(matches!(check(&offline, &stable("0.3.1")), Outcome::Unknown(e) if e.contains("resolve")));
    let no_sig = Fake { calls: RefCell::new(vec![]), meta: Ok(GOOD_META.into()), sig: Err("404".into()) };
    assert!(matches!(check(&no_sig, &stable("0.3.1")), Outcome::Unknown(_)));
    assert!(matches!(check(&Fake::ok("<html>captive portal</html>", GOOD_SIG), &stable("0.3.1")), Outcome::Rejected(UpdateError::BadSignature(_))));
    assert!(matches!(check(&Fake::ok(GOOD_META, "garbage"), &stable("0.3.1")), Outcome::Rejected(UpdateError::BadSignature(_))));
    assert!(matches!(check(&Fake::ok(&"x".repeat(MAX_METADATA + 100), GOOD_SIG), &stable("0.3.1")), Outcome::Unknown(_)), "an oversized answer is cut by the feed limit");
    assert!(matches!(check(&Fake::ok(GOOD_META, GOOD_SIG), &ctx(Channel::Stable, Channel::Stable, PUBKEY, "0.3.1", NOW + 86_400 * 400)), Outcome::Rejected(UpdateError::Expired)));
}

#[test]
fn no_request_is_made_without_a_key_or_for_a_channel_the_build_must_not_use() {
    let f = Fake::ok(GOOD_META, GOOD_SIG);
    let off = ctx(Channel::Stable, Channel::Stable, "", "0.3.1", NOW);
    assert!(matches!(check(&f, &off), Outcome::Inactive(_)));
    let forbidden = ctx(Channel::Stable, Channel::Testing, PUBKEY, "0.3.1", NOW);
    assert!(matches!(check(&f, &forbidden), Outcome::Inactive(_)));
    assert!(f.calls.borrow().is_empty(), "nothing was fetched: {:?}", f.calls.borrow());
    // the testing build may look at both channels
    let t = ctx(Channel::Testing, Channel::Stable, PUBKEY, "0.3.1", NOW);
    assert!(matches!(check(&f, &t), Outcome::Offer(o) if o.use_as == OfferUse::SwitchToStable));
}

#[test]
fn the_curl_feed_refuses_addresses_before_it_starts_anything() {
    let feed = CurlFeed { curl: std::path::PathBuf::from("definitely-not-a-program-tsw") };
    for url in ["http://github.com/x", "ftp://github.com/x", "https://github.com/x y", "https://github.com/x?y=1", "https://user@github.com/x", "file:///etc/passwd", "", "https://github.com/\u{e4}"] {
        assert_eq!(feed.fetch(url, 100), Err("invalid update address".into()), "{url:?}");
    }
    // a valid address with a program that does not exist: an ordinary error, no panic
    let e = feed.fetch("https://github.com/Glaubenskrieger/TSW-Forever/releases/latest/download/update-stable.txt", 100).unwrap_err();
    assert!(e.contains("cannot start curl"), "{e}");
}
