; Installer for Gig Control Center.
;
; Built by .github/workflows/release.yml on every tag, and runnable by hand —
; the command is in cmds.md.
;
; Per-user by design. The app keeps settings.json next to its own exe (see
; config_dir in src/settings.rs) and nothing in it ever elevates, so a
; machine-wide install under Program Files would leave every settings save
; failing — silently, because #![windows_subsystem = "windows"] means there is
; no console for the warning to reach. PrivilegesRequired=lowest puts {autopf}
; in %LOCALAPPDATA%\Programs, which the user can write, and gets there without
; a UAC prompt.

#ifndef MyAppVersion
  #error Pass the version in, e.g. ISCC /DMyAppVersion=1.0.0
#endif

#ifndef SourceExe
  #define SourceExe "..\target\release\gig-center.exe"
#endif

#define MyAppName "Gig Control Center"
#define MyAppPublisher "Merbac"
#define MyAppURL "https://github.com/mercibac/gig-center"
#define MyAppExeName "Gig Control Center.exe"

[Setup]
; Generated once, and never to be changed: Windows identifies the installed
; product by this, so a different one would install v2 beside v1 rather than
; over it.
AppId={{B017BB90-6109-43F9-A43D-3A44838A1302}
AppName={#MyAppName}
AppVersion={#MyAppVersion}
AppVerName={#MyAppName} {#MyAppVersion}
AppPublisher={#MyAppPublisher}
AppPublisherURL={#MyAppURL}
AppSupportURL={#MyAppURL}/issues
AppUpdatesURL={#MyAppURL}/releases
VersionInfoVersion={#MyAppVersion}

; The install directory must not be named "debug" or "release" — config_dir()
; reads either as a cargo run and goes looking in the working directory for
; settings.json instead of beside the exe.
DefaultDirName={autopf}\{#MyAppName}
DefaultGroupName={#MyAppName}
DisableProgramGroupPage=yes
PrivilegesRequired=lowest
PrivilegesRequiredOverridesAllowed=dialog

LicenseFile=..\LICENSE
SetupIconFile=..\assets\icon.ico
UninstallDisplayIcon={app}\{#MyAppExeName}
UninstallDisplayName={#MyAppName}

OutputDir=..\dist
OutputBaseFilename=Gig-Control-Center-v{#MyAppVersion}-installer
Compression=lzma2/max
SolidCompression=yes
WizardStyle=modern

[Languages]
Name: "english"; MessagesFile: "compiler:Default.isl"

[Tasks]
Name: "desktopicon"; Description: "{cm:CreateDesktopIcon}"; GroupDescription: "{cm:AdditionalIcons}"; Flags: unchecked

[Files]
; One file, because the build is self-contained: the UI, the icons and the
; screen-match templates are all compiled in. settings.json is deliberately
; not here — the app writes its own defaults on first save, and shipping one
; would plant whoever built the release's devices and preset paths on every
; machine that installed it.
Source: "{#SourceExe}"; DestDir: "{app}"; DestName: "{#MyAppExeName}"; Flags: ignoreversion

[Icons]
Name: "{group}\{#MyAppName}"; Filename: "{app}\{#MyAppExeName}"
Name: "{autodesktop}\{#MyAppName}"; Filename: "{app}\{#MyAppExeName}"; Tasks: desktopicon

[Run]
Filename: "{app}\{#MyAppExeName}"; Description: "{cm:LaunchProgram,{#StringChange(MyAppName, '&', '&&')}}"; Flags: nowait postinstall skipifsilent

; No [UninstallDelete]. Uninstalling removes what was installed and leaves
; settings.json where it is, because it is the user's configuration and not
; ours to discard — the folder stays behind holding it, which is the usual
; trade for not throwing someone's setup away on an upgrade.
