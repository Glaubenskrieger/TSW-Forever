//! The launcher update through the Flatpak update portal (`org.freedesktop.portal.Flatpak`, `CreateUpdateMonitor` / `UpdateMonitor.Update`).
//!
//! Verified against the real portal (Flatpak 1.18.4, interface version 8) with a signed static OSTree repository: `UpdateAvailable`, `Update` with
//! `Progress` 0..100 and status 2, delta used, new version at the next start, unsigned commits ignored, offline harmless. The portal needs no sandbox
//! permission; the host's Flatpak checks the signature and installs. What this module adds: the signals as [`Event`] values and the one call the user's
//! decision triggers. Everything that is not D-Bus lives in `tsw_launch::update::flatpak` (pure, tested there).
//!
//! The portal ties a monitor to the connection that created it, so the connection is kept for the life of the program.

use std::collections::HashMap;
use std::sync::Arc;

use futures_lite::{future::block_on, StreamExt};
use tsw_launch::update::flatpak::Event;
use zbus::zvariant::{OwnedValue, Value};
use zbus::{Connection, Proxy};

const DEST: &str = "org.freedesktop.portal.Flatpak";
const PATH: &str = "/org/freedesktop/portal/Flatpak";
const IFACE: &str = "org.freedesktop.portal.Flatpak";
const MONITOR_IFACE: &str = "org.freedesktop.portal.Flatpak.UpdateMonitor";

type Info = HashMap<String, OwnedValue>;

fn text(info: &Info, key: &str) -> Option<String> {
    info.get(key).and_then(|v| v.downcast_ref::<&str>().ok().map(str::to_string))
}

fn number(info: &Info, key: &str) -> Option<u32> {
    info.get(key).and_then(|v| v.downcast_ref::<u32>().ok())
}

/// `UpdateAvailable` -> [`Event::Available`] (`None` if a commit is missing).
pub fn available_event(info: &Info) -> Option<Event> {
    Some(Event::Available { running: text(info, "running-commit")?, local: text(info, "local-commit")?, remote: text(info, "remote-commit")? })
}

/// `Progress` -> [`Event::Progress`] (`None` without a status).
pub fn progress_event(info: &Info) -> Option<Event> {
    Some(Event::Progress {
        status: number(info, "status")?,
        percent: number(info, "progress").unwrap_or(0),
        error: text(info, "error").unwrap_or_default(),
        message: text(info, "error_message").unwrap_or_default(),
    })
}

/// A running monitor. Cheap to clone; the user's decision calls [`Handle::request_update`].
#[derive(Clone)]
pub struct Handle {
    monitor: Arc<Proxy<'static>>,
}

impl Handle {
    /// Ask the portal to install the update (the user agreed). The answer arrives as `Progress` signals through the callback given to [`start`]; a failure of
    /// the call itself is reported through `on_event` as [`Event::CallFailed`]. Returns at once.
    pub fn request_update(&self, on_event: impl Fn(Event) + Send + 'static) {
        let monitor = self.monitor.clone();
        std::thread::spawn(move || {
            let options: HashMap<&str, Value<'_>> = HashMap::new();
            let r: zbus::Result<()> = block_on(monitor.call("Update", &("", options)));
            if let Err(e) = r {
                on_event(Event::CallFailed(e.to_string()));
            }
        });
    }
}

