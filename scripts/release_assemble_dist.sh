#!/usr/bin/env bash
# Assembles the four release files in `dist/` from the verified candidate parts (`parts/<artifact>/`) and verifies the set: checksums of every part, SHA256SUMS of the release, the exact
# release set, the audits of the source archive, and that the archive carries the version. Shared by the release workflow (job `draft`) and the read-only release rehearsal. Writes `dist/` only.
#
#   release_assemble_dist.sh            Environment: VERSION, GITHUB_SHA
set -euo pipefail
: "${VERSION:?}" "${GITHUB_SHA:?}"
v="$VERSION"
win="parts/tsw-forever-${v}-windows-${GITHUB_SHA}"
(cd "$win" && sha256sum -c SHA256SUMS-windows)
(cd "parts/tsw-forever-${v}-source-${GITHUB_SHA}" && sha256sum -c SHA256SUMS-source)
mkdir -p dist
(cd "parts/tsw-forever-${v}-flatpak-${GITHUB_SHA}" && sha256sum -c SHA256SUMS-flatpak)
cp "parts/tsw-forever-${v}-flatpak-${GITHUB_SHA}/tsw-forever-${v}.flatpak" "dist/tsw-forever-${v}.flatpak"
cp "parts/tsw-forever-${v}-source-${GITHUB_SHA}/tsw-forever-${v}-src.tar.gz" dist/
cp "$win/tsw-forever-${v}-windows-setup.exe" dist/
(cd dist && sha256sum "tsw-forever-${v}.flatpak" "tsw-forever-${v}-src.tar.gz" "tsw-forever-${v}-windows-setup.exe" > SHA256SUMS && cat SHA256SUMS)
python3 -I scripts/check_release_set.py dist --version "$v" --with-windows
python3 -I scripts/check_public_source.py "dist/tsw-forever-${v}-src.tar.gz"
python3 -I scripts/check_release_artifacts.py "dist/tsw-forever-${v}-src.tar.gz"
tar xzOf "dist/tsw-forever-${v}-src.tar.gz" "tsw-forever-${v}/packaging/project.conf" | grep -qx "APP_VERSION=$v"
