# DumonVoice

**Dictée vocale universelle 100 % locale pour Windows, avec compagnon mobile (PWA).**
Dictez ou envoyez du texte dans *n'importe quelle* application Windows active — depuis le micro du PC,
le micro du téléphone, ou le clavier / la dictée native du téléphone. *Install & Forget* : aucun compte,
aucun serveur tiers, tout reste sur votre réseau local.

> Implémentation du [cahier des charges](cahier_des_charges_outil_de_dict_e_vocale_windows.md).

## Fonctionnalités

| Domaine | Ce qui est livré |
| :--- | :--- |
| **Déclencheurs PC** | Raccourci global via hook `WH_KEYBOARD_LL` (Push-to-Talk ou Basculer, arrêt auto sur silence), touches uniques (Ctrl droit, AltGr, Verr. Défil, F1-F24, boutons latéraux de souris via `WH_MOUSE_LL`), capture « Cliquer pour définir la touche », détection des conflits avec les raccourcis système critiques, `Échap` pour annuler. |
| **Mot d'éveil** | OpenWakeWord (ONNX Runtime) : `hey_jarvis`, `alexa`, `hey_mycroft` téléchargés automatiquement + import de modèles `.onnx` personnalisés (ex. « Hey Dictée »). Arrêt automatique par VAD (0,8-3 s), pause automatique pendant les appels visio (`IAudioSessionManager2`). |
| **Compagnon mobile** | PWA `/remote` appairée par QR code (code 32 octets, 120 s, usage unique ; jeton d'appareil stocké haché SHA-256). Mode **Micro PC** (AudioWorklet → PCM 16 kHz S16LE en paquets de 40 ms sur `wss://`, Push-to-Talk / Toggle, glisser pour annuler, Wake Lock, haptique) et mode **Clavier / Dictée native** (`Envoyer`, `↵ Auto`, `✨ IA`, `📋 Coller & Envoyer`, historique, anti-perte de brouillon, ancrage au-dessus du clavier virtuel). |
| **Moteurs** | Whisper intégré (`whisper-rs`, profils Rapide / Équilibré / Haute précision, téléchargement 1 clic avec progression, GPU via features `whisper-cuda` / `whisper-vulkan`) et Ollama (audio multimodal type `lfm3-audio`, mode « Dictée propre », post-traitement texte `refine_text`, `keep_alive`, `ollama pull` avec progression, repli automatique sur Whisper). |
| **Injection** | Presse-papiers sécurisé (`CF_UNICODETEXT` + `ExcludeClipboardContentFromMonitorProcessing`, `CanIncludeInClipboardHistory=0`, `CanUploadToCloudClipboard=0`), `Ctrl+V` / `Ctrl+Maj+V` / `Maj+Inser` selon le terminal, `VK_RETURN` optionnel, restauration du presse-papiers d'origine ; `SendInput` Unicode (paires de substitution). Bascule forcée sur le presse-papiers au-delà de 200 caractères, multi-lignes ou emojis. |
| **Robustesse** | FSM à verrou de source exclusif (`BUSY`), idempotence par `message_id` (60 s), watchdog de flux mobile (2 s), heartbeat WS, ring buffer 500 ms (pré-roll 300 ms), filtre Silero VAD (< 250 ms ignoré) + filtre d'hallucinations Whisper, fenêtre Admin (UIPI) → copie + notification, changement de fenêtre → copie, changement de micro à chaud, sortie de veille (`WM_POWERBROADCAST`), overlay masqué en plein écran exclusif, PKI locale régénérée au changement d'IP, mDNS `.local`. |
| **Interface** | HUD Win32 non activable (`WS_EX_NOACTIVATE | TOPMOST | TRANSPARENT | TOOLWINDOW`) avec onde audio, System Tray, sons synthétisés, *Audio Ducking*, PWA de réglages (4 onglets + « Avancé »), assistant de premier lancement en 3 étapes, page guidée `/setup-ca`. |

## Architecture

```
daemon/          Binaire Windows (Rust)
  src/app.rs           Assemblage, application à chaud de la configuration
  src/fsm.rs           Orchestrateur FSM (verrou, idempotence, watchdog, historique)
  src/config.rs        config.json (%APPDATA%\DumonVoice) — écriture atomique, JSON Merge Patch
  src/protocol.rs      Contrat WebSocket / REST (section 6.4)
  src/platform.rs      Abstraction plateforme + règles d'injection testables
  src/hotkey_keys.rs   Table des touches, validation, automate PTT/Toggle
  src/audio/           Ring buffer, rééchantillonnage, WAV, capture (cpal/WASAPI), Silero VAD, OpenWakeWord
  src/engines/         Trait TranscriptionEngine, Whisper, Ollama, post-traitement
  src/server/          Double listener (HTTP 127.0.0.1:18450 + HTTPS 0.0.0.0:18451), TLS rcgen, mDNS, auth, REST, WS
  src/windows_os/      Hook clavier/souris, injecteur, overlay, tray, sessions audio, sons, état système
pwa/             Svelte 5 + TypeScript + Vite + Tailwind (≈ 55 Ko gzip, embarquée via rust-embed)
  public/pcm-processor.js   AudioWorklet 16 kHz S16LE
  src/lib/                  wsClient (reconnexion, heartbeat, ACK), mobileAudio, api, store
  src/views/                DesktopSettings, MobileRemote, OnboardingWizard, SetupCa
installer/       Script Inno Setup (installation utilisateur, règles pare-feu profil Privé)
scripts/         build-windows.ps1, firewall-rule.ps1
```

## Compiler

Prérequis Windows : [Rust](https://rustup.rs) (toolchain MSVC), Node.js 20+, CMake, LLVM (`libclang`, pour
`whisper-rs`), [Inno Setup 6](https://jrsoftware.org/isinfo.php) pour l'installeur.

```powershell
.\scripts\build-windows.ps1            # PWA + daemon + installeur
.\scripts\build-windows.ps1 -Cuda      # avec accélération NVIDIA
```

Étapes manuelles :

```bash
cd pwa && npm ci && npm run build      # PWA -> pwa/dist (embarquée dans le .exe)
cd daemon && cargo build --release     # daemon/target/release/dumonvoice.exe
```

Features Cargo : `whisper` (défaut), `onnx` (défaut : Silero VAD + mot d'éveil), `whisper-cuda`, `whisper-vulkan`.
`cargo build --no-default-features` produit un binaire sans moteurs natifs (Ollama uniquement, VAD énergétique).

La CI GitHub Actions (`.github/workflows/ci.yml`) vérifie la PWA (svelte-check, vitest, build), lance les tests
Rust sous Linux et Windows, compile le `.exe`, effectue un test de fumée (démarrage + API protégée) et produit
l'installeur en artefact.

## Tests

```bash
cd pwa && npm test                     # client WS (ACK, backoff, heartbeat, révocation), AudioWorklet, utilitaires
cd daemon && cargo test                # FSM, idempotence, watchdog, auth/appairage, PKI, config, raccourcis, injection…
```

Tests sur modèles réels (ignorés par défaut) : Silero VAD, OpenWakeWord et Whisper sur de la parole réelle.

```bash
DV_MODELS_DIR=<dossier models> DV_SPEECH_WAV=parole.wav DV_WAKE_WAV=hey_jarvis.wav \
  cargo test --release -- --include-ignored
```

Test de bout en bout du daemon (local + HTTPS LAN + WebSocket : appairage, texte direct, idempotence,
BUSY, dictée audio streamée transcrite, watchdog, révocation) :

```bash
DUMONVOICE_HOME=/tmp/dv ./target/release/dumonvoice --background &
NODE_EXTRA_CA_CERTS=/tmp/dv/certs/ca.crt node tests/e2e/e2e.mjs /tmp/dv parole.wav
```

### État des vérifications

| Vérification | Résultat |
| :--- | :--- |
| PWA : `svelte-check`, 13 tests vitest, build | ✔ (≈ 55 Ko gzip) |
| Daemon Linux, toutes features : 69 tests dont modèles réels | ✔ Silero : 2,0 s de parole détectée sur 2,6 s ; « hey jarvis » : score 0,996 ; Whisper tiny : « Bonjour, … » en ~0,4 s |
| Binaire réel piloté de bout en bout (23 contrôles) | ✔ dictée mobile streamée → transcription → injection en ~340 ms |
| Compilation croisée Windows (`x86_64-pc-windows-gnu`, sans moteurs natifs) | ✔ 0 erreur, 0 avertissement (hook, injecteur, overlay, tray, COM audio) |
| Exécution réelle sous Windows (MSVC + Whisper + ONNX, injection, overlay, tray) | ⏳ non testée : à valider via la CI `windows-latest` ou sur un PC |

## Utilisation

1. Installez puis lancez DumonVoice : l'assistant s'ouvre dans le navigateur (micro, moteur, essai).
2. Maintenez `Ctrl + Espace` et parlez : le texte s'écrit dans la fenêtre active.
3. Téléphone : icône du System Tray → *Afficher le QR code mobile* → scannez avec l'appareil photo (même Wi-Fi),
   acceptez une fois l'avertissement de certificat local (ou installez l'autorité via `/setup-ca`).

Données : `%APPDATA%\DumonVoice\` (`config.json`, `certs\`, `models\`, `logs\`). Variable `DUMONVOICE_HOME`
pour un autre emplacement, `DUMONVOICE_LOG=debug` pour des journaux détaillés.

## Confidentialité

Aucun flux audio ni texte ne quitte votre réseau local. Les seuls accès Internet sont les téléchargements de
modèles que vous déclenchez (Whisper sur Hugging Face, Silero VAD et OpenWakeWord sur GitHub, `ollama pull`).
Si vous utilisez la dictée du clavier de votre téléphone, celle-ci dépend des réglages de votre smartphone.

## Écarts et limites connues

* **Mot d'éveil par défaut** : aucun modèle « Hey Dictée » public n'existe ; les mots fournis sont ceux
  d'OpenWakeWord (`hey_jarvis` par défaut) et un modèle personnalisé peut être importé (`.onnx`).
* **Changement de micro à chaud** : détecté par surveillance du périphérique par défaut et des erreurs de flux
  (équivalent fonctionnel de `IMMNotificationClient`).
* **Audio vers Ollama** : l'audio WAV est transmis dans `images` (API native) ou `input_audio`
  (API compatible OpenAI, réglage « API Ollama ») selon le support du modèle.
* **Notifications** : les avertissements (fenêtre Admin, focus changé) s'affichent dans l'overlay HUD et sur le
  téléphone, pas dans le centre de notifications Windows.
* **Phase 3** (modes contextuels, commandes vocales, streaming temps réel) : hors périmètre de cette version.
