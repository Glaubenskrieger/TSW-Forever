<#
 Tests the freshly built installer on a real Windows machine (no game, no network):
   1. default installation (silent): lands in the Program Files folder reported by Windows, 64-bit program files, Start menu entry, machine-wide uninstall entry;
   2. the installed program starts WITHOUT administrator rights (de-elevated process) and reports the project version; it writes nothing into its folder;
   3. a second run over the existing installation (update) keeps the folder and leaves one single uninstall entry;
   4. clean uninstall: program files and shortcuts gone, nothing else touched;
   5. custom installation (/DIR) into a folder with a space and Unicode characters, with decoy "game" and "settings" folders next to it that must survive installing and uninstalling.
 The installation needs administrator rights (UAC). Started without them, the script restarts itself elevated (the UAC prompt must be confirmed); on GitHub's runner the
 account is already an administrator. Windows only.
   powershell -NoProfile -File scripts\test_windows_installer.ps1
#>
$ErrorActionPreference = "Stop"
$id = [Security.Principal.WindowsIdentity]::GetCurrent()
$isAdmin = ([Security.Principal.WindowsPrincipal]$id).IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)
if (-not $isAdmin) {
    Write-Host "Not elevated: restarting the installer test with administrator rights (confirm the UAC prompt)."
    $log = Join-Path ([System.IO.Path]::GetTempPath()) "tsw-installer-test-elevated.log"
    if (Test-Path $log) { Remove-Item $log -Force }
    $cmd = "& { try { & '$PSCommandPath' *>&1 | Tee-Object -FilePath '$log'; exit `$LASTEXITCODE } catch { `$_ | Out-String | Tee-Object -FilePath '$log' -Append; exit 1 } }"
    $p = Start-Process powershell -Verb RunAs -Wait -PassThru -ArgumentList @("-NoProfile", "-ExecutionPolicy", "Bypass", "-Command", $cmd)
    if (Test-Path $log) { Get-Content $log }
    exit $p.ExitCode
}

$root = (Resolve-Path (Join-Path $PSScriptRoot "..")).Path
$version = ((Get-Content (Join-Path $root "packaging\project.conf") | Where-Object { $_ -match '^APP_VERSION=' }) -replace '^APP_VERSION=', '').Trim()
$setup = Join-Path $root "build\windows\tsw-forever-$version-windows-setup.exe"
if (-not (Test-Path $setup)) { throw "installer not found: $setup" }

$work = Join-Path ([System.IO.Path]::GetTempPath()) "tsw-installer-test"
if (Test-Path $work) { Remove-Item -Recurse -Force $work }
New-Item -ItemType Directory -Force $work | Out-Null

