#!/usr/bin/env bash
# Pin third-party GitHub Actions to commit SHAs.
#   scripts/pin_actions.sh --check                 fail if any `uses:` in .github/workflows is not a 40-hex commit SHA (offline)
#   scripts/pin_actions.sh --resolve --allow-network   print `uses: owner/repo@<sha> # <tag>` lines for the tags in use (GitHub REST API via curl, read-only; no git)
# The release workflow runs --check first, so a release cannot happen with tag-only pins. Resolving needs the network (not done by agents
# without approval); review the printed SHAs against the action's release page before replacing them.
set -euo pipefail
root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
mode="${1:---check}"
uses() { grep -hoE 'uses:[[:space:]]*[^[:space:]]+' "$root"/.github/workflows/*.yml | sed -E 's/uses:[[:space:]]*//' | sort -u; }
case "$mode" in
  --check)
    bad=0
    while read -r u; do
      case "$u" in ./*) continue;; esac
      if ! printf '%s' "$u" | grep -qE '@[0-9a-f]{40}$'; then echo "not SHA-pinned: $u" >&2; bad=1; fi
    done < <(uses)
    [ "$bad" = 0 ] && echo "all actions are pinned to commit SHAs"
    exit "$bad";;
  --resolve)
    [ "${2:-}" = "--allow-network" ] || { echo "pin_actions: --resolve reads github.com; pass --allow-network" >&2; exit 13; }
    while read -r u; do
      repo="${u%@*}"; tag="${u#*@}"
      sha="$(curl -fsSL -H 'Accept: application/vnd.github+json' "https://api.github.com/repos/$repo/commits/$tag" | sed -n 's/^  "sha": "\([0-9a-f]\{40\}\)".*/\1/p' | head -n1)"
      echo "uses: $repo@${sha:-UNRESOLVED} # $tag"
    done < <(uses | grep -vE '^\./|@[0-9a-f]{40}$');;
  *) sed -n '2,7p' "${BASH_SOURCE[0]}"; exit 2;;
esac
