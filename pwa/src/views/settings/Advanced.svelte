<script lang="ts">
  import { api } from '../../lib/api';
  import { store } from '../../lib/store.svelte';
  import type { HistoryEntry } from '../../lib/types';
  import Toggle from '../../components/Toggle.svelte';

  let c = $derived(store.config!);
  let open = $state(false);
  let history = $state<HistoryEntry[]>([]);
  let newFrom = $state('');
  let newTo = $state('');
  let prompt = $state(store.config?.engine.ollama.custom_system_prompt ?? '');

  async function loadHistory() {
    try {
      history = await api.history();
    } catch {
      history = [];
    }
  }

  $effect(() => {
    if (open) {
      void store.lastResult;
      loadHistory();
    }
  });

  function addWord() {
    if (!newFrom.trim() || !newTo.trim()) return;
    store.patch({ injection: { custom_dictionary: { [newFrom.trim()]: newTo.trim() } } });
    newFrom = '';
    newTo = '';
  }

  function removeWord(k: string) {
    // JSON Merge Patch : null supprime la clé.
    store.patch({ injection: { custom_dictionary: { [k]: null } } });
  }

  const num = (e: Event) => +(e.target as HTMLInputElement).value;
</script>

<details class="card mt-4" bind:open>
  <summary class="cursor-pointer font-semibold">Avancé</summary>
  <div class="mt-4 space-y-6">
    <div>
      <label class="label" for="prompt">Prompt système Ollama personnalisé</label>
      <textarea
        id="prompt"
        class="input min-h-24"
        bind:value={prompt}
        placeholder="Laisser vide pour le prompt ASR strict par défaut"
        onchange={() => store.patch({ engine: { ollama: { custom_system_prompt: prompt.trim() ? prompt : null } } })}
      ></textarea>
    </div>

    <div class="grid gap-4 sm:grid-cols-2">
      <div>
        <label class="label" for="method">Méthode d'injection</label>
        <select
          id="method"
          class="input"
          value={c.injection.method}
          onchange={(e) => store.patch({ injection: { method: (e.target as HTMLSelectElement).value as any } })}
        >
          <option value="clipboard_paste">Presse-papiers sécurisé (recommandé)</option>
          <option value="send_input">Frappe simulée (SendInput)</option>
        </select>
        <p class="muted mt-1 text-xs">Le presse-papiers est forcé pour les textes longs, multi-lignes ou avec emojis.</p>
      </div>
      <div>
        <label class="label" for="pd">Délai avant restauration du presse-papiers (ms)</label>
        <input id="pd" type="number" min="50" max="2000" class="input" value={c.injection.paste_delay_ms}
          onchange={(e) => store.patch({ injection: { paste_delay_ms: num(e) } })} />
      </div>
    </div>
    <Toggle
      label="Restaurer le presse-papiers d’origine"
      checked={c.injection.restore_clipboard}
      onchange={(v) => store.patch({ injection: { restore_clipboard: v } })}
    />
    <Toggle
      label="Vérifier la fenêtre cible"
      hint="Si vous changez de fenêtre pendant la transcription, le texte est copié au lieu d’être inséré."
      checked={c.injection.check_window_focus}
      onchange={(v) => store.patch({ injection: { check_window_focus: v } })}
    />

    <div>
      <p class="label">Dictionnaire personnalisé (noms propres, termes métier)</p>
      <ul class="mb-2 space-y-1">
        {#each Object.entries(c.injection.custom_dictionary) as [k, v] (k)}
          <li class="flex items-center gap-2 text-sm">
            <span class="flex-1"><code>{k}</code> → <strong>{v}</strong></span>
            <button class="btn-ghost px-2 py-0.5 text-xs" onclick={() => removeWord(k)}>✕</button>
          </li>
        {/each}
      </ul>
      <div class="flex flex-wrap gap-2">
        <input class="input max-w-48" placeholder="entendu (ex. dumon voice)" bind:value={newFrom} />
        <input class="input max-w-48" placeholder="écrit (ex. DumonVoice)" bind:value={newTo} />
        <button class="btn-ghost" onclick={addWord}>Ajouter</button>
      </div>
    </div>

    <div>
      <Toggle
        label="Mini-historique local (10 dernières dictées, en mémoire)"
        checked={c.ui.keep_history_count > 0}
        onchange={(v) => store.patch({ ui: { keep_history_count: v ? 10 : 0 } })}
      />
      {#if c.ui.keep_history_count > 0 && history.length}
        <ul class="mt-2 space-y-1 text-sm">
          {#each history as h (h.id)}
            <li class="flex gap-2 rounded-lg bg-slate-50 px-2 py-1 dark:bg-slate-800/60">
              <span class="muted shrink-0 text-xs">{new Date(h.at).toLocaleTimeString()}</span>
              <span class="flex-1 truncate" title={h.text}>{h.text}</span>
              <span class="shrink-0">{h.injected ? '✔' : '⧉'}</span>
            </li>
          {/each}
        </ul>
      {/if}
    </div>

    <div class="grid gap-4 sm:grid-cols-3">
      <div>
        <label class="label" for="lp">Port local</label>
        <input id="lp" type="number" class="input" value={c.server.local_port}
          onchange={(e) => store.patch({ server: { local_port: num(e) } })} />
        <p class="muted mt-1 text-xs">Pris en compte au prochain démarrage.</p>
      </div>
      <div>
        <label class="label" for="lanp">Port LAN (HTTPS)</label>
        <input id="lanp" type="number" class="input" value={c.remote_mobile.lan_port}
          onchange={(e) => store.patch({ remote_mobile: { lan_port: num(e) } })} />
      </div>
      <div>
        <label class="label" for="mdns">Nom mDNS (.local)</label>
        <input id="mdns" class="input" value={c.remote_mobile.mdns_name}
          onchange={(e) => store.patch({ remote_mobile: { mdns_name: (e.target as HTMLInputElement).value } })} />
      </div>
      <div>
        <label class="label" for="ka">Ollama keep_alive</label>
        <input id="ka" class="input" value={c.engine.ollama.keep_alive}
          onchange={(e) => store.patch({ engine: { ollama: { keep_alive: (e.target as HTMLInputElement).value } } })} />
      </div>
      <div>
        <label class="label" for="api">API Ollama</label>
        <select id="api" class="input" value={c.engine.ollama.api_style}
          onchange={(e) => store.patch({ engine: { ollama: { api_style: (e.target as HTMLSelectElement).value as any } } })}>
          <option value="native">Native (/api/chat)</option>
          <option value="openai">OpenAI (/v1/chat/completions)</option>
        </select>
      </div>
      <div>
        <label class="label" for="mtl">Longueur max. texte mobile</label>
        <input id="mtl" type="number" class="input" value={c.remote_mobile.max_text_length}
          onchange={(e) => store.patch({ remote_mobile: { max_text_length: num(e) } })} />
      </div>
    </div>
  </div>
</details>
