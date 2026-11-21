#!/usr/bin/env bash
# Build the Flatpak of the launcher. Platform-neutral: the CI definition is only a thin wrapper around this script.
#
#   scripts/build_flatpak.sh fetch     cargo fetch --locked for the tests on a machine with an empty cargo cache   (NEEDS NETWORK)
#   scripts/build_flatpak.sh test      unit tests of the launcher (offline) + policy tests            (needs cargo, python3)
#   scripts/build_flatpak.sh stage     copy ONLY the launcher sources into build/flatpak/src            (needs rsync; TSW_RELEASE_BUILD=1 marks the tree as a RELEASE build)
#   scripts/build_flatpak.sh check     stage + validate manifest, desktop file, metainfo, staged tree (no network, no flatpak)
#   scripts/build_flatpak.sh vendor    cargo vendor from the committed Cargo.lock into build/flatpak/src/vendor   (NEEDS NETWORK)
#   scripts/build_flatpak.sh build     flatpak-builder (offline; needs the runtime, SDK, rust extension and vendor/ installed)
#   scripts/build_flatpak.sh bundle    export build/flatpak/tsw-launcher.flatpak (one installable file; no update source, no key) from the build repository
#   scripts/build_flatpak.sh bundle-args  check the FLATPAK_BRANCH of the bundle (stable, testing or local); nothing is built
#   scripts/build_flatpak.sh all       test, check, vendor (only with --allow-network), build, bundle
#
# Nothing here commits, pushes, publishes, signs or downloads game data. Only `vendor` (crates from the lockfile, with --allow-network) uses the network; the Flatpak build
# itself runs offline from the vendored crates.
set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
out="$root/build/flatpak"
stage="$out/src"
manifest="$root/packaging/flatpak/org.tswlauncher.Launcher.json"
app_id="org.tswlauncher.Launcher"
# reproducible timestamps: the date of the latest change recorded in the sources, never "now"
export SOURCE_DATE_EPOCH="${SOURCE_DATE_EPOCH:-1793404800}"
allow_network=0
for a in "$@"; do [ "$a" = "--allow-network" ] && allow_network=1; done

need() { command -v "$1" >/dev/null 2>&1 || { echo "build_flatpak: missing tool '$1' ($2)" >&2; exit 10; }; }

cmd_fetch() {
  [ "$allow_network" = 1 ] || { echo "build_flatpak: 'fetch' downloads the crates of Cargo.lock; run it with --allow-network (CI does)" >&2; exit 13; }
  need cargo "install a Rust toolchain"
  (cd "$root/patcher" && cargo fetch --locked)
  (cd "$root/launcher" && cargo fetch --locked)
  (cd "$root/launcher-gui" && cargo fetch --locked)
}

