//! Starting the client so that it outlives the launcher.
//!
//! `spawn_detached` is what "launch and close the launcher" needs: the client's Wine process gets its own process group
//! (terminal hang-ups and Ctrl-C aimed at the launcher do not reach it), no stdin, and its output goes nowhere (or, on
//! request, into a log file). The caller does not wait for it; when the caller exits the process is adopted by init,
//! so no helper process stays behind.
//!
//! Output is discarded by default on purpose: the client prints its log (which may later contain account names) to
//! stdout, and a launcher must not keep that around unasked.

use std::fs::File;
use std::io;
use std::path::Path;
use std::process::{Command, Stdio};

#[cfg(unix)]
fn create_private(path: &Path) -> io::Result<File> {
    use std::os::unix::fs::OpenOptionsExt;
    std::fs::OpenOptions::new().write(true).create(true).truncate(true).mode(0o600).open(path)
}

#[cfg(not(unix))]
fn create_private(path: &Path) -> io::Result<File> {
    File::create(path)
}

/// Start `cmd` detached and return its process id. Unix: own process group; other platforms: plain spawn.
pub fn spawn_detached(cmd: &mut Command, log_file: Option<&Path>) -> io::Result<u32> {
    spawn_detached_child(cmd, log_file).map(|c| c.id())
}

/// [`spawn_detached`] returning the child, for callers that must keep waiting for it: inside a Flatpak sandbox every process of the sandbox ends when its
/// main process ends, so "detached" only works if the launcher stays alive for as long as the client runs.
pub fn spawn_detached_child(cmd: &mut Command, log_file: Option<&Path>) -> io::Result<std::process::Child> {
    cmd.stdin(Stdio::null());
    match log_file {
        Some(path) => {
            // RAW and PRIVATE: the client's own output (it can contain the chat registration with nick and password) goes here
            // unfiltered, because a filter would need a helper process that outlives the launcher. Mode 0600; never share this file.
            let out = create_private(path)?;
            cmd.stderr(Stdio::from(out.try_clone()?)).stdout(Stdio::from(out));
        }
        None => {
            cmd.stdout(Stdio::null()).stderr(Stdio::null());
        }
    }
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        cmd.process_group(0);
    }
    cmd.spawn()
}

/// Every process below `root` (children, grandchildren and orphans a sub-reaper below it has adopted), read from `/proc`.
#[cfg(target_os = "linux")]
fn descendants(root: u32) -> Vec<u32> {
    let mut parent_of: Vec<(u32, u32)> = Vec::new();
    if let Ok(dir) = std::fs::read_dir("/proc") {
        for e in dir.flatten() {
            let Some(pid) = e.file_name().to_str().and_then(|n| n.parse::<u32>().ok()) else { continue };
            // "pid (comm) S ppid ...": the command name may contain spaces and brackets, so look after the LAST ')'
            let Ok(stat) = std::fs::read_to_string(e.path().join("stat")) else { continue };
            if let Some(ppid) = stat.rsplit_once(')').and_then(|(_, rest)| rest.split_whitespace().nth(1)).and_then(|v| v.parse::<u32>().ok()) {
                parent_of.push((pid, ppid));
            }
        }
    }
    let mut found = vec![root];
    let mut i = 0;
    while i < found.len() {
        let p = found[i];
        for &(pid, ppid) in &parent_of {
            if ppid == p && !found.contains(&pid) {
                found.push(pid);
            }
        }
        i += 1;
    }
    found.remove(0);
    found
}

/// Is a process whose program (first word of its command line, not a mere argument of a wrapper such as Gamescope) is `exe_name` running below `root`?
#[cfg(target_os = "linux")]
pub fn client_alive_below(root: u32, exe_name: &str) -> bool {
    descendants(root).into_iter().any(|pid| {
        let Ok(raw) = std::fs::read(format!("/proc/{pid}/cmdline")) else { return false };
        let first = raw.split(|b| *b == 0).next().unwrap_or_default();
        String::from_utf8_lossy(first).rsplit(['/', '\\']).next().is_some_and(|n| n.eq_ignore_ascii_case(exe_name))
    })
}

