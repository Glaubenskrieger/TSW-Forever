#!/usr/bin/env bash
# Build the Flatpak of the launcher. Platform-neutral: the CI definition is only a thin wrapper around this script.
#
#   scripts/build_flatpak.sh fetch     cargo fetch --locked for the tests on a machine with an empty cargo cache   (NEEDS NETWORK)
#   scripts/build_flatpak.sh test      unit tests of the launcher (offline) + policy tests            (needs cargo, python3)
#   scripts/build_flatpak.sh stage     copy ONLY the launcher sources into build/flatpak/src            (needs rsync)
#   scripts/build_flatpak.sh check     stage + validate manifest, desktop file, metainfo, staged tree (no network, no flatpak)
#   scripts/build_flatpak.sh vendor    cargo vendor from the committed Cargo.lock into build/flatpak/src/vendor   (NEEDS NETWORK)
#   scripts/build_flatpak.sh build     flatpak-builder (offline; needs the runtime, SDK, rust extension and vendor/ installed)
#   scripts/build_flatpak.sh bundle    export build/flatpak/tsw-launcher.flatpak from the build repository
#   scripts/build_flatpak.sh bundle-args  print the extra `flatpak build-bundle` arguments of the current FLATPAK_BRANCH (stable: the update source and the public key); nothing is built
#   scripts/build_flatpak.sh stable-repo  build the UNSIGNED stable repository of this version (mirrors the live one first when FLATPAK_LIVE_REPO_URL is set); signing is offline
#   scripts/build_flatpak.sh all       test, check, vendor (only with --allow-network), build, bundle
#
# Nothing here commits, pushes, publishes, signs or downloads game data. Only `vendor` (crates from the lockfile, with --allow-network) and `stable-repo` with
# FLATPAK_LIVE_REPO_URL (mirror of the published stable branch) use the network; the Flatpak build itself runs offline from the vendored crates.
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
  # the CI jobs that must run the security tests (signing, mirror, delta update, release) set TSW_REQUIRE_TOOLS=1: a missing program is then a clear error here, before anything runs, and
  # never a silent skip of those tests (scripts/testtools.py makes the tests themselves fail as well). Without the variable (a developer machine) such tests are skipped.
  if [ "${TSW_REQUIRE_TOOLS:-}" = 1 ]; then
    for t in flatpak ostree gpg tar sha256sum timeout; do need "$t" "needed by the signing, mirror, delta and release tests because TSW_REQUIRE_TOOLS=1; in CI install flatpak ostree gnupg"; done
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
  python3 -I "$root/scripts/test_flatpak_channel.py"
  python3 -I "$root/scripts/test_assemble_pages_site.py"
  python3 -I -W ignore "$root/scripts/test_website.py"
  python3 -I "$root/scripts/test_make_update_metadata.py"
  python3 -I "$root/scripts/test_flatpak_keys.py"
  python3 -I "$root/scripts/test_flatpak_sign_ci.py"
  python3 -I "$root/scripts/test_check_flatpak_live.py"
  python3 -I "$root/scripts/test_check_bundle_matches_repo.py"
  python3 -I "$root/scripts/test_release_draft.py"
  python3 -I "$root/scripts/test_release_candidate.py"
  python3 -I "$root/scripts/test_check_flatpak_delta_update.py"
  python3 -I "$root/scripts/test_flatpak_release_offline.py"
  python3 -I "$root/scripts/test_flatpak_mirror_live.py"
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
  # a stable bundle must come from a SIGNED repository (installing it enables signature checking); signing is OFFLINE, after the build:
  # scripts/flatpak_keys.sh sign-repo (see scripts/flatpak_release_offline.sh). No signing key is ever used in a build or in CI.
  flatpak-builder --force-clean --disable-download --disable-rofiles-fuse --default-branch="${FLATPAK_BRANCH:-local}" --state-dir="$out/state" --repo="$out/repo" "$out/build-dir" "$manifest"
  echo "built into $out/repo (branch ${FLATPAK_BRANCH:-local})"
}

