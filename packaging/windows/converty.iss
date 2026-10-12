; Inno Setup script for the Converty installer (Converty-<version>-windows-x64-setup.exe).
; Build: iscc /DAppVersion=0.1.0 /DSourceDir=..\..\target\release packaging\windows\converty.iss

#ifndef AppVersion
  #define AppVersion "0.0.0"
#endif
#ifndef SourceDir
  #define SourceDir "..\..\target\release"
#endif

[Setup]
AppId={{6B0E1D2A-3C55-4B6F-9C0B-7E2F8A9D4C11}
AppName=Converty
AppVersion={#AppVersion}
AppVerName=Converty {#AppVersion}
AppPublisher=Converty contributors
AppPublisherURL=https://github.com/DerKellerPotato/Converty
AppSupportURL=https://github.com/DerKellerPotato/Converty/issues
DefaultDirName={autopf}\Converty
DefaultGroupName=Converty
DisableProgramGroupPage=yes
LicenseFile=..\..\LICENSE
OutputDir=..\..\dist
OutputBaseFilename=Converty-{#AppVersion}-windows-x64-setup
SetupIconFile=converty.ico
UninstallDisplayIcon={app}\converty.exe
Compression=lzma2
SolidCompression=yes
WizardStyle=modern
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
PrivilegesRequiredOverridesAllowed=dialog

[Languages]
Name: "english"; MessagesFile: "compiler:Default.isl"
Name: "german"; MessagesFile: "compiler:Languages\German.isl"

[Tasks]
Name: "desktopicon"; Description: "{cm:CreateDesktopIcon}"; GroupDescription: "{cm:AdditionalIcons}"; Flags: unchecked

[Files]
Source: "{#SourceDir}\converty.exe"; DestDir: "{app}"; Flags: ignoreversion
Source: "..\..\LICENSE"; DestDir: "{app}"; DestName: "LICENSE.txt"
Source: "..\..\README.md"; DestDir: "{app}"

[Icons]
Name: "{autoprograms}\Converty"; Filename: "{app}\converty.exe"
Name: "{autodesktop}\Converty"; Filename: "{app}\converty.exe"; Tasks: desktopicon

[Run]
Filename: "{app}\converty.exe"; Description: "{cm:LaunchProgram,Converty}"; Flags: nowait postinstall skipifsilent
