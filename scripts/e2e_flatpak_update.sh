#!/usr/bin/env bash
# End-to-end test of the Flatpak update CHAIN with TEMPORARY keys and a LOCAL repository. Linux only (needs flatpak, ostree, gpg, python3, curl).
#
#   scripts/e2e_flatpak_update.sh --self-check     only the isolation checks (no keys, no repository, no server, no install): run this first
#   scripts/e2e_flatpak_update.sh [--keep]         the whole test (--keep keeps the temporary folder, which then holds temporary private TEST keys)
#
# What it proves, with the real Flatpak and OSTree doing the security work (this script only drives them and our own tools):
#   S0  our tool `flatpak_keys.sh verify-repo` accepts a repository signed with the right key and refuses an unsigned one and one signed with another key
#   S1  install from a repository signed with key A (the client trusts A only)
#   S2  update to a new version (signed, with static deltas, the same signing path as a release: `flatpak_keys.sh sign-repo`)
#   S3  manipulated updates are refused and the installed version stays: unsigned, signed with a foreign key, a corrupted object
#   S4  a replayed OLD signed repository state is not taken as an update (rollback); the AUTHORISED restore of the previous version (`--commit`) works
#   S5  key rotation and revocation: dual-signed transition, rotation, a client that never got the new key, and a compromised old key after revocation
#
# Isolation (checked by `--self-check`, and re-checked at the end of every run):
#   * every Flatpak call of this script runs inside a throw-away environment: HOME and the XDG folders point into the temporary folder, FLATPAK_USER_DIR, FLATPAK_SYSTEM_DIR and
#     FLATPAK_CONFIG_DIR point into it as well, and every Flatpak command that touches an installation is a `--user` command of ITS OWN throw-away installation (one per test client).
#     So the real user installation, the system installation, their remotes and the real GPG home are neither read nor written by the test.
#   * before the test the real installations are only LISTED (read-only) and after the test they are listed again and compared; any difference is reported as a failure.
#   * every key is created in a throw-away GNUPGHOME below the temporary folder (GNUPGHOME of the test is set there as well, so a stray gpg call cannot reach ~/.gnupg);
#     the repository is a folder in the temporary folder, served on 127.0.0.1 only by a Python server that is stopped at the end;
#   * at the end (also after an error, Ctrl-C or SIGTERM) the server and the per-keyring gpg-agents are stopped and the whole temporary folder is deleted. Only SIGKILL or a power
#     loss can leave a folder /tmp/tsw-e2e-XXXXXX behind; it holds only temporary test data and is safe to delete.
# No production key is created or read, nothing is published, no network is used (127.0.0.1 only), no `sudo`, no polkit prompt (user installations only).
# Exit code 0 = all checks passed, 1 = a check failed, 2 = usage or root, 10 = a tool is missing.
#
# S4's replay check depends on Flatpak's own downgrade protection. It is reported as a hard failure only with E2E_STRICT_ROLLBACK=1 (default: reported loudly, not failing),
# because the behaviour belongs to the installed Flatpak version and is exactly what this test is meant to find out on the target systems (SteamOS, distributions).
set -euo pipefail
root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
keys="$root/scripts/flatpak_keys.sh"
keep=0; selfcheck=0
for a in "$@"; do
  case "$a" in
    --keep) keep=1;;
    --self-check) selfcheck=1;;
    *) echo "usage: $0 [--self-check] [--keep]" >&2; exit 2;;
  esac
done
[ "$(id -u)" != 0 ] || { echo "e2e: do not run this as root" >&2; exit 2; }
tools="flatpak ostree gpg gpgconf python3 curl"; [ "$selfcheck" = 1 ] && tools="flatpak gpgconf"
for t in $tools; do
  command -v "$t" >/dev/null || { echo "e2e: missing tool '$t'. On SteamOS do not change the read-only system for this; run the test on another Linux machine or in CI." >&2; exit 10; }
done

APP="org.tswlauncher.E2eTest"
BRANCH="stable"
ARCH="$(flatpak --default-arch)"
REF="app/$APP/$ARCH/$BRANCH"
pass=0; fail=0; info=0
server_pid=""; verified=0

