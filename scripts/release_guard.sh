#!/usr/bin/env bash
# The checks of the release guard, shared by the release workflow (job `guard`) and the read-only release rehearsal. One implementation, so that what the rehearsal runs is what the release runs.
#
#   release_guard.sh              REAL run (tag push): TAG_NAME must be vMAJOR.MINOR.PATCH, the checkout must be the tagged commit, which must be on origin/main; the tag must equal
#                                 APP_VERSION; actions are pinned; the version must be newer than the newest published release (no downgrade, no re-publish)
#   release_guard.sh --rehearse   REHEARSAL (no tag exists): the tag is taken as v<APP_VERSION>; the same checks run, except the identity of a tag that does not exist
#
# Environment: GITHUB_SHA, GITHUB_REPOSITORY, GH_TOKEN (read access only), TAG_NAME (real run), GITHUB_OUTPUT (optional: `version=` is written to it).
# Only reads: git history, `gh release list`. Creates nothing, publishes nothing.
set -euo pipefail
mode="real"
[ "${1:-}" = "--rehearse" ] && mode="rehearse"
die() { echo "release_guard: $*" >&2; exit 1; }
: "${GITHUB_SHA:?GITHUB_SHA is needed}"
python3 scripts/version.py check
version="$(python3 scripts/version.py show)"
if [ "$mode" = "real" ]; then
  tag="${TAG_NAME:?TAG_NAME is needed}"
  [[ "$tag" =~ ^v(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)$ ]] || die "not a version tag: $tag"
  test "$(git rev-parse HEAD)" = "$(git rev-parse "${GITHUB_SHA}^{commit}")" || die "Tag changed after the checkout"
else
  tag="v$version"
  test "$(git rev-parse HEAD)" = "$(git rev-parse "${GITHUB_SHA}^{commit}")" || die "the checkout is not the commit $GITHUB_SHA"
fi
git merge-base --is-ancestor "$GITHUB_SHA" origin/main || die "$GITHUB_SHA is not on main: releases are made from a tag on main only"
[ "$tag" = "v$version" ] || die "tag $tag does not match APP_VERSION $version"
scripts/pin_actions.sh --check
[ -z "${GITHUB_OUTPUT:-}" ] || echo "version=$version" >> "$GITHUB_OUTPUT"
# the version must be newer than the newest published release (drafts and pre-releases do not count)
latest="$(gh release list --repo "${GITHUB_REPOSITORY:?GITHUB_REPOSITORY is needed}" --exclude-drafts --exclude-pre-releases --limit 100 --json tagName --jq '.[].tagName' | sed -n 's/^v\([0-9][0-9]*\.[0-9][0-9]*\.[0-9][0-9]*\)$/\1/p' | sort -V | tail -1)"
if [ -z "$latest" ]; then echo "no published release yet: $version is the first"; exit 0; fi
[ "$(python3 scripts/version.py compare "$version" "$latest")" = 1 ] || die "$version is not newer than the published release $latest"
echo "$version is newer than the published release $latest"
