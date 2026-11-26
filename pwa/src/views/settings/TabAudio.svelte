<script lang="ts">
  import { onMount } from 'svelte';
  import { store } from '../../lib/store.svelte';
  import Toggle from '../../components/Toggle.svelte';
  import Vumeter from '../../components/Vumeter.svelte';

  let s = $derived(store.status!);
  let c = $derived(store.config!);
  const POS = [
    ['top_left', 'Haut gauche'],
    ['top_center', 'Haut centre'],
    ['top_right', 'Haut droite'],
    ['bottom_left', 'Bas gauche'],
    ['bottom_center', 'Bas centre'],
    ['bottom_right', 'Bas droite'],
  ];
  onMount(() => {
    store.refreshStatus(true);
  });
</script>

<div class="space-y-4">
  <section class="card">
    <h2 class="mb-3 font-semibold">🎙 Micro du PC</h2>
    <select
      class="input"
      value={c.audio.pc_device_id}
      onchange={(e) => store.patch({ audio: { pc_device_id: (e.target as HTMLSelectElement).value } })}
    >
      <option value="default">Périphérique par défaut de Windows{s.microphone.current ? ` (${s.microphone.current})` : ''}</option>
      {#each s.microphone.devices ?? [] as d (d)}
        <option value={d}>{d}</option>
      {/each}
    </select>
    <div class="mt-3">
      <Vumeter level={store.level} />
      <p class="muted mt-1 text-xs">
        Parlez pour tester le niveau. Bascule automatique si le casque est débranché. VAD : {s.vad === 'silero' ? 'Silero' : 'énergétique (Silero en cours de téléchargement)'}.
      </p>
    </div>
  </section>

  <section class="card">
    <h2 class="mb-2 font-semibold">Confort</h2>
    <div class="grid gap-4 sm:grid-cols-2">
      <div>
        <label class="label" for="pos">Position de l'indicateur</label>
        <select
          id="pos"
          class="input"
          value={c.ui.overlay_position}
          onchange={(e) => store.patch({ ui: { overlay_position: (e.target as HTMLSelectElement).value } })}
        >
          {#each POS as [v, l] (v)}<option value={v}>{l}</option>{/each}
        </select>
      </div>
      <div>
        <Toggle
          label="Indicateur flottant (HUD)"
          checked={c.ui.overlay_enabled}
          onchange={(v) => store.patch({ ui: { overlay_enabled: v } })}
        />
      </div>
    </div>
    <Toggle
      label="Sons de retour"
      hint="Début, fin, texte reçu du téléphone, erreur."
      checked={c.audio.sound_feedback}
      onchange={(v) => store.patch({ audio: { sound_feedback: v } })}
    />
    <Toggle
      label="Audio Ducking"
      hint="Baisse le volume des autres applications pendant l’écoute."
      checked={c.audio.duck_other_apps}
      onchange={(v) => store.patch({ audio: { duck_other_apps: v } })}
    />
    {#if c.audio.duck_other_apps}
      <label class="label mt-2" for="duck">Volume pendant l’écoute : {Math.round(c.audio.duck_level * 100)} %</label>
      <input
        id="duck"
        type="range"
        min="0"
        max="1"
        step="0.05"
        class="w-full"
        value={c.audio.duck_level}
        onchange={(e) => store.patch({ audio: { duck_level: +(e.target as HTMLInputElement).value } })}
      />
    {/if}
    <Toggle
      label="Démarrer avec Windows"
      checked={c.launch_at_startup}
      onchange={(v) => store.patch({ launch_at_startup: v })}
    />
  </section>
</div>
