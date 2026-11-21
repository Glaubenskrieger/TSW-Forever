; Inno Setup script of the native Windows launcher (decision record: documentation/building-windows.md).
; Native x64 installation into the Program Files folder (administrator rights only for the installation, never for starting the program), a custom folder
; through the folder page, clean update and uninstall. No game files, no Microsoft components, no settings or other changeable data in the program folder.
; Build: scripts/build_windows_installer.ps1 (passes /DAppVersion=<APP_VERSION> and /DStageDir=<folder with the staged files>).
; The installer is NOT code-signed unless the owner supplies a certificate (documented; Windows SmartScreen will warn for unsigned builds).
;
; UPDATE MODE (manual update, product decision 2026-11: Windows has no automatic update). The installer recognises an existing installation of ITS OWN identity (the AppId
; below, which ties the installation to its Uninstall registry key) and then behaves like an update wizard: no licence, folder, task or Start-menu page; one compact page
; "TSW-Forever will be updated from version X to version Y" with the button "Update"; the folder of the existing installation is reused; the user's settings live outside the
; program folder and are never touched. The same version offers a repair, an OLDER installer warns before it replaces a newer installation (a silent installation refuses
; unless /ALLOWDOWNGRADE=1 is given). An installation that cannot be read clearly (no or unreadable version, missing program file or uninstaller) falls back to the normal wizard.
; The detection uses what Inno Setup writes anyway (the Uninstall key of the AppId: DisplayVersion, "Inno Setup: App Path", UninstallString); running launchers are closed by
; Inno Setup's own mechanism (CloseApplications, Restart Manager).
; scripts/test_installer_update_mode.ps1 tests this with a TEST identity (/DTestIdentity=1: other AppId and folder, no administrator rights), never with the real installation.

#ifndef AppVersion
  #error AppVersion is not defined (pass /DAppVersion=X.Y.Z; scripts/build_windows_installer.ps1 reads it from packaging/project.conf)
#endif
#ifndef StageDir
  #error StageDir is not defined (pass /DStageDir=<folder with tsw-launcher.exe, tsw-launch.exe and the licence files>)
#endif
#ifndef OutDir
  #define OutDir "."
#endif

#ifdef TestIdentity
  ; ONLY for scripts/test_installer_update_mode.ps1: another identity so that the real installation is never involved
  #define AppName "TSW-Forever installer test"
  #define AppIdGuid "{5D2C7A10-0E41-4F6B-9B7D-3C1E8A5F2D64}"
#else
  #define AppName "TSW-Forever"
  ; fixed identity of the installation (never change it: it ties updates and the uninstaller to the same entry)
  #define AppIdGuid "{8B3F5F0E-5C1D-4C58-9E5A-7A2D3B6F7A11}"
#endif
; the AppId directive needs the opening brace doubled ("{{"); the code below uses the plain GUID
#define AppIdValue "{" + AppIdGuid
; the folder of the launcher's per-user data (winnative::data_dir); the installer writes the marker file "installer-closing" there (see [Code])
#ifdef TestIdentity
  #define DataFolder "tsw-forever-installer-test"
#else
  #define DataFolder "tsw-forever"
#endif

