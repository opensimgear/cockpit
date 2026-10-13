<script lang="ts">
  import { onMount } from 'svelte';
  import Icon from './lib/Icon.svelte';
  import DeviceIllustration from './lib/DeviceIllustration.svelte';
  import RuntimeEditor from './lib/RuntimeEditor.svelte';
  import { cloneConfig, configEqual, type RuntimeConfig } from './lib/runtime-config';
  import { settingsEqual, type Device, type Settings } from './lib/devices';
  import { releaseOnExit, openCockpitLink, type GatewayRequest, type GatewayModule } from './lib/gateway';
  import { GatewayWorkspace, type GatewayEntry } from './lib/gateway-workspace';

  type View = 'gateways' | 'devices' | 'device' | 'topology' | 'activity' | 'preferences';
  type WorkspaceDevice = Device & { key: string; gatewayId: string };
  const navigation: { id: View; label: string; icon: string }[] = [
    { id: 'gateways', label: 'Gateways', icon: 'network' },
    { id: 'devices', label: 'Devices', icon: 'devices' },
    { id: 'topology', label: 'Topology', icon: 'network' },
    { id: 'activity', label: 'Activity', icon: 'activity' },
  ];
  function showDialog(node: HTMLDialogElement) {
    node.showModal();
    return () => node.close();
  }
  function stored<T>(key: string, fallback: T): T {
    try {
      const value: unknown = JSON.parse(localStorage.getItem(key) ?? 'null');
      if (typeof value !== typeof fallback || value === null) return fallback;
      return value as T;
    } catch { return fallback; }
  }
  const deviceKey = (gatewayId: string, moduleId: string) => `${gatewayId}/${moduleId}`;
  const nameForGateway = (backend?: string) => backend === 'stm32' ? 'OpenSimGear CAN gateway' : 'Pico W USB gateway';
  const inputSummary = (module: GatewayModule) => [module.buttonCount ? `${module.buttonCount} buttons` : '', module.hatCount ? `${module.hatCount} hats` : '', module.axisCount ? `${module.axisCount} axes` : ''].filter(Boolean).join(' · ');
  let light = $state(stored<boolean>('osg-light', false));
  let view = $state<View>('gateways');
  let entries = $state<GatewayEntry[]>([]);
  let inventoryError = $state('');
  let drafts = $state<Record<string, Settings>>({});
  let runtimeDrafts = $state<Record<string, RuntimeConfig>>({});
  let selectedId = $state('');
  let search = $state('');
  let filter = $state('all');
  let tab = $state('setup');
  let sessionId = $state<string | null>(null);
  let sessionTransition = $state(false);
  const notificationDuration = 5000;
  let toast = $state('');
  let toastSequence = $state(0);
  let toastTimer: ReturnType<typeof setTimeout> | undefined;
  let modal = $state<'help' | 'remove' | null>(null);
  let removeTarget = $state({ key: '', id: '', label: '', gatewayId: '' });
  let disposed = false;
  let workspace: GatewayWorkspace | undefined;
  let events = $state([{ time: 'Just now', message: 'Workspace opened', detail: 'All compatible gateways connect independently and discover their devices.' }]);
  const devices = $derived(entries.flatMap(entry => (entry.state?.modules ?? []).map((module): WorkspaceDevice => ({
    key: deviceKey(entry.id, module.id), gatewayId: entry.id, id: module.id,
    name: module.name, kind: module.kind, settings: module.settings,
    online: !!entry.state?.connected && module.online,
    subtitle: entry.port.backend === 'stm32' ? 'Discovered on your CAN chain.' : 'Your controls, within reach.',
    route: `${entry.port.serialNumber ?? entry.id} · ${module.directUsb ? 'direct USB owns inputs' : entry.port.backend === 'stm32' ? 'CAN FD' : 'USB gateway'}`,
    inputs: inputSummary(module),
    firmware: `${nameForGateway(entry.port.backend)} ${entry.state?.firmwareVersion ?? ''}`,
  }))));
  const connectedGateways = $derived(entries.filter(entry => entry.state?.connected));
  const selected = $derived(devices.find(device => device.key === selectedId)!);
  const selectedEntry = $derived(entries.find(entry => entry.id === selected?.gatewayId));
  const gateway = $derived(selectedEntry?.state ?? null);
  const gatewayBusy = $derived(!!selectedEntry?.busy);
  const gatewayError = $derived(selectedEntry?.error ?? '');
  const gatewayConnected = $derived(!!gateway?.connected);
  const physicalGateway = $derived(gateway?.backend === 'stm32');
  const gatewayName = $derived(nameForGateway(gateway?.backend));
  const needsReconnect = $derived(!!gateway?.needsReconnect);
  const sessionDevice = $derived(devices.find(device => device.key === sessionId));
  const removeEntry = $derived(entries.find(entry => entry.id === removeTarget.gatewayId));
  const canRemove = $derived(!!removeEntry?.state?.connected && !removeEntry.busy && !removeEntry.state.reconnecting && !removeEntry.state.claimedModules?.length && !!removeEntry.state.modules.find(module => module.id === removeTarget.id)?.registered);
  const draft = $derived(drafts[selectedId]);
  const dirty = $derived(!!selected && !!draft && !settingsEqual(draft, selected.settings));
  const editing = $derived(!!selected && sessionId === selectedId && selected.online && gatewayConnected);
  const runtimeSnapshot = $derived(selected ? gateway?.moduleConfigs?.[selected.id] : undefined);
  const filtered = $derived(devices.filter(device =>
    `${device.name} ${device.settings.label} ${device.id} ${device.route}`.toLowerCase().includes(search.toLowerCase()) && (filter !== 'online' || device.online)
  ));
  const hidModule = $derived(gateway?.modules.find(module => module.id === selected?.id));
  const controlsReady = $derived(gatewayConnected && !!gateway?.running && !!hidModule?.active && !!selected?.online && !hidModule?.directUsb);
  const held = $derived(controlsReady ? hidModule?.buttons ?? [] : []);
  const axes = $derived(controlsReady ? hidModule?.axes ?? [] : Array.from({ length: hidModule?.axisCount ?? 0 }, (_, i) => runtimeSnapshot?.config.axes[i]?.[4] ?? 0));
  const axisNames = $derived(Array.from({ length: hidModule?.axisCount ?? 0 }, (_, i) => selected?.kind === 'pedals' ? ['Throttle', 'Brake', 'Clutch'][i] ?? `Axis ${i + 1}` : selected?.kind === 'handbrake' && i === 0 ? 'Handbrake' : selected?.kind === 'wheel' && i === 0 ? 'Steering' : `Axis ${i + 1}`));
  const buttonNames = $derived(Array.from({ length: hidModule?.buttonCount ?? 0 }, (_, i) => selected?.kind === 'shifter' && i < 3 ? ['Downshift', 'Neutral', 'Upshift'][i] : `Button ${String(i + 1).padStart(2, '0')}`));

  function syncWorkspace(next: GatewayEntry[], error: string) {
    if (disposed) return;
    const ids: string[] = [];
    for (const entry of next) {
      const before = entries.find(previous => previous.id === entry.id)?.state;
      for (const module of entry.state?.modules ?? []) {
        const key = deviceKey(entry.id, module.id);
        ids.push(key);
        const previous = before?.modules.find(item => item.id === module.id);
        if (!drafts[key] || (previous && settingsEqual(drafts[key], previous.settings))) drafts[key] = { ...module.settings };
        if (previous && (previous.boot !== module.boot || previous.configurationGeneration !== module.configurationGeneration)) delete runtimeDrafts[key];
        const snapshot = entry.state?.moduleConfigs?.[module.id];
        const oldSnapshot = before?.moduleConfigs?.[module.id];
        if (snapshot && (!runtimeDrafts[key] || (oldSnapshot && configEqual(runtimeDrafts[key], oldSnapshot.config)))) runtimeDrafts[key] = cloneConfig(snapshot.config);
        if (sessionId === key && (!entry.state?.connected || !module.online || !entry.state?.claimedModules?.includes(module.id))) sessionId = null;
      }
    }
    entries = next;
    inventoryError = error;
    for (const key of Object.keys(drafts)) if (!ids.includes(key)) { delete drafts[key]; delete runtimeDrafts[key]; }
    if (selectedId && !ids.includes(selectedId)) { selectedId = ''; if (view === 'device') view = 'devices'; }
    if (sessionId && !ids.includes(sessionId)) sessionId = null;
    if (modal === 'remove' && !ids.includes(removeTarget.key)) { modal = null; notify('That module has already been removed.'); }
  }
  async function sendGateway(gatewayId: string, request: GatewayRequest, quiet = false) {
    try {
      if (!workspace) return false;
      await workspace.request(gatewayId, request);
      return true;
    } catch (error) {
      if (!quiet) notify(error instanceof Error ? error.message : 'Gateway connection failed.');
      return false;
    }
  }
  async function retryGateway(gatewayId: string) {
    try { await workspace?.retry(gatewayId); }
    catch (error) { notify(error instanceof Error ? error.message : 'Gateway connection failed.'); }
  }
  function gatewayStatus(entry: GatewayEntry): string {
    if (entry.state?.reconnecting) return 'Reconnecting USB…';
    if (entry.busy) return 'Working…';
    if (!entry.state?.connected) return entry.present ? entry.error ? 'Unavailable' : 'Connecting' : 'Offline';
    return entry.state.running ? 'USB HID active' : 'Connected';
  }
  function confirmRemove(device: WorkspaceDevice) {
    removeTarget = { key: device.key, id: device.id, label: device.settings.label, gatewayId: device.gatewayId };
    modal = 'remove';
  }
  onMount(() => {
    disposed = false;
    workspace = new GatewayWorkspace({ onChange: syncWorkspace });
    void workspace.refresh();
    const timer = setInterval(() => { void workspace?.refresh(); }, 3000);
    const release = () => { if (sessionDevice) releaseOnExit(sessionDevice.id, sessionDevice.gatewayId); };
    window.addEventListener('pagehide', release);
    return () => { release(); disposed = true; clearTimeout(toastTimer); clearInterval(timer); window.removeEventListener('pagehide', release); workspace?.dispose(); };
  });
  function openLink(event: MouseEvent) {
    const opening = openCockpitLink(event);
    if (opening) void opening.catch(error => notify(error instanceof Error ? error.message : 'This link could not be opened.'));
  }
  function dismissNotification() { clearTimeout(toastTimer); toastTimer = undefined; toast = ''; }
  function notify(message: string) {
    if (disposed) return;
    dismissNotification(); toast = message; toastSequence++;
    toastTimer = setTimeout(dismissNotification, notificationDuration);
  }
  function log(message: string, detail: string) { events = [{ time: new Date().toLocaleTimeString([], { hour: '2-digit', minute: '2-digit' }), message, detail }, ...events]; }
  function toggleTheme() {
    light = !light;
    try { localStorage.setItem('osg-light', JSON.stringify(light)); }
    catch { notify('Theme changed for this session. Local storage is unavailable.'); }
  }
  async function selectDevice(key: string) {
    const device = devices.find(item => item.key === key);
    if (!device) return;
    selectedId = key; tab = 'setup'; view = 'device';
    const state = entries.find(entry => entry.id === device.gatewayId)?.state;
    if (state?.backend === 'stm32' && state.connected && state.modules.find(module => module.id === device.id)?.canConfigure) await sendGateway(device.gatewayId, { command: 'config_read', moduleId: device.id });
  }
  async function toggleSession() {
    if (sessionTransition) return;
    sessionTransition = true;
    try {
      if (sessionDevice) {
        const device = sessionDevice;
        if (!await sendGateway(device.gatewayId, { command: 'release', moduleId: device.id })) return;
        sessionId = null;
        log('Setup session released', `${device.name} · ${device.gatewayId}. HID controls remain available.`);
        notify('Setup session ended. HID controls remain available.');
      } else if (selected) {
        const device = selected;
        if (!await sendGateway(device.gatewayId, { command: 'claim', moduleId: device.id })) return;
        sessionId = device.key;
        log('Setup session started', `${device.name} · ${device.gatewayId}.`);
        notify('Setup session started. Settings are now editable.');
      }
    } finally { sessionTransition = false; }
  }
  async function applySettings() {
    if (!editing || !dirty || !draft.label.trim() || !selected) return;
    const device = selected;
    const settings = { ...draft, label: draft.label.trim() };
    if (!await sendGateway(device.gatewayId, { command: 'configure', moduleId: device.id, settings })) return;
    drafts[device.key] = { ...settings };
    log('Configuration saved', `${device.name} · ${device.gatewayId}. Settings are held in Pico RAM until reset.`);
    notify('Configuration applied to the gateway.');
  }
  function discard() { if (selected) drafts[selected.key] = { ...selected.settings }; notify('Draft discarded. Saved settings restored.'); }
  async function runtimeRequest(command: GatewayRequest['command'], config?: RuntimeConfig) {
    if (!selected) return false;
    const device = selected;
    if (!await sendGateway(device.gatewayId, { command, moduleId: device.id, config })) return false;
    const state = entries.find(entry => entry.id === device.gatewayId)?.state;
    const snapshot = state?.moduleConfigs?.[device.id];
    if (snapshot) runtimeDrafts[device.key] = cloneConfig(snapshot.config);
    log('Module configuration updated', `${device.name} · ${device.gatewayId} · ${command === 'config_apply' ? 'Applied on the module.' : command === 'config_apply_save' || command === 'config_save' ? 'Saved on the module.' : 'Draft restored.'}`);
    if (state?.needsReconnect) notify('Module updated. End setup and apply this gateway’s layout to publish the changed controls.');
    return true;
  }
  async function rescan() { await workspace?.refresh(); log('Discovery refreshed', `${devices.filter(device => device.online).length} devices across ${connectedGateways.length} gateways.`); }
  async function registerDevice(key: string) {
    const device = devices.find(item => item.key === key);
    if (device && await sendGateway(device.gatewayId, { command: 'register', moduleId: device.id })) {
      log('Controller registration staged', `${device.id} · ${device.gatewayId}.`);
      notify('Controller added to the pending layout. Apply layout when ready.');
    }
  }
  async function removeModule() {
    if (!canRemove) return;
    const target = { ...removeTarget };
    if (await sendGateway(target.gatewayId, { command: 'unregister', moduleId: target.id })) {
      modal = null;
      log('Controller registration removed', `${target.id} · ${target.gatewayId}.`);
      notify('Registration removed. Apply this gateway’s layout to update USB controllers.');
    }
  }
  async function reconnectGateway(gatewayId: string) {
    if (!await sendGateway(gatewayId, { command: 'reconnect' })) return;
    log('Gateway reconnected', `${gatewayId} · HID controllers rebuilt with the registered layout.`);
    notify('USB layout updated.');
    void workspace?.refresh();
  }
