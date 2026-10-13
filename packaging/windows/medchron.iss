; Medchron Windows x64 installer. Run with Inno Setup 6 after cargo build --release --bins.
#ifndef MyAppVersion
  #define MyAppVersion "0.2.6"
#endif
#ifndef MyVersionInfoVersion
  #define MyVersionInfoVersion "0.2.6.0"
#endif

[Setup]
AppId={{1A89619B-AEB9-47C8-A42C-60B22D687B9D}
AppName=Medchron
AppVersion={#MyAppVersion}
AppVerName=Medchron {#MyAppVersion}
VersionInfoVersion={#MyVersionInfoVersion}
AppPublisher=Medchron
AppPublisherURL=https://github.com/TomSellecksMustache/Medchron
AppSupportURL=https://github.com/TomSellecksMustache/Medchron/issues
DefaultDirName={autopf}\Medchron
DefaultGroupName=Medchron
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
PrivilegesRequired=admin
OutputDir=..\..\dist
OutputBaseFilename=Medchron-Setup-v{#MyAppVersion}-windows-x64
SetupIconFile=medchron.ico
UninstallDisplayIcon={app}\medchron.ico
Compression=lzma2
SolidCompression=yes
WizardStyle=modern
DisableWelcomePage=no
CloseApplications=yes

[Languages]
Name: "english"; MessagesFile: "compiler:Default.isl"

[Tasks]
Name: "desktopicon"; Description: "Create a Medchron desktop shortcut"; GroupDescription: "Additional shortcuts:"; Flags: checkedonce

[Dirs]
Name: "{app}\bin"

[Files]
Source: "..\..\target\release\medchron.exe"; DestDir: "{app}\bin"; DestName: "medchron-server.exe"; Flags: ignoreversion
Source: "..\..\target\release\medchron-launcher.exe"; DestDir: "{app}\bin"; Flags: ignoreversion
Source: "medchron.ico"; DestDir: "{app}"; Flags: ignoreversion
Source: "medchron-logo.png"; DestDir: "{app}"; Flags: ignoreversion
Source: "..\..\README.md"; DestDir: "{app}"; Flags: ignoreversion
Source: "..\..\LICENSE"; DestDir: "{app}"; Flags: ignoreversion
Source: "..\..\WINDOWS_INSTALLER.md"; DestDir: "{app}"; Flags: ignoreversion
#include "synthetic-pdfs.iss"
Source: "..\..\SYNTHETIC_RECORDS.md"; DestDir: "{app}"; Flags: ignoreversion

[Icons]
Name: "{group}\Medchron"; Filename: "{app}\bin\medchron-launcher.exe"; WorkingDir: "{app}\bin"; IconFilename: "{app}\medchron.ico"; Comment: "Start Medchron locally and open its browser interface"
Name: "{autodesktop}\Medchron"; Filename: "{app}\bin\medchron-launcher.exe"; WorkingDir: "{app}\bin"; IconFilename: "{app}\medchron.ico"; Tasks: desktopicon; Comment: "Start Medchron locally and open its browser interface"
Name: "{group}\Uninstall Medchron"; Filename: "{uninstallexe}"

[Run]
Filename: "{app}\bin\medchron-launcher.exe"; Description: "Launch Medchron"; Flags: nowait postinstall skipifsilent
