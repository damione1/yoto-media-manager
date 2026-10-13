#define AppVersion "0.1.0"
[Setup]
AppId={{DA05F87D-145F-413D-80E4-C3C61D448D5B}
AppName=Yoto
AppVersion={#AppVersion}
DefaultDirName={localappdata}\Programs\Yoto
DefaultGroupName=Yoto
PrivilegesRequired=lowest
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
OutputDir=..\dist\x86_64-pc-windows-msvc
OutputBaseFilename=Yoto-{#AppVersion}-x64-setup
SetupIconFile=..\assets\branding\app-icon.ico
UninstallDisplayIcon={app}\yoto-desktop.exe
Compression=lzma2
SolidCompression=yes
[Files]
Source: "..\target\x86_64-pc-windows-msvc\release\yoto-desktop.exe"; DestDir: "{app}"
Source: "..\target\x86_64-pc-windows-msvc\release\yoto-mcp.exe"; DestDir: "{app}"
Source: "..\tools\x86_64-pc-windows-msvc\*"; DestDir: "{app}\tools"; Flags: recursesubdirs createallsubdirs
[Icons]
Name: "{group}\Yoto"; Filename: "{app}\yoto-desktop.exe"
[Run]
Filename: "{app}\yoto-desktop.exe"; Description: "Ouvrir Yoto"; Flags: nowait postinstall skipifsilent