# the environment of the user, kept to LOOK (read-only) at the real installations before and after
ORIG_HOME="$HOME"; ORIG_FUD="${FLATPAK_USER_DIR-}"; ORIG_FSD="${FLATPAK_SYSTEM_DIR-}"; ORIG_FCD="${FLATPAK_CONFIG_DIR-}"; ORIG_XDG="${XDG_DATA_HOME-}"
real_state() {
  (
    export HOME="$ORIG_HOME"
    unset FLATPAK_USER_DIR FLATPAK_SYSTEM_DIR FLATPAK_CONFIG_DIR XDG_DATA_HOME
    if [ -n "$ORIG_FUD" ]; then export FLATPAK_USER_DIR="$ORIG_FUD"; fi
    if [ -n "$ORIG_FSD" ]; then export FLATPAK_SYSTEM_DIR="$ORIG_FSD"; fi
    if [ -n "$ORIG_FCD" ]; then export FLATPAK_CONFIG_DIR="$ORIG_FCD"; fi
    if [ -n "$ORIG_XDG" ]; then export XDG_DATA_HOME="$ORIG_XDG"; fi
    echo "user remotes:";   flatpak --user remotes --columns=name,url 2>&1 || true
    echo "user apps:";      flatpak --user list --columns=ref,commit 2>&1 || true
    echo "system remotes:"; flatpak --system remotes --columns=name,url 2>&1 || true
    echo "system apps:";    flatpak --system list --columns=ref,commit 2>&1 || true
    d="${FLATPAK_USER_DIR:-${XDG_DATA_HOME:-$HOME/.local/share}/flatpak}"
    echo "user installation folder: $d"
    if [ -f "$d/repo/config" ]; then echo "repo config:"; sha256sum "$d/repo/config"; fi
    if [ -d "$d/app" ]; then echo "app folders:"; ls -1 "$d/app"; fi
  )
}

work="$(mktemp -d "${TMPDIR:-/tmp}/tsw-e2e-XXXXXX")"
case "$work" in */tsw-e2e-??????) ;; *) echo "e2e: unexpected temporary folder $work" >&2; exit 1;; esac
# an early, minimal cleanup so that even a failure of the next lines leaves nothing behind (replaced by the full cleanup below)
trap 'chmod -R u+rwX "$work" 2>/dev/null || true; rm -rf "${work:?}"' EXIT
real_state >"$work/real-before.txt" 2>&1

# from here on EVERYTHING runs in the throw-away environment
mkdir -p "$work/home/data" "$work/home/config" "$work/home/cache" "$work/home/state" "$work/guard-user" "$work/guard-system" "$work/guard-config" "$work/gpg-guard"
chmod 700 "$work/gpg-guard"
export HOME="$work/home" XDG_DATA_HOME="$work/home/data" XDG_CONFIG_HOME="$work/home/config" XDG_CACHE_HOME="$work/home/cache" XDG_STATE_HOME="$work/home/state"
export FLATPAK_USER_DIR="$work/guard-user" FLATPAK_SYSTEM_DIR="$work/guard-system" FLATPAK_CONFIG_DIR="$work/guard-config" GNUPGHOME="$work/gpg-guard"

say()  { printf '\n== %s\n' "$*"; }
ok()   { pass=$((pass+1)); printf '   ok   %s\n' "$*"; }
bad()  { fail=$((fail+1)); printf '   FAIL %s\n' "$*"; }
note() { info=$((info+1)); printf '   NOTE %s\n' "$*"; }
# check "description" command...   (passes when the command succeeds)
check() { local d="$1"; shift; if "$@" >"$work/last.out" 2>&1; then ok "$d"; else bad "$d"; sed 's/^/        /' "$work/last.out" | tail -8; fi; }
# refuse "description" command...  (passes when the command FAILS)
refuse() { local d="$1"; shift; if "$@" >"$work/last.out" 2>&1; then bad "$d (it succeeded, it must not)"; sed 's/^/        /' "$work/last.out" | tail -5; else ok "$d"; fi; }

