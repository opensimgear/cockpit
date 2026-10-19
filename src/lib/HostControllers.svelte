<script lang="ts">
  import Notification from './Notification.svelte';
  import { onMount } from 'svelte';
  import Icon from './Icon.svelte';
  import DeviceVisibility from './DeviceVisibility.svelte';
  import OptionalDependencies from './OptionalDependencies.svelte';
  import { gatewayRequest, type GatewayRequest } from './gateway';
  import { controllerRoles, controlLabel, inputLabel, type ControllerResult, type HostController } from './controllers';
  let { topology = false, initialSourceId = '', detailsOnly = false }: { topology?: boolean; initialSourceId?: string; detailsOnly?: boolean } = $props();
  let inventory = $state<ControllerResult>({ sources: [], offset: 0, total: 0, limit: 32 });
  let detail = $state<ControllerResult | null>(null);
  let selected = $state('');
  let name = $state('');
  let role = $state('controller');
  let remembered = $state('');
  let error = $state('');
  let actionError = $state('');
  let busy = $state(false);
  let page = $state(0);
  let controlPage = $state(0);
  let disposed = false;
  let polling = false;
  let selectionEpoch = 0;
  let initializedSourceId = '';
  async function request(request: GatewayRequest) {
    const state = await gatewayRequest(request);
    if (!state.controllers) throw new Error('Controller discovery is unavailable. Restart the native service after updating Cockpit.');
    return state.controllers;
  }
  async function refresh() {
    if (polling || busy || disposed) return;
    polling = true;
    const epoch = selectionEpoch;
    try {
      const next = await request({ command: 'controller_list', offset: page });
      if (disposed) return;
      inventory = next;
      if (selected) {
        const nextDetail = await request({ command: 'controller_inspect', sourceId: selected, offset: controlPage });
        if (!disposed && epoch === selectionEpoch) {
          detail = nextDetail;
          if (nextDetail.source && initializedSourceId !== nextDetail.source.id) {
            name = nextDetail.source.name;
            role = nextDetail.source.role;
            initializedSourceId = nextDetail.source.id;
          }
        }
      }
      error = '';
    } catch (e) { if (!disposed) error = e instanceof Error ? e.message : String(e); }
    finally { polling = false; }
  }
  function select(source: HostController) {
    selectionEpoch++; selected = source.id; detail = null; name = source.name; role = source.role; initializedSourceId = source.id; remembered = ''; controlPage = 0;
    void refresh();
  }
  async function mutate(command: GatewayRequest) {
    busy = true; selectionEpoch++;
    try {
      const next = await request(command);
      if (command.command === 'controller_forget') { selected = ''; initializedSourceId = ''; detail = null; selectionEpoch++; }
      else { detail = next; selected = next.source?.id ?? selected; name = next.source?.name ?? name; role = next.source?.role ?? role; initializedSourceId = next.source?.id ?? initializedSourceId; }
      actionError = '';
    } catch (e) { actionError = e instanceof Error ? e.message : String(e); }
    finally { busy = false; }
    void refresh();
  }
  onMount(() => {
    selected = initialSourceId;
    void refresh();
    const timer = setInterval(() => { void refresh(); }, 250);
    return () => { disposed = true; clearInterval(timer); };
  });
</script>

