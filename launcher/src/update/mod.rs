//! The launcher's own update.
//!
//! Product decision (2026-11, version 0.4.1): **no platform updates the launcher by itself.** On Linux and on Windows the launcher only shows a notice that a newer version
//! exists, and its button opens the download area of the official web site (see `crate::releasenotice`); the user downloads and installs the new version. There is no
//! hosted Flatpak repository, no update portal client and no downloader in the shipped programs.
//!
//! The signed-metadata updater for Windows (`signed`, `windows`: signed metadata, download, locked installer start, restore of the previous version) is kept in the private
//! development repository for possible later use, but it exists only behind the Cargo feature `windows-self-update`. No product build enables that feature, so the
//! shipped programs contain none of it: it is not compiled, not linked, and cannot be reached.

#[cfg(feature = "windows-self-update")]
mod signed;
#[cfg(feature = "windows-self-update")]
pub use signed::*;
