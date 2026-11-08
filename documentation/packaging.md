# Packaging, CI and releases

How TSW-Forever is built, packaged and released. Release rules (versions, tags, channels) are in `versioning-policy.md`; trust and secrets in `security.md`; third-party components in `third-party-components.md`.

## 1. Formats
| Format | Build | Notes |
|---|---|---|
| Flatpak `org.tswlauncher.Launcher` | `scripts/build_flatpak.sh` | official Linux binary format; not published on Flathub |
| Source archive | `scripts/native_stage.sh` | deterministic `tsw-forever-<version>-src.tar.gz`; Git source is also available |

There are no distribution-specific Linux packages. The native Windows build has its own x64 installer (`packaging/windows/`, Inno Setup; it installs into Program Files and needs administrator rights only for the installation; see `building-windows.md`); it is a separate operating system, not a Linux distribution package. No artifact contains game data, Funcom or Microsoft files, or a Wine/Proton runner. The Flatpak app ID must remain unchanged to preserve update identity.

## 2. Flatpak
* Runtime Freedesktop 25.08, SDK extension `rust-stable`, 32-bit support through `--allow=multiarch` (the game client is a 32-bit program). Programs: `/app/bin/tsw-launcher` (GUI) and `/app/bin/tsw-launch` (CLI, in the same folder so the GUI finds it).
* **Sandbox permissions** (`finish-args`, each one needed): `--socket=x11`, `--socket=wayland` (GUI, and Wine through XWayland), `--device=dri` (GPU), `--socket=pulseaudio` (audio), `--share=network` (game servers, downloads, update check), `--share=ipc` (X11 shared memory), `--allow=multiarch`, and exactly one folder: `--filesystem=~/Games/The Secret World:create`. No home, no `~/Games` as a whole, no Steam folder, no `--device=all`, no system or session bus, no `flatpak-spawn --host`. `scripts/check_flatpak_package.py` rejects anything else.
* **Data:** everything managed lives in `~/Games/The Secret World/{prefix,runtime,downloads,cache,logs}`; settings in `~/.var/app/org.tswlauncher.Launcher/config/tsw-launch/`. A Flatpak update only replaces `/app`. `flatpak uninstall --delete-data` removes the app data directory but not `~/Games/The Secret World`.
* **Wine/Proton:** not in the package. The launcher downloads a pinned GE-Proton archive (SHA-256 and size compiled in, https from GitHub) after explicit consent, verifies it before unpacking (only the expected root folder, no `..` or absolute paths) and installs it atomically into `<base>/runtime`. An existing runtime is never replaced.
* **Update check:** the launcher compares its installed commit (`/.flatpak-info`) with the repository ref and shows a hint (`flatpak update org.tswlauncher.Launcher`, then restart). The installation is done by the host Flatpak, which verifies the GPG signature; the launcher never modifies itself. The repository is not published yet, so the check stays silent.
* **Build:** `scripts/build_flatpak.sh` has the steps `stage`, `fetch`, `vendor`, `test`, `check`, `build`, `bundle` (offline after `fetch`/`vendor`). Fixed inputs: runner `ubuntu-24.04`, Freedesktop 25.08, `Cargo.lock` with `--locked`, vendored crates, `SOURCE_DATE_EPOCH`. Not fixed: apt package state on the runner and the Flathub state of the runtime branch.

## 3. Source archive
`scripts/native_stage.sh` selects the three Rust crates, lockfiles, UI, translations, icons, licences, install script and build instructions. It normalises entry order, owner, timestamps, modes and locale and uses `gzip -n`. No vendored libraries are included. See [`building-from-source.md`](building-from-source.md) for dependencies, building, offline vendoring and installation with `scripts/install_native.sh`.

A source build uses the host's 32-bit Wine dependencies and Vulkan drivers; `tsw-launch doctor` reports missing libraries. External tools include `curl`, `tar`, `gzip`, `bsdtar`, `cp`, `sh` and optionally `gamescope`. The source-built programs run outside the Flatpak sandbox.

## 4. CI workflows (`.github/workflows/`)
| Workflow | Trigger | Does | Must not |
|---|---|---|---|
| `ci.yml` | every push and pull request | format, Clippy (`-D warnings`), Rust and policy tests, secret scan, prepublication and static Flatpak checks | publish or use secrets |
| `flatpak.yml` | push to `main`, manual | tests, checks, vendoring, Flatpak build and bundle artifact | publish, use secrets, build other refs |
| `rc-artifacts.yml` | manual only | Flatpak and source archive, reproducibility, offline source compilation check, installed-bundle version and content audits | create releases, push, tag, use secrets or write permissions |
| `release-flatpak.yml` | manual only, confirmation `PUBLISH` and channel | signed Flatpak repository on Pages, protected environment | run automatically or bypass owner approval |

Policy tests enforce main/ref guards, ancestor checks, version consistency, read-only RC permissions, no publication and pinned action SHAs. RC builds may be selected manually from `main` or an existing version tag; tags must equal `v` + `APP_VERSION` and point to a commit on `main`. Agents never create tags.