<section class="host-controllers" class:embedded={detailsOnly} aria-label="Host controllers">
  {#if !detailsOnly}
  <div class="panel-heading"><div><h2>{topology ? 'USB controllers connected to this computer' : 'Host controllers'}</h2><p>Physical controller input · {inventory.backendStatus ?? 'Connecting'}</p></div><span class="quiet-tag">Experimental</span></div>
  {#if !topology}<OptionalDependencies />{/if}
  <div class="host-grid">
    {#each inventory.sources ?? [] as source (source.id)}
      <button class="host-card" class:selected={selected === source.id} class:offline={!source.online} onclick={() => select(source)}>
        <Icon name="usb" size={22} /><strong>{source.name.trim()}</strong>
        <span>{source.osgHid ? 'OpenSimGear HID' : 'Host HID controller'} · {source.role}</span>
        <span>{source.online ? 'Connected' : 'Offline'} · {source.controlCount} controls</span>
        <small>{source.vendorId.toString(16).padStart(4, '0')}:{source.productId.toString(16).padStart(4, '0')}</small>
      </button>
    {:else}<p>{inventory.backendStatus === 'ready' ? 'Connect a joystick, gamepad, wheel or other HID controller.' : 'Capture requires a supported native platform service.'}</p>{/each}
  </div>
  {#if inventory.total > inventory.limit}<div class="paging"><button class="secondary-button" disabled={page === 0} onclick={() => { page = Math.max(0, page - 32); void refresh(); }}>Previous controllers</button><span>{page + 1}–{Math.min(page + 32, inventory.total)} / {inventory.total}</span><button class="secondary-button" disabled={page + 32 >= inventory.total} onclick={() => { page += 32; void refresh(); }}>Next controllers</button></div>{/if}
  {/if}
  {#if inventory.diagnostic}<Notification message={String(inventory.diagnostic)} kind="error" />{/if}
  {#if error}<Notification message={String(error)} kind="error" />{/if}
  {#if actionError}<Notification message={String(actionError)} kind="error" />{/if}
  {#if detail?.source}
    {@const source = detail.source}
    <section class="panel host-detail" aria-label="Selected host controller">
      <div class="panel-heading"><div><h2>{detailsOnly ? 'Controller settings' : source.name.trim()}</h2><p>Local identity, visibility preferences, and live input.</p></div>{#if !detailsOnly}<button class="text-button" onclick={() => { selectionEpoch++; selected = ''; detail = null; }}>Close</button>{/if}</div>
      <div class="host-detail-body">
      <div class="controller-status"><span class="status-dot" class:offline={!source.online}></span><strong>{source.online ? source.baselineReady ? 'Live input' : 'Waiting for a complete baseline' : 'Offline · input values cleared'}</strong><span>Connection {source.generation} · report {source.sequence}</span></div>
      <div class="metadata">
        <label>Local name<input class="text-input" bind:value={name} maxlength="80" /></label>
        <label>Role<select bind:value={role}>{#each controllerRoles as option}<option value={option}>{option}</option>{/each}</select></label>
        <button class="secondary-button" disabled={busy || !name.trim()} onclick={() => mutate({ command: 'controller_update', sourceId: source.id, name, role })}>Save local details</button>
      </div>
      {#each detail.diagnostics ?? [] as message}<p class="controller-diagnostic">{message}</p>{/each}
      {#if source.online && (inventory.sources ?? []).some(s => !s.online)}
        <div class="metadata"><label>Moved to another USB port?<select bind:value={remembered}><option value="">Choose a remembered offline controller</option>{#each (inventory.sources ?? []).filter(s => !s.online) as previous}<option value={previous.id}>{previous.name}</option>{/each}</select></label><button class="secondary-button" disabled={busy || !remembered} onclick={() => mutate({ command: 'controller_reassign', sourceId: remembered, observedId: source.id })}>Reassign this connection</button></div>
      {/if}
      {#key source.id}<DeviceVisibility {source} />{/key}
      <div class="controls-heading"><h3>Live inputs</h3><span class="quiet-tag">{source.controlCount} controls</span></div>
      <div class="control-grid">
        {#each detail.controls ?? [] as control (control.capability.id)}
          <div class="control" class:pressed={control.capability.kind === 'button' && control.value?.normalized === 1}>
            <strong>{controlLabel(control)}</strong><span>{inputLabel(control)}</span>
            {#if control.capability.kind === 'axis' && control.value?.normalized != null}<meter min={control.capability.min < 0 ? -32768 : 0} max="32767" value={control.value.normalized} aria-label={controlLabel(control)}></meter>{/if}
            <small>Report {control.capability.reportId} · collection {control.capability.collection} · field {control.capability.dataIndex}</small>
            <small>Native {control.value?.native ?? '—'} · range {control.capability.min}…{control.capability.max}</small>
          </div>
        {:else}<p class="controls-empty">{source.online ? 'No input controls reported by this controller.' : 'Live input values will appear when this controller reconnects.'}</p>{/each}
      </div>
      {#if detail.total > detail.limit}<div class="paging"><button class="secondary-button" disabled={controlPage === 0} onclick={() => { controlPage = Math.max(0, controlPage - 32); void refresh(); }}>Previous controls</button><span>{controlPage + 1}–{Math.min(controlPage + 32, detail.total)} / {detail.total}</span><button class="secondary-button" disabled={controlPage + 32 >= detail.total} onclick={() => { controlPage += 32; void refresh(); }}>Next controls</button></div>{/if}
      <details class="controller-identity"><summary>Identity and capabilities</summary><dl><div><dt>Local source ID</dt><dd><code>{source.id}</code></dd></div><div><dt>Identity evidence</dt><dd>{source.identityConfidence === 'serial' ? 'Serial and collection; duplicate matches require reassignment' : 'Local path; a port change requires reassignment'}</dd></div><div><dt>Observed path</dt><dd><code>{detail.identity?.path ?? 'Not reported'}</code></dd></div><div><dt>Collection</dt><dd>{detail.identity?.collection ?? 'Not reported'}</dd></div><div><dt>Capability fingerprint</dt><dd><code>{detail.identity?.fingerprint ?? 'Not reported'}</code></dd></div></dl></details>
      {#if !source.online && !detailsOnly}<button class="secondary-button" disabled={busy} onclick={() => mutate({ command: 'controller_forget', sourceId: source.id })}>Forget offline controller</button>{/if}
      </div>
    </section>
  {:else if selected && !error}
    <div class="panel controller-loading" role="status">Loading controller details…</div>
  {/if}
</section>
<style>
  .host-controllers { margin-top: 24px; text-align: left; min-width: 0; }
  .host-controllers.embedded { margin-top: 0; }
  .host-grid, .control-grid { display: grid; grid-template-columns: repeat(auto-fill, minmax(min(100%, 190px), 1fr)); gap: 10px; }
  .host-card, .control { padding: 18px; border: 1px solid var(--line); background: var(--surface); border-radius: 10px; color: inherit; text-align: left; display: flex; flex-direction: column; gap: 8px; }
  .host-card { cursor: pointer; }
  .host-card.selected, .control.pressed { border-color: var(--accent); background: var(--accent-bg); }
  .offline { opacity: .55; }
  .host-card strong, .control strong { font-size: 11px; font-weight: 570; }
  .host-card span, .control span { font-size: 10px; color: var(--soft); line-height: 1.6; }
  small { font-size: 9px; color: var(--muted); line-height: 1.6; overflow-wrap: anywhere; }
  .control { min-width: 0; padding: 14px; gap: 6px; border-radius: 6px; background: var(--input-bg); }
  .host-detail { margin-top: 20px; }
  .embedded .host-detail { margin-top: 0; }
  .host-detail-body { padding: 20px 21px; }
  .controller-status { display: flex; align-items: center; flex-wrap: wrap; gap: 8px; font-size: 10px; color: var(--muted); line-height: 1.6; }
  .controller-status strong { font-weight: 500; color: var(--soft); }
  .controller-status > span:last-child { margin-left: auto; font-size: 9px; }
  .metadata, .paging { display: flex; flex-wrap: wrap; gap: 12px; align-items: end; margin: 20px 0; font-size: 10px; color: var(--muted); }
  label { display: flex; flex-direction: column; gap: 8px; flex: 1; min-width: min(100%, 180px); font-size: 10px; color: var(--soft); }
  input, select { width: 100%; min-width: 0; background: var(--input-bg); color: var(--text); border: 1px solid var(--line); border-radius: 5px; padding: 10px 11px; font-size: 10px; }
  .controller-diagnostic { margin: 12px 0; padding: 12px; border: 1px solid var(--line); background: var(--subtle); border-radius: 5px; font-size: 10px; color: var(--muted); line-height: 1.7; }
  .controls-heading { display: flex; justify-content: space-between; gap: 12px; margin-bottom: 14px; }
  .controls-heading h3 { font-size: 12px; font-weight: 570; }
  .controls-empty, .controller-loading { padding: 20px; font-size: 10px; color: var(--muted); line-height: 1.7; }
  .controls-empty { grid-column: 1 / -1; border: 1px dashed var(--line); border-radius: 6px; }
  meter { width: 100%; }
  .controller-identity { margin-top: 20px; border-top: 1px solid var(--line); padding-top: 16px; }
  summary { cursor: pointer; font-size: 10px; color: var(--soft); }
  dl { margin: 12px 0 0; }
  dl > div { display: grid; grid-template-columns: minmax(100px, 150px) minmax(0, 1fr); gap: 12px; padding: 11px 0; border-bottom: 1px solid var(--faint-line); font-size: 10px; line-height: 1.6; }
  dt { color: var(--muted); }
  dd { margin: 0; color: var(--soft); overflow-wrap: anywhere; }
  code { font-size: 9px; }
  @media (max-width: 600px) { .host-detail-body { padding: 16px; } .controller-status > span:last-child { margin-left: 0; width: 100%; } .metadata > button { width: 100%; } dl > div { grid-template-columns: 1fr; gap: 4px; } }
</style>
