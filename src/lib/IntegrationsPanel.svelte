<script lang="ts">
  import { onMount, tick } from 'svelte';
  import IntegrationInputRow from './IntegrationInputRow.svelte';
  import IntegrationOutputRow from './IntegrationOutputRow.svelte';
  import { assignIntegrationRole, renameIntegrationRole, exportIntegrationDocument, importIntegrationDocument, integrationEqual, integrationError, integrationFingerprint, integrationStartError, newIntegrationConfig, nextIntegrationId, revLightOutput, roleTarget, syntheticTelemetryFrame,
    type IntegrationConfig, type IntegrationState, type IntegrationRequest, type IntegrationProvider, type IntegrationTarget, type IntegrationCatalogue } from './integration-profile';
  let { draft = $bindable(null), status, targets, busy, request, refresh }: {
    draft?: IntegrationConfig | null; status?: IntegrationState; targets: IntegrationTarget[]; busy: boolean;
    request: (request: IntegrationRequest) => Promise<IntegrationState | false>;
    refresh: (gatewayId: string,moduleId: string) => Promise<unknown>;
  } = $props();
  let pending = $state(false), notice = $state(''), documentText = $state(''), exportUrl = $state(''), query = $state('');
  let validationKey = $state(''), validationRevision = $state<number | undefined>(), validationCatalogue = $state<string | number | undefined>();
  let catalogue = $state<IntegrationCatalogue>();
  let simRpm = $state(6300), simGear = $state(4), simSpeed = $state(124.25), simMax = $state(7500), simEpoch = $state(1), simPacket = $state(0);
  let simStatus = $state<'live'|'paused'|'replay'|'off'>('live'), simStream = $state(false), simFrozen = $state(false);
  const readOnly = $derived(busy || pending || !!status?.running);
  const configError = $derived(draft ? integrationError(draft) : 'Loading profile…');
  const startError = $derived(draft ? integrationStartError(draft,targets) : 'Loading profile…');
  const saved = $derived(!!draft && integrationEqual(draft,status?.config));
  const validated = $derived(!!draft && validationKey === integrationFingerprint(draft) && validationRevision === status?.revision && validationCatalogue === status?.validation?.catalogueRevision && !!status?.validation?.valid);
  const diagnostics = $derived(status?.validation?.diagnostics ?? status?.diagnostics ?? []);
  function resetValidation() { validationKey='';validationRevision=undefined;validationCatalogue=undefined; }
  function restore() { draft=status?.config ? structuredClone($state.snapshot(status.config)) : newIntegrationConfig();resetValidation();notice=''; }
  function providerChanged(provider: IntegrationProvider) { draft=newIntegrationConfig(provider);catalogue=undefined;resetValidation();notice='Choose local connection settings and device roles for this provider.'; }
  onMount(() => {
    let active=true, sending=false;
    void request({command:'integration_get_state'}).then(async result => {
      if(!active) return;
      if(!draft) draft=result && result.config ? structuredClone(result.config) : newIntegrationConfig();
      if(result && result.catalogue && draft && (result.catalogue.provider === draft.document.provider || result.catalogue.aircraftModule === draft.document.context.aircraftModule && result.catalogue.cataloguePath === draft.connection.cataloguePath)) catalogue=result.catalogue;
      await tick();if(active && draft?.assignments.length) void refreshAssigned();
    });
    const timer=setInterval(() => {
      if (active && simStream && !sending && status?.running && draft?.document.provider === 'assetto-corsa-rally' && draft.connection.source === 'synthetic') {
        sending=true;void sendSynthetic().finally(() => {sending=false;});
      }
    },100);
    return () => {active=false;clearInterval(timer);if(exportUrl) URL.revokeObjectURL(exportUrl);};
  });
  async function loadCatalogue() {
    if(!draft || busy || pending) return;
    pending=true;notice='';
    try {const result=await request({command:'integration_catalogue',integrationConfig:$state.snapshot(draft),query,offset:0,limit:64});catalogue=result ? result.catalogue : undefined;if(!catalogue) notice='The catalogue could not be loaded. Review local provider settings.';}
    finally {pending=false;}
  }
  async function refreshAssigned() {
    if(!draft || busy || pending) return;
    pending=true;
    try {await Promise.all([...new Map(draft.assignments.map(a => [`${a.gatewayId}/${a.moduleId}`,a])).values()].map(a => refresh(a.gatewayId,a.moduleId)));}
    finally {pending=false;}
  }
  async function validate() {
    if(!draft || configError || readOnly) return;
    pending=true;notice='';const snapshot=$state.snapshot(draft),key=integrationFingerprint(snapshot);
    try {
      const result=await request({command:'integration_validate',integrationConfig:snapshot});
      if(result && result.validation?.valid && draft && integrationFingerprint(draft)===key) {validationKey=key;validationRevision=result.revision;validationCatalogue=result.validation.catalogueRevision;notice='Validation passed for this draft. Save and Start remain explicit.';}
      else {resetValidation();notice='Review validation diagnostics before saving or starting.';}
    } finally {pending=false;}
  }
  async function save() {
    if(!draft || !validated || readOnly) return;
    pending=true;
    try {const result=await request({command:'integration_save',integrationConfig:$state.snapshot(draft),revision:status?.revision});if(result && result.config && !result.error) {draft=structuredClone(result.config);resetValidation();notice='Profile saved. Validate the saved settings before starting.';}}
    finally {pending=false;}
  }
  async function start() {if(!draft || !saved || !validated || startError || readOnly) return;await request({command:'integration_start',revision:status?.revision});}
  async function stop() {simStream=false;await request({command:'integration_stop'});resetValidation();}
  async function readFile(file?: File) {
    if(!file) return;if(file.size>65536) {notice='Use a profile file no larger than 64 KiB.';return;}
    documentText=await file.text();importPreview();
  }
  function importPreview() {
    const preview=importIntegrationDocument(documentText,draft);
    if(preview.error) {notice=preview.error;return;}
    draft=preview.config!;catalogue=undefined;resetValidation();notice='Import preview loaded. Assign device roles and validate explicitly. No profile was saved or started.';
  }
  function prepareExport() {
    if(!draft) return;
    try {documentText=exportIntegrationDocument($state.snapshot(draft));if(exportUrl) URL.revokeObjectURL(exportUrl);exportUrl=URL.createObjectURL(new Blob([documentText],{type:'application/json'}));notice='Portable export excludes local paths, connection settings and device assignments.';}
    catch(error) {notice=error instanceof Error ? error.message : 'Complete the portable profile before exporting.';}
  }
  function assign(role: string,key: string) {
    if(!draft) return;const target=targets.find(t => `${t.gatewayId}/${t.module.id}`===key);draft=assignIntegrationRole(draft,role,target);resetValidation();if(target) void refresh(target.gatewayId,target.module.id);
  }
  function addRole() {if(!draft || draft.document.roles.length>=8) return;let index=1;while(draft.document.roles.some(r => r.id===`role-${index}`)) index++;draft.document.roles.push({id:`role-${index}`,label:`Device role ${index}`});}
  function renameRole(previous: string,next: string) {if(!draft) return;try {draft=renameIntegrationRole(draft,previous,next);resetValidation();} catch(error) {notice=error instanceof Error ? error.message : 'Choose a valid role identifier.';}}
  function removeRole(id: string) {if(!draft) return;if([...draft.document.inputs,...draft.document.outputs].some(b => b.role===id)) {notice='Remove or reassign this role’s bindings first.';return;}draft.document.roles=draft.document.roles.filter(r => r.id!==id);draft.assignments=draft.assignments.filter(a => a.role!==id);}
  function addInput() {if(draft && draft.document.inputs.length<16) draft.document.inputs.push({id:nextIntegrationId(draft,'input'),role:draft.document.roles[0]?.id ?? '',kind:'button',control:0,command:'',interface:'set_state',pressedValue:1,releasedValue:0});}
  function addOutput() {if(draft && draft.document.outputs.length<16) draft.document.outputs.push({id:nextIntegrationId(draft,'output'),role:draft.document.roles[0]?.id ?? '',endpoint:'',actionId:'',channel:'',output:0,format:{kind:'text',align:'left',overflow:'truncate'}});}
  function addRevLights(role: string) {if(!draft) return;const target=roleTarget(draft,role,targets),row=target && revLightOutput(draft,role,target,catalogue);if(!row) {notice='Load the rev_ratio channel and refresh a device with an RGB or Boolean LED setter.';return;}if(draft.document.outputs.some(o => o.role===role && o.endpoint===row.endpoint)) {notice='That endpoint is already bound. Edit its numeric bands or threshold.';return;}draft.document.outputs.push(row);notice='Rev-light rule added. Review thresholds, validate and save explicitly.';}
  async function sendSynthetic() {
    if(!status?.running || draft?.document.provider!=='assetto-corsa-rally' || draft.connection.source!=='synthetic') return;
    try {if(!simFrozen) simPacket=(simPacket+1)>>>0;const frame=syntheticTelemetryFrame({packetId:simPacket,epoch:simEpoch,rpm:simRpm,gear:simGear,speed:simSpeed,maxRpm:simMax,status:simStatus});await request({command:'integration_simulate',telemetryFrame:frame});}
    catch(error) {simStream=false;notice=error instanceof Error ? error.message : 'Synthetic telemetry was rejected.';}
  }
