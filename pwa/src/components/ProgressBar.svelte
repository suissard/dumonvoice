<script lang="ts">
  import type { DownloadState } from '../lib/store.svelte';
  let { state }: { state: DownloadState | undefined } = $props();
  let pct = $derived(state?.total ? Math.round((state.downloaded / state.total) * 100) : null);
  const mb = (b: number) => (b / 1024 / 1024).toFixed(0);
</script>

{#if state && !state.done}
  <div class="mt-2">
    <div class="h-2 w-full overflow-hidden rounded-full bg-slate-200 dark:bg-slate-800">
      <div class="h-full bg-brand-600 transition-[width]" style="width: {pct ?? 5}%"></div>
    </div>
    <p class="muted mt-1 text-xs">
      {pct !== null ? `${pct} % — ${mb(state.downloaded)} / ${mb(state.total ?? 0)} Mo` : 'Téléchargement…'}
    </p>
  </div>
{:else if state?.error}
  <p class="mt-1 text-xs text-red-600">{state.error}</p>
{/if}