verify_untouched() {
  verified=1
  real_state >"$work/real-after.txt" 2>&1
  if cmp -s "$work/real-before.txt" "$work/real-after.txt"; then ok "the real Flatpak installations (user and system), their remotes and applications are unchanged"
  else bad "the real Flatpak state CHANGED during the test"; diff "$work/real-before.txt" "$work/real-after.txt" | sed 's/^/        /' | head -20; fi
}

cleanup() {
  local rc=$?
  trap - EXIT INT TERM HUP
  if [ -n "$server_pid" ]; then kill "$server_pid" 2>/dev/null || true; wait "$server_pid" 2>/dev/null || true; fi
  for h in "$work"/gpg-*; do if [ -d "$h" ]; then gpgconf --homedir "$h" --kill all 2>/dev/null || true; fi; done
  if [ "$verified" = 0 ] && [ -f "$work/real-before.txt" ]; then
    verify_untouched
    if [ "$fail" -gt 0 ] && [ "$rc" = 0 ]; then rc=1; fi
  fi
  if [ "$keep" = 1 ]; then
    echo "kept: $work  (it contains temporary private TEST keys; delete it yourself: rm -rf '$work')" >&2
  else
    chmod -R u+rwX "$work" 2>/dev/null || true
    rm -rf "${work:?}"
    if [ -e "$work" ]; then echo "WARNING: $work could not be removed completely; it only holds temporary test data and can be deleted" >&2
    else echo "cleaned up: the temporary folder, keys, repository, server and throw-away installations are gone"; fi
  fi
  exit "$rc"
}
trap cleanup EXIT
trap 'exit 130' INT
trap 'exit 143' TERM
trap 'exit 129' HUP

if [ "$selfcheck" = 1 ]; then
  say "isolation self-check (flatpak $(flatpak --version | awk '{print $2}'))"
  check "HOME and the XDG data folder are inside the temporary folder" test "$HOME" = "$work/home" -a "$XDG_DATA_HOME" = "$work/home/data"
  check "FLATPAK_USER_DIR, FLATPAK_SYSTEM_DIR and FLATPAK_CONFIG_DIR are inside the temporary folder" test "${FLATPAK_USER_DIR#"$work"/}" != "$FLATPAK_USER_DIR" -a "${FLATPAK_SYSTEM_DIR#"$work"/}" != "$FLATPAK_SYSTEM_DIR" -a "${FLATPAK_CONFIG_DIR#"$work"/}" != "$FLATPAK_CONFIG_DIR"
  check "GNUPGHOME is inside the temporary folder (not ~/.gnupg)" test "$(gpgconf --list-dirs homedir 2>/dev/null || echo "$GNUPGHOME")" = "$work/gpg-guard"
  out="$(FLATPAK_USER_DIR="$work/client-canary" flatpak --user remotes 2>&1 || true)"
  check "a throw-away user installation starts with no remote (nothing of the real one is visible)" test -z "$(printf '%s' "$out" | grep -v '^$' || true)"
  out="$(FLATPAK_USER_DIR="$work/client-canary" flatpak --user list 2>&1 || true)"
  check "and no installed application" test -z "$(printf '%s' "$out" | grep -v -i 'nothing\|^$' || true)"
  if [ -d "$work/client-canary" ]; then ok "Flatpak used the throw-away folder $work/client-canary"; else note "Flatpak created no folder for the empty installation (fine)"; fi
  verify_untouched
  printf '\n   %d passed, %d failed, %d note(s)\n' "$pass" "$fail" "$info"
  [ "$fail" -eq 0 ]
  exit $?
fi

