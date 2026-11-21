#!/bin/bash
# Proves that a SIGNED Flatpak/OSTree repository can really be UPDATED by a user's Flatpak, the way the update portal does it: over HTTP, with the commit signature
# and the summary signature checked, and over the STATIC DELTA path.
#
#   scripts/check_flatpak_delta_update.sh [--require-delta] [--extra-key <public-key.gpg>]... <repository> <public-key.gpg> [ref ...]
#
# Why this exists: `flatpak_keys.sh verify-repo` checks the commit signatures and the summary, but a static delta carries its OWN copy of the commit and of its
# signature (inside the delta's superblock). A delta that was generated BEFORE the commit was signed has no signature in it, so a client that takes the delta path
# stops with "GPG verification enabled, but no signatures found", although verify-repo is green. (Found when a signed test update failed at 0 %: the throw-away
# test repository had run `flatpak build-update-repo --generate-static-deltas` before the signing.) The release flow signs first (flatpak_keys.sh sign-repo builds
# the deltas afterwards); this check makes sure that stays true.
#
# What it does, for every application ref of the repository (or the refs given):
#   1. serves the repository from a throw-away local http server (127.0.0.1, a free port; nothing outside the temporary folder is touched);
#   2. a fresh scratch repository trusts ONLY the given public key (gpg-verify and gpg-verify-summary on) and must read the signed summary;
#   3. for EVERY static delta that targets the newest commit of the ref (from scratch, and from each older commit): a fresh scratch repository is seeded with the
#      old commit (nothing for "from scratch") and then pulls the ref. The pull must succeed, must report delta parts (the delta path, not the object fallback) and
#      the server must have been asked for a delta superblock; the resulting commit must be the newest commit.
# --require-delta: a ref without any delta that targets its newest commit is a failure (the release flow always generates them); without it that ref is only reported.
# Trust: ONLY the given public key is trusted. The older commits of a delta (the history the update starts from) must be verifiable with it too, so after a KEY ROTATION (history
# signed with the previous key, the newest commit with the new one) the check fails, loudly, until the previous public key is named EXPLICITLY with --extra-key (repeatable). An extra
# key is trusted for the older commits of the delta tests only: the summary and the newest commit are always verified with the given key ALONE, and every extra key is listed in the
# output. Key files must hold PUBLIC keys; a file with a secret key is refused (exit 2).
# Exit codes: 0 = everything verified, 1 = a check failed (unsigned or wrongly signed delta, summary, missing delta, ...), 2 = wrong call or a missing tool.
set -uo pipefail

require=0
args=()
extras=()
while [ $# -gt 0 ]; do
  case "$1" in
    --require-delta) require=1; shift;;
    --extra-key) [ $# -ge 2 ] || { echo "--extra-key needs a public key file" >&2; exit 2; }; extras+=("$2"); shift 2;;
    -h|--help) sed -n 2,30p "$0"; exit 0;;
    -*) echo "unknown option: $1" >&2; exit 2;;
    *) args+=("$1"); shift;;
  esac
done
[ "${#args[@]}" -ge 2 ] || { echo "usage: $0 [--require-delta] [--extra-key <public-key.gpg>]... <repository> <public-key.gpg> [ref ...]" >&2; exit 2; }
repo="$(realpath -- "${args[0]}")"; pub="$(realpath -- "${args[1]}")"; refs=("${args[@]:2}")
for t in ostree python3 timeout; do command -v "$t" >/dev/null || { echo "missing tool: $t" >&2; exit 2; }; done
[ -d "$repo/objects" ] || { echo "$repo is not an OSTree repository" >&2; exit 2; }
[ -s "$pub" ] || { echo "the public key file is empty or missing: $pub" >&2; exit 2; }
if [ "${#refs[@]}" -eq 0 ]; then
  while read -r r; do [ -n "$r" ] && refs+=("$r"); done < <(ostree refs --repo="$repo" | grep '^app/')
fi
[ "${#refs[@]}" -gt 0 ] || { echo "the repository has no application ref" >&2; exit 2; }

# key files: PUBLIC keys only (a file with a secret key is refused), listed with their fingerprints so that the trust is visible in the output
keyinfo() { # <file>: fingerprint of the first key (empty without gpg)
  command -v gpg >/dev/null 2>&1 && gpg --show-keys --with-colons "$1" 2>/dev/null | awk -F: '/^fpr/{print $10; exit}'
}
for kf in "$pub" "${extras[@]+"${extras[@]}"}"; do
  [ -s "$kf" ] || { echo "the key file is empty or missing: $kf" >&2; exit 2; }
  if command -v gpg >/dev/null 2>&1 && gpg --list-packets "$kf" 2>/dev/null | grep -q ':secret key packet'; then echo "$kf holds a SECRET key: only public keys are accepted" >&2; exit 2; fi
done
extras_abs=()
for kf in "${extras[@]+"${extras[@]}"}"; do extras_abs+=("$(realpath -- "$kf")"); done

tmp="$(mktemp -d)" || exit 2
srv=""
cleanup() { [ -n "$srv" ] && kill "$srv" 2>/dev/null; [ -n "$srv" ] && wait "$srv" 2>/dev/null; rm -rf "${tmp:?}"; }
trap cleanup EXIT
mkdir "$tmp/site" && ln -s "$repo" "$tmp/site/repo"
log="$tmp/http.log"
python3 -u -I -m http.server 0 --bind 127.0.0.1 --directory "$tmp/site" >"$log" 2>&1 &
srv=$!
port=""
for _ in $(seq 1 50); do
  port="$(sed -nE 's/.*port ([0-9]+).*/\1/p' "$log" | head -1)"
  [ -n "$port" ] && break
  kill -0 "$srv" 2>/dev/null || break
  sleep 0.1
