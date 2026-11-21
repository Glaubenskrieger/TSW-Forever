# The Secret World – Forever (TSW-Forever)

TSW-Forever is a free, unofficial launcher for the **original The Secret World (2012)** on **Linux and SteamOS** (Flatpak, through Wine/Proton) and on **Windows 10/11** (native, with an installer). It helps install the original game and remembers your game display and performance settings. The launcher window is available in English, German, French, Spanish, Brazilian Portuguese and Polish. It is not for **Secret World Legends**, and it provides no game servers or account registration.

**Version `0.4.0`.** This guide covers installing the Linux Flatpak bundle (sections 2–9) and the Windows installer (section 2a). Download files from the public Releases page and check that the listed asset names match this guide. Hardware compatibility beyond the reported tests is not guaranteed.

## 1. Requirements

- **Linux:** a Linux **x86-64** computer, including compatible SteamOS devices. ARM systems are not supported.
- **Windows:** Windows 10 or 11, 64-bit (x64). The game runs natively there; no Wine or Proton is involved (see section 2a and `documentation/building-windows.md`).
- A valid account entitled to the **original The Secret World**. A Secret World Legends account alone does not establish access. The launcher cannot create an original-game account or grant game rights.
- Flatpak, an internet connection, working Vulkan graphics drivers with the required 32-bit support, and XWayland for the tested Wine/Proton path.

You do not need to install a separate system Wine/Proton for the normal Flatpak setup: the launcher can download its checked GE-Proton runner. Gamescope is not required.

Check that Flatpak is available:

```sh
flatpak --version
```

If the command is missing, install Flatpak through your distribution's software manager. On Ubuntu/Debian, the terminal command is `sudo apt install flatpak`; on regular Arch Linux it is `sudo pacman -S flatpak` (**not on SteamOS**). Fedora Workstation, Silverblue and Kinoite normally include Flatpak already. After first installing Flatpak, log out and back in or restart. Other distributions have [official setup instructions](https://flathub.org/en/setup).

## 2. Download and install

### Download the bundle

