<script lang="ts">
  import { onMount, tick, type Snippet } from 'svelte';
  import Icon from './Icon.svelte';
  import Notification from './Notification.svelte';
  import SystemTopology from './SystemTopology.svelte';
  import HostControllers from './HostControllers.svelte';
  import SystemUsbDetails from './SystemUsbDetails.svelte';
  import { gatewayRequest } from './gateway';
  import type { GatewayEntry } from './gateway-workspace';
  import type { GameId } from './games';
  import { SystemInventoryController, integrationGameId } from './system-inventory-providers';
  import { filterSystem, isVisibleSystemComponent, systemStateLabel, systemTypeLabels, type SystemComponent, type SystemSnapshot, type InventoryStorage } from './system-inventory';
  let { active, entries, gatewayError = '', openModule, openCompanions, openIntegration, gatewayDetails, oninventory }: {
    active: boolean; entries: GatewayEntry[]; openModule: (gatewayId: string, moduleId: string) => void;
    gatewayError?: string;
    openCompanions: () => void; openIntegration: (gameId: GameId) => void;
    gatewayDetails: Snippet<[string]>; oninventory: (snapshot: SystemSnapshot) => void;
  } = $props();
  let snapshot = $state.raw<SystemSnapshot>({ components: [], diagnostics: [], observedAt: 0 });
  let mode = $state('list'), search = $state(''), filter = $state('all'), transport = $state('all'), selectedId = $state('');
  let detailsOpen = $state(false);
  let inventoryElement: HTMLDivElement | undefined;
  let busy = $state(false), error = $state(''), confirmation = $state<SystemComponent[]>([]);
  let refreshTask: Promise<SystemSnapshot> | undefined;
  let controller: SystemInventoryController;
  let reviewEpoch = 0;
  const visible = $derived(filterSystem(snapshot.components, search, filter, transport));
  const components = $derived(snapshot.components.filter(isVisibleSystemComponent));
  const selected = $derived(components.find(c => c.id === selectedId));
  const showingDetails = $derived(detailsOpen && !!selected && active);
  const eligible = $derived(visible.filter(c => c.forget.allowed));
  const icon = (c: SystemComponent) => c.type === 'gateway' ? 'network' : c.type === 'integration' ? 'play' : c.type === 'virtual' ? 'monitor' : 'devices';
  const date = (timestamp: number) => timestamp ? new Date(timestamp).toLocaleString() : 'Never observed connected';
  function select(id: string) { selectedId = id; detailsOpen = true; }
  async function backToDevices() {
    detailsOpen = false;
    await tick();
    const target = mode === 'topology' ? '.topology-viewport' : `[data-component-id="${CSS.escape(selectedId)}"]`;
    inventoryElement?.querySelector<HTMLElement>(target)?.focus();
  }
  function focusDetails(node: HTMLButtonElement) { node.focus(); }
  async function refresh(quiet = false) {
    if (!controller || busy || (quiet && refreshTask)) return;
    if (!quiet) busy = true;
    const task = refreshTask ?? controller.refresh();
    refreshTask = task;
    try { await task; if (!quiet) error = ''; }
    catch (e) { error = String(e); }
    finally {
      if (refreshTask === task) refreshTask = undefined;
      if (!quiet) busy = false;
    }
  }
  function setMode(value: string) { mode = value; try { localStorage.setItem('osg-devices-mode', value); } catch { /* mode remains usable */ } }
  async function reviewForget(items: SystemComponent[]) {
    if (busy) return;
    const epoch = ++reviewEpoch;
    busy = true; confirmation = items;
    try {
      await refreshTask?.catch(() => undefined);
      if (epoch !== reviewEpoch) return;
      const reviewed = await controller.reviewForget(items.map(c => c.id));
      if (epoch === reviewEpoch) confirmation = reviewed;
    }
    finally { busy = false; }
  }
  async function forget() {
    if (busy) return;
    busy = true; const failures: string[] = [];
    await refreshTask?.catch(() => undefined);
    for (const c of confirmation) {
      try { await controller.forget(c.id); if (selectedId === c.id) { selectedId = ''; detailsOpen = false; } }
      catch (e) { failures.push(`${c.name}: ${String(e)}`); }
    }
    confirmation = []; error = failures.join('\n'); busy = false;
    await refresh();
    if (failures.length) error = failures.join('\n');
  }
  function dialog(node: HTMLDialogElement) { node.showModal(); return { destroy: () => node.close() }; }
  onMount(() => {
    let storage: InventoryStorage;
    try { storage = localStorage; mode = storage.getItem('osg-devices-mode') === 'topology' ? 'topology' : 'list'; }
    catch { storage = { getItem: () => { throw Error('Local inventory storage unavailable.'); }, setItem: () => { throw Error('Local inventory storage unavailable.'); } }; }
    controller = new SystemInventoryController({ request: gatewayRequest, gateways: () => entries, gatewayError: () => gatewayError, storage, onChange: next => { snapshot = next; oninventory(next); } });
    snapshot = controller.snapshot;
    void refresh();
    const timer = setInterval(() => { if (active && !document.hidden) void refresh(true); }, 3000);
    return () => { reviewEpoch++; clearInterval(timer); controller.dispose(); };
  });
