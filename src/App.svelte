<script lang="ts">
  import { onMount } from 'svelte';
  import { SvelteMap, SvelteSet } from 'svelte/reactivity';
  import Icon from './lib/Icon.svelte';
  import DeviceIllustration from './lib/DeviceIllustration.svelte';
  import { settingsEqual, type Device, type Settings } from './lib/devices';
  import { gatewayRequest, releaseOnExit, openCockpitLink, GatewayError, type GatewayRequest, type GatewayState, type NewModule } from './lib/gateway';

  type Layout = 'workbench' | 'focus' | 'map';
  type View = 'overview' | 'devices' | 'connections' | 'activity' | 'preferences';
  const layouts: { id: Layout; name: string; icon: string; description: string }[] = [
    { id: 'workbench', name: 'Workbench', icon: 'grid', description: 'A little of everything. Your devices and their settings, side by side.' },
    { id: 'focus', name: 'Device focus', icon: 'sliders', description: 'One device, all the details. More room for setup and input testing.' },
    { id: 'map', name: 'Rig map', icon: 'network', description: 'See how it connects. Follow a gateway’s chain and inspect each module.' },
  ];
  const navigation: { id: View; label: string; icon: string }[] = [
    { id: 'overview', label: 'Overview', icon: 'grid' },
    { id: 'devices', label: 'Devices', icon: 'devices' },
    { id: 'connections', label: 'Connections', icon: 'network' },
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
  const initialLayout = stored<Layout>('osg-layout', 'workbench');
  let layout = $state<Layout>(layouts.some(item => item.id === initialLayout) ? initialLayout : 'workbench');
  let light = $state(stored<boolean>('osg-light', false));
  let view = $state<View>('overview');
  let selectedPort = $state('');
  let devices = $state<Device[]>([]);
  let drafts = $state<Record<string, Settings>>({});
  let selectedId = $state('');
  let search = $state('');
  let filter = $state('all');
  let tab = $state('setup');
  let sessionId = $state<string | null>(null);
  let toast = $state('');
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
  let connectionEpoch = 0;
  const draftCache = new SvelteMap<string, Record<string, Settings>>();
  let calibrated = $state<Record<string, boolean>>({});
  let events = $state([{ time: 'Just now', message: 'Simulator workspace opened', detail: 'Connect a flashed Pico W, then start the gateway to test real USB HID inputs.' }]);
  const sessionDevice = $derived(devices.find(device => device.id === sessionId));
  const selected = $derived(devices.find(device => device.id === selectedId)!);
  const draft = $derived(drafts[selectedId]);
  const dirty = $derived(!!selected && !!draft && !settingsEqual(draft, selected.settings));
  const dirtyCount = $derived(devices.filter(device => !settingsEqual(drafts[device.id], device.settings)).length);
  const gatewayConnected = $derived(!!gateway?.connected);
  const editing = $derived(!!selected && sessionId === selectedId && selected.online && gatewayConnected);
  const filtered = $derived(devices.filter(device =>
    `${device.name} ${device.settings.label} ${device.id}`.toLowerCase().includes(search.toLowerCase()) && (filter !== 'online' || device.online)
  ));
  const activeLayout = $derived(view === 'connections' ? 'map' : layout);
  const layoutDescription = $derived(layouts.find(item => item.id === activeLayout)!.description);
  const hidModule = $derived(gateway?.modules.find(module => module.id === selectedId));
  const controlsReady = $derived(gatewayConnected && !!gateway?.running && !!hidModule?.active && !!selected?.online);
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
  function draftKey(state: GatewayState | null = gateway) { return `pico:${state?.gatewaySerial ?? 'local'}`; }
  function rememberDrafts() {
    if (devices.length) draftCache.set(draftKey(), Object.fromEntries(Object.entries(drafts).map(([id, settings]) => [id, { ...settings }])));
  }

  function syncGateway(state: GatewayState) {
    const connectionChanged = state.gatewaySerial && gateway?.gatewaySerial !== state.gatewaySerial;
    if (connectionChanged || (gateway?.connected && !state.connected) || state.reconnecting) {
      cancelInputs(); sessionId = null;
      if (connectionChanged) { rememberDrafts(); devices = []; drafts = { ...draftCache.get(draftKey(state)) }; calibrated = {}; }
    }
    const ids = new Set(state.modules.map(module => module.id));
    if (modal === 'remove' && !ids.has(removeTarget.id)) { modal = null; notify('That module has already been removed.'); }
    devices = devices.filter(device => ids.has(device.id));
    gateway = state;
    if (state.ports && !state.ports.some(port => port.path === selectedPort)) selectedPort = state.ports[0]?.path ?? '';
    needsReconnect = state.needsReconnect;
    for (const module of state.modules) {
      const device = devices.find(device => device.id === module.id);
      if (device) {
        const clean = settingsEqual(drafts[device.id], device.settings);
        device.online = module.online && !!state.connected;
        device.firmware = `Pico ${state.firmwareVersion ?? 'simulator'}`;
        device.settings = { ...module.settings };
        if (clean) drafts[device.id] = { ...module.settings };
      } else {
        devices.push({ id: module.id, name: module.name, kind: module.kind, subtitle: 'Your controls, within reach.', online: module.online && !!state.connected, route: 'Gateway 01 · simulated CAN', inputs: [module.buttonCount ? `${module.buttonCount} buttons` : '', module.axisCount ? `${module.axisCount} axes` : ''].filter(Boolean).join(' · '), firmware: `Pico ${state.firmwareVersion ?? 'simulator'}`, settings: { ...module.settings } });
        drafts[module.id] ??= { ...module.settings };
      }
      if (!module.online || (!inputSending && !axisTimers.has(module.id) && !pendingInputs.has(module.id))) inputValues[module.id] = { buttons: [...module.buttons], axes: [...module.axes] };
    }
    for (const id of Object.keys(drafts)) if (!ids.has(id)) { delete drafts[id]; delete inputValues[id]; delete calibrated[id]; clearTimeout(axisTimers.get(id)); axisTimers.delete(id); pendingInputs.delete(id); }
    if (!ids.has(selectedId)) selectedId = devices[0]?.id ?? '';
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
  async function toggleGateway() {
    const stopping = gateway?.running;
    if (stopping) cancelInputs();
    if (await sendGateway({ command: stopping ? 'stop' : 'start' })) {
      log(stopping ? 'Gateway stopped' : 'Gateway started', stopping ? 'HID inputs neutralized. USB controller interfaces remain connected.' : 'Module controllers are available to host HID applications.');
      notify(stopping ? 'Gateway stopped.' : 'HID input testing is ready.');
    }
  }
  async function connectPico() {
    cancelInputs();
    if (await sendGateway({ command: gateway?.connected ? 'disconnect' : 'connect', port: selectedPort })) {
      log(gateway?.connected ? 'Pico connected' : 'Pico disconnected', gateway?.connected ? `${gateway.gatewaySerial} · ${gateway.port}. Start the gateway to test inputs.` : 'Serial access released; HID inputs neutralized.');
      notify(gateway?.connected ? 'Pico connected. Start the gateway to test inputs.' : 'Pico disconnected.');
    }
  }
  async function setPresence() {
    const online = !selected.online;
    const id = selectedId;
    const name = selected.name;
    cancelInputs();
    if (await sendGateway({ command: 'presence', moduleId: id, online })) log(online ? 'Module reconnected' : 'Module disconnected', `${name} · its HID controller slot remains registered.`);
  }
  async function sendInput(id: string) {
    if (!gateway?.running) return;
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
  onMount(() => {
    void sendGateway({ command: 'status' }, true);
    const timer = setInterval(() => { if (!gatewayBusy && !inputSending) void sendGateway({ command: 'status' }, true); }, 3000);
    const release = () => { if (sessionId) releaseOnExit(sessionId); };
    window.addEventListener('pagehide', release);
    return () => { clearInterval(timer); for (const timer of axisTimers.values()) clearTimeout(timer); window.removeEventListener('pagehide', release); release(); };
  });

  function openLink(event: MouseEvent) {
    const opening = openCockpitLink(event);
    if (opening) void opening.catch(error => notify(error instanceof Error ? error.message : 'This link could not be opened.'));
  }

  function persist(key: string, value: unknown): boolean {
    try { localStorage.setItem(key, JSON.stringify(value)); return true; } catch { return false; }
  }
  function notify(message: string) { toast = message; }
  function log(message: string, detail: string) {
    events = [{ time: new Date().toLocaleTimeString([], { hour: '2-digit', minute: '2-digit' }), message, detail }, ...events];
  }
  function chooseLayout(next: Layout) {
    layout = next;
    view = 'overview';
    if (!persist('osg-layout', next)) notify('Layout changed for this session. Local storage is unavailable.');
  }
  function toggleTheme() { light = !light; if (!persist('osg-light', light)) notify('Theme changed for this session. Local storage is unavailable.'); }
  function selectDevice(id: string) { selectedId = id; tab = 'setup'; }
  async function toggleSession() {
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
  async function rescan() { if (await sendGateway({ command: 'status' })) { log('Discovery refreshed', `${devices.filter(device => device.online).length} simulated modules online.`); notify('Simulator inventory refreshed.'); } }
  async function addModule() {
    if (!newModuleValid || !await sendGateway({ command: 'add', module: { ...newModule, name: newModule.name.trim() } })) return;
    modal = null;
    selectDevice(newModule.id);
    log('Module added', 'Registration is pending. Reconnect the gateway to publish the updated USB HID layout.');
    notify('Module added. Reconnect the gateway to publish its HID controller.');
  }
  function openAdd() {
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
      log('Module removed', `${id} · reconnect the gateway to remove its published USB HID interface.`);
      notify(needsReconnect ? 'Module removed. Reconnect the gateway to update USB controllers.' : 'Pending module removed.');
    }
  }
  async function reconnectDemo() {
    cancelInputs(); sessionId = null;
    if (!await sendGateway({ command: 'reconnect' })) return;
    log('Gateway reconnected', 'HID controllers rebuilt with the registered layout. Check host controller bindings.');
    notify('USB layout updated. Start the gateway to test inputs.');
  }
</script>

<div class:light class="prototype">
  <div class="variant-bar">
    <div class="variant-label"><span class="prototype-dot"></span><strong>COCKPIT</strong><span class="divider"></span><span>Layout exploration</span></div>
    <div class="variant-switch" aria-label="Layout variants">
      {#each layouts as item, i (item.id)}
        <button class:active={layout === item.id} aria-pressed={layout === item.id} onclick={() => chooseLayout(item.id)}><Icon name={item.icon} size={15} /><span>{item.name}</span><small>0{i + 1}</small></button>
      {/each}
    </div>
    <span class="version-label">PROTOTYPE / 0.1</span>
  </div>

  <div class="app-shell">
    <aside class="sidebar">
      <a class="brand" href="https://www.opensimgear.org" target="_blank" rel="noreferrer" onclick={openLink}><img src={light ? '/logo-light.svg' : '/logo-dark.svg'} alt="" /><span><strong>Open</strong>SimGear<small>COCKPIT</small></span></a>
      <div class="rig-switcher"><span class="rig-avatar"><Icon name="wheel" size={21} /></span><div><strong>My racing rig</strong><small>Gateway simulator</small></div><span class="rig-chevron"><Icon name="chevron" size={14} /></span></div>
      <div class="nav-label">WORKSPACE</div>
      <nav aria-label="Main navigation">
        {#each navigation as item (item.id)}
          <button title={item.label} class:current={view === item.id} onclick={() => { view = item.id; }}><Icon name={item.icon} size={19} />{item.label}{#if item.id === 'devices'}<span class="nav-count">{devices.length}</span>{/if}</button>
        {/each}
      </nav>
      <div class="sidebar-bottom">
        <button class="sidebar-link" class:current={view === 'preferences'} onclick={() => { view = 'preferences'; }}><Icon name="settings" size={18} />Preferences</button>
        <a class="sidebar-link" href="https://www.opensimgear.org/docs/architecture/desktop-cockpit/" target="_blank" rel="noreferrer" onclick={openLink}><Icon name="help" size={18} />Documentation<Icon name="external" size={13} /></a>
        <div class="demo-note"><Icon name="info" size={17} /><div><strong>Virtual modules. Real HID.</strong><p>Connect a Pico W to test real USB controllers in your simulator.</p></div></div>
        <div class="sidebar-version"><span>Open hardware. Your control.</span><small>v0.1.0</small></div>
      </div>
    </aside>

    <div class="main-shell">
      <header class="topbar"><div class="breadcrumb">Workspace<Icon name="chevron" size={13} /><strong>{view.charAt(0).toUpperCase() + view.slice(1)}</strong></div><div class="topbar-actions"><span class="demo-pill">{gateway?.running ? 'USB HID active' : 'Simulator'}</span><button class="icon-button" aria-label={light ? 'Use dark theme' : 'Use light theme'} onclick={toggleTheme}><Icon name={light ? 'moon' : 'sun'} size={18} /></button><button class="icon-button" aria-label="About this prototype" onclick={() => { modal = 'help'; }}><Icon name="help" size={18} /></button></div></header>
      <main>
        {#if view === 'activity'}
          <div class="page-heading"><div><div class="eyebrow">THE RECORD OF YOUR RIG</div><h1>Activity</h1><p>Setup sessions, saved changes, and connection events.</p></div><span class="quiet-tag">{events.length} events</span></div>
          <section class="panel activity-panel"><div class="panel-heading"><h2>Recent activity</h2><span class="quiet-tag">This demo session</span></div>{#each events as event, i (i)}<div class="event-row"><span class="event-icon"><Icon name="check" size={18} /></span><div><strong>{event.message}</strong><p>{event.detail}</p></div><time>{event.time}</time></div>{/each}</section>
        {:else if view === 'preferences'}
          <div class="page-heading"><div><div class="eyebrow">MAKE IT YOURS</div><h1>Preferences</h1><p>A familiar cockpit, arranged around the way you work.</p></div></div>
          <section class="panel preferences-panel"><div class="panel-heading"><h2>Appearance</h2></div><div class="preference-row"><div><strong>Color theme</strong><p>The green and slate palette from opensimgear.org.</p></div><button class="secondary-button" onclick={toggleTheme}><Icon name={light ? 'moon' : 'sun'} size={16} />{light ? 'Switch to dark' : 'Switch to light'}</button></div><div class="preference-row"><div><strong>Default layout</strong><p>Your choice is remembered on this device.</p></div><select aria-label="Default layout" value={layout} onchange={event => { layout = event.currentTarget.value as Layout; persist('osg-layout', layout); }}>{#each layouts as item (item.id)}<option value={item.id}>{item.name}</option>{/each}</select></div></section>
        {:else}
          <div class="page-heading"><div><div class="eyebrow">YOUR HARDWARE, WORKING TOGETHER</div><h1>{view === 'connections' ? 'Everything connected.' : view === 'devices' ? 'Meet your devices.' : activeLayout === 'focus' ? 'Dial in the details.' : activeLayout === 'map' ? 'Everything connected.' : 'Your rig. In sync.'}</h1><p>{layoutDescription}</p></div><button class="secondary-button scan-button" disabled={gatewayBusy} onclick={rescan}><Icon name="refresh" size={16} />Rescan devices</button></div>
          <section class="gateway-control" aria-label="Gateway simulator">
            <span class="gateway-icon"><Icon name="network" size={23} /></span>
            <div class="gateway-copy">
              <strong>Pico W USB gateway <span class="quiet-tag">{gatewayBusy ? 'Working…' : gateway?.running ? `${gateway.modules.filter(module => module.active).length} active modules` : gatewayConnected ? 'Stopped' : 'Disconnected'}</span></strong>
              <p>{gateway?.connected ? `${gateway.gatewaySerial} · ${gateway.port}. Simulated CAN modules publish real USB HID reports.` : 'Flash the gateway firmware, connect your Pico W with a data cable, and choose its USB serial port.'}</p>
              <div class="gateway-connection">

                  <label>USB serial port<select aria-label="Pico USB serial port" bind:value={selectedPort} disabled={gatewayBusy || !!gateway?.connected}><option value="">{gateway?.ports?.length ? 'Choose a USB port' : 'No USB serial ports found'}</option>{#each gateway?.ports ?? [] as port (port.path)}<option value={port.path}>{port.path}{port.manufacturer ? ` · ${port.manufacturer}` : ''}</option>{/each}</select></label>
                  <button class="icon-button" aria-label="Refresh USB ports" disabled={gatewayBusy || !!gateway?.connected} onclick={() => sendGateway({ command: 'ports' })}><Icon name="refresh" size={16} /></button>

              </div>
              {#if gatewayError}<p class="gateway-error" role="alert">{gatewayError}</p>{/if}
              {#if gateway?.connected && gateway.hidNaming?.error}<p class="gateway-error" role="alert">Controller names could not be updated: {gateway.hidNaming.error}</p>{/if}
            </div>
            <div class="gateway-actions">
              <button class="secondary-button" disabled={gatewayBusy || sessionId !== null || (!gateway?.connected && !selectedPort)} onclick={connectPico}><Icon name="usb" size={15} />{gateway?.connected ? 'Disconnect Pico' : 'Connect Pico'}</button>
              <button class="primary-button" disabled={gatewayBusy || sessionId !== null || !gatewayConnected} onclick={toggleGateway}><Icon name={gateway?.running ? 'stop' : 'play'} size={15} />{gateway?.running ? 'Stop gateway' : 'Start gateway'}</button>
              <button class="secondary-button" disabled={gatewayBusy || !gatewayConnected || devices.length >= 8 || sessionId !== null} onclick={openAdd}><Icon name="plus" size={15} />Add device</button>
              <a class="text-button" href="/pico-flashing.html" target="_blank" rel="noreferrer" onclick={openLink}>Flashing instructions<Icon name="external" size={12} /></a>
            </div>
          </section>
          <div class="stat-grid">
            <div class="stat"><span class="stat-icon"><Icon name="devices" size={20} /></span><div><small>Simulated modules online</small><strong>{devices.filter(device => device.online).length}<span> / {devices.length}</span></strong></div><span class="status-dot"></span></div>
            <div class="stat"><span class="stat-icon"><Icon name="network" size={20} /></span><div><small>Gateway</small><strong>{gateway?.running ? '01' : '00'}<span> · USB</span></strong></div><span class="stat-tag">{gateway?.running ? 'Active' : 'Stopped'}</span></div>
            <div class="stat"><span class="stat-icon"><Icon name="sliders" size={20} /></span><div><small>Configuration</small><strong>{dirtyCount ? String(dirtyCount).padStart(2, '0') : 'All saved'}{#if dirtyCount}<span> draft{dirtyCount > 1 ? 's' : ''}</span>{/if}</strong></div></div>
            <div class="stat"><span class="stat-icon"><Icon name="usb" size={20} /></span><div><small>Setup session</small><strong>{sessionId ? 'Active' : 'Released'}</strong></div><span class="stat-tag" class:amber={sessionId !== null}>{sessionId ? 'Cockpit' : 'Available'}</span></div>
          </div>

          {#if needsReconnect}<div class="reconnect-notice"><Icon name="alert" size={19} /><div><strong>Gateway 01 needs a reconnect</strong><p>Module names and controller layout changes take effect on USB after reconnection. Check your simulator bindings afterward.</p></div><button class="secondary-button" disabled={gatewayBusy || sessionId !== null} onclick={reconnectDemo}><Icon name="usb" size={15} />Reconnect gateway</button></div>{/if}

          {#if selected}
          <div class="workspace" class:focus-layout={activeLayout === 'focus'} class:map-layout={activeLayout === 'map'}>
            <div class="workspace-primary">
              {#if activeLayout === 'workbench'}
                <div class="section-heading"><div><h2>Your devices <span>{devices.length}</span></h2><p>A place for every part of your rig.</p></div></div>
                <div class="inventory-toolbar"><label class="search-field"><Icon name="search" size={16} /><input aria-label="Search devices" placeholder="Find a device…" bind:value={search} /></label><select aria-label="Filter devices" bind:value={filter}><option value="all">All devices</option><option value="online">Connected</option></select></div>
                <div class="device-grid">
                  {#each filtered as device (device.id)}
                    <button class="device-card" class:selected={selectedId === device.id} class:offline={!device.online} onclick={() => selectDevice(device.id)} aria-pressed={selectedId === device.id}>
                      <div class="card-top"><span class="connection-badge"><span class="status-dot" class:offline={!device.online}></span>{device.online ? 'Connected' : 'Offline'}</span>{#if selectedId === device.id}<span class="selected-check"><Icon name="check" size={13} /></span>{:else}<Icon name="arrow" size={15} />{/if}</div>
                      <div class="card-illustration"><DeviceIllustration kind={device.kind} compact /></div>
                      <div class="card-copy"><h3>{device.settings.label}</h3><p>{device.inputs}</p><div class="card-route"><Icon name={device.online ? 'network' : 'usb'} size={12} />{device.route}{#if !settingsEqual(drafts[device.id], device.settings)}<span class="draft-dot" title="Unsaved changes"></span>{/if}</div></div>
                    </button>
                  {:else}<div class="empty-state"><Icon name="search" size={26} /><strong>No matching devices</strong><p>Try another name or show all devices.</p><button class="text-button" onclick={() => { search = ''; filter = 'all'; }}>Clear filters</button></div>{/each}
                </div>
                <div class="connection-summary"><span class="gateway-icon"><Icon name="network" size={23} /></span><div><strong>Gateway 01</strong><p>One virtual gateway. Stable controller identities.</p></div><button aria-label="View gateway connections" class="icon-button" onclick={() => { view = 'connections'; }}><Icon name="arrow" size={19} /></button><span class="status-dot"></span></div>
              {:else if activeLayout === 'focus'}
                <div class="device-rail" aria-label="Select a device">{#each devices as device (device.id)}<button class:active={selectedId === device.id} onclick={() => selectDevice(device.id)}><span class="status-dot" class:offline={!device.online}></span>{device.settings.label}</button>{/each}</div>
                <section class="panel focus-stage"><div class="stage-title"><div><span class="eyebrow">DEVICE WORKSPACE</span><h2>{selected.name}</h2><p>{selected.subtitle}</p></div><span class="connection-badge"><span class="status-dot" class:offline={!selected.online}></span>{selected.online ? 'Connected' : 'Offline'}</span></div><div class="large-device"><DeviceIllustration kind={selected.kind} /></div><div class="stage-metadata"><div><small>STABLE IDENTITY</small><strong>{selected.id}</strong></div><div><small>ACTIVE ROUTE</small><strong>{selected.route}</strong></div><div><small>CAPABILITIES</small><strong>{selected.inputs}</strong></div></div></section>
                <section class="panel focus-inputs"><div class="panel-heading"><h2>Input test</h2><span class="quiet-tag">{controlsReady ? 'Live HID output' : 'Simulator inputs'}</span></div>{@render inputTester()}</section>
              {:else}
                <section class="panel rig-map"><div class="panel-heading"><div><h2>Your connection map</h2><p>Stable device identities. A single linear CAN chain.</p></div><span class="quiet-tag"><span class="status-dot" class:offline={!gatewayConnected}></span>{gatewayConnected ? 'Connected' : 'Last known'}</span></div><div class="map-host"><Icon name="monitor" size={28} /><strong>Your computer</strong><small>HID host</small></div><div class="usb-line"><span>USB HID + serial</span></div><div class="map-gateway"><span class="gateway-icon"><Icon name="network" size={23} /></span><div><strong>Gateway 01</strong><small>{gateway?.gatewaySerial ?? 'OSG-GW-SIM'} · one simulated CAN chain</small></div><span class="status-dot"></span></div><div class="can-spine"><span>Simulated CAN chain</span></div><div class="map-nodes">{#each devices.filter(device => device.online) as device (device.id)}<button class:active={selectedId === device.id} onclick={() => selectDevice(device.id)}><div class="node-art"><DeviceIllustration kind={device.kind} compact /></div><strong>{device.settings.label}</strong><small>{device.id}</small><span class="connection-badge"><span class="status-dot"></span>Connected</span></button>{/each}</div><div class="map-terminators"><span><i></i>Gateway endpoint terminated</span><span>Final module endpoint terminated<i></i></span></div>{#if devices.some(device => device.id === 'OSG-HB-0023' && !device.online)}<div class="map-direct"><Icon name="usb" size={19} /><div><strong>Reserved controller slot</strong><small>Handbrake · simulated module currently offline</small></div><button class="text-button" onclick={() => selectDevice('OSG-HB-0023')}>Inspect<Icon name="arrow" size={14} /></button></div>{/if}</section>
                <div class="map-footnote"><Icon name="info" size={16} /><p>Gateway sockets continue the same bus. Device identities stay consistent as connections change.</p></div>
              {/if}
            </div>

            <section class="panel inspector" aria-label="Selected device settings">
              <div class="inspector-heading"><div><span class="eyebrow">{activeLayout === 'focus' ? 'CONFIGURATION' : 'SELECTED DEVICE'}</span><h2>{selected.settings.label}</h2></div><span class="model-badge">{selected.kind === 'shifter' ? 'SQ' : selected.kind === 'pedals' ? 'PD' : selected.kind === 'panel' ? 'BP' : selected.kind === 'wheel' ? 'WH' : 'HB'}</span></div>
              {#if activeLayout !== 'focus'}<div class="inspector-art"><DeviceIllustration kind={selected.kind} compact /><span class="art-id">{selected.id}</span></div>{/if}
              <div class="inspector-tabs" aria-label="Device sections">{#each ['setup', 'inputs', 'details'] as item (item)}<button class:active={tab === item} aria-pressed={tab === item} onclick={() => { tab = item; }}>{item.charAt(0).toUpperCase() + item.slice(1)}</button>{/each}</div>
              <div class="inspector-body">
                {#if !selected.online}<div class="notice offline-notice"><Icon name="alert" size={17} /><div><strong>Device is offline</strong><p>Reconnect this module to configure it. Its saved identity and settings are preserved.</p></div></div>{/if}
                {#if tab === 'setup'}
                  <div class="session-notice" class:session-active={editing}><Icon name={editing ? 'lock' : 'usb'} size={17} /><div><strong>{editing ? 'Setup session active' : sessionId ? `Setup active: ${sessionDevice?.settings.label}` : !selected.online ? 'Reconnect to configure' : 'Ready to configure'}</strong><p>{editing ? 'Cockpit has configuration access.' : sessionId ? 'End that session before configuring this module.' : !selected.online ? 'Setup needs a connected module.' : 'Start a session to edit this device.'}</p></div></div>
                  <label class="field-label" for="device-name">Device name<span>Saved in simulator</span></label><input class="text-input" id="device-name" bind:value={draft.label} disabled={!editing} maxlength="80" />
                  <div class="setting-heading"><label for="debounce">Input debounce</label><span>{draft.debounce}<small> ms</small></span></div><input id="debounce" aria-label="Input debounce" type="range" min="1" max="50" step="1" bind:value={draft.debounce} disabled={!editing} /><div class="range-labels"><span>1 ms</span><span>50 ms</span></div><p class="field-help">Ignore brief contact noise for a clean input.</p>
                  {#if selected.kind !== 'panel'}<div class="toggle-row"><div><strong>Invert {selected.kind === 'shifter' ? 'shift direction' : 'input direction'}</strong><p>{selected.kind === 'shifter' ? 'Swap the upshift and downshift inputs.' : 'Reverse the reported input direction.'}</p></div><button class="toggle" class:on={draft.inverted} role="switch" aria-checked={draft.inverted} aria-label="Invert input direction" disabled={!editing} onclick={() => { draft.inverted = !draft.inverted; }}><span></span></button></div>{/if}
                  {#if selected.kind === 'pedals' || selected.kind === 'handbrake'}<div class="calibration-row"><div><strong>Axis calibration</strong><p>{calibrated[selected.id] ? 'Demo range captured in this session.' : 'Capture a fresh range for this hardware.'}</p></div><button class="text-button" disabled={!editing} onclick={() => { calibrated[selected.id] = true; log('Demo calibration captured', `${selected.name} · simulated axis range only.`); notify('Simulated axis range captured. Real calibration requires connected hardware.'); }}>{calibrated[selected.id] ? 'Recapture' : 'Calibrate'}</button></div>{/if}
                  <div class="configuration-status"><span class="status-dot" class:draft-dot={dirty}></span>{dirty ? 'Unsaved changes' : 'Configuration up to date'}<span>{dirty ? 'Draft' : 'Saved'}</span></div>
                {:else if tab === 'inputs'}
                  <div class="input-heading"><h3>Input monitor</h3><p>Exercise module controls and send HID reports to macOS.</p></div>{@render inputTester()}
                {:else}
                  <dl class="device-details"><div><dt>Stable identity</dt><dd>{selected.id}</dd></div><div><dt>Connection</dt><dd>{selected.route}</dd></div><div><dt>Firmware</dt><dd>{selected.firmware}</dd></div><div><dt>Capabilities</dt><dd>{selected.inputs}</dd></div><div><dt>Configuration owner</dt><dd>{editing ? 'Cockpit simulator' : 'None'}</dd></div></dl><div class="notice"><Icon name="info" size={17} /><p>{`USB interfaces share the Pico serial ${gateway?.gatewaySerial ?? ''}. This module maps to USB interface ${hidModule?.hidInterface ?? 'pending'}. Settings stay in RAM until the Pico resets.`}</p></div>
                {/if}
              </div>
              <div class="inspector-footer">
                <button class={editing || sessionId ? 'secondary-button session-button' : 'primary-button session-button'} disabled={gatewayBusy || (!selected.online && !sessionId)} onclick={toggleSession}><Icon name={sessionId ? 'stop' : 'play'} size={15} />{sessionId && !editing ? `End ${sessionDevice?.settings.label} session` : sessionId ? 'End setup session' : 'Start setup session'}</button>
                {#if dirty}<div class="save-actions"><button class="text-button" onclick={discard}>Discard</button><button class="primary-button" disabled={gatewayBusy || !editing || !draft.label.trim()} onclick={applySettings}><Icon name="check" size={15} />Apply & save</button></div>{/if}
                <button class="text-button remove-module" disabled={gatewayBusy || !gatewayConnected || sessionId !== null} onclick={() => { removeTarget = { id: selected.id, label: selected.settings.label }; modal = 'remove'; }}><Icon name="close" size={14} />Remove module</button>
                <p><Icon name="info" size={12} />HID input tests run independently of setup sessions.</p>
              </div>
            </section>
          </div>
          {:else}
            <section class="panel empty-gateway"><Icon name="usb" size={32} /><h2>{gatewayConnected ? 'No modules registered' : 'Connect your gateway'}</h2><p>{gatewayConnected ? 'Use Add device on the Pico gateway card, reconnect the gateway, and start testing.' : 'Connect a flashed Pico W to discover its modules. The USB serial port opens only when you connect.'}</p></section>
          {/if}
          <div class="layout-caption"><span>LAYOUT {activeLayout === 'workbench' ? '01' : activeLayout === 'focus' ? '02' : '03'}</span><p>{activeLayout === 'workbench' ? 'Workbench · an overview that keeps configuration within reach.' : activeLayout === 'focus' ? 'Device focus · a spacious workspace for one module at a time.' : 'Rig map · connection context for setup and troubleshooting.'}</p></div>
        {/if}
      </main>
      <footer class="statusbar"><span><span class="status-dot"></span>{sessionId ? `Simulator setup: ${sessionDevice?.settings.label}` : 'Setup access released'}</span><span class="statusbar-right">{gateway?.running ? 'USB HID active' : gatewayConnected ? 'Gateway stopped' : 'Gateway disconnected'}<span class="divider"></span>Simulated CAN modules<span class="divider"></span><Icon name="check" size={12} />Local prototype</span></footer>
    </div>
  </div>

  {#if toast}<div class="toast" role="status"><span class="toast-icon"><Icon name="check" size={16} /></span><p>{toast}</p><button class="icon-button" aria-label="Dismiss notification" onclick={() => { toast = ''; }}><Icon name="close" size={15} /></button></div>{/if}
  {#if modal}
    <div class="modal-backdrop"><dialog class="modal" aria-labelledby="modal-title" {@attach showDialog} onclose={() => { modal = null; }}>
      <button class="modal-close icon-button" aria-label="Close dialog" onclick={() => { modal = null; }}><Icon name="close" /></button>
      {#if modal === 'help'}
        <span class="modal-icon"><Icon name="wheel" size={30} /></span><span class="eyebrow">WELCOME TO YOUR COCKPIT</span><h2 id="modal-title">Built around your hardware.</h2><p>Connect your flashed Pico W, start the gateway, and test its real USB HID controllers with simulated module inputs.</p><div class="notice"><Icon name="info" size={18} /><p>Add or remove modules, then explicitly reconnect to update the USB layout. Offline modules retain their controller slots with neutral inputs. Settings and registration stay in Pico RAM until reset. CAN and axis calibration are simulated.</p></div><a class="primary-button" href="/pico-flashing.html" target="_blank" rel="noreferrer" onclick={openLink}>Flashing instructions<Icon name="external" size={16} /></a>
      {:else if modal === 'remove'}
        <span class="modal-icon"><Icon name="close" size={30} /></span><span class="eyebrow">REMOVE REGISTRATION</span><h2 id="modal-title">Remove {removeTarget.label}?</h2><p>This removes {removeTarget.id} from the gateway inventory and discards its settings and unsaved draft. Published inputs are neutralized immediately. Reconnect the gateway to remove its USB HID interface.</p><div class="modal-actions"><button class="secondary-button" onclick={() => { modal = null; }}>Cancel</button><button class="primary-button" disabled={gatewayBusy} onclick={removeModule}>Remove module</button></div>
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
    <div class="hid-state"><span class="status-dot" class:offline={!controlsReady}></span><span>{controlsReady ? `HID · ${selected.id}` : !gateway?.running ? 'Start the gateway to publish HID inputs' : !selected.online ? 'Module offline · HID slot retained' : 'Controller pending gateway reconnect'}</span></div>
    {#if buttonNames.length}
      <div class="input-buttons" class:panel-buttons={selected.kind === 'panel'}>{#each buttonNames as control, i (control)}<button class:pressed={held.includes(i + 1)} disabled={!controlsReady || inputSending || gatewayBusy} onclick={() => toggleButton(i + 1)}><Icon name={control === 'Neutral' ? 'grid' : 'arrow'} size={19} /><span>{control}</span><small>{held.includes(i + 1) ? 'ON' : 'OFF'}</small></button>{/each}</div>
    {/if}
      {#each axisNames as name, i (name)}<label class="axis-test"><span>{name}<strong>{Math.round((axes[i] ?? 0) / 655.35)}%</strong></span><input aria-label={`${name} simulated input`} type="range" min="0" max="100" value={Math.round((axes[i] ?? 0) / 655.35)} disabled={!controlsReady || gatewayBusy} oninput={event => setAxis(i, Number(event.currentTarget.value))} /><span class="range-labels"><span>Released</span><span>Full travel</span></span></label>{/each}
    <p class="field-help">{controlsReady ? 'Inputs publish to the host while this gateway is running. Setup sessions are independent.' : 'Simulated modules can be exercised once their HID controller is active.'}</p>
    <div class="module-presence"><span>{selected.online ? 'Module online' : 'Module offline'}</span><button class="text-button" disabled={gatewayBusy || !gatewayConnected} onclick={setPresence}>{selected.online ? 'Disconnect module' : 'Reconnect module'}</button></div>
  </div>
{/snippet}