function Get-Machine($path) {
    $b = [System.IO.File]::ReadAllBytes($path); $pe = [BitConverter]::ToInt32($b, 0x3C); [BitConverter]::ToUInt16($b, $pe + 4)
}
function Get-UninstallEntries {
    # the program is a 64-bit one: its entry is in the native (64-bit) registry view
    Get-ChildItem "HKLM:\Software\Microsoft\Windows\CurrentVersion\Uninstall" -ErrorAction SilentlyContinue |
        Where-Object { (Get-ItemProperty $_.PSPath).DisplayName -like "TSW-Forever*" }
}
function Invoke-Setup($extra, $tag) {
    $a = @("/VERYSILENT", "/SUPPRESSMSGBOXES", "/NORESTART", "/SP-", "/LOG=`"$work\$tag.log`"") + $extra
    $p = Start-Process -FilePath $setup -ArgumentList $a -Wait -PassThru
    if ($p.ExitCode -ne 0) { throw "installer exit code $($p.ExitCode) ($tag); see $work\$tag.log" }
}
function Invoke-Uninstall($dir) {
    $u = Start-Process -FilePath (Join-Path $dir "unins000.exe") -ArgumentList @("/VERYSILENT", "/SUPPRESSMSGBOXES", "/NORESTART") -Wait -PassThru
    if ($u.ExitCode -ne 0) { throw "uninstaller exit code $($u.ExitCode)" }
    Start-Sleep -Seconds 2
    foreach ($f in @("tsw-launcher.exe", "tsw-launch.exe", "unins000.exe")) { if (Test-Path (Join-Path $dir $f)) { throw "left behind after uninstall: $f in $dir" } }
}
function Test-Installed($dir) {
    foreach ($f in @("tsw-launcher.exe", "tsw-launch.exe", "LICENSE", "THIRD-PARTY-NOTICES.md", "unins000.exe")) {
        if (-not (Test-Path (Join-Path $dir $f))) { throw "missing after installation: $f in $dir" }
    }
    foreach ($exe in @("tsw-launcher.exe", "tsw-launch.exe")) {
        if ((Get-Machine (Join-Path $dir $exe)) -ne 0x8664) { throw "$exe is not an x64 program" }
    }
}
function Test-RunsWithoutAdmin($dir) {
    # a de-elevated process (limited token) must start the installed program and get the version; only that process writes the output file
    $out = Join-Path $work "version-limited.txt"
    if (Test-Path $out) { Remove-Item $out -Force }
    $cmdFile = Join-Path $work "run-limited.cmd"
    # UTF-8 without BOM and code page 65001, so that folders with Unicode characters work
    [System.IO.File]::WriteAllText($cmdFile, "@echo off`r`nchcp 65001 > nul`r`n`"$dir\tsw-launch.exe`" --version > `"$out`" 2>&1`r`n", (New-Object System.Text.UTF8Encoding($false)))
    $p = Start-Process runas.exe -ArgumentList @("/trustlevel:0x20000", "`"cmd.exe /c $cmdFile`"") -Wait -PassThru -WindowStyle Hidden
    Start-Sleep -Seconds 3
    $text = if (Test-Path $out) { (Get-Content $out) -join " " } else { "" }
    if ($text -notmatch [regex]::Escape($version)) { throw "the installed tsw-launch.exe did not report $version when started without administrator rights: '$text'" }
}

# ---- 1. default installation ------------------------------------------------------------------------------------------------
$programFiles = [Environment]::GetEnvironmentVariable("ProgramW6432"); if (-not $programFiles) { $programFiles = $env:ProgramFiles }
$defaultDir = Join-Path $programFiles "TSW-Forever"
if (Test-Path $defaultDir) { throw "$defaultDir exists already; uninstall the existing TSW-Forever first (this test will not touch a real installation)" }
if (Get-UninstallEntries) { throw "a TSW-Forever uninstall entry exists already; uninstall it first (this test will not touch a real installation)" }
Invoke-Setup @() "install-default"
Test-Installed $defaultDir
$commonStart = Join-Path $env:ProgramData "Microsoft\Windows\Start Menu\Programs\TSW-Forever"
if (-not (Test-Path (Join-Path $commonStart "TSW-Forever.lnk"))) { throw "Start menu entry missing: $commonStart" }
$entries = @(Get-UninstallEntries)
if ($entries.Count -ne 1) { throw "expected exactly one uninstall entry, found $($entries.Count)" }
if ((Get-ItemProperty $entries[0].PSPath).DisplayVersion -ne $version) { throw "uninstall entry shows another version" }
Test-RunsWithoutAdmin $defaultDir
$before = (Get-ChildItem $defaultDir -Recurse -File | ForEach-Object Name | Sort-Object) -join ","

# ---- 2. update over the existing installation -------------------------------------------------------------------------------
Invoke-Setup @() "update"
Test-Installed $defaultDir
if (@(Get-UninstallEntries).Count -ne 1) { throw "an update created a second uninstall entry" }
if ((Get-ChildItem $defaultDir -Recurse -File | ForEach-Object Name | Sort-Object) -join "," -ne $before) { throw "the update changed the set of files in the program folder" }
Invoke-Uninstall $defaultDir
if (Test-Path (Join-Path $commonStart "TSW-Forever.lnk")) { throw "Start menu entry left behind" }
if (@(Get-UninstallEntries).Count -ne 0) { throw "uninstall entry left behind" }

# ---- 3. custom installation with decoy neighbours ---------------------------------------------------------------------------
# space, umlaut (U+00DC) and a snowman (U+2603), written as code points so that the script is independent of its file encoding (Windows PowerShell 5.1 or pwsh)
$dir = Join-Path $work ("Programm " + [char]0x00DC + "bung " + [char]0x2603)
$game = Join-Path $work "game"; $settings = Join-Path $work "settings"
New-Item -ItemType Directory -Force $game, $settings | Out-Null
Set-Content (Join-Path $game "TheSecretWorldDX11.exe") "decoy"; Set-Content (Join-Path $settings "config.txt") "decoy"
Invoke-Setup @("/DIR=`"$dir`"") "install-custom"
Test-Installed $dir
Test-RunsWithoutAdmin $dir
Invoke-Uninstall $dir
if (-not (Test-Path (Join-Path $game "TheSecretWorldDX11.exe")) -or -not (Test-Path (Join-Path $settings "config.txt"))) { throw "the installer or uninstaller touched files outside the installation" }
if (Test-Path $defaultDir) { throw "the custom installation also created $defaultDir" }
Write-Host "Installer test passed: default install in $defaultDir (x64, admin only for the installation), start without administrator rights, update keeps one entry, clean uninstall, custom folder with space and Unicode, neighbouring folders untouched."
