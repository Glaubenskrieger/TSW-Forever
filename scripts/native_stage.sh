#!/bin/bash
# Stage the sources for building the launcher on your own and write the deterministic SOURCE ARCHIVE (the second official Linux artifact next to the Flatpak).
#
#   scripts/native_stage.sh     ->  build/native/stage/tsw-forever-<version>/   and   build/native/tsw-forever-<version>-src.tar.gz (+ .sha256)
#
# Same source selection as the Flatpak staging (scripts/build_flatpak.sh stage), plus the licence texts, the notices, the build instructions and the install step.
# No vendored crates: a build fetches them from the lock files, or uses an offline vendor folder (documentation/building-from-source.md).
# Reproducible: sorted entries, fixed owner and time, file modes normalised to 644/755 (independent of the umask of the machine), `gzip -n`. Writes only below build/native/.
set -euo pipefail
export LC_ALL=C   # the sort order of the entries must not depend on the locale of the machine
root=$(cd "$(dirname "$0")/.." && pwd)
ver=$(python3 -I "$root/scripts/version.py" show)
name="tsw-forever-$ver"; out="$root/build/native"; dir="$out/stage/$name"
command -v rsync >/dev/null || { echo "native_stage: rsync is needed" >&2; exit 1; }
case "$dir" in "$root/build/native/stage/"*) ;; *) echo "native_stage: refusing to write outside build/native" >&2; exit 11;; esac
mkdir -p "$dir"
rsync -a --delete --delete-excluded \
  --include='/patcher/' --include='/patcher/Cargo.toml' --include='/patcher/Cargo.lock' --include='/patcher/src/***' \
  --include='/launcher/' --include='/launcher/Cargo.toml' --include='/launcher/Cargo.lock' --include='/launcher/src/***' --include='/launcher/tests/***' \
  --include='/launcher-gui/' --include='/launcher-gui/Cargo.toml' --include='/launcher-gui/Cargo.lock' --include='/launcher-gui/build.rs' \
  --include='/launcher-gui/src/***' --include='/launcher-gui/ui/***' --include='/launcher-gui/assets/***' --include='/launcher-gui/translations/***' --include='/launcher-gui/packaging/***' \
  --include='/packaging/' --include='/packaging/project.conf' --include='/packaging/flatpak/' --include='/packaging/flatpak/***' --include='/packaging/windows/***' \
  --include='/scripts/' --include='/scripts/install_native.sh' --include='/scripts/version.py' --include='/scripts/build_flatpak.sh' --include='/scripts/check_flatpak_package.py' \
  --include='/scripts/build_windows_installer.ps1' --include='/scripts/test_windows_installer.ps1' --include='/documentation/building-windows.md' \
  --include='/documentation/' --include='/documentation/building-from-source.md' \
  --include='/documentation/architecture.md' --include='/documentation/packaging.md' --include='/documentation/security.md' \
  --include='/documentation/third-party-components.md' --include='/documentation/versioning-policy.md' \
  --include='/LICENSES/***' --include='/scripts/check_licenses.py' \
  --include='/documentation/user-guide.md' --include='/documentation/public-export.md' --include='/documentation/microsoft-components.md' \
  --include='/LICENSE' --include='/THIRD-PARTY-NOTICES.md' --include='/README.md' --include='/CHANGELOG.md' --include='/rustfmt.toml' --include='/.cargo/' --include='/.cargo/config.toml' \
  --exclude='target/' --exclude='*' "$root/" "$dir/"
mkdir -p "$out"
tar --sort=name --owner=0 --group=0 --numeric-owner --mode='u=rwX,go=rX' --mtime='2026-11-01 00:00:00Z' -C "$out/stage" -cf - "$name" | gzip -9n > "$out/$name-src.tar.gz"
(cd "$out" && sha256sum "$name-src.tar.gz" > "$name-src.tar.gz.sha256" && cat "$name-src.tar.gz.sha256")
echo "staged: $dir"