# ---- the clients: each has its own throw-away Flatpak user installation ------------------------------------------------------------------
fp() { local client="$1"; shift; FLATPAK_USER_DIR="$work/client-$client" flatpak --user "$@"; }
installed_version() { cat "$work/client-$1/app/$APP/$ARCH/$BRANCH/active/files/VERSION" 2>/dev/null || echo "none"; }
installed_commit() { fp "$1" info --show-commit "$APP" 2>/dev/null || echo "none"; }
# update_blocked client version "description": the update may fail or do nothing, but the installed version MUST stay (the state is what counts)
update_blocked() {
  local c="$1" want="$2" d="$3" st=0 got
  fp "$c" update --noninteractive --no-deps --no-related "$APP" >"$work/last.out" 2>&1 || st=$?
  got="$(installed_version "$c")"
  if [ "$got" = "$want" ]; then ok "$d (installed version stays $want; update exit status $st)"; else bad "$d: the client moved to version $got"; sed 's/^/        /' "$work/last.out" | tail -5; fi
}
expect_version() { local c="$1" want="$2" got; got="$(installed_version "$c")"; if [ "$got" = "$want" ]; then ok "client $c has version $want"; else bad "client $c has version $got, expected $want"; fi; }

# ---- keys (temporary) ----------------------------------------------------------------------------------------------------------------
newsplit() { # name -> like newkey, but the PRODUCTION model: a certify-only primary key and a separate signing subkey (flatpak_keys.sh generate-split)
  local n="$1" home="$work/gpg-$1"
  local fpr; fpr="$(TSW_TEST_KEY_ALGO="${E2E_KEY_ALGO:-ed25519}" "$keys" generate-split "$home" "TSW E2E split test key $n (temporary)" 2d 2d 2>/dev/null | tail -1)"
  [ -n "$fpr" ] || { echo "e2e: split key generation failed" >&2; exit 1; }
  "$keys" export-public "$home" "$fpr" "$work/pub-$n.gpg" >/dev/null
  printf -v "${n}_fpr" '%s' "$fpr"; printf -v "${n}_home" '%s' "$home"; printf -v "${n}_pub" '%s' "$work/pub-$n.gpg"
}
say "temporary keys"
E2E_KEY_ALGO=rsa4096 newsplit A   # the production algorithm (RSA 4096) for the key that signs through the pipeline's script
E2E_KEY_ALGO=ed25519 newsplit B
check "key A and key B exist and differ" test "$A_fpr" != "$B_fpr"
# a keyring that holds BOTH secret keys, for the dual-signed transition (still temporary, inside the work folder)
mkdir -m 700 "$work/gpg-AB"
GNUPGHOME="$A_home" gpg --batch --export-secret-keys "$A_fpr" | GNUPGHOME="$work/gpg-AB" gpg --batch --import 2>/dev/null
GNUPGHOME="$B_home" gpg --batch --export-secret-keys "$B_fpr" | GNUPGHOME="$work/gpg-AB" gpg --batch --import 2>/dev/null

