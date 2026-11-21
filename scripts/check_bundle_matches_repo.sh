#!/usr/bin/env bash
# Proves that the Flatpak BUNDLE that is attached to a release is the bundle of the SIGNED repository: it carries the same commit as the repository, the project's update source
# (the https repository address) and the project's public key. This refuses a bundle that was built from an unsigned or another repository (for example the `local` bundle of a CI test build).
#
#   scripts/check_bundle_matches_repo.sh <bundle.flatpak> <ostree repository> <public-key.gpg> <repo-url> [ref]
#
# ref defaults to the only app ref of the repository. Writes only into a temporary folder. Exit 0 = matches, 1 = mismatch, 2 = wrong call or missing tool.
set -euo pipefail
export LC_ALL=C
die() { echo "check_bundle_matches_repo: $*" >&2; exit "${2:-1}"; }
bundle="${1:?bundle}"; repo="${2:?repository}"; pub="${3:?public key}"; url="${4:?repo url}"; ref="${5:-}"
for t in flatpak ostree python3; do command -v "$t" >/dev/null 2>&1 || die "missing tool $t" 2; done
[ -s "$bundle" ] || die "the bundle is missing or empty: $bundle" 2
[ -d "$repo/objects" ] || die "$repo is not an OSTree repository" 2
[ -s "$pub" ] || die "the public key file is missing or empty: $pub" 2
case "$url" in https://*) ;; *) die "the repository address must be an https address" 2;; esac
if [ -z "$ref" ]; then
  refs="$(ostree refs --repo="$repo" | grep '^app/' || true)"
  [ "$(printf '%s\n' "$refs" | grep -c .)" -eq 1 ] || die "the repository must have exactly one app ref (give the ref): $refs" 2
  ref="$refs"
fi
tmp="$(mktemp -d)"; trap 'rm -rf "${tmp:?}"' EXIT
ostree --repo="$tmp/imp" init --mode=archive
flatpak build-import-bundle "$tmp/imp" "$bundle" >/dev/null 2>&1 || die "the file is not a Flatpak bundle"
inbundle="$(ostree rev-parse --repo="$tmp/imp" "$ref" 2>/dev/null)" || die "the bundle does not contain $ref"
inrepo="$(ostree rev-parse --repo="$repo" "$ref" 2>/dev/null)" || die "the repository does not contain $ref"
[ "$inbundle" = "$inrepo" ] || die "the bundle carries commit ${inbundle:0:12}, the signed repository ${inrepo:0:12}: this is not the bundle of the signed repository"
# the update source and the public key are stored in the bundle's metadata as plain bytes
python3 -I - "$bundle" "$pub" "$url" <<'PY' || exit 1
import sys
bundle, pub, url = sys.argv[1:4]
data = open(bundle, "rb").read()
key = open(pub, "rb").read()
if url.encode() not in data:
    sys.exit("check_bundle_matches_repo: the bundle does not carry the update source " + url)
if key not in data:
    sys.exit("check_bundle_matches_repo: the bundle does not carry the project's public key")
PY
echo "bundle matches the signed repository: $ref ${inrepo:0:12}, update source and public key present"
