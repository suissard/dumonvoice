<script lang="ts">
  import { api } from '../../lib/api';
  import { store } from '../../lib/store.svelte';
  import type { PairedDevice } from '../../lib/types';
  import Toggle from '../../components/Toggle.svelte';
  import Segmented from '../../components/Segmented.svelte';
  import QrPairing from '../../components/QrPairing.svelte';
  import ProgressBar from '../../components/ProgressBar.svelte';
  import { keyLabel } from '../../lib/keys';

  let s = $derived(store.status!);
  let c = $derived(store.config!);
  let local = $derived(s.client.scope === 'local');
  let capturing = $state(false);
  let captureMsg = $state('');
  let devices = $state<PairedDevice[]>([]);
  let importName = $state('');



  async function startCapture() {
    capturing = true;
    captureMsg = 'Appuyez sur la touche ou la combinaison souhaitée… (Échap pour annuler)';
    store.hotkeyCaptured = null;
    try {
      await api.hotkeyCapture();
    } catch (e: any) {
      capturing = false;
      captureMsg = e?.message ?? 'Capture impossible';
    }
  }

  $effect(() => {
    const r = store.hotkeyCaptured;
    if (!r || !capturing) return;
    capturing = false;
    if (r.valid) {
      captureMsg = '';
      store.patch({ triggers: { hotkey: { keys: r.keys } } });
    } else {
      captureMsg = r.message ?? 'Combinaison refusée';
    }
  });

  async function loadDevices() {
    if (!local) return;
    try {
      devices = await api.devices();
    } catch {
      devices = [];
    }
  }

  $effect(() => {
    void store.devicesVersion;
    loadDevices();
  });

  async function revoke(id: string) {
    if (!confirm('Révoquer cet appareil ? Il devra être appairé à nouveau.')) return;
    await api.revoke(id);
    loadDevices();
  }

  async function importModel(e: Event) {
    const f = (e.target as HTMLInputElement).files?.[0];
    if (!f) return;
    const name = (importName || f.name.replace(/\.onnx$/i, '')).toLowerCase().replace(/[^a-z0-9_-]/g, '_').slice(0, 40);
    try {
      await api.importWakeword(name, f);
      store.toast(`Mot d'éveil « ${name} » importé`, 'success');
      await store.patch({ triggers: { wake_word: { model: name } } });
    } catch (err: any) {
      store.toast(err?.message ?? 'Import impossible', 'error');
    }
  }

  let wwInstalled = $derived(s.wake_word.words.find((w) => w.name === c.triggers.wake_word.model)?.installed ?? false);
</script>

