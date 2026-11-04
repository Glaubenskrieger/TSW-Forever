//! Diagnostics vocabulary plus the one report that exists today: the start record ([`LaunchRecord`]). The wider reporter
//! uses diaglog::bundle for the implemented user-triggered reduced package.
//!
//! Only the vocabulary lives here so that new code can already describe what happened in a structured way:
//! a category, a severity and a message that never contains credentials. A later reporter implements [`Recorder`];
//! until then [`Discard`] is used and nothing is stored, written or sent anywhere.

/// What part of the launcher an event is about. Stable names: they become the category field of future reports.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Category {
    Config,
    Discovery,
    Runtime,
    Prefix,
    Client,
    Gui,
    Patcher,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Severity {
    Info,
    Warning,
    Error,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Event {
    pub category: Category,
    pub severity: Severity,
    /// Human-readable and free of secrets by construction (no environment dumps, no command lines, no file contents).
    pub message: String,
}

pub trait Recorder: Send + Sync {
    fn record(&self, event: Event);
}

/// Default recorder: keeps nothing.
pub struct Discard;
impl Recorder for Discard {
    fn record(&self, _: Event) {}
}

/// What a start was made of, written next to the configuration (`last-launch.txt`) so that a manual test can later be
/// understood: which prefix, which runtime, virtual desktop or not, which graphics components and overrides, which
/// arguments. Free of credentials by construction: arguments are passed through [`crate::redacted`] by the caller,
/// environment variables are listed by name only (except the DLL overrides, which are not secret), and the home
/// directory is shown as `~`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct LaunchRecord {
    pub unix_time: u64,
    pub launcher_version: String,
    pub client: String,
    pub runner: String,
    /// Runtime version string from the prefix manifest (`wine-11.0 (Staging)`), if the prefix has one.
    pub runtime_version: Option<String>,
    pub prefix: String,
    pub virtual_desktop: Option<(u32, u32)>,
    pub resolution: Option<(u32, u32)>,
    /// `WINEDLLOVERRIDES` as handed to Wine (merged value).
    pub dll_overrides: Option<String>,
    /// Manifest components (relative path + md5) of the prefix.
    pub components: Vec<String>,
    /// Arguments of the client, already redacted.
    pub args: Vec<String>,
    /// Names of the extra environment variables (never their values).
    pub env_names: Vec<String>,
    /// Frame-rate limit handed to DXVK for this start (wine runner only), e.g. `60` or `unlimited`.
    pub fps_limit: Option<String>,
}

impl LaunchRecord {
    /// Text form. `home` is replaced by `~` everywhere.
    pub fn render(&self, home: Option<&str>) -> String {
        let mask = |s: &str| match home.filter(|h| h.len() > 1) {
            Some(h) => s.replace(h, "~"),
            None => s.to_string(),
        };
        let size = |v: Option<(u32, u32)>| v.map_or("no".to_string(), |(w, h)| format!("{w}x{h}"));
        let mut out = String::from("# tsw-launch start record (arguments redacted, environment variables by name only)\n");
        // every field passes the central masker; the arguments were already redacted flag by flag (their secret values are gone, the
        // flag names stay readable), so they only get the URL-credential check
        let mut line = |k: &str, v: String| {
            let v = if k == "args" { crate::secrets::mask_url_credentials(&v) } else { crate::secrets::mask_line(&v) };
            out.push_str(&format!("{k}: {v}\n"));
        };
        line("time_unix", self.unix_time.to_string());
        line("launcher", self.launcher_version.clone());
        line("client", mask(&self.client));
        line("runner", mask(&self.runner));
        line("runtime_version", self.runtime_version.clone().unwrap_or_else(|| "unknown (prefix has no manifest)".into()));
        line("prefix", mask(&self.prefix));
        line("virtual_desktop", size(self.virtual_desktop));
        line("resolution_written_to_prefs", size(self.resolution));
        line("WINEDLLOVERRIDES", self.dll_overrides.clone().unwrap_or_else(|| "(none)".into()));
        line("args", mask(&self.args.join(" ")));
        if let Some(f) = &self.fps_limit {
            line("fps_limit", f.clone());
        }
        line("extra_env_names", if self.env_names.is_empty() { "(none)".into() } else { self.env_names.join(", ") });
        line("prefix_components", self.components.len().to_string());
        for c in &self.components {
            out.push_str(&format!("  {}\n", crate::secrets::mask_line(c)));
        }
        out
    }
}

/// Write the record (mode 0600 on Unix). Best effort for the caller: a failure must never stop a start.
pub fn write_record(path: &std::path::Path, text: &str) -> std::io::Result<()> {
    if let Some(dir) = path.parent().filter(|d| !d.as_os_str().is_empty()) {
        std::fs::create_dir_all(dir)?;
    }
    #[cfg(unix)]
    {
        use std::io::Write;
        use std::os::unix::fs::OpenOptionsExt;
        let mut f = std::fs::OpenOptions::new().write(true).create(true).truncate(true).mode(0o600).open(path)?;
        f.write_all(text.as_bytes())
    }
    #[cfg(not(unix))]
    {
        std::fs::write(path, text)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_record_names_what_a_manual_test_needs_and_hides_home_and_secrets() {
        let r = LaunchRecord {
            unix_time: 1,
            launcher_version: "0.1.0".into(),
            client: "/home/someone/proj/tsw-copy/TheSecretWorldDX11.exe".into(),
            runner: "/home/someone/GE-Proton11-11/files/bin/wine".into(),
            runtime_version: Some("wine-11.0 (Staging)".into()),
            prefix: "/home/someone/proj/prefix-managed".into(),
            virtual_desktop: None,
            resolution: Some((1920, 1200)),
            dll_overrides: Some("d3d11,dxgi=n,b".into()),
            components: vec!["drive_c/windows/system32/d3d11.dll md5=00".into()],
            args: crate::redacted(&["-loginkey".into(), "SECRETVALUE".into(), "-novideo".into(), "-dx11".into()]),
            env_names: vec!["WINEDEBUG".into()],
            fps_limit: Some("60".into()),
        };
        let t = r.render(Some("/home/someone"));
        for want in [
            "runtime_version: wine-11.0 (Staging)",
            "virtual_desktop: no",
            "resolution_written_to_prefs: 1920x1200",
            "WINEDLLOVERRIDES: d3d11,dxgi=n,b",
            "-novideo -dx11",
            "extra_env_names: WINEDEBUG",
            "fps_limit: 60",
            "prefix: ~/proj/prefix-managed",
            "prefix_components: 1",
        ] {
            assert!(t.contains(want), "{want}\n{t}");
        }
        assert!(!t.contains("SECRETVALUE") && !t.contains("someone"), "{t}");
    }

    #[test]
    fn the_record_file_is_private_and_replaced_not_appended() {
        let d = std::env::temp_dir().join(format!("tsw-diag-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        let p = d.join("sub").join("last-launch.txt");
        write_record(&p, "one\n").unwrap();
        write_record(&p, "two\n").unwrap();
        assert_eq!(std::fs::read_to_string(&p).unwrap(), "two\n");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(std::fs::metadata(&p).unwrap().permissions().mode() & 0o777, 0o600);
        }
        let _ = std::fs::remove_dir_all(&d);
    }
}