# The STABLE repository of this version, UNSIGNED: signing is done offline afterwards (scripts/flatpak_keys.sh sign-repo). If the live repository is given
# (FLATPAK_LIVE_REPO_URL + FLATPAK_STABLE_KEY, the public key), its stable branch is mirrored first (scripts/flatpak_mirror_live.sh), so the new commit has the previous one as its
# parent and the static deltas (previous -> new) can be made: users then download the difference only. Without a live repository (first release) there is no history.
# A live repository that was GIVEN must be mirrored: if that fails (unreachable, a summary or commit that does not verify, a 404) NO release is built (exit 19), because it would
# otherwise go out silently without its history; only a live repository that was read, with a verified summary, and that has no stable branch yet counts as a first release.
# After a key rotation the previous PUBLIC key(s) must be named deliberately (FLATPAK_RELEASE_EXTRA_TRUSTED_KEYS, colon-separated): they vouch for the old history only.
cmd_stable_repo() {
  need flatpak-builder "install flatpak-builder"; need flatpak "install flatpak"; need ostree "install ostree"
  [ -d "$stage/vendor" ] || { echo "build_flatpak: run 'vendor --allow-network' first" >&2; exit 14; }
  cmd_check
  local repo="$out/repo" ref="app/$app_id/x86_64/stable"
  rm -rf "$repo"
  ostree init --repo="$repo" --mode=archive-z2
  if [ -z "${FLATPAK_LIVE_REPO_URL:-}" ]; then
    echo "build_flatpak: first release: no live repository is configured (FLATPAK_LIVE_REPO_URL is empty): no history, the new commit has no parent and there are no deltas from a previous release" >&2
  else
    local key="${FLATPAK_STABLE_KEY:-$root/packaging/flatpak/stable-repo.gpg}" extra=() item items=() rc=0
    [ -s "$key" ] || { echo "build_flatpak: the public key ($key) is needed to mirror the live repository" >&2; exit 18; }
    if [ -n "${FLATPAK_RELEASE_EXTRA_TRUSTED_KEYS:-}" ]; then
      IFS=: read -r -a items <<< "$FLATPAK_RELEASE_EXTRA_TRUSTED_KEYS"
      for item in "${items[@]}"; do [ -z "$item" ] || extra+=("$item"); done
    fi
    [ -f "$root/scripts/flatpak_mirror_live.sh" ] || { echo "build_flatpak: scripts/flatpak_mirror_live.sh is missing: a live repository can only be mirrored with it (a tool of the release owner, not part of the source archive)" >&2; exit 19; }
    bash "$root/scripts/flatpak_mirror_live.sh" "$repo" "$ref" "$FLATPAK_LIVE_REPO_URL" "$key" "${extra[@]+"${extra[@]}"}" || rc=$?
    case "$rc" in
      0) ;;  # mirrored: the new commit gets the live branch as its parent
      3) echo "build_flatpak: nothing to take over (a first release of this branch, see the MIRROR line above): continuing without history" >&2;;
      *) echo "build_flatpak: the live repository was given but could not be mirrored (exit $rc): no release is built, so that it never goes out without its history" >&2; exit "$rc";;
    esac
  fi
  flatpak-builder --force-clean --disable-download --disable-rofiles-fuse --default-branch=stable --state-dir="$out/state" --repo="$repo" "$out/build-dir" "$manifest"
  echo "unsigned stable repository: $repo (sign it offline before it is bundled or published)"
}

# Extra arguments of `flatpak build-bundle`, one per line. A STABLE bundle knows its update source: installing it configures the project's repository as the
# remote (with its public signing key), so the host's Flatpak and the update portal can update it. A local or testing bundle has no update source (testing
# packages are handed out by hand). Any other branch is refused; so is a stable bundle without the public key (it would silently have no update source).
bundle_args() {
  local branch="${FLATPAK_BRANCH:-local}" key
  case "$branch" in
    local|testing) ;;
    stable)
      key="${FLATPAK_STABLE_KEY:-$root/packaging/flatpak/stable-repo.gpg}"
      [ -s "$key" ] || { echo "build_flatpak: a stable bundle needs the public signing key ($key); see documentation/packaging.md (key management)" >&2; exit 19; }
      # shellcheck disable=SC1091
      ( . "$root/packaging/project.conf"; case "$FLATPAK_REPO_URL" in https://*) ;; *) echo "build_flatpak: FLATPAK_REPO_URL must be an https address" >&2; exit 19;; esac
        printf '%s\n' "--repo-url=$FLATPAK_REPO_URL" "--gpg-keys=$key" );;
    *) echo "build_flatpak: FLATPAK_BRANCH must be stable, testing or local (not '$branch')" >&2; exit 17;;
  esac
}

cmd_bundle() {
  need flatpak "install flatpak"
  [ -d "$out/repo" ] || { echo "build_flatpak: run 'build' first" >&2; exit 15; }
  local extra=()
  mapfile -t extra < <(bundle_args)
  if [ "${FLATPAK_BRANCH:-local}" = stable ]; then
    # installing a bundle with a public key enables signature checking: an unsigned repository would give a bundle nobody can install
    "$root/scripts/flatpak_keys.sh" verify-repo "${FLATPAK_STABLE_KEY:-$root/packaging/flatpak/stable-repo.gpg}" "$out/repo" \
      || { echo "build_flatpak: the repository is not signed with the public key: sign it first (FLATPAK_GPG_KEY_ID at build time, or scripts/flatpak_keys.sh sign-repo)" >&2; exit 20; }
  fi
  flatpak build-bundle ${extra[@]+"${extra[@]}"} "$out/repo" "$out/tsw-launcher.flatpak" "$app_id" "${FLATPAK_BRANCH:-local}"
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
  bundle-args) bundle_args;;
  stable-repo) cmd_stable_repo;;
  all) cmd_test; cmd_check; [ "$allow_network" = 1 ] && cmd_vendor; cmd_build; cmd_bundle;;
  *) sed -n '2,18p' "${BASH_SOURCE[0]}"; exit 2;;
esac
