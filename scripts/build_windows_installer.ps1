<#
 Builds the native Windows launcher and its per-user installer (Inno Setup). Windows only; the private workflow windows.yml calls it.
 Reads the version from packaging/project.conf. Writes only below build\windows. No signing, no network except what `cargo` needs for the
 crates of the committed Cargo.lock (use -Offline when they are already vendored/cached). Nothing is published or uploaded here.

   powershell -NoProfile -File scripts\build_windows_installer.ps1 [-Target x86_64-pc-windows-msvc] [-SkipInstaller] [-Offline]
#>
param(
    [string]$Target = "x86_64-pc-windows-msvc",
    [switch]$SkipInstaller,
    [switch]$Offline
)
$ErrorActionPreference = "Stop"
$root = (Resolve-Path (Join-Path $PSScriptRoot "..")).Path
$conf = Get-Content (Join-Path $root "packaging\project.conf") | Where-Object { $_ -match '^APP_VERSION=' }
$version = ($conf -replace '^APP_VERSION=', '').Trim()
if ($version -notmatch '^(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)$') { throw "APP_VERSION '$version' is not a plain MAJOR.MINOR.PATCH version" }

$out = Join-Path $root "build\windows"
$stage = Join-Path $out "stage"
if (Test-Path $stage) { Remove-Item -Recurse -Force $stage }
New-Item -ItemType Directory -Force $stage | Out-Null

$cargoArgs = @("build", "--release", "--locked", "--target", $Target)
if ($Offline) { $cargoArgs += "--offline" }
foreach ($crate in @("launcher", "launcher-gui")) {
    Push-Location (Join-Path $root $crate)
    try { & cargo @cargoArgs; if ($LASTEXITCODE -ne 0) { throw "cargo build failed in $crate" } } finally { Pop-Location }
}
Copy-Item (Join-Path $root "launcher-gui\target\$Target\release\tsw-launcher.exe") $stage
Copy-Item (Join-Path $root "launcher\target\$Target\release\tsw-launch.exe") $stage
Copy-Item (Join-Path $root "LICENSE") $stage
Copy-Item (Join-Path $root "THIRD-PARTY-NOTICES.md") $stage

# the staged tree must hold exactly the program files: no game data, no Microsoft or Funcom files
$expected = @("LICENSE", "THIRD-PARTY-NOTICES.md", "tsw-launch.exe", "tsw-launcher.exe")
$found = (Get-ChildItem $stage -File | ForEach-Object Name | Sort-Object)
if (($found -join ",") -ne ($expected -join ",")) { throw "unexpected staged files: $($found -join ', ')" }

function Assert-Pe($path, [UInt16[]]$machines = @(0x8664)) {
    $b = [System.IO.File]::ReadAllBytes($path)
    if ($b.Length -lt 4096 -or $b[0] -ne 0x4D -or $b[1] -ne 0x5A) { throw "$path is not a Windows executable (no MZ header)" }
    $pe = [BitConverter]::ToInt32($b, 0x3C)
    if ([BitConverter]::ToUInt32($b, $pe) -ne 0x00004550) { throw "$path has no PE signature" }
    if ($machines -notcontains [BitConverter]::ToUInt16($b, $pe + 4)) { throw ("{0} has an unexpected machine type 0x{1:x}" -f $path, [BitConverter]::ToUInt16($b, $pe + 4)) }
}
foreach ($exe in @("tsw-launcher.exe", "tsw-launch.exe")) { Assert-Pe (Join-Path $stage $exe) }

# The window program must be a GUI program (no console window) and the command-line tool a console program; the launcher carries the project icon as a resource.
function Get-Subsystem($path) { $b = [System.IO.File]::ReadAllBytes($path); $pe = [BitConverter]::ToInt32($b, 0x3C); [BitConverter]::ToUInt16($b, $pe + 24 + 68) }
if ((Get-Subsystem (Join-Path $stage "tsw-launcher.exe")) -ne 2) { throw "tsw-launcher.exe is not a Windows GUI program (it would open a console window)" }
if ((Get-Subsystem (Join-Path $stage "tsw-launch.exe")) -ne 3) { throw "tsw-launch.exe is not a console program" }
Add-Type -AssemblyName System.Drawing
function Get-IconBytes($path) {
    $icon = [System.Drawing.Icon]::ExtractAssociatedIcon($path); $ms = New-Object System.IO.MemoryStream
    $icon.ToBitmap().Save($ms, [System.Drawing.Imaging.ImageFormat]::Png); $icon.Dispose(); ,$ms.ToArray()
}
$withIcon = [Convert]::ToBase64String((Get-IconBytes (Join-Path $stage "tsw-launcher.exe")))
$noIcon = [Convert]::ToBase64String((Get-IconBytes (Join-Path $stage "tsw-launch.exe")))
if ($withIcon -eq $noIcon) { throw "tsw-launcher.exe shows the generic Windows icon: the project icon resource is missing" }

