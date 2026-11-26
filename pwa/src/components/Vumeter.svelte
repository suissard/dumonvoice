<script lang="ts">
  let { level = 0 }: { level?: number } = $props();
  // Échelle perceptive (RMS → dB) pour un vumètre lisible.
  let pct = $derived(Math.max(0, Math.min(100, ((20 * Math.log10(Math.max(level, 1e-4)) + 60) / 60) * 100)));
</script>

<div class="h-3 w-full overflow-hidden rounded-full bg-slate-200 dark:bg-slate-800" aria-label="Niveau micro">
  <div
    class="h-full rounded-full transition-[width] duration-75 {pct > 85 ? 'bg-red-500' : pct > 60 ? 'bg-amber-500' : 'bg-emerald-500'}"
    style="width: {pct}%"
  ></div>
</div>
