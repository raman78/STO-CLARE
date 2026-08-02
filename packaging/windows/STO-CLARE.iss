; Inno Setup script for the STO-CLARE Windows installer.
;
; Builds a per-user installer (no admin required) that installs the
; self-contained release .exe under
; %LOCALAPPDATA%\Programs\STO-CLARE\, and adds an uninstaller.
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
;     iscc /DAppVersion=2.0.0 packaging\windows\STO-CLARE.iss

#ifndef AppVersion
  #define AppVersion "0.0.0"
#endif

[Setup]
; Anchor relative paths on the repo root, not this .iss file's directory.
SourceDir=..\..
AppId={{2C7B4E1A-9D3F-4A62-B58E-6F1C0A9E7D42}
AppName=STO-CLARE
AppVersion={#AppVersion}
AppPublisher=raman78
AppPublisherURL=https://github.com/raman78/STO-CLARE
AppSupportURL=https://github.com/raman78/STO-CLARE/issues
DefaultDirName={localappdata}\Programs\STO-CLARE
DisableProgramGroupPage=yes
PrivilegesRequired=lowest
PrivilegesRequiredOverridesAllowed=dialog
OutputDir=dist\installer
OutputBaseFilename=STO-CLARE-{#AppVersion}-setup
SetupIconFile=icon\icon.ico
UninstallDisplayIcon={app}\sto-clare.exe
UninstallDisplayName=STO-CLARE {#AppVersion}
Compression=lzma2/ultra64
SolidCompression=yes
WizardStyle=modern
ArchitecturesInstallIn64BitMode=x64compatible
ArchitecturesAllowed=x64compatible

[Languages]
Name: "english"; MessagesFile: "compiler:Default.isl"

[InstallDelete]
; The AppId is unchanged across the rename, so installing over a 1.8.x install
; upgrades it in place — into the same folder, where the old executable would
; otherwise sit forever under its old name. Removing it here is also what lets
; the app clean up the Start Menu shortcut of that old name on first run.
Type: files; Name: "{app}\STO_CombatLogAnalyzer.exe"

[Files]
Source: "target\release\sto-clare.exe"; DestDir: "{app}"; Flags: ignoreversion
Source: "icon\icon.ico"; DestDir: "{app}"; Flags: ignoreversion
Source: "licenses.html"; DestDir: "{app}"; Flags: ignoreversion skipifsourcedoesntexist

[Run]
; Register the Start Menu shortcut via the app's own logic.
Filename: "{app}\sto-clare.exe"; Parameters: "--install-desktop"; Flags: runhidden
Filename: "{app}\sto-clare.exe"; Description: "Launch STO-CLARE"; Flags: nowait postinstall skipifsilent

[UninstallRun]
; Remove the shortcut before the exe itself is deleted.
Filename: "{app}\sto-clare.exe"; Parameters: "--uninstall-desktop"; Flags: runhidden; RunOnceId: "RemoveDesktopEntry"
