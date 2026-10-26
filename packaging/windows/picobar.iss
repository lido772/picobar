; PicoBar installer (Inno Setup 6). Built in CI:
;   ISCC.exe /DVersion=0.2.0 /DSource=path\to\PicoBar.exe packaging\windows\picobar.iss
; Installs per user (no admin rights), so the built-in updater can replace
; PicoBar.exe in place.

#ifndef Version
  #define Version "0.0.0"
#endif
#ifndef Source
  #define Source "..\..\target\release\picobar.exe"
#endif

[Setup]
AppId={{6F1C6B2E-4C1B-4C87-9E0F-6C2B7A1D5E3A}
AppName=PicoBar
AppVersion={#Version}
AppPublisher=Walid Ben Miled
AppPublisherURL=https://github.com/lido772/picobar
DefaultDirName={localappdata}\Programs\PicoBar
DefaultGroupName=PicoBar
DisableProgramGroupPage=yes
PrivilegesRequired=lowest
OutputDir=.
OutputBaseFilename=PicoBar-{#Version}-windows-x64-setup
Compression=lzma2/max
SolidCompression=yes
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
UninstallDisplayName=PicoBar
WizardStyle=modern
CloseApplications=yes

[Files]
Source: "{#Source}"; DestDir: "{app}"; DestName: "PicoBar.exe"; Flags: ignoreversion

[Icons]
Name: "{userprograms}\PicoBar"; Filename: "{app}\PicoBar.exe"

[Registry]
Root: HKCU; Subkey: "Software\Microsoft\Windows\CurrentVersion\Run"; ValueType: string; ValueName: "PicoBar"; ValueData: """{app}\PicoBar.exe"""; Tasks: startup

[InstallDelete]
Type: files; Name: "{userstartup}\PicoBar.lnk"

[Tasks]
Name: "startup"; Description: "Start PicoBar when I sign in"

[Run]
Filename: "{app}\PicoBar.exe"; Description: "Start PicoBar"; Flags: nowait postinstall skipifsilent

[UninstallRun]
Filename: "{sys}\taskkill.exe"; Parameters: "/IM PicoBar.exe /F"; Flags: runhidden; RunOnceId: "StopPicoBar"

[Code]
procedure CurUninstallStepChanged(CurUninstallStep: TUninstallStep);
begin
  if CurUninstallStep = usUninstall then
    RegDeleteValue(HKCU, 'Software\Microsoft\Windows\CurrentVersion\Run', 'PicoBar');
end;
