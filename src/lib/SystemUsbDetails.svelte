<script lang="ts">
  import Icon from './Icon.svelte';
  import { onMount } from 'svelte';
  import Notification from './Notification.svelte';
  import RuntimeEditor from './RuntimeEditor.svelte';
  import ActionControl from './ActionControl.svelte';
  import { gatewayRequest, type GatewayCommand } from './gateway';
  import { ModuleUsbClient, usbRoutesEqual, type UsbModule, type UsbActions, type UsbMetadataConfig } from './module-usb';
  import { observeUsbInputs, type UsbInputObservationState } from './usb-input-observations';
  import { observeUsbMetadata, type UsbMetadataObservationState, type UsbMetadataPayload } from './usb-metadata-observations';
  import { cloneConfig, type RuntimeConfig, type RuntimeSnapshot } from './runtime-config';
  import type { DeepReadonly } from './profiles';
  import type { DeviceAction, ActionValue } from './device-actions';
  let { endpointKey }: { endpointKey: string } = $props();
  const client = new ModuleUsbClient(async request => { const result = (await gatewayRequest(request)).moduleUsb; if (!result) throw Error('USB service unavailable.'); return result; });
  let module = $state.raw<DeepReadonly<UsbModule>>();
  let inputObservation = $state.raw<UsbInputObservationState>({ status: 'connecting' });
  let metadataObservation = $state.raw<UsbMetadataObservationState>({ status: 'connecting' });
  let draft = $state<RuntimeConfig>();
  let busy = $state(false), editing = $state(false), error = $state('');
  let disposed = false;
  let claimSnapshot: DeepReadonly<UsbModule> | undefined;
  let commandModule: DeepReadonly<UsbModule> | undefined;
  let latestMetadata: DeepReadonly<UsbMetadataPayload> | undefined;
  let claimReceiptRevision: number | undefined;
  let lossEpoch = 0;
  const snapshot = $derived(module?.snapshot.config ? editorSnapshot(module.snapshot.config) : undefined);
  const actions = $derived(module?.snapshot.actions?.status === 'ready' ? module.snapshot.actions as UsbActions : undefined);
  const inputs = $derived(inputObservation.status === 'ready' && inputObservation.value?.ready && inputObservation.value.route
    && module?.snapshot.connected && module.snapshot.route && usbRoutesEqual(inputObservation.value.route, module.snapshot.route) ? inputObservation.value.state : null);
  const inputUnavailable = $derived(inputObservation.error || inputObservation.value?.error || (inputObservation.status === 'connecting' ? 'Waiting for a USB input baseline.' : 'USB inputs are unavailable for this route.'));
  $effect(() => observeUsbInputs(endpointKey, state => { inputObservation = state; }));
  $effect(() => observeUsbMetadata(endpointKey, state => {
    metadataObservation = state;
    if (state.status === 'ready' && state.value) {
      latestMetadata = state.value;
      if (commandModule) commandModule = applyMetadata(commandModule, state.value);
      module = commandModule;
      if (!busy) reconcileEditing();
    } else if (state.status === 'lost' || state.status === 'error') {
      lossEpoch++;
      latestMetadata = undefined;
      editing = false; claimSnapshot = undefined; claimReceiptRevision = undefined;
      module = undefined;
    }
  }));
  function revision(value: DeepReadonly<UsbModule> | undefined): number | undefined {
    const result = (value?.snapshot.config as { revision?: number } | null)?.revision;
    return typeof result === 'number' && Number.isSafeInteger(result) && result > 0 ? result : undefined;
  }
  function editorSnapshot(value: DeepReadonly<UsbMetadataConfig>): RuntimeSnapshot {
    return { ...value, config: cloneConfig(JSON.parse(JSON.stringify(value.config))), layout: value.layout ? { ...value.layout } : null };
  }
  function sameSource(left: NonNullable<DeepReadonly<UsbModule>['snapshot']['route']>, right: NonNullable<DeepReadonly<UsbMetadataPayload>['route']>): boolean {
    return left.moduleId === right.moduleId && left.serial === right.serial && left.connectionGeneration === right.connectionGeneration && left.boot === right.boot;
  }
  function sameScope(left: DeepReadonly<UsbModule>['snapshot']['route'], right: DeepReadonly<UsbMetadataPayload>['route']): boolean {
    return left === null && right === null || !!left && !!right && usbRoutesEqual(left, right);
  }
  function receiptCanLead(receipt: DeepReadonly<UsbModule>, publicValue: DeepReadonly<UsbMetadataPayload> | undefined): boolean {
    if (!publicValue || !sameScope(receipt.snapshot.route, publicValue.route)) return true;
    const received = revision(receipt), observed = publicValue.config?.revision;
    return received !== undefined && (observed === undefined || received >= observed);
  }
  function applyMetadata(base: DeepReadonly<UsbModule>, value: DeepReadonly<UsbMetadataPayload>, preferReceipt = false): DeepReadonly<UsbModule> {
    const currentRoute = base.snapshot.route, publicRoute = value.route;
    const exactScope = sameScope(currentRoute, publicRoute);
    const sourceChanged = !!currentRoute && !!publicRoute && !sameSource(currentRoute, publicRoute);
    if (preferReceipt) return base;
    if (value.connected && exactScope && currentRoute && publicRoute) {
      const currentRevision = revision(base), publicRevision = value.config?.revision;
      if (currentRevision !== undefined && publicRevision !== undefined && publicRevision < currentRevision) return base;
    }
    const retiredRoute = !value.connected || sourceChanged || !exactScope;
    const snapshot = { ...base.snapshot, connected: value.connected, port: value.port, identity: value.identity,
      route: value.route, config: (value.config ?? (!value.connected ? base.snapshot.config : null)) as typeof base.snapshot.config,
      actions: retiredRoute ? null : base.snapshot.actions, events: retiredRoute ? null : base.snapshot.events,
      observedAtMs: value.observedAtMs, error: value.error };
    return { ...base, port: value.port === null ? base.port : { ...base.port, path: value.port }, snapshot } as DeepReadonly<UsbModule>;
  }
  function reconcileEditing(): void {
    if (claimSnapshot && claimReceiptRevision && commandModule?.snapshot.claimedBy && !latestMetadata && metadataObservation.status === 'connecting') {
      editing = false;
      return;
    }
    if (!claimSnapshot || !claimReceiptRevision || !commandModule?.snapshot.claimedBy) {
      editing = false;
      if (commandModule?.snapshot.claimedBy) {
        commandModule = { ...commandModule, snapshot: { ...commandModule.snapshot, claimedBy: null } } as DeepReadonly<UsbModule>;
        module = metadataObservation.status === 'lost' || metadataObservation.status === 'error' ? undefined : commandModule;
      }
      return;
    }
    const current = commandModule.snapshot, expected = claimSnapshot.snapshot;
    const publicValue = latestMetadata;
    const publicConfig = publicValue?.config;
    if (publicConfig && publicConfig.revision < claimReceiptRevision) { editing = false; return; }
    const sameRoute = !!publicValue && !!publicValue.connected && !!publicValue.route && !!expected.route && usbRoutesEqual(publicValue.route, expected.route);
    const sameOwner = !!publicConfig && publicConfig.owner === expected.config?.owner && current.claimedBy === expected.claimedBy;
    if (!sameRoute || !sameOwner || !publicConfig || publicConfig.revision > claimReceiptRevision) {
      editing = false; claimSnapshot = undefined; claimReceiptRevision = undefined;
      commandModule = { ...commandModule, snapshot: { ...commandModule.snapshot, claimedBy: null } } as DeepReadonly<UsbModule>;
      module = metadataObservation.status === 'lost' || metadataObservation.status === 'error' ? undefined : commandModule;
      return;
    }
    editing = true;
  }
  async function run(operation: () => Promise<DeepReadonly<UsbModule>>, replaceDraft = true, claiming = false, configurationChange = false, receiptPreference = false): Promise<boolean> {
    if (busy) return false;
    const startedLossEpoch = lossEpoch;
    const publicAtStart = latestMetadata;
    busy = true;
    try {
      const next = await operation();
      if (disposed) { if (claiming || next.snapshot.claimedBy && next.snapshot.claimedBy === claimSnapshot?.snapshot.claimedBy) await client.release(next).catch(() => {}); return false; }
      if (claiming && startedLossEpoch !== lossEpoch) {
        if (next.snapshot.claimedBy) await client.release(next).catch(() => {});
        error = 'USB metadata continuity changed during setup; the claim was not accepted.';
        return false;
      }
      const nextRoute = next.snapshot.route, claimedRoute = claimSnapshot?.snapshot.route;
      const expectedConfiguration = configurationChange && nextRoute && claimedRoute && nextRoute.moduleId === claimedRoute.moduleId && nextRoute.serial === claimedRoute.serial && nextRoute.connectionGeneration === claimedRoute.connectionGeneration && nextRoute.boot === claimedRoute.boot;
      if (claiming) { claimSnapshot = next; claimReceiptRevision = revision(next); }
      else if (claimSnapshot && next.snapshot.connected && next.snapshot.claimedBy === claimSnapshot.snapshot.claimedBy
        && (!nextRoute || !claimedRoute || usbRoutesEqual(nextRoute, claimedRoute) || expectedConfiguration)) {
        const nextRevision = revision(next);
        if (nextRevision !== undefined && (claimReceiptRevision === undefined || nextRevision >= claimReceiptRevision)) {
          claimSnapshot = next; claimReceiptRevision = nextRevision;
        }
      }
      const mayPreferReceipt = receiptPreference && startedLossEpoch === lossEpoch && latestMetadata === publicAtStart && receiptCanLead(next, publicAtStart);
      commandModule = latestMetadata ? applyMetadata(next, latestMetadata, mayPreferReceipt) : next;
      module = metadataObservation.status === 'lost' || metadataObservation.status === 'error' ? undefined : commandModule;
      reconcileEditing();
      const receiptRevision = revision(next), newerPublic = !!latestMetadata?.config && receiptRevision !== undefined && latestMetadata.config.revision > receiptRevision;
      if (replaceDraft && next.snapshot.config && !newerPublic && metadataObservation.status !== 'lost' && metadataObservation.status !== 'error') {
        draft = cloneConfig(JSON.parse(JSON.stringify(next.snapshot.config.config)));
      }
      error = ''; return true;
    }
    catch (e) { if (!disposed) error = String(e); return false; }
    finally { if (!disposed) { busy = false; reconcileEditing(); } }
  }
  async function connect() {
    await run(async () => {
      const ports = await client.ports(), port = ports.ports.find(p => p.key === endpointKey);
      if (!port) throw Error('This exact USB endpoint is no longer present. Refresh inventory.');
      return client.connect(port);
    }, true, false, false, true);
  }
  async function claim() { if (module) await run(() => client.claim(module!), true, true); }
  async function release() { if (module && await run(() => client.release(module!))) { editing = false; claimSnapshot = undefined; claimReceiptRevision = undefined; } }
  async function configure(command: GatewayCommand, config?: RuntimeConfig): Promise<boolean> {
    if (!module || !editing) return false;
    if (command === 'config_apply' || command === 'config_apply_save') return run(() => client.apply(module!, config!, command === 'config_apply_save'), true, false, true, true);
    if (command === 'config_save') return run(() => client.save(module!));
    if (command === 'config_discard') return run(() => client.discard(module!));
    if (command === 'config_reset') return run(() => client.reset(module!));
    return false;
  }
  async function invoke(action: DeviceAction, values: ActionValue[]) { return module ? run(() => client.invoke(module!, action.id, values), false) : false; }
  onMount(() => {
    void run(() => client.status(endpointKey));
    return () => { disposed = true; if (claimSnapshot) void client.release(claimSnapshot).catch(() => {}); };
  });
