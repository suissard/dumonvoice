<script lang="ts">
  import { onMount } from 'svelte';
  import { store } from '../lib/store.svelte';
  import TabHome from './settings/TabHome.svelte';
  import TabTriggers from './settings/TabTriggers.svelte';
  import TabEngine from './settings/TabEngine.svelte';
  import TabAudio from './settings/TabAudio.svelte';
  import Advanced from './settings/Advanced.svelte';

  let { embedded = false }: { embedded?: boolean } = $props();

  type Tab = 'home' | 'mobile' | 'engine' | 'audio';
  const TABS: { id: Tab; label: string }[] = [
    { id: 'home', label: 'Accueil & Test' },
    { id: 'mobile', label: 'Déclenchement & Mobile' },
    { id: 'engine', label: 'Moteur & Langue' },
    { id: 'audio', label: 'Audio & Confort' },
  ];

  function initialTab(): Tab {
    const h = typeof location !== 'undefined' ? location.hash.replace('#', '') : '';
    return (TABS.find((t) => t.id === h)?.id ?? 'home') as Tab;
  }
  let tab = $state<Tab>(initialTab());

  onMount(() => {
    if (!embedded) {
      store.init('desktop');
      store.refresh();
    }
    const onHash = () => (tab = initialTab());
    window.addEventListener('hashchange', onHash);
    return () => window.removeEventListener('hashchange', onHash);
  });

  function select(t: Tab) {
    tab = t;
    if (!embedded) history.replaceState(null, '', `#${t}`);
  }
</script>

<div class={embedded ? '' : 'mx-auto max-w-4xl px-4 py-6'}>
  {#if !embedded}
    <header class="mb-6 flex items-center gap-3">
      <div class="flex h-10 w-10 items-center justify-center rounded-xl bg-brand-600 text-xl text-white">🎙</div>
      <div class="flex-1">
        <h1 class="text-xl font-bold">DumonVoice</h1>
        <p class="muted text-xs">
          Dictée universelle 100 % locale{store.status ? ` · ${store.status.pc_name} · v${store.status.version}` : ''}
        </p>
      </div>
      <span
        class="rounded-full px-3 py-1 text-xs font-semibold {store.conn === 'open'
          ? 'bg-emerald-100 text-emerald-800 dark:bg-emerald-900/40 dark:text-emerald-300'
          : 'bg-red-100 text-red-800 dark:bg-red-900/40 dark:text-red-300'}"
      >
        {store.conn === 'open' ? 'Daemon connecté' : 'Daemon injoignable'}
      </span>
    </header>
  {/if}

  <nav class="mb-4 flex gap-1 overflow-x-auto rounded-2xl bg-slate-200/60 p-1 dark:bg-slate-900" aria-label="Onglets">
    {#each TABS as t (t.id)}
      <button
        class="shrink-0 rounded-xl px-4 py-2 text-sm font-semibold transition {tab === t.id
          ? 'bg-white shadow dark:bg-slate-700'
          : 'text-slate-600 hover:text-slate-900 dark:text-slate-400'}"
        aria-current={tab === t.id}
        onclick={() => select(t.id)}>{t.label}</button
      >
    {/each}
  </nav>

  {#if !store.config || !store.status}
    <div class="card muted">Chargement…</div>
  {:else}
    {#if tab === 'home'}
      <TabHome />
    {:else if tab === 'mobile'}
      <TabTriggers />
    {:else if tab === 'engine'}
      <TabEngine />
    {:else}
      <TabAudio />
    {/if}
    <Advanced />
  {/if}
</div>
