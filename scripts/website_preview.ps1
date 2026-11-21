# Local preview of the web site (website\) on Windows - nothing is published, nothing is deployed; the only requests that leave the machine are those the page itself makes to the GitHub API.
#   powershell -NoProfile -File scripts\website_preview.ps1 [-Port 8000]            the folder website\ at http://127.0.0.1:8000/      stop with Ctrl-C
#   powershell -NoProfile -File scripts\website_preview.ps1 -AsPages [-Port 8000]   the site assembled exactly like the deployment (scripts\assemble_pages_site.py), at http://127.0.0.1:8000/TSW-Forever/
#   powershell -NoProfile -File scripts\website_preview.ps1 -FromArtifact <folder|zip> [-Port 8000]   the site that CI kept (artifact tsw-forever-site-<commit>), checked first, at /TSW-Forever/
param([int]$Port = 8000, [switch]$AsPages, [string]$FromArtifact = "")
$ErrorActionPreference = "Stop"
$root = (Resolve-Path (Join-Path $PSScriptRoot "..")).Path
if (-not (Test-Path (Join-Path $root "website\index.html"))) { throw "website\index.html is missing" }
$py = (Get-Command python -ErrorAction SilentlyContinue).Source
if (-not $py) { $py = (Get-Command py -ErrorAction SilentlyContinue).Source }
if (-not $py) { throw "Python 3 is needed" }
if ($FromArtifact) {
    if (-not (Test-Path $FromArtifact)) { throw "$FromArtifact does not exist" }
    $src = Join-Path $root "build\site-preview-src"
    if (Test-Path $src) { Remove-Item -Recurse -Force $src }
    New-Item -ItemType Directory -Force -Path $src | Out-Null
    if ((Get-Item $FromArtifact).PSIsContainer) { Copy-Item -Recurse -Force (Join-Path $FromArtifact "*") $src } else { Expand-Archive -LiteralPath $FromArtifact -DestinationPath $src -Force }
    $site = if (Test-Path (Join-Path $src "site\index.html")) { Join-Path $src "site" } elseif (Test-Path (Join-Path $src "index.html")) { $src } else { throw "no site (index.html) found in the artifact" }
    $sums = Join-Path $src "SHA256SUMS-site"
    if (Test-Path $sums) {
        foreach ($line in Get-Content $sums) {
            if ($line -match '^([0-9a-f]{64})\s+\*?(.+)$') {
                $file = Join-Path $site ($Matches[2] -replace '^\./', '')
                if ((Get-FileHash -Algorithm SHA256 $file).Hash.ToLower() -ne $Matches[1]) { throw "the checksum of $($Matches[2]) does not match" }
            }
        }
        "checksums of the artifact: ok"
    }
    & $py -I (Join-Path $root "scripts\assemble_pages_site.py") --verify $site
    if ($LASTEXITCODE -ne 0) { throw "the artifact is not a plain web site" }
    $out = Join-Path $root "build\site-preview"
    if (Test-Path $out) { Remove-Item -Recurse -Force $out }
    New-Item -ItemType Directory -Force -Path $out | Out-Null
    Copy-Item -Recurse -Force $site (Join-Path $out "TSW-Forever")
    "TSW-Forever web site preview (the site that CI kept): http://127.0.0.1:$Port/TSW-Forever/  (Ctrl-C stops it)"
    & $py -I -m http.server $Port --bind 127.0.0.1 --directory $out
} elseif ($AsPages) {
    $out = Join-Path $root "build\site-preview"
    if (Test-Path $out) { Remove-Item -Recurse -Force $out }
    New-Item -ItemType Directory -Force -Path $out | Out-Null
    & $py -I (Join-Path $root "scripts\assemble_pages_site.py") --website (Join-Path $root "website") --out (Join-Path $out "TSW-Forever")
    if ($LASTEXITCODE -ne 0) { throw "assembling the site failed" }
    "TSW-Forever web site preview (assembled like the deployment): http://127.0.0.1:$Port/TSW-Forever/  (Ctrl-C stops it)"
    & $py -I -m http.server $Port --bind 127.0.0.1 --directory $out
} else {
    "TSW-Forever web site preview: http://127.0.0.1:$Port/  (Ctrl-C stops it)"
    & $py -I -m http.server $Port --bind 127.0.0.1 --directory (Join-Path $root "website")
}
