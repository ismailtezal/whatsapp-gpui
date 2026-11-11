; Installer for whatsapp-gpui (Inno Setup 6).
; Installs for the current user only, so it needs no administrator rights.
; Build: ISCC.exe installer\whatsapp-gpui.iss   (after cargo build --release)

#define AppVersion GetEnv("APP_VERSION")
#if AppVersion == ""
  #define AppVersion "0.1.0"
#endif

[Setup]
AppId={{8E6C2F1A-5B7D-4C39-9A21-3F0D6E4B7C11}
AppName=whatsapp-gpui
AppVersion={#AppVersion}
AppPublisher=whatsapp-gpui
DefaultDirName={localappdata}\Programs\whatsapp-gpui
DisableProgramGroupPage=yes
PrivilegesRequired=lowest
OutputDir=..\dist
OutputBaseFilename=whatsapp-gpui-setup-{#AppVersion}
Compression=lzma2
SolidCompression=yes
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
SetupIconFile=..\assets\app.ico
UninstallDisplayIcon={app}\whatsapp-gpui.exe
WizardStyle=modern
CloseApplications=yes

[Tasks]
Name: "desktopicon"; Description: "Create a desktop shortcut"; Flags: unchecked
Name: "startup"; Description: "Start with Windows"; Flags: unchecked

[Files]
Source: "..\target\release\whatsapp-gpui.exe"; DestDir: "{app}"; Flags: ignoreversion

[Icons]
Name: "{autoprograms}\whatsapp-gpui"; Filename: "{app}\whatsapp-gpui.exe"
Name: "{autodesktop}\whatsapp-gpui"; Filename: "{app}\whatsapp-gpui.exe"; Tasks: desktopicon
Name: "{userstartup}\whatsapp-gpui"; Filename: "{app}\whatsapp-gpui.exe"; Tasks: startup

[Run]
Filename: "{app}\whatsapp-gpui.exe"; Description: "Start whatsapp-gpui"; Flags: nowait postinstall skipifsilent
