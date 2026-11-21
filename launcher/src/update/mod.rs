//! The launcher's own update.
//!
//! Product decision (2026-11): **Linux** updates through the Flatpak update portal (`flatpak`), from our own signed Flatpak repository. **Windows has no automatic
//! update**: the user installs a newer installer from the official GitHub release by hand (the launcher only shows a notice, see `crate::releasenotice`).
//!
//! The signed-metadata updater for Windows (`signed`, `windows`: signed metadata, download, locked installer start, restore of the previous version) is kept in the private
//! development repository for possible later use, but it exists only behind the Cargo feature `windows-self-update`. No product build enables that feature, so the
//! shipped programs contain none of it: it is not compiled, not linked, and cannot be reached.

pub mod flatpak;

#[cfg(feature = "windows-self-update")]
mod signed;
#[cfg(feature = "windows-self-update")]
pub use signed::*;
