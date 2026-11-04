//! How the client executable is started on the current platform.
//!
//! Windows: the executable is started directly. Linux: through a Wine-compatible program (`wine`, or a Proton
//! build's `wine`), with the prefix taken from the caller. Nothing here is specific to one machine: all paths come from
//! the command line or the environment.

use std::path::{Path, PathBuf};
use std::process::Command;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Runner {
    /// Start the executable itself (Windows).
    Native,
    /// Start `<wine> <exe> args` (Linux/macOS). `wine` is a program name or a path; `prefix` sets `WINEPREFIX`.
    Wine { wine: PathBuf, prefix: Option<PathBuf> },
}

impl Runner {
    /// Default for the platform this binary was compiled for.
    pub fn platform_default() -> Runner {
        if cfg!(windows) {
            Runner::Native
        } else {
            Runner::Wine { wine: PathBuf::from("wine"), prefix: None }
        }
    }

    /// `native` | `wine` | a path to a Wine program; `None` = platform default. `prefix` is the WINEPREFIX for Wine runners.
    pub fn from_spec(spec: Option<&str>, prefix: Option<PathBuf>) -> Runner {
        match spec {
            None => match Runner::platform_default() {
                Runner::Wine { wine, .. } => Runner::Wine { wine, prefix },
                n => n,
            },
            Some("native") => Runner::Native,
            Some("wine") => Runner::Wine { wine: PathBuf::from("wine"), prefix },
            Some(other) => Runner::Wine { wine: PathBuf::from(other), prefix },
        }
    }

    /// Build (but do not start) the command. The working directory is the executable's directory, like the patcher does.
    pub fn command(&self, exe: &Path, args: &[String]) -> Command {
        self.command_wrapped(&[], exe, args)
    }

    /// Like [`Runner::command`], but the whole command line is prefixed with a wrapper program and its arguments
    /// (for example `gamemoderun`, or `gdb -batch --args` for diagnosis). The wrapper becomes the parent of the client.
    pub fn command_wrapped(&self, wrapper: &[String], exe: &Path, args: &[String]) -> Command {
        self.command_full(wrapper, &[], exe, args)
    }

    /// Full form: `wrapper... <wine> wine_args... <exe> args...`. `wine_args` are arguments for the Wine program itself
    /// (for example `explorer /desktop=Name,1920x1200`); they are ignored by the native runner.
    pub fn command_full(&self, wrapper: &[String], wine_args: &[String], exe: &Path, args: &[String]) -> Command {
        let dir = exe.parent().filter(|p| !p.as_os_str().is_empty()).unwrap_or_else(|| Path::new("."));
        let (program, mut lead): (PathBuf, Vec<std::ffi::OsString>) = match self {
            Runner::Native => (exe.to_path_buf(), Vec::new()),
            Runner::Wine { wine, .. } => {
                let mut lead: Vec<std::ffi::OsString> = wine_args.iter().map(Into::into).collect();
                lead.push(exe.as_os_str().to_owned());
                (wine.clone(), lead)
            }
        };
        let mut cmd = match wrapper.split_first() {
            None => Command::new(&program),
            Some((w, rest)) => {
                let mut c = Command::new(w);
                c.args(rest);
                lead.insert(0, program.into_os_string());
                c
            }
        };
        if let Runner::Wine { prefix: Some(p), .. } = self {
            cmd.env("WINEPREFIX", p);
        }
        cmd.args(lead).args(args).current_dir(dir);
        cmd
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wine_command_shape() {
        let r = Runner::Wine { wine: PathBuf::from("/opt/wine/bin/wine"), prefix: Some(PathBuf::from("/p")) };
        let c = r.command(Path::new("/g/TheSecretWorldDX11.exe"), &["-dx11".to_string()]);
        assert_eq!(c.get_program(), "/opt/wine/bin/wine");
        let args: Vec<_> = c.get_args().map(|a| a.to_string_lossy().into_owned()).collect();
        assert_eq!(args, ["/g/TheSecretWorldDX11.exe", "-dx11"]);
        assert_eq!(c.get_current_dir(), Some(Path::new("/g")));
        assert!(c.get_envs().any(|(k, v)| k == "WINEPREFIX" && v == Some(std::ffi::OsStr::new("/p"))));
    }

    #[test]
    fn wrapper_becomes_the_parent_of_the_client() {
        let r = Runner::Wine { wine: PathBuf::from("wine"), prefix: Some(PathBuf::from("/p")) };
        let w = ["gdb".to_string(), "-batch".to_string(), "--args".to_string()];
        let c = r.command_wrapped(&w, Path::new("/g/t.exe"), &["-dx11".to_string()]);
        assert_eq!(c.get_program(), "gdb");
        let args: Vec<_> = c.get_args().map(|a| a.to_string_lossy().into_owned()).collect();
        assert_eq!(args, ["-batch", "--args", "wine", "/g/t.exe", "-dx11"]);
        assert!(c.get_envs().any(|(k, _)| k == "WINEPREFIX"));
        let n = Runner::Native.command_wrapped(&["gamemoderun".to_string()], Path::new("C:/t.exe"), &[]);
        assert_eq!(n.get_program(), "gamemoderun");
        assert_eq!(n.get_args().next().unwrap(), "C:/t.exe");
    }

    #[test]
    fn wine_arguments_come_between_wine_and_the_client() {
        let r = Runner::Wine { wine: PathBuf::from("wine"), prefix: None };
        let wa = ["explorer".to_string(), "/desktop=TSW,1920x1200".to_string()];
        let c = r.command_full(&[], &wa, Path::new("/g/t.exe"), &["-dx11".to_string()]);
        let args: Vec<_> = c.get_args().map(|a| a.to_string_lossy().into_owned()).collect();
        assert_eq!(c.get_program(), "wine");
        assert_eq!(args, ["explorer", "/desktop=TSW,1920x1200", "/g/t.exe", "-dx11"]);
        let n = Runner::Native.command_full(&[], &wa, Path::new("C:/t.exe"), &[]);
        assert_eq!(n.get_program(), "C:/t.exe", "native runner ignores wine arguments");
    }

    #[test]
    fn native_command_shape() {
        let c = Runner::Native.command(Path::new("C:/Games/tsw.exe"), &["-dx9".to_string()]);
        assert_eq!(c.get_program(), "C:/Games/tsw.exe");
    }
}
