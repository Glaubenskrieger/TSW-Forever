#!/usr/bin/env bash
# Checks the LIVE Flatpak update source of the project (after the Pages deployment, before the release is published): that every needed file is reachable and that the signed
# repository really verifies the way a user's Flatpak verifies it. Nothing is written outside a temporary folder; nothing is changed anywhere.
#
#   scripts/check_flatpak_live.sh --site <https://host/path> --pub <public-key.gpg> --expect-commit <commit> [--app-id ID] [--expect-repo-url URL]
#                                 [--retries N] [--interval SECONDS] [--allow-loopback-http]
#
# One round checks:  (1) the web site's start page answers,  (2) repo/summary and repo/summary.sig answer,  (3) tsw-forever.flatpakrepo and <app-id>.flatpakref answer, name the
# expected repository address and carry exactly the project's public key,  (4) a scratch repository that trusts ONLY the public key and requires a valid commit AND summary signature
# can read the summary and pull the app ref, and the pulled commit is the expected one.  A round that fails is repeated (Pages needs minutes to become reachable) up to --retries
# times, --interval seconds apart (defaults 30 and 20); the last failures are printed. The expected repository address defaults to FLATPAK_REPO_URL of packaging/project.conf and
# the app id to APP_ID there.
# --site must be https. --allow-loopback-http permits http for 127.0.0.1 only (the automated tests); it is never used by the pipeline.
# Exit codes: 0 = everything reachable and verified, 1 = a check failed after all rounds, 2 = wrong call or missing tool.
set -uo pipefail
export LC_ALL=C
root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
site=""; pub=""; expect=""; app=""; exprepo=""; retries=30; interval=20; loop=0
while [ $# -gt 0 ]; do
  case "$1" in
    --site) site="${2:-}"; shift 2;; --pub) pub="${2:-}"; shift 2;; --expect-commit) expect="${2:-}"; shift 2;; --app-id) app="${2:-}"; shift 2;;
    --expect-repo-url) exprepo="${2:-}"; shift 2;; --retries) retries="${2:-}"; shift 2;; --interval) interval="${2:-}"; shift 2;;
    --allow-loopback-http) loop=1; shift;; -h|--help) sed -n 2,19p "$0"; exit 0;;
    *) echo "unknown option: $1" >&2; exit 2;;
  esac
done
# shellcheck disable=SC1091
[ -n "$app" ] || app="$(. "$root/packaging/project.conf"; printf '%s' "${APP_ID:-}")"
[ -n "$exprepo" ] || exprepo="$(. "$root/packaging/project.conf"; printf '%s' "${FLATPAK_REPO_URL:-}")"
site="${site%/}"
[ -n "$site" ] && [ -n "$pub" ] && [ -n "$expect" ] && [ -n "$app" ] && [ -n "$exprepo" ] || { echo "usage: $0 --site <https url> --pub <public key> --expect-commit <commit>" >&2; exit 2; }
[[ "$expect" =~ ^[0-9a-f]{64}$ ]] || { echo "--expect-commit must be the 64 hex digit OSTree commit" >&2; exit 2; }
[[ "$retries" =~ ^[0-9]+$ && "$interval" =~ ^[0-9]+$ && "$retries" -ge 1 ]] || { echo "--retries and --interval must be numbers" >&2; exit 2; }
case "$site" in
  https://*) ;;
  http://127.0.0.1:*|http://127.0.0.1/*) [ "$loop" = 1 ] || { echo "--site must be https" >&2; exit 2; };;
  *) echo "--site must be https" >&2; exit 2;;
