#!/usr/bin/env bash
# NOT FOR PUBLICATION (since 2026-11-20 the release pipeline .github/workflows/release.yml signs in CI with the signing subkey; see documentation/release-pipeline.md). Kept as a TEST and EMERGENCY tool.
# The OFFLINE build of the stable Flatpak: build, sign with a private key, verify, bundle and package, on the OWNER's machine. The private key stays in a GNUPGHOME
# outside every repository and is never given to CI. Nothing is published by this script: it ends with the exact commands for you to run.
#
#   scripts/flatpak_release_offline.sh <gnupg-home> <key-id> [--live-url <https repo url>] [--allow-dirty --allow-untagged]
#
# Preconditions (checked): the tree is clean and HEAD is the commit of the tag v<APP_VERSION> (a release is built from its tag); scripts/version.py check passes;
# packaging/flatpak/stable-repo.gpg exists and IS the public half of <key-id>; the build tools and the vendored crates exist (scripts/build_flatpak.sh vendor --allow-network).
# FLATPAK_STABLE_KEY overrides the public key file (for trying only). --allow-dirty / --allow-untagged exist ONLY for trying the procedure with a throw-away key: such output must never be published.
# FLATPAK_RELEASE_EXTRA_TRUSTED_KEYS (key rotation only): colon-separated PUBLIC key files of PREVIOUS signing keys. Without it only the public key of this release is trusted, so the check of
# step (4) refuses an older commit in the repository that is signed only with an older key. With it, those keys are trusted for the OLDER commits of that check only (listed with a WARNING; the
# newest commit and the summary must verify with this release's key alone). Set it deliberately, for the one release that rotates the key; see documentation/packaging.md. The same variable is
# read by the mirror of step (1) (scripts/flatpak_mirror_live.sh): a live history signed only with a previous key can be taken over only with that key named; without it the build stops (exit 19).
# A live repository that was given and cannot be mirrored (unreachable, 404, a signature that does not verify) stops the release: it is never taken for a first release. For a real first release give no --live-url.
#
# Steps: (1) mirror the live stable branch (if --live-url or FLATPAK_LIVE_REPO_URL: the new commit gets the previous one as its parent, so users download only the difference; it must succeed),
# (2) build the UNSIGNED stable repository, (3) sign commits and summary with your key (the static deltas are generated AFTER the commits are signed), (4) verify against the public key
# AND prove the update itself: scripts/check_flatpak_delta_update.sh --require-delta updates a scratch repository over HTTP over every static delta of the newest commit with the
# commit and summary signatures checked; a failure stops the release here (exit 21) and nothing is bundled, (5) build the signed stable bundle (it carries the update source and the
# public key), (6) write build/flatpak-release/<version>/: the bundle, the repository archive and the checksums.
set -euo pipefail
root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$root"
die() { echo "flatpak_release_offline: $*" >&2; exit "${2:-1}"; }
home="${1:?gnupg home (outside the repository)}"; key="${2:?key id}"; shift 2
live="${FLATPAK_LIVE_REPO_URL:-}"; dirty=0; untagged=0
while [ $# -gt 0 ]; do
  case "$1" in
    --live-url) live="${2:?url}"; shift 2;;
    --allow-dirty) dirty=1; shift;;
    --allow-untagged) untagged=1; shift;;
    *) die "unknown option $1" 2;;
  esac