# ---- the test application and the repository -----------------------------------------------------------------------------------------
mkapp() { # dir version
  rm -rf "$1"; mkdir -p "$1/files/bin" "$1/export/share/applications"
  printf '[Application]\nname=%s\nruntime=org.freedesktop.Platform/%s/25.08\nsdk=org.freedesktop.Sdk/%s/25.08\ncommand=tsw-e2e\n' "$APP" "$ARCH" "$ARCH" >"$1/metadata"
  printf '#!/bin/sh\necho %s\n' "$2" >"$1/files/bin/tsw-e2e"; chmod +x "$1/files/bin/tsw-e2e"
  printf '%s\n' "$2" >"$1/files/VERSION"
  printf '[Desktop Entry]\nType=Application\nName=TSW E2E test\nExec=tsw-e2e\n' >"$1/export/share/applications/$APP.desktop"
}
export_version() { # repo version  (explicit, strictly increasing commit times, like real releases)
  local repo="$1" v="$2"
  mkapp "$work/app-$v" "$v"
  flatpak build-export --arch="$ARCH" --timestamp="2026-01-0${v}T00:00:00+00:00" --subject="E2E version $v" "$repo" "$work/app-$v" "$BRANCH" >/dev/null
}
head_of() { ostree --repo="$1" rev-parse "$REF"; }
# our own, production signing path (single key)
sign_with_tool() { "$keys" sign-repo "$1" "$2" "$3" >/dev/null; }
# the pipeline's signing step: ONLY the signing subkey (no primary secret key) from the environment, scripts/flatpak_sign_ci.sh signs, verifies and proves the update over the signed deltas
sign_with_ci() { # <home of the split key> <primary fingerprint> <public key file> <repository>
  local sub="$work/ci-subkey-$2.b64" subid
  "$keys" export-ci-subkey "$1" "$2" "$sub" >/dev/null || return 1
  subid="$("$keys" fingerprints "$3" | awk '$1=="subkey"{print substr($2, length($2)-15)}')"
  TSW_FLATPAK_SIGNING_KEY="$(cat "$sub")" TSW_FLATPAK_SIGNING_FINGERPRINT="$2" TSW_FLATPAK_SIGNING_SUBKEY_ID="$subid" TSW_FLATPAK_SIGNING_PASSPHRASE="" TSW_FLATPAK_SIGNING_PIN_FILE=/dev/null \
    "$root/scripts/flatpak_sign_ci.sh" "$4" "$3" >"$work/ci-sign.out" 2>&1; local rc=$?
  rm -f "$sub"; return $rc
}
# signing with one or more keys of one keyring, WITHOUT static deltas (so that a corrupted object is really the one that gets pulled)
sign_plain() { # home repo key...
  local home="$1" repo="$2"; shift 2
  local args=(); local k; for k in "$@"; do args+=("--gpg-sign=$k"); done
  flatpak build-sign "${args[@]}" --gpg-homedir="$home" "$repo" "$APP" "$BRANCH" >/dev/null
  flatpak build-update-repo "${args[@]}" --gpg-homedir="$home" --prune --prune-depth=2 "$repo" >/dev/null
}
sign_dual() { # repo : keys A and B
  local repo="$1"
  flatpak build-sign --gpg-sign="$A_fpr" --gpg-sign="$B_fpr" --gpg-homedir="$work/gpg-AB" "$repo" "$APP" "$BRANCH" >/dev/null
  flatpak build-update-repo --gpg-sign="$A_fpr" --gpg-sign="$B_fpr" --gpg-homedir="$work/gpg-AB" --generate-static-deltas --prune --prune-depth=2 "$repo" >/dev/null
}

good="$work/good"; www="$work/www"; mkdir -p "$www"
say "S0 repository tools: the signature check of our release tooling"
export_version "$good" 1
unsigned="$work/unsigned"; cp -a "$good" "$unsigned"; flatpak build-update-repo "$unsigned" >/dev/null
refuse "verify-repo refuses an UNSIGNED repository" "$keys" verify-repo "$A_pub" "$unsigned"
sign_with_tool "$A_home" "$A_fpr" "$good"
check  "verify-repo accepts the repository signed with key A" "$keys" verify-repo "$A_pub" "$good"
refuse "verify-repo refuses the same repository for key B" "$keys" verify-repo "$B_pub" "$good"
c1="$(head_of "$good")"; cp -a "$good" "$work/state-v1"      # the signed state of version 1, kept for the replay test

# ---- the local server: a symlink decides which repository state is served ---------------------------------------------------------------
port="$(python3 -c 'import socket;s=socket.socket();s.bind(("127.0.0.1",0));print(s.getsockname()[1])')"
serve() { ln -sfn "$1" "$www/repo"; }
serve "$good"
python3 -m http.server --bind 127.0.0.1 --directory "$www" "$port" >"$work/server.log" 2>&1 &
server_pid=$!
for _ in $(seq 1 50); do curl -fs "http://127.0.0.1:$port/repo/config" >/dev/null 2>&1 && break; sleep 0.2; done
URL="http://127.0.0.1:$port/repo"
newclient() { # name keyfile...
  local c="$1"; shift; local imports=(); local f; for f in "$@"; do imports+=("--gpg-import=$f"); done
  mkdir -p "$work/client-$c"
  fp "$c" remote-add --no-enumerate "${imports[@]}" e2e "$URL"
}

