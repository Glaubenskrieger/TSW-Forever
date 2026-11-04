//! `tsw-patch`: the signed client-patch system of TSW-Forever, as a reusable component without any GUI or launcher dependency.
//!
//! Flow: fetch `manifest.txt` + `manifest.txt.minisig` -> [`sig::verify`] with the trusted public key -> [`manifest::Manifest::parse`]
//! -> [`apply::plan`] (compare the installed files with the manifest) -> [`apply::apply`] (download artifacts, build every new file
//! in a staging folder, verify SHA-256, back up, swap atomically, verify again, roll back on any failure) -> only then is the new
//! patch version recorded. A manifest can only describe files (replace or delta); it has no way to name a command.
//!
//! Network access is isolated behind [`fetch::Fetch`]; everything else is local and tested with synthetic files only.

/// Version of this engine (crate version). Independent of the launcher version and of the client patch level.
pub const ENGINE_VERSION: &str = env!("CARGO_PKG_VERSION");
/// Highest manifest format this engine reads (`tsw-forever-patch <format>`). A different number is a different, incompatible grammar.
pub const MANIFEST_FORMAT: u32 = 1;

pub mod apply;
pub mod delta;
pub mod fetch;
pub mod manifest;
pub mod path;
pub mod sha256;
pub mod sig;

use std::fmt;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PatchError {
    /// The manifest text is malformed (line number, reason).
    Manifest(String),
    /// Signature missing, malformed or not made by the trusted key.
    Signature(String),
    /// A path in the manifest is unsafe, or a symbolic link is in the way.
    UnsafePath(String),
    /// A checksum or size did not match.
    Hash(String),
    Delta(String),
    /// The installed state does not fit the manifest (wrong base version, downgrade, drifted files without a full copy).
    State(String),
    /// The game is running, another update is running, or the transaction was cancelled.
    Busy(String),
    Cancelled,
    Io(String),
    Network(String),
}

impl fmt::Display for PatchError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let (k, m) = match self {
            PatchError::Manifest(m) => ("manifest", m),
            PatchError::Signature(m) => ("signature", m),
            PatchError::UnsafePath(m) => ("unsafe path", m),
            PatchError::Hash(m) => ("checksum", m),
            PatchError::Delta(m) => ("delta", m),
            PatchError::State(m) => ("state", m),
            PatchError::Busy(m) => ("busy", m),
            PatchError::Io(m) => ("i/o", m),
            PatchError::Network(m) => ("network", m),
            PatchError::Cancelled => return write!(f, "cancelled"),
        };
        write!(f, "{k}: {m}")
    }
}

impl std::error::Error for PatchError {}

pub(crate) fn io(what: &str, e: std::io::Error) -> PatchError {
    PatchError::Io(format!("{what}: {e}"))
}
