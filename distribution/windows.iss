#ifndef AppVersion
#error AppVersion must be provided from the validated release version
#endif
#ifndef YotoAppId
#error YotoAppId must be provided from distribution/windows-app-id.txt
#endif
; The AppId never changes: it is what makes this an upgrade of an install made under the
; former name "Yoto", and the in-app updater writes to the uninstall entry it names.
[Setup]
AppId={{{#YotoAppId}}
AppName=YotoMediaManager
AppVersion={#AppVersion}
DefaultDirName={code:InstallFolder}
UsePreviousAppDir=no
DefaultGroupName=YotoMediaManager
UsePreviousGroup=no
PrivilegesRequired=lowest
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
OutputDir=..\dist\x86_64-pc-windows-msvc
OutputBaseFilename=YotoMediaManager-{#AppVersion}-x64-setup
SetupIconFile=..\assets\branding\app-icon.ico
UninstallDisplayIcon={app}\yoto-desktop.exe
Compression=lzma2
SolidCompression=yes
[Files]
Source: "..\target\x86_64-pc-windows-msvc\release\yoto-desktop.exe"; DestDir: "{app}"
Source: "..\target\x86_64-pc-windows-msvc\release\yoto-mcp.exe"; DestDir: "{app}"
Source: "..\tools\x86_64-pc-windows-msvc\*"; DestDir: "{app}\tools"; Flags: recursesubdirs createallsubdirs
[Icons]
Name: "{group}\YotoMediaManager"; Filename: "{app}\yoto-desktop.exe"
[Run]
Filename: "{app}\yoto-desktop.exe"; Description: "Ouvrir YotoMediaManager"; Flags: nowait postinstall skipifsilent
[Code]
var
  PreviousGroup: String;

// A setting of the installed version, read from its uninstall entry ('' when absent).
function PreviousSetting(Name: String): String;
begin
  if not RegQueryStringValue(HKCU,
    'Software\Microsoft\Windows\CurrentVersion\Uninstall\{' + '{#YotoAppId}' + '}_is1',
    Name, Result) then
    Result := '';
end;

function LegacyFolder: String;
begin
  Result := ExpandConstant('{localappdata}\Programs\Yoto');
end;

// A default "Programs\Yoto" install moves to "Programs\YotoMediaManager"; a folder the
// person chose themselves is kept, since it may hold more than this app.
function InstallFolder(Param: String): String;
var
  Previous: String;
begin
  Previous := RemoveBackslashUnlessRoot(PreviousSetting('Inno Setup: App Path'));
  if (Previous = '') or SameText(Previous, LegacyFolder) then
    Result := ExpandConstant('{localappdata}\Programs\YotoMediaManager')
  else
    Result := Previous;
end;

function InitializeSetup: Boolean;
begin
  // Read before this install rewrites the uninstall entry.
  PreviousGroup := PreviousSetting('Inno Setup: Icon Group');
  Result := True;
end;

// Once the new copy is in place, remove the former default folder and shortcut. Only that
// exact folder, and only while it still holds the app, is deleted; the library lives in
// {localappdata}\Yoto and is not touched. A folder still in use is retried next install.
procedure CurStepChanged(CurStep: TSetupStep);
var
  Shortcuts: String;
begin
  if CurStep <> ssPostInstall then
    Exit;
  if not SameText(RemoveBackslashUnlessRoot(ExpandConstant('{app}')), LegacyFolder)
    and FileExists(LegacyFolder + '\yoto-desktop.exe') then
  begin
    if not DelTree(LegacyFolder, True, True, True) then
      Log('Could not fully remove the former install folder ' + LegacyFolder);
  end;
  if SameText(PreviousGroup, 'Yoto') then
  begin
    Shortcuts := ExpandConstant('{userprograms}\Yoto');
    DeleteFile(Shortcuts + '\Yoto.lnk');
    RemoveDir(Shortcuts);
  end;
end;