done
[ -n "$port" ] || { echo "the local test server did not start" >&2; exit 2; }
url="http://127.0.0.1:$port/repo"

fails=0
fail() { echo "FAIL: $*"; fails=$((fails + 1)); }
n=0
S=""
scratch() { # <key file>: a fresh scratch repository that trusts only the keys in that file; its path is left in $S (no subshell: the counter must count)
  n=$((n + 1)); S="$tmp/s$n"
  ostree init --repo="$S" --mode=bare-user-only >/dev/null 2>&1 && ostree --repo="$S" remote add --gpg-import="$1" --set=gpg-verify-summary=true t "$url" >/dev/null 2>&1
}

# who is trusted: the given key for everything; extra keys (key rotation) only for the older commits of the delta tests
trusted="$pub"
echo "trusted key (summary, newest commit and history): ${pub} $(keyinfo "$pub")"
if [ "${#extras_abs[@]}" -gt 0 ]; then
  trusted="$tmp/trusted.gpg"
  cat "$pub" "${extras_abs[@]}" > "$trusted"
  for kf in "${extras_abs[@]}"; do echo "WARNING: also trusting, for OLDER commits only (explicit --extra-key): ${kf} $(keyinfo "$kf")"; done
fi

scratch "$pub" || { echo "cannot create a scratch repository" >&2; exit 2; }
s="$S"
if out="$(timeout 120 ostree --repo="$s" remote summary t 2>&1)"; then
  echo "ok: the summary is read and its signature is valid for the given key"
else
  fail "the summary could not be verified with the given public key: $(printf '%s' "$out" | tail -1 | cut -c1-200)"
fi

for ref in "${refs[@]}"; do
  head="$(ostree rev-parse --repo="$repo" "$ref" 2>/dev/null)" || { fail "$ref: no such ref in the repository"; continue; }
  froms=()
  found=0
  while read -r d; do
    [ -n "$d" ] || continue
    if [ "$d" = "$head" ]; then froms+=(""); found=$((found + 1)); elif [ "${d#*-}" = "$head" ] && [ "${d%%-*}" != "$d" ]; then froms+=("${d%%-*}"); found=$((found + 1)); fi
  done < <(ostree static-delta list --repo="$repo" 2>/dev/null)
  if [ "$found" -eq 0 ]; then
    if [ "$require" = 1 ]; then fail "$ref: no static delta targets the newest commit ${head:0:12} (a release must have them)"; else echo "note: $ref: no static delta targets ${head:0:12}; the delta path was not tested"; fi
    continue
  fi
  for from in "${froms[@]}"; do
    fromshort="${from:0:12}"; [ -n "$from" ] || fromshort="scratch"
    what="$ref: $fromshort -> ${head:0:12}"
    scratch "$trusted" || { echo "cannot create a scratch repository" >&2; exit 2; }
    s="$S"
    if [ -n "$from" ]; then
      if ! out="$(timeout 120 ostree --repo="$s" pull t "$ref@$from" 2>&1)"; then
        fail "$what: the older commit could not be fetched and verified first: $(printf '%s' "$out" | tail -1 | cut -c1-200)"
        continue
      fi
    fi
    mark="$(wc -l < "$log")"
    out="$(timeout 120 ostree --repo="$s" pull t "$ref" 2>&1)"; rc=$?
    asked="$(tail -n +$((mark + 1)) "$log" | grep -c '/superblock')"
    if [ "$rc" -ne 0 ]; then
      fail "$what: the update pull failed: $(printf '%s' "$out" | tail -1 | cut -c1-220)"
    elif ! printf '%s\n' "$out" | grep -Eq '(^|[ ,])[1-9][0-9]* delta parts'; then
      fail "$what: the pull did not use the delta ($(printf '%s' "$out" | tail -1 | cut -c1-120))"
    elif [ "$asked" -lt 1 ]; then
      fail "$what: the server was never asked for a delta superblock"
    elif [ "$(ostree rev-parse --repo="$s" "$ref" 2>/dev/null)" != "$head" ]; then
      fail "$what: the pulled commit is not the newest one"
    else
      echo "ok: $what: updated over the signed delta path (commit and summary signatures verified)"
    fi
  done
  if [ "${#extras_abs[@]}" -gt 0 ]; then # an extra key must never vouch for the NEW state: the newest commit has to verify with the given key alone
    scratch "$pub" || { echo "cannot create a scratch repository" >&2; exit 2; }
    if out="$(timeout 120 ostree --repo="$S" pull t "$ref" 2>&1)" && [ "$(ostree rev-parse --repo="$S" "$ref" 2>/dev/null)" = "$head" ]; then
      echo "ok: $ref: the newest commit ${head:0:12} verifies with the given key alone"
    else
      fail "$ref: the newest commit ${head:0:12} does not verify with the given key alone: $(printf '%s' "$out" | tail -1 | cut -c1-200)"
    fi
  fi
done

if [ "$fails" -gt 0 ]; then echo "FAILED: $fails check(s)"; exit 1; fi
echo "all checks passed"
