//! File chooser through the desktop portal (`org.freedesktop.portal.FileChooser`).
//!
//! Why a portal: inside a Flatpak sandbox the launcher cannot read arbitrary host paths, and typing one would fail. The portal shows the
//! desktop's own dialog OUTSIDE the sandbox and hands back only the one file the user picked (as a path below `/run/user/<uid>/doc/…`), so no
//! filesystem permission is needed. Outside a sandbox the same call works on desktops that run a portal (KDE, GNOME); where none answers, the
//! caller keeps the text field, so nothing depends on it.
//!
//! The only data that leaves the program is the dialog title. UNVERIFIED against a running portal in the Flatpak sandbox (the pure parts below
//! are unit-tested); see documentation/packaging.md.

use std::collections::HashMap;
use std::path::PathBuf;

use futures_lite::StreamExt;
use zbus::zvariant::{OwnedValue, Value};

const DEST: &str = "org.freedesktop.portal.Desktop";

/// `file:///a%20b/c.exe` -> `/a b/c.exe`. Only local `file://` URIs are accepted; a malformed escape rejects the whole URI.
pub fn uri_to_path(uri: &str) -> Option<PathBuf> {
    let rest = uri.strip_prefix("file://")?;
    let rest = rest.strip_prefix("localhost").unwrap_or(rest);
    if !rest.starts_with('/') {
        return None;
    }
    let b = rest.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' {
            let h = std::str::from_utf8(b.get(i + 1..i + 3)?).ok()?;
            out.push(u8::from_str_radix(h, 16).ok()?);
            i += 3;
        } else {
            out.push(b[i]);
            i += 1;
        }
    }
    if out.contains(&0) {
        return None;
    }
    Some(PathBuf::from(String::from_utf8(out).ok()?))
}

/// The Request object path the portal will answer on: `/org/freedesktop/portal/desktop/request/<sender>/<token>` where `<sender>` is the unique
/// bus name without the colon and with dots turned into underscores.
pub fn request_path(unique_name: &str, token: &str) -> String {
    format!("/org/freedesktop/portal/desktop/request/{}/{}", unique_name.trim_start_matches(':').replace('.', "_"), token)
}

/// Show the portal's dialog and wait for the answer. `Ok(None)` = the user cancelled. Blocks: call it from a worker thread.
pub fn pick(title: &str, directory: bool) -> Result<Option<PathBuf>, String> {
    // native Windows: Windows' own dialog (there is no D-Bus session bus and no sandbox)
    if cfg!(windows) {
        return tsw_launch::winnative::pick(title, directory);
    }
    futures_lite::future::block_on(pick_async(title, directory))
}

async fn pick_async(title: &str, directory: bool) -> Result<Option<PathBuf>, String> {
    let e = |what: &str, x: zbus::Error| format!("{what}: {x}");
    let conn = zbus::Connection::session().await.map_err(|x| e("no session bus (is a desktop portal available?)", x))?;
    let unique = conn.unique_name().map(|n| n.to_string()).ok_or("the bus gave no unique name")?;
    let handle = format!("tsw{}", std::process::id());
    let path = request_path(&unique, &handle);
    // subscribe BEFORE the call, otherwise a fast answer can be missed
    let request = zbus::Proxy::new(&conn, DEST, path.as_str(), "org.freedesktop.portal.Request").await.map_err(|x| e("portal request", x))?;
    let mut responses = request.receive_signal("Response").await.map_err(|x| e("portal request", x))?;
    let mut options: HashMap<&str, Value<'_>> = HashMap::new();
    options.insert("handle_token", Value::from(handle.as_str()));
    options.insert("modal", Value::from(true));
    options.insert("multiple", Value::from(false));
    options.insert("directory", Value::from(directory));
    conn.call_method(Some(DEST), "/org/freedesktop/portal/desktop", Some("org.freedesktop.portal.FileChooser"), "OpenFile", &("", title, options))
        .await
        .map_err(|x| e("the portal refused the file dialog", x))?;
    let msg = responses.next().await.ok_or("the portal closed without an answer")?;
    let (code, results): (u32, HashMap<String, OwnedValue>) = msg.body().deserialize().map_err(|x| e("unexpected portal answer", x))?;
    if code != 0 {
        return Ok(None); // 1 = cancelled by the user, 2 = ended in another way: nothing was chosen
    }
    let uris: Vec<String> = results.get("uris").and_then(|v| v.try_clone().ok()).and_then(|v| Vec::<String>::try_from(Value::from(v)).ok()).ok_or("the portal answer has no file")?;
    let first = uris.first().ok_or("the portal answer has no file")?;
    uri_to_path(first).map(Some).ok_or_else(|| "the portal returned an address that is not a local file".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn local_file_uris_are_decoded_and_everything_else_is_rejected() {
        assert_eq!(uri_to_path("file:///home/a/My%20Games/x.exe"), Some(PathBuf::from("/home/a/My Games/x.exe")));
        assert_eq!(uri_to_path("file://localhost/tmp/x"), Some(PathBuf::from("/tmp/x")));
        assert_eq!(uri_to_path("file:///run/user/1000/doc/ab12/Installer.exe"), Some(PathBuf::from("/run/user/1000/doc/ab12/Installer.exe")));
        for bad in ["http://example.org/x", "file://host/x", "file:///a%2", "file:///a%zz", "file:///a%00b", "/plain/path", "file:relative", ""] {
            assert_eq!(uri_to_path(bad), None, "{bad}");
        }
    }

    #[test]
    fn the_request_path_follows_the_portal_convention() {
        assert_eq!(request_path(":1.42", "tsw7"), "/org/freedesktop/portal/desktop/request/1_42/tsw7");
    }
}

/// Only the two reviewed licence destinations can be opened by the setup view.
fn microsoft_terms_url(which: i32) -> Option<&'static str> {
    match which {
        0 => Some(tsw_launch::dxredist::SDK_TERMS_URL),
        1 => Some(tsw_launch::dxredist::RUNTIME_TERMS_URL),
        _ => None,
    }
}

/// Open the online licence copy through the desktop portal, without shell commands or host permissions.
pub fn open_microsoft_terms(which: i32) -> Result<(), String> {
    let url = microsoft_terms_url(which).ok_or("unknown Microsoft licence link")?;
    futures_lite::future::block_on(async {
        let conn = zbus::Connection::session().await.map_err(|e| e.to_string())?;
        let options: HashMap<&str, Value<'_>> = HashMap::new();
        conn.call_method(Some(DEST), "/org/freedesktop/portal/desktop", Some("org.freedesktop.portal.OpenURI"), "OpenURI", &("", url, options)).await.map_err(|e| e.to_string())?;
        Ok(())
    })
}

#[cfg(test)]
mod licence_link_tests {
    #[test]
    fn only_reviewed_https_licence_links_are_allowed() {
        assert_eq!(super::microsoft_terms_url(0), Some(tsw_launch::dxredist::SDK_TERMS_URL));
        assert_eq!(super::microsoft_terms_url(1), Some(tsw_launch::dxredist::RUNTIME_TERMS_URL));
        for which in [-1, 2, i32::MAX] {
            assert!(super::open_microsoft_terms(which).is_err());
        }
        assert!(super::microsoft_terms_url(0).unwrap().starts_with("https://github.com/apitrace/dxsdk/blob/"));
        assert!(super::microsoft_terms_url(1).unwrap().starts_with("https://store.steampowered.com/eula/"));
    }
}
