; Installateur d'Atelier (Inno Setup 6).
;
; Installation par utilisateur, sans droits d'administrateur, dans le dossier
; que le programme reconnaît comme le sien : son bouton « Installer » et cet
; installateur aboutissent au même résultat.
;
; Compilation, depuis la racine du dépôt, après `cargo build --release` :
;   ISCC.exe /DAppVersion=0.1.0 installer\atelier.iss
; Résultat : target\installer\atelier-<version>-setup.exe

#define AppName "Atelier"
#define AppExe "atelier.exe"
#define AppRepository "https://github.com/pedrokarim/atelier"
#ifndef AppVersion
  #define AppVersion "0.0.0"
#endif

[Setup]
; Identifiant stable : ne jamais le changer, il relie les mises à jour.
AppId={{72EE76B1-750A-4514-8052-89B3A2644E44}
AppName={#AppName}
AppVersion={#AppVersion}
AppVerName={#AppName} {#AppVersion}
AppPublisher=Ascencia
AppPublisherURL={#AppRepository}
AppSupportURL={#AppRepository}/issues
AppUpdatesURL={#AppRepository}/releases
VersionInfoVersion={#AppVersion}
VersionInfoDescription=Installateur d'Atelier

DefaultDirName={localappdata}\Programs\atelier
DisableDirPage=yes
DisableProgramGroupPage=yes
DisableReadyPage=yes
PrivilegesRequired=lowest
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
MinVersion=10.0.17763

; Atelier tourne en permanence : l'installateur le ferme avant de remplacer
; l'exécutable, et la mise à jour se fait sans redémarrer Windows.
CloseApplications=force
CloseApplicationsFilter={#AppExe}
RestartApplications=no

OutputDir=..\target\installer
OutputBaseFilename=atelier-{#AppVersion}-setup
SetupIconFile=..\assets\atelier.ico
UninstallDisplayIcon={app}\{#AppExe}
UninstallDisplayName={#AppName}
LicenseFile=..\LICENSE
WizardStyle=modern
Compression=lzma2/max
SolidCompression=yes

[Languages]
Name: "french"; MessagesFile: "compiler:Languages\French.isl"

[Tasks]
Name: "autostart"; Description: "Lancer Atelier à l'ouverture de session (indispensable au suivi du temps d'écran)"; GroupDescription: "Démarrage :"
Name: "desktopicon"; Description: "{cm:CreateDesktopIcon}"; GroupDescription: "{cm:AdditionalIcons}"; Flags: unchecked

[Files]
Source: "..\target\release\{#AppExe}"; DestDir: "{app}"; Flags: ignoreversion
Source: "..\LICENSE"; DestDir: "{app}"; DestName: "LICENSE.txt"; Flags: ignoreversion
Source: "..\assets\fonts\OFL.txt"; DestDir: "{app}"; DestName: "OFL-Geist.txt"; Flags: ignoreversion

[Icons]
; Même nom que le raccourci créé par le programme lui-même.
Name: "{userprograms}\Atelier"; Filename: "{app}\{#AppExe}"; Comment: "Widgets et outils du bureau"
Name: "{userdesktop}\Atelier"; Filename: "{app}\{#AppExe}"; Tasks: desktopicon

[Registry]
; Même valeur que l'interrupteur « Lancer à l'ouverture de session ».
Root: HKCU; Subkey: "Software\Microsoft\Windows\CurrentVersion\Run"; ValueType: string; ValueName: "atelier"; ValueData: """{app}\{#AppExe}"""; Tasks: autostart; Flags: uninsdeletevalue

[Run]
Filename: "{app}\{#AppExe}"; Description: "{cm:LaunchProgram,{#AppName}}"; Flags: nowait postinstall skipifsilent

[UninstallRun]
; Le programme réside dans la zone de notification : on l'arrête avant de
; retirer ses fichiers.
Filename: "{sys}\taskkill.exe"; Parameters: "/F /IM {#AppExe}"; Flags: runhidden; RunOnceId: "StopAtelier"

[UninstallDelete]
Type: filesandordirs; Name: "{app}"

[Code]
// L'entrée de démarrage peut avoir été créée depuis le programme plutôt que
// par l'installateur : on la retire dans tous les cas, pour ne pas laisser
// Windows lancer un exécutable disparu.
procedure CurUninstallStepChanged(CurUninstallStep: TUninstallStep);
begin
  if CurUninstallStep = usPostUninstall then
    RegDeleteValue(HKEY_CURRENT_USER, 'Software\Microsoft\Windows\CurrentVersion\Run', 'atelier');
end;