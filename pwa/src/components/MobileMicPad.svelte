<script lang="ts">
  import { onDestroy } from 'svelte';
  import { api } from '../lib/api';
  import { haptic, MobileAudio } from '../lib/mobileAudio';
  import { errorText, store } from '../lib/store.svelte';
  import { uuidv4 } from '../lib/uuid';
  import Segmented from './Segmented.svelte';

  let { audio }: { audio: MobileAudio } = $props();

  type Phase = 'idle' | 'starting' | 'listening' | 'processing';
  let phase = $state<Phase>('idle');
  let level = $state(0);
  let lastText = $state<string>(loadLast());
  let mode = $state<'ptt' | 'toggle'>(loadMode());
  let sessionId: string | null = null;
  let pressing = false;
  let processingTimer: ReturnType<typeof setTimeout> | null = null;

  function loadMode(): 'ptt' | 'toggle' {
    try {
      return localStorage.getItem('dv_mic_mode') === 'toggle' ? 'toggle' : 'ptt';
    } catch {
      return 'ptt';
    }
  }
  function loadLast(): string {
    try {
      return localStorage.getItem('dv_last_text') ?? '';
    } catch {
      return '';
    }
  }
  function setMode(m: 'ptt' | 'toggle') {
    mode = m;
    try {
      localStorage.setItem('dv_mic_mode', m);
    } catch {
      /* ignore */
    }
  }

  $effect(() => {
    audio.onChunk = (buf) => {
      store.ws?.sendBinary(buf);
    };
    audio.onLevel = (rms) => {
      level = rms;
    };
  });

  const off = store.ws?.onEvent((e) => {
    if (!sessionId) return;
    if (e.type === 'injection_result' && e.id === sessionId) {
      done();
      lastText = e.text;
      try {
        localStorage.setItem('dv_last_text', e.text);
      } catch {
        /* ignore */
      }
      haptic(e.injected ? 25 : [20, 60, 20]);
    } else if (e.type === 'error' && e.id === sessionId) {
      done();
      if (e.code === 'EMPTY_AUDIO') store.toast('Aucune parole détectée', 'info');
      else store.toast(errorText(e.code, e.message), 'error');
      haptic([40, 40, 40]);
    } else if (e.type === 'state_changed' && e.session_id === sessionId && phase === 'listening') {
      // Arrêt automatique (VAD) décidé par le PC en mode « Appuyer ».
      if (e.state === 'TRANSCRIBING') {
        audio.stop();
        phase = 'processing';
        armProcessingTimeout();
      } else if (e.state === 'IDLE') {
        done();
      }
    }
  });

  function done() {
    audio.stop();
    phase = 'idle';
    sessionId = null;
    if (processingTimer) clearTimeout(processingTimer);
    processingTimer = null;
  }

  function armProcessingTimeout() {
    if (processingTimer) clearTimeout(processingTimer);
    processingTimer = setTimeout(() => {
      if (phase === 'processing') {
        done();
        store.toast('Pas de réponse du PC', 'warn');
      }
    }, 25000);
  }

  async function begin(ready: Promise<void>) {
    if (store.conn !== 'open') {
      store.toast('Non connecté au PC', 'warn');
      return;
    }
    phase = 'starting';
    try {
      await ready;
    } catch {
      phase = 'idle';
      store.toast('Accès au micro refusé (vérifiez la connexion HTTPS et les autorisations)', 'error');
      return;
    }
    if (mode === 'ptt' && !pressing) {
      phase = 'idle';
      return;
    }
    sessionId = uuidv4();
    const ok = store.ws?.send({
      type: 'session_start',
      session_id: sessionId,
      audio_format: { encoding: 'pcm_s16le', sample_rate: 16000, channels: 1 },
      trigger_mode: mode === 'ptt' ? 'push_to_talk' : 'toggle',
      auto_stop_vad: mode === 'toggle',
    });
    if (!ok) {
      phase = 'idle';
      sessionId = null;
      return;
    }
    audio.start();
    phase = 'listening';
    haptic(15);
  }

  function finish() {
    if (phase !== 'listening' || !sessionId) {
      if (phase === 'starting') phase = 'idle';
      return;
    }
    audio.stop();
    store.ws?.send({ type: 'session_stop', session_id: sessionId });
    phase = 'processing';
    haptic([10, 30, 10]);
    armProcessingTimeout();
  }

  function cancel() {
    if (sessionId && phase === 'listening') {
      store.ws?.send({ type: 'session_cancel', session_id: sessionId });
      haptic([30, 30]);
    }
    done();
  }

  function onDown(e: PointerEvent) {
    e.preventDefault();
    // Libère la capture implicite (tactile) pour détecter la sortie du bouton.
    try {
      (e.currentTarget as HTMLElement).releasePointerCapture(e.pointerId);
    } catch {
      /* ignore */
    }
    const ready = audio.prime(); // DOIT rester synchrone dans le geste (iOS)
    if (mode === 'ptt') {
      pressing = true;
      begin(ready);
    } else if (phase === 'listening') {
      finish();
    } else if (phase === 'idle') {
      begin(ready);
    }
  }

  function onUp() {
    if (mode !== 'ptt' || !pressing) return;
    pressing = false;
    finish();
  }

  function onLeave() {
    if (mode !== 'ptt' || !pressing) return;
    pressing = false;
    cancel();
  }

  async function quick(action: 'enter' | 'undo_last') {
    haptic(10);
    if (!store.ws?.send({ type: 'action', action })) {
      try {
        await api.action(action);
      } catch (e: any) {
        store.toast(e?.message ?? 'Action impossible', 'error');
      }
    }
  }

  onDestroy(() => {
    off?.();
    cancel();
  });

  let scale = $derived(phase === 'listening' ? 1 + Math.min(level * 4, 0.6) : 1);
