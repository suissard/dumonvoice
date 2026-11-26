<script lang="ts">
  import { onMount } from 'svelte';
  import { api } from '../lib/api';
  import { store } from '../lib/store.svelte';
  import Vumeter from '../components/Vumeter.svelte';
  import QrPairing from '../components/QrPairing.svelte';
  import ProgressBar from '../components/ProgressBar.svelte';
  import { comboLabel } from '../lib/keys';

  let { ondone }: { ondone: () => void } = $props();
  let step = $state(1);
  let sandbox = $state('');
  let s = $derived(store.status);
  let c = $derived(store.config);

  // Recommandation silencieuse : Ollama présent → lfm3-audio ; sinon Whisper Équilibré
  // (Rapide sans GPU).
  let recommendOllama = $derived(!!s?.engine.ollama.reachable);
  let whisperFile = $derived(s?.gpu ? 'ggml-small-q5_1.bin' : 'ggml-base-q5_1.bin');
  let whisperProfile = $derived(s?.gpu ? 'balanced' : 'fast');
  let dl = $derived(
    recommendOllama ? store.downloads[`ollama:lfm3-audio`] : store.downloads[`whisper:${whisperFile}`],
  );
  let engineReady = $derived(
    !!s && (s.engine.active === 'whisper' ? s.engine.whisper.available : s.engine.ollama.reachable),
  );

  onMount(() => {
    store.refreshStatus(true);
  });

  async function applyEngine() {
    if (recommendOllama) {
      await store.patch({ engine: { active: 'ollama', ollama: { model: 'lfm3-audio' } } });
      const models = await api.ollamaModels().catch(() => null);
      if (!models?.models.some((m) => m.name.startsWith('lfm3-audio'))) await api.download('ollama', 'lfm3-audio');
    } else {
      await store.patch({ engine: { active: 'whisper', whisper: { profile: whisperProfile as any, model_file: whisperFile } } });
      if (!s?.engine.whisper.installed.includes(whisperFile)) await api.download('whisper', whisperFile);
    }
    step = 3;
  }

  async function finish() {
    await store.patch({ onboarding_done: true });
    ondone();
  }
</script>

<div class="mx-auto max-w-2xl px-4 py-10">
  <div class="mb-6 text-center">
    <div class="mx-auto mb-3 flex h-14 w-14 items-center justify-center rounded-2xl bg-brand-600 text-3xl text-white">🎙</div>
    <h1 class="text-2xl font-bold">Bienvenue dans DumonVoice</h1>
    <p class="muted">Trois étapes, moins d’une minute. Tout reste sur votre PC.</p>
  </div>
  <ol class="mb-6 flex justify-center gap-2" aria-label="Étapes">
    {#each [1, 2, 3] as n (n)}
      <li class="h-2 w-16 rounded-full {step >= n ? 'bg-brand-600' : 'bg-slate-300 dark:bg-slate-700'}"></li>
    {/each}
  </ol>

  {#if step === 1}
    <section class="card space-y-4">
      <h2 class="text-lg font-semibold">1. Micro & téléphone</h2>
      <div>
        <p class="label">Micro du PC : {s?.microphone.current ?? 'aucun détecté'}</p>
        <Vumeter level={store.level} />
        <p class="muted mt-1 text-xs">Dites quelques mots : la barre doit bouger.</p>
      </div>
      {#if c?.remote_mobile.enabled && s?.mobile.running}
        <div class="border-t border-slate-200 pt-4 dark:border-slate-800">
          <p class="mb-2 text-sm font-medium">Optionnel : utilisez votre téléphone comme micro sans fil et clavier</p>
          <QrPairing />
        </div>
      {/if}
      <div class="flex justify-end">
        <button class="btn-primary" onclick={() => (step = 2)}>Continuer</button>
      </div>
    </section>
  {:else if step === 2}
    <section class="card space-y-4">
      <h2 class="text-lg font-semibold">2. Moteur de transcription</h2>
      <p class="muted text-sm">
        {s?.gpu ? `GPU détecté : ${s.gpu}.` : 'Aucun GPU dédié détecté.'}
        {s?.engine.ollama.reachable ? `Ollama ${s.engine.ollama.version} est installé.` : 'Ollama n’est pas installé.'}
      </p>
      <div class="rounded-2xl border-2 border-brand-600 bg-brand-50 p-4 dark:bg-brand-700/20">
        {#if recommendOllama}
          <p class="font-semibold">Recommandé : Ollama avec lfm3-audio</p>
          <p class="muted text-xs">Dictée « propre » (sans hésitations) en une passe, repli automatique sur Whisper.</p>
        {:else}
          <p class="font-semibold">Recommandé : Whisper {s?.gpu ? 'Équilibré' : 'Rapide'}</p>
          <p class="muted text-xs">Modèle {whisperFile} téléchargé une seule fois, puis 100 % hors ligne.</p>
        {/if}
      </div>
      <ProgressBar state={dl} />
      <div class="flex justify-between">
        <button class="btn-ghost" onclick={() => (step = 1)}>Retour</button>
        <div class="flex gap-2">
          <button class="btn-ghost" onclick={() => (step = 3)}>Passer</button>
          <button class="btn-primary" onclick={applyEngine}>Utiliser ce moteur</button>
        </div>
      </div>
    </section>
  {:else}
    <section class="card space-y-4">
      <h2 class="text-lg font-semibold">3. Essai immédiat</h2>
      <p class="muted text-sm">
        Cliquez dans la zone, {c?.triggers.hotkey.mode === 'toggle' ? 'appuyez sur' : 'maintenez'}
        <kbd class="rounded bg-slate-200 px-1.5 py-0.5 text-xs dark:bg-slate-800">{comboLabel(c?.triggers.hotkey.keys ?? [])}</kbd>
        et parlez (ou envoyez un texte depuis le téléphone).
      </p>
      {#if dl && !dl.done}
        <ProgressBar state={dl} />
      {:else if !engineReady}
        <p class="text-sm text-amber-700">Le moteur n’est pas encore prêt.</p>
      {/if}
      <textarea class="input min-h-28" bind:value={sandbox} placeholder="Votre texte apparaîtra ici…"></textarea>
      {#if store.lastResult}
        <p class="text-sm text-emerald-700 dark:text-emerald-400">✔ Inséré en {store.lastResult.latency_ms} ms</p>
      {/if}
      <div class="flex justify-between">
        <button class="btn-ghost" onclick={() => (step = 2)}>Retour</button>
        <button class="btn-primary" onclick={finish}>Terminer — vous pouvez fermer cet onglet</button>
      </div>
    </section>
  {/if}
</div>
