<script lang="ts">
  import { onMount, untrack } from 'svelte';
  import SystemControllerInputs from './SystemControllerInputs.svelte';
  import { controllerSourceId } from './controller-inputs';
  import { observeControllerInputs } from './controller-observations';
  import type { ControllerControl } from './controllers';
  import type { DeepReadonly } from './profiles';
  import type { ResourceSummary } from './profile-inventory';

  let { resource, active = true }: { resource?: DeepReadonly<ResourceSummary>; active?: boolean } = $props();
  let mounted = $state(false), controls = $state.raw<readonly ControllerControl[]>([]), error = $state(''), ready = $state(false);
  const sourceId = $derived(controllerSourceId(resource));
  let observedOnline = $state<boolean | undefined>();
  const online = $derived(observedOnline ?? resource?.availability === 'online');
  const generation = $derived(resource?.generation);
  onMount(() => { mounted = true; });
  $effect(() => {
    const id = sourceId;
    void generation;
    controls = []; error = ''; ready = false; observedOnline = undefined;
    if (!mounted || !active || !id) return;
    return untrack(() => observeControllerInputs(id, state => {
      if (state.detail?.source) {
        observedOnline = state.detail.source.online;
        controls = state.detail.controls ?? [];
        ready = state.detail.source.baselineReady;
      } else {
        controls = controls.map(control => ({ ...control, value: null }));
        ready = false;
      }
      error = state.status === 'error' || state.status === 'lost'
        ? state.error ?? 'Input values are unavailable.' : '';
    }));
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
