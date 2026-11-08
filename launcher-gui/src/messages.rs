//! Translatable messages for everything the launcher library reports with a stable code (`Finding::code`, `Problem`,
//! detection notes). The English text here is the message id; it is checked against the library's own English text by the
//! tests, so the two cannot drift apart. A code this table does not know is shown with the library's English text as it is.

use tsw_launch::discover::Problem;
use tsw_launch::doctor::Finding;

use crate::i18n::{Arg, Msg};

/// `(what, hint)` for a finding. `hint` is empty when the finding has none.
pub fn finding(f: &Finding) -> (Msg, Msg) {
    let a = |i: usize| f.args.get(i).cloned().unwrap_or_default();
    let m = |id: &str, idx: &[usize]| Msg::new(id, idx.iter().map(|i| a(*i)).collect::<Vec<String>>());
    let none = Msg::default();
    match f.code {
        "doctor.font_found" | "doctor.x11_found" => (m("32-bit {0} found: {1}", &[0, 1]), none),
        "doctor.font_missing" => (
            m("32-bit {0} not found in {1}", &[0, 1]),
            m("install {0}; without it the 32-bit client has no Wine font engine (AddFontMemResourceEx fails). A runtime that ships 32-bit libraries (Steam Linux Runtime) also avoids this.", &[2]),
        ),
        "doctor.x11_missing" => (m("32-bit {0} not found", &[0]), Msg { id: "install {0}: {1}".into(), args: vec![Arg::Text(a(1)), Arg::Id(a(2))] }),
        "doctor.exe_unreadable" => (m("client executable {0} is not readable: {1}", &[0, 1]), m("give --exe or client_exe a path to TheSecretWorld[DX11].exe", &[])),
        "doctor.exe_not_windows" => (m("{0} is not a Windows executable", &[0]), m("wrong file?", &[])),
        "doctor.exe_ok" => (m("client executable recognised: {0}", &[0]), none),
        "doctor.exe_unknown_build" => (m("{0} is not a recognised client build (patcher-key check not found)", &[0]), m("only TheSecretWorld.exe / TheSecretWorldDX11.exe can be started", &[])),
        "doctor.wine_ok" => (m("wine program found: {0}", &[0]), none),
        "doctor.wine_missing" => (m("wine program {0} not found", &[0]), m("use --runner /path/to/wine (a Proton/GE-Proton 'files/bin/wine') or install wine", &[])),
        "doctor.prefix_ok" => (m("prefix exists: {0}", &[0]), none),
        "doctor.prefix_new" => (m("prefix {0} does not exist yet", &[0]), m("wine will create an empty prefix; the game needs the usual components (DXVK, d3dx, vcrun) installed in it", &[])),
        "doctor.prefix_unset" => (m("no prefix set (wine would use ~/.wine)", &[]), m("pass --prefix or prefix = ... in the config", &[])),
        "doctor.display_ok" => (m("a display is available (DISPLAY or WAYLAND_DISPLAY)", &[]), none),
        "doctor.display_missing" => (m("no DISPLAY / WAYLAND_DISPLAY in the environment", &[]), m("start from a graphical session; the client needs a window", &[])),
        "doctor.vulkan_ok" => (m("a Vulkan driver description (ICD) is installed", &[]), none),
        "doctor.vulkan_missing" => (m("no Vulkan ICD found", &[]), m("DXVK needs a Vulkan driver (vulkan-radeon / nvidia-utils / vulkan-intel)", &[])),
        "doctor.directx_ok" => (m("the Direct3D libraries of the DirectX June 2010 runtime are present", &[]), none),
        "doctor.directx_missing" => (
            Msg { id: "These Direct3D libraries are missing on this computer: {0}".into(), args: vec![Arg::Text(f.args.join(", "))] },
            m("install Microsoft's DirectX End-User Runtimes (June 2010) yourself", &[]),
        ),
        "prefix.override_rejected" => (m("manifest override rejected: {0}", &[0]), m("only the documented graphics DLLs with mode n/b may be overridden; recreate the prefix with `tsw-launch prefix init`", &[])),
        "prefix.native_unverified" => (m("native override for {0} has no verified component in the manifest", &[0]), m("a native override is only valid together with the installed, hash-recorded file", &[])),
        "prefix.component_changed" => (m("{0} changed since the prefix was prepared", &[0]), m("re-run `tsw-launch prefix init` to restore the recorded component", &[])),
        "prefix.component_gone" => (m("{0} from the manifest is missing", &[0]), m("run `tsw-launch prefix init`", &[])),
        "prefix.manifest_ok" => (m("manifest: {0} components unchanged, overrides {1}", &[0, 1]), none),
        "prefix.metrics_none" => (m("prefix has no WindowMetrics yet", &[]), m("run `tsw-launch prefix init` or start Wine once in this prefix", &[])),
        "prefix.metric_ok" => (m("window metric {0}={1}", &[0, 1]), none),
        "prefix.metric_bad" => (
            m("window metric {0}={1} is implausible (expected {2}..={3})", &[0, 1, 2, 3]),
            m("the prefix was created without the 32-bit FreeType/Fontconfig libraries; install them and create a NEW prefix (`tsw-launch prefix init`)", &[]),
        ),
        "prefix.not_initialised" => (m("{0} is not an initialised Wine prefix (no user.reg)", &[0]), m("run `tsw-launch prefix init --prefix <dir> --runner <wine>`", &[])),
        "prefix.runtime_differs" => (m("{0} differs from the runtime's copy", &[0]), m("re-run `tsw-launch prefix init` to refresh the components", &[])),
        "prefix.file_missing" => (m("{0} is missing", &[0]), m("run `tsw-launch prefix init`", &[])),
        "prefix.runtime_unreadable" => (m("runtime component {0} unreadable: {1}", &[0, 1]), m("use a GE-Proton/Proton runtime that ships DXVK and vkd3d", &[])),
        "prefix.no_manifest" => (m("no component manifest: DXVK is not enabled for this prefix", &[]), m("run `tsw-launch prefix init`", &[])),
        "prefix.profile_ok" => (m("compiler profile {0} installed", &[0]), none),
        "prefix.no_profile" => (
            m("no native shader-compiler profile is installed in this prefix", &[]),
            m("the tested configuration uses Microsoft's d3dcompiler_43 and the D3DX libraries; with Wine's built-in compiler character clothing was black in our tests. Install with `tsw-launch prefix native-install --native-dir <folder with the 32-bit DLLs>` (advanced view)", &[]),
        ),
        "gamescope.missing" => (
            m("Gamescope is switched on, but was not found", &[]),
            if a(0) == "flatpak" {
                m("Install the Flatpak extension org.freedesktop.Platform.VulkanLayer.gamescope (matching the runtime branch), or switch Gamescope off: the normal start needs neither.", &[])
            } else {
                m("Install the gamescope package of the distribution (SteamOS and the Steam Machine ship it), or switch Gamescope off: the normal start needs neither.", &[])
            },
        ),
        "gamescope.ready" => (m("Gamescope found ({0}), {1}", &[0, 1]), none),
        "gamescope.nested" => (
            m("this session already runs inside gamescope (Game Mode)", &[]),
            m("Start the game without Gamescope here: Game Mode already provides it.", &[]),
        ),
        "cache.none" => (m("no local shader cache yet (the game creates it on the first run)", &[]), none),
        "cache.unreadable" => (m("local shader cache could not be inspected: {0}", &[0]), m("the game ignores or rebuilds an unreadable cache; nothing was changed", &[])),
        "cache.vkd3d_blobs" => (
            m("the local shader cache holds {0} of {1} shader(s) compiled by Wine's built-in compiler (vkd3d-shader)", &[0, 1]),
            m("reused in every run; in our tests such an entry kept character clothing black until the cache was rebuilt. This is a fact about the file, not proof that a shader is wrong. Rebuild with `tsw-launch cache quarantine` (reversible: `cache restore`)", &[]),
        ),
        "cache.context_changed" => (
            m("the local shader cache was last used with the shader compiler setup {0}, this prefix has {1}", &[0, 1]),
            m("entries compiled under the old setup stay in the cache; consider rebuilding it with `tsw-launch cache quarantine` (reversible)", &[]),
        ),
        "cache.ok" => (m("local shader cache: {0} shader(s), none compiled by Wine's built-in compiler", &[0]), none),
        _ => (Msg::raw(f.what.clone()), if f.hint.is_empty() { none } else { Msg::raw(f.hint.clone()) }),
    }
}