</script>

<section aria-label="Simulator integrations">
  <header><div><h2>Simulator integrations</h2><p>Portable profiles bind logical device roles to named commands and telemetry.</p></div><span role="status">{status?.running ? 'Running' : status?.configured ? 'Saved · stopped' : 'Stopped'}</span></header>
  {#if draft}
    <fieldset disabled={readOnly}><legend>Profile and provider</legend><div class="fields">
      <label>Provider<select value={draft.document.provider} onchange={event => providerChanged(event.currentTarget.value as IntegrationProvider)}><option value="dcs-bios">DCS-BIOS</option><option value="assetto-corsa-rally">Assetto Corsa Rally</option></select></label>
      <label>Profile name<input bind:value={draft.document.name} maxlength="63" /></label><label>Profile identifier<input bind:value={draft.document.id} maxlength="31" spellcheck="false" /></label>
      {#if draft.document.provider === 'dcs-bios'}<label>Installed aircraft module<input bind:value={draft.document.context.aircraftModule} placeholder="A-10C" spellcheck="false" /></label><label>Installed DCS-BIOS doc/json directory<input bind:value={draft.connection.cataloguePath} spellcheck="false" /></label>{:else}<label>Telemetry source<select bind:value={draft.connection.source}><option value="shared-memory">Windows ACR shared memory</option><option value="synthetic">Authored synthetic telemetry</option></select></label>{#if draft.connection.source === 'shared-memory'}<label>Windows game process ID<input type="number" min="1" max="4294967295" step="1" bind:value={draft.connection.processId} /></label>{/if}{/if}
      <label>Freshness timeout (ms)<input type="number" min="100" max="10000" step="1" bind:value={draft.connection.freshnessMs} /></label>
    </div>
    {#if draft.document.provider === 'dcs-bios'}<details><summary>Local UDP connection</summary><div class="fields"><label>Command host<input bind:value={draft.connection.commandHost} /></label><label>Command port<input type="number" min="1" max="65535" step="1" bind:value={draft.connection.commandPort} /></label><label>Feedback address<input bind:value={draft.connection.listenAddress} /></label><label>Feedback port<input type="number" min="1" max="65535" step="1" bind:value={draft.connection.listenPort} /></label><label>Multicast group (optional)<input bind:value={draft.connection.multicastGroup} /></label><label>Multicast interface<input bind:value={draft.connection.multicastInterface} /></label></div></details>{:else}<p>ACR is telemetry-only. Native acquisition uses Windows shared memory. Synthetic frames exercise the same parser; they are separate from a game UDP protocol. Live game and hardware validation are pending.</p>{/if}
    </fieldset>
    <div class="tools"><label>Find named controls<input type="search" bind:value={query} placeholder="RPM, radio, master caution…" /></label><button disabled={busy || pending} onclick={loadCatalogue}>Load provider catalogue</button><button disabled={busy || pending || !draft.assignments.length} onclick={refreshAssigned}>Refresh assigned devices</button><span>{catalogue?.total ?? catalogue?.channels?.length ?? catalogue?.controls?.length ?? 0} matching controls</span></div>
    <fieldset disabled={readOnly}><legend>Local device roles</legend><p>Choose each exact gateway/module target. Import never rebinds a role. Output devices need no HID controller; input roles require disabled HID and inactive direct USB.</p>
      {#each draft.document.roles as role, index (index)}{@const target=roleTarget(draft,role.id,targets)}<div class="role-row"><label>Role identifier<input value={role.id} onchange={event => renameRole(role.id,event.currentTarget.value)} maxlength="31" spellcheck="false" /></label><label>Role label<input bind:value={role.label} maxlength="63" /></label><label>Assigned device<select value={target ? `${target.gatewayId}/${target.module.id}` : ''} onchange={event => assign(role.id,event.currentTarget.value)}><option value="">Unassigned</option>{#each targets as item (`${item.gatewayId}/${item.module.id}`)}<option value={`${item.gatewayId}/${item.module.id}`}>{item.label} · {item.gatewayId} · {item.module.id}</option>{/each}</select></label><button onclick={() => removeRole(role.id)}>Remove role</button>{#if draft.document.provider==='assetto-corsa-rally'}<button disabled={!target || draft.document.outputs.length>=16} onclick={() => addRevLights(role.id)}>Add rev lights</button>{/if}</div>{/each}
      <button disabled={readOnly || draft.document.roles.length>=8} onclick={addRole}>Add device role</button>
    </fieldset>
    <div class="binding-columns">{#if draft.document.provider==='dcs-bios'}<section aria-label="Integration input bindings"><div class="heading"><h3>Inputs</h3><button disabled={readOnly || draft.document.inputs.length>=16} onclick={addInput}>Add input</button></div>{#each draft.document.inputs as binding, index (index)}<IntegrationInputRow bind:binding={draft.document.inputs[index]} config={draft} {catalogue} {targets} disabled={readOnly} remove={() => draft!.document.inputs.splice(index,1)} {refresh} />{/each}</section>{/if}
      <section aria-label="Integration output bindings"><div class="heading"><h3>Outputs</h3><button disabled={readOnly || draft.document.outputs.length>=16} onclick={addOutput}>Add output</button></div>{#each draft.document.outputs as binding, index (index)}<IntegrationOutputRow bind:binding={draft.document.outputs[index]} config={draft} {catalogue} {targets} disabled={readOnly} running={!!status?.running} busy={busy || pending} status={status?.outputs?.find(o => o.id===binding.id)} remove={() => draft!.document.outputs.splice(index,1)} {refresh} {request} />{/each}{#if !draft.document.outputs.length}<p>Add a readable LED/RGB endpoint or a display with advertised byte, cell and encoding bounds.</p>{/if}</section>
    </div>
    <details class="portable"><summary>Import or export portable profile JSON</summary><p>The document contains semantic roles and bindings. Local paths, connections and physical assignments stay in Cockpit.</p><label>Import JSON file<input type="file" accept="application/json,.json" disabled={readOnly} onchange={event => readFile(event.currentTarget.files?.[0])} /></label><label>Portable document JSON<textarea rows="12" bind:value={documentText} spellcheck="false" disabled={readOnly}></textarea></label><div class="tools"><button disabled={readOnly || !documentText.trim()} onclick={importPreview}>Preview import</button><button disabled={readOnly} onclick={prepareExport}>Prepare portable export</button>{#if exportUrl}<a href={exportUrl} download={`${draft.document.id}.json`}>Download profile JSON</a>{/if}</div></details>
    {#if notice}<p class="notice" role="status">{notice}</p>{/if}{#if status?.error}<p class="error" role="alert">{status.error}</p>{/if}
    {#if diagnostics.length}<section aria-label="Integration validation diagnostics"><h3>Validation diagnostics</h3><ul>{#each diagnostics as item,index (index)}<li class:error={item.severity==='error'}>{item.bindingId ? `${item.bindingId}: ` : ''}{item.message} <span>({item.code})</span></li>{/each}</ul></section>{/if}
    {#if !status?.running && (configError || startError)}<p>{configError || startError}</p>{/if}
    <div class="tools"><button disabled={readOnly || !!configError} onclick={validate}>Validate draft</button><button disabled={readOnly || !validated} onclick={save}>Save validated profile</button><button disabled={readOnly || !saved || !validated || !!startError} onclick={start}>Start integration</button><button disabled={busy || !status?.running} onclick={stop}>Stop integration</button><button disabled={readOnly} onclick={restore}>Restore saved</button></div>
    <p>{status?.running ? 'Processing continues in the native background service. Stop before changing this profile.' : !saved ? 'Preview changes require validation and explicit Save.' : validated ? 'Saved profile validated. Starting remains explicit.' : 'Saved profile requires current validation before Start.'}</p>
    {#if draft.document.provider==='assetto-corsa-rally' && draft.connection.source==='synthetic'}
      <fieldset><legend>Synthetic ACR telemetry</legend><p>Authored byte prefixes enter the production decoder. First attachment requires an advancing physics packet before feedback becomes fresh.</p><div class="fields"><label>RPM<input type="number" min="0" max="100000" step="1" bind:value={simRpm} /></label><label>Maximum RPM (0 = unknown)<input type="number" min="0" max="100000" step="1" bind:value={simMax} /></label><label>Normalized gear (−1 reverse, 0 neutral)<input type="number" min="-1" max="31" step="1" bind:value={simGear} /></label><label>Speed km/h<input type="number" min="0" max="2000" step="any" bind:value={simSpeed} /></label><label>Source epoch<input type="number" min="1" step="1" bind:value={simEpoch} /></label><label>Session<select bind:value={simStatus}><option value="live">Live</option><option value="paused">Paused</option><option value="replay">Replay</option><option value="off">Off</option></select></label></div><div class="tools"><label class="check"><input type="checkbox" bind:checked={simFrozen} />Freeze physics packet</label><label class="check"><input type="checkbox" bind:checked={simStream} disabled={!status?.running} />Stream synthetic frames</label><button disabled={busy || !status?.running} onclick={sendSynthetic}>Send synthetic frame</button></div></fieldset>
    {/if}
  {:else}<p>Loading integration settings…</p>{/if}
  {#if status?.telemetry}<section aria-label="Integration telemetry status"><h3>Telemetry</h3><p>{status.telemetry.status ?? 'Unknown'} · {status.telemetry.fresh ? 'Fresh' : 'Unknown or stale'} · age {status.telemetry.ageMs ?? 'unknown'} ms</p>{#if status.telemetry.channels}<dl>{#each Object.entries(status.telemetry.channels) as [id,channel] (id)}<div><dt>{id}</dt><dd>{channel.value ?? 'Unknown'} · {channel.status}</dd></div>{/each}</dl>{/if}</section>{/if}
</section>

<style>
  header, .heading, .tools { display: flex; flex-wrap: wrap; align-items: center; gap: 12px; } header, .heading { justify-content: space-between; }
  h2 { font-size: 18px; margin: 0; } h3 { font-size: 14px; margin: 0; } p, header span, .tools span, li { color: var(--soft); font-size: 11px; line-height: 1.7; }
  fieldset, .portable { min-width: 0; margin: 16px 0; padding: 16px; border: 1px solid var(--line); border-radius: 6px; } legend { font-size: 12px; font-weight: 600; }
  .fields, .binding-columns { display: grid; grid-template-columns: repeat(2,minmax(0,1fr)); gap: 12px; }
  label { display: block; color: var(--soft); font-size: 11px; } input, select, textarea { display: block; width: 100%; min-width: 0; margin-top: 5px; padding: 8px; border: 1px solid var(--line); border-radius: 4px; background: var(--input-bg); color: var(--text); font-size: 11px; }
  textarea { font-family: var(--font-mono,monospace); resize: vertical; } .role-row { display: grid; grid-template-columns: 1fr 1fr 2fr auto; align-items: end; gap: 10px; margin: 12px 0; }
  button, a { padding: 8px 12px; border: 1px solid var(--line); border-radius: 4px; background: var(--input-bg); color: var(--text); font-size: 11px; cursor: pointer; text-decoration: none; } :disabled { opacity: .55; cursor: default; }
  :focus-visible { outline: 2px solid var(--accent); outline-offset: 2px; } .tools { margin: 16px 0; } .tools > label:not(.check) { flex: 1; }
  summary { color: var(--soft); font-size: 12px; cursor: pointer; } details p { margin: 10px 0; } .notice { padding: 12px; border: 1px solid var(--line); border-radius: 5px; } .error { color: var(--amber); }
  .check { display: flex; align-items: center; gap: 7px; } .check input { display: inline; width: auto; margin: 0; } dl { display: flex; flex-wrap: wrap; gap: 18px; color: var(--soft); font-size: 11px; } dd { margin: 5px 0; color: var(--text); }
  @media (max-width: 900px) { .binding-columns { grid-template-columns: 1fr; } .role-row { grid-template-columns: repeat(2,minmax(0,1fr)); } } @media (max-width: 650px) { .fields, .role-row { grid-template-columns: 1fr; } }
</style>