Open the [TSW-Forever Releases page](https://github.com/Glaubenskrieger/TSW-Forever/releases). Under **Assets**, download these two files into the same folder:

- `tsw-forever-0.4.0.flatpak` — the Linux application installer.
- `SHA256SUMS` — the matching checksums.

The release also contains `tsw-forever-0.4.0-windows-setup.exe` (the Windows installer, section 2a) and the optional `tsw-forever-0.4.0-src.tar.gz` with the source code; the source archive is not needed to install or use the launcher. The GitHub-generated “Source code” ZIP is not an application installer. Use the project's release files rather than an arbitrary re-upload. Do not use an unverified re-upload.

Open a terminal **in the download folder** (many file managers offer “Open Terminal Here”). `./` in the following commands means that folder. Check the files:

```sh
sha256sum --check --ignore-missing SHA256SUMS
```

The result must include `tsw-forever-0.4.0.flatpak: OK`. The optional source archive is checked too if present. If verification fails, stop and download the matching files again.

### Install for your user

Install the local bundle:

```sh
flatpak install --user ./tsw-forever-0.4.0.flatpak
```

Read and confirm Flatpak's permission and installation prompts. **Do not use `sudo` for these commands.** The application ID is `org.tswlauncher.Launcher`.

The bundle needs **Freedesktop Platform 25.08**, plus its 32-bit compatibility and graphics components. Accept any required runtime downloads offered by Flatpak. If your Flatpak installation has no configured source for the runtime, enable Flathub through your distribution's software manager or follow the [official Flatpak setup guide](https://flathub.org/en/setup), then retry. The development SDK is not needed for installation. A working desktop portal is needed for the terms links and file chooser; minimal Linux desktops may need their distribution’s `xdg-desktop-portal` package and the backend matching that desktop. SteamOS Desktop Mode normally provides this integration.

**AMD/Intel Mesa graphics:** also provide the matching 32-bit Mesa graphics extension, which this bundle does not automatically download:

```sh
flatpak install --user flathub org.freedesktop.Platform.GL32.default//25.08
```

**Proprietary NVIDIA graphics:** the matching **32-bit GL32 NVIDIA extension must also be installed explicitly**; this bundle does not automatically download GL32 extensions. Use the extension matching your active NVIDIA driver, not the Mesa command above. The exact extension name depends on that driver; if it is missing, provide the output of `flatpak --gl-drivers` when reporting the problem. NVIDIA installation/gameplay has not been practically verified. Keep the supported host driver current and run `flatpak update --user`; if the game reports missing 32-bit Vulkan support, report that exact error rather than installing the Mesa extension as a generic fix.

### 2a. Windows installation

Open the [TSW-Forever Releases page](https://github.com/Glaubenskrieger/TSW-Forever/releases) and download `tsw-forever-0.4.0-windows-setup.exe` and `SHA256SUMS` into the same folder. Compare the installer's checksum with the matching line of `SHA256SUMS` in PowerShell:

```powershell
Get-FileHash .\tsw-forever-0.4.0-windows-setup.exe -Algorithm SHA256
```

Then run the installer. It asks Windows for administrator permission once (UAC stays on) and installs the launcher below `C:\Program Files\TSW-Forever`. Its wizard is available in the six languages of the launcher; the language dialog at the start lets you choose.

**The installer is not code-signed.** Windows may show "Windows protected your PC" (SmartScreen): choose *More info*, then *Run anyway*, only after you checked the SHA-256 value. A computer with *Smart App Control* switched on can refuse unsigned programs altogether.

**Updating an existing installation:** run the new installer over the old one. It recognises the installed version and offers an update; the program folder is reused, a running launcher is asked to close first, and your settings (`%APPDATA%\tsw-forever`), the chosen game folder and the game's own files are kept. **Uninstall** through *Settings → Apps*; the game itself and your settings are not removed.

At the first start the launcher offers three choices: download the official Funcom installer, use an installer you already have, or use an existing installation. The launcher never includes or modifies the game files on its own. When a newer stable release exists on GitHub, a small notice appears at the bottom right of the window; its button opens the release page in your browser. **Windows has no automatic update:** nothing is downloaded or installed by the launcher. More details: `documentation/user-guide.md`.

## 3. First start and setup

Open **The Secret World – Forever** from your application menu, or run:

```sh
flatpak run --user org.tswlauncher.Launcher
```

If the menu entry has not appeared yet, log out and back in; the command above can be used immediately. On SteamOS, perform the initial setup in Desktop Mode. Adding the installed launcher to Steam as a non-Steam application is optional; Gaming Mode/controller behaviour is not generally validated.

### Install the original game

1. On the setup screen, read the download information and the linked original Microsoft SDK/runtime terms. The links open online copies in your browser. The runtime page also contains other product terms; only its Microsoft runtime section is relevant here.
2. Select **I agree to the downloads** and the separate **Microsoft SDK/runtime terms** checkbox only if you agree to each. Both can be selected before anything is downloaded or installed. Then press **INSTALL THE GAME**. The launcher fetches the original Funcom installer, GE-Proton runner and Microsoft Direct3D components from their configured original sources and verifies pinned checksums. If an upstream file changes and verification fails, stop and report it; do not bypass the checksum check. Without both choices, installation does not start. Leaving Microsoft unchecked declines that installation step.
3. Keep the original Funcom installer's **default destination and settings**. Its Windows path belongs to the launcher's managed Wine environment; do not redirect it to another application's Wine prefix.
4. Let the original Funcom patcher download and verify the game. If interrupted, reopen the launcher and continue the offered step; the patcher can resume.
5. Close the Funcom patcher when it has finished if necessary. The launcher switches to **PLAY** when completion is recognised. If it does not, close the patcher and press **VERIFY INSTALLATION**.

Microsoft acceptance is stored locally for the exact checked package and terms. It is not requested at every game start; a different package/terms requires a new check before new component installation. Existing installed profiles are not overwritten merely to ask again.

The managed game, Wine prefix, runner and installation downloads live under `~/Games/The Secret World/`. Launcher settings and additional app data live under `~/.var/app/org.tswlauncher.Launcher/`. The prefix contains game preferences and may contain account/session information: do not treat it as disposable settings or share it publicly.

### Use an existing installation instead

Open the **gear → Advanced → INSTALLATION → Use an existing installation …**, then select the original game's `TheSecretWorldDX11.exe`. The launcher adopts the selected installation in place rather than downloading another copy. It still needs a compatible Wine/Proton environment and access to all required game files; choosing an executable alone does not provide these.

Flatpak normally shares only `~/Games/The Secret World/`, plus its own app data and files explicitly granted through the desktop file chooser. An executable picked from elsewhere does **not** necessarily grant access to its surrounding game data. If the launcher cannot access the complete installation, use a separate managed installation rather than granting access to your entire home folder. Do not initialise, overwrite or “repair” a Lutris/Steam/other application's prefix through this launcher. Incomplete files may require verification with the original patcher.

If you already have a trusted Funcom installer, **Advanced → INSTALLATION → Use a local installer …** selects it. An unknown fingerprint needs a separate confirmation; do not approve an installer whose origin you do not trust.

## 4. Play and display settings

Press **PLAY**, then sign in with your original-game account in the game's own login window. The launcher normally minimises while the game runs and closes after the game ends.

The gear opens Advanced; the arrow in the same position returns to the main view. In **DISPLAY**:

- **Game resolution:** “Managed by the game” is the default. An explicit choice is saved and reapplied on every start using a private Wine desktop. It affects the game, not the Linux/SteamOS desktop resolution. Only detected modes are offered. After changing displays, choose an available mode or the game-managed default if a saved mode is unavailable.
- **Frame rate:** the free slider ranges from **30 to 240 FPS**, default **60 FPS**. **Unlimited** removes the launcher's cap; the game or compositor can still impose another limit.
- **DXVK FPS counter:** enable its checkbox to display the DXVK frame-rate overlay.

A working normal installation does not need manual compiler-profile changes. Avoid profile or shader-cache maintenance while playing.

## 5. Manual updates

**The Flatpak bundle of this release carries the project's signed update source** (https://glaubenskrieger.github.io/TSW-Forever/repo, signed with the project's GPG key; Flatpak checks every update against that key). Once it is installed, the launcher offers new versions itself (**Update launcher**), and `flatpak update` and your software center use the same source. If you installed an earlier single-file bundle (0.3.0 or older), install this bundle once; the one-time move is described in `documentation/user-guide.md` (“Moving an earlier installation”). (On Windows see section 2a.)

You can also add the update source without the bundle: `flatpak install --user https://glaubenskrieger.github.io/TSW-Forever/org.tswlauncher.Launcher.flatpakref` (the same project key is used).

To install a bundle by hand (for example without an internet connection to the update source), close the game and launcher, download the bundle and its matching `SHA256SUMS` from the approved release page, check them as above, then install the file. This command can also reinstall the current version:

```sh
flatpak install --user --reinstall ./tsw-forever-0.4.0.flatpak
```

For a future version, replace the filename with the one you actually downloaded. Confirm the prompt, then start the launcher normally. You do not need to uninstall the old app first or use `--delete-data`. This updates the same user-installed app and Flatpak branch; it does not import a system-wide installation, a different branch or another launcher’s Wine prefix. If you used another installation layout, preserve it and report the mismatch rather than deleting or moving game data.

A bundle update/reinstallation replaces the application and keeps its Flatpak app data/settings and the separate game folder. Back up your settings and game prefix before maintenance. An isolated update between earlier private builds preserved test settings/data, and the owner has confirmed installation and game start on one Steam Machine. Recovery after an interrupted update and migration from other installation layouts are not established by those tests.

## 6. Uninstall

Close the game and launcher, then remove the application:

```sh
flatpak uninstall --user org.tswlauncher.Launcher
```

Normal removal leaves launcher settings/app data in `~/.var/app/org.tswlauncher.Launcher/` and the managed game, prefix, runtime, downloads and caches in `~/Games/The Secret World/`. Flatpak's shared runtimes can also remain for other applications.

If you also want to remove launcher settings/app data, back them up first, then remove `~/.var/app/org.tswlauncher.Launcher/` separately in your file manager after uninstalling. Enable “Show Hidden Files” (usually Ctrl+H) to see `.var`. This removes settings and any data stored inside that app directory. The Microsoft acknowledgement and verified component cache are stored separately under `~/Games/The Secret World/cache/native/` in the default managed layout; removing Flatpak app data does not remove them.

The external game folder and adopted installations are separate. Delete `~/Games/The Secret World/` only if you intend to delete the managed game, its Wine prefix, preferences, runner and caches. Never delete an unrelated Lutris/Steam installation as part of uninstalling this app.

## 7. Troubleshooting and known limitations

| Problem | What to do |
|---|---|
| `flatpak: command not found` | Install Flatpak as described under Requirements, then log out/in or restart. |
| Bundle file not found | Open a terminal in the folder containing the downloaded `.flatpak`; check the exact filename, including any browser-added suffix. |
| “Already installed” | Use the manual-update command with `--reinstall`. Keep `--user` consistent; do not create an accidental second system-wide installation. |
| App not installed when starting | Run `flatpak info --user org.tswlauncher.Launcher` to check the user installation; install the downloaded bundle if missing. |
| Missing runtime / no Flatpak source | Enable Flathub as described in the installation section. Once it is configured, install the runtime with the command below and retry the bundle. Check internet/proxy access if downloads fail. |
| Missing Vulkan/32-bit graphics support or game will not start | Update your distribution's graphics drivers and Flatpak runtimes. The game needs working 32-bit graphics support as well as a working launcher GUI. See the graphics note below. |
| Patcher finished but no PLAY button | Close the Funcom patcher and use VERIFY INSTALLATION. Completion detection may be unavailable without a working sound server. |
| Saved resolution unavailable | Reopen DISPLAY and select a current mode or “Managed by the game”. If detection fails, the game-managed default remains usable. |
| File chooser or existing game inaccessible | Check that your desktop's file portal is working. A single-file permission may not cover sibling game files; use the managed installation path instead. |
| Unproven/damaged prefix refused | Preserve it and report the error; do not delete ownership records or force initialisation of another program's prefix. |

To provide the base runtime and 32-bit compatibility component explicitly:

```sh
flatpak install --user flathub org.freedesktop.Platform//25.08 org.freedesktop.Platform.Compat.i386//25.08
```

For AMD/Intel Mesa graphics, check that the 32-bit extension from the installation section is installed. NVIDIA needs extensions matching the active NVIDIA driver; do not substitute the Mesa command as a universal NVIDIA fix. Use your distribution's supported driver setup and report the exact error if unsure. To update installed user runtimes and driver extensions:

```sh
flatpak update --user
```

The owner confirmed installation and game start on one Steam Machine with a preceding private test build using the same game and installation logic. Earlier private tests on that device also covered multiplayer and persistent resolution. The prior Flatpak passed isolated installation/update and basic Wayland GUI checks. These reports do not separately establish browser/portal interaction or recovery after interrupted installation. This is not a guarantee for every Linux distribution, GPU, Steam Deck, multi-monitor/HDR setup or Gaming Mode. Windows is future work. Original game updates still come from Funcom's patcher; the launcher's client-patch channel is inactive.

### Report a problem

Use the [project issue tracker](https://github.com/Glaubenskrieger/TSW-Forever/issues). If it is not available yet, contact the person who provided your test build.

Include the launcher version, distribution/SteamOS version, desktop/session, GPU/driver, installation method, steps to reproduce and exact error. The following prints the installed launcher version:

```sh
flatpak run --user --command=tsw-launch org.tswlauncher.Launcher --version
```

Diagnostic logging is optional and off by default; no automatic upload or telemetry is added. If you create a diagnostic package, **read it before sharing**. Filtering reduces sensitive data but cannot guarantee its absence. Never post passwords, account/session keys, raw game logs or an entire Wine prefix.

## 8. Legal notices and licences

TSW-Forever is an independent, non-commercial community project, **not affiliated with, endorsed by or sponsored by Funcom or Microsoft**. Game and product names belong to their respective rights holders. A free launcher does not grant rights to the game or proprietary components.

**No original game files, Funcom installer, Microsoft DLLs or Wine/Proton runner are included in the bundle or source archive.** Required components are obtained separately on the user's machine with consent. Microsoft's terms apply to its components; private use and interoperability are not blanket permission to disregard licences. The original terms can also be read before setup via these checked online copies: [SDK terms](https://github.com/apitrace/dxsdk/blob/d964b66467aaa734edbc24326da8119f5f063dd3/Documentation/License%20Agreements/DirectX%20SDK%20EULA.txt) and [Microsoft runtime section](https://store.steampowered.com/eula/17300_eula_1#:~:text=MICROSOFT%20DIRECTX%20END%20USER%20RUNTIME). These are not Microsoft-hosted pages; the runtime page also contains unrelated product terms. Public-distribution legal questions remain subject to separate review; this README is not that clearance. These bundles are not project-GPG-signed. Check the release source and SHA256SUMS; checksums detect corruption but do not by themselves authenticate an untrusted download source.

Own launcher code is **GPL-3.0-or-later**; the combined Slint GUI is distributed under **GPLv3**. The matching source archive is available alongside the bundle. Complete installed licence/copyright notices are in `/app/share/licenses/tsw-forever/` inside the Flatpak. The optional matching source archive contains `LICENSE`, `THIRD-PARTY-NOTICES.md` and `LICENSES/README.md`; you do not need to download it or read another project document to follow this installation guide.

## 9. Building from Source

Use the source archive or a checkout in a separate build directory. You need stable Rust/Cargo (tested with Rust 1.99), a C/C++ compiler, `pkg-config`, and development libraries for fontconfig, FreeType, xkbcommon, Wayland, X11, XCB, Xcursor and Xi. Cargo needs the locked Rust dependencies available locally or network access to fetch them. From the source root:

```sh
cargo build --release --locked --manifest-path launcher/Cargo.toml
cargo build --release --locked --manifest-path launcher-gui/Cargo.toml
```

The programs are `launcher/target/release/tsw-launch` and `launcher-gui/target/release/tsw-launcher`. Keep them in the same directory when running the GUI. The optional `scripts/install_native.sh` stages both programs, desktop integration and licence notices; run it with a destination directory rather than a system path until you have reviewed its options. A native game launch also needs suitable 32-bit Wine dependencies and Vulkan drivers.

For a local Flatpak build, install `flatpak-builder`, `rsync`, Python 3, Freedesktop Platform/SDK 25.08 and the matching `rust-stable` SDK extension. The build script stages the required source files, vendors locked Rust dependencies, then builds offline:

```sh
scripts/build_flatpak.sh check
scripts/build_flatpak.sh vendor --allow-network
scripts/build_flatpak.sh build
scripts/build_flatpak.sh bundle
```

The resulting local bundle is `build/flatpak/tsw-launcher.flatpak`. Vendoring requires network access unless the required crates are already cached. These commands build locally; they do not publish an update repository or a GitHub release.
