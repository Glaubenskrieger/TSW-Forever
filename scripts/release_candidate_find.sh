#!/usr/bin/env bash
# Finds the build run (workflow build.yml) of EXACTLY the given commit and checks that its candidate artifacts are complete and not expired. Shared by the release workflow (job `candidate`)
# and the read-only release rehearsal. Reads only (`gh run list`, `gh api`). FAIL-CLOSED: every doubt stops with a message that says what is wrong and what to do; nothing is ever
# substituted:
#   * only runs whose head commit is exactly GITHUB_SHA count; a run of another commit is never used;
#   * the NEWEST run of that commit must be successful: if it failed, is running or was cancelled, an OLDER successful run of the same commit is NOT used instead;
#   * every one of the four artifacts (flatpak, source, windows, candidate) must exist and must not be expired (artifacts are kept 30 days in production, 7 days in development).
#
# Environment: GH_TOKEN (read), GITHUB_REPOSITORY, GITHUB_SHA, VERSION, REF_NAME (only for the message), GITHUB_OUTPUT (optional: `run_id=`).
set -euo pipefail
die() { echo "release candidate: $*" >&2; exit 1; }
: "${GITHUB_REPOSITORY:?}" "${GITHUB_SHA:?}" "${VERSION:?}"
[[ "$GITHUB_SHA" =~ ^[0-9a-f]{40}$ ]] || die "GITHUB_SHA is not a full commit id: $GITHUB_SHA"
ref="${REF_NAME:-<ref>}"
again="Run 'gh workflow run build.yml --ref $ref' (or push the commit to main) and wait for it to finish green; then start this workflow again. Nothing is built here, nothing is substituted and nothing was published."

runs="$(gh run list --repo "$GITHUB_REPOSITORY" --workflow build.yml --commit "$GITHUB_SHA" --limit 50 --json databaseId,headSha,conclusion,status,workflowName,createdAt)" \
  || die "cannot ask GitHub for the build runs of $GITHUB_SHA (no access or no connection). Nothing was checked."
latest="$(jq -c --arg sha "$GITHUB_SHA" '[.[] | select(.headSha == $sha and .workflowName == "build")] | sort_by(.createdAt) | last // empty' <<<"$runs")" \
  || die "the list of build runs could not be read."
[ -n "$latest" ] || die "there is no run of the workflow 'build' for exactly $GITHUB_SHA. $again"
run_id="$(jq -r '.databaseId' <<<"$latest")"
status="$(jq -r '.status // ""' <<<"$latest")"
conclusion="$(jq -r '.conclusion // ""' <<<"$latest")"
[ "$conclusion" = "success" ] || die "the newest build run of $GITHUB_SHA (run $run_id) is not successful (status: ${status:-unknown}, conclusion: ${conclusion:-none yet}). An older run of the same commit is not used instead. $again"

artifacts="$(gh api --paginate "repos/$GITHUB_REPOSITORY/actions/runs/$run_id/artifacts?per_page=100" --jq '.artifacts[] | "\(.name)\t\(.expired)"')" \
  || die "cannot list the artifacts of build run $run_id. Nothing was checked."
for kind in flatpak source windows candidate; do
  name="tsw-forever-${VERSION}-${kind}-${GITHUB_SHA}"
  line="$(grep -F -x -e "${name}"$'\t'"false" -e "${name}"$'\t'"true" <<<"$artifacts" || true)"
  [ -n "$line" ] || die "artifact $name of build run $run_id does not exist. $again"
  [ "$line" = "${name}"$'\t'"false" ] || die "artifact $name of build run $run_id has expired (artifacts are kept 30 days in production and 7 days in the development repository). $again"
done
echo "build run $run_id is the successful build of $GITHUB_SHA; its four artifacts exist and have not expired"
[ -z "${GITHUB_OUTPUT:-}" ] || echo "run_id=$run_id" >> "$GITHUB_OUTPUT"
