# TSW-Forever versioning policy

## Published baseline and historical development versions

The first public version is `0.2.0`; the last public version is `0.3.0`. Their public source and release assets are in `Glaubenskrieger/TSW-Forever`. The private development tree carries `APP_VERSION=0.4.0` in `packaging/project.conf`, the next public release. An internal intermediate version `0.3.1` existed only in the private development repository and was never published (no public release, no public tag `v0.3.1`); its changes are part of `0.4.0` and are listed in its changelog entry. Historical `0.2.0-rc.N` tags are private candidates, not numbers to reuse. Existing private candidate tags, assets and history remain unchanged. `0.4.0` brings new launcher languages, the update overlay and the release checks after the public `0.3.0`; the next actual functional change selects the next number.

Earlier private release rules and the evidence behind them remain in the historical reports and candidate tags. They do not govern future version names or authorize any agent to publish.

## Future application versions

Use exactly `MAJOR.MINOR.PATCH` with three decimal integers and no suffix or build metadata. The public tag format is `vMAJOR.MINOR.PATCH`. Do not use alpha, beta, RC, preview or dev identifiers for future application versions, tags, release names or artifact names. Never republish different content under `0.2.0` or change a historical tag.

- **PATCH:** fixes, security corrections and small compatible improvements. The implementing agent decides whether this scope applies; for example, `0.2.0` to `0.2.1`.
- **MINOR:** new features or substantial changes. The implementing agent decides whether this scope applies and resets PATCH to zero; for example, `0.2.1` to `0.3.0`.
- **MAJOR:** only the project owner may decide or approve it. Agents may propose a major change with reasons but must not select `1.0.0` themselves.

Do not change a version number for documentation-only synchronization. Record actual changes under `Unreleased` in `CHANGELOG.md` until the owner chooses to publish a new version. Every version decision must be explained in the completion report. Historical private version strings may remain in historical records.

## Version sources and independent axes

`APP_VERSION` in `packaging/project.conf` is the application source of truth. At the next functional development version transition, use `scripts/version.py set <numeric-version>` to update it, the launcher and GUI Cargo manifests/lockfiles, and Flatpak AppStream metadata together. Then run `scripts/version.py check`, version/policy tests and the affected Rust and Flatpak checks. The current private version script still supports the historical RC value; it must be migrated and tested for the numeric-only rule when the private source version is advanced. The public 0.2.0 export already implements numeric-only validation. Do not copy public scripts wholesale into this private tree because they omit private CI and research checks.

The patch engine's crate version, the integer patch-manifest format and an installed client's integer patch level are separate from `APP_VERSION`. The patch-engine crate is `0.1.0` (numeric; `scripts/version.py check` verifies it) and is not part of the launcher update system. The client-patch channel remains inactive until a trusted public signing key is configured; original game updates continue through Funcom's patcher. Changes to a manifest grammar require an explicit format update, compatibility review and tests. Do not infer a client patch level from an application release.

## Publication and ownership

Public publication belongs to the project owner. In the private development repository agents may commit, push and run workflows as `AGENTS.md` states. The public repository's `.github/workflows/release.yml` reacts to an owner-pushed numeric version tag, verifies the project version and builds/checks the Flatpak, source archive and `SHA256SUMS` before attaching them to a GitHub release. A normal commit does not publish a release. The private `ci.yml`, `flatpak.yml`, `rc-artifacts.yml` and `release-flatpak.yml` are development workflows and must not be substituted for the public tag workflow or started automatically by an agent.

The Flatpak app ID is `org.tswlauncher.Launcher`; preserve it for update identity and existing user data. The project's own Flatpak repository (the only Linux update path) is prepared but not yet operating; the Windows launcher has no automatic update (manual installer from the GitHub release). Updating from a local bundle keeps app data and the separate managed game folder; the README gives the user procedure. The project does not distribute original game files, Microsoft DLLs or a Wine/Proton runner in its release assets.

## Readiness and evidence

Distinguish code implementation, automated test success, practical hardware testing and broad compatibility. The owner confirmed successful installation and gameplay on one Steam Machine with private test builds, including multiplayer and resolution persistence. This does not establish every GPU, distribution or desktop session. The Microsoft SDK/runtime consent is separately gated and locally recorded for the reviewed package/terms. Residual legal questions are documented in `microsoft-components.md`; private use and interoperability do not automatically waive licence obligations.

Before any owner publication, review version consistency, changelog, rustfmt, Clippy, Rust and policy tests, licence inventory, source/content/secret audits, Flatpak build and install evidence, and exact artifact checksums. The public release workflow enforces its own applicable checks. A successful build is not a substitute for a game runtime test. Agents prepare local changes and report evidence; the owner approves every public release action.
