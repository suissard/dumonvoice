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
; Windows 10 1809+ (API presse-papiers et UI utilisées par le daemon).
MinVersion=10.0.17763

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

[Code]
// --- Vérification des prérequis avant l'installation --------------------------------------------
// dumonvoice.exe est compilé avec la toolchain MSVC : il a besoin du « Microsoft Visual C++
// Redistributable 2015-2022 (x64) ». S'il manque, l'installeur propose de le télécharger et de
// l'installer automatiquement (confirmation administrateur), ou d'ouvrir le lien officiel.
// Ollama est optionnel (moteur Whisper intégré) : sa présence est seulement signalée.

const
  VCRedistUrl = 'https://aka.ms/vs/17/release/vc_redist.x64.exe';
  OllamaUrl = 'https://ollama.com/download/windows';

var
  DownloadPage: TDownloadWizardPage;
  NeedVCRedist: Boolean;

function VCRedistInstalled: Boolean;
var
  Installed: Cardinal;
begin
  Result := RegQueryDWordValue(HKLM64, 'SOFTWARE\Microsoft\VisualStudio\14.0\VC\Runtimes\x64', 'Installed', Installed) and (Installed = 1);
  if not Result then
    Result := FileExists(ExpandConstant('{sys}\vcruntime140.dll')) and FileExists(ExpandConstant('{sys}\vcruntime140_1.dll')) and FileExists(ExpandConstant('{sys}\msvcp140.dll'));
end;

function OllamaInstalled: Boolean;
begin
  Result := FileExists(ExpandConstant('{localappdata}\Programs\Ollama\ollama.exe')) or FileExists(ExpandConstant('{commonpf64}\Ollama\ollama.exe'));
end;

procedure OpenUrl(const Url: String);
var
  ErrorCode: Integer;
begin
  ShellExec('open', Url, '', '', SW_SHOWNORMAL, ewNoWait, ErrorCode);
end;

function InitializeSetup: Boolean;
var
  Choice: Integer;
begin
  Result := True;
  NeedVCRedist := not VCRedistInstalled;
  if NeedVCRedist and not WizardSilent then
  begin
    Choice := MsgBox('Prérequis manquant : Microsoft Visual C++ Redistributable 2015-2022 (x64).' + #13#10#13#10 +
      'Oui : le télécharger et l''installer automatiquement (≈ 25 Mo, confirmation administrateur).' + #13#10 +
      'Non : ouvrir la page de téléchargement et quitter l''installation.' + #13#10 +
      'Annuler : quitter.' + #13#10#13#10 + VCRedistUrl,
      mbConfirmation, MB_YESNOCANCEL);
    if Choice = IDNO then
      OpenUrl(VCRedistUrl);
    Result := Choice = IDYES;
  end;
end;

procedure InitializeWizard;
begin
  DownloadPage := CreateDownloadPage(SetupMessage(msgWizardPreparing), 'Téléchargement des prérequis…', nil);
end;

function InstallVCRedist: String;
var
  ResultCode: Integer;
begin
  Result := '';
  DownloadPage.Clear;
  DownloadPage.Add(VCRedistUrl, 'vc_redist.x64.exe', '');
  DownloadPage.Show;
  try
    try
      DownloadPage.Download;
    except
      Result := 'Téléchargement du runtime Visual C++ impossible : ' + GetExceptionMessage + #13#10 +
        'Installez-le manuellement : ' + VCRedistUrl;
      Exit;
    end;
  finally
    DownloadPage.Hide;
  end;
  // Le redistribuable s'installe pour toute la machine : élévation via « runas ».
  if not ShellExec('runas', ExpandConstant('{tmp}\vc_redist.x64.exe'), '/install /passive /norestart', '', SW_SHOWNORMAL, ewWaitUntilTerminated, ResultCode) then
    Result := 'Installation du runtime Visual C++ annulée ou impossible : ' + SysErrorMessage(ResultCode)
  // 0 = OK, 1638 = version plus récente déjà présente, 3010 = redémarrage requis.
  else if (ResultCode <> 0) and (ResultCode <> 1638) and (ResultCode <> 3010) then
    Result := 'Le runtime Visual C++ a échoué (code ' + IntToStr(ResultCode) + '). Installez-le manuellement : ' + VCRedistUrl
  else if not VCRedistInstalled then
    Result := 'Le runtime Visual C++ n''est toujours pas détecté. Installez-le manuellement : ' + VCRedistUrl;
end;

function PrepareToInstall(var NeedsRestart: Boolean): String;
begin
  Result := '';
  if NeedVCRedist then
  begin
    if WizardSilent then
      Result := 'Microsoft Visual C++ Redistributable 2015-2022 (x64) est requis : ' + VCRedistUrl
    else
      Result := InstallVCRedist;
  end;
end;

procedure CurStepChanged(CurStep: TSetupStep);
begin
  if (CurStep = ssPostInstall) and not WizardSilent and not OllamaInstalled then
    if MsgBox('Optionnel : Ollama n''est pas installé.' + #13#10 +
      'DumonVoice fonctionne sans (moteur Whisper intégré) ; Ollama ajoute la « Dictée propre » et le post-traitement IA.' + #13#10#13#10 +
      'Ouvrir la page de téléchargement d''Ollama ?', mbInformation, MB_YESNO) = IDYES then
      OpenUrl(OllamaUrl);
end;
