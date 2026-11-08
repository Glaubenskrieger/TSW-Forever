#!/bin/sh
# The install step of a build FROM SOURCE (documentation/building-from-source.md): copies the two programs, the desktop entry, the AppStream metadata, the icons
# and the licence texts below DESTDIR. It does not build anything and touches nothing outside DESTDIR. (The official binary format is the Flatpak.)
#
#   install_native.sh <source-root> <bin-dir> <DESTDIR> [--prefix /usr] [--licence-dir DIR] [--doc-dir DIR]
#
#   <source-root>  the unpacked source archive or the checkout     <bin-dir>  folder with the built tsw-launch and tsw-launcher (target/release)
#   --licence-dir  where LICENSE and THIRD-PARTY-NOTICES.md go (default: <prefix>/share/licenses/tsw-forever)
set -eu
[ $# -ge 3 ] || { sed -n '2,9p' "$0" >&2; exit 2; }
src=$1; bin=$2; dest=$3; shift 3
prefix=/usr; licdir=; docdir=
while [ $# -gt 0 ]; do
  case $1 in
    --prefix) prefix=$2; shift 2;;
    --licence-dir) licdir=$2; shift 2;;
    --doc-dir) docdir=$2; shift 2;;
    *) echo "install_native: unknown option $1" >&2; exit 2;;
  esac
done
id=org.tswlauncher.Launcher
[ -n "$licdir" ] || licdir=$prefix/share/licenses/tsw-forever
[ -n "$docdir" ] || docdir=$prefix/share/doc/tsw-forever
for f in "$bin/tsw-launch" "$bin/tsw-launcher" "$src/LICENSE" "$src/THIRD-PARTY-NOTICES.md" "$src/LICENSES/inventory.json" "$src/LICENSES/README.md" "$src/launcher-gui/packaging/linux/$id.desktop" "$src/packaging/flatpak/$id.metainfo.xml"; do
  [ -f "$f" ] || { echo "install_native: missing $f" >&2; exit 1; }
done
# both programs in the SAME folder: the window finds tsw-launch next to itself
install -Dm755 "$bin/tsw-launch" "$dest$prefix/bin/tsw-launch"
install -Dm755 "$bin/tsw-launcher" "$dest$prefix/bin/tsw-launcher"
install -Dm644 "$src/launcher-gui/packaging/linux/$id.desktop" "$dest$prefix/share/applications/$id.desktop"
install -Dm644 "$src/packaging/flatpak/$id.metainfo.xml" "$dest$prefix/share/metainfo/$id.metainfo.xml"
for d in "$src"/launcher-gui/packaging/linux/icons/hicolor/*; do
  s=$(basename "$d")
  if [ "$s" = scalable ]; then install -Dm644 "$d/apps/$id.svg" "$dest$prefix/share/icons/hicolor/scalable/apps/$id.svg"
  else install -Dm644 "$d/apps/$id.png" "$dest$prefix/share/icons/hicolor/$s/apps/$id.png"; fi
done
install -Dm644 "$src/LICENSE" "$dest$licdir/LICENSE"
install -Dm644 "$src/THIRD-PARTY-NOTICES.md" "$dest$licdir/THIRD-PARTY-NOTICES.md"
mkdir -p "$dest$licdir/LICENSES"
cp -R "$src/LICENSES/." "$dest$licdir/LICENSES/"
install -Dm644 "$src/README.md" "$dest$docdir/README.md"
[ ! -f "$src/CHANGELOG.md" ] || install -Dm644 "$src/CHANGELOG.md" "$dest$docdir/CHANGELOG.md"
[ ! -f "$src/documentation/building-from-source.md" ] || install -Dm644 "$src/documentation/building-from-source.md" "$dest$docdir/building-from-source.md"
