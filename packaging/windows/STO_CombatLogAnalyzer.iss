; Inno Setup script for the STO_CombatLogAnalyzer Windows installer.
;
; Builds a per-user installer (no admin required) that installs the
; self-contained release .exe under
; %LOCALAPPDATA%\Programs\STO_CombatLogAnalyzer\, and adds an uninstaller.
; The stable AppId means an upgrade replaces the previous install in place
; rather than creating a second copy.
;
; Start Menu / Desktop shortcuts are NOT created by Inno here. Instead the
; app owns its own shortcut logic (the same code used by `cargo install`
; users and on Linux/macOS): the installer runs the exe once with
; --install-desktop after install, and --uninstall-desktop on removal. This
; keeps a single source of truth and avoids duplicate menu entries.
;
; The version is injected from CI:
;
;     iscc /DAppVersion=1.5.0 packaging\windows\STO_CombatLogAnalyzer.iss

#ifndef AppVersion
  #define AppVersion "0.0.0"
#endif

[Setup]
; Anchor relative paths on the repo root, not this .iss file's directory.
SourceDir=..\..
AppId={{2C7B4E1A-9D3F-4A62-B58E-6F1C0A9E7D42}
AppName=STO Combat Log Analyzer
AppVersion={#AppVersion}
AppPublisher=raman78
AppPublisherURL=https://github.com/raman78/STO_CombatLogAnalyzer
AppSupportURL=https://github.com/raman78/STO_CombatLogAnalyzer/issues
DefaultDirName={localappdata}\Programs\STO_CombatLogAnalyzer
DisableProgramGroupPage=yes
PrivilegesRequired=lowest
PrivilegesRequiredOverridesAllowed=dialog
OutputDir=dist\installer
OutputBaseFilename=STO_CombatLogAnalyzer-{#AppVersion}-setup
SetupIconFile=icon\icon.ico
UninstallDisplayIcon={app}\STO_CombatLogAnalyzer.exe
UninstallDisplayName=STO Combat Log Analyzer {#AppVersion}
Compression=lzma2/ultra64
SolidCompression=yes
WizardStyle=modern
ArchitecturesInstallIn64BitMode=x64compatible
ArchitecturesAllowed=x64compatible

[Languages]
Name: "english"; MessagesFile: "compiler:Default.isl"

[Files]
Source: "target\release\STO_CombatLogAnalyzer.exe"; DestDir: "{app}"; Flags: ignoreversion
Source: "icon\icon.ico"; DestDir: "{app}"; Flags: ignoreversion
Source: "Linux_STO_CombatLogAnalyzer\licenses.html"; DestDir: "{app}"; Flags: ignoreversion skipifsourcedoesntexist

[Run]
; Register the Start Menu shortcut via the app's own logic.
Filename: "{app}\STO_CombatLogAnalyzer.exe"; Parameters: "--install-desktop"; Flags: runhidden
Filename: "{app}\STO_CombatLogAnalyzer.exe"; Description: "Launch STO Combat Log Analyzer"; Flags: nowait postinstall skipifsilent

[UninstallRun]
; Remove the shortcut before the exe itself is deleted.
Filename: "{app}\STO_CombatLogAnalyzer.exe"; Parameters: "--uninstall-desktop"; Flags: runhidden; RunOnceId: "RemoveDesktopEntry"
