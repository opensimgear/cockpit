<script lang="ts">
  import { onMount } from 'svelte';
  import { SvelteMap, SvelteSet } from 'svelte/reactivity';
  import { automaticGatewayAction, gatewayCandidates } from './lib/gateway-automation';
  import Icon from './lib/Icon.svelte';
  import DeviceIllustration from './lib/DeviceIllustration.svelte';
  import { settingsEqual, type Device, type Settings } from './lib/devices';
  import { gatewayRequest, releaseOnExit, openCockpitLink, GatewayError, type GatewayRequest, type GatewayState, type NewModule } from './lib/gateway';

  type View = 'gateways' | 'devices' | 'device' | 'topology' | 'activity' | 'preferences';
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
      if (typeof fallback === 'object') {
        const setting = value as Partial<Settings>;
        if (typeof setting.label !== 'string' || !setting.label.trim() || setting.label.length > 40 ||
          typeof setting.debounce !== 'number' || !Number.isInteger(setting.debounce) || setting.debounce < 1 || setting.debounce > 50 ||
          typeof setting.inverted !== 'boolean') return fallback;
      }
      return value as T;
    } catch { return fallback; }
  }
  let light = $state(stored<boolean>('osg-light', false));
  let view = $state<View>('gateways');
  let devices = $state<Device[]>([]);
  let drafts = $state<Record<string, Settings>>({});
  let selectedId = $state('');
  let search = $state('');
  let filter = $state('all');
  let tab = $state('setup');
  let sessionId = $state<string | null>(null);
  const notificationDuration = 5000;
  let toast = $state('');
  let toastSequence = $state(0);
  let toastTimer: ReturnType<typeof setTimeout> | undefined;
  let modal = $state<'help' | 'register' | 'remove' | null>(null);
  let removeTarget = $state({ id: '', label: '' });
  let newModule = $state<NewModule>({ id: '', name: 'OSG Button panel', kind: 'panel', buttonCount: 16, axisCount: 0 });
  let needsReconnect = $state(false);
  let gateway = $state<GatewayState | null>(null);
  let gatewayBusy = $state(false);
  let gatewayError = $state('');
  let inputValues = $state<Record<string, { buttons: number[]; axes: number[] }>>({});
  let inputSending = $state(false);
  const axisTimers = new SvelteMap<string, ReturnType<typeof setTimeout>>();
  const pendingInputs = new SvelteSet<string>();
  let refreshing = false;
  let monitoring = false;
  let disposed = false;
  let automaticPaused = $state(false);
  const failedAutoPorts = new SvelteSet<string>();
  let connectionEpoch = 0;
  const draftCache = new SvelteMap<string, Record<string, Settings>>();
  let events = $state([{ time: 'Just now', message: 'Workspace opened', detail: 'Compatible gateways connect automatically and discover their devices.' }]);
  const physicalGateway = $derived(gateway?.backend === 'stm32');
  const gatewayName = $derived(physicalGateway ? 'OpenSimGear CAN gateway' : 'Pico W USB gateway');
  const chainName = $derived(physicalGateway ? 'CAN FD chain' : 'simulated CAN chain');
  const sessionDevice = $derived(devices.find(device => device.id === sessionId));
  const selected = $derived(devices.find(device => device.id === selectedId)!);
  const draft = $derived(drafts[selectedId]);
  const dirty = $derived(!!selected && !!draft && !settingsEqual(draft, selected.settings));
  const automation = $derived(automaticGatewayAction(gateway, { paused: automaticPaused, setupActive: sessionId !== null, failedPorts: failedAutoPorts }));
  const gatewayConnected = $derived(!!gateway?.connected);
  const editing = $derived(!physicalGateway && !!selected && sessionId === selectedId && selected.online && gatewayConnected);
  const filtered = $derived(devices.filter(device =>
    `${device.name} ${device.settings.label} ${device.id}`.toLowerCase().includes(search.toLowerCase()) && (filter !== 'online' || device.online)
  ));
  const hidModule = $derived(gateway?.modules.find(module => module.id === selectedId));
  const controlsReady = $derived(gatewayConnected && !!gateway?.running && !!hidModule?.active && !!selected?.online && !hidModule?.directUsb);
  const held = $derived(inputValues[selectedId]?.buttons ?? []);
  const axes = $derived(inputValues[selectedId]?.axes ?? []);
  const axisNames = $derived(Array.from({ length: hidModule?.axisCount ?? 0 }, (_, i) => selected?.kind === 'pedals' ? ['Throttle', 'Brake', 'Clutch'][i] ?? `Axis ${i + 1}` : selected?.kind === 'handbrake' && i === 0 ? 'Handbrake' : selected?.kind === 'wheel' && i === 0 ? 'Steering' : `Axis ${i + 1}`));
  const buttonNames = $derived(Array.from({ length: hidModule?.buttonCount ?? 0 }, (_, i) => selected?.kind === 'shifter' && i < 3 ? ['Downshift', 'Neutral', 'Upshift'][i] : selected?.id === 'OSG-BP-0007' && i >= 12 ? `Encoder ${Math.floor((i - 12) / 2) + 1} ${i % 2 === 0 ? '−' : '+'}` : `Button ${String(i + 1).padStart(2, '0')}`));
  const newModuleValid = $derived(/^[A-Za-z0-9_-]{1,40}$/.test(newModule.id) && !devices.some(device => device.id === newModule.id) && newModule.name.trim().length > 0 && newModule.name.trim().length <= 80 && Number.isInteger(newModule.buttonCount) && newModule.buttonCount >= 0 && newModule.buttonCount <= 32 && Number.isInteger(newModule.axisCount) && newModule.axisCount >= 0 && newModule.axisCount <= 6 && newModule.buttonCount + newModule.axisCount > 0);

  function cancelInputs() {
    connectionEpoch++;
    for (const timer of axisTimers.values()) clearTimeout(timer);
    axisTimers.clear(); pendingInputs.clear();
    inputValues = {};
  }
  function draftKey(state: GatewayState | null = gateway) { return `${state?.backend ?? 'pico'}:${state?.gatewaySerial ?? 'local'}`; }
  function rememberDrafts() {
    if (devices.length) draftCache.set(draftKey(), Object.fromEntries(Object.entries(drafts).map(([id, settings]) => [id, { ...settings }])));
  }

  function syncGateway(state: GatewayState) {
    const connectionChanged = !!state.gatewaySerial && (gateway?.gatewaySerial !== state.gatewaySerial || gateway?.backend !== state.backend);
    if (connectionChanged || (gateway?.connected && !state.connected) || state.reconnecting) {
      cancelInputs(); sessionId = null;
      if (connectionChanged) { rememberDrafts(); devices = []; drafts = { ...draftCache.get(draftKey(state)) }; }
    }
    const ids = new Set(state.modules.map(module => module.id));
    if (modal === 'remove' && !ids.has(removeTarget.id)) { modal = null; notify('That module has already been removed.'); }
    devices = devices.filter(device => ids.has(device.id));
    gateway = state;
    needsReconnect = state.needsReconnect;
    for (const module of state.modules) {
      const device = devices.find(device => device.id === module.id);
      if (device) {
        const clean = settingsEqual(drafts[device.id], device.settings);
        device.online = module.online && !!state.connected;
        device.firmware = state.backend === 'stm32' ? `Gateway ${state.firmwareVersion ?? ''} · CAN v${state.canProtocol ?? 2}` : `Pico ${state.firmwareVersion ?? 'simulator'}`;
        device.route = `${state.backend === 'stm32' ? 'OpenSimGear CAN gateway' : 'Pico W USB gateway'} · ${module.directUsb ? 'direct USB owns inputs' : state.backend === 'stm32' ? 'CAN FD' : 'simulated CAN'}`;
        device.inputs = [module.buttonCount ? `${module.buttonCount} buttons` : '', module.hatCount ? `${module.hatCount} hats` : '', module.axisCount ? `${module.axisCount} axes` : ''].filter(Boolean).join(' · ');
        device.settings = { ...module.settings };
        if (clean) drafts[device.id] = { ...module.settings };
      } else {
        devices.push({ id: module.id, name: module.name, kind: module.kind, subtitle: state.backend === 'stm32' ? 'Discovered on your CAN chain.' : 'Your controls, within reach.', online: module.online && !!state.connected, route: `${state.backend === 'stm32' ? 'OpenSimGear CAN gateway' : 'Pico W USB gateway'} · ${module.directUsb ? 'direct USB owns inputs' : state.backend === 'stm32' ? 'CAN FD' : 'simulated CAN'}`, inputs: [module.buttonCount ? `${module.buttonCount} buttons` : '', module.hatCount ? `${module.hatCount} hats` : '', module.axisCount ? `${module.axisCount} axes` : ''].filter(Boolean).join(' · '), firmware: state.backend === 'stm32' ? `Gateway ${state.firmwareVersion ?? ''} · CAN v${state.canProtocol ?? 2}` : `Pico ${state.firmwareVersion ?? 'simulator'}`, settings: { ...module.settings } });
        drafts[module.id] ??= { ...module.settings };
      }
      if (!module.online || (!inputSending && !axisTimers.has(module.id) && !pendingInputs.has(module.id))) inputValues[module.id] = { buttons: [...module.buttons], axes: [...module.axes] };
    }
    for (const id of Object.keys(drafts)) if (!ids.has(id)) { delete drafts[id]; delete inputValues[id]; clearTimeout(axisTimers.get(id)); axisTimers.delete(id); pendingInputs.delete(id); }
    if (!ids.has(selectedId)) { selectedId = ''; if (view === 'device') view = 'devices'; }
    if (sessionId && (!state.modules.find(module => module.id === sessionId)?.online || (state.claimedModules && !state.claimedModules.includes(sessionId)))) { sessionId = null; notify('Setup access was released. Start a new session to save changes.'); }
  }
  async function sendGateway(request: GatewayRequest, quiet = false) {
    if (quiet && refreshing) return false;
    if (quiet) refreshing = true;
    if (!quiet) gatewayBusy = true;
    if (['claim', 'release', 'configure'].includes(request.command)) connectionEpoch++;
    const epoch = connectionEpoch;
    try {
      const state = await gatewayRequest(request);
      if (epoch !== connectionEpoch) return false;
      syncGateway(state);
      if (!quiet) gatewayError = '';
      return true;
    } catch (error) {
      if (epoch !== connectionEpoch) return false;
      if (error instanceof GatewayError && error.state) syncGateway(error.state);
      else if (gateway) { cancelInputs(); sessionId = null; gateway = { ...gateway, running: false, connected: false, modules: gateway.modules.map(module => ({ ...module, active: false, online: false })) }; devices = devices.map(device => ({ ...device, online: false })); }
      gatewayError = error instanceof Error ? error.message : 'Gateway connection failed.';
      if (!quiet) notify(gatewayError);
      return false;
    } finally { if (!quiet) gatewayBusy = false; else refreshing = false; }
  }
  async function retryGateway() {
    automaticPaused = false;
    failedAutoPorts.clear();
    await monitorGateway();
  }
  function confirmRemove(device: Device) {
    removeTarget = { id: device.id, label: device.settings.label };
    modal = 'remove';
  }
  async function setPresence() {
    if (physicalGateway) return;
    const online = !selected.online;
    const id = selectedId;
    const name = selected.name;
    cancelInputs();
    if (await sendGateway({ command: 'presence', moduleId: id, online })) log(online ? 'Module reconnected' : 'Module disconnected', `${name} · its HID controller slot remains registered.`);
  }
  async function sendInput(id: string) {
    if (physicalGateway || !gateway?.running) return;
    pendingInputs.add(id);
    if (inputSending) return;
    inputSending = true;
    const epoch = connectionEpoch;
    try {
      while (pendingInputs.size && gateway?.running && epoch === connectionEpoch) {
        const moduleId = pendingInputs.values().next().value!;
        pendingInputs.delete(moduleId);
        const values = { buttons: [...inputValues[moduleId].buttons], axes: [...inputValues[moduleId].axes] };
        const state = await gatewayRequest({ command: 'input', moduleId, ...values });
        if (epoch !== connectionEpoch) break;
        syncGateway(state);
        if (!state.modules.some(module => module.id === moduleId)) continue;
        if (JSON.stringify(inputValues[moduleId]) !== JSON.stringify(values)) pendingInputs.add(moduleId);
        gatewayError = '';
      }
    } catch (error) {
      if (epoch !== connectionEpoch) return;
      if (error instanceof GatewayError && error.state) syncGateway(error.state);
      else if (gateway) { cancelInputs(); sessionId = null; gateway = { ...gateway, running: false, connected: false, modules: gateway.modules.map(module => ({ ...module, active: false, online: false })) }; devices = devices.map(device => ({ ...device, online: false })); }
      pendingInputs.clear();
      gatewayError = error instanceof Error ? error.message : 'HID report failed.';
    } finally { inputSending = false; }
  }
  function toggleButton(index: number) {
    const values = inputValues[selectedId] ?? { buttons: [], axes: [] };
    inputValues[selectedId] = { ...values, buttons: values.buttons.includes(index) ? values.buttons.filter(button => button !== index) : [...values.buttons, index] };
    void sendInput(selectedId);
  }
  function setAxis(index: number, percent: number) {
    const id = selectedId;
    const values = inputValues[id] ?? { buttons: [], axes: Array(axisNames.length).fill(0) };
    const next = [...values.axes]; next[index] = Math.round(percent * 655.35);
    inputValues[id] = { ...values, axes: next };
    clearTimeout(axisTimers.get(id));
    axisTimers.set(id, setTimeout(() => { axisTimers.delete(id); void sendInput(id); }, 60));
  }
  async function runAutomaticGateway() {
    if (disposed || gatewayBusy || inputSending) return;
    let next = automaticGatewayAction(gateway, { paused: automaticPaused, setupActive: sessionId !== null, failedPorts: failedAutoPorts });
    if (next.command === 'connect') {
      const port = next.port!;
      failedAutoPorts.add(port);
      if (!await sendGateway({ command: 'connect', port, backend: next.backend }) || disposed) return;
      failedAutoPorts.delete(port);
      log('Gateway connected automatically', gateway?.gatewaySerial ?? port);
      next = automaticGatewayAction(gateway, { paused: automaticPaused, setupActive: sessionId !== null, failedPorts: failedAutoPorts });
    }
    if (next.command === 'start') {
      if (await sendGateway({ command: 'start' })) log('Gateway started automatically', 'Compatible modules are ready for USB HID input testing.');
      else automaticPaused = true;
    }
  }
  async function monitorGateway() {
    if (disposed || monitoring || gatewayBusy || inputSending || refreshing) return;
    monitoring = true;
    try {
      if (!await sendGateway({ command: 'status' }, true) || disposed) return;
      if (!gatewayConnected && gatewayCandidates(gateway?.ports).length === 0) automaticPaused = false;
      for (const port of failedAutoPorts) if (!gateway?.ports?.some(item => item.path === port)) failedAutoPorts.delete(port);
      await runAutomaticGateway();
    } finally { monitoring = false; }
  }
  onMount(() => {
    disposed = false;
    void monitorGateway();
    const timer = setInterval(() => { void monitorGateway(); }, 3000);
    const release = () => { if (sessionId) releaseOnExit(sessionId); };
    window.addEventListener('pagehide', release);
    return () => { disposed = true; clearTimeout(toastTimer); clearInterval(timer); for (const timer of axisTimers.values()) clearTimeout(timer); window.removeEventListener('pagehide', release); release(); };
  });

  function openLink(event: MouseEvent) {
    const opening = openCockpitLink(event);
    if (opening) void opening.catch(error => notify(error instanceof Error ? error.message : 'This link could not be opened.'));
  }

  function persist(key: string, value: unknown): boolean {
    try { localStorage.setItem(key, JSON.stringify(value)); return true; } catch { return false; }
  }
  function dismissNotification() {
    clearTimeout(toastTimer);
    toastTimer = undefined;
    toast = '';
  }
  function notify(message: string) {
    if (disposed) return;
    dismissNotification();
    toast = message;
    toastSequence++;
    toastTimer = setTimeout(dismissNotification, notificationDuration);
  }
  function log(message: string, detail: string) {
    events = [{ time: new Date().toLocaleTimeString([], { hour: '2-digit', minute: '2-digit' }), message, detail }, ...events];
  }
  function toggleTheme() { light = !light; if (!persist('osg-light', light)) notify('Theme changed for this session. Local storage is unavailable.'); }
  function selectDevice(id: string) { selectedId = id; tab = 'setup'; view = 'device'; }
  async function toggleSession() {
    if (physicalGateway) return;
    if (sessionId) {
      const id = sessionId;
      if (!await sendGateway({ command: 'release', moduleId: id })) return;
      log('Setup session released', `${devices.find(device => device.id === id)?.name}. HID controls remain available.`);
      sessionId = null;
      notify('Setup session ended. HID controls remain available.');
    } else {
      const id = selectedId;
      const name = selected.name;
      if (!await sendGateway({ command: 'claim', moduleId: id })) return;
      sessionId = id;
      log('Setup session started', `${name} · configuration access claimed in the gateway bridge.`);
      notify('Simulator setup session started. Settings are now editable.');
    }
  }
  async function applySettings() {
    if (!editing || !dirty || !draft.label.trim()) return;
    const settings = { ...draft, label: draft.label.trim() };
    const id = selectedId;
    if (!await sendGateway({ command: 'configure', moduleId: id, settings })) return;
    const device = devices.find(device => device.id === id)!;
    device.settings = settings;
    drafts[id] = { ...settings };
    log('Configuration saved', `${device.name} · ${settings.debounce} ms debounce. Settings are held in Pico RAM until reset.`);
    notify('Configuration applied to the gateway simulator.');
  }
  function discard() { drafts[selectedId] = { ...selected.settings }; notify('Draft discarded. Saved settings restored.'); }
  async function rescan() { if (await sendGateway({ command: 'status' })) { log('Discovery refreshed', `${devices.filter(device => device.online).length} modules online.`); notify('Device inventory refreshed.'); } }
  async function registerDevice(id: string) {
    if (await sendGateway({ command: 'register', moduleId: id })) {
      log('Controller registration staged', `${id} · apply the layout to publish this controller over USB.`);
      notify('Controller added to the pending layout. Apply layout when ready.');
    }
  }
  async function addModule() {
    if (!newModuleValid || !await sendGateway({ command: 'add', module: { ...newModule, name: newModule.name.trim() } })) return;
    modal = null;
    selectDevice(newModule.id);
    log('Module added', 'Registration is pending. Reconnect the gateway to publish the updated USB HID layout.');
    notify('Module added. Reconnect the gateway to publish its HID controller.');
  }
  function openAdd() {
    if (physicalGateway) return;
    newModule = { id: `OSG-BP-${crypto.randomUUID().slice(0, 8).toUpperCase()}`, name: 'OSG Button panel', kind: 'panel', buttonCount: 16, axisCount: 0 };
    modal = 'register';
  }
  function chooseModuleKind(kind: NewModule['kind']) {
    const defaults = { shifter: ['OSG Sequential shifter', 3, 0], pedals: ['OSG Pedal set', 0, 3], panel: ['OSG Button panel', 16, 0], handbrake: ['OSG Handbrake', 0, 1], wheel: ['OSG Steering wheel', 8, 1] } as const;
    const [name, buttonCount, axisCount] = defaults[kind];
    newModule = { ...newModule, kind, name, buttonCount, axisCount };
  }
  async function removeModule() {
    const id = removeTarget.id;
    if (!devices.some(device => device.id === id)) { modal = null; return; }
    cancelInputs();
    if (await sendGateway({ command: 'remove', moduleId: id })) {
      const cached = draftCache.get(draftKey()); if (cached) delete cached[id];
      modal = null;
      log(physicalGateway ? 'Controller registration removed' : 'Module removed', `${id} · apply the gateway layout to remove its published controller.`);
      notify(physicalGateway ? needsReconnect ? 'Registration removed. Apply layout to update USB controllers.' : 'Pending registration removed. The device remains discoverable.' : needsReconnect ? 'Module removed. Reconnect the gateway to update USB controllers.' : 'Pending module removed.');
    }
  }
  async function reconnectGateway() {
    cancelInputs(); sessionId = null;
    if (!await sendGateway({ command: 'reconnect' })) return;
    log('Gateway reconnected', 'HID controllers rebuilt with the registered layout. Check host controller bindings.');
    notify('USB layout updated.');
    await runAutomaticGateway();
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
      <header class="topbar"><div class="breadcrumb">Workspace<Icon name="chevron" size={13} /><strong>{view === 'device' ? selected?.settings.label ?? 'Device' : view.charAt(0).toUpperCase() + view.slice(1)}</strong></div><div class="topbar-actions"><span class="connection-pill">{gateway?.running ? 'USB HID active' : gatewayConnected ? physicalGateway ? 'USB connected' : 'Gateway stopped' : 'Gateway disconnected'}</span><button class="icon-button" aria-label={light ? 'Use dark theme' : 'Use light theme'} onclick={toggleTheme}><Icon name={light ? 'moon' : 'sun'} size={18} /></button><button class="icon-button" aria-label="About Cockpit" onclick={() => { modal = 'help'; }}><Icon name="help" size={18} /></button></div></header>
      <main>
        {#if view === 'activity'}
          <div class="page-heading"><div><div class="eyebrow">THE RECORD OF YOUR RIG</div><h1>Activity</h1><p>Setup sessions, saved changes, and connection events.</p></div><span class="quiet-tag">{events.length} events</span></div>
          <section class="panel activity-panel"><div class="panel-heading"><h2>Recent activity</h2><span class="quiet-tag">This session</span></div>{#each events as event, i (i)}<div class="event-row"><span class="event-icon"><Icon name="check" size={18} /></span><div><strong>{event.message}</strong><p>{event.detail}</p></div><time>{event.time}</time></div>{/each}</section>
        {:else if view === 'preferences'}
          <div class="page-heading"><div><div class="eyebrow">MAKE IT YOURS</div><h1>Preferences</h1><p>A familiar cockpit, arranged around the way you work.</p></div></div>
          <section class="panel preferences-panel"><div class="panel-heading"><h2>Appearance</h2></div><div class="preference-row"><div><strong>Color theme</strong><p>The green and slate palette from opensimgear.org.</p></div><button class="secondary-button" onclick={toggleTheme}><Icon name={light ? 'moon' : 'sun'} size={16} />{light ? 'Switch to dark' : 'Switch to light'}</button></div></section>
        {:else if view === 'gateways'}
          <div class="page-heading"><div><div class="eyebrow">YOUR HARDWARE, WORKING TOGETHER</div><h1>Gateways</h1><p>Your connected gateways and their devices.</p></div></div>
          <section class="gateway-list" aria-label="Gateways">
            {#if gatewayConnected}
              <section class="gateway-control" aria-label="Connected gateway">
                <div class="gateway-card-header">
                  <span class="gateway-icon"><Icon name="network" size={23} /></span>
                  <div class="gateway-copy">
                    <strong>{gatewayName} <span class="quiet-tag">{gatewayBusy ? 'Working…' : gateway?.running ? 'USB HID active' : physicalGateway ? 'No active controllers' : 'Waiting to start'}</span></strong>
                    <p>{gateway?.gatewaySerial} · {gateway?.port}</p>
                    <p class="automation-status" role="status">{automation.reason}</p>
                    {#if gatewayError}<p class="gateway-error" role="alert">{gatewayError}</p><button class="text-button" disabled={gatewayBusy} onclick={retryGateway}>Retry gateway</button>{/if}
                    {#if gateway?.hidNaming?.error}<p class="gateway-error" role="alert">Controller names could not be updated: {gateway.hidNaming.error}</p>{/if}
                  </div>
                </div>
                <div class="gateway-modules">
                  <div class="gateway-modules-heading"><h3>Devices <span>{devices.length}</span></h3>{#if physicalGateway}<span class="quiet-tag">{gateway?.modules.filter(module => module.registered).length} / {gateway?.maxModules ?? 10} controller slots</span>{:else}<button class="secondary-button" disabled={gatewayBusy || devices.length >= 8 || sessionId !== null || !!gateway?.claimedModules?.length} onclick={openAdd}><Icon name="plus" size={14} />Add device</button>{/if}</div>
                  <ul class="gateway-device-list">
                    {#each devices as device (device.id)}
                      {@const module = gateway?.modules.find(item => item.id === device.id)}
                      <li>
                        <button class="gateway-device-link" onclick={() => selectDevice(device.id)}><span class="status-dot" class:offline={!device.online}></span><span><strong>{device.settings.label}</strong><small>{device.inputs} · {module?.directUsb ? 'Direct USB' : device.online ? 'Online' : 'Offline'}{#if physicalGateway} · {module?.registered ? module.pendingProfileMatch === false ? 'Profile changed' : module.activeSlot === module.pendingSlot ? 'Registered' : 'Pending apply' : (module?.activeSlot ?? -1) >= 0 ? 'Removal pending' : 'Discovered'}{/if}</small></span><Icon name="chevron" size={13} /></button>
                        {#if physicalGateway && (!module?.registered || module.pendingProfileMatch === false)}<button class="secondary-button" disabled={gatewayBusy || !device.online || !!module?.directUsb || (!module?.registered && (gateway?.modules.filter(item => item.registered).length ?? 0) >= (gateway?.maxModules ?? 10))} onclick={() => registerDevice(device.id)}>{module?.registered ? 'Update profile' : 'Add controller'}</button>{/if}
                        {#if !physicalGateway || module?.registered}<button class="icon-button delete-device" aria-label={`Remove ${device.settings.label} registration`} title={`Remove ${device.settings.label} registration`} disabled={gatewayBusy || sessionId !== null || !!gateway?.claimedModules?.length} onclick={() => confirmRemove(device)}><Icon name="trash" size={15} /></button>{/if}
                      </li>
                    {:else}
                      <li class="gateway-devices-empty">{physicalGateway ? 'Connect a powered CAN device. It will appear here automatically.' : 'No devices registered. Add a simulated module to get started.'}</li>
                    {/each}
                  </ul>
                </div>
              </section>
              {#if needsReconnect}<div class="reconnect-notice"><Icon name="alert" size={19} /><div><strong>{physicalGateway ? 'Controller layout changes pending' : `${gatewayName} needs a reconnect`}</strong><p>Apply the pending controller layout to reconnect USB. Check your game bindings afterward.</p></div><button class="secondary-button" disabled={gatewayBusy || sessionId !== null} onclick={reconnectGateway}><Icon name="usb" size={15} />{physicalGateway ? 'Apply layout' : 'Reconnect gateway'}</button></div>{/if}

            {:else}
              <p class="gateway-discovery" role="status">Connect an OpenSimGear gateway to start.</p>
              {#if gatewayError && gatewayCandidates(gateway?.ports).length}<div class="notice amber-notice" role="alert"><p>{gatewayError}</p><button class="text-button" disabled={gatewayBusy} onclick={retryGateway}>Retry discovery</button></div>{/if}
            {/if}
          </section>
        {:else if view === 'devices'}
          <div class="page-heading"><div><div class="eyebrow">YOUR CONTROLLERS</div><h1>Devices</h1><p>Select a device to configure it and test its inputs.</p></div><button class="secondary-button scan-button" disabled={gatewayBusy} onclick={rescan}><Icon name="refresh" size={16} />Rescan devices</button></div>
                <div class="inventory-toolbar"><label class="search-field"><Icon name="search" size={16} /><input aria-label="Search devices" placeholder="Find a device…" bind:value={search} /></label><select aria-label="Filter devices" bind:value={filter}><option value="all">All devices</option><option value="online">Connected</option></select></div>
                <div class="device-grid">
                  {#each filtered as device (device.id)}
                    <button class="device-card" class:offline={!device.online} onclick={() => selectDevice(device.id)}>
                      <div class="card-top"><span class="connection-badge"><span class="status-dot" class:offline={!device.online}></span>{device.online ? 'Connected' : 'Offline'}</span><Icon name="arrow" size={15} /></div>
                      <div class="card-illustration"><DeviceIllustration kind={device.kind} compact /></div>
                      <div class="card-copy"><h3>{device.settings.label}</h3><p>{device.inputs}</p><div class="card-route"><Icon name={device.online ? 'network' : 'usb'} size={12} />{device.route}{#if !settingsEqual(drafts[device.id], device.settings)}<span class="draft-dot" title="Unsaved changes"></span>{/if}</div></div>
                    </button>
                  {:else}<div class="empty-state"><Icon name="search" size={26} /><strong>{devices.length ? 'No matching devices' : 'No devices yet'}</strong><p>{devices.length ? 'Try another name or show all devices.' : 'Plug in your gateway to discover devices automatically.'}</p><button class="text-button" onclick={() => { search = ''; filter = 'all'; }}>Clear filters</button></div>{/each}
                </div>
        {:else if view === 'topology'}
          <div class="page-heading"><div><div class="eyebrow">YOUR HARDWARE, WORKING TOGETHER</div><h1>Topology</h1><p>Follow your gateway’s connected devices.</p></div></div>
                <section class="panel rig-map"><div class="panel-heading"><div><h2>Your connection map</h2><p>Stable device identities. A single linear CAN chain.</p></div><span class="quiet-tag"><span class="status-dot" class:offline={!gatewayConnected}></span>{gatewayConnected ? 'Connected' : 'Last known'}</span></div><div class="map-host"><Icon name="monitor" size={28} /><strong>Your computer</strong><small>HID host</small></div><div class="usb-line"><span>USB HID + serial</span></div><div class="map-gateway"><span class="gateway-icon"><Icon name="network" size={23} /></span><div><strong>{gatewayName}</strong><small>{gateway?.gatewaySerial ?? 'OSG-GW-SIM'} · one {chainName}</small></div><span class="status-dot"></span></div><div class="can-spine"><span>{chainName}</span></div><div class="map-nodes">{#each devices.filter(device => device.online) as device (device.id)}<button class:active={selectedId === device.id} onclick={() => selectDevice(device.id)}><div class="node-art"><DeviceIllustration kind={device.kind} compact /></div><strong>{device.settings.label}</strong><small>{device.id}</small><span class="connection-badge"><span class="status-dot"></span>Connected</span></button>{/each}</div><div class="map-terminators"><span><i></i>Gateway endpoint terminated</span><span>Final module endpoint terminated<i></i></span></div>{#if devices.some(device => device.id === 'OSG-HB-0023' && !device.online)}<div class="map-direct"><Icon name="usb" size={19} /><div><strong>Reserved controller slot</strong><small>Handbrake · simulated module currently offline</small></div><button class="text-button" onclick={() => selectDevice('OSG-HB-0023')}>Inspect<Icon name="arrow" size={14} /></button></div>{/if}</section>
                <div class="map-footnote"><Icon name="info" size={16} /><p>Gateway sockets continue the same bus. Device identities stay consistent as connections change.</p></div>
        {:else if view === 'device' && selected}
          <button class="text-button device-back" onclick={() => { view = 'devices'; }}><Icon name="arrow" size={14} />Back to devices</button>
          <div class="page-heading"><div><div class="eyebrow">DEVICE WORKSPACE</div><h1>{selected.settings.label}</h1><p>Configuration and live controller inputs.</p></div></div>
          {#if needsReconnect}<div class="reconnect-notice"><Icon name="alert" size={19} /><div><strong>{physicalGateway ? 'Controller layout changes pending' : `${gatewayName} needs a reconnect`}</strong><p>Apply the pending controller layout to reconnect USB. Check your game bindings afterward.</p></div><button class="secondary-button" disabled={gatewayBusy || sessionId !== null} onclick={reconnectGateway}><Icon name="usb" size={15} />{physicalGateway ? 'Apply layout' : 'Reconnect gateway'}</button></div>{/if}

          {#if gatewayError}<div class="notice amber-notice" role="alert"><Icon name="alert" size={17} /><p>{gatewayError}</p></div>{/if}
          <div class="device-workspace">
            <div>
                <section class="panel device-stage"><div class="stage-title"><div><span class="eyebrow">DEVICE WORKSPACE</span><h2>{selected.name}</h2><p>{selected.subtitle}</p></div><span class="connection-badge"><span class="status-dot" class:offline={!selected.online}></span>{selected.online ? 'Connected' : 'Offline'}</span></div><div class="large-device"><DeviceIllustration kind={selected.kind} /></div><div class="stage-metadata"><div><small>STABLE IDENTITY</small><strong>{selected.id}</strong></div><div><small>ACTIVE ROUTE</small><strong>{selected.route}</strong></div><div><small>CAPABILITIES</small><strong>{selected.inputs}</strong></div></div></section>
                <section class="panel device-inputs"><div class="panel-heading"><h2>{physicalGateway ? 'Live inputs' : 'Input test'}</h2><span class="quiet-tag">{controlsReady ? 'Live HID output' : physicalGateway ? 'CAN device' : 'Simulator inputs'}</span></div>{@render inputTester()}</section>
            </div>
            <section class="panel inspector" aria-label="Selected device settings">
              <div class="inspector-heading"><div><span class="eyebrow">CONFIGURATION</span><h2>{selected.settings.label}</h2></div><span class="model-badge">{selected.kind === 'shifter' ? 'SQ' : selected.kind === 'pedals' ? 'PD' : selected.kind === 'panel' ? 'BP' : selected.kind === 'wheel' ? 'WH' : 'HB'}</span></div>
              <div class="inspector-tabs" aria-label="Device sections">{#each ['setup', 'details'] as item (item)}<button class:active={tab === item} aria-pressed={tab === item} onclick={() => { tab = item; }}>{item.charAt(0).toUpperCase() + item.slice(1)}</button>{/each}</div>
              <div class="inspector-body">
                {#if !selected.online}<div class="notice offline-notice"><Icon name="alert" size={17} /><div><strong>Device is offline</strong><p>Reconnect this module to configure it. Its saved identity and settings are preserved.</p></div></div>{/if}
                {#if physicalGateway && tab === 'setup'}
                  <div class="notice"><Icon name="info" size={17} /><p>Inputs and capabilities are discovered from the device. Calibration and device settings are currently built into its firmware.</p></div>
                  <dl class="device-details"><div><dt>Controller registration</dt><dd>{hidModule?.registered ? 'Registered' : 'Discovered'}</dd></div><div><dt>USB report ID</dt><dd>{hidModule?.reportId ?? 'Pending'}</dd></div><div><dt>Input owner</dt><dd>{hidModule?.directUsb ? 'Device USB' : 'Gateway'}</dd></div></dl>
                  {#if hidModule?.profileMatch === false && (hidModule?.activeSlot ?? -1) >= 0}<div class="notice amber-notice"><Icon name="alert" size={17} /><p>The device profile has changed. Update its registration and apply the controller layout to resume inputs.</p></div>{/if}
                {:else if tab === 'setup'}
                  <div class="session-notice" class:session-active={editing}><Icon name={editing ? 'lock' : 'usb'} size={17} /><div><strong>{editing ? 'Setup session active' : sessionId ? `Setup active: ${sessionDevice?.settings.label}` : !selected.online ? 'Reconnect to configure' : 'Ready to configure'}</strong><p>{editing ? 'Cockpit has configuration access.' : sessionId ? 'End that session before configuring this module.' : !selected.online ? 'Setup needs a connected module.' : 'Start a session to edit this device.'}</p></div></div>
                  <label class="field-label" for="device-name">Device name<span>Saved in simulator</span></label><input class="text-input" id="device-name" bind:value={draft.label} disabled={!editing} maxlength="80" />
                  <div class="setting-heading"><label for="debounce">Input debounce</label><span>{draft.debounce}<small> ms</small></span></div><input id="debounce" aria-label="Input debounce" type="range" min="1" max="50" step="1" bind:value={draft.debounce} disabled={!editing} /><div class="range-labels"><span>1 ms</span><span>50 ms</span></div><p class="field-help">Ignore brief contact noise for a clean input.</p>
                  {#if selected.kind !== 'panel'}<div class="toggle-row"><div><strong>Invert {selected.kind === 'shifter' ? 'shift direction' : 'input direction'}</strong><p>{selected.kind === 'shifter' ? 'Swap the upshift and downshift inputs.' : 'Reverse the reported input direction.'}</p></div><button class="toggle" class:on={draft.inverted} role="switch" aria-checked={draft.inverted} aria-label="Invert input direction" disabled={!editing} onclick={() => { draft.inverted = !draft.inverted; }}><span></span></button></div>{/if}
                  <div class="configuration-status"><span class="status-dot" class:draft-dot={dirty}></span>{dirty ? 'Unsaved changes' : 'Configuration up to date'}<span>{dirty ? 'Draft' : 'Saved'}</span></div>
                {:else}
                  <dl class="device-details"><div><dt>Stable identity</dt><dd>{selected.id}</dd></div><div><dt>Connection</dt><dd>{selected.route}</dd></div><div><dt>Firmware</dt><dd>{selected.firmware}</dd></div><div><dt>Capabilities</dt><dd>{selected.inputs}</dd></div><div><dt>Configuration owner</dt><dd>{editing ? 'Cockpit simulator' : 'None'}</dd></div></dl><div class="notice"><Icon name="info" size={17} /><p>{physicalGateway ? `The gateway serial is ${gateway?.gatewaySerial ?? ''}. This controller uses report ID ${hidModule?.reportId ?? 'pending'} on the shared HID interface. Registration is saved in gateway flash when the layout is applied.` : `USB interfaces share the Pico serial ${gateway?.gatewaySerial ?? ''}. This module maps to USB interface ${hidModule?.hidInterface ?? 'pending'}. Settings stay in RAM until the Pico resets.`}</p></div>
                {/if}
              </div>
              {#if !physicalGateway}<div class="inspector-footer">
                <button class={editing || sessionId ? 'secondary-button session-button' : 'primary-button session-button'} disabled={gatewayBusy || (!selected.online && !sessionId)} onclick={toggleSession}><Icon name={sessionId ? 'stop' : 'play'} size={15} />{sessionId && !editing ? `End ${sessionDevice?.settings.label} session` : sessionId ? 'End setup session' : 'Start setup session'}</button>
                {#if dirty}<div class="save-actions"><button class="text-button" onclick={discard}>Discard</button><button class="primary-button" disabled={gatewayBusy || !editing || !draft.label.trim()} onclick={applySettings}><Icon name="check" size={15} />Apply & save</button></div>{/if}
              </div>{/if}
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
        <span class="modal-icon"><Icon name="wheel" size={30} /></span><span class="eyebrow">WELCOME TO YOUR COCKPIT</span><h2 id="modal-title">Built around your hardware.</h2><p>Plug in your CAN gateway. Cockpit discovers connected devices automatically. Add their controller registrations and apply the layout to publish them over USB. The Pico simulator remains available for testing.</p><div class="notice"><Icon name="info" size={18} /><p>Offline devices retain their controller slots with neutral inputs. CAN gateway registrations persist in flash. Pico simulator settings and registration stay in RAM until reset.</p></div><a class="primary-button" href="/pico-flashing.html" target="_blank" rel="noreferrer" onclick={openLink}>Flashing instructions<Icon name="external" size={16} /></a>
      {:else if modal === 'remove'}
        <span class="modal-icon"><Icon name="close" size={30} /></span><span class="eyebrow">REMOVE REGISTRATION</span><h2 id="modal-title">Remove {removeTarget.label}?</h2><p>{physicalGateway ? `Remove the controller registration for ${removeTarget.id}. The device remains discoverable on CAN. Apply the layout to remove its published USB controller.` : `This removes ${removeTarget.id} from the simulator inventory and discards its settings. Reconnect the gateway to remove its USB HID interface.`}</p><div class="modal-actions"><button class="secondary-button" onclick={() => { modal = null; }}>Cancel</button><button class="primary-button" disabled={gatewayBusy} onclick={removeModule}>Remove module</button></div>
      {:else}
        <span class="modal-icon"><Icon name="plus" size={30} /></span><span class="eyebrow">SIMULATED CAN MODULE</span><h2 id="modal-title">Add a module</h2>

          <p>Register up to eight modules. New controllers appear after an explicit gateway reconnect.</p>
          <div class="module-form">
            <label for="module-kind">Module type</label><select id="module-kind" value={newModule.kind} onchange={event => chooseModuleKind(event.currentTarget.value as NewModule['kind'])}><option value="panel">Button panel</option><option value="shifter">Sequential shifter</option><option value="pedals">Pedals</option><option value="handbrake">Handbrake</option><option value="wheel">Steering wheel</option></select>
            <label for="module-id">Stable module identity</label><input id="module-id" class="text-input" bind:value={newModule.id} maxlength="40" /><p class="field-help">Use letters, numbers, underscores, or hyphens. Each identity must be unique.</p>
            <label for="module-name">Device name</label><input id="module-name" class="text-input" bind:value={newModule.name} maxlength="80" />
            <div class="module-counts"><label>Buttons<input class="text-input" aria-label="Module button count" type="number" min="0" max="32" step="1" bind:value={newModule.buttonCount} /></label><label>Axes<input class="text-input" aria-label="Module axis count" type="number" min="0" max="6" step="1" bind:value={newModule.axisCount} /></label></div>
          </div>

        <div class="notice amber-notice"><Icon name="alert" size={18} /><p>The reconnect changes the host controller layout. Check your game bindings afterward.</p></div><div class="modal-actions"><button class="secondary-button" onclick={() => { modal = null; }}>Cancel</button><button class="primary-button" disabled={gatewayBusy || (!newModuleValid)} onclick={addModule}>Register module</button></div>
      {/if}
    </dialog></div>
  {/if}
</div>

{#snippet inputTester()}
  <div class="input-tester">
    <div class="hid-state"><span class="status-dot" class:offline={!controlsReady}></span><span>{controlsReady ? `HID · ${selected.id}` : physicalGateway && hidModule?.directUsb ? 'Device USB owns these inputs' : physicalGateway && !hidModule?.registered ? 'Add this controller and apply the layout' : !gateway?.running ? physicalGateway ? 'Apply the layout to publish HID inputs' : 'Start the gateway to publish HID inputs' : !selected.online ? 'Module offline · HID slot retained' : physicalGateway && (hidModule?.activeSlot ?? -1) >= 0 && hidModule?.profileMatch ? 'Waiting for fresh device input' : 'Controller pending gateway reconnect'}</span></div>
    {#if buttonNames.length}
      <div class="input-buttons" class:panel-buttons={selected.kind === 'panel'}>{#each buttonNames as control, i (control)}<button class:pressed={held.includes(i + 1)} disabled={physicalGateway || !controlsReady || inputSending || gatewayBusy} onclick={() => toggleButton(i + 1)}><Icon name={control === 'Neutral' ? 'grid' : 'arrow'} size={19} /><span>{control}</span><small>{held.includes(i + 1) ? 'ON' : 'OFF'}</small></button>{/each}</div>
    {/if}
      {#each axisNames as name, i (name)}<label class="axis-test"><span>{name}<strong>{Math.round((axes[i] ?? 0) / 655.35)}%</strong></span><input aria-label={`${name} ${physicalGateway ? 'live' : 'simulated'} input`} type="range" min="0" max="100" value={Math.round((axes[i] ?? 0) / 655.35)} disabled={physicalGateway || !controlsReady || gatewayBusy} oninput={event => setAxis(i, Number(event.currentTarget.value))} /><span class="range-labels"><span>Released</span><span>Full travel</span></span></label>{/each}
    <p class="field-help">{physicalGateway ? 'Live device inputs are read-only. Inputs remain neutral until registration and layout are active.' : controlsReady ? 'Inputs publish to the host while this gateway is running. Setup sessions are independent.' : 'Simulated modules can be exercised once their HID controller is active.'}</p>
    {#if physicalGateway}<div class="device-details">{#each hidModule?.hats ?? [] as hat, i (i)}<p>Hat {i + 1}: {hat === 8 ? 'Neutral' : ['N', 'NE', 'E', 'SE', 'S', 'SW', 'W', 'NW'][hat]}</p>{/each}</div>{:else}<div class="module-presence"><span>{selected.online ? 'Module online' : 'Module offline'}</span><button class="text-button" disabled={gatewayBusy || !gatewayConnected} onclick={setPresence}>{selected.online ? 'Disconnect module' : 'Reconnect module'}</button></div>{/if}
  </div>
{/snippet}