/// The detection hints (`Detected::note_codes`).
pub fn note(code: &str, args: &[String], fallback: &str) -> Msg {
    let first = args.first().cloned().unwrap_or_default();
    match code {
        "discover.many_runtimes" => Msg::new("Several runtimes were found; {0} is used.", [first]),
        "discover.untested_runtime" => Msg::new("The runtime {0} has not been tested with this client yet.", [first]),
        _ => Msg::raw(fallback),
    }
}

/// Why no installation/runtime could be chosen.
pub fn problem(p: &Problem) -> Msg {
    match p {
        Problem::NoInstall => Msg::plain("No TSW installation was found. Set one up under INSTALLATION in the advanced view, adopt an existing installation, or enter the client there."),
        Problem::AmbiguousInstall(v) => {
            Msg::new("Several TSW test installations were found ({0}). Please choose one in the advanced view.", [v.iter().map(|p| p.display().to_string()).collect::<Vec<_>>().join(", ")])
        }
        Problem::NoRuntime => Msg::plain(
            "No suitable Wine/Proton runtime was found (searched: Steam compatibility tools with DXVK and vkd3d, for example GE-Proton). Install one or enter the Wine program in the advanced view.",
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::i18n::Catalog;
    use std::path::{Path, PathBuf};
    use tsw_launch::doctor::{self, Level};
    use tsw_launch::prefix;
    use tsw_launch::runner::Runner;

    fn tmp(tag: &str) -> PathBuf {
        // unique per call: tests run in parallel and several of them build the sample findings
        static N: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let d = std::env::temp_dir().join(format!("tsw-msg-{tag}-{}-{}", std::process::id(), N.fetch_add(1, std::sync::atomic::Ordering::SeqCst)));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    /// Findings from the real library code in many situations.
    fn sample_findings() -> Vec<Finding> {
        let d = tmp("sample");
        let lib = d.join("lib32");
        std::fs::create_dir_all(&lib).unwrap();
        for f in ["libfreetype.so.6", "libXi.so.6"] {
            std::fs::write(lib.join(f), b"x").unwrap();
        }
        let mut v = Vec::new();
        v.extend(doctor::check_32bit_fonts(&[]));
        v.extend(doctor::check_32bit_fonts(std::slice::from_ref(&lib)));
        v.extend(doctor::check_32bit_x11(&[]));
        v.extend(doctor::check_32bit_x11(std::slice::from_ref(&lib)));
        let wine = Runner::Wine { wine: d.join("no-wine"), prefix: None };
        let fake_wine = d.join("wine");
        std::fs::write(&fake_wine, b"x").unwrap();
        let wine_ok = Runner::Wine { wine: fake_wine, prefix: Some(d.join("pfx")) };
        let (not_mz, mz) = (d.join("notmz.exe"), d.join("mz.exe"));
        std::fs::write(&not_mz, b"ELF").unwrap();
        std::fs::write(&mz, b"MZ....").unwrap();
        for exe in [d.join("missing.exe"), not_mz, mz] {
            v.extend(doctor::check_all(&exe, &wine, None, &[], None, false));
            v.extend(doctor::check_all(&exe, &wine_ok, Some(&d.join("pfx")), &[], None, true));
        }
        std::fs::create_dir_all(d.join("pfx")).unwrap();
        v.extend(doctor::check_all(&d.join("missing.exe"), &wine_ok, Some(&d.join("pfx")), &[], None, true));
        // prefixes
        let reg_ok = "[Control Panel\\\\Desktop\\\\WindowMetrics] 1\n\"CaptionHeight\"=\"25\"\n";
        let reg_bad = "[Control Panel\\\\Desktop\\\\WindowMetrics] 1\n\"CaptionHeight\"=\"5434094\"\n";
        v.extend(prefix::check(&d.join("nothing"), None));
        for (tag, reg, manifest) in [
            ("a", reg_ok, "dxvk=installed\ndll_overrides=d3d11,dxgi=n,b\n"),
            ("b", reg_bad, "dxvk=installed\ndll_overrides=kernel32=n\n"),
            ("c", "", "runtime=x\n"),
            ("d", reg_ok, "dxvk=installed\ndll_overrides=d3dcompiler_47=n\ncomponent=drive_c/windows/syswow64/gone.dll md5=00\ncomponent=drive_c/windows/syswow64/changed.dll md5=00\n"),
        ] {
            let p = d.join(format!("p-{tag}"));
            std::fs::create_dir_all(p.join("drive_c/windows/syswow64")).unwrap();
            std::fs::write(p.join("user.reg"), reg).unwrap();
            std::fs::write(p.join("drive_c/windows/syswow64/changed.dll"), b"y").unwrap();
            std::fs::write(p.join(prefix::MANIFEST), manifest).unwrap();
            v.extend(prefix::check(&p, None));
        }
        let ok = d.join("p-ok");
        std::fs::create_dir_all(ok.join("drive_c/windows/syswow64")).unwrap();
        std::fs::write(ok.join("user.reg"), reg_ok).unwrap();
        std::fs::write(ok.join("drive_c/windows/syswow64/d3d11.dll"), b"z").unwrap();
        std::fs::write(ok.join(prefix::MANIFEST), format!("dxvk=installed\ndll_overrides=d3d11=n,b\ncomponent=drive_c/windows/syswow64/d3d11.dll md5={}\n", md5_hex(b"z"))).unwrap();
        v.extend(prefix::check(&ok, None));
        // a runtime tree with a differing / missing component
        let rt = d.join("rt");
        let ops = prefix::plan_components(&rt, &ok);
        std::fs::create_dir_all(ops[0].from.parent().unwrap()).unwrap();
        std::fs::write(&ops[0].from, b"runtime copy").unwrap();
        std::fs::create_dir_all(ops[0].to.parent().unwrap()).unwrap();
        std::fs::write(&ops[0].to, b"other").unwrap();
        v.extend(prefix::check(&ok, Some(&rt)));
        let _ = std::fs::remove_dir_all(&d);
        // Gamescope (optional)
        let plan = tsw_launch::gamescope::Plan { size: (1920, 1200) };
        v.extend(tsw_launch::gamescope::check(Some(&plan), None, false, false));
        v.extend(tsw_launch::gamescope::check(Some(&plan), None, false, true));
        v.extend(tsw_launch::gamescope::check(Some(&plan), Some(Path::new("/usr/bin/gamescope")), true, false));
        v
    }

    fn md5_hex(b: &[u8]) -> String {
        tsw_launch::md5::md5_hex(b)
    }

    #[test]
    fn the_english_message_ids_equal_the_librarys_own_text_for_every_code_it_produces() {
        let en = Catalog::new("en");
        let findings = sample_findings();
        let mut seen = std::collections::BTreeSet::new();
        for f in &findings {
            if f.code.is_empty() {
                continue;
            }
            let (what, hint) = finding(f);
            assert_eq!(en.render(&what), f.what, "what of {}", f.code);
            if !f.hint.is_empty() {
                // the x11 hint embeds an English message id as its last argument
                assert_eq!(en.render(&hint), f.hint, "hint of {}", f.code);
            }
            seen.insert(f.code);
        }
        assert!(seen.len() >= 25, "the sample should cover most codes, covered {}: {seen:?}", seen.len());
    }

    #[test]
    fn every_code_the_library_defines_is_known_here_and_nothing_is_uncoded() {
        let sources = [include_str!("../../launcher/src/doctor.rs"), include_str!("../../launcher/src/prefix.rs"), include_str!("../../launcher/src/gamescope.rs")];
        let mut codes = Vec::new();
        for src in sources {
            let mut rest = src;
            while let Some(i) = rest.find(".coded(\"") {
                rest = &rest[i + 8..];
                codes.push(rest[..rest.find('"').unwrap()].to_string());
            }
        }
        assert!(codes.len() >= 30, "found {} codes", codes.len());
        for code in &codes {
            let probe = Finding { level: Level::Warn, what: "w".into(), hint: String::new(), code: Box::leak(code.clone().into_boxed_str()), args: vec!["a".into(); 4] };
            assert!(!matches!(finding(&probe).0, Msg { ref id, .. } if id == "w"), "{code} has no entry in messages::finding");
        }
        for f in sample_findings() {
            assert!(!f.code.is_empty(), "finding without code: {}", f.what);
        }
    }

    #[test]
    fn an_unknown_code_shows_the_librarys_english_text_unchanged() {
        let f = Finding { level: Level::Warn, what: "something new".into(), hint: "do this".into(), code: "future.code", args: vec![] };
        let (w, h) = finding(&f);
        assert_eq!(Catalog::new("de").render(&w), "something new");
        assert_eq!(Catalog::new("de").render(&h), "do this");
    }

    #[test]
    fn problems_and_notes_are_messages() {
        assert!(problem(&Problem::NoRuntime).id.contains("runtime"));
        let amb = problem(&Problem::AmbiguousInstall(vec![Path::new("/a").into(), Path::new("/b").into()]));
        assert_eq!(Catalog::new("en").render(&amb), "Several TSW test installations were found (/a, /b). Please choose one in the advanced view.");
        assert_eq!(Catalog::new("en").render(&note("discover.untested_runtime", &["GE-Proton12".into()], "")), "The runtime GE-Proton12 has not been tested with this client yet.");
        assert_eq!(Catalog::new("en").render(&note("future", &[], "German fallback text")), "German fallback text");
    }
}
