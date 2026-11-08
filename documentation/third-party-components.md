# Third-party components, provenance and open legal questions

This is a technical record, **not legal advice**. TSW-Forever contains no game data and no Funcom or Microsoft file; everything proprietary is obtained on the user's machine, after explicit consent, from the original source.

## 1. What ships and what does not
`scripts/check_release_artifacts.py` audits the source archive and installed Flatpak tree (names, Windows `MZ` headers, size) in `rc-artifacts`. Each new artifact set must pass its own audit; earlier package audits do not validate a new build. The branding image is excluded from artifacts; program icons are own artwork (`launcher-gui/assets/icon.svg`).

| Handling | TSW-Forever |
|---|---|
| **Download** | the launcher on the user's machine downloads from Funcom, Microsoft and GitHub, only after consent, with pinned checksums |
| **Install** | extraction of three DLLs into the user's prefix without running Microsoft's installer (no licence dialog appears; Winetricks does the same with `cabextract`) |
| **Redistribute** | does not happen: no such file in Git, Flatpak or source archive |

## 2. Components
| Component | Source | Verification | When |
|---|---|---|---|
| Funcom installer | `https://cdn.funcom.com/downloads/tsw/client/TheSecretWorldInstaller.exe` | pinned SHA-256 (own measurement) | on request with consent, or the user's own file |
| GE-Proton (Wine, DXVK, vkd3d-proton) | GitHub release of GloriousEggroll | pinned SHA-256 and size | on request with consent |
| Direct3D components (profile `d3d-native-ms2010`, default) | `download.microsoft.com/…/directx_Jun2010_redist.exe` | pinned SHA-256, then size and MD5 per DLL | on request with consent (`tsw-launch install shaders`, GUI) |

### Funcom installer (`launcher/src/install.rs`)
Size 33 164 664 bytes, SHA-256 `9b9a71fbf198d5eab7fbea80a1cc93994d79d1526a1e199f62532fc2705d908e`, unchanged on the server since 2017-06-22. Funcom publishes no checksum, so the pin is **trust on first use**: it protects against a changed or damaged file, not against a file that was already wrong when measured. Only https is accepted (also after redirects; plain HTTP is rejected). The Authenticode signature is valid and matches the content (signer "FUNCOM OSLO", timestamp 2017-06-22), but the certificate chain could not be closed up to a trusted root locally; this is an indication, not independent proof. Download only after consent for exactly this address; the file gets its name after the check passes and is deleted on mismatch. An **own installer** is copied and its SHA-256 shown; an unknown file runs only after the user confirms exactly that fingerprint. The installer starts only as part of the user-initiated installation flow after consent and verification; it is not an unsolicited background action. The user is told to keep the default settings and path.

