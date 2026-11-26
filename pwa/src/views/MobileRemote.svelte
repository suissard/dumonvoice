<script lang="ts">
  import { onDestroy, onMount } from 'svelte';
  import { api, getDeviceToken, guessDeviceName, setDeviceToken } from '../lib/api';
  import { MobileAudio, ScreenWakeLock } from '../lib/mobileAudio';
  import { store } from '../lib/store.svelte';
  import MobileMicPad from '../components/MobileMicPad.svelte';
  import MobileTextPad from '../components/MobileTextPad.svelte';
  import Segmented from '../components/Segmented.svelte';
  import Toggle from '../components/Toggle.svelte';
  import DesktopSettings from './DesktopSettings.svelte';

  type Mode = 'mic' | 'text';
  const initiallyPaired = !!getDeviceToken();
  let paired = $state(initiallyPaired);
  let pairing = $state(false);
  let pairError = $state('');
  let mode = $state<Mode>(loadMode());
  let showSettings = $state(false);
  let fullSettings = $state(false);
  let enterSends = $state(readFlag('dv_enter_sends'));
  let vv = $state({ top: 0, height: typeof window !== 'undefined' ? window.innerHeight : 0 });
  const audio = new MobileAudio();
  const wake = new ScreenWakeLock();

  function readFlag(k: string): boolean {
    try {
      return localStorage.getItem(k) === '1';
    } catch {
      return false;
    }
  }
  function loadMode(): Mode {
    try {
      return localStorage.getItem('dv_mode') === 'text' ? 'text' : 'mic';
    } catch {
      return 'mic';
    }
  }
  function setMode(m: Mode) {
    mode = m;
    try {
      localStorage.setItem('dv_mode', m);
    } catch {
      /* ignore */
    }
  }

  // Ancrage au-dessus du clavier virtuel (iOS Safari ignore interactive-widget).
  function onViewport() {
    const v = window.visualViewport;
    vv = v ? { top: v.offsetTop, height: v.height } : { top: 0, height: window.innerHeight };
  }

  function onVisibility() {
    if (document.visibilityState === 'visible') {
      store.ws?.reconnectNow();
      store.refreshStatus();
    } else {
      // Micro libéré en arrière-plan.
      audio.release();
    }
  }

  async function pair(code: string) {
    pairing = true;
    pairError = '';
    try {
      const r = await api.pairingExchange(code, guessDeviceName());
      setDeviceToken(r.device_token, r.device_id);
      start();
      paired = true;
    } catch (e: any) {
      pairError = e?.message ?? 'Appairage impossible';
    } finally {
      pairing = false;
      history.replaceState(null, '', '/remote');
    }
  }

  let started = false;
  function start() {
    if (started) return;
    started = true;
    store.init('mobile');
    store.refresh();
    wake.enable();
  }

  // Initialisation AVANT le rendu des sous-vues (elles s'abonnent au client WS).
  if (initiallyPaired && typeof location !== 'undefined' && !new URLSearchParams(location.search).get('pair')) start();

  function forget() {
    setDeviceToken(null);
    location.reload();
  }

  onMount(async () => {
    onViewport();
    window.visualViewport?.addEventListener('resize', onViewport);
    window.visualViewport?.addEventListener('scroll', onViewport);
    document.addEventListener('visibilitychange', onVisibility);
    const code = new URLSearchParams(location.search).get('pair');
    if (code) await pair(code);
    if (paired) start();
  });

  onDestroy(() => {
    window.visualViewport?.removeEventListener('resize', onViewport);
    window.visualViewport?.removeEventListener('scroll', onViewport);
    document.removeEventListener('visibilitychange', onVisibility);
    wake.disable();
    audio.release();
  });

  $effect(() => {
    if (store.conn === 'unauthorized') {
      setDeviceToken(null);
      paired = false;
      pairError = 'Cet appareil a été révoqué sur le PC. Scannez un nouveau QR code.';
    }
  });
</script>

<div
  class="fixed inset-x-0 flex flex-col overflow-hidden bg-slate-50 dark:bg-slate-950"
  style="top: {vv.top}px; height: {vv.height}px; min-height: 0"
