# Distribution licences and corresponding source

The launcher author's code is GPL-3.0-or-later (`../LICENSE`). The GUI uses Slint under GPL-3.0-only; the combined GUI binary is distributed under GPL version 3. Component licences and notices are preserved separately and are not relicensed by this directory.

`inventory.json` records the Linux x86-64 normal and build dependency closure of `launcher-gui`, including `launcher` and `patcher`. It conservatively includes build-time components, not only code linked into the executable. `../THIRD-PARTY-NOTICES.md` additionally lists the full cross-platform lockfile. Optional non-Linux dependencies are not claimed as part of the distributed Linux binary.

Every collected notice has its original source and SHA256. Most texts are copied from the exact vendored registry package. Where upstream omitted a licence file from its crate, the original text was retrieved from the repository revision recorded in that crate's `.cargo_vcs_info.json`. No substitute copyright holder or licence grant was invented.

Fonts: Slint's Inter attribution is retained with its upstream SIL OFL text. The fallback Cantarell font in `sctk-adwaita` carries the copyright recorded in its embedded name table; its SIL OFL text is included in `fonts/`. System fonts selected at runtime are supplied by the runtime/host, not redistributed here. The launcher uses its own generated/vector application icons and no Funcom artwork.

## Corresponding source

Download the source archive next to the binary from the same release. It contains our preferred editable sources, the three Cargo.lock files and the build instructions in `README.md`. Each registry dependency's exact full source archive is available at the `source_url` recorded in `inventory.json`; its SHA256 is the lockfile checksum. The complete cross-platform dependency versions and checksums remain in Cargo.lock. These external downloads need network access; `cargo vendor --locked` prepares an offline build as described in the README.

Distributors remain responsible for equivalent access to the complete corresponding sources while distributing the binary (GPLv3 section 6(d)). Keep the exact dependency archives or a source mirror if the designated registry becomes unavailable. Do not claim that the small application source archive itself contains vendored dependencies. Build scripts and project configuration are included; Flatpak's separately distributed system runtime/SDK have their own source and licence channels.

Validate with `python3 scripts/check_licenses.py`. Changing a dependency, feature selection, embedded asset or font requires an inventory review. Required texts are also installed in `/app/share/licenses/tsw-forever/` in the Flatpak.