esac
case "$exprepo" in https://*) ;; *) echo "the expected repository address must be https" >&2; exit 2;; esac
[ -s "$pub" ] || { echo "the public key file is missing or empty: $pub" >&2; exit 2; }
for t in curl ostree gpg base64; do command -v "$t" >/dev/null 2>&1 || { echo "missing tool: $t" >&2; exit 2; }; done
if gpg --list-packets "$pub" 2>/dev/null | grep -q ':secret key packet'; then echo "$pub holds a SECRET key" >&2; exit 2; fi
ref="app/$app/x86_64/stable"
tmp="$(mktemp -d)"; trap 'rm -rf "${tmp:?}"' EXIT
fprs() { gpg --show-keys --with-colons "$1" 2>/dev/null | awk -F: '$1=="fpr"{print toupper($10)}' | sort | tr '\n' ' '; }
want_fprs="$(fprs "$pub")"
[ -n "$want_fprs" ] || { echo "the public key file holds no key: $pub" >&2; exit 2; }
fails=()
fail() { fails+=("$*"); }
get() { # <relative path> <output file>; 200 only, redirects stay on https (or loopback http for the tests)
  curl --proto '=https,http' --max-redirs 3 -fsS --max-time 60 -o "$2" "$site/$1" 2>/dev/null
}
keyfrom() { # <install file>: the GPGKey fingerprints inside
  local b; b="$(sed -n 's/^GPGKey=//p' "$1" | head -1)"; [ -n "$b" ] || return 1
  printf '%s' "$b" | base64 -d 2>/dev/null > "$tmp/k.gpg" || return 1
  fprs "$tmp/k.gpg"
}
round() {
  fails=()
  local f
  get "" "$tmp/index.html" && [ -s "$tmp/index.html" ] || fail "the web site start page is not reachable"
  for f in summary summary.sig; do get "repo/$f" "$tmp/$f" && [ -s "$tmp/$f" ] || fail "repo/$f is not reachable"; done
  get tsw-forever.flatpakrepo "$tmp/r.flatpakrepo" || fail "tsw-forever.flatpakrepo is not reachable"
  get "$app.flatpakref" "$tmp/r.flatpakref" || fail "$app.flatpakref is not reachable"
  if [ -s "$tmp/r.flatpakrepo" ]; then
    grep -qx "Url=$exprepo" "$tmp/r.flatpakrepo" || fail "tsw-forever.flatpakrepo does not name $exprepo"
    [ "$(keyfrom "$tmp/r.flatpakrepo")" = "$want_fprs" ] || fail "tsw-forever.flatpakrepo does not carry exactly the project's public key"
  fi
  if [ -s "$tmp/r.flatpakref" ]; then
    grep -qx "Url=$exprepo" "$tmp/r.flatpakref" || fail "$app.flatpakref does not name $exprepo"
    grep -qx "Name=$app" "$tmp/r.flatpakref" || fail "$app.flatpakref does not name the application $app"
    grep -qx "Branch=stable" "$tmp/r.flatpakref" || fail "$app.flatpakref is not for the stable branch"
    grep -qx "IsRuntime=false" "$tmp/r.flatpakref" || fail "$app.flatpakref is not an application ref"
    [ "$(keyfrom "$tmp/r.flatpakref")" = "$want_fprs" ] || fail "$app.flatpakref does not carry exactly the project's public key"
  fi
  # the client view: only the public key is trusted, summary AND commit signatures are required
  local s="$tmp/client"; rm -rf "$s"
  ostree init --repo="$s" --mode=bare-user-only >/dev/null 2>&1
  ostree --repo="$s" remote add --gpg-import="$pub" --set=gpg-verify-summary=true live "$site/repo" >/dev/null 2>&1
  local out
  if out="$(timeout 180 ostree --repo="$s" remote summary live 2>&1)"; then
    if out="$(timeout 300 ostree --repo="$s" pull live "$ref" 2>&1)"; then
      local got; got="$(ostree rev-parse --repo="$s" "$ref" 2>/dev/null)"
      [ "$got" = "$expect" ] || fail "the live repository serves commit ${got:0:12}, expected ${expect:0:12}"
    else fail "pulling $ref with signature checks failed: $(printf '%s' "$out" | tail -1 | cut -c1-200)"; fi
  else fail "the live summary does not verify against the project's public key: $(printf '%s' "$out" | tail -1 | cut -c1-200)"; fi
  [ "${#fails[@]}" -eq 0 ]
}
n=0
while :; do
  n=$((n + 1))
  if round; then echo "live source verified (round $n): site, summary, install files and $ref ${expect:0:12} with valid signatures"; exit 0; fi
  [ "$n" -ge "$retries" ] && break
  sleep "$interval"
done
echo "FAILED after $n round(s):" >&2
printf '  - %s\n' "${fails[@]}" >&2
exit 1
