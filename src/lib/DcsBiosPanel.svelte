<script lang="ts">
  import { onMount } from 'svelte';
  import DcsInputBinding from './DcsInputBinding.svelte';
  import DcsOutputBinding from './DcsOutputBinding.svelte';
  import { dcsStatusLabel, dcsFeedbackLabel, type DcsBiosStatus } from './dcs-bios';
  import { newDcsProfile, dcsProfileEqual, dcsProfileError, dcsProfileStartError, boundDcsTargets, dcsTargetKey, dcsOutputActions, physicalDcsControls, dcsBindingState, type DcsProfileConfig, type DcsCataloguePage, type DcsCatalogueControl, type DcsTarget } from './dcs-profile';
  import type { GatewayRequest, GatewayState } from './gateway';
  let { draft = $bindable(null), selectedTarget = $bindable(''), status, targets, busy, request, requestGateway, disableController, canDisable }: {
    draft: DcsProfileConfig | null; selectedTarget: string; status?: DcsBiosStatus; targets: DcsTarget[]; busy: boolean;
    request: (request: GatewayRequest) => Promise<GatewayState | false>;
    requestGateway: (gatewayId: string, request: GatewayRequest) => Promise<boolean>;
    disableController: (gatewayId: string, moduleId: string) => void;
    canDisable: (gatewayId: string, moduleId: string) => boolean;
  } = $props();
  let pending = $state(false), notice = $state(''), query = $state('');
  let catalogue = $state<DcsCatalogueControl[]>([]);
  let page = $state<DcsCataloguePage>();
  const configError = $derived(draft ? dcsProfileError(draft) : 'Loading profile…');
  const startError = $derived(draft ? dcsProfileStartError(draft, targets, status?.config) : 'Loading profile…');
  const saved = $derived(!!draft && dcsProfileEqual(draft, status?.config));
  const readOnly = $derived(busy || pending || !!status?.running);
  const bound = $derived(draft ? boundDcsTargets(draft) : []);
  const selected = $derived(targets.find(target => `${target.gatewayId}/${target.module.id}` === selectedTarget) ?? targets[0]);
  const controls = $derived(page?.cataloguePath === draft?.cataloguePath ? catalogue : []);
  function restore() { draft = newDcsProfile(status?.config); notice = ''; }
  onMount(() => {
    let cancelled = false;
    void request({ command: 'dcs_status' }).then(result => {
      if (!cancelled && !draft) draft = newDcsProfile(result ? result.dcsBios?.config : status?.config);
      if (!cancelled && draft?.cataloguePath) void loadCatalogue();
      if (!cancelled && draft) void refreshBound();
    });
    return () => { cancelled = true; };
  });
  async function refresh(gatewayId: string, moduleId: string) {
    const results = await Promise.allSettled([requestGateway(gatewayId, { command: 'events_state', moduleId }), requestGateway(gatewayId, { command: 'action_describe', moduleId })]);
    if (results.some(result => result.status === 'rejected' || !result.value)) notice = 'Some device capabilities could not be refreshed. Reconnect the device and refresh before starting.';
  }
  async function refreshBound() { if (draft) await Promise.all(boundDcsTargets(draft).map(target => refresh(target.gatewayId, target.moduleId))); }
  async function loadCatalogue(offset = 0) {
    if (!draft || busy || pending || !draft.cataloguePath.trim()) return;
    pending = true; notice = '';
    if (!offset) { catalogue = []; page = undefined; }
    try {
      const response = await request({ command: 'dcs_catalogue', cataloguePath: draft.cataloguePath, aircraftModule: 'A-10C', kind: 'all', query, offset, limit: 32 });
      const next = response && response.dcsBios?.catalogue;
      if (next) { catalogue = offset ? [...new Map([...catalogue, ...next.controls].map(control => [control.id, control])).values()] : next.controls; page = next; }
      else notice = 'The catalogue could not be loaded. Check the installed doc/json folder.';
    } finally { pending = false; }
  }
  function nextId(prefix: string) {
    const ids = [...draft!.inputs, ...draft!.outputs].map(binding => binding.id);
    let index = 1; while (ids.includes(`${prefix}-${index}`)) index++;
    return `${prefix}-${index}`;
  }
  function addInput() {
    if (!draft || readOnly || draft.inputs.length >= 16) return;
    draft.inputs.push({ id: nextId('input'), gatewayId: selected?.gatewayId ?? '', moduleId: selected?.module.id ?? '', kind: 'button', control: 0, command: '', interface: 'set_state', pressedValue: 1, releasedValue: 0 });
    if (selected) void refresh(selected.gatewayId, selected.module.id);
  }
  function addOutput() {
    if (!draft || readOnly || draft.outputs.length >= 16) return;
    draft.outputs.push({ id: nextId('output'), gatewayId: selected?.gatewayId ?? '', moduleId: selected?.module.id ?? '', control: '', output: 0, actionId: '', format: { kind: 'text', overflow: 'truncate' } });
    if (selected) void refresh(selected.gatewayId, selected.module.id);
  }
  function masterCaution() {
    if (!draft || !selected || readOnly || draft.inputs.length >= 16 || draft.outputs.length >= 16) return;
    const events = selected.state?.moduleEvents?.[selected.module.id], led = dcsOutputActions('boolean', selected.state?.moduleActions?.[selected.module.id])[0];
    draft.inputs.push({ id: nextId('master-caution-button'), gatewayId: selected.gatewayId, moduleId: selected.module.id, kind: 'button', control: physicalDcsControls('button', events)[0]?.value ?? 0, command: 'UFC_MASTER_CAUTION', interface: 'set_state', pressedValue: 1, releasedValue: 0 });
    draft.outputs.push({ id: nextId('master-caution-led'), gatewayId: selected.gatewayId, moduleId: selected.module.id, control: 'MASTER_CAUTION', output: 0, actionId: led?.id ?? '', format: { kind: 'boolean' } });
    void refresh(selected.gatewayId, selected.module.id);
  }
  async function send(command: 'dcs_configure' | 'dcs_start' | 'dcs_stop') {
    if (busy || !draft || command !== 'dcs_stop' && pending || command !== 'dcs_stop' && status?.running) return;
    if (command === 'dcs_configure' && configError || command === 'dcs_start' && (!saved || startError)) return;
    const response = await request(command === 'dcs_configure' ? { command, dcsConfig: $state.snapshot(draft) } : { command });
    if (response && command === 'dcs_configure') {
      if (response.dcsBios?.config) draft = newDcsProfile(response.dcsBios.config);
      notice = 'Profile saved. Start DCS-BIOS explicitly when the devices are ready.';
    }
  }
