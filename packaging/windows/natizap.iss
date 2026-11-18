; The Windows installer. Built by the release workflow:
;   iscc /DVersion=0.1.0 /DArch=x86_64 packaging\windows\natizap.iss
; Installs for the current user only, so natizap can update itself
; without asking for an administrator.

#ifndef Version
  #error Pass the version: iscc /DVersion=x.y.z
#endif
#ifndef Arch
  #error Pass the architecture: iscc /DArch=x86_64 or /DArch=arm64
#endif
#if Arch == "arm64"
  #define Allowed "arm64"
#else
  #define Allowed "x64compatible"
#endif

[Setup]
AppId={{6D355D5E-27AB-423B-8C70-B3642DABEAA1}
AppName=natizap
AppVersion={#Version}
AppPublisher=natizap
AppPublisherURL=https://github.com/Murilinho145SG/natizap
AppSupportURL=https://github.com/Murilinho145SG/natizap/issues
AppUpdatesURL=https://github.com/Murilinho145SG/natizap/releases
DefaultDirName={localappdata}\Programs\natizap
DefaultGroupName=natizap
DisableProgramGroupPage=yes
PrivilegesRequired=lowest
ArchitecturesAllowed={#Allowed}
ArchitecturesInstallIn64BitMode={#Allowed}
LicenseFile=..\..\LICENSE
OutputDir=..\..\dist
OutputBaseFilename=natizap-{#Version}-{#Arch}-setup
SetupIconFile=natizap.ico
UninstallDisplayIcon={app}\natizap.ico
Compression=lzma2
SolidCompression=yes
WizardStyle=modern
; An update runs while natizap is open: close it, and open it again after.
CloseApplications=force
RestartApplications=no

[Files]
Source: "..\..\target\release\natizap.exe"; DestDir: "{app}"; Flags: ignoreversion
Source: "natizap.ico"; DestDir: "{app}"; Flags: ignoreversion
Source: "..\..\LICENSE"; DestDir: "{app}"; DestName: "LICENSE.txt"

[Icons]
Name: "{autoprograms}\natizap"; Filename: "{app}\natizap.exe"; IconFilename: "{app}\natizap.ico"
Name: "{autodesktop}\natizap"; Filename: "{app}\natizap.exe"; IconFilename: "{app}\natizap.ico"; Tasks: desktopicon

[Tasks]
Name: "desktopicon"; Description: "{cm:CreateDesktopIcon}"; GroupDescription: "{cm:AdditionalIcons}"

[Run]
; Without `skipifsilent`, so an update installed silently opens natizap again.
Filename: "{app}\natizap.exe"; Description: "{cm:LaunchProgram,natizap}"; Flags: nowait postinstall
