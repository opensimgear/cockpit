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
        if (!disposed && epoch === selectionEpoch) detail = nextDetail;
      }
      error = '';
    } catch (e) { if (!disposed) error = e instanceof Error ? e.message : String(e); }
    finally { polling = false; }
  }
  function select(source: HostController) {
    selectionEpoch++; selected = source.id; detail = null; name = source.name; role = source.role; remembered = ''; controlPage = 0;
    void refresh();
  }
  async function mutate(command: GatewayRequest) {
    busy = true; selectionEpoch++;
    try {
      const next = await request(command);
      if (command.command === 'controller_forget') { selected = ''; detail = null; selectionEpoch++; }
      else { detail = next; selected = next.source?.id ?? selected; name = next.source?.name ?? name; role = next.source?.role ?? role; }
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

<section class="host-controllers" aria-label="Host controllers">
  {#if !detailsOnly}
  <div class="panel-heading"><div><h2>{topology ? 'USB controllers connected to this computer' : 'Host controllers'}</h2><p>Physical controller input · {inventory.backendStatus ?? 'Connecting'}</p></div><span class="quiet-tag">Experimental</span></div>
  {#if !topology}<OptionalDependencies />{/if}
  {#if inventory.diagnostic}<Notification message={String(inventory.diagnostic)} kind="error" />{/if}
  {#if error}<Notification message={String(error)} kind="error" />{/if}
  {#if actionError}<Notification message={String(actionError)} kind="error" />{/if}
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
  {#if detail?.source}
    {@const source = detail.source}
    <section class="panel host-detail" aria-label="Selected host controller">
      <div class="panel-heading"><h3>{source.name.trim()}</h3><button class="text-button" onclick={() => { selectionEpoch++; selected = ''; detail = null; }}>Close</button></div>
      <p>{source.online ? source.baselineReady ? 'Live input' : 'Waiting for a complete baseline' : 'Offline · input values cleared'} · connection {source.generation} · report {source.sequence}</p>
      <div class="metadata">
        <label>Local name<input class="text-input" bind:value={name} maxlength="80" /></label>
        <label>Role<select bind:value={role}>{#each controllerRoles as option}<option value={option}>{option}</option>{/each}</select></label>
        <button class="secondary-button" disabled={busy || !name.trim()} onclick={() => mutate({ command: 'controller_update', sourceId: source.id, name, role })}>Save local details</button>
      </div>
      {#each detail.diagnostics ?? [] as message}<p>{message}</p>{/each}
      {#if source.online && (inventory.sources ?? []).some(s => !s.online)}
        <div class="metadata"><label>Moved to another USB port?<select bind:value={remembered}><option value="">Choose a remembered offline controller</option>{#each (inventory.sources ?? []).filter(s => !s.online) as previous}<option value={previous.id}>{previous.name}</option>{/each}</select></label><button class="secondary-button" disabled={busy || !remembered} onclick={() => mutate({ command: 'controller_reassign', sourceId: remembered, observedId: source.id })}>Reassign this connection</button></div>
      {/if}
      {#key source.id}<DeviceVisibility {source} />{/key}
      <div class="control-grid">
        {#each detail.controls ?? [] as control (control.capability.id)}
          <div class="control" class:pressed={control.capability.kind === 'button' && control.value?.normalized === 1}>
            <strong>{controlLabel(control)}</strong><span>{inputLabel(control)}</span>
            {#if control.capability.kind === 'axis' && control.value?.normalized != null}<meter min={control.capability.min < 0 ? -32768 : 0} max="32767" value={control.value.normalized} aria-label={controlLabel(control)}></meter>{/if}
            <small>Report {control.capability.reportId} · collection {control.capability.collection} · field {control.capability.dataIndex}</small>
            <small>Native {control.value?.native ?? '—'} · range {control.capability.min}…{control.capability.max}</small>
          </div>
        {/each}
      </div>
      {#if detail.total > detail.limit}<div class="paging"><button class="secondary-button" disabled={controlPage === 0} onclick={() => { controlPage = Math.max(0, controlPage - 32); void refresh(); }}>Previous controls</button><span>{controlPage + 1}–{Math.min(controlPage + 32, detail.total)} / {detail.total}</span><button class="secondary-button" disabled={controlPage + 32 >= detail.total} onclick={() => { controlPage += 32; void refresh(); }}>Next controls</button></div>{/if}
      <details><summary>Identity and capabilities</summary><dl><dt>Local source ID</dt><dd>{source.id}</dd><dt>Identity evidence</dt><dd>{source.identityConfidence === 'serial' ? 'Serial and collection; duplicate matches require reassignment' : 'Local path; a port change requires reassignment'}</dd><dt>Observed path</dt><dd>{detail.identity?.path}</dd><dt>Collection</dt><dd>{detail.identity?.collection}</dd><dt>Capability fingerprint</dt><dd>{detail.identity?.fingerprint}</dd></dl></details>
      {#if !source.online && !detailsOnly}<button class="secondary-button" disabled={busy} onclick={() => mutate({ command: 'controller_forget', sourceId: source.id })}>Forget offline controller</button>{/if}
    </section>
  {/if}
</section>
<style>
  .host-controllers { margin-top: 24px; text-align: left; }
  .panel-heading p { opacity: .65; margin: 6px 0; }
  .host-grid, .control-grid { display: grid; grid-template-columns: repeat(auto-fill, minmax(230px, 1fr)); gap: 12px; }
  .host-card, .control { padding: 18px; border: 1px solid var(--line); background: var(--surface); border-radius: 10px; color: inherit; text-align: left; display: flex; flex-direction: column; gap: 8px; }
  .host-card { cursor: pointer; }
  .host-card.selected, .control.pressed { border-color: var(--accent); background: var(--accent-bg); }
  .offline { opacity: .55; }
  .host-card span, small { font-size: 12px; opacity: .7; }
  .host-detail { padding: 24px; margin-top: 20px; }
  .metadata, .paging { display: flex; flex-wrap: wrap; gap: 16px; align-items: end; margin: 20px 0; }
  label { display: flex; flex-direction: column; gap: 8px; flex: 1; min-width: 180px; }
  select { background: var(--input-bg); border: 1px solid var(--line); border-radius: 5px; padding: 9px; }
  meter { width: 100%; }
  details { margin: 20px 0; }
  dd { margin: 4px 0 16px; overflow-wrap: anywhere; }
</style>
