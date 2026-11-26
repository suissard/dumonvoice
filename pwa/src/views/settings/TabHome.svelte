<script lang="ts">
  import { store } from '../../lib/store.svelte';
  import Toggle from '../../components/Toggle.svelte';

  let s = $derived(store.status!);
  let c = $derived(store.config!);
  let sandbox = $state('');

  const STATE_LABEL: Record<string, string> = {
    PAUSED: 'En pause',
    IDLE: 'Prêt',
    LISTENING: 'Écoute…',
    TRANSCRIBING: 'Transcription…',
    INJECTING: 'Insertion…',
    ERROR: 'Erreur',
  };

  let engineOk = $derived(
    s.engine.active === 'whisper' ? s.engine.whisper.available : s.engine.ollama.reachable,
  );
  let hotkey = $derived(c.triggers.hotkey.keys.join(' + '));
</script>

<div class="space-y-4">
  <section class="card">
    <div class="flex flex-wrap gap-2">
      <span
        class="rounded-full px-3 py-1 text-xs font-semibold {s.state === 'PAUSED'
          ? 'bg-slate-200 dark:bg-slate-800'
          : s.state === 'ERROR'
            ? 'bg-red-100 text-red-800'
            : 'bg-emerald-100 text-emerald-800 dark:bg-emerald-900/40 dark:text-emerald-300'}"
      >
        ● {STATE_LABEL[s.state] ?? s.state}
      </span>
      <span class="rounded-full bg-slate-100 px-3 py-1 text-xs font-semibold dark:bg-slate-800">
        🎙 {s.microphone.current ?? 'Aucun micro PC'}
      </span>
      <span
        class="rounded-full px-3 py-1 text-xs font-semibold {engineOk
          ? 'bg-slate-100 dark:bg-slate-800'
          : 'bg-amber-100 text-amber-800 dark:bg-amber-900/40 dark:text-amber-300'}"
      >
        ⚙ {s.engine.active === 'whisper' ? `Whisper · ${s.engine.whisper.model_file}` : `Ollama · ${s.engine.ollama.model}`}
        {engineOk ? '' : ' (indisponible)'}
      </span>
      <span class="rounded-full bg-slate-100 px-3 py-1 text-xs font-semibold dark:bg-slate-800">
        📱 {s.mobile.connected_devices.length ? 'Téléphone connecté' : s.mobile.enabled ? 'Aucun téléphone' : 'Mobile désactivé'}
      </span>
    </div>
    <div class="mt-4">
      <Toggle
        big
        label={c.enabled ? 'Dictée active' : 'Dictée en pause'}
        hint={c.enabled ? 'Raccourci, mot d’éveil et téléphone sont opérationnels.' : 'Aucun déclencheur n’est actif.'}
        checked={c.enabled}
        onchange={(v) => store.patch({ enabled: v })}
      />
    </div>
    {#if !engineOk}
      <p class="mt-2 rounded-xl bg-amber-50 p-3 text-sm text-amber-900 dark:bg-amber-900/30 dark:text-amber-200">
        Le moteur sélectionné n’est pas prêt : rendez-vous dans « Moteur & Langue » pour télécharger un modèle ou démarrer Ollama.
      </p>
    {/if}
  </section>

  <section class="card">
    <h2 class="mb-1 font-semibold">Bac à sable</h2>
    <p class="muted mb-3">
      Cliquez dans la zone puis {c.triggers.hotkey.mode === 'push_to_talk' ? 'maintenez' : 'appuyez sur'}
      <kbd class="rounded bg-slate-200 px-1.5 py-0.5 text-xs dark:bg-slate-800">{hotkey}</kbd>
      et parlez — ou envoyez un texte / une dictée depuis le téléphone.
    </p>
    <textarea class="input min-h-32" bind:value={sandbox} placeholder="Le texte dicté apparaîtra ici…"></textarea>
    {#if store.lastResult}
      <p class="muted mt-2 text-xs">
        Dernière injection : {store.lastResult.injected ? '✔' : '✖'}
        {store.lastResult.source === 'local_mic' ? 'micro PC' : store.lastResult.source === 'remote_audio' ? 'micro mobile' : 'texte mobile'}
        · latence <strong>{store.lastResult.latency_ms} ms</strong>
        {store.lastResult.target_window ? `· ${store.lastResult.target_window}` : ''}
      </p>
    {/if}
  </section>
</div>