done
for t in git gpg flatpak ostree tar sha256sum python3; do command -v "$t" >/dev/null || die "missing tool $t" 10; done
python3 -I scripts/version.py check >/dev/null || die "scripts/version.py check fails"
version="$(python3 -I scripts/version.py show)"
pub="${FLATPAK_STABLE_KEY:-$root/packaging/flatpak/stable-repo.gpg}"
[ -s "$pub" ] || die "packaging/flatpak/stable-repo.gpg is missing: the public key of the project is provided by the owner (see documentation/release-pipeline.md)"
case "$(realpath -m "$home")/" in "$root"/*) die "the keyring must be outside the repository" 3;; esac
# key rotation: previous public keys for the history check, only when named deliberately; checked here, before anything is built
extra_keys=()
if [ -n "${FLATPAK_RELEASE_EXTRA_TRUSTED_KEYS:-}" ]; then
  IFS=: read -r -a extra_files <<< "$FLATPAK_RELEASE_EXTRA_TRUSTED_KEYS"
  for f in "${extra_files[@]}"; do
    [ -n "$f" ] || continue
    [ -s "$f" ] || die "FLATPAK_RELEASE_EXTRA_TRUSTED_KEYS: $f is empty or missing" 2
    extra_keys+=(--extra-key "$f")
  done
  [ "${#extra_keys[@]}" -eq 0 ] || echo "WARNING: key rotation: ${#extra_files[@]} previous public key(s) are trusted for the OLDER commits of the update check (FLATPAK_RELEASE_EXTRA_TRUSTED_KEYS)" >&2
fi
# the public key file must be the public half of the key that will sign
tmpkey="$(mktemp)"; trap 'rm -f "$tmpkey"' EXIT
GNUPGHOME="$home" gpg --export "$key" > "$tmpkey" 2>/dev/null || die "key $key not found in $home"
cmp -s "$tmpkey" "$pub" || { rm -f "$tmpkey"; die "packaging/flatpak/stable-repo.gpg is not the public key of $key"; }
rm -f "$tmpkey"
if [ "$dirty" = 0 ] && [ -n "$(git status --porcelain)" ]; then die "the working tree is not clean: a release is built from a committed tree"; fi
if [ "$untagged" = 0 ]; then
  tag_commit="$(git rev-parse -q --verify "refs/tags/v$version^{commit}" || true)"
  [ -n "$tag_commit" ] || die "the tag v$version does not exist here (git fetch --tags)"
  [ "$tag_commit" = "$(git rev-parse HEAD)" ] || die "HEAD is not the commit of the tag v$version: check out the tag first"
fi
[ "$dirty" = 0 ] && [ "$untagged" = 0 ] || echo "WARNING: --allow-dirty/--allow-untagged: this output is for trying only, never for publication" >&2

export FLATPAK_BRANCH=stable FLATPAK_STABLE_KEY="$pub"
[ -n "$live" ] && export FLATPAK_LIVE_REPO_URL="$live"
scripts/build_flatpak.sh stable-repo
scripts/flatpak_keys.sh sign-repo "$home" "$key" build/flatpak/repo
scripts/flatpak_keys.sh verify-repo "$pub" build/flatpak/repo
# verify-repo checks the refs and the summary, not the static deltas; a delta generated before the signing carries no signature, and a client that takes the delta path refuses it.
# So the update itself is proven here, with this release's public key only (plus the previous keys named in FLATPAK_RELEASE_EXTRA_TRUSTED_KEYS for the older commits): no bundle without it.
scripts/check_flatpak_delta_update.sh --require-delta "${extra_keys[@]+"${extra_keys[@]}"}" build/flatpak/repo "$pub" || die "the signed repository cannot be updated over its static deltas (see above): nothing is bundled" 21
scripts/build_flatpak.sh bundle

dest="build/flatpak-release/$version"
rm -rf "${dest:?}"; mkdir -p "$dest"
cp build/flatpak/tsw-launcher.flatpak "$dest/tsw-forever-$version.flatpak"
# the repository as an archive; no owner, deterministic order
tar --sort=name --owner=0 --group=0 --numeric-owner -C build/flatpak/repo -czf "$dest/tsw-forever-$version-flatpak-repo.tar.gz" .
(cd "$dest" && sha256sum "tsw-forever-$version.flatpak" "tsw-forever-$version-flatpak-repo.tar.gz" > SHA256SUMS-flatpak && cat SHA256SUMS-flatpak)
cat <<MSG

Done: $dest
  tsw-forever-$version.flatpak                signed stable bundle (update source + public key); same name as the release's bundle
  tsw-forever-$version-flatpak-repo.tar.gz    the signed repository for GitHub Pages
Nothing was published. This is a test and emergency tool: a real release is built, signed and published by the pipeline (documentation/release-pipeline.md); do not upload these files by hand.
MSG
