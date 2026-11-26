; Installeur DumonVoice (Inno Setup 6) — installation dans l'espace utilisateur.
#ifndef AppVersion
  #define AppVersion "0.1.0"
#endif

[Setup]
AppId={{6E0C1B57-3B2E-4F0A-9C55-6A3D2F1B9E41}
AppName=DumonVoice
AppVersion={#AppVersion}
AppPublisher=DumonVoice
DefaultDirName={localappdata}\Programs\DumonVoice
DisableDirPage=yes
DisableProgramGroupPage=yes
PrivilegesRequired=lowest
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
OutputDir=Output
OutputBaseFilename=DumonVoice-Setup-{#AppVersion}
Compression=lzma2/ultra
SolidCompression=yes
WizardStyle=modern
UninstallDisplayIcon={app}\dumonvoice.exe
CloseApplications=yes

[Languages]
Name: "french"; MessagesFile: "compiler:Languages\French.isl"
Name: "english"; MessagesFile: "compiler:Default.isl"

[Tasks]
Name: "autostart"; Description: "Lancer DumonVoice au démarrage de Windows"; Flags: checkedonce
Name: "firewall"; Description: "Autoriser le compagnon téléphone sur le réseau privé (pare-feu Windows, confirmation administrateur)"; Flags: checkedonce

[Files]
Source: "..\daemon\target\release\dumonvoice.exe"; DestDir: "{app}"; Flags: ignoreversion
Source: "..\daemon\target\release\*.dll"; DestDir: "{app}"; Flags: ignoreversion skipifsourcedoesntexist
Source: "..\README.md"; DestDir: "{app}"; Flags: ignoreversion

[Icons]
Name: "{userprograms}\DumonVoice"; Filename: "{app}\dumonvoice.exe"
Name: "{userprograms}\Désinstaller DumonVoice"; Filename: "{uninstallexe}"

[Run]
; Choix du démarrage automatique enregistré dans config.json + clé Run (HKCU).
Filename: "{app}\dumonvoice.exe"; Parameters: "--autostart=1 --configure-only"; Flags: runhidden waituntilterminated; Tasks: autostart
Filename: "{app}\dumonvoice.exe"; Parameters: "--autostart=0 --configure-only"; Flags: runhidden waituntilterminated; Tasks: not autostart
; Règles pare-feu limitées au profil Privé (HTTPS mobile + mDNS).
Filename: "{sys}\netsh.exe"; Parameters: "advfirewall firewall add rule name=""DumonVoice Mobile"" dir=in action=allow protocol=TCP localport=18451 profile=private program=""{app}\dumonvoice.exe"""; Flags: runhidden shellexec waituntilterminated; Verb: runas; Tasks: firewall; StatusMsg: "Configuration du pare-feu…"
Filename: "{sys}\netsh.exe"; Parameters: "advfirewall firewall add rule name=""DumonVoice mDNS"" dir=in action=allow protocol=UDP localport=5353 profile=private program=""{app}\dumonvoice.exe"""; Flags: runhidden shellexec waituntilterminated; Verb: runas; Tasks: firewall
Filename: "{app}\dumonvoice.exe"; Description: "Lancer DumonVoice"; Flags: nowait postinstall skipifsilent

[UninstallRun]
Filename: "{cmd}"; Parameters: "/C taskkill /IM dumonvoice.exe /F"; Flags: runhidden; RunOnceId: "kill"
Filename: "{sys}\netsh.exe"; Parameters: "advfirewall firewall delete rule name=""DumonVoice Mobile"""; Flags: runhidden shellexec; Verb: runas; RunOnceId: "fw1"
Filename: "{sys}\netsh.exe"; Parameters: "advfirewall firewall delete rule name=""DumonVoice mDNS"""; Flags: runhidden shellexec; Verb: runas; RunOnceId: "fw2"

[Registry]
Root: HKCU; Subkey: "Software\Microsoft\Windows\CurrentVersion\Run"; ValueName: "DumonVoice"; Flags: uninsdeletevalue dontcreatekey
