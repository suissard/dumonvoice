<script lang="ts" generics="T extends string">
  let {
    value,
    options,
    onchange,
    small = false,
  }: { value: T; options: { value: T; label: string }[]; onchange: (v: T) => void; small?: boolean } = $props();
</script>

<div class="inline-flex rounded-full bg-slate-200/70 p-1 dark:bg-slate-800" role="radiogroup">
  {#each options as o (o.value)}
    <button
      type="button"
      role="radio"
      aria-checked={value === o.value}
      onclick={() => onchange(o.value)}
      class="rounded-full font-semibold transition {small ? 'px-3 py-1 text-xs' : 'px-4 py-1.5 text-sm'}
        {value === o.value
        ? 'bg-white text-slate-900 shadow dark:bg-slate-600 dark:text-white'
        : 'text-slate-600 dark:text-slate-300'}"
    >
      {o.label}
    </button>
  {/each}
</div>
