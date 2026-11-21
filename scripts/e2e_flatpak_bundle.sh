#!/usr/bin/env bash
# End-to-end test of the Flatpak BUNDLE install and update path with a STAND-IN application. Linux only (needs flatpak, python3).
#
#   scripts/e2e_flatpak_bundle.sh [--bundle <file.flatpak>] [--keep]
#
# Since 0.4.1 the launcher is distributed as one installable file, `flatpak install --user <file>.flatpak`, and a newer version is installed the same way, over the old one. This test shows,
# with the real Flatpak doing the work, that exactly that path behaves as the project documents:
#   B1  a fresh install of a bundle works and creates the branch `local` (the branch of the earlier bundles)
#   B2  a newer bundle of the same application ID and branch installed over it is an UPDATE in place: the new files are active, the application ID and the user installation stay the
#       same and NO second branch appears (an installation of 0.3.0 or older is updated like this, without any migration)
#   B3  the user's data (~/.var/app/<id>: settings, game folder links) is untouched by the update
#   B4  installing the same bundle again, or an OLDER bundle, is reported by Flatpak (what exactly happens is printed as INFO: it belongs to the installed Flatpak and is the
#       reason why the documentation names the newer bundle only)
#   B5  no update source is configured: the bundle's remote has no address, so `flatpak update` finds nothing to fetch and never fails
# --bundle <file> additionally installs a REAL bundle (for example build/flatpak/tsw-launcher.flatpak) into the throw-away installation and checks that it is the branch local of the
# project's application ID, that no remote with an address was created and that the files of the application are there. The real bundle is only installed and inspected, never run.
#
# Isolation: HOME, the XDG folders, FLATPAK_USER_DIR, FLATPAK_CONFIG_DIR and FLATPAK_SYSTEM_DIR point into a throw-away folder; every install is `--user` there and uses --no-deps (the
# runtime is not needed for a stand-in and nothing is downloaded). The real Flatpak installations are listed read-only before and after the test and compared. No network, no sudo, no
# key, nothing is published. The throw-away folder is deleted at the end (also after an error); --keep keeps it.
# Exit code 0 = all checks passed, 1 = a check failed, 2 = usage, 10 = a tool is missing.
# shellcheck disable=SC2015,SC1091
set -euo pipefail
root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
keep=0; real=""
while [ $# -gt 0 ]; do
  case "$1" in
    --keep) keep=1; shift;;
    --bundle) real="${2:-}"; shift 2;;
    *) echo "usage: $0 [--bundle <file.flatpak>] [--keep]" >&2; exit 2;;
  esac
done
[ "$(id -u)" != 0 ] || { echo "e2e: do not run this as root" >&2; exit 2; }
for t in flatpak python3; do command -v "$t" >/dev/null || { echo "e2e: missing tool '$t'" >&2; exit 10; }; done
[ -z "$real" ] || [ -s "$real" ] || { echo "e2e: no such bundle: $real" >&2; exit 2; }
# shellcheck disable=SC1091
APP_REAL="$(. "$root/packaging/project.conf"; printf '%s' "$APP_ID")"

tmp="$(mktemp -d /tmp/tsw-e2e-bundle-XXXXXX)"
pass=0; fail=0
cleanup() { [ "$keep" = 1 ] && echo "kept: $tmp" || rm -rf "$tmp"; }
trap cleanup EXIT
ok() { pass=$((pass + 1)); echo "PASS  $*"; }
bad() { fail=$((fail + 1)); echo "FAIL  $*"; }
info() { echo "INFO  $*"; }
check() { local what="$1"; shift; if "$@" >/dev/null 2>&1; then ok "$what"; else bad "$what"; fi; }

listing() { # the real installations, read only (before and after must be identical)
  ( unset FLATPAK_USER_DIR FLATPAK_SYSTEM_DIR FLATPAK_CONFIG_DIR; flatpak list --app --columns=application,branch,commit 2>/dev/null | sort || true )
}
before="$(listing)"

export HOME="$tmp/home" XDG_DATA_HOME="$tmp/home/.local/share" XDG_CONFIG_HOME="$tmp/home/.config" XDG_CACHE_HOME="$tmp/home/.cache" XDG_STATE_HOME="$tmp/home/.local/state"
export FLATPAK_USER_DIR="$tmp/flatpak-user" FLATPAK_CONFIG_DIR="$tmp/flatpak-config" FLATPAK_SYSTEM_DIR="$tmp/flatpak-system"
mkdir -p "$HOME" "$FLATPAK_USER_DIR" "$FLATPAK_CONFIG_DIR" "$FLATPAK_SYSTEM_DIR"
fp() { flatpak "$@"; }

