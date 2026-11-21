# Local preview of the web site (website\) at http://localhost:8000 on Windows - nothing is published, nothing leaves this machine.
#   powershell -NoProfile -File scripts\website_preview.ps1 [-Port 8000]      stop with Ctrl-C
param([int]$Port = 8000)
$ErrorActionPreference = "Stop"
$root = (Resolve-Path (Join-Path $PSScriptRoot "..")).Path
if (-not (Test-Path (Join-Path $root "website\index.html"))) { throw "website\index.html is missing" }
$py = (Get-Command python -ErrorAction SilentlyContinue).Source
if (-not $py) { $py = (Get-Command py -ErrorAction SilentlyContinue).Source }
if (-not $py) { throw "Python 3 is needed" }
"TSW-Forever web site preview: http://localhost:$Port  (Ctrl-C stops it)"
& $py -I -m http.server $Port --bind 127.0.0.1 --directory (Join-Path $root "website")