/// Signal processes by number through the shell built-in (no `unsafe`, no extra dependency; a Flatpak runtime has no `kill` program).
#[cfg(target_os = "linux")]
fn signal(sig: &str, pids: &[u32]) {
    if pids.is_empty() {
        return;
    }
    let _ = Command::new("sh").arg("-c").arg("kill -s \"$@\" 2>/dev/null").arg("sh").arg(sig).args(pids.iter().map(|p| p.to_string())).status();
}

/// Wait for `child` - and end the whole tree when the client is gone but the tree is not. A wrapper such as Gamescope waits for ALL of its descendants,
/// so one orphaned Wine helper (`winedevice.exe` blocked after the client was killed) would keep the launcher and an empty full-screen window alive
/// forever. The client must have been seen once; after `grace` without it the tree gets SIGTERM (Wine saves its registry on it), then SIGKILL.
#[cfg(target_os = "linux")]
pub fn wait_for_client(child: &mut std::process::Child, exe_name: &str, grace: std::time::Duration, poll: std::time::Duration) -> io::Result<std::process::ExitStatus> {
    use std::time::Instant;
    let (mut seen, mut gone_since) = (false, None::<Instant>);
    loop {
        if let Some(status) = child.try_wait()? {
            return Ok(status);
        }
        if client_alive_below(child.id(), exe_name) {
            (seen, gone_since) = (true, None);
        } else if seen && gone_since.get_or_insert_with(Instant::now).elapsed() >= grace {
            let mut tree = descendants(child.id());
            tree.push(child.id());
            signal("TERM", &tree);
            let until = Instant::now() + std::time::Duration::from_secs(5);
            while Instant::now() < until && child.try_wait()?.is_none() {
                std::thread::sleep(poll);
            }
            if child.try_wait()?.is_none() {
                let mut rest = descendants(child.id());
                rest.push(child.id());
                signal("KILL", &rest);
            }
            return child.wait();
        }
        std::thread::sleep(poll);
    }
}

/// A running process found by [`find_processes`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Running {
    pub pid: u32,
    /// When it started (boot time plus its start tick); `None` if that could not be read.
    pub started: Option<std::time::SystemTime>,
}

/// Processes (not zombies) whose PROGRAM is one of `names` (the first word of the command line, case-insensitive; Wine renames `comm`) and that work
/// below `under` (their current directory; a process whose directory cannot be read counts). `/proc` only: inside a Flatpak sandbox that is the
/// sandbox's own process list.
#[cfg(target_os = "linux")]
pub fn find_processes(names: &[&str], under: &Path) -> Vec<Running> {
    let boot = std::fs::read_to_string("/proc/stat").ok().and_then(|s| s.lines().find_map(|l| l.strip_prefix("btime ").and_then(|v| v.trim().parse::<u64>().ok())));
    let mut found = Vec::new();
    let Ok(dir) = std::fs::read_dir("/proc") else { return found };
    for e in dir.flatten() {
        let Some(pid) = e.file_name().to_str().and_then(|n| n.parse::<u32>().ok()) else { continue };
        let Ok(raw) = std::fs::read(e.path().join("cmdline")) else { continue };
        let first = String::from_utf8_lossy(raw.split(|b| *b == 0).next().unwrap_or_default()).into_owned();
        let program = first.rsplit(['/', '\\']).next().unwrap_or_default();
        if !names.iter().any(|n| n.eq_ignore_ascii_case(program)) {
            continue;
        }
        let Ok(stat) = std::fs::read_to_string(e.path().join("stat")) else { continue };
        let Some(fields) = stat.rsplit_once(')').map(|(_, rest)| rest.split_whitespace().map(str::to_string).collect::<Vec<_>>()) else { continue };
        if fields.first().is_none_or(|s| s == "Z" || s == "X") {
            continue;
        }
        if std::fs::read_link(e.path().join("cwd")).is_ok_and(|cwd| !cwd.starts_with(under)) {
            continue;
        }
        // field 22 of /proc/<pid>/stat is the start time in clock ticks (USER_HZ = 100 on Linux) since boot
        let started = boot.zip(fields.get(19).and_then(|v| v.parse::<u64>().ok())).map(|(b, t)| std::time::UNIX_EPOCH + std::time::Duration::from_millis(b * 1000 + t * 10));
        found.push(Running { pid, started });
    }
    found
}

