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
#   scripts/build_flatpak.sh publish-repo <branch>...  release workflow only: build into the repository, GPG-signed, on the given Flatpak branches (stable, testing);
#                                      keeps the branches that stay as they are by mirroring them from the live repository first
#                                      (needs FLATPAK_GPG_KEY_ID + the key in GNUPGHOME; optional FLATPAK_LIVE_REPO_URL + FLATPAK_PUBLIC_KEY to mirror)
#   scripts/build_flatpak.sh all       test, check, vendor (only with --allow-network), build, bundle
#
# Nothing here commits, pushes, publishes or downloads game data. Only `vendor` uses the network (crates from the lockfile), and only
# when called with --allow-network; the Flatpak build itself runs offline from the vendored crates.
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

cmd_test() {
  need cargo "install a Rust toolchain"; need python3 "needed for the policy checks"
  python3 -I "$root/scripts/check_licenses.py"
  (cd "$root/patcher" && cargo test --offline --locked)
  (cd "$root/launcher" && cargo test --offline --locked)
  (cd "$root/launcher-gui" && cargo test --offline --locked)
  python3 -I "$root/scripts/version.py" check
  cmd_check
}

cmd_stage() {
  need rsync "copies the sources"
  case "$stage" in "$root/build/flatpak/src") ;; *) echo "build_flatpak: refusing to write outside build/flatpak/src" >&2; exit 11;; esac
  mkdir -p "$stage"
  # keep a previously created vendor/ directory, replace everything else (sources are copied from an explicit include list)
  rsync -a --delete --exclude=/vendor/ --exclude=/.cargo/ \
    --include='/LICENSE' --include='/THIRD-PARTY-NOTICES.md' --include='/LICENSES/***' \
    --include='/patcher/' --include='/patcher/Cargo.toml' --include='/patcher/Cargo.lock' --include='/patcher/src/***' \
    --include='/packaging/project.conf' \
    --include='/launcher/' --include='/launcher/Cargo.toml' --include='/launcher/Cargo.lock' --include='/launcher/src/***' \
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
  flatpak-builder --force-clean --disable-download --disable-rofiles-fuse --default-branch="${FLATPAK_BRANCH:-local}" --state-dir="$out/state" --repo="$out/repo" "$out/build-dir" "$manifest"
  echo "built into $out/repo (branch ${FLATPAK_BRANCH:-local})"
}

cmd_publish_repo() {
  need flatpak-builder "install flatpak-builder"; need flatpak "install flatpak"; need ostree "install ostree"
  local branches=() b
  for b in "$@"; do case "$b" in stable|testing) branches+=("$b");; --*) ;; *) echo "build_flatpak: unknown branch '$b' (stable or testing)" >&2; exit 17;; esac; done
  [ "${#branches[@]}" -ge 1 ] || { echo "build_flatpak: publish-repo needs at least one branch (stable, testing)" >&2; exit 17; }
  [ -n "${FLATPAK_GPG_KEY_ID:-}" ] || { echo "build_flatpak: FLATPAK_GPG_KEY_ID is not set" >&2; exit 16; }
  [ -d "$stage/vendor" ] || { echo "build_flatpak: run 'vendor --allow-network' first" >&2; exit 14; }
  cmd_check
  local repo="$out/repo" ref_prefix="app/$app_id/x86_64"
  rm -rf "$repo"
  ostree init --repo="$repo" --mode=archive-z2
  # A GitHub Pages deployment replaces the whole site, so the branches that are NOT republished must be carried over from the live repository
  # (mirror pull keeps the original signatures). The remote is verified against the project's public key, never trusted blindly.
  if [ -n "${FLATPAK_LIVE_REPO_URL:-}" ] && [ -n "${FLATPAK_LIVE_BRANCHES:-}" ]; then
    [ -s "${FLATPAK_PUBLIC_KEY:-}" ] || { echo "build_flatpak: FLATPAK_PUBLIC_KEY is required to mirror the live repository" >&2; exit 18; }
    ostree remote add --repo="$repo" --gpg-import="$FLATPAK_PUBLIC_KEY" live "$FLATPAK_LIVE_REPO_URL"
    for b in $FLATPAK_LIVE_BRANCHES; do
      case " ${branches[*]} " in *" $b "*) continue;; esac
      ostree pull --repo="$repo" --mirror live "$ref_prefix/$b"
    done
  fi
  flatpak-builder --force-clean --disable-download --disable-rofiles-fuse --default-branch="${branches[0]}" --gpg-sign="$FLATPAK_GPG_KEY_ID" --state-dir="$out/state" --repo="$repo" "$out/build-dir" "$manifest"
  for b in "${branches[@]:1}"; do
    flatpak build-commit-from --gpg-sign="$FLATPAK_GPG_KEY_ID" --src-ref="$ref_prefix/${branches[0]}" "$repo" "$ref_prefix/$b"
  done
  # depth 2 keeps the new and the previous commit of every branch, so static deltas can be made and the site stays small
  flatpak build-update-repo --gpg-sign="$FLATPAK_GPG_KEY_ID" --generate-static-deltas --prune --prune-depth=2 "$repo"
  echo "signed repository: $repo (branches: ${branches[*]})"
}

cmd_bundle() {
  need flatpak "install flatpak"
  [ -d "$out/repo" ] || { echo "build_flatpak: run 'build' first" >&2; exit 15; }
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
  publish-repo) shift; cmd_publish_repo "$@";;
  all) cmd_test; cmd_check; [ "$allow_network" = 1 ] && cmd_vendor; cmd_build; cmd_bundle;;
  *) sed -n '2,16p' "${BASH_SOURCE[0]}"; exit 2;;
esac
