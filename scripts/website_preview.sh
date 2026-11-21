#!/usr/bin/env bash
# Local preview of the web site (website/) at http://localhost:8000 - nothing is published, nothing leaves this machine.
#
#   scripts/website_preview.sh [port]      default port 8000; stop with Ctrl-C
#
# Serves only the folder website/ on the loopback address (127.0.0.1), so other computers cannot reach it. Uses the Python 3 that the project needs anyway.
set -euo pipefail
root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
port="${1:-8000}"
case "$port" in ''|*[!0-9]*) echo "website_preview: the port must be a number" >&2; exit 2;; esac
[ -f "$root/website/index.html" ] || { echo "website_preview: $root/website/index.html is missing" >&2; exit 1; }
command -v python3 >/dev/null || { echo "website_preview: python3 is needed" >&2; exit 10; }
python3 -I "$root/scripts/gen_website_themes.py" --check || { echo "website_preview: website/css/themes.css is out of date; run scripts/gen_website_themes.py" >&2; exit 1; }
echo "TSW-Forever web site preview: http://localhost:$port  (Ctrl-C stops it)"
exec python3 -I -m http.server "$port" --bind 127.0.0.1 --directory "$root/website"
