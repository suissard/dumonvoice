<script lang="ts">
  import { api } from '../../lib/api';
  import { store } from '../../lib/store.svelte';
  import Toggle from '../../components/Toggle.svelte';
  import ProgressBar from '../../components/ProgressBar.svelte';

  let s = $derived(store.status!);
  let c = $derived(store.config!);
  let models = $state<{ name: string; size: number; audio_capable: boolean }[]>([]);
  let ollamaReachable = $state<boolean | null>(null);
  let pullName = $state('lfm3-audio');

  const PROFILES = [
    { id: 'fast', label: 'Rapide', hint: 'PC portable sans GPU' },
    { id: 'balanced', label: 'Équilibré', hint: 'Recommandé' },
    { id: 'accurate', label: 'Haute précision', hint: 'GPU dédié' },
  ] as const;

  const LANGS = [
    ['auto', 'Automatique'],
    ['fr', 'Français'],
    ['en', 'Anglais'],
    ['es', 'Espagnol'],
    ['de', 'Allemand'],
    ['it', 'Italien'],
    ['pt', 'Portugais'],
    ['nl', 'Néerlandais'],
  ];

  async function loadModels() {
    try {
      const r = await api.ollamaModels();
      models = r.models;
      ollamaReachable = r.reachable;
    } catch {
      ollamaReachable = false;
    }
  }
  loadModels();

  function setProfile(p: 'fast' | 'balanced' | 'accurate') {
    const file = s.engine.whisper.catalog.filter((m) => m.profile === p)[p === 'fast' ? 1 : 0]?.file;
    store.patch({ engine: { whisper: { profile: p, ...(file ? { model_file: file } : {}) } } });
  }

  async function download(kind: 'whisper' | 'ollama', model: string) {
    try {
      await api.download(kind, model);
    } catch (e: any) {
      store.toast(e?.message ?? 'Téléchargement impossible', 'error');
    }
  }

  let wModel = $derived(c.engine.whisper.model_file);
  let wInstalled = $derived(s.engine.whisper.installed.includes(wModel));
  let wDl = $derived(store.downloads[`whisper:${wModel}`]);
  let pullDl = $derived(store.downloads[`ollama:${pullName}`]);
  $effect(() => {
    if (pullDl?.done && !pullDl.error) loadModels();
  });
</script>

