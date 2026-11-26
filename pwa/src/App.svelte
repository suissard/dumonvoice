<script lang="ts">
  import { onMount } from 'svelte';
  import { isLan, store } from './lib/store.svelte';
  import Toasts from './components/Toasts.svelte';
  import DesktopSettings from './views/DesktopSettings.svelte';
  import MobileRemote from './views/MobileRemote.svelte';
  import OnboardingWizard from './views/OnboardingWizard.svelte';
  import SetupCa from './views/SetupCa.svelte';

  // Routeur léger : `/` (réglages PC) · `/remote` (compagnon) · `/setup-ca` (certificat).
  const path = location.pathname.replace(/\/+$/, '') || '/';
  const route: 'desktop' | 'remote' | 'setup-ca' =
    path === '/setup-ca' ? 'setup-ca' : path === '/remote' || isLan ? 'remote' : 'desktop';

  let skipWizard = $state(false);

  onMount(() => {
    if (route === 'desktop') {
      store.init('desktop');
      store.refresh();
    }
  });
</script>

<Toasts />
{#if route === 'setup-ca'}
  <SetupCa />
{:else if route === 'remote'}
  <MobileRemote />
{:else if store.config && !store.config.onboarding_done && !skipWizard}
  <OnboardingWizard ondone={() => (skipWizard = true)} />
{:else}
  <DesktopSettings />
{/if}