say "S1 install from the signed repository (client X trusts key A only)"
check "a new throw-away installation has no remote and no application (nothing of the real one is visible)" test -z "$(fp X remotes 2>/dev/null)$(fp X list --columns=application 2>/dev/null)"
newclient X "$A_pub"
check "the remote lives in the throw-away installation only (its own repo/config below the temporary folder)" grep -q 'remote "e2e"' "$work/client-X/repo/config"
check "install version 1 from the signed repository" fp X install --noninteractive --no-deps --no-related e2e "$APP//$BRANCH"
expect_version X 1
check "the installed commit is the published one" test "$(installed_commit X)" = "$c1"

say "S1b install from the generated install files (the .flatpakref and .flatpakrepo files that the web site offers)"
# The files come from scripts/gen_flatpak_refs.sh with the public key, exactly as the publish workflow makes them. The test changes ONLY the address (to the local server),
# the application name (to the test application) and drops the runtime source (no network here); the key, the branch and the remote name stay as generated.
refs="$work/refs"; mkdir -p "$refs"
check "gen_flatpak_refs writes the install files from the public key" "$root/scripts/gen_flatpak_refs.sh" "$refs" "$A_pub"
sed -e "s#^Url=.*#Url=$URL#" -e "s#^Name=.*#Name=$APP#" -e '/^RuntimeRepo=/d' "$refs/org.tswlauncher.Launcher.flatpakref" > "$refs/local.flatpakref"
sed -e "s#^Url=.*#Url=$URL#" "$refs/tsw-forever.flatpakrepo" > "$refs/local.flatpakrepo"
check "the .flatpakref names the stable branch and the generated remote name" grep -q '^Branch=stable$' "$refs/local.flatpakref"
check "the .flatpakref and the .flatpakrepo embed the public key" sh -c "grep -q '^GPGKey=.\\{20,\\}' '$refs/local.flatpakref' && grep -q '^GPGKey=.\\{20,\\}' '$refs/local.flatpakrepo'"
mkdir -p "$work/client-Z"
check "the .flatpakref installs the application AND sets up the signed remote in one step" fp Z install --noninteractive --no-deps --no-related --from "$refs/local.flatpakref"
expect_version Z 1
check "the remote it created is the suggested one (tsw-forever) and points at the repository" sh -c "FLATPAK_USER_DIR='$work/client-Z' flatpak --user remotes --columns=name,url | grep -q '^tsw-forever.*$URL'"
check "the installed commit is the published one" test "$(installed_commit Z)" = "$c1"
mkdir -p "$work/client-V"
check "the .flatpakrepo only adds the signed source (nothing is installed by it)" fp V remote-add --from tsw-forever "$refs/local.flatpakrepo"
check "after the .flatpakrepo the application can be installed from that source" fp V install --noninteractive --no-deps --no-related tsw-forever "$APP//$BRANCH"
expect_version V 1
# an install file carrying the WRONG key must not install the repository signed with key A
"$root/scripts/gen_flatpak_refs.sh" "$work/refs-wrong" "$B_pub" >/dev/null
sed -e "s#^Url=.*#Url=$URL#" -e "s#^Name=.*#Name=$APP#" -e '/^RuntimeRepo=/d' "$work/refs-wrong/org.tswlauncher.Launcher.flatpakref" > "$work/refs-wrong/local.flatpakref"
mkdir -p "$work/client-W"
refuse "an install file that carries a different key refuses the repository signed with key A" fp W install --noninteractive --no-deps --no-related --from "$work/refs-wrong/local.flatpakref"
check "... and nothing was installed by it" test "$(installed_version W)" = none

