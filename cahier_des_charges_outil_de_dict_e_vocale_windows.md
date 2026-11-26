# Cahier des Charges — Outil de Dictée Vocale Universelle sous Windows (PC & Compagnon Mobile PWA)

**Rôle :** Ingénieur Logiciel Senior  
**Plateforme cible :** Windows 10 (21H2+) & Windows 11 (x64 / ARM64) + Compagnon Mobile via navigateur (Android Chrome / iOS Safari)  
**Philosophie produit :** *« Install & Forget »* — 100 % local, confidentiel, invisible au quotidien, permettant de dicter ou d'envoyer du texte dans n'importe quelle application Windows depuis le micro du PC, le micro du téléphone ou le clavier/dictée native du téléphone.  
**Usage du document :** Ce cahier des charges combine les spécifications produit et les **contrats techniques exécutables** destinés à guider directement un agent IA ou un développeur lors de l'implémentation.

---

## 1. Vision Produit & Principes Directeurs

L'objectif est de concevoir un utilitaire tournant silencieusement en arrière-plan sous Windows, permettant à l'utilisateur d'injecter du texte **dans n'importe quelle application active sur son PC** (navigateur, suite Office, IDE, messagerie, terminal, champ de recherche Windows) via 3 moyens complémentaires :
1. **Voix sur PC :** Microphone Windows (raccourci clavier ou mot d'éveil).
2. **Voix sur Smartphone (IA PC) :** Microphone du téléphone streamé en local vers le moteur Whisper/Ollama du PC via la PWA.
3. **Saisie & Dictée native Smartphone :** Zone de texte dans la PWA permettant d'utiliser le moteur de dictée intégré au téléphone (Gboard, Dictée Apple, etc.), le clavier tactile (swipe, emojis) ou le presse-papiers du téléphone pour envoyer du texte instantanément sur le PC.

### Les 5 piliers de conception
1. **Simplicité radicale post-installation :** Une fois configuré lors du premier lancement (ou appairé en 1 scan de QR code sur téléphone), l'utilisateur n'a plus à toucher aux réglages.
2. **Souveraineté & 100 % Local :** Aucun flux audio ni texte n'est transmis par l'outil vers un serveur tiers. Tout le transit et le traitement IA de l'outil restent sur le réseau local (LAN) de l'utilisateur *(note : si l'utilisateur choisit délibérément d'utiliser la dictée clavier native de son téléphone, celle-ci dépend des réglages système de son smartphone)*.
3. **Zéro vol de focus :** L'outil Windows ne doit jamais interrompre la saisie en cours ni faire passer une fenêtre en arrière-plan.
4. **Pipeline d'injection unifié :** Qu'il provienne du micro PC, du micro téléphone ou d'un envoi texte direct depuis le smartphone, le texte aboutit dans le même injecteur Windows sécurisé.
5. **Architecture modulaire :** Découplage strict entre les sources d'entrée, les déclencheurs, les moteurs de transcription et les injecteurs de texte.

---

## 2. Périmètre Fonctionnel

### 2.1. Modes de déclenchement et de saisie

L'utilisateur peut utiliser indifféremment l'un des modes suivants :

#### A. Déclenchement PC — Raccourci clavier global
* **Mode *Push-to-Talk* (Maintenir) :** L'enregistrement démarre tant que la touche/combinaison est enfoncée et la transcription s'exécute au relâchement.
* **Mode *Toggle* (Basculer) :** Un appui démarre l'écoute, un second appui (ou un silence prolongé configurable) l'arrête.
* **Personnalisation libre :** Combinaisons classiques (`Ctrl + Espace`, `Win + Alt`, `AltGr`) et touches uniques sous-utilisées (`Ctrl droit`, `Verr. Défil`, `F9`, bouton latéral de souris). Détection automatique des conflits avec les raccourcis système critiques.

#### B. Déclenchement PC — Mot d'éveil (*Wake Word*)
* Écoute passive locale sur le micro PC à très faible consommation CPU ($< 2\,\%$) via un moteur léger dédié (*OpenWakeWord* via ONNX Runtime).
* Liste de mots pré-configurés simples (ex. *« Hey Dictée »*, *« Écris »*) + import possible de modèles `.onnx` personnalisés.
* **Arrêt automatique intelligent (VAD) :** Utilisation de *Silero VAD* pour clôturer la dictée après $X$ secondes de silence (réglable de $0{,}8\,\text{s}$ à $3{,}0\,\text{s}$, défaut : $1{,}5\,\text{s}$).

#### C. Déclenchement Distant — PWA Mobile : Mode Micro IA (PC)
* L'utilisateur ouvre la PWA sur son smartphone (connecté au même réseau Wi-Fi que le PC), appairé en 1 scan de **QR Code** (sans compte ni mot de passe).
* Un gros bouton tactile central permet de déclencher la dictée :
  * **Mode *Maintenir pour parler* (Push-to-Talk)** ou **Mode *Toucher pour démarrer / arrêter* (Toggle)**.
  * Le téléphone capture la voix via son microphone et la streame en temps réel vers le PC via WebSocket sécurisé (`wss://`).
  * Le PC transcrit l'audio avec le moteur configuré (Whisper ou Ollama) et **injecte immédiatement le texte dans la fenêtre active sur Windows**.

#### D. Déclenchement Distant — PWA Mobile : Mode Texte Direct & Clavier/Dictée Native
Pensé pour les cas où la dictée intégrée au smartphone (Gboard, Dictée iOS) est jugée plus rapide/pratique, en environnement bruyant/silencieux, ou pour taper/coller rapidement un texte, un lien ou des emojis depuis le téléphone vers le PC :
* **Zone de saisie native (`<textarea>`) :** En touchant la zone de texte, le clavier habituel du téléphone s'ouvre immédiatement. L'utilisateur peut alors :
  * Appuyer sur le bouton micro de son clavier mobile (dictée native ultra-rapide sur l'appareil), relire ou corriger un mot en 1 seconde, puis envoyer ;
  * Taper au clavier tactile / glide-typing / insérer des emojis ;
  * Coller un texte ou lien copié depuis une autre app mobile via un bouton rapide **« Coller & Envoyer »** (ou coller dans la zone).
* **Options d'envoi sans friction :**
  * Bouton **Envoyer** (injecte le texte tel quel dans la fenêtre active du PC avec une latence réseau $< 50\,\text{ms}$).
  * Option/bouton rapide **Envoyer + `↵`** (injecte le texte puis simule la touche `Entrée` sur le PC — idéal pour discuter dans Slack, Teams, Discord, ChatGPT, etc.).
  * Interrupteur optionnel **« Nettoyer via IA PC »** *(désactivé par défaut pour une latence nulle)* : si activé, le texte brut dicté sur le téléphone passe d'abord par le modèle Ollama du PC pour ponctuation/reformulation avant d'être injecté.
* **Sécurité anti-perte de brouillon :** Le champ texte sur le téléphone n'est vidé **qu'après réception de la confirmation d'injection (`ACK`) du PC**. En cas de coupure Wi-Fi ou de PC occupé, le texte reste intact dans le champ pour réessayer en 1 tap.

#### E. Annulation & Actions rapides
* Sur PC : `Échap` pendant l'écoute annule immédiatement la capture sans rien transcrire ni injecter.
* Sur Mobile (Mode Micro) : glisser hors du bouton (PTT) ou toucher *Annuler* coupe la session sans injecter.
* Sur Mobile (Barre d'actions rapides) : boutons discrets pour envoyer `Entrée` sur le PC, annuler la dernière injection (`Ctrl+Z`), ou effacer.

---

### 2.2. Moteurs de Transcription (Choix Utilisateur)

L'architecture repose sur un trait/interface abstrait `TranscriptionEngine` proposant deux familles de moteurs :

#### Option A : Moteur STT Classique (Local natif)
* **Implémentation cible :** `whisper.cpp` (via `whisper-rs`) avec accélération matérielle automatique (CUDA / Vulkan / DirectML si GPU détecté, sinon CPU optimisé AVX2/INT8).
* **Simplicité UX :** Profils clairs présentés à l'utilisateur :
  * *Rapide* (`ggml-tiny` ou `ggml-base` quantifié Q5_1 — PC portable sans GPU)
  * *Équilibré* (`ggml-small` ou `ggml-medium-q5_0` — défaut recommandé)
  * *Haute précision* (`ggml-large-v3-turbo-q5_0` — machines avec GPU dédié)

#### Option B : Modèle Ollama (Audio natif ou Hybride)
* **Intégration directe avec l'API locale Ollama** (`http://127.0.0.1:11434`, URL configurable).
* **Support des modèles multimodaux audio (ex. `lfm3-audio`, `lfm3-omni` ou tout modèle audio exposé par Ollama) :**
  * L'audio PCM 16 kHz mono est encapsulé en WAV en mémoire et transmis à l'API Ollama avec un prompt système ASR configurable.
  * **Mode *« Dictée Propre »* (activable) :** Prompt demandant au modèle de supprimer les hésitations (*« euh »*, faux départs) et d'appliquer une ponctuation naturelle en une seule passe.
* **Mode Post-traitement Texte (Hybride & Mobile Texte Direct) :** Possibilité d'utiliser un modèle Ollama texte (ex. `lfm3`) pour nettoyer ou reformater la sortie de Whisper ou un texte brut envoyé depuis le clavier du téléphone.

---

### 2.3. Injection Universelle du Texte sous Windows

Stratégie adaptative à deux niveaux pour garantir l'écriture dans toutes les applications :

1. **Méthode principale — Presse-papiers sécurisé (Recommandée, et forcée automatiquement pour les textes $> 200$ caractères, multi-lignes ou contenant des emojis) :**
   * Sauvegarde en mémoire des formats présents dans le presse-papiers Windows $\rightarrow$ Injection du texte en `CF_UNICODETEXT` **avec les marqueurs d'exclusion Windows** (`ExcludeClipboardContentFromMonitorProcessing` et `CanIncludeInClipboardHistory = 0`) $\rightarrow$ Envoi synthétique de `Ctrl + V` (ou `Ctrl + Shift + V` / `Shift + Insert` si le processus actif appartient à la liste des terminaux : `WindowsTerminal.exe`, `cmd.exe`, `powershell.exe`, `pwsh.exe`, `mintty.exe`, `alacritty.exe`, `wezterm-gui.exe`) $\rightarrow$ Attente de $150\,\text{ms}$ $\rightarrow$ Si `press_enter_after == true`, envoi de `VK_RETURN` après $50\,\text{ms}$ $\rightarrow$ Restauration silencieuse du presse-papiers d'origine.
2. **Méthode alternative — Frappe clavier simulée (`SendInput` Unicode) :**
   * Utilisation de l'API Win32 `SendInput` avec `KEYEVENTF_UNICODE` (gestion correcte des paires de substitution UTF-16 pour les emojis et caractères hors BMP).
   * Utile pour les applications bloquant le coller (consoles RDP/VNC spécifiques, certains champs web courts).

---

### 2.4. Retour Visuel et Sonore (PC & Mobile)

* **Micro-indicateur flottant PC (HUD Overlay) :**
  * Pilule discrète (position configurable), fenêtre Win32 avec `WS_EX_NOACTIVATE | WS_EX_TOPMOST | WS_EX_TRANSPARENT | WS_EX_TOOLWINDOW` (ne vole jamais le focus, n'apparaît pas dans Alt-Tab).
  * Affiche la source active (icône micro PC, icône micro mobile ou icône clavier mobile) et 4 états : *Écoute* (onde audio temps réel), *Traitement*, *Succès* (fondu $300\,\text{ms}$), *Erreur*.
* **Retours sonores (activables/désactivables) :** Sons doux et courts sur le PC (début, fin, réception texte mobile, erreur) + retour haptique (`navigator.vibrate`) sur smartphone.
* **Icône System Tray Windows :** Menu minimaliste (*Activer / Pause*, *Ouvrir réglages (PWA)*, *Afficher QR Code Mobile*, *Changer de moteur*, *Copier dernière dictée*, *Quitter*).

---

## 3. Interface PWA : Réglages PC & Compagnon Mobile

Le daemon Windows sert une **Progressive Web App (PWA)** unique et responsive embarquée dans le binaire. Elle s'adapte automatiquement au contexte :

### 3.1. Vue Desktop (Réglages — 4 onglets maximum)

| Onglet | Contenu & Ergonomie |
| :--- | :--- |
| **1. Accueil & Test** | • Pastille d'état globale (*Prêt*, *Micro PC*, *Moteur actif*, *Téléphone connecté*)<br>• Gros interrupteur **Actif / Pause**<br>• **Bac à sable :** champ texte pour tester immédiatement la dictée PC, le micro mobile ou l'envoi texte mobile et afficher la latence réelle. |
| **2. Déclenchement & Mobile** | • **Carte Clavier :** bouton « Cliquer pour définir la touche » + mode *Maintenir* / *Basculer*.<br>• **Carte Wake Word :** ON/OFF, choix du mot, sensibilité, délai de silence VAD.<br>• **Carte Compagnon Téléphone (Wi-Fi local) :** interrupteur ON/OFF, affichage du **QR Code d'appairage**, liste des appareils appairés avec bouton *Révoquer*, autorisation de l'envoi texte direct, lien d'aide certificat. |
| **3. Moteur & Langue** | • Sélecteur 2 choix : **Classique (Whisper intégré)** vs **Ollama (IA locale)**.<br>• Téléchargement 1-clic des modèles Whisper ou `ollama pull` avec barre de progression temps réel.<br>• Sélecteur de langue (*Auto*, *Français*, *Anglais*, etc.). |
| **4. Audio & Confort** | • Choix du micro PC + **vumètre temps réel**.<br>• Position de l'overlay, sons ON/OFF, *Audio Ducking* (baisse du volume média pendant l'écoute), démarrage auto avec Windows. |
| **Accordéon « Avancé »** *(replié par défaut)* | • Prompt système personnalisé Ollama, méthode d'injection, dictionnaire personnalisé (noms propres/termes métier), mini-historique local des 10 dernières dictées (désactivable), ports réseau. |

### 3.2. Vue Mobile (Compagnon Tactile — Route `/remote` ou détection mobile)

Conçue pour une utilisation fluide à une main, utilisant toute la hauteur disponible (`100dvh`) :
* **Bandeau supérieur minimaliste :**
  * Statut de connexion WebSocket (*Connecté à PC-BUREAU* en vert, ou bouton *Reconnecter*) + bouton roue crantée pour les réglages complets.
  * **Sélecteur de mode en pilule (2 choix, mémorisé dans `localStorage`) :**
    * `[ 🎙️ Micro PC ]` *(Capture audio vers Whisper/Ollama du PC)*
    * `[ ⌨️ Clavier / Dictée Tel ]` *(Saisie directe & dictée native du téléphone)*
* **Sous-vue A — Mode `🎙️ Micro PC` :**
  * **Gros bouton Micro circulaire** centré (`PointerDown`/`PointerUp` en Push-to-Talk, ou clic simple en Toggle) + onde audio circulaire temps réel.
  * Commutateur discret `[ Maintenir | Appuyer ]` + bouton `Annuler` pendant l'écoute.
  * Barre inférieure : aperçu de la dernière phrase injectée + actions rapides `[ ↵ Entrée ]`, `[ ⌫ Annuler sur PC ]`.
* **Sous-vue B — Mode `⌨️ Clavier / Dictée Tel` :**
  * **Zone de texte multi-lignes (`<textarea>`)** auto-extensible, avec `enterkeyhint="send"` (si l'option *Entrée envoie* est cochée) ou `enterkeyhint="enter"`, placeholder clair : *« Tapez ou utilisez le micro de votre clavier… »*.
  * **Barre d'outils compacte juste au-dessus du clavier virtuel** (toujours visible grâce au redimensionnement dynamique) :
    * Bouton discret `📋 Coller` (lit le presse-papiers du téléphone via `navigator.clipboard.readText()` si autorisé).
    * Interrupteur compact `↵ Auto` (envoie la touche `Entrée` sur le PC après l'injection).
    * Interrupteur compact `✨ IA` *(optionnel, si Ollama actif)* pour faire reformuler/ponctuer par le PC avant injection.
    * Gros bouton principal **`Envoyer ➤`** (désactivé si vide ou en cours d'envoi ; spinner bref jusqu'à l'ACK du PC).
  * **Zone d'historique rapide au-dessus du champ :** Les 2-3 derniers textes envoyés (cliquables pour ré-injecter ou copier) + bouton `[ ⌫ Annuler sur PC ]`.

---

## 4. Anticipation des Cas de Figure & Cas Limites (Edge Cases)

| # | Cas de figure / Problème | Impact si non géré | Solution logicielle obligatoire |
| :--- | :--- | :--- | :--- |
| **1** | **Blocage `getUserMedia` sur HTTP LAN (Critique Mobile)** | Chrome Android et Safari iOS **interdisent strictement l'accès au micro et au presse-papiers** sur une IP locale en `http://` (hors `localhost`). | Le serveur LAN dédié au mobile **doit obligatoirement fonctionner en HTTPS/WSS** (certificat TLS local auto-généré avec SAN IP LAN + mDNS). Voir Section 6.2. |
| **2** | **Clavier virtuel mobile qui masque le bouton Envoyer** | Sur iOS Safari et Android Chrome, l'ouverture du clavier masque le bas de page (`100vh` inclut la zone sous le clavier). | Utiliser `<meta name="viewport" content="width=device-width, initial-scale=1, interactive-widget=resizes-content">`, des hauteurs en `100dvh` et écouter `window.visualViewport.addEventListener('resize', ...)` pour ancrer la barre d'envoi juste au-dessus du clavier. |
| **3** | **Double-envoi ou perte de brouillon en mode Texte Mobile** | Si le Wi-Fi hésite, l'utilisateur tape 2 fois sur Envoyer (texte injecté en double) ou perd un long paragraphe dicté si le PC renvoie une erreur. | 1) Chaque envoi texte porte un `message_id` (UUID v4) dédupliqué pendant $60\,\text{s}$ côté daemon (idempotence).<br>2) La PWA verrouille le bouton pendant l'envoi et **ne vide le `<textarea>` qu'à réception d'un `injection_result` positif** (et sauvegarde le brouillon en cours dans `sessionStorage`). |
| **4** | **Textes longs, multi-lignes et Emojis depuis le téléphone** | `SendInput` caractère par caractère prend plusieurs secondes sur un paragraphe et peut casser les emojis (UTF-16 surrogate pairs) ou les retours ligne. | Si `text.chars().count() > 200` ou présence de `\n` ou de caractères hors BMP (emojis), forcer automatiquement l'injection par **presse-papiers sécurisé (`CF_UNICODETEXT`)** même si `SendInput` est sélectionné par défaut. |
| **5** | **Caprices d'iOS Safari (`AudioContext` & Fréquence)** | Safari iOS suspend `AudioContext` s'il n'est pas créé/repris dans un gestionnaire de geste utilisateur, et impose souvent $48\,\text{kHz}$ au lieu de $16\,\text{kHz}$. | Instancier ou appeler `audioContext.resume()` **strictement dans l'événement `pointerdown`/`click`**. Lire `audioContext.sampleRate` dynamiquement et rééchantillonner vers $16\,\text{kHz}$ dans l'`AudioWorklet`. |
| **6** | **Mise en veille de l'écran du téléphone** | L'écran s'éteint pendant une session de travail : le WebSocket se ferme et le micro coupe. | Utiliser **Screen Wake Lock** (`navigator.wakeLock.request('screen')`) tant que `/remote` est au premier plan, avec réacquisition sur `visibilitychange`, et reconnexion WebSocket automatique (backoff exponentiel). |
| **7** | **Coupure Wi-Fi en pleine phrase audio mobile** | Le flux audio s'interrompt avant `session_stop` : le PC reste bloqué en état `LISTENING`. | Heartbeat WebSocket (`ping`/`pong` toutes les $5\,\text{s}$) + **watchdog de flux** ($2\,\text{s}$ sans paquet binaire pendant `LISTENING` $\rightarrow$ finalisation propre ou annulation, puis retour garanti à `IDLE`). |
| **8** | **Conflit simultané PC vs Téléphone** | L'utilisateur envoie un texte ou parle sur téléphone alors qu'une dictée PC est active. | Verrou exclusif dans la machine à états (`active_source`). Le premier arrivé verrouille la session ; la seconde source reçoit immédiatement `BUSY` (sans effacer le brouillon mobile). |
| **9** | **Pare-feu Windows & Changement d'IP LAN** | Windows Defender bloque le port mobile ou la box attribue une nouvelle IP au PC. | 1) Règle pare-feu entrante limitée au profil réseau **Privé uniquement**.<br>2) Publication **mDNS** (`<nom-pc>.local`) et régénération automatique du certificat SAN si l'IP locale change. |
| **10** | **Fenêtres avec droits Administrateur (UIPI)** | Windows bloque `SendInput` venant d'un processus non-élevé dans un terminal Admin. | Détecter le niveau d'intégrité (`GetTokenInformation` / échec d'injection), copier le texte dans le presse-papiers et afficher un toast (PC + Mobile) : *« Fenêtre Admin détectée : texte copié, faites Ctrl+V »*. |
| **11** | **Début de phrase tronqué (PC)** | L'utilisateur parle trop vite après le déclenchement : première syllabe coupée. | Maintenir un **Ring Buffer RAM de $500\,\text{ms}$** sur WASAPI et préfixer la capture avec les $300\,\text{ms}$ précédant le déclenchement. |
| **12** | **Changement de micro PC à chaud** | Débranchement d'un casque USB/Bluetooth plante la capture. | Écouter `IMMNotificationClient` (Windows Core Audio) et basculer silencieusement sur le nouveau périphérique par défaut. |
| **13** | **Appels Visio (Teams, Zoom, Discord)** | Le Wake Word se déclenche pendant une réunion. | Détecter les sessions audio actives capturant le micro via WASAPI (`IAudioSessionManager2`) et mettre le Wake Word en pause automatique (activé par défaut). |
| **14** | **Jeux vidéo & Applications plein écran** | L'overlay HUD provoque un retour bureau ou gêne le jeu. | Vérifier `SHQueryUserNotificationState` (`QUNS_RUNNING_D3D_FULL_SCREEN`). En plein écran exclusif : masquer l'overlay automatiquement. |
| **15** | **Démarrage à froid ou panne d'Ollama** | Ollama décharge `lfm3-audio` ou n'est pas lancé : latence surprise ou dictée perdue. | Envoyer `keep_alive` configurable (défaut `30m`), afficher *« Chargement du modèle... »* sur l'overlay et le mobile, et proposer un **fallback automatique** vers Whisper (ou injection brute si mode texte direct avec option IA). |
| **16** | **Hallucinations sur du silence** | Appui accidentel $\rightarrow$ Whisper hallucine *« Sous-titres... »*. | Filtrage systématique par **Silero VAD** côté daemon PC (pour PC et Mobile Audio) : si durée vocale effective $< 250\,\text{ms}$, ignorer silencieusement. |
| **17** | **Perte de focus pendant le calcul** | L'utilisateur change de fenêtre sur PC pendant l'inférence IA. | Pour la voix : capturer `target_hwnd` au *début* de la dictée ; si `GetForegroundWindow()` diffère à la fin, copier dans le presse-papiers + notifier. Pour le texte direct : capturer `target_hwnd` à réception du message. |
| **18** | **Confidentialité du presse-papiers Windows** | Les textes injectés polluent l'historique `Win + V` et la synchro cloud Microsoft. | Positionner systématiquement `ExcludeClipboardContentFromMonitorProcessing` et `CanIncludeInClipboardHistory = 0` avant chaque coller. |
| **19** | **Mise en veille / Batterie PC portable** | La sortie de veille casse WASAPI ou le réseau LAN. | Écouter `WM_POWERBROADCAST` : réinitialiser WASAPI, les hooks clavier et les interfaces réseau au réveil. |

---

## 5. Architecture Technique Globale

```text
+-------------------------------------------------------------------------------+
|                         DAEMON WINDOWS (Rust / .NET AOT)                      |
|                                                                               |
|  [System Tray]      [Overlay HUD Win32]      [Hooks Clavier & WinEvents]      |
|         \                    |                           /                    |
|          +-------------------+--------------------------+                     |
|                              |                                                |
|                  [Orchestrateur FSM (Mutex + Idempotence)]                    |
|                 /            |               |            \                   |
|  +-------------+---+   +-----+----------+    |    +--------+-----------+      |
|  | Sources Audio   |   | Pipeline IA    |    |    | Injecteur Windows  |      |
|  | (PCM 16kHz S16) |   | • Silero VAD   |    |    | • Clipboard Guard  |      |
|  | • WASAPI + Ring |-->| • Whisper.cpp  |--->+--->| • SendInput UTF-16 |      |
|  | • WakeWord ONNX |   | • Client Ollama|    ^    | • Exclusion Win+V  |      |
|  | • WS Audio Bus  |   |   (Audio/Texte)|    |    | • Option VK_RETURN |      |
|  +--------+--------+   +----------------+    |    +--------------------+      |
|           |                     ^            |                                |
|           | (Binaire PCM)       | (Option IA)| (Texte Direct - latence <50ms) |
|  +--------+---------------------+------------+--------------------+           |
|  |                    SERVEUR WEB EMBARQUÉ (Axum)                 |           |
|  |  • Listener Local : http://127.0.0.1:18450 (Toujours actif)    |           |
|  |  • Listener LAN   : https://0.0.0.0:18451  (Si Mobile activé)  |           |
|  |  • Static Assets PWA embarqués + REST API + WebSocket          |           |
|  +-----------------+----------------------------------+-----------+           |
+--------------------|----------------------------------|-----------------------+
                     | (Localhost)                      | (Wi-Fi LAN - HTTPS/WSS)
            +--------v--------+              +----------v--------------------+
            |  PWA Desktop    |              |  PWA Mobile (/remote)         |
            |  (Réglages PC)  |              |  • Onglet 1: Micro AudioWorklet|
            +-----------------+              |  • Onglet 2: Clavier/Dictée   |
                                             |    native + Envoi Texte Direct|
                                             +-------------------------------+
```

### Stack Technologique Recommandée (Optimisée pour un binaire autonome)
* **Backend / Daemon Windows :** **Rust** (édition 2021+).
  * *Web & WebSocket :* `axum`, `tokio`, `tokio-rustls` (TLS natif sans OpenSSL externe), `rcgen` (génération auto de certificats CA/Serveur), `mdns-sd` (découverte `.local`), `rust-embed` (embarque la PWA dans le `.exe`).
  * *Audio PC & IA :* `cpal` ou `wasapi`, `whisper-rs`, `ort` (ONNX Runtime pour Silero VAD et OpenWakeWord), `reqwest` (client HTTP Ollama).
  * *Intégration Windows :* `windows` (crate officiel Microsoft Win32), `tray-icon`, hook bas niveau `WH_KEYBOARD_LL` (pour gérer Push-to-Talk fiable et touches uniques type `Ctrl droit`).
* **Frontend PWA :** **Svelte 5 + TypeScript + Vite + Tailwind CSS** (build statique léger $< 150\,\text{Ko}$ gzippé, zéro dépendance CDN externe, embarqué via `rust-embed`).

---

## 6. Spécifications Techniques Détaillées (Guide d'Implémentation pour Agent IA)

Cette section définit les contrats stricts à respecter lors du codage pour éliminer toute ambiguïté d'architecture.

### 6.1. Machine à États Finis (FSM) de l'Orchestrateur

Le cœur du daemon est piloté par un état unique partagé (`Arc<RwLock<AppState>>`) diffusé en temps réel à l'overlay HUD et à tous les clients WebSocket (Desktop & Mobile) :

```text
États possibles :
- PAUSED       : Aucun déclencheur actif (sauf réactivation manuelle).
- IDLE         : En attente d'un déclencheur (Hotkey, WakeWord, Audio Mobile ou Texte Mobile).
- LISTENING    : Capture audio active depuis `active_source` (LocalMic | RemoteAudio(device_id)).
- TRANSCRIBING : Traitement IA en cours (VAD + Whisper/Ollama audio, OU post-traitement texte Ollama).
- INJECTING    : Insertion du texte dans la fenêtre Windows capturée (`target_hwnd`).
- ERROR(code)  : État transitoire (affiché 3s) puis retour automatique à IDLE.
```

**Règles de transition strictes :**
1. **Flux Voix (`IDLE` $\rightarrow$ `LISTENING` $\rightarrow$ `TRANSCRIBING` $\rightarrow$ `INJECTING` $\rightarrow$ `IDLE`) :**
   * Entrée en `LISTENING` uniquement si `active_source` est `None`. Capture immédiate de `target_hwnd = GetForegroundWindow()`, génération `session_id = Uuid::new_v4()`, activation optionnelle d'*Audio Ducking*.
   * `LISTENING` $\rightarrow$ `IDLE` sur `Cancel` (`Échap` ou `session_cancel`) : vide le buffer, restaure le volume média, n'injecte rien.
   * `LISTENING` $\rightarrow$ `TRANSCRIBING` sur fin de capture : restaure le volume média, exécute Silero VAD (court-circuit vers `IDLE` avec `EMPTY_AUDIO` si parole $< 250\,\text{ms}$), puis moteur STT/Ollama.
2. **Flux Texte Direct Mobile (`IDLE` $\rightarrow$ `INJECTING` $\rightarrow$ `IDLE`, ou via `TRANSCRIBING` si `refine_with_ollama == true`) :**
   * Vérifier que `remote_mobile.allow_direct_text == true` et que la taille UTF-8 est $\le$ `max_text_length` (défaut $50\,\text{Ko}$ / $20\,000$ caractères).
   * Vérifier le cache d'idempotence (`HashMap<Uuid, Instant>` purgé au-delà de $60\,\text{s}$) : si `message_id` a déjà été traité, renvoyer immédiatement le résultat en cache sans ré-injecter.
   * Si l'état courant n'est pas `IDLE`, rejeter immédiatement avec `code: "BUSY"` (sans rien perdre côté mobile).
   * Capturer `target_hwnd = GetForegroundWindow()` à réception du message.
   * Si `refine_with_ollama == true`, passer brièvement en `TRANSCRIBING` pour nettoyer le texte via Ollama (avec fallback silencieux sur le texte original si Ollama échoue) ; sinon passer directement en `INJECTING`.
   * Exécuter l'injection (+ `VK_RETURN` si `press_enter_after == true`), ajouter au mini-historique RAM, et diffuser `injection_result` contenant le `message_id` et `injected: true`.

---

### 6.2. Réseau Local, HTTPS Mobile & Sécurité d'Appairage

Pour contourner proprement la contrainte *Secure Context* de `getUserMedia()` et `navigator.clipboard.readText()` sur smartphone :

#### Architecture à double listener
1. **Listener Local (toujours actif) :** `http://127.0.0.1:18450`
   * Exclusivement accessible depuis le PC lui-même.
   * Authentification par jeton local généré au démarrage, injecté dans l'URL ouverte depuis le System Tray (`http://127.0.0.1:18450/?auth=<local_token>`) puis stocké en cookie `HttpOnly; SameSite=Strict`.
2. **Listener LAN Mobile (actif uniquement si `remote_mobile.enabled == true`) :** `https://0.0.0.0:18451`
   * Utilise TLS via `rustls`.
   * **Génération automatique PKI locale (`rcgen`) :**
     * Génère une autorité locale persistante (`%APPDATA%\<AppName>\certs\ca.key` et `ca.crt`, validité 10 ans).
     * Génère un certificat serveur (`server.crt` / `server.key`) signé par cette CA, incluant en **Subject Alternative Names (SAN)** : toutes les IPv4 locales privées (`192.168.x.x`, `10.x.x.x`, `172.16-31.x.x`), `localhost`, `127.0.0.1`, et `<hostname>.local`.
     * À chaque démarrage (ou changement réseau), vérifie si l'IP LAN courante figure dans les SAN ; sinon régénère `server.crt` à la volée.
   * **UX Certificat simplifiée sur téléphone :**
     * *Mode rapide (zéro installation) :* Scan du QR Code $\rightarrow$ acceptation unique de l'avertissement de certificat local $\rightarrow$ micro, presse-papiers et `wss://` fonctionnent immédiatement dans l'onglet.
     * *Mode PWA installable (optionnel, guidé sur `/setup-ca`) :* Installation de `ca.crt` pour approuver le certificat local et épingler la PWA sur l'écran d'accueil.

#### Protocole d'appairage par QR Code
1. La PWA Desktop appelle `POST /api/pairing/create` (authentifié localement).
2. Le daemon génère un `pair_code` cryptographique aléatoire (32 octets base64url), valide **$120\,\text{s}$** et à **usage unique**.
3. La PWA Desktop affiche le QR Code encodant `https://<LAN_IP>:18451/remote?pair=<pair_code>`.
4. Le téléphone appelle `POST /api/pairing/exchange` avec `{ "pair_code": "...", "device_name": "Pixel / iPhone" }`.
5. Le daemon détruit `pair_code`, génère un `device_id` + un `device_token` persistant (stocké hashé SHA-256 dans `config.json`), et le renvoie au téléphone (`localStorage` + cookie `Secure; SameSite=Strict`).
6. Toute requête mobile ultérieure utilise `Authorization: Bearer <device_token>` (REST) ou le premier message `auth` (WebSocket).

---

### 6.3. Pipeline Audio Mobile (`AudioWorklet` $\rightarrow$ WebSocket Binaire)

**Ne pas utiliser `MediaRecorder` (WebM/Opus/MP4)** (incompatibilités iOS/Android et latence). Utiliser le pipeline PCM brut :

1. **Contraintes `getUserMedia` :**
   ```typescript
   const stream = await navigator.mediaDevices.getUserMedia({
     audio: {
       channelCount: 1,
       echoCancellation: true,
       noiseSuppression: true,
       autoGainControl: true
     },
     video: false
   });
   ```
2. **Capture & Rééchantillonnage via `AudioWorkletProcessor` (`pcm-processor.js`) :**
   * Lire `sampleRate` matériel (ex. $48000\,\text{Hz}$ ou $44100\,\text{Hz}$).
   * Rééchantillonner par interpolation linéaire vers **$16000\,\text{Hz}$**.
   * Convertir `Float32` ($[-1.0, 1.0]$) en entier signé **16-bit Little-Endian** (`Int16`, $[-32768, 32767]$).
   * Accumuler **640 échantillons** ($40\,\text{ms}$ = **$1280\,\text{octets}$**), calculer le niveau RMS local (pour le vumètre mobile), puis transférer l'`ArrayBuffer` via *Transferable Objects* et l'envoyer en trame binaire WebSocket (`ws.send(buffer)`).
   * *Débit :* $32\,\text{Ko/s}$ ($256\,\text{kbps}$, 25 paquets/s).

---

### 6.4. Contrat d'API REST & WebSocket

#### Endpoints REST essentiels

| Méthode & Route | Portée | Rôle |
| :--- | :--- | :--- |
| `GET /api/status` | Local + Mobile | État FSM, moteur actif, liste micros, statut Ollama, nom du PC. |
| `GET /api/config` | Local + Mobile | Lecture de la configuration (sans les hashes de tokens). |
| `PATCH /api/config` | Local + Mobile | Mise à jour partielle validée + rechargement à chaud. |
| `POST /api/pairing/create` | **Local uniquement** | Génère un `pair_code` temporaire ($120\,\text{s}$). |
| `POST /api/pairing/exchange` | LAN (public avec code) | Échange un `pair_code` valide contre un `device_token` permanent. |
| `DELETE /api/pairing/devices/{id}` | **Local uniquement** | Révoque immédiatement un smartphone et ferme son WebSocket. |
| `POST /api/remote/inject-text` | Local + Mobile | Injection directe de texte en REST (fallback si WS coupé, ou automatisation type Raccourcis iOS/Android). Même schéma JSON que le message WS `inject_text`. |
| `POST /api/remote/action` | Local + Mobile | Action rapide sur PC (`enter`, `undo_last`, `backspace`). |
| `GET /api/engines/ollama/models` | Local + Mobile | Interroge Ollama (`/api/tags`) et identifie les modèles disponibles. |
| `POST /api/engines/download` | Local + Mobile | Démarre le téléchargement d'un modèle Whisper ou `ollama pull` (progression via WS). |
| `GET /api/history` | Local + Mobile | Retourne les $N \le 10$ dernières injections/dictées en mémoire. |
| `GET /ca.crt` | LAN | Téléchargement du certificat racine local pour installation PWA optionnelle. |

#### Protocole WebSocket (`/ws`)

* **Trames Texte :** JSON UTF-8 avec champ discriminateur `"type"`.
* **Trames Binaires :** Uniquement du client Mobile vers le PC, uniquement pendant une session audio active (`PCM S16LE 16000 Hz Mono`).

**Messages Client (Mobile / Desktop) $\rightarrow$ Daemon PC :**
```json
// 1. Authentification obligatoire (premier message sur connexion LAN)
{ "type": "auth", "token": "<device_token>", "client_kind": "mobile" }

// 2. Démarrer une dictée audio depuis le micro du téléphone
{
  "type": "session_start",
  "session_id": "550e8400-e29b-41d4-a716-446655440000",
  "audio_format": { "encoding": "pcm_s16le", "sample_rate": 16000, "channels": 1 },
  "trigger_mode": "push_to_talk",
  "auto_stop_vad": false
}

// 3. Terminer la dictée audio et lancer transcription + injection sur PC
{ "type": "session_stop", "session_id": "550e8400-e29b-41d4-a716-446655440000" }

// 4. Annuler la dictée audio en cours sans rien injecter
{ "type": "session_cancel", "session_id": "550e8400-e29b-41d4-a716-446655440000" }

// 5. Envoyer du texte directement depuis le clavier / la dictée native du téléphone
{
  "type": "inject_text",
  "message_id": "9b1deb4d-3b7d-4bad-9bdd-2b0d7b3dcb6d",
  "text": "Texte dicté via Gboard/iOS ou tapé au clavier du téléphone 👍",
  "press_enter_after": false,
  "refine_with_ollama": false
}

// 6. Heartbeat
{ "type": "ping", "ts": 1732150000000 }
```

**Messages Daemon PC $\rightarrow$ Clients (Mobile & Desktop) :**
```json
// Accusé d'authentification
{ "type": "auth_ok", "pc_name": "PC-BUREAU", "state": "IDLE" }

// Changement d'état FSM (diffusé à tous les clients)
{
  "type": "state_changed",
  "state": "INJECTING",
  "source": "remote_text", // local_mic | remote_audio | remote_text
  "session_id": "9b1deb4d-3b7d-4bad-9bdd-2b0d7b3dcb6d",
  "target_window": "Visual Studio Code"
}

// Niveau audio PC (pour le vumètre de la PWA Desktop uniquement, max 20 Hz)
{ "type": "audio_level", "rms": 0.14 }

// Résultat unifié d'une dictée audio ou d'un envoi texte direct (sert d'ACK au mobile)
{
  "type": "injection_result",
  "id": "9b1deb4d-3b7d-4bad-9bdd-2b0d7b3dcb6d", // session_id ou message_id
  "source": "remote_text", // local_mic | remote_audio | remote_text
  "text": "Texte dicté via Gboard/iOS ou tapé au clavier du téléphone 👍",
  "injected": true,
  "injection_note": null, // ex. "ADMIN_WINDOW_CLIPBOARD_ONLY"
  "target_window": "Visual Studio Code",
  "latency_ms": 28
}

// Erreur explicite (avec id optionnel pour débloquer le bouton côté mobile)
{
  "type": "error",
  "id": "9b1deb4d-3b7d-4bad-9bdd-2b0d7b3dcb6d",
  "code": "BUSY", // BUSY | TEXT_TOO_LARGE | OLLAMA_UNREACHABLE | ADMIN_WINDOW_FALLBACK | EMPTY_AUDIO | TIMEOUT
  "message": "Une dictée est déjà en cours sur le PC."
}
```

---

### 6.5. Intégration Ollama (Audio & Post-Traitement Texte)

L'adaptateur `OllamaEngine` expose deux méthodes découplées :
1. **`transcribe_audio(pcm_16k: &[i16], lang: &str)` :**
   * Encapsule `pcm_16k` en WAV standard en mémoire (en-tête RIFF 44 octets + PCM) encodé en Base64.
   * Appelle l'endpoint Ollama configuré (`http://127.0.0.1:11434/api/chat` ou `/v1/chat/completions`) avec `model` (ex. `lfm3-audio`), `keep_alive` (`"30m"`), `stream: false`, `options.temperature: 0.0` et un prompt système ASR strict interdisant toute conversation.
   * Fallback automatique vers `WhisperEngine` en cas d'erreur réseau/timeout ($> 15\,\text{s}$) si activé.
2. **`refine_text(raw_text: &str, lang: &str)` :**
   * Utilisé en mode Pipeline Hybride ou lorsque l'utilisateur mobile active le bouton `✨ IA` en mode Texte Direct.
   * Prompt système minimal : corrige uniquement la ponctuation, la casse et les hésitations sans altérer le sens ni ajouter de préambule.
   * Fallback garanti : en cas d'erreur/timeout ($> 8\,\text{s}$), injecter `raw_text` sans bloquer l'utilisateur.
* **Nettoyage post-inférence commun :** `trim()`, suppression d'éventuels guillemets parasites enveloppant toute la réponse, application du `custom_dictionary`.

---

### 6.6. Schéma de Configuration (`%APPDATA%\<AppName>\config.json`)

```json
{
  "version": 1,
  "enabled": true,
  "launch_at_startup": true,
  "language": "fr",
  "triggers": {
    "hotkey": {
      "enabled": true,
      "keys": ["ControlLeft", "Space"],
      "mode": "push_to_talk"
    },
    "wake_word": {
      "enabled": false,
      "model": "hey_dictee",
      "sensitivity": 0.65,
      "silence_timeout_ms": 1500,
      "pause_during_calls": true
    }
  },
  "remote_mobile": {
    "enabled": true,
    "allow_direct_text": true,
    "max_text_length": 20000,
    "lan_port": 18451,
    "mdns_name": "dictee-pc",
    "paired_devices": [
      {
        "id": "dev_01",
        "name": "Mon Téléphone",
        "token_hash": "<sha256_hex>",
        "paired_at": "2026-11-26T10:00:00Z"
      }
    ]
  },
  "engine": {
    "active": "ollama",
    "whisper": {
      "profile": "balanced",
      "model_file": "ggml-small-q5_1.bin",
      "use_gpu": true
    },
    "ollama": {
      "base_url": "http://127.0.0.1:11434",
      "model": "lfm3-audio",
      "keep_alive": "30m",
      "clean_dictation": true,
      "custom_system_prompt": null,
      "fallback_to_whisper": true
    }
  },
  "audio": {
    "pc_device_id": "default",
    "duck_other_apps": true,
    "duck_level": 0.15,
    "sound_feedback": true
  },
  "injection": {
    "method": "clipboard_paste",
    "restore_clipboard": true,
    "paste_delay_ms": 150,
    "check_window_focus": true,
    "custom_dictionary": {}
  },
  "ui": {
    "overlay_position": "bottom_center",
    "keep_history_count": 10
  }
}
```

---

### 6.7. Arborescence Projet Recommandée (Monorepo)

Structure propre facilitant le travail itératif d'un agent IA :

```text
/
├── daemon/                         # Binaire Windows Rust
│   ├── Cargo.toml
│   └── src/
│       ├── main.rs                 # Point d'entrée, init Tray + Runtime Tokio
│       ├── config.rs               # Chargement/sauvegarde atomique config.json
│       ├── fsm.rs                  # Orchestrateur d'états + verrou source + cache idempotence
│       ├── audio/
│       │   ├── mod.rs              # Types PCM 16kHz & RingBuffer 500ms
│       │   ├── wasapi_capture.rs   # Capture PC + IMMNotificationClient + Ducking
│       │   ├── vad.rs              # Silero VAD (ONNX Runtime)
│       │   └── wake_word.rs        # OpenWakeWord (ONNX Runtime)
│       ├── engines/
│       │   ├── mod.rs              # Trait TranscriptionEngine
│       │   ├── whisper.rs          # Implémentation whisper-rs + downloader
│       │   └── ollama.rs           # Client Ollama audio & refine_text + healthcheck
│       ├── windows_os/
│       │   ├── hotkey.rs           # Hook clavier global (PTT + Toggle)
│       │   ├── injector.rs         # Clipboard Guard (sans Win+V) + SendInput UTF-16 + VK_RETURN
│       │   ├── overlay.rs          # Fenêtre HUD WS_EX_NOACTIVATE
│       │   ├── tray.rs             # System Tray natif
│       │   └── system_state.rs     # Détection Admin UIPI, Plein écran, Veille
│       └── server/
│           ├── mod.rs              # Double listener HTTP local + HTTPS LAN
│           ├── tls.rs              # Génération auto CA + Certs SAN (rcgen)
│           ├── mdns.rs             # Publication mDNS .local
│           ├── auth.rs             # Pairing QR Code + vérification Bearer
│           ├── rest.rs             # Routes /api/* (dont /api/remote/inject-text)
│           └── ws.rs               # Flux temps réel, PCM Mobile & messages inject_text
├── pwa/                            # Frontend Svelte + TS + Tailwind
│   ├── package.json
│   ├── public/
│   │   ├── manifest.webmanifest
│   │   ├── sw.js                   # Service Worker minimal
│   │   └── pcm-processor.js        # AudioWorklet 16kHz S16LE
│   └── src/
│       ├── lib/
│       │   ├── wsClient.ts         # Client WS auto-reconnect + heartbeat + ACK tracker
│       │   ├── mobileAudio.ts      # Capture micro + WakeLock + Haptics
│       │   └── api.ts              # Appels REST typés
│       ├── components/
│       │   ├── MobileMicPad.svelte   # Sous-vue Micro PC (PTT/Toggle + vumètre)
│       │   └── MobileTextPad.svelte  # Sous-vue Clavier/Dictée native + visualViewport
│       ├── views/
│       │   ├── DesktopSettings.svelte  # 4 onglets réglages + QR Code
│       │   ├── MobileRemote.svelte     # Conteneur mobile avec sélecteur 2 modes
│       │   └── OnboardingWizard.svelte # Assistant premier lancement
│       └── App.svelte              # Routeur léger (/ vs /remote vs /setup-ca)
└── scripts/
    └── build-windows.ps1           # Build PWA -> embed dans daemon -> installeur
```

---

## 7. Parcours d'Installation & Premier Lancement (Onboarding)

1. **Installateur unique (`.exe` / MSIX) :** Installation dans l'espace utilisateur, ajout de la règle pare-feu Windows (profil *Privé* uniquement) pour le mode mobile, proposition de lancement avec Windows.
2. **Assistant 3 étapes (< 60 s, ouvert dans la PWA Desktop) :**
   * *Étape 1 — Micro & Téléphone :* Test automatique du micro PC et/ou affichage direct du QR Code pour appairer son téléphone (micro sans fil + clavier/dictée mobile).
   * *Étape 2 — Moteur :* Détection silencieuse du GPU et d'Ollama. Proposition en 1 clic de `lfm3-audio` (si Ollama présent) ou téléchargement automatique de Whisper *Équilibré*.
   * *Étape 3 — Essai immédiat :* Test de dictée (ou d'envoi texte mobile) en direct dans le bac à sable $\rightarrow$ Validation visuelle $\rightarrow$ L'utilisateur ferme l'onglet et oublie l'outil.

---

## 8. Exigences Non-Fonctionnelles (Objectifs Mesurables)

| Critère | Objectif cible |
| :--- | :--- |
| **Latence Envoi Texte Direct Mobile $\rightarrow$ PC** | $< 50\,\text{ms}$ sur Wi-Fi local (injection quasi instantanée, hors option `✨ IA`). |
| **Latence Voix PC (Fin de parole $\rightarrow$ Texte affiché)** | $< 600\,\text{ms}$ avec GPU ; $< 1{,}5\,\text{s}$ sur CPU moderne (phrase de 5-10 s). |
| **Surcoût latence Micro Mobile (Wi-Fi LAN)** | $< 80\,\text{ms}$ (streaming continu pendant la parole : l'audio est déjà sur le PC au relâchement). |
| **Empreinte RAM en veille** | $< 45\,\text{Mo}$ (hors poids du modèle IA chargé). |
| **Consommation CPU en veille** | $< 0{,}1\,\%$ (raccourci clavier / attente mobile) ; $< 2\,\%$ (Wake Word PC actif). |
| **Fiabilité / Uptime** | Fonctionnement multi-semaines sans fuite mémoire, avec reconnexion transparente du téléphone au retour sur le Wi-Fi. |

---

## 9. Feuille de Route (Livraison Incrémentale)

### Phase 1 — MVP (Cœur PC + Compagnon Mobile Audio & Texte)
* Daemon Rust Windows : System Tray, FSM avec cache d'idempotence, `config.json`, double serveur HTTP local (`127.0.0.1`) + HTTPS LAN (`rcgen`).
* Capture WASAPI 16 kHz + Ring Buffer + Silero VAD.
* Déclenchement par raccourci clavier global (*Push-to-Talk* et *Toggle*).
* **Compagnon Mobile complet (`/remote`) :** QR Code d'appairage, bascule fluide entre **Mode Micro PC** (`AudioWorklet` 16 kHz PCM S16LE, WakeLock, haptique) et **Mode Clavier / Dictée Native** (`MobileTextPad` avec envoi direct, `Envoyer + ↵`, collage presse-papiers et protection anti-perte de brouillon).
* Support des 2 moteurs : Whisper (`whisper-rs`) et Ollama (`lfm3-audio` + `refine_text`).
* Injection universelle par presse-papiers sécurisé (exclusion `Win + V`, support emojis/multilignes, option `VK_RETURN`) + fallback `SendInput` + Overlay HUD minimaliste.

### Phase 2 — Wake Word, Confort & Résilience Windows
* Intégration du **Wake Word** local PC (OpenWakeWord) avec arrêt auto VAD.
* Changement de micro PC à chaud (`IMMNotificationClient`), *Audio Ducking*, détection appels visio et plein écran/jeux.
* Gestion avancée des cas limites : détection fenêtres Admin (UIPI), mDNS `.local`, gestionnaire de téléchargement de modèles intégré, assistant d'onboarding et page guidée `/setup-ca`.

### Phase 3 — Productivité Avancée (Sans casser la simplicité)
* **Modes contextuels automatiques :** Formatage adapté selon l'application active sur le PC (Discord, Outlook, VS Code).
* **Commandes vocales :** Actions via compréhension `lfm3-audio` (*« nouvelle ligne »*, *« supprime la dernière phrase »*, *« traduis en anglais »*).
* **Streaming temps réel :** Affichage progressif des mots pendant les dictées longues (PC et Mobile).