# Tests of the private development repository only: they check its own .gitignore and tooling, which the public export does not contain. The two trees are told apart by
# AGENTS.md, which exists in the private development repository and never in the public export or the source archive (check_public_source.py refuses it there).
#  - private repository (AGENTS.md present): the test file MUST exist and run; a missing file is an error (exit 1), never a silent skip;
#  - public tree (no AGENTS.md): the file is not part of the tree, a note is printed and nothing else is skipped.
private_only_test() {
  if [ -f "$root/AGENTS.md" ]; then
    [ -f "$root/scripts/$1" ] || { echo "ERROR: $1 is required in the private development repository but missing" >&2; exit 1; }
    python3 -I "$root/scripts/$1"
  else
    echo "(private development test $1 is not part of this tree)"
  fi
}
cmd_test() {
  need cargo "install a Rust toolchain"; need python3 "needed for the policy checks"; need git "needed by the secret-scanner tests"
  # the CI jobs that must run the Flatpak and release tests set TSW_REQUIRE_TOOLS=1: a missing program is then a clear error here, before anything runs, and never a silent skip of
  # those tests (scripts/testtools.py makes the tests themselves fail as well). Without the variable (a developer machine) such tests are skipped.
  if [ "${TSW_REQUIRE_TOOLS:-}" = 1 ]; then
    for t in flatpak tar sha256sum timeout; do need "$t" "needed by the Flatpak and release tests because TSW_REQUIRE_TOOLS=1; in CI install flatpak"; done
  fi
  python3 -I "$root/scripts/check_licenses.py"
  python3 -I "$root/scripts/test_check_licenses.py"
  python3 -I "$root/scripts/test_check_public_source.py"
  (cd "$root/patcher" && cargo test --offline --locked)
  (cd "$root/launcher" && cargo test --offline --locked)
  (cd "$root/launcher-gui" && cargo test --offline --locked)
  python3 -I "$root/scripts/test_localization_metadata.py"
  private_only_test test_check_secrets_in_tree.py
  python3 -I "$root/scripts/test_check_flatpak_package.py"
  python3 -I "$root/scripts/version.py" check
  python3 -I -W ignore "$root/scripts/test_version.py"
  python3 -I "$root/scripts/check_flatpak_package.py" workflow "$root/.github/workflows/flatpak.yml" "$manifest"
  python3 -I "$root/scripts/check_flatpak_package.py" rc-workflow "$root/.github/workflows/rc-artifacts.yml" "$manifest"
  python3 -I "$root/scripts/check_flatpak_package.py" ci-policy "$root/.github/workflows" "$manifest"
  python3 -I "$root/scripts/test_assemble_pages_site.py"
  python3 -I -W ignore "$root/scripts/test_website.py"
  python3 -I "$root/scripts/test_make_update_metadata.py"
  python3 -I "$root/scripts/test_release_draft.py"
  python3 -I "$root/scripts/test_release_candidate.py"
  private_only_test test_testtools.py
  private_only_test test_private_only_test.py
  python3 -I "$root/scripts/test_check_release_artifacts.py"
  python3 -I "$root/scripts/test_check_release_set.py"
  python3 -I "$root/scripts/test_install_native.py"
  python3 -I "$root/scripts/test_native_stage.py"
  python3 -I "$root/scripts/test_distribution_policy.py"
  python3 -I "$root/scripts/test_ci_disk_report.py"
}

cmd_stage() {
  need rsync "copies the sources"
  case "$stage" in "$root/build/flatpak/src") ;; *) echo "build_flatpak: refusing to write outside build/flatpak/src" >&2; exit 11;; esac
  mkdir -p "$stage"
  # keep a previously created vendor/ directory, replace everything else (sources are copied from an explicit include list)
  rsync -a --delete --exclude=/vendor/ --exclude=/.cargo/ \
    --include='/LICENSE' --include='/THIRD-PARTY-NOTICES.md' --include='/LICENSES/***' \
    --include='/patcher/' --include='/patcher/Cargo.toml' --include='/patcher/Cargo.lock' --include='/patcher/src/***' \
    --include='/packaging/project.conf' --include='/packaging/channels/' --include='/packaging/channels/stable.conf' \
    --include='/launcher/' --include='/launcher/Cargo.toml' --include='/launcher/Cargo.lock' --include='/launcher/build.rs' --include='/launcher/src/***' \
    --include='/launcher-gui/' --include='/launcher-gui/Cargo.toml' --include='/launcher-gui/Cargo.lock' --include='/launcher-gui/build.rs' \
    --include='/launcher-gui/src/***' --include='/launcher-gui/ui/***' --include='/launcher-gui/assets/***' --include='/launcher-gui/translations/***' \
    --include='/launcher-gui/packaging/***' --include='/packaging/' --include='/packaging/flatpak/' --include='/packaging/flatpak/org.tswlauncher.Launcher.metainfo.xml' \
    --exclude='*' "$root/" "$stage/"
  mkdir -p "$stage/.cargo"
  install -m 644 "$root/packaging/flatpak/cargo-config.toml" "$stage/.cargo/config.toml"
  # RELEASE build metadata (launcher/build.rs reads it): only the release packaging, TSW_RELEASE_BUILD=1, marks the staged tree. Developer builds, test runs and the source archive never do;
  # the Flatpak branch is `local` for both, so it cannot tell them apart.
  if [ "${TSW_RELEASE_BUILD:-}" = 1 ]; then printf 'release\n' > "$stage/packaging/release-build"; else rm -f "$stage/packaging/release-build"; fi
  echo "staged: $stage"
}