[Setup]
AppId={#AppIdValue}
AppName={#AppName}
AppVersion={#AppVersion}
AppVerName={#AppName} {#AppVersion}
AppPublisher=TSW-Forever project
AppPublisherURL=https://github.com/Glaubenskrieger/TSW-Forever
AppSupportURL=https://github.com/Glaubenskrieger/TSW-Forever
AppUpdatesURL=https://github.com/Glaubenskrieger/TSW-Forever/releases
VersionInfoVersion={#AppVersion}.0
#ifdef TestIdentity
PrivilegesRequired=lowest
DefaultDirName={autopf}\TSW-Forever-installer-test
#else
; administrator for the installation only: in 64-bit mode {autopf} is the real Program Files folder (C:\Program Files on a standard system, taken from Windows)
PrivilegesRequired=admin
DefaultDirName={autopf}\{#AppName}
#endif
DefaultGroupName={#AppName}
DisableProgramGroupPage=yes
; the folder page is always offered for a NEW installation ("Custom installation"); an update does not show it and reuses the folder of the existing installation
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
Name: "french"; MessagesFile: "compiler:Languages\French.isl"
Name: "spanish"; MessagesFile: "compiler:Languages\Spanish.isl"
Name: "brazilianportuguese"; MessagesFile: "compiler:Languages\BrazilianPortuguese.isl"
Name: "polish"; MessagesFile: "compiler:Languages\Polish.isl"

[CustomMessages]
english.UpdatePageCaption=Update TSW-Forever
english.RepairPageCaption=Repair TSW-Forever
english.DowngradePageCaption=Install an older version of TSW-Forever
english.UpdatePageDescription=An existing installation was found.
english.UpdateText=TSW-Forever will be updated from version %1 to version %2.
english.RepairText=TSW-Forever %1 is already installed. The program files will be installed again (repair).
english.DowngradeText=A NEWER version (%1) is installed. This installer contains the older version %2. Continuing replaces the newer version with the older one.
english.KeepText=Folder: %1%nYour settings and the game stay as they are. A running launcher is closed first (if it belongs to an older version that cannot be closed by the installer, close it yourself).
english.ButtonUpdate=Update
english.ButtonRepair=Repair
english.ButtonDowngrade=Downgrade
english.DowngradeConfirm=Replace the installed version %1 with the older version %2?%n%nThis is normally not needed. Your settings stay as they are, but an older version may not understand settings of a newer one.
english.DowngradeRefused=Version %1 is installed and this installer contains the older version %2. Nothing was changed. To install the older version anyway, start this installer with /ALLOWDOWNGRADE=1.
german.UpdatePageCaption=TSW-Forever aktualisieren
german.RepairPageCaption=TSW-Forever reparieren
german.DowngradePageCaption=Ältere Version von TSW-Forever installieren
german.UpdatePageDescription=Es wurde eine bestehende Installation gefunden.
german.UpdateText=TSW-Forever wird von Version %1 auf Version %2 aktualisiert.
german.RepairText=TSW-Forever %1 ist bereits installiert. Die Programmdateien werden erneut installiert (Reparatur).
german.DowngradeText=Es ist eine NEUERE Version (%1) installiert. Dieser Installer enthält die ältere Version %2. Wenn Sie fortfahren, wird die neuere Version durch die ältere ersetzt.
german.KeepText=Ordner: %1%nIhre Einstellungen und das Spiel bleiben unverändert. Ein laufender Launcher wird vorher geschlossen (gehört er zu einer älteren Version, die sich nicht vom Installer schließen lässt, schließen Sie ihn bitte selbst).
german.ButtonUpdate=Aktualisieren
german.ButtonRepair=Reparieren
german.ButtonDowngrade=Downgrade
german.DowngradeConfirm=Die installierte Version %1 durch die ältere Version %2 ersetzen?%n%nDas ist normalerweise nicht nötig. Ihre Einstellungen bleiben erhalten, aber eine ältere Version kennt die Einstellungen einer neueren möglicherweise nicht.
german.DowngradeRefused=Version %1 ist installiert, dieser Installer enthält die ältere Version %2. Es wurde nichts geändert. Um die ältere Version trotzdem zu installieren, starten Sie diesen Installer mit /ALLOWDOWNGRADE=1.
french.UpdatePageCaption=Mettre à jour TSW-Forever
french.RepairPageCaption=Réparer TSW-Forever
french.DowngradePageCaption=Installer une version antérieure de TSW-Forever
french.UpdatePageDescription=Une installation existante a été trouvée.
french.UpdateText=TSW-Forever sera mis à jour de la version %1 vers la version %2.
french.RepairText=TSW-Forever %1 est déjà installé. Les fichiers du programme seront réinstallés (réparation).
french.DowngradeText=Une version PLUS RÉCENTE (%1) est installée. Cet installateur contient la version antérieure %2. Si vous continuez, la version plus récente sera remplacée par l’ancienne.
french.KeepText=Dossier : %1%nVos paramètres et le jeu restent inchangés. Un launcher en cours d’exécution est fermé au préalable (s’il appartient à une version plus ancienne que l’installateur ne peut pas fermer, fermez-le vous-même).
french.ButtonUpdate=Mettre à jour
french.ButtonRepair=Réparer
french.ButtonDowngrade=Version antérieure
french.DowngradeConfirm=Remplacer la version installée %1 par la version antérieure %2 ?%n%nCela n’est normalement pas nécessaire. Vos paramètres sont conservés, mais une version plus ancienne peut ne pas comprendre les paramètres d’une version plus récente.
french.DowngradeRefused=La version %1 est installée et cet installateur contient la version antérieure %2. Rien n’a été modifié. Pour installer tout de même la version antérieure, lancez cet installateur avec /ALLOWDOWNGRADE=1.
spanish.UpdatePageCaption=Actualizar TSW-Forever
spanish.RepairPageCaption=Reparar TSW-Forever
spanish.DowngradePageCaption=Instalar una versión anterior de TSW-Forever
spanish.UpdatePageDescription=Se encontró una instalación existente.
spanish.UpdateText=TSW-Forever se actualizará de la versión %1 a la versión %2.
spanish.RepairText=TSW-Forever %1 ya está instalado. Los archivos del programa se instalarán de nuevo (reparación).
spanish.DowngradeText=Hay instalada una versión MÁS RECIENTE (%1). Este instalador contiene la versión anterior %2. Si continúa, la versión más reciente se reemplazará por la anterior.
spanish.KeepText=Carpeta: %1%nSu configuración y el juego se mantienen. Un launcher en ejecución se cierra antes (si pertenece a una versión anterior que el instalador no puede cerrar, ciérrelo usted mismo).
spanish.ButtonUpdate=Actualizar
spanish.ButtonRepair=Reparar
spanish.ButtonDowngrade=Versión anterior
spanish.DowngradeConfirm=¿Reemplazar la versión instalada %1 por la versión anterior %2?%n%nNormalmente no es necesario. Su configuración se conserva, pero una versión anterior puede no entender la configuración de una más reciente.
spanish.DowngradeRefused=La versión %1 está instalada y este instalador contiene la versión anterior %2. No se cambió nada. Para instalar la versión anterior de todos modos, inicie este instalador con /ALLOWDOWNGRADE=1.
brazilianportuguese.UpdatePageCaption=Atualizar o TSW-Forever
brazilianportuguese.RepairPageCaption=Reparar o TSW-Forever
brazilianportuguese.DowngradePageCaption=Instalar uma versão anterior do TSW-Forever
brazilianportuguese.UpdatePageDescription=Foi encontrada uma instalação existente.
brazilianportuguese.UpdateText=O TSW-Forever será atualizado da versão %1 para a versão %2.
brazilianportuguese.RepairText=O TSW-Forever %1 já está instalado. Os arquivos do programa serão instalados novamente (reparo).
brazilianportuguese.DowngradeText=Há uma versão MAIS RECENTE (%1) instalada. Este instalador contém a versão anterior %2. Se você continuar, a versão mais recente será substituída pela anterior.
brazilianportuguese.KeepText=Pasta: %1%nSuas configurações e o jogo permanecem como estão. Um launcher em execução é fechado antes (se ele pertencer a uma versão mais antiga que o instalador não consegue fechar, feche-o você mesmo).
brazilianportuguese.ButtonUpdate=Atualizar
brazilianportuguese.ButtonRepair=Reparar
brazilianportuguese.ButtonDowngrade=Versão anterior
brazilianportuguese.DowngradeConfirm=Substituir a versão instalada %1 pela versão anterior %2?%n%nNormalmente isso não é necessário. Suas configurações são mantidas, mas uma versão anterior pode não entender as configurações de uma versão mais recente.
brazilianportuguese.DowngradeRefused=A versão %1 está instalada e este instalador contém a versão anterior %2. Nada foi alterado. Para instalar mesmo assim a versão anterior, inicie este instalador com /ALLOWDOWNGRADE=1.
polish.UpdatePageCaption=Aktualizacja TSW-Forever
polish.RepairPageCaption=Naprawa TSW-Forever
polish.DowngradePageCaption=Instalacja starszej wersji TSW-Forever
polish.UpdatePageDescription=Znaleziono istniejącą instalację.
polish.UpdateText=TSW-Forever zostanie zaktualizowany z wersji %1 do wersji %2.
polish.RepairText=TSW-Forever %1 jest już zainstalowany. Pliki programu zostaną zainstalowane ponownie (naprawa).
polish.DowngradeText=Zainstalowana jest NOWSZA wersja (%1). Ten instalator zawiera starszą wersję %2. Jeśli kontynuujesz, nowsza wersja zostanie zastąpiona starszą.
polish.KeepText=Folder: %1%nTwoje ustawienia i gra pozostają bez zmian. Uruchomiony launcher zostanie wcześniej zamknięty (jeśli należy do starszej wersji, której instalator nie może zamknąć, zamknij go samodzielnie).
polish.ButtonUpdate=Aktualizuj
polish.ButtonRepair=Napraw
polish.ButtonDowngrade=Starsza wersja
polish.DowngradeConfirm=Zastąpić zainstalowaną wersję %1 starszą wersją %2?%n%nZwykle nie jest to potrzebne. Twoje ustawienia zostaną zachowane, ale starsza wersja może nie rozumieć ustawień nowszej.
polish.DowngradeRefused=Zainstalowana jest wersja %1, a ten instalator zawiera starszą wersję %2. Nic nie zmieniono. Aby mimo to zainstalować starszą wersję, uruchom ten instalator z parametrem /ALLOWDOWNGRADE=1.

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
; The launcher's own update starts this installer silently with /RELAUNCH=1: the launcher is then started again as the ORIGINAL user (not elevated). A plain
; silent installation (without the parameter) starts nothing. (Used only by the development feature windows-self-update; harmless without it.)
Filename: "{app}\tsw-launcher.exe"; Flags: nowait runasoriginaluser; Check: RelaunchRequested

; The uninstaller removes only the files above. The user's settings (%APPDATA% / %LOCALAPPDATA%\TSW-Forever) and the game installation are never touched.

[Code]
const
  UninstallKey = 'Software\Microsoft\Windows\CurrentVersion\Uninstall\{#AppIdGuid}_is1';
  ModeNew = 0;
  ModeUpdate = 1;
  ModeRepair = 2;
  ModeDowngrade = 3;

var
  InstallMode: Integer;
  InstalledVersion, InstalledDir: String;
  UpdatePage: TOutputMsgWizardPage;

function RelaunchRequested: Boolean;
begin
  Result := ExpandConstant('{param:RELAUNCH|0}') = '1';
end;

function RemoveQuotes(const S: String): String;
begin
  Result := S;
  if (Length(Result) >= 2) and (Result[1] = '"') then
  begin
    Delete(Result, 1, 1);
    if Pos('"', Result) > 0 then
      Result := Copy(Result, 1, Pos('"', Result) - 1);
  end;
end;

{ Reads the existing installation of THIS identity from the Uninstall key that Inno Setup wrote. Returns True only if everything agrees: a readable version, a folder
  that holds the launcher and an uninstaller that exists. Anything else is "not usable": the normal wizard runs (and says why in the setup log). }
function FindInstallation(var Version, Dir, Why: String): Boolean;
var
  UninstallString: String;
  V: Int64;
begin
  Result := False;
  Version := '';
  Dir := '';
  if not RegQueryStringValue(HKA, UninstallKey, 'DisplayVersion', Version) then
  begin
    Why := 'no installation entry or no version';
    Exit;
  end;
  if not StrToVersion(Version, V) then
  begin
    Why := 'unreadable version "' + Version + '"';
    Exit;
  end;
  if not RegQueryStringValue(HKA, UninstallKey, 'Inno Setup: App Path', Dir) or (Dir = '') then
  begin
    Why := 'no installation folder recorded';
    Exit;
  end;
  if not FileExists(AddBackslash(Dir) + 'tsw-launcher.exe') then
  begin
    Why := 'the program file is missing in ' + Dir;
    Exit;
  end;
  if not RegQueryStringValue(HKA, UninstallKey, 'UninstallString', UninstallString) or not FileExists(RemoveQuotes(UninstallString)) then
  begin
    Why := 'the uninstaller is missing';
    Exit;
  end;
  Result := True;
end;

function InitializeSetup: Boolean;
var
  Why: String;
  Installed, Mine: Int64;
begin
  Result := True;
  InstallMode := ModeNew;
  if FindInstallation(InstalledVersion, InstalledDir, Why) then
  begin
    StrToVersion(InstalledVersion, Installed);
    StrToVersion('{#AppVersion}', Mine);
    case ComparePackedVersion(Installed, Mine) of
      -1: InstallMode := ModeUpdate;
       0: InstallMode := ModeRepair;
    else
      InstallMode := ModeDowngrade;
    end;
    Log('update mode: existing installation found: installed=' + InstalledVersion + ' new={#AppVersion} folder=' + InstalledDir + ' mode=' +
      IntToStr(InstallMode) + ' (0 new, 1 update, 2 repair, 3 downgrade)');
  end
  else
    Log('update mode: no usable existing installation (' + Why + '): normal installation, mode=0');
  if (InstallMode = ModeDowngrade) and WizardSilent and (ExpandConstant('{param:ALLOWDOWNGRADE|0}') <> '1') then
  begin
    Log('update mode: refused a silent downgrade (give /ALLOWDOWNGRADE=1 to allow it)');
    SuppressibleMsgBox(FmtMessage(CustomMessage('DowngradeRefused'), [InstalledVersion, '{#AppVersion}']), mbError, MB_OK, IDOK);
    Result := False;
  end;
end;

function UpdateMessage: String;
begin
  case InstallMode of
    ModeUpdate: Result := FmtMessage(CustomMessage('UpdateText'), [InstalledVersion, '{#AppVersion}']);
    ModeRepair: Result := FmtMessage(CustomMessage('RepairText'), [InstalledVersion]);
    ModeDowngrade: Result := FmtMessage(CustomMessage('DowngradeText'), [InstalledVersion, '{#AppVersion}']);
  else
    Result := '';
  end;
  if Result <> '' then
    Result := Result + #13#10#13#10 + FmtMessage(CustomMessage('KeepText'), [InstalledDir]);
end;

function UpdateCaption: String;
begin
  case InstallMode of
    ModeRepair: Result := CustomMessage('RepairPageCaption');
    ModeDowngrade: Result := CustomMessage('DowngradePageCaption');
  else
    Result := CustomMessage('UpdatePageCaption');
  end;
end;

{ The running launcher asks "really quit?" before it closes. While the installer replaces the program files it must accept the close request of Inno Setup's
  CloseApplications without asking: the installer leaves a marker file in the launcher's data folder, which the launcher honours for ten minutes (winnative::installer_wants_close).
  Nothing is terminated by force; the game is a separate process. The marker is removed when Setup ends. }
function MarkerFile: String;
begin
  Result := ExpandConstant('{localappdata}\{#DataFolder}\installer-closing');
end;

function PrepareToInstall(var NeedsRestart: Boolean): String;
begin
  Result := '';
  if ForceDirectories(ExtractFileDir(MarkerFile)) then
    SaveStringToFile(MarkerFile, 'installer', False);
end;

procedure DeinitializeSetup;
begin
  DeleteFile(MarkerFile);
end;

procedure InitializeWizard;
begin
  { one compact page that replaces licence, folder, tasks and the ready page of the normal wizard; its heading follows the mode }
  UpdatePage := CreateOutputMsgPage(wpSelectTasks, UpdateCaption, CustomMessage('UpdatePageDescription'), UpdateMessage);
  { an update goes to the folder of the existing installation (unless the folder was given on the command line) }
  if (InstallMode <> ModeNew) and (ExpandConstant('{param:DIR|}') = '') then
    WizardForm.DirEdit.Text := RemoveBackslashUnlessRoot(InstalledDir);
end;

function ShouldSkipPage(PageID: Integer): Boolean;
begin
  Result := False;
  if InstallMode = ModeNew then
    Result := PageID = UpdatePage.ID
  else
    Result := (PageID = wpLicense) or (PageID = wpSelectDir) or (PageID = wpSelectProgramGroup) or (PageID = wpSelectTasks) or (PageID = wpReady);
end;

procedure CurPageChanged(CurPageID: Integer);
begin
  if CurPageID = UpdatePage.ID then
    case InstallMode of
      ModeUpdate: WizardForm.NextButton.Caption := CustomMessage('ButtonUpdate');
      ModeRepair: WizardForm.NextButton.Caption := CustomMessage('ButtonRepair');
      ModeDowngrade: WizardForm.NextButton.Caption := CustomMessage('ButtonDowngrade');
    end;
end;

function NextButtonClick(CurPageID: Integer): Boolean;
begin
  Result := True;
  { a downgrade is never silent: the user must confirm (the default answer is No) }
  if (CurPageID = UpdatePage.ID) and (InstallMode = ModeDowngrade) then
    Result := MsgBox(FmtMessage(CustomMessage('DowngradeConfirm'), [InstalledVersion, '{#AppVersion}']), mbConfirmation, MB_YESNO or MB_DEFBUTTON2) = IDYES;
end;

procedure CurStepChanged(CurStep: TSetupStep);
begin
  if CurStep = ssInstall then
    Log('update mode: installing into ' + ExpandConstant('{app}') + ' (mode=' + IntToStr(InstallMode) + ')');
end;