### GE-Proton (`launcher/src/runtimedl.rs`)
One compiled-in entry (`GE-Proton11-11-x86_64.tar.gz`; 564 789 552 bytes; SHA-256 `aa85986c…3c1`, matching the release's checksum file). Verified before unpacking, only the expected root folder, atomic rename, never replaces an existing folder. Not redistributed.

### Direct3D components (`launcher/src/dxredist.rs`, `launcher/src/prefix.rs`)
The working Lutris-based setup uses `d3dcompiler_43`, `d3dx9_43`, `d3dx11_43` (native overrides). The launcher fetches the same Microsoft package as Winetricks (`directx_Jun2010_redist.exe`, 100 275 120 bytes, SHA-256 `053f76dcbb28802e23341b6a787e3b0791c0fa5c8d4d011b1044172dbf89c73b`; **identical to the pin in Winetricks 20260125-next**), unpacks with `bsdtar` and re-checks each DLL (`D3DCompiler_43.dll` 2 106 216 B md5 `1c9b45e8…`, `d3dx9_43.dll` 1 998 168 B md5 `86e39e91…`, `d3dx11_43.dll` 248 672 B md5 `8e0bb968…`; byte-identical to the working reference). The package is deleted afterwards, the three DLLs (4 MB) stay in the cache `<base>/cache/native/d3d-native-ms2010/`. Winetricks itself is not used: it needs `cabextract` (not in the Flatpak runtime), downloads arbitrary verbs and runs scripts, which conflicts with the rule never to run downloaded scripts uncontrolled.

**`d3dcompiler_47.dll`** is not downloaded, not shipped and not part of the default profile. It appears only in the older experimental profile `d3d-native-v7`, installed through the developer command `tsw-launch prefix native-install --native-dir <folder>`, which checks (size and MD5) and copies DLLs from a folder **the user supplies**. Winetricks fetches `d3dcompiler_47` from Windows SDK installer `.cab` files at `download.microsoft.com` (32-bit `2630bae9…cab`, 64-bit `61d57a7a…cab`) without a visible checksum, and that version (3 466 856 B) differs from the one in the reference setup, so no lawful source of the reference version is known. *Known deviation:* the help text of `prefix native-install` still names `d3d-native-v7` as the default profile, unlike the installation path (`ms2010`). Smallest clean correction (not done for the frozen RC): make `d3d-native-ms2010` the default of that command and label `v7` experimental. Whether `ms2010` renders identically to `v7` is confirmed only by the practical test; until then `v7` stays the reference.

## 3. Licence of the project
Own code is licensed **GPL-3.0-or-later** (`LICENSE` is the unmodified GPL-3.0 text). Official releases stay free and open source; adaptations (for example for *Secret World Legends*) are welcome under the GPL terms, and distributors may charge for derived versions. There is no non-commercial clause (it would be incompatible with the GPL). The GPL grants no rights to Funcom's game, trademarks, graphics or proprietary components.
* **Slint 1.18.1** is licensed `GPL-3.0-only OR` royalty-free `OR` commercial; the GUI uses the GPL-3.0 option, so the combined binary is effectively GPLv3.
* The full GUI lockfile has 542 registry packages across platforms; the Linux normal/build closure has 325. Apart from the separately described Slint licence, the selected dependency licences are permissive (MIT, Apache-2.0, BSD, ISC, Zlib, Unicode-3.0, BSL-1.0, Unlicense/CC0, 0BSD, and two crates `… OR LGPL-2.1-or-later` used under MIT). No GPL-incompatible licence was found. Notices for binary distribution: `THIRD-PARTY-NOTICES.md`.
* Separately downloaded, not redistributed by our release: GE-Proton/Wine (LGPL and others), DXVK (zlib), vkd3d-proton (LGPL), Microsoft DLLs and the Funcom installer. Their terms still govern use; this does not remove obligations for libraries actually linked into the launcher. The private-candidate decision and residual public-review questions are described below.
* Optional: SPDX headers in source files are not set.

## 4. Funcom
Sources read on 2026-10-31: `funcom.com/legal/` (FAQ), *TSW Fansite Kit License*, *SWL EULA and TOS* (last modified 2017-06-14). Facts: the SWL EULA §3 forbids without permission distributing or selling the software, using it to develop another product, reverse engineering ("except as otherwise expressly permitted by law, including for interoperability reasons"), modifying it or creating derivative works, scraping or vulnerability testing, tools that intercept or redirect communication or read game memory, and removing notices. The Fansite Kit license covers only kit materials (not used here) and is revocable. The FAQ welcomes non-commercial fan use, allows trademarks only for description, and states that use of Funcom's logos is not permitted at all.

Assessment: the SWL EULA concerns *Secret World Legends*; whether it applies to the 2012 game (which shows its own EULA at installation) is **not established**. The launcher distributes no client, does not modify it, calls the original installer and patcher, and uses the name descriptively with a trademark notice. Higher risk: the **client patch system** (`tsw-patch` replaces game files; currently switched off, clarify before activating) and reverse-engineering results (offsets, formats), which are not part of this repository.

## 5. Branding image
`assets/branding/Dramatisches Fraktions-Emblem mit Ankh.png` (1254 × 1254 RGB PNG, 2 369 510 bytes, SHA-256 `c85adf050c5d78fa88af085f73c69583dcfadc208e0b70b4ca967bc1d46380c9`) shows a round emblem with an ankh, a red cross, an eye in a triangle and an ouroboros. **Author, tool and terms of use are not documented** (likely AI-generated; unconfirmed), and it evokes the faction symbols of the game. It is a raster image with no vector original and is **not** automatically GPL-licensed. It is used neither in the app nor in any package. `scripts/prepublish_check.py` allows exactly this file (path, SHA-256, 2.5 MB limit) only while it is byte-identical to the reviewed one and this document exists. It was excluded from public `0.2.0`; keep it private unless the owner establishes provenance and terms or replaces it with cleared original artwork.

## 6. Open questions
1. Microsoft DirectX June 2010 extraction and Wine loading: the exact package and terms were reviewed in `microsoft-components.md`; the remaining interpretation question is disclosed there, not resolved by the public release.
2. `d3dcompiler_47.dll`: whether and under which terms Microsoft allows redistribution (Windows SDK licence, `REDIST.TXT`) could not be confirmed at the original source. Without consequence while we neither ship nor download it.
3. Funcom: rights concerning the installer download, a non-commercial community launcher, and the patch system; a request to Funcom is pending.
4. Branding image rights (section 5).
5. Legal advice before any long-term native client project.

## 7. Hardening review update — 2026-11-03

The owner's rc.3 Steam Machine test has now confirmed the normal ms2010 installation path, game/multiplayer operation and persisted resolution. This supersedes earlier pending-practice statements, not their historical provenance. The GUI compiler section labels the older manual v7 route experimental.

`LICENSES/inventory.json` now records 325 Linux normal/build registry dependencies (the full cross-platform lockfile still has 542). Original notices/texts and font attribution are retained and installed in the Flatpak. See `LICENSES/README.md` for GPLv3 combined-binary terms, own-code GPL-3.0-or-later and complete corresponding-source access. The absence of redistributed Wine/Microsoft binaries does not remove duties for libraries actually linked into our own programs.

The exact Microsoft download was statically reviewed, including its SDK and separate runtime agreements: `microsoft-components.md`. The EU/German assessment supports Variant B, with separate original-term consent in the existing setup view; the owner subsequently published public `0.2.0`. This does not establish definitive legal clearance. The private branding PNG remains excluded from the public source. No original game file or real prefix was accessed for this review.
