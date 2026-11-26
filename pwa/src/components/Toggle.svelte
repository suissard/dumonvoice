<script lang="ts">
  let {
    checked = false,
    label = '',
    hint = '',
    disabled = false,
    big = false,
    onchange,
  }: {
    checked?: boolean;
    label?: string;
    hint?: string;
    disabled?: boolean;
    big?: boolean;
    onchange?: (v: boolean) => void;
  } = $props();
</script>

<label class="flex cursor-pointer items-center justify-between gap-4 py-2 {disabled ? 'opacity-50' : ''}">
  <span class="min-w-0">
    {#if label}<span class="block text-sm font-medium">{label}</span>{/if}
    {#if hint}<span class="muted block text-xs">{hint}</span>{/if}
  </span>
  <button
    type="button"
    role="switch"
    aria-checked={checked}
    aria-label={label}
    {disabled}
    onclick={() => onchange?.(!checked)}
    class="relative shrink-0 rounded-full transition-colors {big ? 'h-9 w-16' : 'h-6 w-11'}
      {checked ? 'bg-brand-600' : 'bg-slate-300 dark:bg-slate-700'}"
  >
    <span
      class="absolute top-1 left-1 rounded-full bg-white shadow transition-transform {big ? 'h-7 w-7' : 'h-4 w-4'}"
      style="transform: translateX({checked ? (big ? '1.75rem' : '1.25rem') : '0'})"
    ></span>
  </button>
</label>
