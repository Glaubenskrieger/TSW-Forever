# Changelog

## [Unreleased]

## [0.3.0] – 2026-11-08

TSW-Forever now supports **Linux and Windows**. Version `0.3.0` adds a native Windows client (Windows x64) that starts the original *The Secret World* (2012) directly, without Wine, Proton or Flatpak. The Linux and SteamOS Flatpak keeps its features. Release files: the Linux Flatpak, the Windows installer, the source archive and `SHA256SUMS`.

### Added

- **Windows installer** `tsw-forever-0.3.0-windows-setup.exe` (x64, Windows 10 or later; tested on Windows 11): installs to `C:\Program Files\TSW-Forever` (a different folder can be chosen on the folder page), adds a Start-menu entry and an optional desktop icon, registers an uninstaller in Windows' installed apps, and offers an English and a German wizard. Administrator rights are needed only for the installation; the launcher itself runs with normal user rights. The installer is **not code-signed**, so Windows SmartScreen may warn.
- **Windows first setup:** without a usable game installation the launcher offers three choices: download the official Funcom installer (after your consent; the download is checked against a pinned checksum), provide a Funcom installer file you already have (an unknown file needs your explicit confirmation), or use an existing installation by selecting its folder. The Funcom installer and patcher are always the original programs; Windows asks for permission when they start, and the launcher offers **PLAY** only after the original patcher has confirmed that the game data is complete (or after you chose an existing installation yourself).
- **Windows game folder and state:** the main window shows the game folder and whether the game is not installed, found but not yet confirmed complete, or ready.
- **Windows: install a new copy in another folder:** the advanced view can start a complete new installation in a folder you choose, next to an existing one. The existing installation is ignored from then on and never changed; it can be taken back with **Use the ignored installation again**.
- **Windows DirectX help:** if the 32-bit Direct3D libraries of Microsoft's DirectX End-User Runtimes (June 2010) are missing, the launcher names them and offers **Install DirectX components …**. Only on your click it downloads Microsoft's own web installer from `download.microsoft.com`, checks its checksum and Microsoft's signature and starts it (Windows asks for permission; Microsoft's installer shows its own terms). **Check again** updates the message afterwards. No Microsoft file is distributed with TSW-Forever.
- **Windows settings:** launcher settings are stored in `%APPDATA%\tsw-forever`, larger data in `%LOCALAPPDATA%\tsw-forever`. Settings of an earlier `tsw-launch` folder are copied without overwriting anything. The interface language follows the Windows display language (English or German) and can be changed in the advanced view.
- The Windows program has no console window and carries the project icon. The program is linked with the static C runtime, so no Visual C++ Redistributable is needed.

### Changed

- **Windows (temporary):** the automatic detection of existing game installations is switched off in this version. Choose an existing installation yourself (**Use an existing TSW installation**). After the launcher has started the original Funcom installer, the new installation is recognised.
- **Windows:** there is no launcher frame-rate limit and no FPS counter (both rely on DXVK, which exists only under Wine). The Linux slider, the unlimited option and the DXVK counter are unchanged. Linux-only settings (Wine/Proton runner, prefix, Microsoft component consent, shader cache, Gamescope) are not shown on Windows.
- Release assets are now the Linux Flatpak, the Windows installer, the source archive and one `SHA256SUMS`; there is no portable Windows ZIP. They are built and checked by one release workflow.
- Version numbers are plain `MAJOR.MINOR.PATCH` everywhere; the application is `0.3.0`.

### Known limitations

- The Windows installer and program are unsigned; SmartScreen may show a warning.
- Windows has been tested practically on one Windows 11 computer. The original client ends every regular exit with an assertion of its own (no data loss observed).
- Automatic Windows detection of existing installations is temporarily off (see above).

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
