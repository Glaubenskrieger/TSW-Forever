#!/usr/bin/env bash
# Mirrors the LIVE stable branch of the Flatpak repository into the repository that is being built, so that the new release gets the previous one as its parent and the static
# deltas (previous -> new) can be made: users then download only the difference. Called by `scripts/build_flatpak.sh stable-repo` when a live repository is configured.
#
#   scripts/flatpak_mirror_live.sh <repository> <ref> <live-url> <public-key.gpg> [<previous-public-key.gpg> ...]
#
# A live repository that was GIVEN must be mirrored: a mirror that fails (unreachable, a bad answer, a summary or commit that does not verify) must never be taken for a first release,
# because that would silently publish a release without its history. So the outcomes are told apart by the exit code AND by a message that starts with "MIRROR:":
#   0   mirrored      the live branch is in <repository>; the new commit gets it as its parent                    "MIRROR: mirrored ..."
#   3   first release the live repository was read and its summary VERIFIED, and it has no <ref> yet: there is no history to take over    "MIRROR: first release of this branch ..."
#                     (an empty <live-url> is also exit 3, "MIRROR: first release (no live repository configured)")
#   18  key problem   a key file is missing, empty, holds a SECRET key, or cannot be imported
#   19  FAILED        the live repository could not be read, verified or mirrored: no release is built from it  "MIRROR: FAILED ..."
#   2   wrong call or a missing tool
# For a real first release (no live repository exists at all) do not give a live URL. A 404 or an unreachable URL is a failure (19), not a first release.
#
# Trust: the summary and every mirrored commit are verified (gpg-verify and gpg-verify-summary on, never off) against <public-key.gpg> and the previous keys named here. After a KEY ROTATION the
# live history is signed with the previous key only; it can be taken over only if that previous PUBLIC key is named explicitly (further arguments, or FLATPAK_RELEASE_EXTRA_TRUSTED_KEYS in
# build_flatpak.sh). Those keys vouch for the OLD history only: the new commit is made locally, is signed afterwards with this release's key, and the release scripts verify it with that key alone.
# Key files must hold PUBLIC keys; a file with a secret key is refused. Everything the script imports lives in the repository that is being built; it deletes nothing.
set -uo pipefail
die() { echo "MIRROR: FAILED: $*" >&2; exit "${2:-19}"; }
usage() { echo "usage: $0 <repository> <ref> <live-url> <public-key.gpg> [<previous-public-key.gpg> ...]" >&2; exit 2; }
[ $# -ge 4 ] || usage
repo="$1"; ref="$2"; url="$3"; key="$4"; shift 4
extras=("$@")
for t in ostree timeout; do command -v "$t" >/dev/null 2>&1 || { echo "missing tool: $t" >&2; exit 2; }; done
[ -d "$repo/objects" ] || { echo "$repo is not an OSTree repository" >&2; exit 2; }
case "$ref" in app/*/*/*) ;; *) echo "not an application ref: $ref" >&2; exit 2;; esac

if [ -z "$url" ]; then
  echo "MIRROR: first release (no live repository configured): no history, the new commit has no parent and there are no deltas from a previous release"
  exit 3
fi
case "$url" in http://*|https://*) ;; *) echo "the live url must be http(s): $url" >&2; exit 2;; esac

command -v gpg >/dev/null 2>&1 || die "gpg is needed to check that the key files hold public keys only" 18
for kf in "$key" "${extras[@]+"${extras[@]}"}"; do
  [ -s "$kf" ] || die "the key file is empty or missing: $kf" 18
  if gpg --list-packets "$kf" 2>/dev/null | grep -q ':secret key packet'; then die "$kf holds a SECRET key: only public keys are accepted" 18; fi
done

trust="$(mktemp)" || exit 2
cat "$key" "${extras[@]+"${extras[@]}"}" > "$trust"
fp() { gpg --show-keys --with-colons "$1" 2>/dev/null | awk -F: '/^fpr/{print $10; exit}'; }
echo "MIRROR: trusted for the live history: $key $(fp "$key")"
for kf in "${extras[@]+"${extras[@]}"}"; do echo "MIRROR: WARNING: also trusting, for the OLD history only (explicit previous key): $kf $(fp "$kf")"; done
if ! ostree remote add --repo="$repo" --gpg-import="$trust" --set=gpg-verify-summary=true live "$url" >/dev/null 2>&1; then rm -f "$trust"; die "the keys could not be imported for the live remote" 18; fi
rm -f "$trust"

if ! out="$(timeout 180 ostree remote refs --repo="$repo" live 2>&1)"; then
  hint=""
  case "$out" in *"public key not found"*|*"signature"*|*"Signature"*) hint=" If the live history was signed with a PREVIOUS key (key rotation), name that previous PUBLIC key explicitly (FLATPAK_RELEASE_EXTRA_TRUSTED_KEYS).";; esac
  die "the live repository $url could not be read or its summary does not verify: $(printf '%s' "$out" | tail -1 | cut -c1-200).$hint For a real FIRST release do not give a live URL." 19
fi
if ! printf '%s\n' "$out" | grep -qxF "live:$ref"; then
  echo "MIRROR: first release of this branch: the live repository $url was read and its summary verified, and it has no $ref yet: no history to take over"
  exit 3
fi
if ! out="$(timeout 900 ostree pull --repo="$repo" --mirror live "$ref" 2>&1)"; then
  hint=""
  case "$out" in *"public key not found"*|*"signature"*|*"Signature"*) hint=" If the live history was signed with a PREVIOUS key (key rotation), name that previous PUBLIC key explicitly (FLATPAK_RELEASE_EXTRA_TRUSTED_KEYS).";; esac
  die "the live branch $ref could not be mirrored: $(printf '%s' "$out" | tail -1 | cut -c1-200).$hint" 19
fi
head="$(ostree rev-parse --repo="$repo" "$ref" 2>/dev/null)" || die "the mirror finished but $ref is not in the repository" 19
echo "MIRROR: mirrored $ref at ${head:0:12}: the new release gets it as its parent"
exit 0