say "S2 update to version 2 (signed with our release signing path, with static deltas)"
export_version "$good" 2
# version 2 is signed the way the release pipeline signs: the signing SUBKEY only (key A is a split key), through scripts/flatpak_sign_ci.sh
check "version 2 is signed through the pipeline's signing step with the subkey only (the primary secret key is not used)" sign_with_ci "$A_home" "$A_fpr" "$A_pub" "$good"
check "the pipeline's signing step reports a verified repository and no key material" sh -c "grep -q 'signed and verified' '$work/ci-sign.out' && ! grep -q 'PRIVATE KEY' '$work/ci-sign.out'"
c2="$(head_of "$good")"
check "update" fp X update --noninteractive --no-deps --no-related "$APP"
expect_version X 2
check "the installed commit is the new one" test "$(installed_commit X)" = "$c2"

say "S3 manipulated updates are refused, the installed version stays"
# 3a unsigned
atk="$work/atk-unsigned"; cp -a "$good" "$atk"; export_version "$atk" 3; flatpak build-update-repo "$atk" >/dev/null
serve "$atk"; update_blocked X 2 "an UNSIGNED new version is refused"
# 3b signed with a foreign key
atk="$work/atk-foreign"; cp -a "$good" "$atk"; export_version "$atk" 3; sign_plain "$B_home" "$atk" "$B_fpr"
serve "$atk"; update_blocked X 2 "a new version signed with a FOREIGN key is refused"
# 3c validly signed, but an object was corrupted afterwards (no deltas: the loose objects are what gets pulled)
atk="$work/atk-corrupt"; cp -a "$good" "$atk"; export_version "$atk" 3; sign_plain "$A_home" "$atk" "$A_fpr"
rm -rf "$atk/deltas"
# The two file objects that are NEW in version 3 (the VERSION file and the start script) are identified by their CHECKSUMS in the commit (`ostree ls -C`), never by file times
# (commit times are set explicitly above and file times say nothing about which objects are new). They swap their contents: both stay well-formed objects, but neither matches
# its checksum, while the commit itself keeps its valid signature.
object_file() { # repo path -> the loose object file of that path in the newest commit of $REF
  local cs ext
  cs="$(ostree --repo="$1" ls -R -C "$REF" "$2" 2>/dev/null | awk -v p="$2" '$NF==p{print $(NF-1)}' | head -1)"
  [ -n "$cs" ] || return 1
  for ext in filez file; do
    if [ -f "$1/objects/${cs:0:2}/${cs:2}.$ext" ]; then echo "$1/objects/${cs:0:2}/${cs:2}.$ext"; return 0; fi
  done
  return 1
}
new_a="$(object_file "$atk" /files/VERSION || true)"; new_b="$(object_file "$atk" /files/bin/tsw-e2e || true)"
old_a="$(object_file "$good" /files/VERSION || true)"; old_b="$(object_file "$good" /files/bin/tsw-e2e || true)"
# the object id (objects/xx/yyyy.ext) of a file: the SAME file object in the old and the new repository has the same id, so a changed id means a new object
oid() { [ -n "$1" ] && echo "${1#*/objects/}" || echo none; }
if [ -n "$new_a" ] && [ -n "$new_b" ] && [ "$(oid "$new_a")" != "$(oid "$old_a")" ] && [ "$(oid "$new_b")" != "$(oid "$old_b")" ] && ! cmp -s "$new_a" "$new_b"; then
  ok "the two objects to corrupt are the NEW file objects of version 3 (found by checksum)"
  cp "$new_a" "$work/swap.tmp"; cp "$new_b" "$new_a"; cp "$work/swap.tmp" "$new_b"
  serve "$atk"; update_blocked X 2 "a signed version with a CORRUPTED object is refused"
else
  # the test could not be prepared: say exactly what was found (this is a FAILURE, the check is never skipped)
  bad "the two new file objects of version 3 could not be identified (the test setup, not the update, is at fault)"
  {
    echo "        new VERSION object: ${new_a:-none}   new script object: ${new_b:-none}"
    echo "        old VERSION object: ${old_a:-none}   old script object: ${old_b:-none}"
    echo "        ostree ls of the new commit:"; ostree --repo="$atk" ls -R -C "$REF" 2>&1 | sed 's/^/          /' | head -20
    echo "        file objects in the repository:"; find "$atk/objects" -name '*.file*' -printf '          %TY-%Tm-%Td %TH:%TM:%TS %p
' 2>&1 | sort | tail -8
    echo "        summary written: $(date -r "$good/summary" '+%F %T' 2>/dev/null || echo unknown)"
  }