cmd_check() {
  need python3 "needed for the policy checks"
  cmd_stage
  python3 -I "$root/scripts/check_flatpak_package.py" manifest "$manifest"
  python3 -I "$root/scripts/check_flatpak_package.py" tree "$stage"
  if command -v desktop-file-validate >/dev/null; then desktop-file-validate "$stage/launcher-gui/packaging/linux/$app_id.desktop"; else echo "note: desktop-file-validate not installed, desktop file not validated"; fi
  if command -v appstreamcli >/dev/null; then
    # warnings (for example the missing homepage URL) are reported; only errors fail
    appstreamcli validate --no-net "$stage/packaging/flatpak/$app_id.metainfo.xml" || { appstreamcli validate --no-net "$stage/packaging/flatpak/$app_id.metainfo.xml" 2>&1 | grep -q '^E:' && exit 12 || true; }
  else echo "note: appstreamcli not installed, metainfo not validated"; fi
  echo "check: ok"
}

cmd_vendor() {
  [ "$allow_network" = 1 ] || { echo "build_flatpak: 'vendor' downloads the crates of Cargo.lock; run it with --allow-network (CI does)" >&2; exit 13; }
  need cargo "install a Rust toolchain"
  cmd_stage
  rm -rf "$stage/vendor"
  (cd "$root/launcher-gui" && cargo vendor --locked "$stage/vendor" >/dev/null)
  echo "vendored $(ls "$stage/vendor" | wc -l) crates into $stage/vendor"
}

cmd_build() {
  need flatpak-builder "install flatpak-builder (host package) and the Freedesktop 25.08 runtime, SDK and rust-stable extension"
  need flatpak "install flatpak"
  [ -d "$stage/vendor" ] || { echo "build_flatpak: $stage/vendor is missing; run 'vendor --allow-network' first" >&2; exit 14; }
  cmd_check
  # --disable-download: the build never fetches sources; --state-dir/--repo stay inside build/
  # the Flatpak branch of a local/CI build is never stable or testing: only the release workflow puts a build on those (FLATPAK_BRANCH to override locally)
  # no key is used anywhere: the bundle is a plain single file (see bundle_args)
  flatpak-builder --force-clean --disable-download --disable-rofiles-fuse --default-branch="${FLATPAK_BRANCH:-local}" --state-dir="$out/state" --repo="$out/repo" "$out/build-dir" "$manifest"
  echo "built into $out/repo (branch ${FLATPAK_BRANCH:-local})"
}

# No extra arguments of `flatpak build-bundle` since 0.4.1: every bundle is a plain single file that is installed with `flatpak install --user <file>.flatpak`; no update source (no
# hosted repository exists) and no key is put into it. The branch is `local` for releases and developer builds alike (the branch of the earlier bundles, so that an installed version is
# updated in place); a RELEASE is told from a developer build by `TSW_RELEASE_BUILD=1` (see cmd_stage), not by the branch. `stable` and `testing` stay accepted for hand-made packages.
bundle_args() {
  local branch="${FLATPAK_BRANCH:-local}"
  case "$branch" in
    local|testing|stable) ;;
    *) echo "build_flatpak: FLATPAK_BRANCH must be stable, testing or local (not '$branch')" >&2; exit 17;;
  esac
}

cmd_bundle() {
  need flatpak "install flatpak"
  [ -d "$out/repo" ] || { echo "build_flatpak: run 'build' first" >&2; exit 15; }
  bundle_args
  flatpak build-bundle "$out/repo" "$out/tsw-launcher.flatpak" "$app_id" "${FLATPAK_BRANCH:-local}"
  (cd "$out" && sha256sum tsw-launcher.flatpak | tee tsw-launcher.flatpak.sha256)
}

case "${1:-}" in
  fetch) cmd_fetch;;
  test) cmd_test;;
  stage) cmd_stage;;
  check) cmd_check;;
  vendor) cmd_vendor;;
  build) cmd_build;;
  bundle) cmd_bundle;;
  bundle-args) bundle_args; echo "(no extra build-bundle arguments)";;
  all) cmd_test; cmd_check; [ "$allow_network" = 1 ] && cmd_vendor; cmd_build; cmd_bundle;;
  *) sed -n '2,18p' "${BASH_SOURCE[0]}"; exit 2;;
esac
