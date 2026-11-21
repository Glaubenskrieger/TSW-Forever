#!/usr/bin/env bash
# The release page of the pipeline: a DRAFT is filled with the release files, verified against the local files, and only after all checks (and the owner's approval at the
# environment `release-publish`) published. Used by the jobs `draft` and `publish` of .github/workflows/release.yml with the job token (contents: write). Never run by hand.
#
#   scripts/release_draft.sh upload  <tag> <notes-file> <dist-dir>   create the draft if it does not exist (a PUBLISHED release is refused), upload the four files,
#                                                                    download them again and compare byte for byte
#   scripts/release_draft.sh verify  <tag> <dist-dir>                the release is still a DRAFT and holds exactly the four files, byte-identical to <dist-dir>
#   scripts/release_draft.sh publish <tag> <dist-dir>                verify, then publish (draft=false), then check that it is public, not a pre-release and still identical
#
# <dist-dir> holds exactly: tsw-forever-<V>.flatpak, tsw-forever-<V>-src.tar.gz, tsw-forever-<V>-windows-setup.exe, SHA256SUMS (checked with sha256sum --check first); the tag
# must be v<V>. Nothing is ever deleted, replaced in a published release, or created as a tag (--verify-tag: the tag of the owner must exist). GH overrides the gh program (tests only).
# Exit codes: 0 ok, 1 a check failed, 2 wrong call or a missing tool.
set -euo pipefail
export LC_ALL=C
die() { echo "release_draft: $*" >&2; exit "${2:-1}"; }
gh_() { "${GH:-gh}" "$@"; }
cmd="${1:-}"; shift || true
tag="${1:-}"; [[ "$tag" =~ ^v(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)$ ]] || die "the tag must be vMAJOR.MINOR.PATCH" 2
version="${tag#v}"
for t in "${GH:-gh}" python3 sha256sum cmp; do command -v "$t" >/dev/null 2>&1 || die "missing tool $t" 2; done
names=("tsw-forever-$version.flatpak" "tsw-forever-$version-src.tar.gz" "tsw-forever-$version-windows-setup.exe" SHA256SUMS)

check_dist() { # <dir>: exactly the four files, checksums valid
  local d="$1" n
  [ -d "$d" ] || die "no such folder: $d" 2
  [ "$(find "$d" -maxdepth 1 -type f | wc -l)" -eq 4 ] || die "$d must hold exactly four files"
  for n in "${names[@]}"; do [ -s "$d/$n" ] || die "$d/$n is missing or empty"; done
  (cd "$d" && sha256sum --check --quiet SHA256SUMS) || die "SHA256SUMS does not match the files in $d"
}
state() { # prints "missing" or "draft"/"public" + "prerelease"/"final" and the asset names, one per line
  local js
  if ! js="$(gh_ release view "$tag" --json isDraft,isPrerelease,assets 2>/dev/null)"; then echo missing; return 0; fi
  python3 -I -c '
import json, sys
d = json.loads(sys.stdin.read())
print("draft" if d["isDraft"] else "public", "prerelease" if d["isPrerelease"] else "final")
for a in d["assets"]:
    print(a["name"])
' <<<"$js"
}
compare_remote() { # <dist-dir>: the remote assets are exactly the four files and identical to the local ones
  local d="$1" tmp n
  tmp="$(mktemp -d)"; trap 'rm -rf "${tmp:?}"' RETURN
  gh_ release download "$tag" --dir "$tmp" >/dev/null
  [ "$(find "$tmp" -maxdepth 1 -type f | wc -l)" -eq 4 ] || die "the release must hold exactly four files, found: $(ls "$tmp" | tr '\n' ' ')"
  for n in "${names[@]}"; do cmp -s "$d/$n" "$tmp/$n" || die "$n on the release differs from the verified file"; done
}

case "$cmd" in
  upload)
    notes="${2:?notes file}"; dist="${3:?dist folder}"
    [ -s "$notes" ] || die "the release notes are missing" 2
    check_dist "$dist"
    st="$(state)"; first="$(head -1 <<<"$st")"
    case "$first" in
      missing) gh_ release create "$tag" --draft --verify-tag --title "TSW-Forever $version" --notes-file "$notes" >/dev/null;;
      draft*) echo "continuing the existing draft of $tag";;
      public*) die "$tag is already PUBLISHED: this pipeline never touches a published release";;
      *) die "unexpected release state: $first";;
    esac
    # an earlier, partial attempt may have left a part of the files: same names are replaced, anything else is a hard stop (nothing is deleted)
    extra="$(tail -n +2 <<<"$(state)" | grep -vxF -f <(printf '%s\n' "${names[@]}") || true)"
    [ -z "$extra" ] || die "the draft holds files that do not belong to the release: $(tr '\n' ' ' <<<"$extra")"
    args=(); for n in "${names[@]}"; do args+=("$dist/$n"); done
    gh_ release upload "$tag" "${args[@]}" --clobber >/dev/null
    compare_remote "$dist"
    echo "draft $tag holds the four verified files";;
  verify|publish)
    dist="${2:?dist folder}"
    check_dist "$dist"
    st="$(state)"
    [ "$(head -1 <<<"$st" | cut -d' ' -f1)" = draft ] || die "$tag is not a draft (state: $(head -1 <<<"$st"))"
    [ "$(tail -n +2 <<<"$st" | sort | tr '\n' ' ')" = "$(printf '%s\n' "${names[@]}" | sort | tr '\n' ' ')" ] || die "the draft does not hold exactly the four release files"
    compare_remote "$dist"
    if [ "$cmd" = verify ]; then echo "draft $tag is intact"; exit 0; fi
    gh_ release edit "$tag" --draft=false >/dev/null
    after="$(state)"
    [ "$(head -1 <<<"$after")" = "public final" ] || die "after publishing, $tag is: $(head -1 <<<"$after")"
    compare_remote "$dist"
    echo "published $tag";;
  *) sed -n 2,14p "${BASH_SOURCE[0]}"; exit 2;;
esac
