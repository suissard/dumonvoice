<script lang="ts">
  import { onMount } from 'svelte';
  import { api } from '../lib/api';
  import { haptic } from '../lib/mobileAudio';
  import { errorText, store } from '../lib/store.svelte';
  import { uuidv4 } from '../lib/uuid';
  import { AckError } from '../lib/wsClient';
  import type { InjectionResult } from '../lib/types';

  const DRAFT = 'dv_draft';
  const HIST = 'dv_text_history';

  function ls(key: string, def: string): string {
    try {
      return localStorage.getItem(key) ?? def;
    } catch {
      return def;
    }
  }
  function lsSet(key: string, v: string) {
    try {
      localStorage.setItem(key, v);
    } catch {
      /* ignore */
    }
  }

  let text = $state(loadDraft());
  let sending = $state(false);
  let autoEnter = $state(ls('dv_auto_enter', '0') === '1');
  let refine = $state(ls('dv_refine', '0') === '1');
  let enterSends = $state(ls('dv_enter_sends', '0') === '1');
  let history = $state<string[]>(JSON.parse(ls(HIST, '[]')));
  let area: HTMLTextAreaElement | undefined = $state();

  let ollamaOk = $derived(!!store.status?.engine.ollama.reachable);
  let canSend = $derived(text.trim().length > 0 && !sending);

  function loadDraft(): string {
    try {
      return sessionStorage.getItem(DRAFT) ?? '';
    } catch {
      return '';
    }
  }

  // Brouillon sauvegardé en continu (anti-perte).
  $effect(() => {
    try {
      sessionStorage.setItem(DRAFT, text);
    } catch {
      /* ignore */
    }
  });

  $effect(() => {
    // auto-extension du textarea
    void text;
    if (area) {
      area.style.height = 'auto';
      area.style.height = Math.min(area.scrollHeight, window.innerHeight * 0.45) + 'px';
    }
  });

  async function deliver(body: string, opts: { enter: boolean; refine: boolean }): Promise<InjectionResult> {
    const message_id = uuidv4();
    const msg = {
      message_id,
      text: body,
      press_enter_after: opts.enter,
      refine_with_ollama: opts.refine && ollamaOk,
    };
    if (store.ws?.isOpen) {
      try {
        return await store.ws.request(message_id, { type: 'inject_text', ...msg });
      } catch (e) {
        // Wi-Fi hésitant : nouvelle tentative REST avec le même message_id (idempotent).
        if (!(e instanceof AckError) || !['OFFLINE', 'TIMEOUT'].includes(e.code)) throw e;
      }
    }
    return await api.injectText(msg);
  }

  function remember(t: string) {
    history = [t, ...history.filter((h) => h !== t)].slice(0, 3);
    lsSet(HIST, JSON.stringify(history));
  }

  async function send(body = text, fromDraft = true) {
    if (!body.trim() || sending) return;
    sending = true;
    try {
      const r = await deliver(body, { enter: autoEnter, refine });
      remember(r.text || body);
      if (r.injected) {
        // Le champ n'est vidé qu'après l'ACK positif du PC.
        if (fromDraft) text = '';
        haptic(20);
      } else {
        haptic([20, 60, 20]);
      }
    } catch (e: any) {
      const code = e?.code ?? '';
      store.toast(errorText(code, e?.message ?? 'Envoi impossible : brouillon conservé'), code === 'BUSY' ? 'warn' : 'error');
      haptic([40, 40, 40]);
    } finally {
      sending = false;
    }
  }

  async function paste() {
    try {
      const clip = await navigator.clipboard.readText();
      if (!clip) return;
      if (!text.trim()) {
        // Champ vide : « Coller & Envoyer ».
        text = clip;
        await send(clip);
      } else {
        text = text + (text.endsWith(' ') || text.endsWith('\n') ? '' : ' ') + clip;
      }
    } catch {
      store.toast('Lecture du presse-papiers refusée : collez dans la zone (appui long)', 'warn');
      area?.focus();
    }
  }

  function onKey(e: KeyboardEvent) {
    if (enterSends && e.key === 'Enter' && !e.shiftKey && !e.isComposing) {
      e.preventDefault();
      send();
    }
  }

  function toggle(key: string, v: boolean) {
    lsSet(key, v ? '1' : '0');
  }

  async function undo() {
    haptic(10);
    if (!store.ws?.send({ type: 'action', action: 'undo_last' })) {
      try {
        await api.action('undo_last');
      } catch (e: any) {
        store.toast(e?.message ?? 'Action impossible', 'error');
      }
    }
  }

  async function copy(t: string) {
    try {
      await navigator.clipboard.writeText(t);
      store.toast('Copié', 'success');
    } catch {
      /* ignore */
    }
  }

  onMount(() => {
    const onStorage = () => (enterSends = ls('dv_enter_sends', '0') === '1');
    window.addEventListener('dv-settings', onStorage);
    return () => window.removeEventListener('dv-settings', onStorage);
  });