</script>

<section aria-label="DCS-BIOS profile editor">
  <header class="profile-heading"><div><h2>A-10C panel profile</h2><p>Map physical controls to named DCS-BIOS commands and feedback to device outputs.</p></div><span class="status" role="status">{dcsStatusLabel(status)}</span></header>
  {#if draft}
    {#if status?.config?.schemaVersion === 1}<p class="notice">Your saved master-caution settings are shown as two bindings. Saving upgrades this profile; saving or restarting Cockpit does not start it.</p>{/if}
    <fieldset disabled={readOnly}><legend>Installed catalogue and connection</legend>
      <label for="dcs-catalogue">Installed DCS-BIOS catalogue folder<input id="dcs-catalogue" type="text" bind:value={draft.cataloguePath} placeholder="…/DCS-BIOS/doc/json" spellcheck="false" /></label><p>Use the catalogue installed with DCS. Select controls by name; Cockpit resolves their addresses.</p>
      <details><summary>UDP connection settings</summary><div class="fields"><label for="dcs-command-host">DCS command host<input id="dcs-command-host" type="text" bind:value={draft.commandHost} spellcheck="false" /></label><label for="dcs-command-port">Command port<input id="dcs-command-port" type="number" min="1" max="65535" step="1" bind:value={draft.commandPort} /></label><label for="dcs-listen-address">Feedback listen address<input id="dcs-listen-address" type="text" bind:value={draft.listenAddress} spellcheck="false" /></label><label for="dcs-listen-port">Feedback port<input id="dcs-listen-port" type="number" min="1" max="65535" step="1" bind:value={draft.listenPort} /></label><label for="dcs-multicast-group">Multicast group (optional)<input id="dcs-multicast-group" type="text" bind:value={draft.multicastGroup} spellcheck="false" /></label><label for="dcs-multicast-interface">Multicast interface<input id="dcs-multicast-interface" type="text" bind:value={draft.multicastInterface} spellcheck="false" /></label><label for="dcs-freshness">Feedback freshness (ms)<input id="dcs-freshness" type="number" min="100" max="10000" step="1" bind:value={draft.freshnessMs} /></label></div><p>Use IPv4 addresses. Leave the multicast group empty for unicast feedback.</p></details>
    </fieldset>
    <div class="catalogue-search"><label for="dcs-control-search">Find catalogue controls<input id="dcs-control-search" type="search" bind:value={query} placeholder="Radio, frequency, master caution…" /></label><button disabled={busy || pending || !draft.cataloguePath.trim()} onclick={() => loadCatalogue()}>Search catalogue</button>{#if page?.cataloguePath === draft.cataloguePath}<span>{catalogue.length} of {page.total} matching controls</span>{#if page.offset + page.controls.length < page.total}<button disabled={busy || pending || page.query !== query} onclick={() => loadCatalogue(page!.offset + page!.controls.length)}>Load more controls</button>{/if}{/if}</div>
    <div class="binding-tools"><label for="dcs-new-target">Device for new bindings<select id="dcs-new-target" bind:value={selectedTarget} disabled={readOnly}><option value="">First available device</option>{#each targets as target (`${target.gatewayId}/${target.module.id}`)}<option value={`${target.gatewayId}/${target.module.id}`}>{target.label} · {target.gatewayId.slice(6)}</option>{/each}</select></label><button disabled={readOnly || !selected || draft.inputs.length >= 16 || draft.outputs.length >= 16} onclick={masterCaution}>Add master-caution preset</button><button disabled={busy || pending || !bound.length} onclick={refreshBound}>Refresh bound devices</button></div>
    <div class="bound-devices">{#each bound as item (dcsTargetKey(item))}{@const target = targets.find(target => target.gatewayId === item.gatewayId && target.module.id === item.moduleId)}{@const needsInput = draft.inputs.some(binding => dcsTargetKey(binding) === dcsTargetKey(item))}<div><strong>{target?.label ?? item.moduleId}</strong><span>{target?.module.online ? needsInput ? target.module.directUsb ? 'Direct USB active' : target.module.enabled === false && target.module.published === false ? 'HID disabled · CAN inputs' : 'Disable HID before starting' : 'Output device · HID unchanged' : 'Offline'}</span>{#if needsInput && (target?.module.enabled !== false || target?.module.published !== false)}<button disabled={readOnly || !canDisable(item.gatewayId, item.moduleId)} onclick={() => disableController(item.gatewayId, item.moduleId)}>Disable controller</button>{/if}</div>{/each}</div>
    <p>Input devices require disabled gateway HID and inactive direct USB to avoid duplicate bindings. Disable uses the existing reconnect confirmation. Output-only devices can retain their USB controllers.</p>
    <div class="binding-columns"><section aria-label="DCS-BIOS input bindings"><div class="list-heading"><h3>Inputs</h3><button disabled={readOnly || draft.inputs.length >= 16} onclick={addInput}>Add input</button></div>{#each draft.inputs as binding, index (binding.id)}<DcsInputBinding bind:binding={draft.inputs[index]} {targets} catalogue={controls} disabled={readOnly} status={dcsBindingState(status, 'input', binding.id)} remove={() => draft!.inputs.splice(index, 1)} {refresh} />{/each}{#if !draft.inputs.length}<p>Add a button, maintained switch, encoder or calibrated axis.</p>{/if}</section><section aria-label="DCS-BIOS output bindings"><div class="list-heading"><h3>Outputs</h3><button disabled={readOnly || draft.outputs.length >= 16} onclick={addOutput}>Add output</button></div>{#each draft.outputs as binding, index (binding.id)}<DcsOutputBinding bind:binding={draft.outputs[index]} {targets} catalogue={controls} disabled={readOnly} status={dcsBindingState(status, 'output', binding.id)} remove={() => draft!.outputs.splice(index, 1)} {refresh} />{/each}{#if !draft.outputs.length}<p>Add an LED or bounded display feedback binding.</p>{/if}</section></div>
    {#if notice}<p class="notice" role="status">{notice}</p>{/if}{#if status?.error}<p class="error" role="alert">{status.error}</p>{/if}{#if !status?.running && (configError || startError)}<p>{configError || startError}</p>{/if}
    <div class="toolbar"><button disabled={readOnly || !!configError || saved && status?.config?.schemaVersion !== 1} onclick={() => send('dcs_configure')}>Save settings</button><button class="start" disabled={busy || pending || !!status?.running || !saved || !!startError} onclick={() => send('dcs_start')}>Start DCS-BIOS</button><button disabled={busy || !status?.running} onclick={() => send('dcs_stop')}>Stop DCS-BIOS</button><button disabled={readOnly} onclick={restore}>Restore saved</button></div><p>{status?.running ? 'Stop before editing this profile. Runtime processing continues independently of this view.' : saved ? status?.config?.schemaVersion === 1 ? 'Your unchanged master-caution profile can start as saved. Save settings to upgrade it explicitly.' : 'Profile saved. Start is explicit after every application restart.' : 'Save this profile before starting.'}</p>
  {:else}<p>Loading the saved profile…</p>{/if}
  <dl aria-label="DCS-BIOS connection status"><div><dt>Aircraft</dt><dd>{status?.aircraft || 'Unknown'}</dd></div><div><dt>Feedback</dt><dd>{dcsFeedbackLabel(status)}</dd></div><div><dt>Commands sent</dt><dd>{status?.commandsSent ?? 0}</dd></div><div><dt>Retained event overwrites</dt><dd>{status?.eventOverflow ?? 0}</dd></div><div><dt>Recovered event gaps</dt><dd>{status?.eventGaps ?? 0}</dd></div></dl>
  {#if status?.gateways?.length}<section class="traffic" aria-label="DCS-BIOS gateway traffic"><h3>Gateway traffic</h3><p>Observed software traffic and latency for this running profile.</p><div class="table-scroll"><table><thead><tr><th>Gateway</th><th>Queue / peak</th><th>Writes / second</th><th>Coalesced</th><th>Mean / p95 / max write ms</th><th>Max input poll ms</th><th>Events</th></tr></thead><tbody>{#each status.gateways as metric (metric.gatewayId)}<tr><td>{metric.gatewayId.slice(6)}</td><td>{metric.queueDepth ?? 0} / {metric.queueHighWater ?? 0}</td><td>{metric.actionWrites ?? 0} / {metric.throughputPerSecond?.toFixed(1) ?? '—'}</td><td>{metric.coalesced ?? 0}</td><td>{metric.latencyMeanMs?.toFixed(1) ?? '—'} / {metric.latencyP95Ms?.toFixed(1) ?? '—'} / {metric.latencyMaxMs?.toFixed(1) ?? '—'}</td><td>{metric.inputPollMaxIntervalMs?.toFixed(1) ?? '—'}</td><td>{metric.eventsProcessed ?? 0}</td></tr>{/each}</tbody></table></div></section>{/if}
</section>

<style>
  h2 { margin: 0; font-size: 18px; } h3 { margin: 0; font-size: 15px; }
  .profile-heading, .list-heading, .toolbar { display: flex; flex-wrap: wrap; align-items: center; gap: 12px; }
  .profile-heading, .list-heading { justify-content: space-between; }
  p, .status, .catalogue-search > span { color: var(--soft); font-size: 11px; line-height: 1.7; }
  .status { padding: 4px 8px; border: 1px solid var(--line); border-radius: 4px; }
  fieldset { min-width: 0; margin: 16px 0; padding: 16px; border: 1px solid var(--line); border-radius: 6px; }
  legend, label { color: var(--soft); font-size: 11px; } label { display: block; margin-top: 10px; }
  input, select { display: block; width: 100%; min-width: 0; margin-top: 5px; padding: 8px; border: 1px solid var(--line); border-radius: 4px; background: var(--input-bg); color: var(--text); font-size: 11px; }
  .fields, .binding-columns { display: grid; grid-template-columns: repeat(2, minmax(0, 1fr)); gap: 0 16px; }
  .catalogue-search, .binding-tools { display: flex; flex-wrap: wrap; align-items: end; gap: 12px; margin: 16px 0; }
  .catalogue-search label, .binding-tools label { flex: 1; min-width: 220px; }
  button { padding: 8px 12px; border: 1px solid var(--line); border-radius: 4px; background: var(--input-bg); color: var(--text); font-size: 11px; cursor: pointer; }
  button.start { border-color: var(--accent); } button:disabled, fieldset:disabled { opacity: .55; } button:disabled { cursor: default; }
  input:focus-visible, select:focus-visible, summary:focus-visible, button:focus-visible { outline: 2px solid var(--accent); outline-offset: 2px; }
  details { margin-top: 16px; } summary { color: var(--soft); font-size: 11px; cursor: pointer; }
  .notice { padding: 12px; border: 1px solid var(--line); border-radius: 5px; background: var(--input-bg); } .error { color: var(--amber); }
  .bound-devices > div { display: flex; flex-wrap: wrap; align-items: center; gap: 12px; padding: 10px 0; border-bottom: 1px solid var(--line); color: var(--soft); font-size: 11px; }
  .bound-devices strong { flex: 1; color: var(--text); } .binding-columns { margin-top: 24px; }
  dl { display: grid; grid-template-columns: repeat(3, minmax(0, 1fr)); gap: 12px; margin-top: 24px; }
  dl > div { border-top: 1px solid var(--line); padding: 10px 0; } dt { color: var(--soft); font-size: 11px; } dd { margin: 5px 0 0; color: var(--text); font-size: 12px; }
  .traffic { margin-top: 24px; } .table-scroll { overflow-x: auto; } table { width: 100%; border-collapse: collapse; font-size: 11px; color: var(--soft); } th, td { text-align: left; padding: 10px 8px; border-bottom: 1px solid var(--line); white-space: nowrap; }
  @media (max-width: 1100px) { .binding-columns { grid-template-columns: 1fr; } } @media (max-width: 700px) { .fields, dl { grid-template-columns: 1fr; } }
</style>