/// Without `/proc` nothing can be found.
#[cfg(not(target_os = "linux"))]
pub fn find_processes(names: &[&str], _under: &Path) -> Vec<Running> {
    // Native Windows: the programs are found by NAME (the original patcher and installer live in Program Files, not below the launcher's data folder, and
    // no Wine installation shares the machine). Elsewhere the list is empty.
    crate::winnative::list_processes(names).into_iter().map(|p| Running { pid: p.pid, started: p.started }).collect()
}

/// What can be read about a running process without touching it: does it have a thread called `thread_name`, and how many bytes did it write so far
/// (`wchar` of `/proc/<pid>/io`)? Either part is `None`/`false` when it cannot be read.
#[cfg(target_os = "linux")]
pub fn probe_process(pid: u32, thread_name: &str) -> (bool, Option<u64>) {
    let threads =
        std::fs::read_dir(format!("/proc/{pid}/task")).map(|d| d.flatten().any(|t| std::fs::read_to_string(t.path().join("comm")).is_ok_and(|c| c.trim_end() == thread_name))).unwrap_or(false);
    let written = std::fs::read_to_string(format!("/proc/{pid}/io")).ok().and_then(|io| io.lines().find_map(|l| l.strip_prefix("wchar:").and_then(|v| v.trim().parse::<u64>().ok())));
    (threads, written)
}

#[cfg(not(target_os = "linux"))]
pub fn probe_process(_pid: u32, _thread_name: &str) -> (bool, Option<u64>) {
    (false, None)
}

/// Without `/proc` there is no tree to inspect: just wait.
#[cfg(not(target_os = "linux"))]
pub fn wait_for_client(child: &mut std::process::Child, _exe_name: &str, _grace: std::time::Duration, _poll: std::time::Duration) -> io::Result<std::process::ExitStatus> {
    child.wait()
}