The workflows remain available for owner-directed validation. Agents do not push, tag, dispatch any workflow or publish. `.github/workflows/release.yml` is the only workflow that publishes: an owner-pushed numeric version tag triggers its build (through `release-set.yml`), validation and GitHub release. Ordinary commits do not trigger a release.

RC steps: checks and tests → measured cleanup of Rust test targets → vendoring and Flatpak build/bundle → measured cleanup of Flatpak build state and SDK → stage the source archive twice and compare SHA-256 → unpack and run `version.py check` and pinned Rust 1.99.0 `cargo check --locked --offline` for both launcher crates using vendored dependencies → measured cleanup of source-check targets → assemble exactly the Flatpak, source archive and `SHA256SUMS` → release-set validation → install bundle and check CLI version → audit source and installed Flatpak for prohibited content → upload private workflow artifacts (30 days). Nine disk measurements retain evidence of resource use. The job limit is 120 minutes.

Checks establish build and artifact properties, not game compatibility. The owner confirmed installation, game start, multiplayer and resolution persistence on one Steam Machine with private builds. This does not prove broad GPU or distribution compatibility for public `0.2.0`. The manifest uses Freedesktop 25.08, `--allow=multiarch`, and the `Compat.i386` and `GL32` extension points following the Steam Flatpak pattern. The only host folder shared is `~/Games/The Secret World`.

### Public release assets
The tag workflow validates its own version and source, builds the Flatpak, the source tarball and the Windows installer, checks `SHA256SUMS`, creates the release as a draft, uploads exactly these four files, verifies the uploaded assets and only then publishes. It accepts only owner-pushed tags of the form `vMAJOR.MINOR.PATCH`.

## 5. Publishing the Flatpak repository (`release-flatpak.yml`)
Hosting through GitHub Pages: the workflow builds, signs commit and summary with GPG, generates `tsw-forever.flatpakrepo` and the `.flatpakref` files (`scripts/gen_flatpak_refs.sh`, URLs from `packaging/project.conf`) and publishes `repo/`. Pages for private repositories needs a paid plan and the published site is public; release assets of private repositories are not anonymously retrievable, so client patches need a public release or another public source. Channels are Flatpak branches of the same app ID: `stable` (final versions only) and `testing` (pre-releases and early finals); `scripts/flatpak_channel.py` enforces: no pre-release in `stable`, only strictly newer versions are published, a new stable is also promoted to `testing`. The repository URL is the single setting `FLATPAK_REPO_URL`.

**One-time setup by the owner:** environment `flatpak-release` with required reviewers; secrets `FLATPAK_GPG_PRIVATE_KEY` and `FLATPAK_GPG_KEY_ID`; Pages source "GitHub Actions"; decide repository visibility. None of this has been done.

### Signing keys
Private keys are generated on the owner's machine, outside the repository (for example `~/.local/share/tsw-forever-keys/`, mode `700`, backed up offline). `.gitignore` also blocks key file patterns. Only public keys go into the code.
* **minisign** (client-patch manifests): `minisign -G -p patch.pub -s patch.key`; put the Base64 line of `patch.pub` into `PATCH_PUBLIC_KEY` in `launcher/src/project.rs` (this activates the patch system). Sign patch releases locally with the maintainer script `make_patch_release.sh` (not part of the public source; `… --key patch.key`) (never in CI, the old files are original game files and must not leave the maintainer's machine). The launcher accepts only prehashed `ED` signatures. The key is compiled in, so rotation needs a new launcher release.
* **GPG** (Flatpak repository): create a dedicated, CI-only key without passphrase and with an expiry (for example `gpg --quick-generate-key "TSW-Forever Flatpak" rsa4096 sign 2y` in a separate `GNUPGHOME`), export the public part for the `.flatpakrepo`, and store the armored private key only as the environment secret. Passphrase handling in the workflow is not implemented. These commands have not been run yet.

## 6. Licence of the build
The project is GPL-3.0-or-later. The GUI uses Slint under its GPL-3.0 option; anyone distributing the binaries must offer the complete source including build scripts, which is why a public repository (or a written source offer) is required for a public release. Third-party Rust libraries and their licences are listed in `THIRD-PARTY-NOTICES.md`.

## Hardening and public preparation

The owner has published public `0.2.0` after private Steam Machine testing. The Microsoft EU/German assessment and integrated original-term consent are documented in `microsoft-components.md`; remaining interpretation questions are still disclosed and require renewed review if circumstances change. The existing private releases remain unchanged.

Flatpak binaries install `LICENSE`, `THIRD-PARTY-NOTICES.md`, the complete `LICENSES` inventory/materials and the GUI lockfile under `/app/share/licenses/tsw-forever/`. The source archive carries the same materials. `scripts/check_licenses.py` verifies source checksums, selected Linux dependencies and every required text; the RC audit checks the installed payload as well. Own code remains GPL-3.0-or-later; the combined GUI and AppStream distribution licence are GPL-3.0-only because that is the selected Slint licence.

There is no automatic GitHub bundle updater. See the public `README.md` for manual installation of a newer bundle, retained data and uninstallation. Later public source snapshots must pass the `check_public_source.py` audit, contain the corresponding editable sources/build information, and omit private history and branding (`public-export.md`).
