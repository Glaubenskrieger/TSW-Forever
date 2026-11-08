# User guide — Windows, Linux and SteamOS

TSW-Forever is an unofficial launcher for the original **The Secret World (2012)**. It is not Secret World Legends, is not affiliated with Funcom or the rights holders, and provides neither a game server nor account management. You need a valid account entitled to the original game. The launcher cannot create that entitlement; a Legends account is not proof of access to the original. This project does not promise that new original-game accounts can currently be purchased or registered.

## Windows

Since version `0.3.0` TSW-Forever runs natively on Windows (x64, Windows 10 or later; tested on Windows 11). The step-by-step installation is in the public `README.md` (sections 2 to 4); this is the overview.

- **Install:** download `tsw-forever-0.3.0-windows-setup.exe` and `SHA256SUMS` from the [public release page](https://github.com/Glaubenskrieger/TSW-Forever/releases), compare the SHA-256 value (PowerShell: `Get-FileHash`), run the installer and confirm the administrator prompt (needed only for installing into `C:\Program Files\TSW-Forever`). The installer is not code-signed; Windows SmartScreen may warn. Start the launcher from the Start menu or the optional desktop icon; it runs with normal user rights.
- **First setup:** the launcher offers three choices: download the official Funcom installer (after your consent; checked against a pinned checksum), provide a Funcom installer file you already have (an unknown fingerprint needs your confirmation), or use an existing installation by selecting its folder. Windows asks for permission when the original Funcom installer or patcher starts. Keep the installer's suggested default settings. **PLAY** appears after the original patcher has confirmed that the game data is complete. The game data is roughly 20 GB.
- **Existing installations:** in this version the launcher does not search for them automatically; select the folder yourself (first setup, or Advanced → INSTALLATION). Advanced → INSTALLATION can also install a new copy in another folder; the existing installation is then ignored and never changed.
- **DirectX:** the game needs the 32-bit Direct3D libraries of Microsoft's DirectX End-User Runtimes (June 2010). If they are missing, the launcher names them; **Install DirectX components …** downloads Microsoft's own web installer on your click, checks it and starts it, and **Check again** refreshes the message.
- **Settings and data:** `%APPDATA%\tsw-forever` (settings) and `%LOCALAPPDATA%\tsw-forever` (larger data such as the downloaded installer). The game's own preferences stay in the game's folders. The program folder holds no changeable data.
- **Differences from Linux:** no Wine, Proton, prefix, shader cache, Gamescope or Microsoft component consent, and **no launcher FPS limit or FPS counter** (those need DXVK on Linux; use your graphics driver's limiter on Windows). The game resolution can be chosen in Advanced → DISPLAY and applies at the next start.
- **Update and uninstall:** run a newer installer over the old one (settings and game are kept). Uninstall through **Settings → Apps → Installed apps**; this removes only the program files, not your settings or the game.
- **Limits:** automatic detection of existing installations is temporarily off; the original client ends every regular exit with its own assertion message (no data loss observed); the Windows build has been practically tested on one Windows 11 computer.

## Requirements and download (Linux)

The official binary formats are the Windows x64 installer (see above) and the Flatpak for Linux x86-64; source is also available. A working Vulkan-capable graphics driver, Flatpak, network access and XWayland for the tested Wine path are required. The Flatpak supplies its runtime environment; the launcher can obtain its pinned GE-Proton runner. A native source build requires its own Wine/Proton and 32-bit dependencies: see `building-from-source.md`.

The owner tested installation, GUI, game start, multiplayer and resolution persistence on one Steam Machine using private builds. The public version is `0.3.0`. Other distributions and GPUs are potential Flatpak targets, not fully validated platforms. The sections below describe the Linux Flatpak.

Use the [public release page](https://github.com/Glaubenskrieger/TSW-Forever/releases), not an arbitrary re-upload. The public README contains the complete installation, update and uninstall procedure. Download the bundle and matching `SHA256SUMS` together; the source archive is optional for installation. Example for version `0.3.0`:

```sh
sha256sum --check --ignore-missing SHA256SUMS
flatpak install --user ./tsw-forever-0.3.0.flatpak
flatpak run --user org.tswlauncher.Launcher
```

Checksums detect changed downloads; a checksum file from the same compromised channel is not an independent signature. Our own signed update repository is not operating yet. No game, Funcom installer, Microsoft DLL or Wine/Proton runner is inside our release artifacts.

## First installation (Linux)

Press **INSTALL THE GAME**, read the download information and consent only if you agree. Additional downloads total about 700 MB for the installer, runner and Direct3D components, followed by roughly 20 GB of original game data. Allow additional space for the prefix, unpacking and caches. Microsoft components have their own terms; the EU/German assessment and residual public-review question are described in `microsoft-components.md`. The complete original terms are accessible through online-copy links in the existing setup view. Read them, select the independent Microsoft checkbox and download checkbox, then press INSTALL THE GAME. No pre-download is needed just to enable agreement. Opening a link does not accept anything. Acceptance remains locally bound to the exact package/text fingerprints, which are still verified before DLL provisioning. Existing installed profiles are not rewritten or prompted at every game start.

Keep the original installer's default destination and settings. The original Funcom patcher downloads and verifies the game. It can resume after interruption. The launcher observes progress and changes to **PLAY** after completion is recognised. You may need to close the patcher manually; that is supported and not a failure. If PLAY does not appear, close the patcher and use **VERIFY INSTALLATION**. Missing files alone do not prove completion.

You may choose a local installer; an unknown fingerprint requires separate confirmation. Do not confirm an installer whose origin you do not trust. **Use existing installation** adopts original data rather than granting new game rights. In Flatpak, paths must be accessible through the game-folder permission or an appropriate file/directory portal grant. Selecting one executable does not guarantee access to every sibling data file. Do not point prefix initialisation at a Lutris/Steam/other application's prefix. Unproven or damaged ownership is refused; choose a new empty managed folder rather than deleting ownership records.

## Storage and settings (Linux)

- Managed installation: `~/Games/The Secret World/`, containing `prefix`, `runtime`, `downloads`, `cache` and `logs`.
- The Wine prefix contains the Windows-side game and its user preferences; it is not merely a disposable launcher setting.
- Flatpak settings: `~/.var/app/org.tswlauncher.Launcher/config/tsw-launch/`.
- Native settings: `~/.config/tsw-launch/`.

A normal bundle update replaces the application, not the external game folder. Back up settings and the prefix before maintenance. Never share the entire prefix: the original client can write account/session information there.

## Play and display (Linux)

Press PLAY. The launcher minimises while the game runs and handles its own exit when the game ends. The gear opens Advanced; the arrow at the same position returns. Escape also leaves Advanced.

**Display:** Managed by the game is the default. A selected mode is obtained from the active game display environment, saved and reapplied at each start using a private Wine desktop and user preferences. The Linux desktop resolution is not changed. After changing monitors or display environment, an unavailable saved mode is refused explicitly: choose an available mode or Managed by the game. Detection failure leaves the managed default available.

The FPS slider remains freely adjustable from 30 to 240, default 60; Unlimited removes the launcher's cap. The DXVK checkbox controls DXVK's FPS counter. Other game/compositor limits may still apply. No mandatory Gamescope dependency exists; legacy Gamescope and manual override panels remain hidden.

## Compiler profile and shader cache (Linux)

The normal installation uses `d3d-native-ms2010`: the three checked 32-bit compiler/D3DX components obtained from Microsoft, not supplied by this project. A working installation needs no manual profile change. The advanced manual profile path is for expert recovery; the older `d3d-native-v7` includes compiler 47 and is experimental, not the supported default. Do not source DLLs from random download sites. Profile changes affect only a managed prefix and retain backups, but interrupted multi-file maintenance is not guaranteed to roll back automatically.

Shader cache rebuild renames the local cache aside; the game rebuilds it. Undo restores the saved cache. It does not repair arbitrary game data. Avoid cache/profile maintenance while the game is running.

## Diagnostics and reporting

Diagnostic logging is voluntary and off by default. Nothing is automatically uploaded; no telemetry service is added. Basic/detailed modes and a user-triggered diagnostic package are available. Saved environment entries are structurally omitted from the package, and known literal values are removed from included records. Other personal information, paths or unknown secrets may remain because masking is heuristic. Read the entire package before sharing it. Never upload raw ClientLog files, a Wine prefix or a raw `--log-file`.

Report bugs through the [public issue tracker](https://github.com/Glaubenskrieger/TSW-Forever/issues). Include launcher version, distribution, desktop/session, GPU/driver, installation method, exact steps and the visible error. Attach only a personally reviewed reduced diagnostic package. A security report should first use the privately designated contact, not a public issue containing secrets.

## Manual update (Linux)

There is no automatic GitHub-bundle updater. Download the newer matching bundle/source/checksum set, verify it, close the game and launcher, then install the new bundle with the same `flatpak install --user ./tsw-forever-<version>.flatpak` command. Confirm the update prompt and restart. The app ID remains `org.tswlauncher.Launcher`. Settings and the external game folder should remain; migration from unrelated installation layouts still needs separate validation. `flatpak update` alone does not retrieve our next GitHub asset while no project repository is operating.

## Uninstall (Linux)

```sh
flatpak uninstall --user org.tswlauncher.Launcher
```

This removes the app, not `~/Games/The Secret World`. `--delete-data` additionally removes Flatpak app data/settings. It still does not remove the external game folder. Back up first; delete that folder yourself only if you intend to delete the game, prefix, runtime and caches. Never apply a deletion command to an adopted unrelated installation.

## Troubleshooting and limits (Linux)

- Missing display modes: use Managed by the game, verify XWayland/the display session, then reopen Advanced.
- Installer interrupted: retry the offered step; partial downloads are retained. Do not erase a prefix to force adoption.
- Patcher finished but not PLAY: close it, then verify installation; completion indicators can be unavailable without a sound server.
- No game start: check displayed runtime/prefix findings and graphics/32-bit dependencies. A successful launcher GUI is not proof that Vulkan or the game works.
- Client patches are inactive without a compiled-in signing key. The original patcher remains responsible for original game data.
- No cross-distribution, Steam Deck, multi-monitor, HDR or general NVIDIA compatibility guarantee is made.

Licensing and source access: `../LICENSE`, `../THIRD-PARTY-NOTICES.md`, `../LICENSES/README.md`. This is a community test release, provided without the warranties described in its licences.

### CLI component consent

`tsw-launch install shaders --consent-download` downloads/verifies the Microsoft package and prints its original terms, but does not install unaccepted components. After reviewing them, repeat with the additional `--accept-microsoft-license` flag. The verified download is reused. This flag is specific to Microsoft component terms, not a general waiver of statutory rights. Acceptance/text records stay in the private component cache. A changed package or reviewed text fingerprint requires a new acknowledgement for new provisioning. An already installed profile is left intact.