fi
serve "$good"

say "S4 rollback: a replayed old state is not an update; the authorised restore works"
serve "$work/state-v1"
if fp X update --noninteractive --no-deps --no-related "$APP" >"$work/last.out" 2>&1 && [ "$(installed_version X)" = "1" ]; then
  if [ "${E2E_STRICT_ROLLBACK:-0}" = 1 ]; then bad "a replayed old (validly signed) repository state DOWNGRADED the client: this Flatpak has no downgrade protection for plain updates"
  else note "a replayed old (validly signed) repository state downgraded the client on THIS Flatpak ($(flatpak --version)): it has no downgrade protection for plain updates; the only guard is that nobody but us can serve the repository (GitHub Pages) and that every published state is newer (flatpak_channel.py)"; fi
  serve "$good"; fp X update --noninteractive --no-deps --no-related --commit="$c2" "$APP" >/dev/null 2>&1 || true
else
  ok "a replayed old signed state is NOT taken as an update (Flatpak's downgrade protection)"
fi
expect_version X 2
serve "$good"
check "the AUTHORISED restore of the previous version works (update --commit=<version 1>)" fp X update --noninteractive --no-deps --no-related --commit="$c1" "$APP"
expect_version X 1
check "the next normal update brings the newest version again" fp X update --noninteractive --no-deps --no-related "$APP"
expect_version X 2

say "S5 key rotation and revocation (A = old key, B = new key, A is later 'compromised')"
# a second client that stays on key A only, installed at version 2, to show the constraint of a rotation
newclient OLD "$A_pub"; check "client OLD (key A only) installs version 2" fp OLD install --noninteractive --no-deps --no-related e2e "$APP//$BRANCH"; expect_version OLD 2
# 5a transition: a version signed with BOTH keys is accepted by a client that only knows A
export_version "$good" 4; sign_dual "$good"
check "a version signed with A and B is accepted by clients that trust only A" fp X update --noninteractive --no-deps --no-related "$APP"; expect_version X 4
check "client OLD (key A only) takes it too" fp OLD update --noninteractive --no-deps --no-related "$APP"; expect_version OLD 4
# 5b rotation: the client receives key B (as a new bundle or repository file would deliver it); then B-only releases are accepted
check "client X imports the new key B" fp X remote-modify --gpg-import="$B_pub" e2e
export_version "$good" 5; sign_with_tool "$B_home" "$B_fpr" "$good"
check "after the rotation a version signed with B only is accepted" fp X update --noninteractive --no-deps --no-related "$APP"; expect_version X 5
# the client that never received B cannot follow a B-only release: a rotation needs the dual-signed period and a new bundle/key file first
update_blocked OLD 4 "a client that never received key B refuses a B-only release (rotation needs the dual-signed period)"
# 5c revocation of the compromised key A: the client is set up again WITH KEY B ONLY (what a new bundle does); a release signed with A only is then refused
fp X remote-delete --force e2e >/dev/null 2>&1 || true
newclient Y "$B_pub"; check "client Y (key B only) installs the current version" fp Y install --noninteractive --no-deps --no-related e2e "$APP//$BRANCH"; expect_version Y 5
export_version "$good" 6; sign_plain "$A_home" "$good" "$A_fpr"
update_blocked Y 5 "after the revocation a release signed only with the compromised key A is refused"
sign_plain "$B_home" "$good" "$B_fpr"
check "the same release signed with B is accepted" fp Y update --noninteractive --no-deps --no-related "$APP"; expect_version Y 6

say "isolation: the real installations after the test"
verify_untouched
say "result"
printf '   %d passed, %d failed, %d note(s)  (flatpak %s, ostree %s)\n' "$pass" "$fail" "$info" "$(flatpak --version | awk '{print $2}')" "$(ostree --version | awk '/Version/{print $2}' | tr -d "'")"
[ "$fail" -eq 0 ]
