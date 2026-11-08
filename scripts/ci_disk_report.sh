#!/usr/bin/env bash
# Disk report for CI steps (read-only): free space, free inodes and the size of the build directories that exist, so the disk use of every build phase is
# documented in the log and in the job summary (one table row per measuring point).
#
#   scripts/ci_disk_report.sh "<label>" [path ...]     default paths: the build outputs, caches and the Flatpak/source work folders of the rc-artifacts workflow
#
# Nothing is created or deleted except one line appended to $GITHUB_STEP_SUMMARY (when set). Missing paths are skipped.
set -uo pipefail
export LC_ALL=C   # the same column names and number format on every machine
label="${1:?usage: ci_disk_report.sh <label> [path ...]}"
shift
if [ "$#" -eq 0 ]; then
  set -- launcher/target launcher-gui/target patcher/target \
    build/flatpak/state build/flatpak/build-dir build/flatpak/repo build/flatpak/src/vendor build/flatpak/tsw-launcher.flatpak \
    build/native build/source-check build/rc "$HOME/.cargo" "$HOME/.rustup" "$HOME/.local/share/flatpak"
fi
echo "=== disk report: $label ==="
state="${RUNNER_TEMP:-${TMPDIR:-/tmp}}/ci_disk_report.last"   # free bytes of the previous measuring point, to print what a phase used or gave back
avail_b=$(df -B1 --output=avail / | tail -n 1 | tr -d ' ')
change="n/a"
if [ -s "$state" ]; then
  prev=$(cat "$state")
  delta=$(( (avail_b - prev) / 1048576 ))
  change=$(printf '%+d MiB' "$delta")
fi
echo "$avail_b" > "$state" 2>/dev/null || true
echo "  free space change since the previous measuring point: $change"
df -h / | sed 's/^/  df -h  /'
df -i / | sed 's/^/  df -i  /'
for p in "$@"; do
  [ -n "$p" ] && [ -e "$p" ] && du -sh "$p" 2>/dev/null | sed 's/^/  du -sh /'
done
if command -v docker >/dev/null 2>&1 && docker info >/dev/null 2>&1; then
  docker system df 2>/dev/null | sed 's/^/  docker /'
fi
if [ -n "${GITHUB_STEP_SUMMARY:-}" ]; then
  [ -s "$GITHUB_STEP_SUMMARY" ] && grep -q '^| measuring point |' "$GITHUB_STEP_SUMMARY" || printf '| measuring point | free | used | free inodes | change since previous |\n|---|---|---|---|---|\n' >> "$GITHUB_STEP_SUMMARY"
  read -r avail pcent < <(df -h --output=avail,pcent / | tail -n 1)
  ifree=$(df --output=iavail / | tail -n 1 | tr -d ' ')   # --output cannot be combined with -i
  printf '| %s | %s | %s | %s | %s |\n' "$label" "$avail" "$pcent" "$ifree" "$change" >> "$GITHUB_STEP_SUMMARY"
fi
exit 0
