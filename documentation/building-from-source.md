# Building TSW-Forever from source

The official binary formats are the **Flatpak** on Linux and the **Windows installer** on Windows (see the README). This document describes the Linux build; building on Windows is described in `building-windows.md`. If you prefer to build the launcher yourself, use the **source archive** `tsw-forever-<version>-src.tar.gz` of a release (or a checkout of the Git repository). There are no distribution-specific packages (`.deb`, `.rpm`, Arch packages) and no AppImage. The build produces two programs: `tsw-launch` (command line) and `tsw-launcher` (graphical front end).

## 1. Check the archive
```sh
sha256sum -c SHA256SUMS          # next to the downloaded files
tar xzf tsw-forever-<version>-src.tar.gz
cd tsw-forever-<version>
```
The archive has one top folder, contains no vendored libraries, no game data, no Funcom or Microsoft file and no local files. Everything needed to build is in it: the three Rust crates with their `Cargo.lock` files, the window definition, the translations, the icons, the licence texts and this document.

## 2. What you need
* **Rust** (stable, with `cargo`). The launcher is developed and checked by CI with **Rust 1.99.0**; no older version is tested and no minimum version is declared.
* **Native build tools:** a C/C++ compiler toolchain, linker and `pkg-config`, plus `tar` and `gzip` to unpack the source archive.
* **C libraries for the window toolkit** (headers for building, libraries at run time): fontconfig, FreeType, xkbcommon (also the X11 part), Wayland, X11, XCB, Xcursor and Xi. On Ubuntu 24.04 the CI installs `libfontconfig1-dev libxkbcommon-dev libxkbcommon-x11-dev libwayland-dev libx11-dev libxcb1-dev libxcursor-dev libxi-dev`. Other distributions name them differently; look up the equivalents of the libraries above.
* **At run time** the launcher calls `curl`, `tar`, `gzip` and `bsdtar` (from libarchive) and, to play, needs the 32-bit parts of Wine's dependencies and a Vulkan driver (also 32-bit) on your system. A Wine/Proton runner is **not** included: the launcher downloads GE-Proton itself (checksum verified) or uses an existing one. `tsw-launch doctor` reports what is missing. Optional: `gamescope`, an XDG desktop portal (file dialogs), PipeWire/PulseAudio (sound).
* Network access for `cargo` to fetch the crates of the lock files (or see section 5 for an offline build).

## 3. Build
```sh
cargo build --release --locked --manifest-path launcher/Cargo.toml
cargo build --release --locked --manifest-path launcher-gui/Cargo.toml
```
The programs are `launcher/target/release/tsw-launch` and `launcher-gui/target/release/tsw-launcher`. They are built with separate lock files; `--locked` keeps the dependency versions the release was tested with. The graphical front end finds `tsw-launch` in its own folder, so keep both programs together.

Tests (optional): `cargo test --locked` in `launcher/`, `launcher-gui/` and `patcher/`. The window tests need at least one installed font.

## 4. Install
```sh
mkdir -p dist
cp launcher/target/release/tsw-launch launcher-gui/target/release/tsw-launcher dist/
sh scripts/install_native.sh . dist "$DESTDIR" --prefix /usr/local     # DESTDIR empty (or /) installs for real; set it to stage into a folder
```
The script copies both programs into one folder, the desktop entry, the AppStream data, the icons and the licence texts below `DESTDIR`. It builds nothing and touches nothing outside `DESTDIR`. Without installing, you can run `dist/tsw-launcher` directly.

## 5. Offline build
On a machine with network access, vendor the crates once and copy the folder:
```sh
(cd launcher-gui && cargo vendor --locked ../vendor)
mkdir -p .cargo
printf '[source.crates-io]\nreplace-with = "vendored-sources"\n\n[source.vendored-sources]\ndirectory = "%s"\n' "$PWD/vendor" > .cargo/config.toml
cargo build --release --locked --offline --manifest-path launcher/Cargo.toml
cargo build --release --locked --offline --manifest-path launcher-gui/Cargo.toml
```
CI unpacks the archive, points Cargo at the vendored crates and runs `cargo check --locked --offline` for both launcher crates. This checks compilation; it does not produce release binaries.

## 6. Where things go
Everything the launcher manages lives in `~/Games/The Secret World/` (Wine prefix with the game, GE-Proton, downloads, cache, logs); settings are in `~/.config/tsw-launch/`. A native build can access your whole home directory, unlike the sandboxed Flatpak. Third-party libraries and their licences: `THIRD-PARTY-NOTICES.md`. The project is licensed GPL-3.0-or-later (`LICENSE`).

## 7. Distribution licence materials

`LICENSES/` contains the pinned Linux dependency texts/font notices and a corresponding-source index. Validate with `python3 scripts/check_licenses.py` (Python 3.11+); source builds need registry sources of Cargo.lock or the documented vendor directory. The application archive does not itself vendor those external sources. Flatpak packaging metadata and `scripts/build_flatpak.sh` are retained for the binary's build/install information. Its build path uses the staged sources and existing SDK; it is not a promise that every optional CI workflow/test helper is included in a source release. Own files are GPL-3.0-or-later; the combined Slint GUI is distributed under GPLv3. See `LICENSES/README.md`.
