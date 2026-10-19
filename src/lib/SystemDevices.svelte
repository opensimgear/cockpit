<script lang="ts">
  import { onMount, tick, type Snippet } from 'svelte';
  import Icon from './Icon.svelte';
  import { systemDeviceIcon } from './system-device-icons';
  import Notification from './Notification.svelte';
  import SystemTopology from './SystemTopology.svelte';
  import HostControllers from './HostControllers.svelte';
  import SystemUsbDetails from './SystemUsbDetails.svelte';
  import { gatewayRequest } from './gateway';
  import type { GatewayEntry } from './gateway-workspace';
  import type { GameId } from './games';
  import { SystemInventoryController, integrationGameId } from './system-inventory-providers';
  import { filterSystem, isVisibleSystemComponent, presenceLabels, systemStateLabel, systemTypeLabels, type SystemComponent, type SystemSnapshot, type InventoryStorage } from './system-inventory';
  let { active, entries, gatewayError = '', openModule, openCompanions, openIntegration, gatewayDetails, oninventory, onNavigate }: {
    active: boolean; entries: GatewayEntry[]; openModule: (gatewayId: string, moduleId: string) => void;
    gatewayError?: string;
    openCompanions: (companionId: string) => void; openIntegration: (gameId: GameId) => void;
    gatewayDetails: Snippet<[string]>; oninventory: (snapshot: SystemSnapshot) => void;
    onNavigate?: (componentId: string | null) => void;
  } = $props();
  let snapshot = $state.raw<SystemSnapshot>({ components: [], diagnostics: [], observedAt: 0 });
  let mode = $state('list'), search = $state(''), filter = $state('all'), transport = $state('all'), selectedId = $state('');
  let detailsOpen = $state(false);
  let inventoryElement: HTMLDivElement | undefined;
  let busy = $state(false), error = $state(''), confirmation = $state<SystemComponent[]>([]);
  let cleanupSelection = $state<string[]>([]), cleanupRunning = $state(false);
  let refreshTask: Promise<SystemSnapshot> | undefined;
  let controller: SystemInventoryController;
  let reviewEpoch = 0;
  const visible = $derived(filterSystem(snapshot.components, search, filter, transport));
  const components = $derived(snapshot.components.filter(isVisibleSystemComponent));
  const selected = $derived(components.find(c => c.id === selectedId));
  const showingDetails = $derived(detailsOpen && !!selected && active);
  const eligible = $derived(visible.filter(c => c.forget.allowed));
  const cleanupAvailable = $derived(confirmation.filter(canCleanup));
  const cleanupSelected = $derived(cleanupAvailable.filter(c => cleanupSelection.includes(c.id)));
  const cleanupAllSelected = $derived(cleanupAvailable.length > 0 && cleanupSelected.length === cleanupAvailable.length);
  const cleanupPartSelected = $derived(cleanupSelected.length > 0 && !cleanupAllSelected);
  const captureLabels: Record<SystemComponent['capture'], string> = { ready: 'Ready', waiting: 'Waiting', unavailable: 'Unavailable', unknown: 'Unknown' };
  const date = (timestamp: number) => timestamp ? new Date(timestamp).toLocaleString() : 'Never observed connected';
  function select(id: string) { selectedId = id; detailsOpen = true; onNavigate?.(id); }
  export function showInventory() { detailsOpen = false; onNavigate?.(null); }
  export function getNavigation(): string | null { return detailsOpen ? selectedId || null : null; }
  export function restoreNavigation(componentId: string | null): void { selectedId = componentId ?? selectedId; detailsOpen = componentId !== null; }
  async function backToDevices() {
    showInventory();
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
  function canCleanup(component: SystemComponent) {
    const current = snapshot.components.find(item => item.id === component.id);
    return !!current?.forget.allowed && current.presence !== 'connected' && !current.owner && !current.hidden;
  }
  function selectCleanup(id: string, checked: boolean) {
    cleanupSelection = checked ? [...new Set([...cleanupSelection, id])] : cleanupSelection.filter(item => item !== id);
  }
  function closeCleanup() {
    if (cleanupRunning) return;
    reviewEpoch++; confirmation = []; cleanupSelection = [];
  }
  async function reviewForget(items: SystemComponent[]) {
    if (busy) return;
    const epoch = ++reviewEpoch;
    busy = true; confirmation = items; cleanupSelection = [];
    try {
      await refreshTask?.catch(() => undefined);
      if (epoch !== reviewEpoch) return;
      const reviewed = await controller.reviewForget(items.map(c => c.id));
      if (epoch === reviewEpoch) {
        confirmation = reviewed;
        cleanupSelection = cleanupSelection.filter(id => reviewed.some(item => item.id === id));
      }
    }
    catch (e) { if (epoch === reviewEpoch) { error = String(e); closeCleanup(); } }
    finally { busy = false; }
  }
  async function forget() {
    const items = [...cleanupSelected];
    if (busy || !items.length) return;
    busy = true; cleanupRunning = true; const failures: string[] = [];
    await refreshTask?.catch(() => undefined);
    for (const c of items) {
      try { await controller.forget(c.id); if (selectedId === c.id) { selectedId = ''; detailsOpen = false; onNavigate?.(null); } }
      catch (e) { failures.push(`${c.name}: ${String(e)}`); }
    }
    confirmation = []; cleanupSelection = []; cleanupRunning = false; error = failures.join('\n'); busy = false;
    await refresh();
    if (failures.length) error = failures.join('\n');
  }
  function dialog(node: HTMLDialogElement) { node.showModal(); node.querySelector<HTMLButtonElement>('[data-cleanup-cancel]')?.focus(); return { destroy: () => node.close() }; }
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
<div class="page-heading inventory-heading"><div><div class="eyebrow">YOUR CONNECTED SYSTEM</div><h1>Devices</h1><p>Every known component, its current state, and how it connects.</p></div><button class="secondary-button cleanup-button" disabled={busy || !eligible.length} onclick={() => reviewForget(eligible)}>Cleanup ({eligible.length})</button></div>
<div class="system-toolbar">
  <div class="mode-switch" role="group" aria-label="Devices presentation"><button class:chosen={mode === 'list'} aria-pressed={mode === 'list'} onclick={() => setMode('list')}>List</button><button class:chosen={mode === 'topology'} aria-pressed={mode === 'topology'} onclick={() => setMode('topology')}>Topology</button></div>
  <label class="search-field"><Icon name="search" size={16} /><input aria-label="Search components" placeholder="Find a component…" bind:value={search} /></label>
  <select aria-label="Filter components" bind:value={filter}><option value="all">All components</option><option value="connected">Connected</option><option value="offline">Offline</option><option value="unknown">Unknown</option><option value="unavailable">Unavailable</option><option value="hidden">Hidden</option>{#each Object.entries(systemTypeLabels).filter(([type]) => type !== 'host') as [type,label]}<option value={type}>{label}</option>{/each}</select>
  <select aria-label="Filter connections" bind:value={transport}><option value="all">All connections</option><option value="usb">USB</option><option value="can">CAN</option><option value="network">Network</option><option value="bluetooth">Bluetooth</option><option value="software">Software</option><option value="unknown">Unknown connection</option></select>
</div>
<div class="type-counts" aria-label="Component counts"><span aria-live="polite">{visible.length} / {components.length} components</span>{#each Object.entries(systemTypeLabels).filter(([type]) => type !== 'host') as [type,label]}<span>{label}: {components.filter(c => c.type === type).length}</span>{/each}</div>
{#if snapshot.diagnostics.length}<details class="provider-diagnostics"><summary>Inventory diagnostics ({snapshot.diagnostics.length})</summary>{#each snapshot.diagnostics as diagnostic}<p>{diagnostic}</p>{/each}</details>{/if}
{#if mode === 'topology'}
  <SystemTopology components={visible} context={snapshot.components} {selectedId} onselect={select} />
{:else}
  <div class="system-list" aria-label="System components">
    {#each visible as c (c.id)}
      <button class="system-card component-tile" data-component-id={c.id} class:chosen={selectedId === c.id} class:offline={c.presence === 'offline'} aria-pressed={selectedId === c.id} aria-label={`${c.name} · ${systemTypeLabels[c.type]} · ${c.role} · ${systemStateLabel(c)} · ${[...new Set(c.connections.map(x => x.transport))].join(', ') || 'Connection unknown'}`} title={systemStateLabel(c)} onclick={() => select(c.id)}>
        <div class="component-heading"><span class="component-icon"><Icon name={systemDeviceIcon(c)} size={24} /></span><div class="component-copy"><strong>{c.name}</strong><span>{systemTypeLabels[c.type]}{c.role ? ` · ${c.role}` : ''}</span></div></div>
        {#if c.type === 'companion' && c.status.includes('Panel: ')}<div class="companion-panels">{c.status.split(' · ').filter(part => part.startsWith('Panel: ')).join(' · ')}</div>{/if}
        <div class="component-transports">{#each [...new Set(c.connections.map(x => x.transport))] as transport}<span class="quiet-tag">{transport === 'software' ? 'Software' : transport === 'network' ? 'Network' : transport === 'unknown' ? 'Connection unknown' : transport.toUpperCase()}</span>{:else}<span class="quiet-tag">Connection unknown</span>{/each}</div>
      </button>
    {:else}<div class="empty-state"><strong>{components.length ? 'No matching components' : busy ? 'Loading known components…' : 'No known components'}</strong><p>Connected, offline and unavailable components appear here when their providers report them.</p><button class="text-button" onclick={() => { search = ''; filter = 'all'; transport = 'all'; }}>Clear filters</button></div>{/each}
  </div>
{/if}
</div>
{#if selected && showingDetails}
  {@const companionId = selected.targets.find(target => target.kind === 'companion')?.id}
  <div class="details-toolbar"><button class="text-button" use:focusDetails onclick={backToDevices}><Icon name="back" size={16} />Back to Devices</button></div>
  <section class="component-details" aria-label="Component details">
    <div class="page-heading detail-heading">
      <div class="detail-title"><span class="detail-icon" class:disconnected={selected.presence !== 'connected'}><Icon name={systemDeviceIcon(selected)} size={26} /></span><div><div class="eyebrow">DEVICE DETAILS</div><h1>{selected.name}</h1><p>{systemTypeLabels[selected.type]}{selected.role ? ` · ${selected.role}` : ''}</p></div></div>
      <span class="connection-badge"><span class="status-dot" class:offline={selected.presence !== 'connected'}></span>{presenceLabels[selected.presence]}</span>
    </div>
    <div class="component-actions">
      {#each selected.targets.filter(t => t.kind === 'module') as t}<button class="primary-button" onclick={() => openModule(t.gatewayId!, t.id)}>Configure device</button>{/each}
      {#if selected.type === 'companion' && companionId}<button class="secondary-button" onclick={() => openCompanions(companionId)}>Manage companion</button>{/if}
      {#if integrationGameId(selected)}<button class="secondary-button" onclick={() => openIntegration(integrationGameId(selected)!)}>Integration settings</button>{/if}
      <button class="secondary-button" disabled={busy || !selected.forget.allowed} title={selected.forget.reason} onclick={() => reviewForget([selected])}>Forget device</button>
    </div>
    <div class="detail-summary">
      <section class="panel" aria-label="Device status">
        <div class="panel-heading"><h2>Status</h2>{#if selected.stale}<span class="quiet-tag">Refresh needed</span>{/if}</div>
        <div class="detail-body">
          <p class="status-summary">{systemStateLabel(selected)}</p>
          <dl class="detail-metadata">
            <div><dt>Input capture</dt><dd>{captureLabels[selected.capture] ?? 'Unknown'}</dd></div>
            <div><dt>Owner</dt><dd>{selected.owner ?? 'None observed'}</dd></div>
            {#if selected.type === 'osg'}
              <div><dt>Input owner</dt><dd>{selected.inputOwner ?? 'None confirmed'}</dd></div>
              <div><dt>Configuration writer</dt><dd>{selected.configurationOwner ?? 'None observed'}</dd></div>
              <div><dt>Configuration route</dt><dd>{selected.configurationRoute ?? 'None observed'}</dd></div>
            {/if}
          </dl>
          {#if !selected.forget.allowed}<p class="detail-help">{selected.forget.reason}</p>{/if}
        </div>
      </section>
      <section class="panel" aria-label="Device connections">
        <div class="panel-heading"><h2>Connections</h2><span class="quiet-tag">{selected.connections.length} {selected.connections.length === 1 ? 'route' : 'routes'}</span></div>
        <ul class="connection-list">
          {#each selected.connections as edge (edge.id)}
            <li><span class="connection-icon"><Icon name={edge.transport === 'usb' ? 'usb' : edge.transport === 'can' || edge.transport === 'network' ? 'network' : 'link'} size={18} /></span><div><strong>{edge.transport.toUpperCase()} · {snapshot.components.find(c => c.id === edge.from)?.name ?? edge.from}</strong>{#if edge.endpoint}<code>{edge.endpoint}</code>{/if}<p>{edge.current ? 'Current connection' : edge.observed === false ? 'Configured connection (unconfirmed)' : 'Last-known connection'}</p></div></li>
          {:else}<li><span class="connection-icon"><Icon name="link" size={18} /></span><div><strong>Connection unknown</strong><p>No connection has been observed for this device.</p></div></li>{/each}
        </ul>
      </section>
    </div>
    {#if selected.diagnostics.length}<section class="panel" aria-label="Device diagnostics"><div class="panel-heading"><h2>Diagnostics</h2></div><ul class="diagnostic-list">{#each selected.diagnostics as diagnostic}<li>{diagnostic}</li>{/each}</ul></section>{/if}
    {#each selected.targets.filter(t => t.kind === 'gateway') as t}{@render gatewayDetails(t.id)}{/each}
    {#each selected.targets.filter(t => t.kind === 'controller') as t (t.id)}{#key t.id}<HostControllers initialSourceId={t.id} detailsOnly />{/key}{/each}
    {#each selected.targets.filter(t => t.kind === 'usb') as t (t.id)}{#key t.id}<SystemUsbDetails endpointKey={t.id} />{/key}{/each}
    <details class="panel identity-details">
      <summary>Identity and evidence</summary>
      <div class="detail-body"><dl class="detail-metadata">
        <div><dt>Stable identity</dt><dd><code>{selected.id}</code></dd></div>
        <div><dt>First seen</dt><dd>{date(selected.firstSeen)}</dd></div>
        <div><dt>Last connected</dt><dd>{date(selected.lastSeen)}</dd></div>
        <div><dt>Observed by</dt><dd>{selected.provider}</dd></div>
        <div><dt>Last observation</dt><dd>{date(selected.observedAt)}{selected.stale ? ' · Observation stale' : ''}</dd></div>
      </dl>{#if selected.evidence.length}<ul class="evidence-list">{#each selected.evidence as evidence}<li><code>{evidence}</code></li>{/each}</ul>{/if}</div>
    </details>
  </section>
{/if}
{#if confirmation.length}
  <dialog class="cleanup-dialog" use:dialog oncancel={event => { if (cleanupRunning) event.preventDefault(); else closeCleanup(); }} aria-labelledby="cleanup-title" aria-describedby="cleanup-help">
    <header class="cleanup-heading"><h2 id="cleanup-title">Cleanup devices</h2><p id="cleanup-help">Select the devices to forget. Review what will be removed before confirming.</p></header>
    <div class="cleanup-body">
      <p class="cleanup-summary" role="status">{cleanupRunning ? 'Cleaning up selected devices…' : busy ? 'Checking stored references…' : `${cleanupSelected.length} of ${confirmation.length} selected`}</p>
      <div class="cleanup-table-wrap">
        <table class="cleanup-table" aria-label="Devices to clean up">
          <colgroup><col class="cleanup-select-column" /><col class="cleanup-name-column" /><col /></colgroup>
          <thead><tr><th scope="col" class="cleanup-checkbox"><label><input type="checkbox" aria-label="Select all" checked={cleanupAllSelected} indeterminate={cleanupPartSelected} disabled={busy || !cleanupAvailable.length} onchange={event => cleanupSelection = event.currentTarget.checked ? cleanupAvailable.map(item => item.id) : []} /><span>Select all</span></label></th><th scope="col">Name</th><th scope="col">Description</th></tr></thead>
          <tbody>{#each confirmation as c (c.id)}
            <!-- svelte-ignore a11y_click_events_have_key_events a11y_no_noninteractive_element_interactions (The native checkbox provides keyboard interaction; the row expands its pointer target.) -->
            <tr class:cleanup-unavailable={!canCleanup(c)} class:cleanup-selectable={!busy && canCleanup(c)} onclick={event => { if (!busy && canCleanup(c) && !(event.target instanceof HTMLInputElement)) selectCleanup(c.id, !cleanupSelection.includes(c.id)); }}>
              <td class="cleanup-checkbox"><input type="checkbox" aria-label={`Select ${c.name}`} checked={cleanupSelected.some(item => item.id === c.id)} disabled={busy || !canCleanup(c)} onchange={event => selectCleanup(c.id, event.currentTarget.checked)} /></td>
              <th scope="row">{c.name}</th>
              <td><p>{c.forget.effect}</p>{#each c.diagnostics.filter(d => d.startsWith('Affected stored references')) as diagnostic}<p class="cleanup-reference">{diagnostic}</p>{/each}{#if c.references.length}<p class="cleanup-reference"><strong>Affected references (retained as unresolved):</strong> {c.references.map(r => r.name).join(', ')}</p>{/if}{#if !canCleanup(c)}<p class="cleanup-reference">{snapshot.components.find(item => item.id === c.id)?.forget.reason || 'This device is no longer eligible for cleanup.'}</p>{/if}</td>
            </tr>
          {/each}</tbody>
        </table>
      </div>
    </div>
    <footer class="cleanup-actions"><button class="secondary-button destructive-button" disabled={busy || !cleanupSelected.length} onclick={forget}>Confirm</button><button class="secondary-button" data-cleanup-cancel disabled={cleanupRunning} onclick={closeCleanup}>Cancel</button></footer>
  </dialog>
{/if}

</section>

<style>
  .system-inventory[hidden] {display:none}
  .inventory-heading {align-items:center}.inventory-heading > div {min-width:0}.cleanup-button {flex-shrink:0}
  .details-toolbar {display:flex;align-items:center;justify-content:space-between;gap:16px;margin-bottom:18px}
  .system-toolbar,.component-actions { display:flex;align-items:center;flex-wrap:wrap;gap:12px;margin-bottom:18px; }
  .mode-switch {display:flex;border:1px solid var(--line);border-radius:8px;overflow:hidden}
  .type-counts {display:flex;flex-wrap:wrap;gap:14px;font-size:11px;color:var(--soft);margin-bottom:18px}
  .mode-switch button {padding:10px 18px;background:var(--surface);color:inherit;border:0;cursor:pointer}.mode-switch .chosen {background:var(--accent-bg);color:var(--accent)}
  select {background:var(--input-bg);color:inherit;border:1px solid var(--line);padding:10px;border-radius:6px}.search-field {flex:1;min-width:180px}
  .system-list {display:grid;grid-template-columns:repeat(auto-fill,minmax(min(100%,248px),1fr));gap:14px;align-items:stretch}
  .system-card {display:flex;flex-direction:column;align-items:stretch;gap:18px;min-width:0;min-height:184px;padding:20px;background:var(--surface);color:var(--text);border:1px solid var(--line);border-radius:12px;text-align:left;cursor:pointer;transition:background .15s,border-color .15s}
  .system-card:hover {background:var(--surface-raised);border-color:var(--muted)}
  .component-heading {display:flex;align-items:flex-start;gap:12px;min-width:0}.component-heading > :global(svg) {flex-shrink:0;margin-top:11px;color:var(--soft)}
  .component-icon {display:grid;place-items:center;flex-shrink:0;width:40px;height:40px;border-radius:9px;background:var(--accent-bg);color:var(--accent)}
  .component-copy {flex:1;display:flex;flex-direction:column;gap:6px;min-width:0}.component-copy strong {font-size:14px;line-height:1.4;overflow-wrap:anywhere}.component-copy span {font-size:12px;line-height:1.5;color:var(--soft);overflow-wrap:anywhere}
  .companion-panels {font-size:12px;line-height:1.6;color:var(--soft);overflow-wrap:anywhere}
  .component-transports {display:flex;flex-wrap:wrap;gap:5px;margin-top:auto;padding-top:12px;border-top:1px solid var(--faint-line)}
  .system-card.offline {background:var(--subtle);border-color:var(--faint-line)}.system-card.offline .component-copy strong {color:var(--soft)}.system-card.offline .component-icon {background:var(--surface);color:var(--muted)}
  .system-card.offline:hover {background:var(--surface);border-color:var(--muted)}
  .system-card.chosen {border-color:var(--accent);box-shadow:inset 0 0 0 1px var(--accent)}.system-card:focus-visible {outline-offset:3px}
  .component-details { display: grid; gap: 20px; min-width: 0; }
  .component-details > *, .detail-title > div { min-width: 0; }
  .detail-heading { margin-bottom: 0; }
  .detail-title { display: flex; align-items: center; gap: 16px; min-width: 0; }
  .detail-title h1 { overflow-wrap: anywhere; }
  .detail-title .eyebrow { margin-bottom: 8px; }
  .detail-icon { display: grid; place-items: center; flex-shrink: 0; width: 52px; height: 52px; border: 1px solid var(--line); border-radius: 10px; color: var(--accent); background: var(--accent-bg); }
  .detail-icon.disconnected { color: var(--muted); background: transparent; border-color: transparent; }
  .detail-heading .connection-badge { flex-shrink: 0; padding: 8px 10px; border: 1px solid var(--line); border-radius: 5px; background: var(--surface); }
  .component-details > .component-actions { margin: 0; gap: 10px; }
  .detail-summary { display: grid; grid-template-columns: repeat(2, minmax(0, 1fr)); gap: 20px; }
  .detail-summary > .panel { min-width: 0; }
  .detail-body { padding: 20px 21px; }
  .status-summary { color: var(--soft); font-size: 12px; line-height: 1.7; margin-bottom: 18px; overflow-wrap: anywhere; }
  .detail-metadata { display: grid; gap: 12px; margin: 0; }
  .detail-metadata > div { display: grid; grid-template-columns: minmax(100px, .4fr) minmax(0, 1fr); gap: 16px; font-size: 10px; line-height: 1.7; }
  .detail-metadata dt { color: var(--muted); }
  .detail-metadata dd { margin: 0; color: var(--soft); overflow-wrap: anywhere; }
  .detail-help { color: var(--muted); font-size: 9px; line-height: 1.7; margin-top: 18px; }
  .connection-list { list-style: none; margin: 0; padding: 0; }
  .connection-list li { display: flex; align-items: flex-start; gap: 12px; padding: 18px 21px; border-bottom: 1px solid var(--line); }
  .connection-list li:last-child { border-bottom: 0; }
  .connection-list li > div { min-width: 0; }
  .connection-icon { display: grid; place-items: center; width: 30px; height: 30px; flex-shrink: 0; border-radius: 5px; color: var(--accent); background: var(--accent-bg); }
  .connection-list strong { font-size: 11px; font-weight: 550; color: var(--soft); overflow-wrap: anywhere; }
  .connection-list code { display: block; margin-top: 7px; }
  .connection-list p { margin-top: 7px; color: var(--muted); font-size: 9px; line-height: 1.7; }
  .component-details code { font-size: 9px; color: var(--soft); line-height: 1.7; overflow-wrap: anywhere; }
  .diagnostic-list { margin: 0; padding: 18px 21px 18px 36px; font-size: 10px; line-height: 1.8; color: var(--soft); overflow-wrap: anywhere; }
  .identity-details > summary { padding: 18px 21px; color: var(--soft); font-size: 11px; font-weight: 550; cursor: pointer; }
  .identity-details[open] > summary { border-bottom: 1px solid var(--line); }
  .identity-details > summary:focus-visible { outline: 2px solid var(--accent); outline-offset: -3px; }
  .evidence-list { margin: 18px 0 0; padding-left: 16px; }
  .provider-diagnostics {margin:16px 0}.provider-diagnostics p {font-size:12px}
  .cleanup-dialog {width:min(920px,calc(100vw - 48px));max-width:none;max-height:80dvh;margin:auto;padding:0;overflow:hidden;background:var(--surface);color:var(--text);border:1px solid var(--line);border-radius:12px;box-shadow:0 24px 80px #0006}
  .cleanup-dialog[open] {display:flex;flex-direction:column}.cleanup-dialog::backdrop {background:#0009}
  .cleanup-heading {padding:24px;flex-shrink:0}.cleanup-heading h2 {font-size:20px;font-weight:600;letter-spacing:-.35px;line-height:1.35}.cleanup-heading p {margin-top:8px;font-size:12px;line-height:1.65;color:var(--muted)}
  .cleanup-body {display:flex;flex-direction:column;gap:12px;min-height:0;padding:0 24px 24px}.cleanup-summary {flex-shrink:0;font-size:11px;line-height:1.5;color:var(--soft)}.cleanup-table-wrap {min-height:0;overflow:auto;border:1px solid var(--line);border-radius:8px}
  .cleanup-table {width:100%;min-width:560px;table-layout:fixed;border-collapse:collapse;text-align:left}.cleanup-select-column {width:108px}.cleanup-name-column {width:24%}.cleanup-table th,.cleanup-table td {padding:16px 12px;vertical-align:top;border-bottom:1px solid var(--line);overflow-wrap:anywhere}.cleanup-table thead th {position:sticky;top:0;background:var(--surface-raised);font-size:11px;font-weight:600;color:var(--soft);z-index:1}.cleanup-table tbody th {font-size:13px;font-weight:600;line-height:1.65;color:var(--text)}.cleanup-table tbody tr:last-child>* {border-bottom:0}
  .cleanup-table .cleanup-checkbox {text-align:left}.cleanup-checkbox label {display:inline-flex;align-items:center;gap:8px;white-space:nowrap;cursor:pointer}.cleanup-checkbox input {width:16px;height:16px;margin:0;vertical-align:middle;accent-color:var(--accent);cursor:pointer}.cleanup-checkbox input:disabled {cursor:not-allowed;opacity:.45}.cleanup-table td>p {font-size:12px;line-height:1.65;color:var(--soft);margin:0}.cleanup-table td>p+p {margin-top:10px}.cleanup-table td>.cleanup-reference {font-size:11px;color:var(--muted)}.cleanup-reference strong {font-weight:600}.cleanup-table .cleanup-unavailable th,.cleanup-table .cleanup-unavailable td {color:var(--muted)}
  .cleanup-selectable {cursor:pointer}.cleanup-selectable:hover {background:var(--surface-raised)}
  .cleanup-actions {display:flex;justify-content:flex-end;align-items:center;gap:12px;padding:24px;border-top:1px solid var(--line);flex-shrink:0}.cleanup-actions button {min-width:86px}.cleanup-actions .destructive-button {background:#b32b38;border-color:#cd4552;color:#fff}.cleanup-actions .destructive-button:hover:enabled {background:#98232e;border-color:#e35d69;color:#fff}
  @media(max-width:700px){.cleanup-dialog {width:calc(100vw - 32px)}.cleanup-heading {padding:20px}.cleanup-body {padding:0 20px 20px}.cleanup-actions {padding:20px}}
  @media(max-width:1000px){.detail-summary {grid-template-columns:minmax(0,1fr)}}
  @media(max-width:700px){.system-list {gap:12px}.system-card {padding:16px;min-height:172px}}
  @media(max-width:520px){.detail-heading {flex-wrap:wrap;gap:14px}.detail-title {gap:10px}.detail-icon {width:40px;height:40px}.detail-body {padding:17px 15px}.connection-list li {padding:17px 15px}.detail-metadata > div {grid-template-columns:1fr;gap:3px}.identity-details > summary {padding:17px 15px}}
</style>