</script>

<div class="flex h-full flex-col">
  <div class="flex-1 overflow-y-auto px-3 pt-3">
    {#if history.length}
      <p class="label">Derniers envois</p>
      <ul class="space-y-2">
        {#each history as h (h)}
          <li class="flex items-center gap-2 rounded-xl bg-white p-2 text-sm shadow-sm dark:bg-slate-900">
            <span class="min-w-0 flex-1 truncate" title={h}>{h}</span>
            <button class="btn-ghost px-2 py-1 text-xs" onclick={() => send(h, false)} disabled={sending} aria-label="Ré-injecter">↻</button>
            <button class="btn-ghost px-2 py-1 text-xs" onclick={() => copy(h)} aria-label="Copier">⧉</button>
          </li>
        {/each}
      </ul>
      <button class="btn-ghost mt-2 w-full text-xs" onclick={undo}>⌫ Annuler sur PC</button>
    {:else}
      <p class="muted mt-6 text-center">
        Tapez, collez ou utilisez le micro de votre clavier (Gboard, dictée iOS) : le texte s'écrit dans la fenêtre active du PC.
      </p>
    {/if}
  </div>

  <div class="border-t border-slate-200 bg-white p-2 dark:border-slate-800 dark:bg-slate-900">
    <textarea
      bind:this={area}
      bind:value={text}
      onkeydown={onKey}
      rows="2"
      class="input resize-none text-base"
      placeholder="Tapez ou utilisez le micro de votre clavier…"
      enterkeyhint={enterSends ? 'send' : 'enter'}
      autocapitalize="sentences"
      spellcheck="true"
    ></textarea>
    <div class="mt-2 flex items-center gap-2">
      <button class="btn-ghost px-3 text-xs" onclick={paste} disabled={sending}>
        📋 {text.trim() ? 'Coller' : 'Coller & Envoyer'}
      </button>
      <button
        class="btn px-3 text-xs {autoEnter ? 'bg-brand-100 text-brand-700 dark:bg-brand-700/30 dark:text-brand-100' : 'bg-slate-100 dark:bg-slate-800'}"
        aria-pressed={autoEnter}
        onclick={() => {
          autoEnter = !autoEnter;
          toggle('dv_auto_enter', autoEnter);
        }}>↵ Auto</button
      >
      {#if ollamaOk}
        <button
          class="btn px-3 text-xs {refine ? 'bg-violet-100 text-violet-700 dark:bg-violet-700/30 dark:text-violet-100' : 'bg-slate-100 dark:bg-slate-800'}"
          aria-pressed={refine}
          title="Ponctuation / reformulation par l'IA du PC"
          onclick={() => {
            refine = !refine;
            toggle('dv_refine', refine);
          }}>✨ IA</button
        >
      {/if}
      <button class="btn-primary ml-auto px-5" onclick={() => send()} disabled={!canSend}>
        {#if sending}
          <svg class="h-4 w-4 animate-spin" viewBox="0 0 24 24" fill="none">
            <circle cx="12" cy="12" r="9" stroke="currentColor" stroke-width="3" opacity="0.3" />
            <path d="M21 12a9 9 0 0 0-9-9" stroke="currentColor" stroke-width="3" stroke-linecap="round" />
          </svg>
        {:else}
          Envoyer ➤
        {/if}
      </button>
    </div>
  </div>
</div>
