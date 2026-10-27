<script lang="ts">
  import { onMount } from 'svelte';
  import SystemControllerInputs from './SystemControllerInputs.svelte';
  import { controllerSourceId, loadControllerInputs } from './controller-inputs';
  import { gatewayRequest } from './gateway';
  import type { ControllerControl } from './controllers';
  import type { DeepReadonly } from './profiles';
  import type { ResourceSummary } from './profile-inventory';

  let { resource, active = true }: { resource?: DeepReadonly<ResourceSummary>; active?: boolean } = $props();
  let mounted = $state(false), controls = $state.raw<ControllerControl[]>([]), error = $state(''), ready = $state(false);
  const sourceId = $derived(controllerSourceId(resource));
  const online = $derived(resource?.availability === 'online');
  const generation = $derived(resource?.generation);
  onMount(() => { mounted = true; });
  $effect(() => {
    const id = sourceId;
    void generation;
    controls = []; error = ''; ready = false;
    if (!mounted || !active || !online || !id) return;
    let disposed = false, busy = false;
    async function refresh() {
      if (disposed || busy || document.hidden) return;
      busy = true;
      try {
        const result = await loadControllerInputs(id, async request => {
          const state = await gatewayRequest(request);
          if (!state.controllers) throw new Error('Input values are unavailable.');
          return state.controllers;
        }, () => !disposed && !document.hidden);
        if (!disposed && result) {
          controls = result.source?.online ? result.controls ?? [] : [];
          error = result.source?.online ? '' : 'Controller is offline.';
          ready = true;
        }
      } catch { if (!disposed) { controls = []; error = 'Input values are unavailable.'; } }
      finally { busy = false; }
    }
    void refresh();
    const timer = window.setInterval(refresh, 250);
    function visibility() { if (document.hidden) { controls = []; ready = false; } else void refresh(); }
    document.addEventListener('visibilitychange', visibility);
    return () => { disposed = true; window.clearInterval(timer); document.removeEventListener('visibilitychange', visibility); };
  });
</script>

<section class="controller-inputs" aria-label="Live inputs">
  <h4>Inputs</h4>
  {#if error}<p role="status">{error}</p>
  {:else if !online}<p>Controller is offline.</p>
  {:else if !sourceId}<p>No input controls available.</p>
  {:else if !ready}<p>Waiting for input values…</p>
  {:else}<SystemControllerInputs {controls} {online} simple/>{/if}
</section>

<style>
  .controller-inputs { flex: 1; min-height: 0; overflow: auto; padding-top: 16px; border-top: 1px solid var(--line); min-width: 0; }
  h4 { margin: 0 0 12px; font-size: var(--ui-heading-size); font-weight: 570; color: var(--soft); }
  p { margin: 0; color: var(--muted); font-size: var(--ui-text-size); line-height: var(--ui-line-height); }
</style>
