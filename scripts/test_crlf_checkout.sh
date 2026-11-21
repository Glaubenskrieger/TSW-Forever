#!/usr/bin/env bash
# Reproduce a WINDOWS CHECKOUT on Linux: copy the SOURCES to build/crlf-checkout, give every source text file CRLF line endings (what git for Windows does without a
# .gitattributes) and run the Rust tests of the launcher and the window program there. The 0.4.1 Windows CI failed in a source-level test that looked for "\n}\n" in an embedded
# source file; this script finds that class of failure before a push, without a Windows machine.
#
#  - Only the explicit source include list below is copied (the same kind of list as scripts/native_stage.sh): never analysis data, logs, game files, reports or anything else of the
#    working tree. Everything is written below build/ of this repository (ignored by Git), nothing in /tmp and nothing outside the workspace.
#  - The copy lives at a FIXED path with its own target directory (build/crlf-target, or TSW_CRLF_TARGET_DIR): build scripts compile their own location in, so a random or shared
#    location would leave a stale path in a shared build cache.
#
#   scripts/test_crlf_checkout.sh            offline; needs cargo, rsync and a warm cargo cache (the same as `scripts/build_flatpak.sh test`)
set -euo pipefail
root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
command -v cargo >/dev/null || { echo "test_crlf_checkout: cargo is needed" >&2; exit 10; }
command -v rsync >/dev/null || { echo "test_crlf_checkout: rsync is needed" >&2; exit 10; }
copy="$root/build/crlf-checkout"
export CARGO_TARGET_DIR="${TSW_CRLF_TARGET_DIR:-$root/build/crlf-target}"
case "$copy" in "$root/build/"*) ;; *) echo "test_crlf_checkout: refusing to write outside build/" >&2; exit 11;; esac
mkdir -p "$copy"
rsync -a --delete --delete-excluded \
  --include='/patcher/' --include='/patcher/Cargo.toml' --include='/patcher/Cargo.lock' --include='/patcher/src/***' --include='/patcher/tests/***' \
  --include='/launcher/' --include='/launcher/Cargo.toml' --include='/launcher/Cargo.lock' --include='/launcher/build.rs' --include='/launcher/src/***' --include='/launcher/tests/***' \
  --include='/launcher-gui/' --include='/launcher-gui/Cargo.toml' --include='/launcher-gui/Cargo.lock' --include='/launcher-gui/build.rs' \
  --include='/launcher-gui/src/***' --include='/launcher-gui/tests/***' --include='/launcher-gui/ui/***' --include='/launcher-gui/assets/***' --include='/launcher-gui/translations/***' --include='/launcher-gui/packaging/***' \
  --include='/packaging/' --include='/packaging/project.conf' --include='/packaging/channels/' --include='/packaging/channels/*.conf' \
  --include='/rustfmt.toml' --include='/.cargo/' --include='/.cargo/config.toml' \
  --exclude='target/' --exclude='*' "$root/" "$copy/"
find "$copy" -type f \( -name '*.rs' -o -name '*.toml' -o -name '*.slint' -o -name '*.conf' -o -name '*.po' -o -name '*.lock' \) -exec sed -i 's/$/\r/' {} +
grep -q $'\r' "$copy/launcher-gui/src/main.rs" || { echo "test_crlf_checkout: the copy is not CRLF" >&2; exit 1; }
(cd "$copy/launcher" && cargo test --offline --locked)
(cd "$copy/launcher-gui" && cargo test --offline --locked)
(cd "$copy/launcher-gui" && cargo test --offline --locked --features windows-self-update)
echo "test_crlf_checkout: all tests pass on a CRLF checkout"
