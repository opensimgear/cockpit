<script lang="ts">
  import Notification from './Notification.svelte';
  import { supportsDeviceContracts } from './gateway-capabilities';
  import ActionControl from './ActionControl.svelte';
  import { actionReady, type ActionValue, type DeviceAction } from './device-actions';
  import type { GatewayModule, GatewayRequest, GatewayState } from './gateway';
  let { state, module, busy, request, profileBlockedReason = '' }: {
    state: GatewayState | null; module: GatewayModule | undefined; busy: boolean;
    request: (request: GatewayRequest) => Promise<boolean>;
    profileBlockedReason?: string;
  } = $props();
  const ready = $derived(actionReady(state, module));
  const snapshot = $derived(module ? state?.moduleActions?.[module.id] : undefined);
  const supported = $derived(supportsDeviceContracts(state) && !!module?.boot);
  async function invoke(action: DeviceAction, values: ActionValue[]) {
    return request({ command: 'action_invoke', moduleId: module?.id, boot: module?.boot,
      catalogueRevision: snapshot?.catalogueRevision, actionId: action.id, values });
  }
</script>

<section class="panel" aria-label="Device actions">
  <div class="heading"><div><h2>Device actions</h2><p>Control the outputs advertised by this device.</p></div><span class="status">{busy ? 'Working…' : !ready ? 'Unavailable' : snapshot?.status === 'ready' ? 'Ready' : snapshot?.status === 'unsupported' ? 'Unsupported' : snapshot?.status === 'uncertain' ? 'Uncertain' : snapshot?.status === 'error' ? 'Read required' : 'Discovering…'}</span></div>
  {#if !supported}<p class="help">This firmware does not advertise device actions. Its controller inputs remain available.</p>
  {:else}
    {#if !ready}<p class="help">Reconnect this device to control its outputs.</p>
    {:else if snapshot?.status === 'unsupported'}<p class="help">This firmware does not advertise device actions. Its controller inputs remain available.</p>
    {:else if snapshot?.error}<Notification message={String(snapshot.error)} kind="error" />
    {:else if snapshot?.status === 'ready' && !snapshot.actions.length}<p class="help">The device advertises no manual actions.</p>{/if}
    {#if snapshot?.status === 'uncertain'}<p class="help">The last command may have applied. Refresh state before another action.</p>{/if}
    {#if profileBlockedReason}<p class="help" role="status">{profileBlockedReason}</p>{/if}
    {#each snapshot?.actions ?? [] as action (`${module?.boot}/${snapshot?.catalogueRevision}/${action.id}`)}

      <ActionControl {action} snapshot={snapshot!} ready={ready && !profileBlockedReason} {busy} {invoke} />
    {/each}
    <div class="toolbar"><button disabled={!ready || busy} onclick={() => request({ command: 'action_describe', moduleId: module?.id })}>{snapshot?.actions.length ? 'Discover again' : 'Discover actions'}</button><button disabled={!ready || busy || !snapshot?.catalogueRevision} onclick={() => request({ command: 'action_state', moduleId: module?.id })}>Refresh state</button></div>
    <p class="help">Actions work while this device’s USB controller is disabled or its direct USB owns the inputs. Applied state is device readback; physical output verification is separate.</p>
  {/if}
</section>

<style>
  .panel{ margin-top: 20px; padding: 22px; border: 1px solid var(--line); border-radius: 8px; background: var(--panel); }.heading{ display: flex; justify-content: space-between; align-items: start; gap: 14px; }h2{ margin: 0; font-size: 15px; }.heading p,.help{ color: var(--soft); font-size: 11px; line-height: 1.6; }.status{ padding: 4px 8px; border: 1px solid var(--line); border-radius: 4px; font-size: 10px; white-space: nowrap; }.toolbar{ display: flex; flex-wrap: wrap; gap: 8px; margin-top: 12px; }button{ padding: 7px 12px; border: 1px solid var(--line); border-radius: 4px; background: var(--input-bg); color: var(--text); font-size: 11px; cursor: pointer; }button:focus-visible{ outline: 2px solid var(--accent); outline-offset: 2px; }button:disabled{ opacity: .5; cursor: default; }
</style>
