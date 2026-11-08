# Native Windows client: build, installer and status

TSW-Forever for Windows is the same Rust/Slint code base as the Linux build (`launcher/`, `launcher-gui/`, `patcher/`), compiled for `x86_64-pc-windows-msvc`. It starts the original *The Secret World* (2012) client directly. It uses no Wine, Proton or Flatpak, needs no administrator rights for normal operation (only the installation into Program Files does), and ships no game files, Funcom files or Microsoft components.

## What differs from Linux

| Area | Linux | Windows |
|---|---|---|
| Runtime | Wine/GE-Proton prefix managed by the launcher | none; the client runs natively |
| Game folder | inside the managed prefix | normal folder, found in `%ProgramFiles(x86)%\Funcom\The Secret World` and Steam libraries, or chosen by the user |
| Settings/data | XDG folders | `%APPDATA%` / `%LOCALAPPDATA%\TSW-Forever`; client preferences in `%LOCALAPPDATA%\Funcom\TSW\Prefs` |
| Funcom installer/patcher | started under Wine | started through the shell (`cmd /C start`), so Windows may show its permission (UAC) prompt; never bypassed |
| Completion check | patcher log plus Wine process signal | patcher log (`Patcher.log`) verdict |
| DirectX | Microsoft components downloaded after consent | not downloaded; `d3dcompiler_43.dll`, `d3dx9_43.dll`, `d3dx11_43.dll` are only detected and a missing one is a warning (install Microsoft's DirectX End-User Runtimes yourself, or let the Funcom installer do it) |
| FPS limit/counter, gamescope, shader cache, resolution via Wine | available | hidden or unavailable; resolution is written to the client's own preferences |

The platform differences live in `launcher/src/winnative.rs` (pure, unit-tested on Linux) and `Capabilities`; the Linux paths are unchanged.

## Build

On Windows with Rust (MSVC toolchain) and optionally Inno Setup 6:

```
powershell -NoProfile -File scripts\build_windows_installer.ps1            # executables, installer
powershell -NoProfile -File scripts\build_windows_installer.ps1 -SkipInstaller
```

The script reads `APP_VERSION` from `packaging/project.conf`, builds both crates with `--locked`, stages exactly four files (`tsw-launcher.exe`, `tsw-launch.exe`, `LICENSE`, `THIRD-PARTY-NOTICES.md`), and writes `build\windows\tsw-forever-<version>-windows-setup.exe`, and `SHA256SUMS-windows`. There is deliberately no portable zip: Windows is distributed as the installer only. It downloads nothing and signs nothing. The crates of `Cargo.lock` are fetched by `cargo` unless `-Offline` is given.

Cross checks possible on Linux (no linking): `cargo clippy --target x86_64-pc-windows-msvc --all-targets -- -D warnings` (and `-gnu`) in each crate. The test suites also compile for Windows; Linux-only fixtures are gated with `cfg(unix)`.

## Observed behaviour of the original Funcom installer (Windows 11, 2026-11-08)

Examined with the unmodified `TheSecretWorldInstaller.exe` from `https://cdn.funcom.com/downloads/tsw/client/TheSecretWorldInstaller.exe` (33 164 664 bytes, SHA-256 `9b9a71fb…d908e`, equal to the pinned value; Authenticode signature valid, signer `FUNCOM OSLO`). Observation only; the installer, its files and the game were not modified. A test installation was made into a non-default folder (`C:\Games\The Secret World`) to find out whether the chosen folder is recorded.

- **Technology:** an Inno Setup 5.4 installer (`TheSecretWorldInstaller.tmp /SL5=…` child process). It asks for administrator rights (UAC) and lets the user choose the folder.
- **Where the chosen folder is recorded:** `HKEY_LOCAL_MACHINE\SOFTWARE\WOW6432Node\Microsoft\Windows\CurrentVersion\Uninstall\The Secret World_is1` (32-bit view) with the values `Inno Setup: App Path` (`C:\Games\The Secret World`, no trailing separator) and `InstallLocation` (`C:\Games\The Secret World\`, with one), plus `UninstallString` (`unins000.exe` in that folder), `Publisher` (`Funcom`), `DisplayVersion` (`1.0.0`) and `InstallDate`. No key below `Software\Funcom` is created. The Start-menu group is `Funcom\The Secret World`.
- **What the installer puts down:** only the bootstrap of the original patcher (`ClientPatcher.exe`, `TheSecretWorld.exe`, `TheSecretWorldDX11.exe`, runtime libraries, `LocalConfig.xml`, `dbDebug.patcher` (`LogFile=Patcher.log`)); the game data (`RDB`, `Data`, …) is fetched afterwards by the original patcher, which the installer starts at its end.
- **Consequence for the launcher:** the launcher reads the key above (read-only, `winnative::registered_game_dirs`; the 32-bit and 64-bit views of `HKLM` and `HKCU` are asked), searches the recorded folder before the default folders (`%ProgramFiles(x86)%\Funcom`, Steam), and accepts it only when the original client `TheSecretWorldDX11.exe` is found there (`install::find_client`). Completion is never derived from the registry or from the existence of the files: the "Play" state needs the patcher's own confirmation (`install::gate_for`) or the user's explicit choice. With the partially patched installation of this test the command line reported "files exist, completion NOT confirmed" and did not release "Play".
- **Fallback:** if there is no key (portable copy, Steam, moved folder) the default folders are searched and the user selects the game folder by hand ("Use an existing TSW installation").

## Frame-rate limit on Windows (decision: none)

Linux limits the frame rate through DXVK (`dxvk.maxFrameRate`, handed to the game process as `DXVK_CONFIG`; `launcher/src/fps.rs`, config keys `fps_limit` and `fps_hud`, a 30 to 240 slider with a switch to turn the limit off). DXVK exists only under Wine/Proton, and the game's own text data holds no frame-rate preference, so the native Windows client has no mechanism the launcher could use without changing the renderer or injecting code. **Decision of the owner (2026-11-08): the Windows version has no frame-rate limit.** The FPS controls stay hidden on Windows (`Capabilities::fps_limit` is false), no DXVK and no other limiter is integrated, and the Direct3D rendering is not touched. The Linux slider and checkbox are unchanged. Anyone who wants a cap on Windows can use the graphics driver's own frame-rate limit.

## Settings and data locations (Windows)

- Settings (the chosen game, the launcher options, the installation state): `%APPDATA%\tsw-forever\config` and `install-state`, with `%APPDATA%` taken from Windows' Known Folder API (`FOLDERID_RoamingAppData`), not from the environment (the variable is only the fallback). `TSW_LAUNCH_CONFIG` still overrides the file for tests.
- Larger data (downloaded installer, caches, logs): `%LOCALAPPDATA%\tsw-forever\`.
- Migration: on the first start the files of the earlier `%APPDATA%\tsw-launch\` folder are copied into the new folder. Existing files are never overwritten, nothing is deleted or moved, so an older version keeps working and no user data can be lost. An earlier `%LOCALAPPDATA%\TSW-Forever` is the same folder (Windows folder names are case-insensitive).
- Nothing changeable is stored in the program folder (`C:\Program Files\TSW-Forever`).

## DirectX help (missing Direct3D libraries)

The original 32-bit client needs `d3dcompiler_43.dll`, `d3dx9_43.dll` and `d3dx11_43.dll` (DirectX End-User Runtimes, June 2010; not part of Windows 10/11). The launcher checks `SysWOW64` and `System32` (`winnative::directx_missing_now`). If something is missing, the main view (not the first-run view) says which libraries and offers two buttons: **Install DirectX components** and **Check again**.

- Nothing happens without the click. The button downloads **Microsoft's own web installer** `dxwebsetup.exe` from `download.microsoft.com` (https only, certificate checked, `curl.exe` of Windows), compares its SHA-256 with the pinned value of the examined release (9.29.1974.0, 295 320 bytes) and asks Windows for its Authenticode signature (valid, signer Microsoft Corporation). Only then it is started through the shell, so Windows shows its permission prompt and Microsoft's installer shows its own terms. The launcher passes no switch that hides or accepts anything, extracts nothing and distributes no Microsoft file (`launcher/src/dxsetup.rs`).
- If Microsoft publishes a newer file the hash no longer matches: the launcher then refuses and names the official page (`https://www.microsoft.com/download/details.aspx?id=35`); the pin is updated after the new file was examined.
- After the installation the user clicks **Check again**; with everything present the block disappears and nothing else changes.
- On the development machine the libraries exist. `TSW_LAUNCH_TEST_DIRECTX_MISSING=all` (or a comma list of file names) simulates missing ones for a test; real users do not set it. The real download is covered by an ignored network test (`cargo test --lib dxsetup -- --ignored`).

## Install a new copy in another folder (all users)

A game installation that the launcher found must never be the only possible state. In the **advanced view** (INSTALLATION card) and, while an installation is found but not confirmed, as a link in the main view, **Install a new copy in another folder …** lets the user start a complete new installation:

1. The user picks a folder. It must be a full local path, not a drive root, not the Windows folder, not a network share, safe to hand to the installer (no `& % ^ | < > " !`), **not overlapping any existing installation**, and either not existing yet or empty (`install::validate_new_target`). Nothing is created or changed during the check.
2. Every installation the launcher can see (detection, configuration) is **ignored from then on**: it is not detected, searched, listed or adopted, and a stored client path that points into it is forgotten. It is never opened for writing, changed, moved or removed. The decision is stored in `install-state` (`ignored_install`, `install_target`) and applied to every detection (`State::apply_search`, `winnative::set_user_search`). **Use the ignored installation again** in the advanced view takes it back.
3. The window shows the first-run choices again (download / installer from disk / existing installation). The folder of the new copy is shown as the game folder and is searched first.
4. The original Funcom installer is started with `/DIR="<new folder>"` (`winnative::start_line_with_dir`) and the notice says to check that the installer shows exactly this folder, otherwise to cancel. Why: Inno Setup reuses the folder of an earlier installation of the same application ID without asking, and rewrites its uninstall entry. A probe with an Inno Setup 6.7 installer (own test ID, temporary folders) showed that an explicit `/DIR` **overrides** the earlier folder: the files of the second run went into the given folder and the first folder stayed untouched; without `/DIR` the earlier folder was reused. The Funcom installer uses Inno Setup 5.4; this was not verified with that version, which is why the user is told to check the folder in the installer's window. A destination that cannot be quoted safely means the installer is not started at all.
5. An existing registry entry of the old installation is rewritten by the Funcom installer to the new folder (this is the installer's own behaviour, not ours); the files of the old installation are not touched.

## Isolated first-installation test (test mode)

To try the complete first installation on a computer that already has the game, the launcher can be told to ignore an existing installation. This is done with environment variables of ONE launcher process, never with registry or file changes:

- `TSW_LAUNCH_EXCLUDE_DIRS=<folder>[;<folder>…]`: nothing in these folders is found, searched, listed or adopted: not by the registry detection, not by the drive search, not through a stored client path, not through "Use an existing TSW installation" (refused). Folders are compared by name only (case-insensitive, canonical, whole folder names), never opened or changed.
- `TSW_LAUNCH_CONFIG=<file>` and `LOCALAPPDATA=<folder>`: a fresh configuration and fresh launcher data, so a stored choice cannot skip the first-run dialog.
- Normal users never set these variables, so they have no effect for them. The normal start (Start menu) is unchanged.
- In test mode the notice before the Funcom installer is replaced: instead of "keep the suggested folder" it says to choose a NEW, empty folder.
- `build\windows\Start-FirstInstallTest.cmd` sets all of this for one launcher start, deletes only its own test folder `%LOCALAPPDATA%\tsw-forever-test`, and compares the protected installation read-only with a manifest before the start. `build\windows\Verify-Protected.cmd` repeats the comparison afterwards.
- **Limit that the launcher cannot remove:** the original Funcom installer (Inno Setup) uses the same application ID as an existing installation. Inno Setup hides the folder page for a detected earlier installation by default and may then install into the old folder; it also rewrites the uninstall registry entry. The test mode cannot prevent that, because the installer is not ours. Whoever runs the original installer next to a protected installation has to pick a new folder, abort if no folder page appears, and keep a registry export (`reg export`) of the key `HKLM\SOFTWARE\WOW6432Node\Microsoft\Windows\CurrentVersion\Uninstall\The Secret World_is1`.

## First start

Without a usable game installation the Windows window offers exactly three choices: **Download installer and go** (the official Funcom installer, checked against the pinned SHA-256, after the download consent), **Provide the installer from disk** (a file the user selects; its fingerprint has to be confirmed unless it is the pinned one) and **Use an existing TSW installation** (the user selects the game folder; nothing is copied or changed). The texts are translated (English, German); the language follows the Windows display language, English is the fallback for unsupported languages, and a saved language choice wins.

## Runtime dependencies and the `VCRUNTIME140.dll` problem

Cause: Rust's MSVC target links the Visual C++ runtime dynamically by default. `tsw-launcher.exe` imported `VCRUNTIME140.dll` and the `api-ms-win-crt-*` DLLs (Universal CRT); a freshly installed Windows 11 has no Visual C++ Redistributable, so the launcher failed to start. A machine with Visual Studio, its Build Tools or many other programs has the runtime, so a successful start there proves nothing. It is a dependency of the launcher itself, not of the game, DirectX or a third-party library.

Fix: `.cargo/config.toml` sets `-C target-feature=+crt-static` for `cfg(all(windows, target_env = "msvc"))` only, so Linux and Flatpak builds are unchanged. No Microsoft DLL is bundled and no redistributable is required. The remaining imports are Windows system DLLs (`kernel32`, `user32`, `gdi32`, `dwmapi`, `shell32`, `comctl32`, `dwrite`, `ws2_32`, `uxtheme`, ...). `scripts/build_windows_installer.ps1` fails the build if an executable lists `vcruntime140`, `msvcp140`, `ucrtbase` or `api-ms-win-crt-*`. Check by hand with `dumpbin /dependents tsw-launcher.exe` (Visual Studio Build Tools, "x64 Native Tools" prompt). The source archive carries `.cargo/config.toml`, so builds from source behave the same.

## Development environment (Windows 11)

Installed with `winget` from the official sources: Python 3.12 (`Python.Python.3.12`), Rustup (`Rustlang.Rustup`) with toolchain `1.99.0` (components rustfmt and clippy; the version pinned in `windows.yml`), Visual Studio 2022 Build Tools with the "Desktop development with C++" workload and the Windows SDK (`Microsoft.VisualStudio.2022.BuildTools`, `--add Microsoft.VisualStudio.Workload.VCTools --includeRecommended`), Inno Setup 6 (`JRSoftware.InnoSetup`). Git and the GitHub CLI are expected; Git Bash runs the POSIX scripts. Some Python scripts and tests are Linux-only (`fcntl`, `mkfifo`, symlinks, `rsync`, `flatpak`) and fail on Windows by design; the Linux CI runs them.

Local check cycle: `cargo fmt --check`, `cargo clippy --locked --all-targets -- -D warnings` and `cargo test --locked` in `patcher`, `launcher`, `launcher-gui`; `powershell -NoProfile -File scripts\build_windows_installer.ps1`; `powershell -NoProfile -File scripts\test_windows_installer.ps1` (asks for the UAC confirmation; installs to Program Files, tests a custom Unicode folder, start without administrator rights, update and uninstall); `python scripts/version.py check`, `python -I scripts/prepublish_check.py --git` and the Python policy tests.

## Installer decision

Chosen technology: **Inno Setup** (script `packaging/windows/tsw-forever.iss`). Reasons: native 64-bit install with `PrivilegesRequired=admin` for the installation only (target `{autopf}\TSW-Forever`, i.e. `C:\Program Files\TSW-Forever`; the folder page offers a custom folder; the launcher is started afterwards with the original user's rights), Start-menu group, optional desktop icon, an uninstaller entry in "Apps & features", English/German wizard, one small text script that is easy to review, and a free compiler that is preinstalled on GitHub's Windows runners. Alternatives rejected: MSI/WiX (heavier tooling and a per-machine default), NSIS (less readable script), MSIX (needs signing to install at all). The uninstaller removes only the installed program files; user settings and the game installation are never touched. The installer contains no game files and no Microsoft or Funcom components.

## Workflow

`.github/workflows/windows.yml` is a manual (`workflow_dispatch`, no inputs) or called (`release-set.yml`), main-or-version-tag-only, read-only, secret-free workflow on `windows-latest`. It checks the version, runs the Rust test suites, builds the installer and uploads the unsigned artifacts for 14 days. It publishes nothing and creates no tag or release itself; the release is made by `release.yml`. The policy test `scripts/check_flatpak_package.py ci-policy` enforces these properties.

## Signing

The installer and executables are **not code-signed**. Windows SmartScreen will warn for unsigned downloads. Signing needs an owner-supplied certificate and a separate decision; no key or secret is part of the repository.

## Temporarily paused automatic detection (Windows)

`winnative::AUTODETECT_PAUSED_ON_WINDOWS` (currently `true`, owner decision of 2026-11-08) switches the automatic detection off on Windows so that a full first installation can be tested on a machine that already has the game. While it is on:

- nothing is found through the registry, the drive search, the default folders (Program Files, Steam) or a stored path of an old configuration, installation state or detection cache; the first-run choices appear at once;
- choosing an installation by hand ("Use an existing TSW installation") works; it records `choice_made` in `install-state`, and from then on stored paths count (a stale path in an old configuration does not block the choice);
- when the launcher starts the original installer it records `install_started` and the folders the registry named at that moment (`registry_baseline`); afterwards the registry is read again but only for entries that are NEW compared with that baseline, so the new installation is found and an old one stays invisible; the state survives a restart;
- Linux is unaffected (`set_autodetect` pauses only `cfg!(windows)`). Set the constant to `false` to bring the automatic detection back.

## Status of the evidence (2026-11-08)

| Level | State |
|---|---|
| Implemented | yes |
| Cross-compilation checked | Clippy `-D warnings`, all targets, `x86_64-pc-windows-msvc` and `-gnu` on Linux |
| Built natively on Windows | yes: GitHub-hosted `windows-latest`, workflow `windows.yml`: rustfmt, Clippy, compile and link, unsigned Inno Setup installer, checksums |
| Tested natively on Windows | yes: all Rust test suites of the three crates; silent per-user install of the installer, Start-menu entry, uninstall entry, installed `tsw-launch.exe --version`, clean uninstall, neighbouring folders untouched. Linux/Wine-only tests are excluded with `cfg(unix)` (reasons in the code) |
| Combined release set | `release-set.yml`: Flatpak + source archive + Windows installer + one `SHA256SUMS` from one commit, verified with `check_release_set.py --with-windows` |
| Practically tested with the original game | partly: on a real Windows 11 PC the owner installed with the local installer of 2026-11-08, ran the complete first installation through the launcher and the original Funcom installer, and started the game into the world (an earlier session of the same day: game start, login and exit). The original client ends every regular exit with its own assertion (`GraphicImpl.cpp:360`, exit code 0, no data loss); no Awesomium problem or start race was reproduced (local investigation report, not in Git) |

Open points: code signing of the installer and executables (an owner decision; until then SmartScreen shows a warning), and switching the temporarily paused automatic detection of existing installations back on.