<div class="space-y-4">
  <!-- Carte Clavier -->
  <section class="card">
    <h2 class="mb-3 font-semibold">⌨ Raccourci clavier</h2>
    <div class="flex flex-wrap items-center gap-3">
      <div class="flex flex-wrap gap-1">
        {#each c.triggers.hotkey.keys as k (k)}
          <kbd class="rounded-lg border border-slate-300 bg-slate-100 px-2.5 py-1 text-sm font-semibold dark:border-slate-700 dark:bg-slate-800"
            >{keyLabel(k)}</kbd
          >
        {/each}
      </div>
      {#if local}
        <button class="btn-ghost" onclick={startCapture} disabled={capturing}>
          {capturing ? 'En attente…' : 'Cliquer pour définir la touche'}
        </button>
      {/if}
    </div>
    {#if captureMsg}<p class="mt-2 text-sm {capturing ? 'text-brand-600' : 'text-red-600'}">{captureMsg}</p>{/if}
    <div class="mt-4 flex flex-wrap items-center gap-4">
      <Segmented
        value={c.triggers.hotkey.mode}
        onchange={(v) => store.patch({ triggers: { hotkey: { mode: v } } })}
        options={[
          { value: 'push_to_talk', label: 'Maintenir (Push-to-Talk)' },
          { value: 'toggle', label: 'Basculer' },
        ]}
      />
    </div>
    <Toggle
      label="Raccourci activé"
      checked={c.triggers.hotkey.enabled}
      onchange={(v) => store.patch({ triggers: { hotkey: { enabled: v } } })}
    />
    {#if c.triggers.hotkey.mode === 'toggle'}
      <Toggle
        label="Arrêt automatique sur silence"
        hint="En mode Basculer, la dictée s’arrête après le délai de silence ci-dessous."
        checked={c.triggers.hotkey.toggle_auto_stop}
        onchange={(v) => store.patch({ triggers: { hotkey: { toggle_auto_stop: v } } })}
      />
    {/if}
    <p class="muted mt-1 text-xs">Échap pendant l’écoute annule la dictée sans rien insérer.</p>
  </section>

  <!-- Carte Wake Word -->
  <section class="card">
    <h2 class="mb-1 font-semibold">🗣 Mot d’éveil</h2>
    {#if !s.wake_word.compiled}
      <p class="muted">Indisponible dans cette version (compilée sans ONNX Runtime).</p>
    {:else}
      <Toggle
        label="Écoute passive du mot d’éveil"
        hint="Détection 100 % locale, très faible consommation CPU."
        checked={c.triggers.wake_word.enabled}
        onchange={(v) => store.patch({ triggers: { wake_word: { enabled: v } } })}
      />
      <div class="mt-2 grid gap-4 sm:grid-cols-2">
        <div>
          <label class="label" for="ww">Mot</label>
          <select
            id="ww"
            class="input"
            value={c.triggers.wake_word.model}
            onchange={(e) => store.patch({ triggers: { wake_word: { model: (e.target as HTMLSelectElement).value } } })}
          >
            {#each s.wake_word.words as w (w.name)}
              <option value={w.name}>{w.name.replace(/_/g, ' ')}{w.builtin ? '' : ' (personnalisé)'}{w.installed ? '' : ' — à télécharger'}</option>
            {/each}
          </select>
          {#if c.triggers.wake_word.enabled && !wwInstalled}
            <ProgressBar state={store.downloads[`wakeword:${c.triggers.wake_word.model}`]} />
          {/if}
        </div>
        <div>
          <label class="label" for="sens">Sensibilité : {Math.round(c.triggers.wake_word.sensitivity * 100)} %</label>
          <input
            id="sens"
            type="range"
            min="0.1"
            max="0.95"
            step="0.05"
            class="w-full"
            value={c.triggers.wake_word.sensitivity}
            onchange={(e) => store.patch({ triggers: { wake_word: { sensitivity: +(e.target as HTMLInputElement).value } } })}
          />
        </div>
        <div>
          <label class="label" for="sil">Délai de silence (arrêt auto) : {(c.triggers.wake_word.silence_timeout_ms / 1000).toFixed(1)} s</label>
          <input
            id="sil"
            type="range"
            min="800"
            max="3000"
            step="100"
            class="w-full"
            value={c.triggers.wake_word.silence_timeout_ms}
            onchange={(e) =>
              store.patch({ triggers: { wake_word: { silence_timeout_ms: +(e.target as HTMLInputElement).value } } })}
          />
        </div>
        <div>
          <Toggle
            label="Pause pendant les appels visio"
            hint="Teams, Zoom, Discord…"
            checked={c.triggers.wake_word.pause_during_calls}
            onchange={(v) => store.patch({ triggers: { wake_word: { pause_during_calls: v } } })}
          />
        </div>
      </div>
      {#if local}
        <details class="mt-3">
          <summary class="muted cursor-pointer text-sm">Importer un modèle .onnx personnalisé (ex. « Hey Dictée »)</summary>
          <div class="mt-2 flex flex-wrap items-center gap-2">
            <input class="input max-w-48" placeholder="nom (ex. hey_dictee)" bind:value={importName} />
            <input type="file" accept=".onnx" onchange={importModel} class="text-sm" />
          </div>
        </details>
      {/if}
    {/if}
  </section>

  <!-- Carte Compagnon Téléphone -->
  <section class="card">
    <h2 class="mb-1 font-semibold">📱 Compagnon téléphone (Wi-Fi local)</h2>
    <Toggle
      label="Autoriser le téléphone"
      hint="Serveur HTTPS sur le réseau local uniquement (profil Privé du pare-feu)."
      checked={c.remote_mobile.enabled}
      onchange={(v) => store.patch({ remote_mobile: { enabled: v } })}
    />
    <Toggle
      label="Autoriser l’envoi de texte direct"
      hint="Clavier, dictée native et presse-papiers du téléphone."
      checked={c.remote_mobile.allow_direct_text}
      onchange={(v) => store.patch({ remote_mobile: { allow_direct_text: v } })}
    />
    {#if c.remote_mobile.enabled}
      {#if !s.mobile.running}
        <p class="mt-2 rounded-xl bg-amber-50 p-3 text-sm text-amber-900 dark:bg-amber-900/30 dark:text-amber-200">
          Le serveur mobile n’est pas démarré (port {s.mobile.port} occupé ou aucun réseau local).
        </p>
      {:else if local}
        <div class="mt-4 grid gap-6 sm:grid-cols-2">
          <QrPairing />
          <div>
            <p class="label">Appareils appairés</p>
            {#if devices.length === 0}
              <p class="muted">Aucun appareil.</p>
            {:else}
              <ul class="space-y-2">
                {#each devices as d (d.id)}
                  <li class="flex items-center gap-2 rounded-xl bg-slate-50 p-2 dark:bg-slate-800/60">
                    <span class="h-2 w-2 rounded-full {d.connected ? 'bg-emerald-500' : 'bg-slate-400'}"></span>
                    <span class="flex-1 text-sm">
                      {d.name}
                      <span class="muted block text-xs">appairé le {new Date(d.paired_at).toLocaleDateString()}</span>
                    </span>
                    <button class="btn-ghost px-3 py-1 text-xs text-red-600" onclick={() => revoke(d.id)}>Révoquer</button>
                  </li>
                {/each}
              </ul>
            {/if}
            <p class="muted mt-3 text-xs">
              Adresse : {s.mobile.lan_url ?? '—'} · {s.mobile.mdns}<br />
              Avertissement de certificat au premier accès ?
              <a class="text-brand-600 underline" href="/setup-ca">Aide certificat</a>
            </p>
          </div>
        </div>
      {/if}
    {/if}
  </section>
</div>
