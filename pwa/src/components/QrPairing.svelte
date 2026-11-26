<script lang="ts">
  import { onDestroy } from 'svelte';
  import { api } from '../lib/api';
  import { store } from '../lib/store.svelte';

  let qr = $state<{ svg: string; url: string; alternatives: string[] } | null>(null);
  let remaining = $state(0);
  let error = $state('');
  let loading = $state(false);
  let timer: ReturnType<typeof setInterval> | null = null;

  async function generate() {
    loading = true;
    error = '';
    try {
      const r = await api.pairingCreate();
      qr = { svg: r.qr_svg, url: r.url, alternatives: r.alternatives };
      remaining = r.expires_in;
      if (timer) clearInterval(timer);
      timer = setInterval(() => {
        remaining--;
        if (remaining <= 0) {
          clearInterval(timer!);
          timer = null;
          qr = null;
        }
      }, 1000);
    } catch (e: any) {
      error = e?.message ?? 'Erreur';
    } finally {
      loading = false;
    }
  }

  // Un téléphone vient de s'appairer : le code (usage unique) est consommé.
  let seenVersion = store.devicesVersion;
  $effect(() => {
    if (store.devicesVersion !== seenVersion) {
      seenVersion = store.devicesVersion;
      if (qr && (store.status?.mobile.connected_devices.length ?? 0) > 0) {
        qr = null;
        if (timer) clearInterval(timer);
        store.toast('Téléphone appairé ✔', 'success');
      }
    }
  });

  onDestroy(() => timer && clearInterval(timer));
</script>

<div class="flex flex-col items-center gap-3 text-center">
  {#if qr}
    <div class="rounded-2xl bg-white p-3 shadow-inner [&_svg]:h-56 [&_svg]:w-56">{@html qr.svg}</div>
    <p class="muted text-xs">Scannez avec l'appareil photo du téléphone (même Wi-Fi) — expire dans {remaining} s</p>
    <details class="w-full text-left text-xs">
      <summary class="muted cursor-pointer">Le scan ne fonctionne pas ?</summary>
      <p class="mt-1 break-all">Ouvrez sur le téléphone : <code>{qr.url}</code></p>
      {#each qr.alternatives.slice(1) as alt (alt)}
        <p class="mt-1 break-all"><code>{alt}</code></p>
      {/each}
    </details>
  {:else}
    <button class="btn-primary" onclick={generate} disabled={loading}>
      {loading ? 'Génération…' : 'Afficher le QR code d’appairage'}
    </button>
  {/if}
  {#if error}<p class="text-sm text-red-600">{error}</p>{/if}
</div>
