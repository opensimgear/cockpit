<script lang="ts">
  import { onMount } from 'svelte';
  import Notification from './Notification.svelte';
  import RuntimeEditor from './RuntimeEditor.svelte';
  import ActionControl from './ActionControl.svelte';
  import { gatewayRequest, type GatewayCommand } from './gateway';
  import { ModuleUsbClient, usbRoutesEqual, type UsbModule, type UsbActions } from './module-usb';
  import { cloneConfig, type RuntimeConfig } from './runtime-config';
  import type { DeepReadonly } from './profiles';
  import type { DeviceAction, ActionValue } from './device-actions';
  let { endpointKey }: { endpointKey: string } = $props();
  const client = new ModuleUsbClient(async request => { const result = (await gatewayRequest(request)).moduleUsb; if (!result) throw Error('USB service unavailable.'); return result; });
  let module = $state.raw<DeepReadonly<UsbModule>>();
  let draft = $state<RuntimeConfig>();
  let busy = $state(false), editing = $state(false), error = $state('');
  let disposed = false;
  let claimSnapshot: DeepReadonly<UsbModule> | undefined;
  const snapshot = $derived(module?.snapshot.config);
  const actions = $derived(module?.snapshot.actions?.status === 'ready' ? module.snapshot.actions as UsbActions : undefined);
  async function run(operation: () => Promise<DeepReadonly<UsbModule>>, replaceDraft = true, claiming = false, configurationChange = false): Promise<boolean> {
    if (busy) return false;
    busy = true;
    try {
      const next = await operation();
      if (disposed) { if (claiming || next.snapshot.claimedBy && next.snapshot.claimedBy === claimSnapshot?.snapshot.claimedBy) await client.release(next).catch(() => {}); return false; }
      const nextRoute = next.snapshot.route, claimedRoute = claimSnapshot?.snapshot.route;
      const expectedConfiguration = configurationChange && nextRoute && claimedRoute && nextRoute.moduleId === claimedRoute.moduleId && nextRoute.serial === claimedRoute.serial && nextRoute.connectionGeneration === claimedRoute.connectionGeneration && nextRoute.boot === claimedRoute.boot;
      if (editing && (!next.snapshot.connected || !nextRoute || !claimedRoute || !usbRoutesEqual(nextRoute, claimedRoute) && !expectedConfiguration || !next.snapshot.claimedBy || next.snapshot.claimedBy !== claimSnapshot?.snapshot.claimedBy)) editing = false;
      if (claiming) claimSnapshot = next;
      else if (claimSnapshot && next.snapshot.connected && next.snapshot.claimedBy === claimSnapshot.snapshot.claimedBy) claimSnapshot = next;
      module = next;
      if (replaceDraft && next.snapshot.config) draft = cloneConfig(JSON.parse(JSON.stringify(next.snapshot.config.config)));
      error = ''; return true;
    }
    catch (e) { if (!disposed) error = String(e); return false; }
    finally { if (!disposed) busy = false; }
  }
  async function connect() {
    await run(async () => {
      const ports = await client.ports(), port = ports.ports.find(p => p.key === endpointKey);
      if (!port) throw Error('This exact USB endpoint is no longer present. Refresh inventory.');
      return client.connect(port);
    });
  }
  async function claim() { if (module && await run(() => client.claim(module!), true, true) && !disposed) editing = true; }
  async function release() { if (module && await run(() => client.release(module!))) { editing = false; claimSnapshot = undefined; } }
  async function configure(command: GatewayCommand, config?: RuntimeConfig): Promise<boolean> {
    if (!module || !editing) return false;
    if (command === 'config_apply' || command === 'config_apply_save') return run(() => client.apply(module!, config!, command === 'config_apply_save'), true, false, true);
    if (command === 'config_save') return run(() => client.save(module!));
    if (command === 'config_discard') return run(() => client.discard(module!));
    if (command === 'config_reset') return run(() => client.reset(module!));
    return false;
  }
  async function invoke(action: DeviceAction, values: ActionValue[]) { return module ? run(() => client.invoke(module!, action.id, values), false) : false; }
  onMount(() => {
    void run(() => client.status(endpointKey));
    const timer = setInterval(() => { if (module?.snapshot.connected && !busy && !document.hidden) void run(() => client.eventsState(module!), false); }, 750);
    return () => { disposed = true; clearInterval(timer); if (claimSnapshot) void client.release(claimSnapshot).catch(() => {}); };
  });
</script>

<section class="panel usb-details" aria-label="Direct USB device details">
  <div class="panel-heading"><div><h2>Direct USB</h2><p>Connection, module configuration and device actions.</p></div><span class="quiet-tag">{!module ? 'Checking…' : editing ? 'Setup active' : module.snapshot.connected ? 'Connected' : 'Disconnected'}</span></div>
  <div class="usb-body">
  {#if error}<Notification message={error} kind="error" />{/if}
  <div class="usb-actions">
    <button class="secondary-button" disabled={busy} onclick={() => run(() => client.status(endpointKey), !editing)}>Refresh USB state</button>
    {#if !module?.snapshot.connected}<button class="secondary-button" disabled={busy} onclick={connect}>Connect USB endpoint</button>
    {:else}<button class="secondary-button" disabled={busy || editing} onclick={() => run(() => client.disconnect(endpointKey))}>Disconnect USB endpoint</button>{/if}
    {#if module?.snapshot.connected}
      {#if editing}<button class="secondary-button" disabled={busy} onclick={release}>End USB setup</button>
      {:else}<button class="secondary-button" disabled={busy} onclick={claim}>Begin USB setup</button>{/if}
    {/if}
  </div>
  {#if snapshot && draft}<div class="usb-configuration"><RuntimeEditor bind:config={draft} snapshot={snapshot as import('./runtime-config').RuntimeSnapshot} {editing} {busy} request={configure} automatic controllerEnabled={false} /></div>{/if}
  {#if module?.snapshot.connected}
    <section class="usb-device-actions" aria-label="USB device actions">
    <div class="actions-heading"><h3>Device actions</h3><button class="secondary-button" disabled={busy} onclick={() => run(() => client.describeActions(module!), false)}>Discover USB actions</button></div>
    {#each actions?.actions ?? [] as action (action.id)}<ActionControl action={action as DeviceAction} snapshot={actions!} ready={!busy} {busy} {invoke} />{/each}
    {#if module.snapshot.events && !('status' in module.snapshot.events)}<p class="usb-input-status">Live inputs: {module.snapshot.events.held.filter(Boolean).length} pressed · {module.snapshot.events.axes?.join(', ') ?? 'No axes'}</p>{/if}
    </section>
  {/if}
  <p class="field-help">Configuration changes require an explicit setup claim and Apply or Save. Connection inspection does not claim the writer.</p>
  </div>
</section>
<style>
  .usb-details { min-width: 0; }
  .usb-body { padding: 20px 21px; }
  .usb-actions { display: flex; flex-wrap: wrap; gap: 10px; margin-bottom: 18px; }
  .usb-configuration, .usb-device-actions { margin-top: 20px; padding-top: 20px; border-top: 1px solid var(--line); }
  .actions-heading { display: flex; justify-content: space-between; align-items: center; gap: 12px; flex-wrap: wrap; margin-bottom: 14px; }
  h3 { font-size: 12px; font-weight: 550; }
  .usb-input-status { color: var(--soft); font-size: 10px; line-height: 1.7; margin-top: 14px; overflow-wrap: anywhere; }
  .field-help { font-size: 9px; line-height: 1.7; }
  @media(max-width:520px) { .usb-body { padding: 17px 15px; } }
</style>
