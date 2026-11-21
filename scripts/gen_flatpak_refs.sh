#!/usr/bin/env bash
# Create the install files for the project's Flatpak repository (stable only): <out>/tsw-forever.flatpakrepo and <out>/org.tswlauncher.Launcher.flatpakref.
#   scripts/gen_flatpak_refs.sh <out-dir> <public-gpg-key-file>
# URLs come from packaging/project.conf. The public key (binary or ASCII-armoured export of the signing key) is embedded base64 as `GPGKey=`.
# Nothing is published by this script.
set -euo pipefail
root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
# shellcheck disable=SC1091
source "$root/packaging/project.conf"
out="${1:?output directory}"; key="${2:?public key file}"
[ -s "$key" ] || { echo "gen_flatpak_refs: the public key file is empty or missing" >&2; exit 1; }
mkdir -p "$out"
if grep -q 'BEGIN PGP' "$key"; then b64="$(gpg --dearmor < "$key" | base64 -w0)"; else b64="$(base64 -w0 < "$key")"; fi
cat > "$out/tsw-forever.flatpakrepo" <<EOT
[Flatpak Repo]
Title=$PROJECT_NAME
Url=$FLATPAK_REPO_URL
Homepage=$REPO_URL
Comment=Launcher for the original The Secret World client
GPGKey=$b64
EOT
cat > "$out/$APP_ID.flatpakref" <<EOT
[Flatpak Ref]
Title=$PROJECT_NAME Launcher
Name=$APP_ID
Branch=stable
Url=$FLATPAK_REPO_URL
IsRuntime=false
SuggestRemoteName=tsw-forever
RuntimeRepo=https://dl.flathub.org/repo/flathub.flatpakrepo
Homepage=$REPO_URL
GPGKey=$b64
EOT
echo "wrote $out/tsw-forever.flatpakrepo and $out/$APP_ID.flatpakref (stable)"