/// Connect (the session bus, or `address` for tests), create the update monitor and start listening. `Err` = no portal (an old Flatpak, no session bus):
/// the caller then keeps the plain hint. Blocks until the monitor exists: call it from a worker thread. Signals arrive on two helper threads for the life of
/// the program.
pub fn start(address: Option<&str>, on_event: impl Fn(Event) + Send + Sync + 'static) -> Result<Handle, String> {
    let on_event = Arc::new(on_event);
    block_on(async {
        let conn: Connection = match address {
            Some(a) => zbus::connection::Builder::address(a).map_err(|e| e.to_string())?.build().await.map_err(|e| e.to_string())?,
            None => Connection::session().await.map_err(|e| e.to_string())?,
        };
        let portal = Proxy::new(&conn, DEST, PATH, IFACE).await.map_err(|e| e.to_string())?;
        let mut options: HashMap<&str, Value<'_>> = HashMap::new();
        options.insert("handle_token", Value::from("tsw"));
        let handle: zbus::zvariant::OwnedObjectPath = portal.call("CreateUpdateMonitor", &(options,)).await.map_err(|e| format!("update monitor: {e}"))?;
        let monitor = Proxy::new(&conn, DEST, handle.as_str().to_owned(), MONITOR_IFACE).await.map_err(|e| e.to_string())?;
        let mut available = monitor.receive_signal("UpdateAvailable").await.map_err(|e| e.to_string())?;
        let mut progress = monitor.receive_signal("Progress").await.map_err(|e| e.to_string())?;
        let cb = on_event.clone();
        std::thread::spawn(move || {
            block_on(async move {
                while let Some(m) = available.next().await {
                    if let Some(e) = m.body().deserialize::<Info>().ok().as_ref().and_then(available_event) {
                        cb(e);
                    }
                }
            })
        });
        let cb = on_event;
        std::thread::spawn(move || {
            block_on(async move {
                while let Some(m) = progress.next().await {
                    if let Some(e) = m.body().deserialize::<Info>().ok().as_ref().and_then(progress_event) {
                        cb(e);
                    }
                }
            })
        });
        Ok(Handle { monitor: Arc::new(monitor) })
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{BufRead, BufReader};
    use std::process::{Child, Command, Stdio};
    use std::sync::mpsc;
    use std::time::Duration;
    use zbus::object_server::SignalEmitter;
    use zbus::zvariant::OwnedObjectPath;

    const A: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
    const B: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";

    fn info(pairs: &[(&str, Value<'_>)]) -> Info {
        pairs.iter().map(|(k, v)| (k.to_string(), v.try_to_owned().unwrap())).collect()
    }

    #[test]
    fn the_portal_dictionaries_become_events_and_incomplete_ones_are_ignored() {
        let ok = info(&[("running-commit", Value::from(A)), ("local-commit", Value::from(A)), ("remote-commit", Value::from(B))]);
        assert_eq!(available_event(&ok), Some(Event::Available { running: A.into(), local: A.into(), remote: B.into() }));
        assert_eq!(available_event(&info(&[("running-commit", Value::from(A))])), None);
        assert_eq!(available_event(&info(&[("running-commit", Value::from(1u32)), ("local-commit", Value::from(A)), ("remote-commit", Value::from(B))])), None, "wrong type");
        let p = info(&[("status", Value::from(0u32)), ("progress", Value::from(42u32)), ("op", Value::from(0u32)), ("n_ops", Value::from(1u32))]);
        assert_eq!(progress_event(&p), Some(Event::Progress { status: 0, percent: 42, error: String::new(), message: String::new() }));
        let f = info(&[("status", Value::from(3u32)), ("error", Value::from("org.x.Error")), ("error_message", Value::from("it broke"))]);
        assert_eq!(progress_event(&f), Some(Event::Progress { status: 3, percent: 0, error: "org.x.Error".into(), message: "it broke".into() }));
        assert_eq!(progress_event(&info(&[("progress", Value::from(5u32))])), None, "no status");
    }

    // ---- a fake portal on a private session bus: the real client code against a service that speaks the same interface ----

    struct FakePortal {
        monitor: OwnedObjectPath,
    }

    #[zbus::interface(name = "org.freedesktop.portal.Flatpak")]
    impl FakePortal {
        async fn create_update_monitor(&self, _options: HashMap<String, Value<'_>>) -> OwnedObjectPath {
            self.monitor.clone()
        }
    }

    struct FakeMonitor {
        fail: bool,
    }

    #[zbus::interface(name = "org.freedesktop.portal.Flatpak.UpdateMonitor")]
    impl FakeMonitor {
        async fn update(&self, _parent_window: &str, _options: HashMap<String, Value<'_>>, #[zbus(signal_emitter)] emitter: SignalEmitter<'_>) -> zbus::fdo::Result<()> {
            let mk = |status: u32, progress: u32| -> Info { info(&[("status", Value::from(status)), ("progress", Value::from(progress))]) };
            Self::progress(&emitter, mk(0, 0)).await?;
            Self::progress(&emitter, mk(0, 60)).await?;
            if self.fail {
                let f = info(&[("status", Value::from(3u32)), ("error", Value::from("org.x.Error")), ("error_message", Value::from("Authorization required"))]);
                Self::progress(&emitter, f).await?;
            } else {
                Self::progress(&emitter, mk(2, 100)).await?;
            }
            Ok(())
        }

        #[zbus(signal)]
        async fn update_available(emitter: &SignalEmitter<'_>, info: Info) -> zbus::Result<()>;

        #[zbus(signal)]
        async fn progress(emitter: &SignalEmitter<'_>, info: Info) -> zbus::Result<()>;
    }

    struct Bus(Child, String);

    impl Drop for Bus {
        fn drop(&mut self) {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }

    /// A private session bus; `None` where `dbus-daemon` is not installed (the test is then skipped, not failed).
    fn private_bus() -> Option<Bus> {
        let mut child = Command::new("dbus-daemon").args(["--session", "--nofork", "--print-address"]).stdout(Stdio::piped()).stderr(Stdio::null()).spawn().ok()?;
        let mut line = String::new();
        BufReader::new(child.stdout.take()?).read_line(&mut line).ok()?;
        let address = line.trim().to_string();
        (!address.is_empty()).then(|| Bus(child, address))
    }

    fn serve(address: &str, fail: bool) -> Connection {
        let path = "/org/freedesktop/portal/Flatpak/update_monitor/test/tsw";
        let monitor = OwnedObjectPath::try_from(path).unwrap();
        block_on(async {
            zbus::connection::Builder::address(address)
                .unwrap()
                .name(DEST)
                .unwrap()
                .serve_at(PATH, FakePortal { monitor })
                .unwrap()
                .serve_at(path, FakeMonitor { fail })
                .unwrap()
                .build()
                .await
                .unwrap()
        })
    }

    fn collect(rx: &mpsc::Receiver<Event>, until: impl Fn(&Event) -> bool) -> Vec<Event> {
        let mut seen = vec![];
        while let Ok(e) = rx.recv_timeout(Duration::from_secs(10)) {
            let done = until(&e);
            seen.push(e);
            if done {
                break;
            }
        }
        seen
    }

    #[test]
    fn the_client_talks_to_a_portal_and_reports_availability_progress_and_completion() {
        let Some(bus) = private_bus() else {
            eprintln!("skipped: no dbus-daemon");
            return;
        };
        let server = serve(&bus.1, false);
        let (tx, rx) = mpsc::channel();
        let tx2 = tx.clone();
        let handle = start(Some(&bus.1), move |e| {
            let _ = tx2.send(e);
        })
        .expect("the monitor is created");
        // the portal announces a newer version
        let emitter = SignalEmitter::new(&server, "/org/freedesktop/portal/Flatpak/update_monitor/test/tsw").unwrap();
        block_on(FakeMonitor::update_available(&emitter, info(&[("running-commit", Value::from(A)), ("local-commit", Value::from(A)), ("remote-commit", Value::from(B))]))).unwrap();
        let seen = collect(&rx, |e| matches!(e, Event::Available { .. }));
        assert_eq!(seen, vec![Event::Available { running: A.into(), local: A.into(), remote: B.into() }]);
        // the user agrees: the portal installs and reports 0 %, 60 %, done
        handle.request_update(move |e| {
            let _ = tx.send(e);
        });
        let seen = collect(&rx, |e| matches!(e, Event::Progress { status: 2, .. }));
        let statuses: Vec<(u32, u32)> = seen.iter().filter_map(|e| if let Event::Progress { status, percent, .. } = e { Some((*status, *percent)) } else { None }).collect();
        assert_eq!(statuses, vec![(0, 0), (0, 60), (2, 100)], "{seen:?}");
    }

    #[test]
    fn a_refused_update_arrives_as_a_failure_with_its_reason() {
        let Some(bus) = private_bus() else {
            return;
        };
        let _server = serve(&bus.1, true);
        let (tx, rx) = mpsc::channel();
        let tx2 = tx.clone();
        let handle = start(Some(&bus.1), move |e| {
            let _ = tx2.send(e);
        })
        .unwrap();
        handle.request_update(move |e| {
            let _ = tx.send(e);
        });
        let seen = collect(&rx, |e| matches!(e, Event::Progress { status: 3, .. }));
        match seen.last() {
            Some(Event::Progress { status: 3, error, message, .. }) => {
                assert_eq!(error, "org.x.Error");
                assert_eq!(tsw_launch::update::flatpak::classify_failure(error, message), tsw_launch::update::flatpak::Failure::NotAllowed);
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn without_a_portal_the_start_fails_cleanly_and_a_missing_bus_is_not_a_crash() {
        let Some(bus) = private_bus() else {
            return;
        };
        // a bus without the portal: creating the monitor fails with a readable error
        let e = start(Some(&bus.1), |_| {}).err().expect("no portal on this bus");
        assert!(!e.is_empty());
        // no bus at all
        assert!(start(Some("unix:path=/nonexistent/tsw-test-bus"), |_| {}).is_err());
    }
}