</script>

<section aria-label="System devices">
{#if error}<Notification message={error} kind="error" />{/if}
<div class="system-inventory" bind:this={inventoryElement} hidden={showingDetails}>
<div class="page-heading"><div><div class="eyebrow">YOUR CONNECTED SYSTEM</div><h1>Devices</h1><p>Every known component, its current state, and how it connects.</p></div></div>
<div class="system-toolbar">
  <div class="mode-switch" role="group" aria-label="Devices presentation"><button class:chosen={mode === 'list'} aria-pressed={mode === 'list'} onclick={() => setMode('list')}>List</button><button class:chosen={mode === 'topology'} aria-pressed={mode === 'topology'} onclick={() => setMode('topology')}>Topology</button></div>
  <label class="search-field"><Icon name="search" size={16} /><input aria-label="Search components" placeholder="Find a component…" bind:value={search} /></label>
  <select aria-label="Filter components" bind:value={filter}><option value="all">All components</option><option value="connected">Connected</option><option value="offline">Offline</option><option value="unknown">Unknown</option><option value="unavailable">Unavailable</option><option value="hidden">Hidden</option>{#each Object.entries(systemTypeLabels).filter(([type]) => type !== 'host') as [type,label]}<option value={type}>{label}</option>{/each}</select>
  <select aria-label="Filter connections" bind:value={transport}><option value="all">All connections</option><option value="usb">USB</option><option value="can">CAN</option><option value="network">Network</option><option value="bluetooth">Bluetooth</option><option value="software">Software</option><option value="unknown">Unknown connection</option></select>
  <span aria-live="polite">{visible.length} / {components.length} components</span>
  <button class="text-button" disabled={busy || !eligible.length} onclick={() => reviewForget(eligible)}>Forget eligible ({eligible.length})</button>
</div>
<div class="type-counts" aria-label="Component counts">{#each Object.entries(systemTypeLabels).filter(([type]) => type !== 'host') as [type,label]}<span>{label}: {components.filter(c => c.type === type).length}</span>{/each}</div>
{#if snapshot.diagnostics.length}<details class="provider-diagnostics"><summary>Inventory diagnostics ({snapshot.diagnostics.length})</summary>{#each snapshot.diagnostics as diagnostic}<p>{diagnostic}</p>{/each}</details>{/if}
{#if mode === 'topology'}
  <SystemTopology components={visible} context={snapshot.components} {selectedId} onselect={select} />
{:else}
  <div class="system-list" aria-label="System components">
    {#each visible as c (c.id)}
      <button class="system-card component-tile" data-component-id={c.id} class:chosen={selectedId === c.id} class:offline={c.presence === 'offline'} aria-pressed={selectedId === c.id} onclick={() => select(c.id)}>
        <div class="component-heading"><span class="component-icon"><Icon name={icon(c)} size={24} /></span><div class="component-copy"><strong>{c.name}</strong><span>{systemTypeLabels[c.type]}{c.role ? ` · ${c.role}` : ''}</span></div><Icon name="chevron" size={14} /></div>
        <div class="component-status">{systemStateLabel(c)}</div>
        <div class="component-transports">{#each [...new Set(c.connections.map(x => x.transport))] as transport}<span class="quiet-tag">{transport === 'software' ? 'Software' : transport === 'unknown' ? 'Connection unknown' : transport.toUpperCase()}{#if c.connections.some(connection => connection.transport === transport && connection.observed === false) && !c.connections.some(connection => connection.transport === transport && connection.current)} · Configured{/if}</span>{:else}<span class="quiet-tag">Connection unknown</span>{/each}</div>
      </button>
    {:else}<div class="empty-state"><strong>{components.length ? 'No matching components' : busy ? 'Loading known components…' : 'No known components'}</strong><p>Connected, offline and unavailable components appear here when their providers report them.</p><button class="text-button" onclick={() => { search = ''; filter = 'all'; transport = 'all'; }}>Clear filters</button></div>{/each}
  </div>
{/if}
</div>
{#if selected && showingDetails}
  <div class="details-toolbar"><button class="text-button" use:focusDetails onclick={backToDevices}><Icon name="back" size={16} />Back to Devices</button></div>
  <section class="panel component-details" aria-label="Component details">
    <div class="panel-heading"><div><h1>{selected.name}</h1><p>{systemTypeLabels[selected.type]} · {selected.role}</p></div></div>
    <p>{systemStateLabel(selected)}</p>
    <p>Capture: {selected.capture}{selected.owner ? ` · Owner: ${selected.owner}` : ''}</p>
    {#if selected.type === 'osg'}<p>Input owner: {selected.inputOwner ?? 'None confirmed'} · Configuration writer: {selected.configurationOwner ?? 'None observed'} · Active configuration route: {selected.configurationRoute ?? 'None observed'}</p>{/if}
    <p class="field-help">First seen: {date(selected.firstSeen)} · Last seen connected: {date(selected.lastSeen)}{selected.stale ? ' · Refresh needed' : ''}</p>
    <p class="field-help">Observed by {selected.provider} · {date(selected.observedAt)}{selected.stale ? ' · Observation stale' : ''}</p>
    {#each selected.diagnostics as diagnostic}<p>{diagnostic}</p>{/each}
    <ul>{#each selected.connections as edge}<li>{edge.transport.toUpperCase()} · {snapshot.components.find(c => c.id === edge.from)?.name ?? edge.from}{edge.endpoint ? ` · ${edge.endpoint}` : ''} · {edge.current ? 'Current connection' : edge.observed === false ? 'Configured connection (unconfirmed)' : 'Last-known connection'}</li>{:else}<li>Connection unknown</li>{/each}</ul>
    <div class="component-actions">
      {#each selected.targets.filter(t => t.kind === 'module') as t}<button class="primary-button" onclick={() => openModule(t.gatewayId!, t.id)}>Configure device</button>{/each}
      {#if selected.type === 'companion'}<button class="secondary-button" onclick={openCompanions}>Manage companion</button>{/if}
      {#if integrationGameId(selected)}<button class="secondary-button" onclick={() => openIntegration(integrationGameId(selected)!)}>Integration settings</button>{/if}
      <button class="secondary-button" disabled={busy || !selected.forget.allowed} title={selected.forget.reason} onclick={() => reviewForget([selected])}>Forget device</button>
    </div>
    {#if !selected.forget.allowed}<p class="field-help">{selected.forget.reason}</p>{/if}
    {#each selected.targets.filter(t => t.kind === 'gateway') as t}{@render gatewayDetails(t.id)}{/each}
    {#each selected.targets.filter(t => t.kind === 'controller') as t (t.id)}{#key t.id}<HostControllers initialSourceId={t.id} detailsOnly />{/key}{/each}
    {#each selected.targets.filter(t => t.kind === 'usb') as t (t.id)}{#key t.id}<SystemUsbDetails endpointKey={t.id} />{/key}{/each}
    <details><summary>Identity and evidence</summary><code>{selected.id}</code>{#each selected.evidence as evidence}<p>{evidence}</p>{/each}</details>
  </section>
{/if}
{#if confirmation.length}
  <dialog use:dialog oncancel={() => { reviewEpoch++; confirmation = []; }} aria-labelledby="forget-title">
    <h2 id="forget-title">Forget {confirmation.length === 1 ? confirmation[0].name : `${confirmation.length} components`}?</h2>
    {#each confirmation as c}<div><h3>{c.name}</h3><p>{c.forget.effect}</p>{#each c.diagnostics.filter(d => d.startsWith('Affected stored references')) as diagnostic}<p>{diagnostic}</p>{/each}{#if c.references.length}<p>Affected references (retained as unresolved): {c.references.map(r => r.name).join(', ')}</p>{/if}</div>{/each}
    <div class="component-actions"><button class="secondary-button" disabled={busy} onclick={() => { reviewEpoch++; confirmation = []; }}>Cancel</button><button class="primary-button" disabled={busy} onclick={forget}>Confirm forget</button></div>
  </dialog>
{/if}

</section>

<style>
  .system-inventory[hidden] {display:none}
  .details-toolbar {display:flex;align-items:center;justify-content:space-between;gap:16px;margin-bottom:18px}
  .system-toolbar,.component-actions { display:flex;align-items:center;flex-wrap:wrap;gap:12px;margin-bottom:18px; }
  .system-toolbar > span {font-size:12px;opacity:.75}.mode-switch {display:flex;border:1px solid var(--line);border-radius:8px;overflow:hidden}
  .type-counts {display:flex;flex-wrap:wrap;gap:14px;font-size:11px;color:var(--soft);margin-bottom:18px}
  .mode-switch button {padding:10px 18px;background:var(--surface);color:inherit;border:0;cursor:pointer}.mode-switch .chosen {background:var(--accent-bg);color:var(--accent)}
  select {background:var(--input-bg);color:inherit;border:1px solid var(--line);padding:10px;border-radius:6px}.search-field {flex:1;min-width:180px}
  .system-list {display:grid;grid-template-columns:repeat(auto-fill,minmax(min(100%,248px),1fr));gap:14px;align-items:stretch}
  .system-card {display:flex;flex-direction:column;align-items:stretch;gap:18px;min-width:0;min-height:184px;padding:20px;background:var(--surface);color:var(--text);border:1px solid var(--line);border-radius:12px;text-align:left;cursor:pointer;transition:background .15s,border-color .15s}
  .system-card:hover {background:var(--surface-raised);border-color:var(--muted)}
  .component-heading {display:flex;align-items:flex-start;gap:12px;min-width:0}.component-heading > :global(svg) {flex-shrink:0;margin-top:11px;color:var(--soft)}
  .component-icon {display:grid;place-items:center;flex-shrink:0;width:40px;height:40px;border-radius:9px;background:var(--accent-bg);color:var(--accent)}
  .component-copy {flex:1;display:flex;flex-direction:column;gap:6px;min-width:0}.component-copy strong {font-size:14px;line-height:1.4;overflow-wrap:anywhere}.component-copy span {font-size:12px;line-height:1.5;color:var(--soft);overflow-wrap:anywhere}
  .component-status {font-size:12px;line-height:1.6;color:var(--soft);overflow-wrap:anywhere}
  .component-transports {display:flex;flex-wrap:wrap;gap:5px;margin-top:auto;padding-top:12px;border-top:1px solid var(--faint-line)}
  .system-card.offline {background:var(--subtle);border-color:var(--faint-line)}.system-card.offline .component-copy strong {color:var(--soft)}.system-card.offline .component-icon {background:var(--surface);color:var(--muted)}
  .system-card.offline:hover {background:var(--surface);border-color:var(--muted)}
  .system-card.chosen {border-color:var(--accent);box-shadow:inset 0 0 0 1px var(--accent)}.system-card:focus-visible {outline-offset:3px}
  .component-details {padding:24px;overflow-wrap:anywhere}.provider-diagnostics {margin:16px 0}.provider-diagnostics p {font-size:12px}dialog {max-width:650px;max-height:80vh;overflow:auto;background:var(--surface);color:inherit;border:1px solid var(--line);border-radius:14px;padding:28px}dialog::backdrop {background:#0009}
  @media(max-width:700px){.system-list {gap:12px}.system-card {padding:16px;min-height:172px}}
</style>