</script>

<div class="flex h-full flex-col">
  <div class="flex flex-1 flex-col items-center justify-center gap-6 px-6">
    <div class="relative flex h-64 w-64 items-center justify-center">
      <div
        class="absolute inset-0 rounded-full transition-transform duration-75
          {phase === 'listening' ? 'bg-red-500/20' : 'bg-brand-500/10'}"
        style="transform: scale({scale})"
      ></div>
      <button
        type="button"
        aria-label={phase === 'listening' ? 'Arrêter la dictée' : 'Démarrer la dictée'}
        class="relative z-10 flex h-48 w-48 touch-none select-none items-center justify-center rounded-full text-white shadow-xl
          transition active:scale-95
          {phase === 'listening'
          ? 'bg-red-600'
          : phase === 'processing'
            ? 'bg-amber-500'
            : 'bg-brand-600'}"
        onpointerdown={onDown}
        onpointerup={onUp}
        onpointerleave={onLeave}
        onpointercancel={onLeave}
        oncontextmenu={(e) => e.preventDefault()}
      >
        {#if phase === 'processing' || phase === 'starting'}
          <svg class="h-14 w-14 animate-spin" viewBox="0 0 24 24" fill="none">
            <circle cx="12" cy="12" r="9" stroke="currentColor" stroke-width="3" opacity="0.3" />
            <path d="M21 12a9 9 0 0 0-9-9" stroke="currentColor" stroke-width="3" stroke-linecap="round" />
          </svg>
        {:else}
          <svg class="h-20 w-20" viewBox="0 0 24 24" fill="currentColor" aria-hidden="true">
            <path d="M12 14a3 3 0 0 0 3-3V5a3 3 0 0 0-6 0v6a3 3 0 0 0 3 3Z" />
            <path
              d="M19 11a1 1 0 1 0-2 0 5 5 0 0 1-10 0 1 1 0 1 0-2 0 7 7 0 0 0 6 6.92V20H8a1 1 0 1 0 0 2h8a1 1 0 1 0 0-2h-3v-2.08A7 7 0 0 0 19 11Z"
            />
          </svg>
        {/if}
      </button>
    </div>

    <p class="muted h-5 text-center">
      {#if phase === 'listening'}
        {mode === 'ptt' ? 'Parlez… relâchez pour envoyer (glissez hors du bouton pour annuler)' : 'Parlez… touchez pour terminer'}
      {:else if phase === 'processing'}
        {store.modelLoading ? 'Chargement du modèle…' : 'Transcription sur le PC…'}
      {:else}
        {mode === 'ptt' ? 'Maintenez pour parler' : 'Touchez pour démarrer'}
      {/if}
    </p>

    <div class="flex items-center gap-3">
      <Segmented
        small
        value={mode}
        onchange={setMode}
        options={[
          { value: 'ptt', label: 'Maintenir' },
          { value: 'toggle', label: 'Appuyer' },
        ]}
      />
      {#if phase === 'listening'}
        <button class="btn-ghost px-3 py-1 text-xs" onclick={cancel}>Annuler</button>
      {/if}
    </div>
  </div>

  <div class="border-t border-slate-200 bg-white/80 p-3 backdrop-blur dark:border-slate-800 dark:bg-slate-900/80">
    <p class="muted mb-2 truncate text-sm" title={lastText}>
      {lastText ? `Dernier : « ${lastText} »` : 'Aucune dictée pour le moment'}
    </p>
    <div class="grid grid-cols-2 gap-2">
      <button class="btn-ghost" onclick={() => quick('enter')}>↵ Entrée</button>
      <button class="btn-ghost" onclick={() => quick('undo_last')}>⌫ Annuler sur PC</button>
    </div>
  </div>
</div>
