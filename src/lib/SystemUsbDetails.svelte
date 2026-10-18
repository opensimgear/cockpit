<script lang="ts">
  import { onMount } from 'svelte';
  import Notification from './Notification.svelte';
  import RuntimeEditor from './RuntimeEditor.svelte';
  import ActionControl from './ActionControl.svelte';
  import { gatewayRequest, type GatewayCommand } from './gateway';
  import { ModuleUsbClient, type UsbModule, type UsbActions } from './module-usb';
  import { cloneConfig, type RuntimeConfig } from './runtime-config';
  import type { DeepReadonly } from './profiles';
  import type { DeviceAction, ActionValue } from './device-actions';
  let { endpointKey }: { endpointKey: string } = $props();
  const client = new ModuleUsbClient(async request => { const result = (await gatewayRequest(request)).moduleUsb; if (!result) throw Error('USB service unavailable.'); return result; });
  let module = $state.raw<DeepReadonly<UsbModule>>();
  let draft = $state<RuntimeConfig>();
  let busy = $state(false), editing = $state(false), error = $state('');
  let disposed = false;
  const snapshot = $derived(module?.snapshot.config);
  const actions = $derived(module?.snapshot.actions?.status === 'ready' ? module.snapshot.actions as UsbActions : undefined);
  async function run(operation: () => Promise<DeepReadonly<UsbModule>>, replaceDraft = true): Promise<boolean> {
    if (busy) return false;
    busy = true;
    try { const next = await operation(); if (disposed) return false; module = next; if (replaceDraft && next.snapshot.config) draft = cloneConfig(JSON.parse(JSON.stringify(next.snapshot.config.config))); error = ''; return true; }
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
  async function claim() { if (module && await run(() => client.claim(module!))) editing = true; }
  async function release() { if (module && await run(() => client.release(module!))) editing = false; }
  async function configure(command: GatewayCommand, config?: RuntimeConfig): Promise<boolean> {
    if (!module || !editing) return false;
    if (command === 'config_apply' || command === 'config_apply_save') return run(() => client.apply(module!, config!, command === 'config_apply_save'));
    if (command === 'config_save') return run(() => client.save(module!));
    if (command === 'config_discard') return run(() => client.discard(module!));
    if (command === 'config_reset') return run(() => client.reset(module!));
    return false;
  }
  async function invoke(action: DeviceAction, values: ActionValue[]) { return module ? run(() => client.invoke(module!, action.id, values), false) : false; }
  onMount(() => {
    void run(() => client.status(endpointKey));
    return () => { disposed = true; if (editing && module) void client.release(module).catch(() => {}); };
  });
</script>

<section aria-label="Direct USB device details">
  <h3>Direct USB</h3>
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
  {#if snapshot && draft}<RuntimeEditor bind:config={draft} snapshot={snapshot as import('./runtime-config').RuntimeSnapshot} {editing} {busy} request={configure} automatic controllerEnabled={false} />{/if}
  {#if module?.snapshot.connected}
    <h3>Device actions</h3>
    <button class="secondary-button" disabled={busy} onclick={() => run(() => client.describeActions(module!), false)}>Discover USB actions</button>
    {#each actions?.actions ?? [] as action (action.id)}<ActionControl action={action as DeviceAction} snapshot={actions!} ready={!busy} {busy} {invoke} />{/each}
    {#if module.snapshot.events && !('status' in module.snapshot.events)}<p>Live inputs: {module.snapshot.events.held.filter(Boolean).length} pressed · {module.snapshot.events.axes?.join(', ') ?? 'No axes'}</p>{/if}
  {/if}
  <p class="field-help">Configuration changes require an explicit setup claim and Apply or Save. Connection inspection does not claim the writer.</p>
</section>
<style>.usb-actions {display:flex;flex-wrap:wrap;gap:12px;margin:16px 0}</style>