</script>

<section class="panel usb-details" aria-label="Direct USB device details">
  <div class="panel-heading"><div><h2>Direct USB</h2><p>Connection, module configuration and device actions.</p></div><span class="quiet-tag">{!module ? 'Checking…' : editing ? 'Setup active' : module.snapshot.connected ? 'Connected' : 'Disconnected'}</span></div>
  <div class="usb-body">
  {#if error}<Notification message={error} kind="error" />{/if}
  <div class="usb-actions">
    <button class="secondary-button" disabled={busy} onclick={() => run(() => client.status(endpointKey), !editing)}><Icon name="refresh" size={14} />Refresh</button>
    {#if !module?.snapshot.connected}<button class="secondary-button" disabled={busy} onclick={connect}><Icon name="usb" size={14} />Connect</button>
    {:else}<button class="secondary-button" disabled={busy || editing} onclick={() => run(() => client.disconnect(endpointKey))}><Icon name="close" size={14} />Disconnect</button>{/if}
    {#if module?.snapshot.connected}
      {#if editing}<button class="secondary-button" disabled={busy} onclick={release}><Icon name="close" size={14} />End setup</button>
      {:else}<button class="secondary-button" disabled={busy} onclick={claim}><Icon name="settings" size={14} />Begin setup</button>{/if}
    {/if}
  </div>
  {#if snapshot && draft}<div class="usb-configuration"><RuntimeEditor bind:config={draft} {snapshot} {editing} {busy} request={configure} automatic controllerEnabled={false} /></div>{/if}
  {#if module?.snapshot.connected}
    <section class="usb-device-actions" aria-label="USB device actions">
    <div class="actions-heading"><h3>Device actions</h3><button class="secondary-button" disabled={busy} onclick={() => run(() => client.describeActions(module!), false)}><Icon name="search" size={14} />Discover</button></div>
    {#each actions?.actions ?? [] as action (action.id)}<ActionControl action={action as DeviceAction} snapshot={actions!} ready={!busy} {busy} {invoke} />{/each}
    </section>
  {/if}
  {#if inputs}<p class="usb-input-status">Live inputs: {inputs.held.filter(Boolean).length} pressed · {inputs.axes.length ? inputs.axes.map((value, index) => inputs?.axisAvailable[index] ? value : 'Unavailable').join(', ') : 'No axes'}</p>
  {:else}<p class="usb-input-status" role="status">{inputUnavailable}</p>{/if}
  <p class="field-help">Configuration changes require an explicit setup claim and Apply or Save. Connection inspection does not claim the writer.</p>
  </div>
</section>
<style>
  .usb-details { min-width: 0; }
  .usb-body { padding: var(--ui-panel-padding); }
  .usb-actions { display: flex; flex-wrap: wrap; gap: 10px; margin-bottom: 18px; }
  .usb-configuration, .usb-device-actions { margin-top: var(--ui-section-gap); padding-top: var(--ui-section-gap); border-top: 1px solid var(--line); }
  .actions-heading { display: flex; justify-content: space-between; align-items: center; gap: 12px; flex-wrap: wrap; margin-bottom: 14px; }
  h3 { font-size: var(--ui-heading-size); font-weight: 550; }
  .usb-input-status { color: var(--soft); font-size: var(--ui-text-size); line-height: var(--ui-line-height); margin-top: 14px; overflow-wrap: anywhere; }
  .field-help { font-size: var(--ui-text-size); line-height: var(--ui-line-height); }
  @media(max-width:520px) { .usb-body { padding: 17px 15px; } }
</style>