<div class="space-y-4">
  <section class="card">
    <h2 class="mb-3 font-semibold">Moteur de transcription</h2>
    <div class="grid gap-3 sm:grid-cols-2">
      <button
        class="rounded-2xl border-2 p-4 text-left transition {c.engine.active === 'whisper'
          ? 'border-brand-600 bg-brand-50 dark:bg-brand-700/20'
          : 'border-slate-200 dark:border-slate-800'}"
        onclick={() => store.patch({ engine: { active: 'whisper' } })}
      >
        <p class="font-semibold">Classique (Whisper intégré)</p>
        <p class="muted text-xs">whisper.cpp local, accélération GPU automatique.</p>
      </button>
      <button
        class="rounded-2xl border-2 p-4 text-left transition {c.engine.active === 'ollama'
          ? 'border-brand-600 bg-brand-50 dark:bg-brand-700/20'
          : 'border-slate-200 dark:border-slate-800'}"
        onclick={() => store.patch({ engine: { active: 'ollama' } })}
      >
        <p class="font-semibold">Ollama (IA locale)</p>
        <p class="muted text-xs">
          Modèle audio multimodal · {s.engine.ollama.reachable ? `Ollama ${s.engine.ollama.version} détecté` : 'Ollama non détecté'}
        </p>
      </button>
    </div>
  </section>

  {#if c.engine.active === 'whisper'}
    <section class="card">
      <h3 class="mb-2 font-semibold">Whisper</h3>
      {#if !s.engine.whisper.compiled}
        <p class="text-sm text-amber-700">Cette version a été compilée sans Whisper intégré : utilisez Ollama.</p>
      {/if}
      <div class="grid gap-2 sm:grid-cols-3">
        {#each PROFILES as p (p.id)}
          <button
            class="rounded-xl border p-3 text-left {c.engine.whisper.profile === p.id
              ? 'border-brand-600 bg-brand-50 dark:bg-brand-700/20'
              : 'border-slate-200 dark:border-slate-800'}"
            onclick={() => setProfile(p.id)}
          >
            <p class="text-sm font-semibold">{p.label}</p>
            <p class="muted text-xs">{p.hint}</p>
          </button>
        {/each}
      </div>
      <div class="mt-3 flex flex-wrap items-center gap-3">
        <select
          class="input max-w-xs"
          value={wModel}
          onchange={(e) => store.patch({ engine: { whisper: { model_file: (e.target as HTMLSelectElement).value } } })}
        >
          {#each s.engine.whisper.catalog as m (m.file)}
            <option value={m.file}>{m.file} ({m.size_mb} Mo){s.engine.whisper.installed.includes(m.file) ? ' ✔' : ''}</option>
          {/each}
        </select>
        {#if wInstalled}
          <span class="text-sm text-emerald-600">✔ Installé</span>
        {:else}
          <button class="btn-primary" disabled={!!wDl && !wDl.done} onclick={() => download('whisper', wModel)}>
            Télécharger
          </button>
        {/if}
      </div>
      <ProgressBar state={wDl} />
      <Toggle
        label="Utiliser le GPU"
        hint={s.gpu ? `GPU détecté : ${s.gpu}` : 'Aucun GPU dédié détecté (CPU optimisé AVX2/INT8).'}
        checked={c.engine.whisper.use_gpu}
        onchange={(v) => store.patch({ engine: { whisper: { use_gpu: v } } })}
      />
      <Toggle
        label="Post-traitement par Ollama (pipeline hybride)"
        hint="Ponctuation et nettoyage de la sortie Whisper par un modèle texte local."
        checked={c.engine.post_process_with_ollama}
        disabled={!s.engine.ollama.reachable}
        onchange={(v) => store.patch({ engine: { post_process_with_ollama: v } })}
      />
    </section>
  {:else}
    <section class="card">
      <h3 class="mb-2 font-semibold">Ollama</h3>
      <div class="grid gap-3 sm:grid-cols-2">
        <div>
          <label class="label" for="ourl">URL de l'API</label>
          <input
            id="ourl"
            class="input"
            value={c.engine.ollama.base_url}
            onchange={(e) => store.patch({ engine: { ollama: { base_url: (e.target as HTMLInputElement).value } } }).then(loadModels)}
          />
        </div>
        <div>
          <label class="label" for="omodel">Modèle audio</label>
          <select
            id="omodel"
            class="input"
            value={c.engine.ollama.model}
            onchange={(e) => store.patch({ engine: { ollama: { model: (e.target as HTMLSelectElement).value } } })}
          >
            {#if !models.some((m) => m.name === c.engine.ollama.model)}
              <option value={c.engine.ollama.model}>{c.engine.ollama.model} (non installé)</option>
            {/if}
            {#each models as m (m.name)}
              <option value={m.name}>{m.name}{m.audio_capable ? ' 🎧' : ''}</option>
            {/each}
          </select>
        </div>
        <div>
          <label class="label" for="otext">Modèle texte (nettoyage ✨ IA)</label>
          <select
            id="otext"
            class="input"
            value={c.engine.ollama.text_model}
            onchange={(e) => store.patch({ engine: { ollama: { text_model: (e.target as HTMLSelectElement).value } } })}
          >
            <option value="">(même que le modèle audio)</option>
            {#if c.engine.ollama.text_model && !models.some((m) => m.name === c.engine.ollama.text_model)}
              <option value={c.engine.ollama.text_model}>{c.engine.ollama.text_model} (non installé)</option>
            {/if}
            {#each models as m (m.name)}
              <option value={m.name}>{m.name}</option>
            {/each}
          </select>
        </div>
        <div>
          <label class="label" for="pull">ollama pull</label>
          <div class="flex gap-2">
            <input id="pull" class="input" bind:value={pullName} />
            <button class="btn-primary" disabled={!ollamaReachable || (!!pullDl && !pullDl.done)} onclick={() => download('ollama', pullName)}
              >Installer</button
            >
          </div>
          <ProgressBar state={pullDl} />
        </div>
      </div>
      {#if ollamaReachable === false}
        <p class="mt-3 rounded-xl bg-amber-50 p-3 text-sm text-amber-900 dark:bg-amber-900/30 dark:text-amber-200">
          Ollama est injoignable à {c.engine.ollama.base_url}. Installez-le depuis ollama.com puis lancez-le.
        </p>
      {/if}
      <Toggle
        label="Mode « Dictée propre »"
        hint="Supprime les hésitations et ponctue naturellement en une seule passe."
        checked={c.engine.ollama.clean_dictation}
        onchange={(v) => store.patch({ engine: { ollama: { clean_dictation: v } } })}
      />
      <Toggle
        label="Repli automatique sur Whisper"
        hint="Si Ollama est indisponible ou dépasse 15 s."
        checked={c.engine.ollama.fallback_to_whisper}
        onchange={(v) => store.patch({ engine: { ollama: { fallback_to_whisper: v } } })}
      />
    </section>
  {/if}

  <section class="card">
    <label class="label" for="lang">Langue de dictée</label>
    <select
      id="lang"
      class="input max-w-xs"
      value={c.language}
      onchange={(e) => store.patch({ language: (e.target as HTMLSelectElement).value })}
    >
      {#each LANGS as [v, l] (v)}
        <option value={v}>{l}</option>
      {/each}
    </select>
  </section>
</div>
