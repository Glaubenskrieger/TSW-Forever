# Changelog

## [0.2.0] – 2026-11-04

First public source release of TSW-Forever for Linux and SteamOS. TSW-Forever is an independent launcher for the original *The Secret World* (2012); it does not supply the game, an account, a Wine/Proton runner, or proprietary Microsoft components.

### Included

- A graphical launcher and command-line tool for installing and starting the original game in a managed Wine/Proton environment. Setup can use the checked Funcom installer download, a trusted local installer, or an existing game installation where Flatpak access permits it.
- Installation progress and completion checks based on the original patcher's evidence, with a manual verification option when automatic recognition is unavailable. The Funcom patcher may still need to be closed by the user.
- A checked GE-Proton download and a checked Microsoft DirectX June 2010 component download. The setup screen distinguishes download consent from separate acknowledgement of the linked Microsoft terms; an unchecked choice prevents provisioning. The acknowledgement is stored locally for the reviewed package and terms.
- Display settings with a game-managed default or detected resolutions applied through Wine without changing the Linux desktop mode. The existing 30–240 FPS slider, unlimited option, and optional DXVK FPS counter remain available.
- An English and German interface, title-bar navigation between the main and advanced views, and dark-theme styling.
- Flatpak packaging with app ID `org.tswlauncher.Launcher`, the matching source and required third-party licence notices. The README explains manual installation, updates, removal, and source builds.
- Prefix provenance checks, pinned download hashes, and diagnostic output filtering. Logging is optional and no telemetry is added.

### Known limitations

- The original game requires an entitled account and its own download. The original Funcom patcher may require manual closure or a verification run before the launcher offers **PLAY**.
- No automatic Flatpak update repository is currently available; install newer bundles manually. The client-patch channel is inactive.
- A previous private build was tested successfully on one Steam Machine. The public `0.2.0` build is not a guarantee of compatibility across all distributions, graphics drivers, SteamOS devices, or display setups. Windows support is future work.