>
  {#if !paired}
    <div class="flex flex-1 flex-col items-center justify-center gap-4 p-8 text-center">
      <div class="text-5xl">📱🔗💻</div>
      <h1 class="text-xl font-bold">DumonVoice — Compagnon</h1>
      {#if pairing}
        <p class="muted">Appairage en cours…</p>
      {:else}
        <p class="muted">
          Sur le PC, ouvrez les réglages DumonVoice (icône près de l'horloge → « Afficher le QR code mobile »), puis scannez
          le QR code avec l'appareil photo de ce téléphone.
        </p>
        {#if pairError}<p class="text-sm text-red-600">{pairError}</p>{/if}
        <a class="text-sm text-brand-600 underline" href="/setup-ca">Aide : certificat et installation</a>
      {/if}
    </div>
  {:else}
    <header class="flex items-center gap-2 border-b border-slate-200 bg-white px-3 py-2 dark:border-slate-800 dark:bg-slate-900">
      <span
        class="h-2.5 w-2.5 shrink-0 rounded-full {store.conn === 'open'
          ? 'bg-emerald-500'
          : store.conn === 'connecting'
            ? 'animate-pulse bg-amber-500'
            : 'bg-red-500'}"
      ></span>
      <div class="min-w-0 flex-1">
        {#if store.conn === 'open'}
          <p class="truncate text-sm font-semibold text-emerald-700 dark:text-emerald-400">Connecté à {store.pcName}</p>
        {:else}
          <button class="text-sm font-semibold text-red-600" onclick={() => store.ws?.reconnectNow()}>
            {store.conn === 'connecting' ? 'Connexion…' : 'Reconnecter'}
          </button>
        {/if}
      </div>
      <Segmented
        small
        value={mode}
        onchange={setMode}
        options={[
          { value: 'mic', label: '🎙️ Micro PC' },
          { value: 'text', label: '⌨️ Clavier / Dictée' },
        ]}
      />
      <button class="btn-ghost h-9 w-9 p-0" aria-label="Réglages" onclick={() => (showSettings = true)}>⚙</button>
    </header>

    <main class="min-h-0 flex-1">
      {#if mode === 'mic'}
        <MobileMicPad {audio} />
      {:else}
        <MobileTextPad />
      {/if}
    </main>
  {/if}
</div>

{#if showSettings}
  <div class="fixed inset-0 z-40 flex flex-col bg-slate-50 dark:bg-slate-950">
    <header class="flex items-center border-b border-slate-200 bg-white px-3 py-2 dark:border-slate-800 dark:bg-slate-900">
      <h2 class="flex-1 font-semibold">{fullSettings ? 'Réglages du PC' : 'Réglages'}</h2>
      <button
        class="btn-ghost"
        onclick={() => {
          if (fullSettings) fullSettings = false;
          else showSettings = false;
        }}>{fullSettings ? '← Retour' : 'Fermer'}</button
      >
    </header>
    <div class="flex-1 overflow-y-auto p-4">
      {#if fullSettings}
        <DesktopSettings embedded />
      {:else}
        <div class="card space-y-2">
          <Toggle
            label="Entrée envoie le texte"
            hint="Touche Entrée du clavier mobile = Envoyer (Maj+Entrée pour une nouvelle ligne)"
            checked={enterSends}
            onchange={(v) => {
              enterSends = v;
              try {
                localStorage.setItem('dv_enter_sends', v ? '1' : '0');
              } catch {
                /* ignore */
              }
              window.dispatchEvent(new Event('dv-settings'));
            }}
          />
          <button class="btn-ghost w-full" onclick={() => (fullSettings = true)}>Réglages complets du PC…</button>
          <a class="btn-ghost w-full" href="/setup-ca">Installer le certificat / l'application</a>
          <button class="btn-danger w-full" onclick={forget}>Oublier ce PC</button>
        </div>
        <p class="muted mt-4 text-center text-xs">
          Tout reste sur votre réseau local. La dictée du clavier du téléphone dépend des réglages de celui-ci.
        </p>
      {/if}
    </div>
  </div>
{/if}
