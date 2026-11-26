<script lang="ts">
  import { store } from '../lib/store.svelte';
</script>

<div class="pointer-events-none fixed inset-x-0 top-3 z-50 flex flex-col items-center gap-2 px-4">
  {#each store.toasts as t (t.id)}
    <div
      class="pointer-events-auto max-w-md rounded-xl px-4 py-2 text-sm font-medium shadow-lg
        {t.level === 'error'
        ? 'bg-red-600 text-white'
        : t.level === 'warn'
          ? 'bg-amber-500 text-white'
          : t.level === 'success'
            ? 'bg-emerald-600 text-white'
            : 'bg-slate-800 text-white'}"
      role="status"
    >
      {t.text}
    </div>
  {/each}
</div>