</script>

<div class:light class="cockpit">
  <div class="app-shell">
    <aside class="sidebar">
      <a class="brand" href="https://www.opensimgear.org" target="_blank" rel="noreferrer" onclick={openLink}><img src={light ? '/logo-light.svg' : '/logo-dark.svg'} alt="" /><span><strong>Open</strong>SimGear<small>COCKPIT</small></span></a>
      <div class="nav-label">WORKSPACE</div>
      <nav aria-label="Main navigation">
        {#each navigation as item (item.id)}
          <button title={item.label} class:current={view === item.id || (item.id === 'devices' && view === 'device')} onclick={() => { view = item.id; }}><Icon name={item.icon} size={19} />{item.label}{#if item.id === 'devices'}<span class="nav-count">{devices.length}</span>{/if}</button>
        {/each}
      </nav>
      <div class="sidebar-bottom">
        <button class="sidebar-link" class:current={view === 'preferences'} onclick={() => { view = 'preferences'; }}><Icon name="settings" size={18} />Preferences</button>
        <a class="sidebar-link" href="https://www.opensimgear.org/docs/architecture/desktop-cockpit/" target="_blank" rel="noreferrer" onclick={openLink}><Icon name="help" size={18} />Documentation<Icon name="external" size={13} /></a>
        <div class="sidebar-version"><span>Open hardware. Your control.</span><small>v0.1.0</small></div>
      </div>
    </aside>

    <div class="main-shell">
      <header class="topbar"><div class="breadcrumb">Workspace<Icon name="chevron" size={13} /><strong>{view === 'device' ? selected?.settings.label ?? 'Device' : view.charAt(0).toUpperCase() + view.slice(1)}</strong></div><div class="topbar-actions"><span class="connection-pill">{connectedGateways.length} {connectedGateways.length === 1 ? 'gateway' : 'gateways'} connected</span><button class="icon-button" aria-label={light ? 'Use dark theme' : 'Use light theme'} onclick={toggleTheme}><Icon name={light ? 'moon' : 'sun'} size={18} /></button><button class="icon-button" aria-label="About Cockpit" onclick={() => { modal = 'help'; }}><Icon name="help" size={18} /></button></div></header>
      <main>
        {#if view === 'activity'}
          <div class="page-heading"><div><div class="eyebrow">THE RECORD OF YOUR RIG</div><h1>Activity</h1><p>Setup sessions, saved changes, and connection events.</p></div><span class="quiet-tag">{events.length} events</span></div>
          <section class="panel activity-panel"><div class="panel-heading"><h2>Recent activity</h2><span class="quiet-tag">This session</span></div>{#each events as event, i (i)}<div class="event-row"><span class="event-icon"><Icon name="check" size={18} /></span><div><strong>{event.message}</strong><p>{event.detail}</p></div><time>{event.time}</time></div>{/each}</section>
        {:else if view === 'preferences'}
          <div class="page-heading"><div><div class="eyebrow">MAKE IT YOURS</div><h1>Preferences</h1><p>A familiar cockpit, arranged around the way you work.</p></div></div>
          <section class="panel preferences-panel"><div class="panel-heading"><h2>Appearance</h2></div><div class="preference-row"><div><strong>Color theme</strong><p>The green and slate palette from opensimgear.org.</p></div><button class="secondary-button" onclick={toggleTheme}><Icon name={light ? 'moon' : 'sun'} size={16} />{light ? 'Switch to dark' : 'Switch to light'}</button></div></section>
        {:else if view === 'gateways'}
          <div class="page-heading"><div><div class="eyebrow">YOUR HARDWARE, WORKING TOGETHER</div><h1>Gateways</h1><p>All your gateways stay connected and run independently.</p></div><span class="quiet-tag">{connectedGateways.length} connected</span></div>
          {#if inventoryError}<div class="notice amber-notice" role="alert"><p>{inventoryError}</p><button class="text-button" onclick={rescan}>Retry discovery</button></div>{/if}
          <section class="gateway-list" aria-label="Gateways">
            {#each entries as entry (entry.id)}
              {@const state = entry.state}
              {@const canGateway = entry.port.backend === 'stm32'}
              {@const gatewayDevices = devices.filter(device => device.gatewayId === entry.id)}
              {@const setupActive = !!state?.claimedModules?.length}
              <section class="gateway-control" class:gateway-offline={!state?.connected} aria-label={`${nameForGateway(entry.port.backend)} ${entry.port.serialNumber}`}>
                <div class="gateway-card-header">
                  <span class="gateway-icon"><Icon name="network" size={23} /></span>
                  <div class="gateway-copy">
                    <strong>{nameForGateway(entry.port.backend)} <span class="quiet-tag"><span class="status-dot" class:offline={!state?.connected}></span>{gatewayStatus(entry)}</span></strong>
                    <p>{entry.port.serialNumber} · {state?.port ?? entry.port.path}</p>
                    {#if state?.connected}<p class="automation-status">{canGateway ? 'CAN discovery is active. Devices appear automatically.' : 'Connected devices report their inputs independently.'}</p>
                    {:else}<p class="automation-status">{entry.present ? 'Opening this gateway’s connection.' : 'Waiting for this gateway to return. Saved controller slots are retained.'}</p>{/if}
                    {#if entry.error}<p class="gateway-error" role="alert">{entry.error}</p>{/if}
                    {#if state?.hidNaming?.error}<p class="gateway-error" role="alert">Controller names could not be updated: {state.hidNaming.error}</p>{/if}
                  </div>
                  {#if !state?.connected || entry.error}<button class="secondary-button" aria-label={`Retry gateway ${entry.port.serialNumber}`} disabled={entry.busy || state?.reconnecting} onclick={() => retryGateway(entry.id)}>Retry gateway</button>{/if}
                </div>
                <div class="gateway-modules">
                  <div class="gateway-modules-heading"><h3>Devices <span>{gatewayDevices.length}</span></h3>{#if canGateway && state}<span class="quiet-tag">{state.modules.filter(module => module.registered).length} / {state.maxModules ?? 10} controller slots</span>{/if}</div>
                  <ul class="gateway-device-list">
                    {#each gatewayDevices as device (device.key)}
                      {@const module = state?.modules.find(item => item.id === device.id)}
                      <li>
                        <button class="gateway-device-link" onclick={() => selectDevice(device.key)}><span class="status-dot" class:offline={!device.online}></span><span><strong>{device.settings.label}</strong><small>{device.inputs} · {module?.directUsb ? 'Direct USB' : device.online ? 'Online' : 'Offline'}{#if canGateway} · {module?.registered ? module.pendingProfileMatch === false ? 'Profile changed' : module.activeSlot === module.pendingSlot ? 'Registered' : 'Pending apply' : (module?.activeSlot ?? -1) >= 0 ? 'Removal pending' : 'Discovered'}{/if}</small></span><Icon name="chevron" size={13} /></button>
                        {#if canGateway && (!module?.registered || module.pendingProfileMatch === false)}<button class="secondary-button" disabled={entry.busy || setupActive || !device.online || !!module?.directUsb || (!module?.registered && (state?.modules.filter(item => item.registered).length ?? 0) >= (state?.maxModules ?? 10))} onclick={() => registerDevice(device.key)}>{module?.registered ? 'Update profile' : 'Add controller'}</button>{/if}
                        {#if canGateway && module?.registered}<button class="icon-button delete-device" aria-label={`Remove ${device.settings.label} registration from ${entry.port.serialNumber}`} title={`Remove ${device.settings.label} registration`} disabled={entry.busy || setupActive || !state?.connected} onclick={() => confirmRemove(device)}><Icon name="trash" size={15} /></button>{/if}
                      </li>
                    {:else}
                      <li class="gateway-devices-empty">{state?.connected ? 'Connect a powered device. It will appear here automatically.' : 'Device inventory will appear when this gateway connects.'}</li>
                    {/each}
                  </ul>
                </div>
                {#if state?.needsReconnect}<div class="reconnect-notice"><Icon name="alert" size={19} /><div><strong>Controller layout changes pending</strong><p>Apply this gateway’s layout to reconnect its USB controllers.</p></div><button class="secondary-button" aria-label={`Apply layout on ${entry.port.serialNumber}`} disabled={entry.busy || setupActive || !state.connected} onclick={() => reconnectGateway(entry.id)}><Icon name="usb" size={15} />{canGateway ? 'Apply layout' : 'Reconnect gateway'}</button></div>{/if}
              </section>
            {:else}
              <p class="gateway-discovery" role="status">Connect an OpenSimGear gateway to start. Every compatible gateway will appear here automatically.</p>
            {/each}
          </section>
        {:else if view === 'devices'}
          <div class="page-heading"><div><div class="eyebrow">YOUR CONTROLLERS</div><h1>Devices</h1><p>Select a device to configure it and test its inputs.</p></div><button class="secondary-button scan-button" onclick={rescan}><Icon name="refresh" size={16} />Rescan devices</button></div>
                <div class="inventory-toolbar"><label class="search-field"><Icon name="search" size={16} /><input aria-label="Search devices" placeholder="Find a device…" bind:value={search} /></label><select aria-label="Filter devices" bind:value={filter}><option value="all">All devices</option><option value="online">Connected</option></select></div>
                <div class="device-grid">
                  {#each filtered as device (device.key)}
                    <button class="device-card" class:offline={!device.online} onclick={() => selectDevice(device.key)}>
                      <div class="card-top"><span class="connection-badge"><span class="status-dot" class:offline={!device.online}></span>{device.online ? 'Connected' : 'Offline'}</span><Icon name="arrow" size={15} /></div>
                      <div class="card-illustration"><DeviceIllustration kind={device.kind} compact /></div>
                      <div class="card-copy"><h3>{device.settings.label}</h3><p>{device.inputs}</p><div class="card-route"><Icon name={device.online ? 'network' : 'usb'} size={12} />{device.route}{#if !settingsEqual(drafts[device.key], device.settings)}<span class="draft-dot" title="Unsaved changes"></span>{/if}</div></div>
                    </button>
                  {:else}<div class="empty-state"><Icon name="search" size={26} /><strong>{devices.length ? 'No matching devices' : 'No devices yet'}</strong><p>{devices.length ? 'Try another name or show all devices.' : 'Plug in your gateway to discover devices automatically.'}</p><button class="text-button" onclick={() => { search = ''; filter = 'all'; }}>Clear filters</button></div>{/each}
                </div>
        {:else if view === 'topology'}
          <div class="page-heading"><div><div class="eyebrow">YOUR HARDWARE, WORKING TOGETHER</div><h1>Topology</h1><p>All gateways connect to your computer, each with its own device chain.</p></div></div>
          <section class="panel rig-map" aria-label="Computer and gateways">
            <div class="map-host"><Icon name="monitor" size={28} /><strong>Your computer</strong><small>HID host · {connectedGateways.length} {connectedGateways.length === 1 ? 'gateway' : 'gateways'} connected</small></div>
            {#if entries.length}
              <ul class="gateway-branches" aria-label="Gateways connected to this computer">
                {#each entries as entry (entry.id)}
                  {@const gatewayDevices = devices.filter(device => device.gatewayId === entry.id)}
                  <li class="gateway-branch" class:branch-offline={!entry.state?.connected}>
                    <section class="gateway-chain" aria-labelledby={`topology-${entry.id} topology-serial-${entry.id}`}>
                      <div class="map-gateway">
                        <span class="gateway-icon"><Icon name="network" size={23} /></span>
                        <div class="map-gateway-label"><h2 id={`topology-${entry.id}`}>{nameForGateway(entry.port.backend)}</h2><small id={`topology-serial-${entry.id}`}>{entry.port.serialNumber}</small></div>
                        <span class="connection-badge"><span class="status-dot" class:offline={!entry.state?.connected}></span>{entry.state?.connected ? 'Connected' : 'Offline'}</span>
                      </div>
                      {#if gatewayDevices.length}
                        <div class="can-spine"><span>{entry.port.backend === 'stm32' ? 'CAN FD chain' : 'Controller chain'}</span></div>
                        <ul class="map-nodes" aria-label={`Devices on ${entry.port.serialNumber}`}>
                          {#each gatewayDevices as device (device.key)}
                            <li><button class:active={selectedId === device.key} class:offline={!device.online} aria-label={`${device.settings.label} · ${device.online ? 'Connected' : 'Offline'} · gateway ${entry.port.serialNumber}`} onclick={() => selectDevice(device.key)}><div class="node-art"><DeviceIllustration kind={device.kind} compact /></div><strong>{device.settings.label}</strong><small>{device.id}</small><span class="connection-badge"><span class="status-dot" class:offline={!device.online}></span>{device.online ? 'Connected' : 'Offline'}</span></button></li>
                          {/each}
                        </ul>
                      {:else}<p class="map-empty">{entry.state?.connected ? 'No devices connected yet.' : 'Device inventory will return when this gateway connects.'}</p>{/if}
                    </section>
                  </li>
                {/each}
              </ul>
            {:else}<p class="map-empty">Connect a gateway to see its device chain.</p>{/if}
          </section>
        {:else if view === 'device' && selected}
          <button class="text-button device-back" onclick={() => { view = 'devices'; }}><Icon name="arrow" size={14} />Back to devices</button>
          <div class="page-heading"><div><div class="eyebrow">DEVICE WORKSPACE</div><h1>{selected.settings.label}</h1><p>Configuration and live controller inputs.</p></div></div>
          {#if needsReconnect}<div class="reconnect-notice"><Icon name="alert" size={19} /><div><strong>{physicalGateway ? 'Controller layout changes pending' : `${gatewayName} needs a reconnect`}</strong><p>Apply the pending controller layout to reconnect USB. Check your game bindings afterward.</p></div><button class="secondary-button" disabled={gatewayBusy || !gatewayConnected || gateway?.reconnecting || !!gateway?.claimedModules?.length} onclick={() => reconnectGateway(selected.gatewayId)}><Icon name="usb" size={15} />{physicalGateway ? 'Apply layout' : 'Reconnect gateway'}</button></div>{/if}

          {#if gatewayError}<div class="notice amber-notice" role="alert"><Icon name="alert" size={17} /><p>{gatewayError}</p></div>{/if}
          <div class="device-workspace">
            <div>
                <section class="panel device-stage"><div class="stage-title"><div><span class="eyebrow">DEVICE WORKSPACE</span><h2>{selected.name}</h2><p>{selected.subtitle}</p></div><span class="connection-badge"><span class="status-dot" class:offline={!selected.online}></span>{selected.online ? 'Connected' : 'Offline'}</span></div><div class="large-device"><DeviceIllustration kind={selected.kind} /></div><div class="stage-metadata"><div><small>STABLE IDENTITY</small><strong>{selected.id}</strong></div><div><small>ACTIVE ROUTE</small><strong>{selected.route}</strong></div><div><small>CAPABILITIES</small><strong>{selected.inputs}</strong></div></div></section>
                <section class="panel device-inputs"><div class="panel-heading"><h2>Live inputs</h2><span class="quiet-tag">{controlsReady ? 'Live HID output' : 'Device inputs'}</span></div>{@render inputTester()}</section>
            </div>
            <section class="panel inspector" aria-label="Selected device settings">
              <div class="inspector-heading"><div><span class="eyebrow">CONFIGURATION</span><h2>{selected.settings.label}</h2></div><span class="model-badge">{selected.kind === 'shifter' ? 'SQ' : selected.kind === 'pedals' ? 'PD' : selected.kind === 'panel' ? 'BP' : selected.kind === 'wheel' ? 'WH' : 'HB'}</span></div>
              <div class="inspector-tabs" aria-label="Device sections">{#each ['setup', 'details'] as item (item)}<button class:active={tab === item} aria-pressed={tab === item} onclick={() => { tab = item; }}>{item.charAt(0).toUpperCase() + item.slice(1)}</button>{/each}</div>
              <div class="inspector-body">
                {#if !selected.online}<div class="notice offline-notice"><Icon name="alert" size={17} /><div><strong>Device is offline</strong><p>Reconnect this module to configure it. Its saved identity and settings are preserved.</p></div></div>{/if}
                {#if physicalGateway && tab === 'setup'}
                  {#if runtimeSnapshot&&runtimeDrafts[selectedId]}<RuntimeEditor bind:config={runtimeDrafts[selectedId]} snapshot={runtimeSnapshot} {editing} busy={gatewayBusy} request={runtimeRequest} />
                  {:else}<div class="notice"><Icon name="info" size={17} /><p>{hidModule?.canConfigure ? 'Read module configuration by opening a setup session.' : 'This module firmware does not expose runtime configuration. Inputs remain available.'}</p></div>{/if}
                  <dl class="device-details"><div><dt>Controller registration</dt><dd>{hidModule?.registered ? 'Registered' : 'Discovered'}</dd></div><div><dt>USB report ID</dt><dd>{hidModule?.reportId ?? 'Pending'}</dd></div><div><dt>Input owner</dt><dd>{hidModule?.directUsb ? 'Device USB' : 'Gateway'}</dd></div></dl>
                  {#if hidModule?.profileMatch === false && (hidModule?.activeSlot ?? -1) >= 0}<div class="notice amber-notice"><Icon name="alert" size={17} /><p>The device profile has changed. Update its registration and apply the controller layout to resume inputs.</p></div>{/if}
                {:else if tab === 'setup'}
                  <div class="session-notice" class:session-active={editing}><Icon name={editing ? 'lock' : 'usb'} size={17} /><div><strong>{editing ? 'Setup session active' : sessionId ? `Setup active: ${sessionDevice?.settings.label}` : !selected.online ? 'Reconnect to configure' : 'Ready to configure'}</strong><p>{editing ? 'Cockpit has configuration access.' : sessionId ? 'End that session before configuring this module.' : !selected.online ? 'Setup needs a connected module.' : 'Start a session to edit this device.'}</p></div></div>
                  <label class="field-label" for="device-name">Device name<span>Saved on gateway</span></label><input class="text-input" id="device-name" bind:value={draft.label} disabled={!editing} maxlength="80" />
                  <div class="setting-heading"><label for="debounce">Input debounce</label><span>{draft.debounce}<small> ms</small></span></div><input id="debounce" aria-label="Input debounce" type="range" min="1" max="50" step="1" bind:value={draft.debounce} disabled={!editing} /><div class="range-labels"><span>1 ms</span><span>50 ms</span></div><p class="field-help">Ignore brief contact noise for a clean input.</p>
                  {#if selected.kind !== 'panel'}<div class="toggle-row"><div><strong>Invert {selected.kind === 'shifter' ? 'shift direction' : 'input direction'}</strong><p>{selected.kind === 'shifter' ? 'Swap the upshift and downshift inputs.' : 'Reverse the reported input direction.'}</p></div><button class="toggle" class:on={draft.inverted} role="switch" aria-checked={draft.inverted} aria-label="Invert input direction" disabled={!editing} onclick={() => { draft.inverted = !draft.inverted; }}><span></span></button></div>{/if}
                  <div class="configuration-status"><span class="status-dot" class:draft-dot={dirty}></span>{dirty ? 'Unsaved changes' : 'Configuration up to date'}<span>{dirty ? 'Draft' : 'Saved'}</span></div>
                {:else}
                  <dl class="device-details"><div><dt>Stable identity</dt><dd>{selected.id}</dd></div><div><dt>Connection</dt><dd>{selected.route}</dd></div><div><dt>Firmware</dt><dd>{selected.firmware}</dd></div><div><dt>Capabilities</dt><dd>{selected.inputs}</dd></div><div><dt>Configuration owner</dt><dd>{editing ? 'Cockpit' : 'None'}</dd></div></dl><div class="notice"><Icon name="info" size={17} /><p>{physicalGateway ? `The gateway serial is ${gateway?.gatewaySerial ?? ''}. This controller uses report ID ${hidModule?.reportId ?? 'pending'} on the shared HID interface. Registration is saved in gateway flash when the layout is applied.` : `USB interfaces share the Pico serial ${gateway?.gatewaySerial ?? ''}. This module maps to USB interface ${hidModule?.hidInterface ?? 'pending'}. Settings stay in RAM until the Pico resets.`}</p></div>
                {/if}
              </div>
              <div class="inspector-footer">
                <button class={editing || sessionId ? 'secondary-button session-button' : 'primary-button session-button'} disabled={sessionTransition || gatewayBusy || (!selected.online && !sessionId) || (physicalGateway&&!hidModule?.canConfigure&&!sessionId)} onclick={toggleSession}><Icon name={sessionId ? 'stop' : 'play'} size={15} />{sessionId && !editing ? `End ${sessionDevice?.settings.label} session` : sessionId ? 'End setup session' : 'Start setup session'}</button>
                {#if !physicalGateway&&dirty}<div class="save-actions"><button class="text-button" onclick={discard}>Discard</button><button class="primary-button" disabled={gatewayBusy || !editing || !draft.label.trim()} onclick={applySettings}><Icon name="check" size={15} />Apply & save</button></div>{/if}
              </div>
            </section>
          </div>
        {/if}
      </main>
    </div>
  </div>

  {#if toast}
    {#key toastSequence}
      <div class="toast" role="status" style={`--toast-duration: ${notificationDuration}ms`}>
        <span class="toast-icon"><Icon name="check" size={16} /></span><p>{toast}</p>
        <button class="icon-button" aria-label="Dismiss notification" onclick={dismissNotification}><Icon name="close" size={15} /></button>
        <span class="toast-countdown" aria-hidden="true"></span>
      </div>
    {/key}
  {/if}
  {#if modal}
    <div class="modal-backdrop"><dialog class="modal" aria-labelledby="modal-title" {@attach showDialog} onclose={() => { modal = null; }}>
      <button class="modal-close icon-button" aria-label="Close dialog" onclick={() => { modal = null; }}><Icon name="close" /></button>
      {#if modal === 'help'}
        <span class="modal-icon"><Icon name="wheel" size={30} /></span><span class="eyebrow">WELCOME TO YOUR COCKPIT</span><h2 id="modal-title">Built around your hardware.</h2><p>Plug in your CAN gateway. Cockpit discovers connected devices automatically. Add their controller registrations and apply the layout to publish them over USB.</p><div class="notice"><Icon name="info" size={18} /><p>Offline devices retain their controller slots with neutral inputs. CAN gateway registrations persist in flash. Pico settings and registration stay in RAM until reset.</p></div><a class="primary-button" href="https://github.com/opensimgear/device-gateway/tree/main/simulation/pico" target="_blank" rel="noreferrer" onclick={openLink}>Flashing instructions<Icon name="external" size={16} /></a>
      {:else if modal === 'remove'}
        <span class="modal-icon"><Icon name="close" size={30} /></span><span class="eyebrow">REMOVE REGISTRATION</span><h2 id="modal-title">Remove {removeTarget.label}?</h2><p>Remove the controller registration for {removeTarget.id}. The device remains discoverable on CAN. Apply the layout to remove its published USB controller.</p><div class="modal-actions"><button class="secondary-button" onclick={() => { modal = null; }}>Cancel</button><button class="primary-button" disabled={!canRemove} onclick={removeModule}>Remove registration</button></div>
      {/if}
    </dialog></div>
  {/if}
</div>

{#snippet inputTester()}
  <div class="input-tester">
    <div class="hid-state"><span class="status-dot" class:offline={!controlsReady}></span><span>{controlsReady ? `HID · ${selected.id}` : physicalGateway && hidModule?.directUsb ? 'Device USB owns these inputs' : physicalGateway && !hidModule?.registered ? 'Add this controller and apply the layout' : !gateway?.running ? physicalGateway ? 'Apply the layout to publish HID inputs' : 'Start the gateway to publish HID inputs' : !selected.online ? 'Module offline · HID slot retained' : physicalGateway && (hidModule?.activeSlot ?? -1) >= 0 && hidModule?.profileMatch ? 'Waiting for fresh device input' : 'Controller pending gateway reconnect'}</span></div>
    {#if buttonNames.length}
      <div class="input-buttons" class:panel-buttons={selected.kind === 'panel'}>{#each buttonNames as control, i (control)}<button class:pressed={held.includes(i + 1)} disabled><Icon name={control === 'Neutral' ? 'grid' : 'arrow'} size={19} /><span>{control}</span><small>{held.includes(i + 1) ? 'ON' : 'OFF'}</small></button>{/each}</div>
    {/if}
      {#each axisNames as name, i (name)}<label class="axis-test"><span>{name}<strong>{Math.round((axes[i] ?? 0) / 655.35)}%</strong></span><input aria-label={`${name} live input`} type="range" min="0" max="100" value={Math.round((axes[i] ?? 0) / 655.35)} disabled /><span class="range-labels"><span>Released</span><span>Full travel</span></span></label>{/each}
    <p class="field-help">Live device inputs are read-only. Inputs remain neutral until registration and layout are active.</p>
    {#if physicalGateway}<div class="device-details">{#each hidModule?.hats ?? [] as hat, i (i)}<p>Hat {i + 1}: {hat === 8 ? 'Neutral' : ['N', 'NE', 'E', 'SE', 'S', 'SW', 'W', 'NW'][hat]}</p>{/each}</div>{/if}
  </div>
{/snippet}