# Fresh-system guard: our executables must not import the Visual C++ runtime or the Universal CRT (VCRUNTIME140.dll, MSVCP140.dll, api-ms-win-crt-*, ucrtbase.dll),
# which a clean Windows 11 does not necessarily have. The C runtime is linked statically (.cargo/config.toml, crt-static). Import names are plain ASCII in the PE file.
foreach ($exe in @("tsw-launcher.exe", "tsw-launch.exe")) {
    $text = [System.Text.Encoding]::ASCII.GetString([System.IO.File]::ReadAllBytes((Join-Path $stage $exe)))
    foreach ($dll in @("vcruntime140", "msvcp140", "concrt140", "ucrtbase", "api-ms-win-crt-")) {
        if ($text.IndexOf($dll, [StringComparison]::OrdinalIgnoreCase) -ge 0) { throw "$exe depends on $dll*.dll, which is not part of a fresh Windows installation (static C runtime expected)" }
    }
}

# our own console program must run on this machine and report exactly the project version (no game, no network)
$reported = (& (Join-Path $stage "tsw-launch.exe") --version) -join " "
if ($LASTEXITCODE -ne 0 -or $reported -notmatch [regex]::Escape($version)) { throw "tsw-launch.exe --version reported '$reported', expected $version" }

$setup = Join-Path $out "tsw-forever-$version-windows-setup.exe"
if (Test-Path $setup) { Remove-Item -Force $setup }
$artifacts = @()
if (-not $SkipInstaller) {
    $iscc = @(
        (Get-Command ISCC.exe -ErrorAction SilentlyContinue).Source,
        "${env:ProgramFiles(x86)}\Inno Setup 6\ISCC.exe",
        "$env:ProgramFiles\Inno Setup 6\ISCC.exe",
        "$env:LOCALAPPDATA\Programs\Inno Setup 6\ISCC.exe"
    ) | Where-Object { $_ -and (Test-Path $_) } | Select-Object -First 1
    if (-not $iscc) { throw "Inno Setup 6 (ISCC.exe) not found. Install it yourself (the CI workflow installs a pinned version) or run with -SkipInstaller; this script downloads nothing." }
    Write-Host "Using $iscc"
    & $iscc "/DAppVersion=$version" "/DStageDir=$stage" "/DOutDir=$out" (Join-Path $root "packaging\windows\tsw-forever.iss")
    if ($LASTEXITCODE -ne 0) { throw "ISCC failed" }
    if (-not (Test-Path $setup)) { throw "ISCC reported success but $setup does not exist" }
    Assert-Pe $setup @(0x14C, 0x8664)   # the Inno Setup stub is a 32-bit program that runs on every Windows
    $artifacts += $setup
}

$sumFile = Join-Path $out "SHA256SUMS-windows"
$sums = foreach ($a in $artifacts) { "{0}  {1}" -f (Get-FileHash -Algorithm SHA256 $a).Hash.ToLower(), (Split-Path $a -Leaf) }
# LF line endings and no BOM, so sha256sum -c works on every platform (Set-Content would write CRLF)
[System.IO.File]::WriteAllText($sumFile, (($sums -join "`n") + "`n"), (New-Object System.Text.ASCIIEncoding))
# verify what was written: every listed file exists with exactly that hash, and nothing else is listed
$listed = @(Get-Content $sumFile)
if ($listed.Count -ne $artifacts.Count) { throw "SHA256SUMS-windows lists $($listed.Count) files, expected $($artifacts.Count)" }
foreach ($line in $listed) {
    $h, $n = $line -split "  ", 2
    if ((Get-FileHash -Algorithm SHA256 (Join-Path $out $n)).Hash.ToLower() -ne $h) { throw "checksum mismatch for $n" }
}
$listed
Write-Host "Windows build ready in $out (unsigned; the game itself is not exercised here)."
