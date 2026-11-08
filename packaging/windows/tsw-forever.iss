; Inno Setup script of the native Windows launcher (decision record: documentation/building-windows.md).
; Native x64 installation into the Program Files folder (administrator rights only for the installation, never for starting the program), a custom folder
; through the folder page, clean update and uninstall. No game files, no Microsoft components, no settings or other changeable data in the program folder.
; Build: scripts/build_windows_installer.ps1 (passes /DAppVersion=<APP_VERSION> and /DStageDir=<folder with the staged files>).
; The installer is NOT code-signed unless the owner supplies a certificate (documented; Windows SmartScreen will warn for unsigned builds).

#ifndef AppVersion
  #error AppVersion is not defined (pass /DAppVersion=X.Y.Z; scripts/build_windows_installer.ps1 reads it from packaging/project.conf)
#endif
#ifndef StageDir
  #error StageDir is not defined (pass /DStageDir=<folder with tsw-launcher.exe, tsw-launch.exe and the licence files>)
#endif
#ifndef OutDir
  #define OutDir "."
#endif

#define AppName "TSW-Forever"

[Setup]
; fixed identity of the installation (never change it: it ties updates and the uninstaller to the same entry)
AppId={{8B3F5F0E-5C1D-4C58-9E5A-7A2D3B6F7A11}
AppName={#AppName}
AppVersion={#AppVersion}
AppVerName={#AppName} {#AppVersion}
AppPublisher=TSW-Forever project
AppPublisherURL=https://github.com/Glaubenskrieger/TSW-Forever
AppSupportURL=https://github.com/Glaubenskrieger/TSW-Forever
AppUpdatesURL=https://github.com/Glaubenskrieger/TSW-Forever/releases
VersionInfoVersion={#AppVersion}.0
; administrator for the installation only: in 64-bit mode {autopf} is the real Program Files folder (C:\Program Files on a standard system, taken from Windows)
PrivilegesRequired=admin
DefaultDirName={autopf}\{#AppName}
DefaultGroupName={#AppName}
DisableProgramGroupPage=yes
; the folder page is always offered ("Custom installation"); an update proposes the folder of the existing installation
DisableDirPage=no
UsePreviousAppDir=yes
; Start menu entry and the uninstaller entry in "Apps & features"
UninstallDisplayName={#AppName}
UninstallDisplayIcon={app}\tsw-launcher.exe
; the project icon, also for the installer itself (the program carries it as a resource)
SetupIconFile=..\..\launcher-gui\assets\tsw-forever.ico
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
MinVersion=10.0
LicenseFile={#StageDir}\LICENSE
OutputDir={#OutDir}
OutputBaseFilename=tsw-forever-{#AppVersion}-windows-setup
Compression=lzma2
SolidCompression=yes
WizardStyle=modern
CloseApplications=yes
RestartApplications=no
; no changes to PATH, file associations or the registry outside the uninstall entry
ChangesEnvironment=no
ChangesAssociations=no

[Languages]
Name: "english"; MessagesFile: "compiler:Default.isl"
Name: "german"; MessagesFile: "compiler:Languages\German.isl"

[Tasks]
Name: "desktopicon"; Description: "{cm:CreateDesktopIcon}"; GroupDescription: "{cm:AdditionalIcons}"; Flags: unchecked

[Files]
Source: "{#StageDir}\tsw-launcher.exe"; DestDir: "{app}"; Flags: ignoreversion
Source: "{#StageDir}\tsw-launch.exe"; DestDir: "{app}"; Flags: ignoreversion
Source: "{#StageDir}\LICENSE"; DestDir: "{app}"; Flags: ignoreversion
Source: "{#StageDir}\THIRD-PARTY-NOTICES.md"; DestDir: "{app}"; Flags: ignoreversion

[Icons]
Name: "{group}\{#AppName}"; Filename: "{app}\tsw-launcher.exe"
Name: "{group}\{cm:UninstallProgram,{#AppName}}"; Filename: "{uninstallexe}"
Name: "{autodesktop}\{#AppName}"; Filename: "{app}\tsw-launcher.exe"; Tasks: desktopicon

[Run]
Filename: "{app}\tsw-launcher.exe"; Description: "{cm:LaunchProgram,{#AppName}}"; Flags: nowait postinstall skipifsilent runasoriginaluser

; The uninstaller removes only the files above. The user's settings (%APPDATA% / %LOCALAPPDATA%\TSW-Forever) and the game installation are never touched.
