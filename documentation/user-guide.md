# User guide — Linux and SteamOS

TSW-Forever is an unofficial launcher for the original **The Secret World (2012)**. It is not Secret World Legends, is not affiliated with Funcom or the rights holders, and provides neither a game server nor account management. You need a valid account entitled to the original game. The launcher cannot create that entitlement; a Legends account is not proof of access to the original. This project does not promise that new original-game accounts can currently be purchased or registered.

## Requirements and download

The official binary format is Flatpak for Linux x86-64; source is also available. A working Vulkan-capable graphics driver, Flatpak, network access and XWayland for the tested Wine path are required. The Flatpak supplies its runtime environment; the launcher can obtain its pinned GE-Proton runner. A native source build requires its own Wine/Proton and 32-bit dependencies: see `building-from-source.md`.

The owner tested installation, GUI, game start, multiplayer and resolution persistence on one Steam Machine using private builds. The version of this guide is `0.4.0`. Other distributions and GPUs are potential Flatpak targets, not fully validated platforms. A native Windows 10/11 (x64) launcher with its own installer is part of this release (see "Updating the launcher (Windows)" and `building-windows.md`); the window is available in English, German, French, Spanish, Brazilian Portuguese and Polish.

Use the [public release page](https://github.com/Glaubenskrieger/TSW-Forever/releases), not an arbitrary re-upload. The public README contains the complete installation, update and uninstall procedure. Download the bundle and matching `SHA256SUMS` together; the source archive is optional for installation. For version `0.4.0`:

```sh
sha256sum --check --ignore-missing SHA256SUMS
flatpak install --user ./tsw-forever-0.4.0.flatpak
flatpak run --user org.tswlauncher.Launcher
```

Checksums detect changed downloads; a checksum file from the same compromised channel is not an independent signature. The Flatpak bundle carries no signature of its own and there is no update source, so its integrity rests on the SHA-256 value (compare it before you install) and on GitHub's release hosting (see “Updating the launcher (Linux)”). No game, Funcom installer, Microsoft DLL or Wine/Proton runner is inside our release artifacts.

## First installation

Press **INSTALL THE GAME**, read the download information and consent only if you agree. Additional downloads total about 700 MB for the installer, runner and Direct3D components, followed by roughly 20 GB of original game data. Allow additional space for the prefix, unpacking and caches. Microsoft components have their own terms; the EU/German assessment and residual public-review question are described in `microsoft-components.md`. The complete original terms are accessible through online-copy links in the existing setup view. Read them, select the independent Microsoft checkbox and download checkbox, then press INSTALL THE GAME. No pre-download is needed just to enable agreement. Opening a link does not accept anything. Acceptance remains locally bound to the exact package/text fingerprints, which are still verified before DLL provisioning. Existing installed profiles are not rewritten or prompted at every game start.

Keep the original installer's default destination and settings. The original Funcom patcher downloads and verifies the game. It can resume after interruption. The launcher observes progress and changes to **PLAY** after completion is recognised. You may need to close the patcher manually; that is supported and not a failure. If PLAY does not appear, close the patcher and use **VERIFY INSTALLATION**. Missing files alone do not prove completion.

You may choose a local installer; an unknown fingerprint requires separate confirmation. Do not confirm an installer whose origin you do not trust. **Use existing installation** adopts original data rather than granting new game rights. In Flatpak, paths must be accessible through the game-folder permission or an appropriate file/directory portal grant. Selecting one executable does not guarantee access to every sibling data file. Do not point prefix initialisation at a Lutris/Steam/other application's prefix. Unproven or damaged ownership is refused; choose a new empty managed folder rather than deleting ownership records.

## Storage and settings

- Managed installation: `~/Games/The Secret World/`, containing `prefix`, `runtime`, `downloads`, `cache` and `logs`.
- The Wine prefix contains the Windows-side game and its user preferences; it is not merely a disposable launcher setting.
- Flatpak settings: `~/.var/app/org.tswlauncher.Launcher/config/tsw-launch/`.
- Native settings: `~/.config/tsw-launch/`.

A normal bundle update replaces the application, not the external game folder. Back up settings and the prefix before maintenance. Never share the entire prefix: the original client can write account/session information there.

## Play and display

Press PLAY. The launcher minimises while the game runs and handles its own exit when the game ends. The gear opens Advanced; the arrow at the same position returns. Escape also leaves Advanced.

**Display:** Managed by the game is the default. A selected mode is obtained from the active game display environment, saved and reapplied at each start using a private Wine desktop and user preferences. The Linux desktop resolution is not changed. After changing monitors or display environment, an unavailable saved mode is refused explicitly: choose an available mode or Managed by the game. Detection failure leaves the managed default available.

The FPS slider remains freely adjustable from 30 to 240, default 60; Unlimited removes the launcher's cap. The DXVK checkbox controls DXVK's FPS counter. Other game/compositor limits may still apply. No mandatory Gamescope dependency exists; legacy Gamescope and manual override panels remain hidden.

## Compiler profile and shader cache

The normal installation uses `d3d-native-ms2010`: the three checked 32-bit compiler/D3DX components obtained from Microsoft, not supplied by this project. A working installation needs no manual profile change. The advanced manual profile path is for expert recovery; the older `d3d-native-v7` includes compiler 47 and is experimental, not the supported default. Do not source DLLs from random download sites. Profile changes affect only a managed prefix and retain backups, but interrupted multi-file maintenance is not guaranteed to roll back automatically.

Shader cache rebuild renames the local cache aside; the game rebuilds it. Undo restores the saved cache. It does not repair arbitrary game data. Avoid cache/profile maintenance while the game is running.

## Diagnostics and reporting

Diagnostic logging is voluntary and off by default. Nothing is automatically uploaded; no telemetry service is added. Basic/detailed modes and a user-triggered diagnostic package are available. Saved environment entries are structurally omitted from the package, and known literal values are removed from included records. Other personal information, paths or unknown secrets may remain because masking is heuristic. Read the entire package before sharing it. Never upload raw ClientLog files, a Wine prefix or a raw `--log-file`.

Before you contact the maintainer about a problem, collect the launcher version, distribution, desktop/session, GPU/driver, installation method, the exact steps and the visible error. Share only a personally reviewed, reduced diagnostic package. A security problem, and anything that contains secrets, must not be posted publicly: report it privately.

## Updating the launcher (Linux and Windows)

From 0.4.1 on, no platform updates the launcher by itself. When a newer stable version has been published, a small notice appears at the bottom right of the window ("A new version of TSW-Forever (<version>) is available.") with the button **Open download page**. The button opens the download area of the official web site (https://glaubenskrieger.github.io/TSW-Forever/) for your system: the Linux area on Linux, the Windows area on Windows. The notice appears again at the next start until you have updated. The launcher downloads and installs nothing by itself, and without an internet connection no notice is shown. At each start it only checks whether a newer version exists: it sends one plain HTTPS request to the latest-release address of the project on GitHub (headers only, without following the redirect) and reads the version number from the answer; the request carries no identifier of your installation. On Windows every normal (stable) installation asks. On Linux only the released Flatpak asks; a launcher you built yourself (a developer build, also on the branch `local`), a native (non-Flatpak) run and testing builds show no notice. The question goes to the published GitHub releases (a draft or a pre-release is never offered); the web site is not needed for it.

## Updating the launcher (Linux)

1. Download the newer `tsw-forever-<version>.flatpak` from the web site (or the release page) and check its SHA-256 against the value shown there or in `SHA256SUMS` (`sha256sum tsw-forever-<version>.flatpak`).
2. Close the game and the launcher.
3. Install the new bundle over the old one: `flatpak install --user ./tsw-forever-<version>.flatpak`. The application ID `org.tswlauncher.Launcher` and the user installation stay the same; your settings and the game folder are kept.
4. Start the launcher from your application menu.

Install only a NEWER bundle. Flatpak refuses to install the same bundle again ("already installed") and installs an older bundle without a warning, which goes back to that version. That is also the way to go back on purpose if a new version causes trouble: install the older bundle of the release you want from the releases page. Please report the problem.

There is no update source: a bundle installs no remote with an address, so `flatpak update` and your software center find nothing to fetch for the launcher (they still update the Flatpak runtimes). A remote left behind by a pre-release test build is never asked any more and can be removed with `flatpak remote-delete --user <name>`.

### Earlier installations (0.3.0 and older)

Nothing special is needed. The bundles of 0.4.1 use the same application ID (`org.tswlauncher.Launcher`), the same user installation and the same Flatpak branch (`local`) as the earlier single-file bundles, so `flatpak install --user ./tsw-forever-<version>.flatpak` updates an earlier installation in place: no second branch appears, and your settings and the game folder are kept (tested with the real Flatpak by `scripts/e2e_flatpak_bundle.sh`). The branch name `local` is historical; it does not mean "not released". Use `--system` instead of `--user` only if your earlier installation is system-wide; keep the same kind (`flatpak list --columns=application,branch,installation`). SteamOS: do this in Desktop Mode. Testing packages are not part of this: they are handed out by hand and are separate applications.

## Updating the launcher (Windows)

The notice described above opens the Windows download area of the web site. To update: download `tsw-forever-<version>-windows-setup.exe` and `SHA256SUMS` from there (or from the release page), compare the SHA-256 of the installer with the line in `SHA256SUMS` (`Get-FileHash .\tsw-forever-<version>-windows-setup.exe` in PowerShell), close the launcher, and run the installer. Windows asks for permission once (the installer is administrator-only, UAC stays on). The new version replaces the old program files; your settings and the game stay as they are.

The installer is not code-signed, because no signing certificate is used. Windows may therefore show **"Windows protected your PC"** (SmartScreen; choose *More info*, then *Run anyway* only after you checked the SHA-256) or name the publisher as unknown in the permission dialog. On a computer with *Smart App Control* switched on, Windows can refuse to start unsigned programs altogether; there is no way around that except switching that protection off, which is your own decision and which we do not ask you to make.

## Manual update

The launcher is updated by installing the newer file by hand, as described in the two sections above. All versions are listed on the [releases page](https://github.com/Glaubenskrieger/TSW-Forever/releases). `flatpak update` alone does not retrieve the launcher: no hosted update source exists.

## Uninstall

```sh
flatpak uninstall --user org.tswlauncher.Launcher
```

This removes the app, not `~/Games/The Secret World`. `--delete-data` additionally removes Flatpak app data/settings. It still does not remove the external game folder. Back up first; delete that folder yourself only if you intend to delete the game, prefix, runtime and caches. Never apply a deletion command to an adopted unrelated installation.

## Troubleshooting and limits

- Missing display modes: use Managed by the game, verify XWayland/the display session, then reopen Advanced.
- Installer interrupted: retry the offered step; partial downloads are retained. Do not erase a prefix to force adoption.
- Patcher finished but not PLAY: close it, then verify installation; completion indicators can be unavailable without a sound server.
- No game start: check displayed runtime/prefix findings and graphics/32-bit dependencies. A successful launcher GUI is not proof that Vulkan or the game works.
- Client patches are inactive without a compiled-in signing key. The original patcher remains responsible for original game data.
- No cross-distribution, Steam Deck, multi-monitor, HDR or general NVIDIA compatibility guarantee is made.

Licensing and source access: `../LICENSE`, `../THIRD-PARTY-NOTICES.md`, `../LICENSES/README.md`. This is a community test release, provided without the warranties described in its licences.

### CLI component consent

`tsw-launch install shaders --consent-download` downloads/verifies the Microsoft package and prints its original terms, but does not install unaccepted components. After reviewing them, repeat with the additional `--accept-microsoft-license` flag. The verified download is reused. This flag is specific to Microsoft component terms, not a general waiver of statutory rights. Acceptance/text records stay in the private component cache. A changed package or reviewed text fingerprint requires a new acknowledgement for new provisioning. An already installed profile is left intact.

