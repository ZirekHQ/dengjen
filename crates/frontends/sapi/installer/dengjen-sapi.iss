; Inno Setup script for the Dengjen SAPI5 voice: installs the DLL, ONNX Runtime, the eSpeak-ng
; data and the voices under Program Files, then registers them machine-wide so the voice also
; works on the lock screen and in UAC prompts. Built by .github/workflows/sapi.yaml:
;   ISCC /DAppVersion=<x.y.z> /DStage=<staged files> /DRedist=<vc_redist.x64.exe> /O<out dir> dengjen-sapi.iss

#ifndef AppVersion
  #define AppVersion "0.0.0"
#endif
#ifndef Stage
  #error Pass /DStage=<dir with dengjen_sapi.dll, onnxruntime.dll, espeak-ng-data and voices>
#endif
#ifndef Redist
  #error Pass /DRedist=<path to vc_redist.x64.exe>
#endif

[Setup]
; A fixed AppId keeps upgrades and the uninstaller pointing at the same installation.
AppId={{6F1D2A0B-3C64-4F7E-9B8A-D3E5C1F0A217}
AppName=Dengjen SAPI5 voice
AppVersion={#AppVersion}
AppPublisher=Zirek
AppPublisherURL=https://github.com/ZirekHQ
AppSupportURL=https://github.com/ZirekHQ/dengjen/issues
DefaultDirName={autopf}\Dengjen\SAPI5 voice
DisableDirPage=yes
DisableProgramGroupPage=yes
PrivilegesRequired=admin
; 64-bit only: Microsoft ships no 32-bit ONNX Runtime after 1.22 (see #573).
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
MinVersion=10.0
OutputBaseFilename=dengjen-sapi-{#AppVersion}-x64-setup
Compression=lzma2/max
SolidCompression=yes
WizardStyle=modern
LicenseFile={#Stage}\LICENSE
UninstallDisplayName=Dengjen SAPI5 voice
; Narrator and screen readers keep the DLL loaded; Setup offers to close them, and anything still
; holding a file is replaced or removed on the next restart.
CloseApplications=yes
RestartIfNeededByRun=no

[Files]
Source: "{#Stage}\onnxruntime.dll"; DestDir: "{app}"; Flags: ignoreversion restartreplace uninsrestartdelete
Source: "{#Stage}\espeak-ng-data\*"; DestDir: "{app}\espeak-ng-data"; Flags: ignoreversion recursesubdirs createallsubdirs
Source: "{#Stage}\voices\*"; DestDir: "{app}\voices"; Flags: ignoreversion recursesubdirs createallsubdirs
Source: "{#Stage}\LICENSE"; DestDir: "{app}"; Flags: ignoreversion
; Registered after every file is in place: DllRegisterServer adds one token per voice it finds
; beside the DLL, and the uninstaller calls DllUnregisterServer before deleting anything.
Source: "{#Stage}\dengjen_sapi.dll"; DestDir: "{app}"; Flags: ignoreversion regserver restartreplace uninsrestartdelete
; Installed from PrepareToInstall, not [Run]: registration loads the DLL, which needs the runtime,
; and [Run] entries only start after registration.
Source: "{#Redist}"; DestName: "vc_redist.x64.exe"; Flags: dontcopy

[UninstallDelete]
Type: dirifempty; Name: "{app}"
Type: dirifempty; Name: "{autopf}\Dengjen"

[Code]
const
  VCRuntimeKey = 'SOFTWARE\Microsoft\VisualStudio\14.0\VC\Runtimes\x64';

{ The runtime the DLLs were built against: 14.40 or later (Visual Studio 2022 17.10). ONNX
  Runtime requires at least that, and older runtimes crash it on load. }
function NeedsVCRedist: Boolean;
var
  Installed, Major, Minor: Cardinal;
begin
  Result := True;
  if RegQueryDWordValue(HKLM64, VCRuntimeKey, 'Installed', Installed) and (Installed = 1)
    and RegQueryDWordValue(HKLM64, VCRuntimeKey, 'Major', Major)
    and RegQueryDWordValue(HKLM64, VCRuntimeKey, 'Minor', Minor) then
    Result := (Major < 14) or ((Major = 14) and (Minor < 40));
end;

{ dengjen_sapi.dll and onnxruntime.dll need the Visual C++ runtime, so it goes in before any file
  is copied or registered. }
function PrepareToInstall(var NeedsRestart: Boolean): String;
var
  ResultCode: Integer;
begin
  Result := '';
  if not NeedsVCRedist then
    Exit;
  ExtractTemporaryFile('vc_redist.x64.exe');
  if not Exec(ExpandConstant('{tmp}\vc_redist.x64.exe'), '/install /quiet /norestart', '',
    SW_HIDE, ewWaitUntilTerminated, ResultCode) then
    Result := 'Could not start the Visual C++ runtime installer: ' + SysErrorMessage(ResultCode)
  { 3010: installed, but the new runtime is only in place after a restart, so registering the
    voice now could load the old one. Inno Setup acts on NeedsRestart only when Result is not
    empty: it stops here, offers the restart, and Setup is run again afterwards.
    1638: a newer version is already there. }
  else if ResultCode = 3010 then
  begin
    NeedsRestart := True;
    Result := 'The Visual C++ runtime was installed and Windows needs to restart to finish. ' +
      'Restart, then run this setup again.';
  end
  else if (ResultCode <> 0) and (ResultCode <> 1638) then
    Result := 'The Visual C++ runtime installer failed with exit code ' + IntToStr(ResultCode) + '.';
end;