APP="org.tswlauncher.StandIn"
BRANCH="local"
make_bundle() { # <version> <out> [branch, default local]
  local v="$1" out="$2" br="${3:-$BRANCH}" dir="$tmp/app-$1-${3:-$BRANCH}" repo="$tmp/repo-$1-${3:-$BRANCH}"
  mkdir -p "$dir/files/bin" "$dir/files/share/standin"
  printf '[Application]\nname=%s\nruntime=org.freedesktop.Platform/x86_64/25.08\nsdk=org.freedesktop.Sdk/x86_64/25.08\ncommand=standin\n' "$APP" > "$dir/metadata"
  printf '#!/bin/sh\necho standin %s\n' "$v" > "$dir/files/bin/standin"; chmod +x "$dir/files/bin/standin"
  printf '%s\n' "$v" > "$dir/files/share/standin/version"
  fp build-finish --command=standin "$dir" >/dev/null
  fp build-export "$repo" "$dir" "$br" >/dev/null
  fp build-bundle "$repo" "$out" "$APP" "$br" >/dev/null
}
installed_version() { cat "$FLATPAK_USER_DIR/app/$1/x86_64/$BRANCH/active/files/share/standin/version" 2>/dev/null || echo none; }

echo "== stand-in bundles"
make_bundle 1 "$tmp/standin-1.flatpak"; make_bundle 2 "$tmp/standin-2.flatpak"
check "two stand-in bundles exist" test -s "$tmp/standin-1.flatpak" -a -s "$tmp/standin-2.flatpak"

echo "== B1 fresh install"
fp install --user --noninteractive --no-deps "$tmp/standin-1.flatpak" >"$tmp/b1.log" 2>&1 && ok "flatpak install --user <bundle> on a fresh system" || { bad "fresh install failed: $(tail -3 "$tmp/b1.log")"; }
[ "$(installed_version "$APP")" = 1 ] && ok "version 1 is active" || bad "version 1 is not active"
fp info --user "$APP//$BRANCH" >/dev/null 2>&1 && ok "the branch $BRANCH is installed for the user" || bad "no $BRANCH branch"
commit1="$(fp info --user --show-commit "$APP//$BRANCH" 2>/dev/null || true)"

echo "== B3 (prepare) the user's data"
mkdir -p "$HOME/.var/app/$APP/config" "$HOME/.var/app/$APP/data"
printf 'resolution=1920x1080\n' > "$HOME/.var/app/$APP/config/settings.ini"
printf 'game folder marker\n' > "$HOME/.var/app/$APP/data/marker.txt"
data_before="$(cd "$HOME/.var/app/$APP" && find . -type f -exec sha256sum {} + | sort)"

echo "== B2 update by installing the newer bundle over the old one"
if fp install --user --noninteractive --no-deps "$tmp/standin-2.flatpak" >"$tmp/b2.log" 2>&1; then ok "flatpak install --user <newer bundle> over the installed one succeeded"; else bad "installing the newer bundle failed: $(tail -3 "$tmp/b2.log")"; fi
[ "$(installed_version "$APP")" = 2 ] && ok "version 2 is active after the update" || bad "version 2 is not active (still $(installed_version "$APP")); log: $(tail -3 "$tmp/b2.log")"
commit2="$(fp info --user --show-commit "$APP//$BRANCH" 2>/dev/null || true)"
[ -n "$commit2" ] && [ "$commit1" != "$commit2" ] && ok "the commit changed" || bad "the commit did not change"
[ "$(fp list --user --app --columns=application | grep -c "^$APP$")" = 1 ] && ok "still exactly one installation of the application ID" || bad "more than one installation of the application"
[ "$(fp list --user --app --columns=application,branch | grep "^$APP" | awk '{print $2}' | sort | tr '\n' ' ')" = "$BRANCH " ] && ok "exactly one branch ($BRANCH) after the update: no parallel branch" || bad "a second branch appeared"
[ "$(fp list --system --app --columns=application 2>/dev/null | grep -c "^$APP$" || true)" = 0 ] && ok "nothing was installed system-wide" || bad "a system-wide installation appeared"

