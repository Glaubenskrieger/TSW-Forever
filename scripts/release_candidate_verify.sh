#!/usr/bin/env bash
# Sorts the downloaded artifacts of a build run (folder `all/`: one folder per artifact) into `manifest/` and `parts/` and verifies every file against candidate.json (version, full commit
# id, SHA-256). Shared by the release workflow (job `candidate`) and the read-only release rehearsal. FAIL-CLOSED: a missing, extra or unexpected artifact, a symbolic link, another commit or
# version in the manifest, a missing or additional file or one changed byte stops everything with a message; nothing is repaired, replaced or taken from elsewhere. Writes only the two
# folders next to `all/`.
#
#   release_candidate_verify.sh            Environment: VERSION, GITHUB_SHA
set -euo pipefail
die() { echo "release candidate: $*" >&2; exit 1; }
: "${VERSION:?}" "${GITHUB_SHA:?}"
[[ "$GITHUB_SHA" =~ ^[0-9a-f]{40}$ ]] || die "GITHUB_SHA is not a full commit id: $GITHUB_SHA"
again="Run 'gh workflow run build.yml' on this commit and start this workflow again; artifacts of another run or commit are never used."
[ -d all ] || die "the artifacts were not downloaded (there is no folder 'all'). They have probably expired or were deleted. $again"
[ -z "$(find all -type l)" ] || die "a symbolic link in the downloaded artifacts: refused"
for kind in candidate flatpak source windows; do
  [ -d "all/tsw-forever-${VERSION}-${kind}-${GITHUB_SHA}" ] || die "the artifact tsw-forever-${VERSION}-${kind}-${GITHUB_SHA} was not downloaded: it has expired, was deleted, or never existed for this commit. $again"
done
cand="all/tsw-forever-${VERSION}-candidate-${GITHUB_SHA}"
[ "$(ls -A "$cand")" = "candidate.json" ] || die "the candidate artifact must hold exactly candidate.json (found: $(ls -A "$cand" | tr '\n' ' '))"
for d in all/*/; do
  case "$(basename "$d")" in
    "tsw-forever-${VERSION}-candidate-${GITHUB_SHA}"|"tsw-forever-${VERSION}-flatpak-${GITHUB_SHA}"|"tsw-forever-${VERSION}-source-${GITHUB_SHA}"|"tsw-forever-${VERSION}-windows-${GITHUB_SHA}") ;;
    *) die "unexpected artifact: $(basename "$d") (only the four artifacts of version ${VERSION} and commit ${GITHUB_SHA} are accepted)" ;;
  esac
done
[ -z "$(find all -mindepth 1 -maxdepth 1 ! -type d)" ] || die "unexpected files next to the artifacts"
mkdir -p manifest parts
mv "$cand/candidate.json" manifest/candidate.json
rmdir "$cand"
for kind in flatpak source windows; do mv "all/tsw-forever-${VERSION}-${kind}-${GITHUB_SHA}" parts/; done
test -z "$(ls -A all)" || die "unexpected artifacts: $(ls all)"
python3 -I scripts/release_candidate.py verify parts manifest/candidate.json --version "$VERSION" --commit "$GITHUB_SHA"
