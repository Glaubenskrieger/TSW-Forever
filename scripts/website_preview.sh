#!/usr/bin/env bash
# Local preview of the web site on the loopback address - nothing is published, nothing is deployed, nothing leaves this machine except the requests the page itself makes to the GitHub API
# (the download buttons look up the newest published release).
#
#   scripts/website_preview.sh [port]                          the folder website/ at http://127.0.0.1:<port>/ (default 8000); stop with Ctrl-C
#   scripts/website_preview.sh --as-pages [port]               the site ASSEMBLED exactly like the deployment (scripts/assemble_pages_site.py, the same tool as the web site build of CI),
#                                                              served at http://127.0.0.1:<port>/TSW-Forever/ - the same sub-path as on GitHub Pages
#   scripts/website_preview.sh --from-artifact <folder|zip> [port]
#                                                              the site that CI built and kept (artifact tsw-forever-site-<commit>: `gh run download <run> -n tsw-forever-site-<commit>`,
#                                                              or the downloaded zip), after checking its checksums and the assembler's rules; served at /TSW-Forever/ as above. This is
#                                                              the byte-identical site that production would deploy from the same commit.
#
# Serves on 127.0.0.1 only, so other computers cannot reach it. Uses the Python 3 that the project needs anyway. Writes below build/ only (ignored by Git).
# Test hooks: TSW_PREVIEW_BUILD=<folder> replaces build/ as the place it writes to; TSW_PREVIEW_PREPARE_ONLY=1 prepares the folder and exits without starting the server.
set -euo pipefail
root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
port=8000
pages=0
artifact=""
while [ $# -gt 0 ]; do
  case "$1" in
    --as-pages) pages=1 ;;
    --from-artifact) shift; [ $# -gt 0 ] || { echo "website_preview: --from-artifact needs a folder or a zip file" >&2; exit 2; }; artifact="$1" ;;
    ''|*[!0-9]*) echo "website_preview: unknown argument '$1' (a port number, --as-pages or --from-artifact <folder|zip>)" >&2; exit 2 ;;
    *) port="$1" ;;
  esac
  shift
done
command -v python3 >/dev/null || { echo "website_preview: python3 is needed" >&2; exit 10; }
base="${TSW_PREVIEW_BUILD:-$root/build}"
out="$base/site-preview"
case "$out" in "$base/"*) ;; *) echo "website_preview: refusing to write outside the build folder" >&2; exit 11;; esac
serve() { [ "${TSW_PREVIEW_PREPARE_ONLY:-}" = 1 ] && { echo "prepared: $1"; exit 0; }; exec python3 -I -m http.server "$port" --bind 127.0.0.1 --directory "$1"; }

if [ -n "$artifact" ]; then
  [ -e "$artifact" ] || { echo "website_preview: $artifact does not exist" >&2; exit 1; }
  src="$base/site-preview-src"
  rm -rf "$src"; mkdir -p "$src"
  if [ -f "$artifact" ]; then
    python3 -I -m zipfile -e "$artifact" "$src" >/dev/null
  else
    cp -R "$artifact/." "$src/"
  fi
  if [ -f "$src/site/index.html" ]; then site="$src/site"; elif [ -f "$src/index.html" ]; then site="$src"; else echo "website_preview: no site (index.html) found in the artifact" >&2; exit 1; fi
  if [ -f "$src/SHA256SUMS-site" ]; then
    (cd "$site" && sha256sum --check --strict --quiet "$src/SHA256SUMS-site") || { echo "website_preview: the checksums of the artifact do not match its files" >&2; exit 1; }
    echo "checksums of the artifact: ok"
  else
    echo "website_preview: note: the artifact has no SHA256SUMS-site; its content is only checked by the assembler's rules" >&2
  fi
  [ -f "$src/COMMIT" ] && echo "built from commit: $(cat "$src/COMMIT")"
  python3 -I "$root/scripts/assemble_pages_site.py" --verify "$site"
  rm -rf "$out"; mkdir -p "$out"
  cp -R "$site" "$out/TSW-Forever"
  echo "TSW-Forever web site preview (the site that CI kept): http://127.0.0.1:$port/TSW-Forever/  (Ctrl-C stops it)"
  serve "$out"
fi

[ -f "$root/website/index.html" ] || { echo "website_preview: $root/website/index.html is missing" >&2; exit 1; }
python3 -I "$root/scripts/gen_website_themes.py" --check || { echo "website_preview: website/css/themes.css is out of date; run scripts/gen_website_themes.py" >&2; exit 1; }
if [ "$pages" = 1 ]; then
  rm -rf "$out"
  mkdir -p "$out"
  python3 -I "$root/scripts/assemble_pages_site.py" --website "$root/website" --out "$out/TSW-Forever"
  echo "TSW-Forever web site preview (assembled like the deployment): http://127.0.0.1:$port/TSW-Forever/  (Ctrl-C stops it)"
  serve "$out"
fi
echo "TSW-Forever web site preview: http://127.0.0.1:$port/  (Ctrl-C stops it)"
serve "$root/website"