echo "== B3 the user's data is untouched"
data_after="$(cd "$HOME/.var/app/$APP" && find . -type f -exec sha256sum {} + | sort)"
[ "$data_before" = "$data_after" ] && ok "settings and data in ~/.var/app/$APP are byte-identical after the update" || bad "the user's data changed"

echo "== B4 the same bundle again, and an older bundle"
if fp install --user --noninteractive --no-deps "$tmp/standin-2.flatpak" >"$tmp/b4a.log" 2>&1; then info "the same bundle again: accepted (exit 0): $(tail -1 "$tmp/b4a.log" | cut -c1-160)"; else info "the same bundle again: refused (exit $?): $(tail -1 "$tmp/b4a.log" | cut -c1-160)"; fi
[ "$(installed_version "$APP")" = 2 ] && ok "still version 2 after installing the same bundle again" || bad "version changed after installing the same bundle again"
if fp install --user --noninteractive --no-deps "$tmp/standin-1.flatpak" >"$tmp/b4b.log" 2>&1; then info "an OLDER bundle: accepted (exit 0), version now $(installed_version "$APP"): $(tail -1 "$tmp/b4b.log" | cut -c1-160)"; else info "an OLDER bundle: refused: $(tail -1 "$tmp/b4b.log" | cut -c1-160)"; fi
info "installed version after the older bundle: $(installed_version "$APP") (the documentation therefore tells users to install the NEWER bundle only)"
data_after2="$(cd "$HOME/.var/app/$APP" && find . -type f -exec sha256sum {} + | sort)"
[ "$data_before" = "$data_after2" ] && ok "the user's data survived every install" || bad "the user's data changed during B4"

echo "== B5 no update source"
fp remotes --user --columns=name,url 2>/dev/null | sed 's/^/INFO  remote: /' || true
if fp remotes --user --columns=url 2>/dev/null | grep -qi 'https\?://'; then bad "a remote with an address exists: the bundle must not configure an update source"; else ok "no remote with an address: there is no update source"; fi
if fp update --user --noninteractive --no-deps "$APP" >"$tmp/b5.log" 2>&1; then ok "flatpak update finds nothing to fetch and does not fail"; else info "flatpak update exited non-zero: $(tail -1 "$tmp/b5.log" | cut -c1-160)"; fi

if [ -n "$real" ]; then
  echo "== real bundle: $real"
  rm -rf "$FLATPAK_USER_DIR/app" "$FLATPAK_USER_DIR/repo"
  if fp install --user --noninteractive --no-deps "$real" >"$tmp/real.log" 2>&1; then ok "the real bundle installs on a fresh system"; else bad "the real bundle did not install: $(tail -3 "$tmp/real.log")"; fi
  fp info --user "$APP_REAL//local" >/dev/null 2>&1 && ok "$APP_REAL is installed on the branch local" || bad "$APP_REAL//local is not installed"
  files="$FLATPAK_USER_DIR/app/$APP_REAL/x86_64/local/active/files"
  [ -x "$files/bin/tsw-launcher" ] && [ -x "$files/bin/tsw-launch" ] && ok "the launcher and its command line program are in the bundle" || bad "the launcher programs are missing"
  if fp remotes --user --columns=url 2>/dev/null | grep -qi 'https\?://'; then bad "the real bundle configured a remote with an address"; else ok "the real bundle has no update source"; fi
  if grep -aq 'tsw-forever-release-build' "$files/bin/tsw-launcher" "$files/bin/tsw-launch"; then ok "the programs carry the RELEASE build marker (a release bundle)"; else info "the programs carry no release build marker (a developer bundle: it never asks for a newer version)"; fi
  want="$(. "$root/packaging/project.conf"; printf '%s' "$APP_VERSION")"
  grep -rqs "$want" "$files/share/metainfo/$APP_REAL.metainfo.xml" && ok "the metainfo names version $want" || bad "the metainfo does not name version $want"
fi

echo "== the real installations were not touched"
after="$(listing)"
[ "$before" = "$after" ] && ok "the list of the real Flatpak applications is unchanged" || bad "the real Flatpak installation list changed"
echo "result: $pass passed, $fail failed"
[ "$fail" = 0 ]