/// Run `cmd`, wait for it, and forward its stdout and stderr to `out`/`err` line by line through the central masker
/// ([`crate::secrets`]): the client writes its log (which can contain the chat registration with nick and password) to these
/// streams, and what the launcher forwards must never carry it.
pub fn run_masked<O, E>(cmd: &mut Command, out: O, err: E) -> io::Result<std::process::ExitStatus>
where
    O: io::Write + Send + 'static,
    E: io::Write + Send + 'static,
{
    cmd.stdout(Stdio::piped()).stderr(Stdio::piped());
    let mut child = cmd.spawn()?;
    let (co, ce) = (child.stdout.take(), child.stderr.take());
    let t_out = std::thread::spawn(move || co.map(|r| crate::secrets::copy_masked(r, out)));
    let t_err = std::thread::spawn(move || ce.map(|r| crate::secrets::copy_masked(r, err)));
    let status = child.wait()?;
    let _ = (t_out.join(), t_err.join());
    Ok(status)
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;

    #[cfg(target_os = "linux")]
    /// `sleep` under the client's name, so its command line starts with the client's name (the way a Wine client looks).
    fn fake_client(dir: &std::path::Path) -> std::path::PathBuf {
        crate::sleep_stand_in(dir, "TheSecretWorldDX11.exe")
    }

    #[test]
    fn a_wrapper_that_outlives_the_client_is_ended_but_one_that_ends_by_itself_is_left_alone() {
        use std::time::{Duration, Instant};
        let _exec = crate::test_exec_lock();
        let dir = std::env::temp_dir().join(format!("tsw-wait-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let exe = fake_client(&dir);
        let (grace, poll) = (Duration::from_millis(600), Duration::from_millis(100));
        // the client ends after 1 s, the "wrapper" (a shell) would stay for 60 s because of an orphaned helper
        let mut lingering = Command::new("sh").arg("-c").arg(format!("'{}' 1; exec /bin/sleep 60", exe.display())).spawn().unwrap();
        let t = Instant::now();
        let status = wait_for_client(&mut lingering, "TheSecretWorldDX11.exe", grace, poll).unwrap();
        assert!(t.elapsed() < Duration::from_secs(10), "the tree was ended after the client: {:?}", t.elapsed());
        assert!(!status.success(), "ended by a signal: {status:?}");
        // a wrapper that ends on its own is not touched and keeps its own exit status
        let mut normal = Command::new("sh").arg("-c").arg(format!("'{}' 1; exit 3", exe.display())).spawn().unwrap();
        assert_eq!(wait_for_client(&mut normal, "TheSecretWorldDX11.exe", Duration::from_secs(30), poll).unwrap().code(), Some(3));
        // a tree in which the client never appeared is not ended (only waited for)
        let mut no_client = Command::new("sh").arg("-c").arg("/bin/sleep 2; exit 4").spawn().unwrap();
        assert_eq!(wait_for_client(&mut no_client, "TheSecretWorldDX11.exe", grace, poll).unwrap().code(), Some(4));
        // the name must be the PROGRAM, not an argument of a wrapper
        let mut wrapper = Command::new("/bin/sleep").arg("3").arg(&exe).spawn().unwrap();
        assert!(!client_alive_below(wrapper.id(), "TheSecretWorldDX11.exe"));
        let _ = wrapper.kill();
        let _ = wrapper.wait();
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn processes_are_found_by_program_and_directory_and_zombies_and_wrappers_do_not_count() {
        use std::time::{Duration, SystemTime};
        let _exec = crate::test_exec_lock();
        let dir = std::env::temp_dir().join(format!("tsw-find-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let exe = crate::sleep_stand_in(&dir, "ClientPatcher.exe");
        let mut running = Command::new(&exe).arg("30").current_dir(&dir).spawn().unwrap();
        crate::wait_until_listed("ClientPatcher.exe", &dir, &mut running);
        let found = find_processes(&["ClientPatcher.exe"], &dir);
        assert_eq!(found.iter().map(|r| r.pid).collect::<Vec<_>>(), vec![running.id()]);
        let started = found[0].started.expect("the start time is read");
        assert!(started <= SystemTime::now() + Duration::from_secs(2) && started + Duration::from_secs(120) > SystemTime::now(), "{started:?}");
        assert!(find_processes(&["clientpatcher.EXE"], &dir).len() == 1, "case-insensitive");
        assert!(find_processes(&["ClientPatcher.exe"], Path::new("/nonexistent/elsewhere")).is_empty(), "a process working in another place is not ours");
        // a wrapper that only carries the name as an argument is not the program
        // (a shell that waits on its open stdin and has the path as an argument; `sleep 30 <path>` would end at once: sleep rejects the path)
        let mut wrapper = Command::new("sh").arg("-c").arg("read line").arg("sh").arg(&exe).stdin(Stdio::piped()).current_dir(&dir).spawn().unwrap();
        crate::wait_until_listed("sh", &dir, &mut wrapper);
        assert_eq!(find_processes(&["ClientPatcher.exe"], &dir).len(), 1);
        let _ = wrapper.kill();
        let _ = wrapper.wait();
        // a finished child that was not waited for is a zombie: not running
        running.kill().unwrap();
        std::thread::sleep(Duration::from_millis(300));
        assert!(find_processes(&["ClientPatcher.exe"], &dir).is_empty(), "a zombie is not a running process");
        running.wait().unwrap();
        let _ = std::fs::remove_dir_all(&dir);
    }

    fn read_link(pid: u32, fd: u32) -> String {
        std::fs::read_link(format!("/proc/{pid}/fd/{fd}")).map(|p| p.display().to_string()).unwrap_or_default()
    }

    #[cfg(target_os = "linux")]
    fn stat_field(pid: u32, idx: usize) -> String {
        let s = std::fs::read_to_string(format!("/proc/{pid}/stat")).unwrap();
        let after = s.rsplit_once(") ").unwrap().1.to_string();
        after.split(' ').nth(idx).unwrap().to_string()
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn the_child_gets_its_own_process_group_no_stdin_and_no_output_and_is_not_waited_for() {
        let _exec = crate::test_exec_lock();
        let mut cmd = Command::new("sh");
        cmd.args(["-c", "sleep 20"]);
        let t = std::time::Instant::now();
        let pid = spawn_detached(&mut cmd, None).unwrap();
        assert!(t.elapsed().as_secs() < 5, "returns immediately, does not wait for the child");
        assert_eq!(stat_field(pid, 2), pid.to_string(), "own process group (pgrp == pid)");
        assert_eq!((read_link(pid, 0).as_str(), read_link(pid, 1).as_str(), read_link(pid, 2).as_str()), ("/dev/null", "/dev/null", "/dev/null"));
        let _ = Command::new("kill").arg(pid.to_string()).status();
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn an_optional_log_file_receives_the_output() {
        let _exec = crate::test_exec_lock();
        let dir = std::env::temp_dir().join(format!("tsw-launch-detach-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let log = dir.join("out.log");
        let mut cmd = Command::new("sh");
        cmd.args(["-c", "echo hallo; echo fehler >&2"]);
        spawn_detached(&mut cmd, Some(&log)).unwrap();
        for _ in 0..100 {
            if std::fs::read_to_string(&log).map(|s| s.contains("fehler")).unwrap_or(false) {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
        let s = std::fs::read_to_string(&log).unwrap();
        assert!(s.contains("hallo") && s.contains("fehler"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_missing_program_is_an_immediate_error_not_a_silent_success() {
        let _exec = crate::test_exec_lock();
        let mut cmd = Command::new("/nonexistent/definitely-not-here");
        assert!(spawn_detached(&mut cmd, None).is_err());
    }

    #[derive(Clone, Default)]
    struct Sink(std::sync::Arc<std::sync::Mutex<Vec<u8>>>);
    impl io::Write for Sink {
        fn write(&mut self, b: &[u8]) -> io::Result<usize> {
            self.0.lock().unwrap().extend_from_slice(b);
            Ok(b.len())
        }
        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    #[test]
    fn the_waiting_mode_forwards_client_output_masked_on_both_streams_and_keeps_the_exit_status() {
        let _exec = crate::test_exec_lock();
        // a stand-in client that prints INVENTED credentials the way the real one prints its chat registration
        let script =
            "echo 'ready'; echo 'chat is registering with nick: Nobody, password: Zx9fakeVALUEqq42'; echo \"PASS Zx9fakeVALUEqq42\" >&2; echo 'Failed to compile shader (0x80004001)' >&2; exit 3";
        let (o, e) = (Sink::default(), Sink::default());
        let status = run_masked(Command::new("sh").arg("-c").arg(script), o.clone(), e.clone()).unwrap();
        assert_eq!(status.code(), Some(3), "the client's exit code is preserved");
        let (so, se) = (String::from_utf8(o.0.lock().unwrap().clone()).unwrap(), String::from_utf8(e.0.lock().unwrap().clone()).unwrap());
        assert!(!so.contains("Zx9fake") && !se.contains("Zx9fake") && !so.contains("Nobody"), "{so} / {se}");
        assert!(so.contains("ready") && se.contains("Failed to compile shader (0x80004001)"), "technical lines survive");
    }
}
