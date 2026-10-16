<script lang="ts">
  import Notification from './Notification.svelte';
  import { onMount } from 'svelte';
  import MappingEditor from './MappingEditor.svelte';
  import CompanionMappings from './CompanionMappings.svelte';
  import { companionInstanceLabel, companionRoleReadiness, exactTargetBindings } from './companion-mappings';
  import OptionalDependencies from './OptionalDependencies.svelte';
  import { gatewayRequest } from './gateway';
  import { controlLabel, type ControllerControl, type HostController } from './controllers';
  import { capturedKind, compatibleControl, directProfile, oneToOneProfile, importMapping, savedAssignments, type CompanionMappingSource, type PanelMappingSource, type MappingAssignment, type MappingKind, type MappingProfile, type MappingRequest, type MappingResult, type MappingValue, type RecordedMapping } from './mappings';
  let createSlot = $state(2);
  let createButtons = $state(32);
  let createHats = $state(1);
  let createAxes = $state<number[]>([0, 1, 2, 3, 4, 5, 6, 7]);
  const axisNames = ['X', 'Y', 'Z', 'Rx', 'Ry', 'Rz', 'Slider', 'Dial', 'Wheel', 'Accelerator', 'Brake', 'Clutch', 'Steering', 'Aileron', 'Rudder', 'Throttle'];
  let slotAssignments = $state<Record<string, number>>({});
  let profiles = $state<MappingProfile[]>([]);
  let inspectOffset = $state(0);
  let learnTarget = $state<{roleIndex: number; controlIndex: number} | null>(null);
  let learning = $state(false);
  let selected = $state('');
  let sources = $state<HostController[]>([]);
  let panels = $state<PanelMappingSource[]>([]);
  let companions = $state<CompanionMappingSource[]>([]);
  const companionOption = (key: string) => companions.find(source => `companion:${source.id}` === key);
  const sourceOrigin = (key: string) => companionOption(key) ? 'companion' as const : key.startsWith('panel-events:') ? 'osg-panel-events' as const : 'host-controller' as const;
  const nativeSourceId = (key: string) => companionOption(key)?.id ?? key;
  const sourceKey = (source: MappingAssignment['source']) => source.origin === 'companion' ? `companion:${source.id}` : source.id;
  let controls = $state<Record<string, ControllerControl[]>>({});
  let assignments = $state<MappingAssignment[]>([]);
  let status = $state<MappingResult>({});
  let virtual = $state<MappingResult | null>(null);
  let records = $state<RecordedMapping[]>([]);
  let cursor = '0';
  let error = $state('');
  let notice = $state('');
  let busy = $state(false);
  let name = $state('Direct mapping');
  let newSource = $state('synthetic');
  let newControl = $state('');
  let syntheticKind = $state<'button' | 'axis' | 'hat'>('button');
  let disposed = false;
  let polling = false;
  let lastCompanionRefresh = 0;
  const profile = $derived(profiles.find(p => p.id === selected));
  const creatorControls = $derived(controls[newSource]?.filter(c => capturedKind(c)) ?? []);
  const companionReadiness = $derived(profile ? companionRoleReadiness(profile, assignments, companions) : []);
  async function request(value: MappingRequest): Promise<MappingResult> {
    const result = (await gatewayRequest(value)).mappings;
    if (!result) throw new Error('Restart the native Cockpit service to use controller mappings.');
    return result;
  }
  async function exportTrace(){try{const bytes:number[]=[];let offset=0;for(;;){const page=await request({command:'mapping_trace_export',offset});if(!page.bytes || typeof page.next!=='number' || page.next<offset || page.next>4194304)throw new Error('Invalid trace chunk');bytes.push(...page.bytes);offset=page.next;if(offset===page.total)break;}const url=URL.createObjectURL(new Blob([new Uint8Array(bytes)],{type:'application/json'}));const link=document.createElement('a');link.href=url;link.download='cockpit-mapping-trace.json';link.click();URL.revokeObjectURL(url);}catch(e){error=String(e);}}
  async function loadControls(id: string) {
    if (controls[id]) return;
    if (sourceOrigin(id) === 'companion') {const detail=await request({command:'mapping_companion_inspect',sourceId:nativeSourceId(id)});if(!detail.companionControls)throw new Error('Companion controls are unavailable.');controls={...controls,[id]:detail.companionControls};return;}
    if (sourceOrigin(id) === 'osg-panel-events') {const detail=await request({command:'mapping_panel_inspect',sourceId:id});if(!detail.panelControls)throw new Error('Panel controls are unavailable.');controls={...controls,[id]:detail.panelControls};return;}
    const all: ControllerControl[] = [];
    for (let offset = 0; offset < 256; offset += 32) {
      const detail = (await gatewayRequest({ command: 'controller_inspect', sourceId: id, offset })).controllers;
      if (!detail?.controls) throw new Error('Controller capabilities are unavailable.');
      all.push(...detail.controls);
      if (offset + 32 >= detail.total) break;
    }
    controls = { ...controls, [id]: all };
  }
  async function loadSources() {
    const physical = async () => {
      const all: HostController[] = [];
      for (let offset = 0; offset < 256; offset += 32) {
        const result = (await gatewayRequest({ command: 'controller_list', offset })).controllers;
        if (!result?.sources) throw new Error('Physical controller discovery is unavailable.');
        all.push(...result.sources);
        if (offset + 32 >= result.total) break;
      }
      return all;
    };
    const results = await Promise.allSettled([physical(), request({command:'mapping_panel_sources'}), request({command:'mapping_companion_sources'})]);
    const [physicalResult,panelResult,companionResult]=results;
    sources=physicalResult.status==='fulfilled'?physicalResult.value:[];
    panels=panelResult.status==='fulfilled'?panelResult.value.panelSources??[]:[];
    companions=companionResult.status==='fulfilled'?companionResult.value.companionSources??[]:[];
    const unavailable=results.flatMap(result=>result.status==='rejected'?[result.reason instanceof Error?result.reason.message:String(result.reason)]:[]);
    if(unavailable.length)notice='Some source types are unavailable: '+unavailable.join(' ');
  }
  async function loadProfiles() {
    const all: MappingProfile[] = [];
    let offset: number | null = 0;
    while (offset !== null && all.length < 128) {
      const result = await request({ command: 'mapping_profiles', offset });
      for (const id of result.ids ?? []) {
        const item = await request({ command: 'mapping_export', profileId: id });
        if (item.document) all.push(item.document);
      }
      offset = typeof result.next === 'number' ? result.next : null;
    }
    profiles = all;
  }
  async function selectProfile(id: string) {
    if(learning){await request({command:'mapping_stop'});learning=false;learnTarget=null;}
    selected = id; assignments = []; notice = '';
    const item = profiles.find(p => p.id === id);
    if (!item) return;
    try { const saved = await savedAssignments(id, request); assignments = item.roles.map(role => { const local = saved.find(a => a.role === role.id); if (!local) throw new Error("Missing role assignment."); return { ...local, controls: role.controls.map(control => { const binding = local.controls.find(c => c.control === control.id); if (!binding) throw new Error("Missing control assignment."); return binding; }) }; }); }
    catch { assignments = item.roles.map(r => ({ role: r.id, source: { origin: r.origin, id: r.origin === 'synthetic' ? r.id : '' }, controls: r.controls.map(c => ({ control: c.id, sourceControl: r.origin === 'synthetic' ? c.id : '' })) })); notice = 'Assign each source and control, then save the local assignments.'; }
    const targets = await request({ command: 'mapping_targets_get', profileId: id });
    slotAssignments = Object.fromEntries((targets.bindings ?? []).map(binding => [binding.target, binding.slot]));
    for (const assignment of assignments) if (assignment.source.origin !== 'synthetic' && assignment.source.id) await loadControls(sourceKey(assignment.source));
  }
  async function action(operation: () => Promise<void>) {
    busy = true; error = ''; notice = '';
    try { await operation(); } catch (e) { error = e instanceof Error ? e.message : String(e); }
    finally { busy = false; }
    await refresh();
  }
  async function refresh() {
    if (disposed || polling || busy) return;
    polling = true;
    try {
      status = await request({ command: 'mapping_status', inspect: true, offset: inspectOffset });
      if (Date.now() - lastCompanionRefresh >= 1000) {
        const next = (await request({command:'mapping_companion_sources'})).companionSources ?? [];
        const cache = { ...controls };
        for (const source of next) if (companions.find(prior => prior.id === source.id)?.publicationHash !== source.publicationHash) delete cache[`companion:${source.id}`];
        controls = cache; companions = next; lastCompanionRefresh = Date.now();
      }
      if(learning && !status.learning) learning=false;
      if (learning && status.learning && status.learning.phase !== 'listening') {
        learning=false;notice=status.learning.reason;
        if(status.learning.candidates.length===1){
          const learned=status.learning.candidates[0];
          if(learnTarget && profile){const {roleIndex,controlIndex}=learnTarget;const native=controls[status.learning.sourceId]?.find(c=>c.capability.id===learned);if(native && compatibleControl(profile.roles[roleIndex].controls[controlIndex].kind,native)){assignments[roleIndex].controls[controlIndex].sourceControl=learned;}else notice='The learned control has a different kind. Choose a compatible control.';}
          else newControl=learned;
        }
      }
      for (let page = 0; page < 8; page++) {
        const result = await request({ command: 'mapping_recording', after: cursor });
        if (result.gap) notice = 'Older preview reports expired. This display is only a recording viewer.';
        const next = result.records ?? [];
        if (next.length) records = [...records, ...next].slice(-64);
        if (typeof result.next === 'string') cursor = result.next;
        if (next.length < 32) break;
      }
    } catch (e) { if (!disposed) error = e instanceof Error ? e.message : String(e); }
    finally { polling = false; }
  }
  async function create() {
    const native = creatorControls.find(c => c.capability.id === newControl);
    const kind: MappingKind | null = newSource === 'synthetic' ? syntheticKind === 'axis' ? { kind: 'axis', range: 'bipolar' } : { kind: syntheticKind } : native ? capturedKind(native) : null;
    if (!kind) throw new Error('Choose an input control.');
    const result = directProfile(name, kind, { origin: newSource === 'synthetic' ? 'synthetic' : sourceOrigin(newSource), id: newSource === 'synthetic' ? 'preview-input' : nativeSourceId(newSource) }, newSource === 'synthetic' ? 'control' : newControl);
    const saved = await importMapping(JSON.stringify(result.profile), request);
    await request({ command: 'mapping_assignments_save', profileId: saved.id, assignments: result.assignments });
    await loadProfiles(); await selectProfile(saved.id); notice = 'Profile and local assignment saved. Start a recording preview when ready.';
  }
  async function learn(id: string, target: {roleIndex:number;controlIndex:number} | null = null) {
    await request({command:'mapping_stop'});
    await loadControls(id);
    await request({command:'mapping_learn_start',sourceId:id});
    learnTarget=target;learning=true;notice='Move one control deliberately. Learning runs for up to 30 seconds and sends no output.';
  }
  async function createPreset() {
    const result=oneToOneProfile(name,{origin:sourceOrigin(newSource),id:nativeSourceId(newSource)},controls[newSource]??[]);
    const saved=await importMapping(JSON.stringify(result.profile),request);
    await request({command:'mapping_assignments_save',profileId:saved.id,assignments:result.assignments});
    await loadProfiles();await selectProfile(saved.id);
  }
  async function chooseSource(index: number, id: string) {
    if(learning){await request({command:'mapping_stop'});learning=false;learnTarget=null;}
    const item = assignments[index];
    assignments[index] = { ...item, source: { ...item.source, id }, controls: item.controls.map(c => ({ ...c, sourceControl: '' })) };
    if (id) await loadControls(sourceKey(assignments[index].source));
  }
  async function simulate(assignment: MappingAssignment, control: string, value: MappingValue) {
    if (!status.token) return;
    await request({ command: 'mapping_simulate', token: status.token, source: assignment.source, changes: [{ control, value }] });
  }
  onMount(() => {
    void action(async () => { await loadProfiles(); await loadSources(); });
    const timer = setInterval(() => void refresh(), 250);
    return () => { disposed = true; clearInterval(timer); };
  });
</script>

<div class="page-heading"><div><div class="eyebrow">YOUR INPUTS, YOUR LAYOUT</div><h1>Controller mappings</h1><p>Map companion instances independently, or assign physical, panel and synthetic inputs to ordinary mapping profiles.</p></div><span class="quiet-tag">Experimental</span></div>
{#if error}<Notification message={String(error)} kind="error" />{/if}
{#if notice}<Notification message={String(notice)} />{/if}
{#if status.storageError}<Notification message={'Saved mapping settings need repair: ' + String(status.storageError)} kind="error" />{/if}
<CompanionMappings send={request} sources={companions}/>
<section class="panel mapping-panel" aria-label="Create a direct mapping">
  <h2>New generic direct mapping</h2>
  <div class="mapping-row">
    <label>Name<input class="text-input" bind:value={name} maxlength="80" /></label>
    <label>Input source<select bind:value={newSource} onchange={() => action(async () => { newControl = ''; if (newSource !== 'synthetic') await loadControls(newSource); })}><option value="synthetic">Synthetic preview input</option>{#each sources.filter(s => s.online) as source}<option value={source.id}>{source.name}</option>{/each}{#each panels as panel}<option value={panel.id}>{panel.name ?? panel.moduleId} · panel events</option>{/each}{#each companions as companion}<option value={`companion:${companion.id}`}>{companionInstanceLabel(companion)} · companion{companion.online ? '' : ' (offline)'}</option>{/each}</select></label>
    {#if newSource === 'synthetic'}<label>Control<select bind:value={syntheticKind}><option value="button">Button</option><option value="axis">Bipolar axis</option><option value="hat">Hat</option></select></label>
    {:else}<label>Control<select bind:value={newControl}><option value="">Choose a control</option>{#each creatorControls as control}<option value={control.capability.id}>{controlLabel(control)} · {control.capability.reportId}:{control.capability.dataIndex}</option>{/each}</select></label>{/if}
    <button class="primary-button" disabled={busy || !name.trim() || (newSource !== 'synthetic' && !newControl)} onclick={() => action(create)}>Save direct mapping</button>
    <button class="secondary-button" disabled={busy} onclick={() => action(loadSources)}>Refresh sources</button>
    {#if newSource!=='synthetic'}<button class="secondary-button" disabled={busy || learning || sourceOrigin(newSource) !== 'host-controller'} onclick={()=>action(()=>learn(newSource))}>Learn input without output</button><button class="secondary-button" disabled={busy} onclick={()=>action(createPreset)}>Save one-to-one preset</button>{/if}
  </div>
  <label class="import-file">Import profile JSON<input type="file" accept=".json,application/json" disabled={busy} onchange={event => { const file = event.currentTarget.files?.[0]; if (file) void action(async () => { if (file.size > 65536) throw new Error('Mapping files are limited to 65536 bytes.'); const saved = await importMapping(await file.text(), request); await loadProfiles(); await selectProfile(saved.id); }); event.currentTarget.value = ''; }} /></label>
  <p class="muted">Generic presets are manual transformations. Use Companion controller mappings above to preserve a published panel’s complete authored targets and declared fallbacks. Saving and importing never start a mapping, change controller layouts, or hide devices.</p>
</section>
<section class="panel mapping-panel" aria-label="Mapping assignments">
  <div class="mapping-row"><h2>Saved profiles</h2><select aria-label="Saved mapping profile" value={selected} disabled={busy} onchange={event => action(() => selectProfile(event.currentTarget.value))}><option value="">Choose a profile</option>{#each profiles as item}<option value={item.id}>{item.name}</option>{/each}</select></div>
  {#if profile}
    <p>{profile.targets.length} logical targets · {profile.nodes.length} nodes</p>
    {#each profile.roles as role, index}
      {@const assigned = assignments[index]}
      {#if assigned}<fieldset><legend>{role.name} · {role.origin}</legend>
        {#if role.origin !== 'synthetic'}<label>Local source<select value={assigned.source.id} disabled={busy} onchange={event => action(() => chooseSource(index, event.currentTarget.value))}><option value="">Choose a controller</option>{#each (role.origin === 'companion' ? companions : role.origin === 'osg-panel-events' ? panels.map(p=>({id:p.id,name:p.name ?? p.moduleId,online:true})) : sources) as source}<option value={source.id}>{role.origin === 'companion' ? companionInstanceLabel(source) : source.name}{source.online ? '' : ' (offline)'}</option>{/each}</select></label>
        {:else}<p>Synthetic inputs are controlled below during a recording preview.</p>{/if}
        {#if role.origin === 'companion'}{@const readiness=companionReadiness.find(item=>item.role===role.id)}{#if readiness}<p class="muted">{readiness.state}: {readiness.reason}</p>{/if}{/if}
        <div class="assignment-controls">{#each role.controls as control, controlIndex}
          {#if role.origin !== 'synthetic'}<label>{control.id}<select aria-label={'Source control for ' + role.name + ' ' + control.id} bind:value={assignments[index].controls[controlIndex].sourceControl} disabled={busy}><option value="">Choose matching control</option>{#each (controls[sourceKey(assigned.source)] ?? []).filter(c => compatibleControl(control.kind, c)) as native}<option value={native.capability.id}>{controlLabel(native)} · {native.capability.reportId}:{native.capability.dataIndex}</option>{/each}</select></label><button class="secondary-button" disabled={busy || learning || !assigned.source.id || role.origin !== 'host-controller'} onclick={()=>action(()=>learn(assigned.source.id,{roleIndex:index,controlIndex}))}>Learn {control.id}</button>
          {:else if role.origin === 'synthetic' && status.running && status.profileId === profile.id}
            <div class="synthetic-control"><span>{control.id}</span>{#if control.kind.kind === 'button'}<button class="secondary-button" disabled={busy} onclick={() => action(() => simulate(assigned, assigned.controls[controlIndex].sourceControl, { kind: 'button', value: true }))}>Press</button><button class="secondary-button" disabled={busy} onclick={() => action(() => simulate(assigned, assigned.controls[controlIndex].sourceControl, { kind: 'button', value: false }))}>Release</button>
            {:else if control.kind.kind === 'axis'}<input aria-label={'Synthetic ' + control.id} type="range" min={control.kind.range === 'bipolar' ? -32768 : 0} max="32767" value="0" onchange={event => action(() => simulate(assigned, assigned.controls[controlIndex].sourceControl, { kind: 'axis', value: Number(event.currentTarget.value) }))} />
            {:else}<select aria-label={'Synthetic ' + control.id} onchange={event => action(() => simulate(assigned, assigned.controls[controlIndex].sourceControl, { kind: 'hat', value: event.currentTarget.value === '' ? null : Number(event.currentTarget.value) }))}><option value="">Neutral</option>{#each ['N','NE','E','SE','S','SW','W','NW'] as label, position}<option value={position}>{label}</option>{/each}</select>{/if}</div>
          {/if}
        {/each}</div>
      </fieldset>{/if}
    {/each}
    <div class="mapping-row"><button class="secondary-button" disabled={busy} onclick={() => action(async () => { await request({ command: 'mapping_assignments_save', profileId: profile.id, assignments }); notice = 'Local assignments saved.'; })}>Save assignments</button><button class="primary-button" disabled={busy} onclick={() => action(async () => { cursor = status.recordingCursor ?? '0'; records = []; await request({ command: 'mapping_preview_start', profileId: profile.id }); })}>Start recording preview</button></div>
  {:else}<p>Create or import a mapping profile to assign its inputs.</p>{/if}
</section>
{#if profile}{#key profile.id}<MappingEditor {profile} validate={text=>importMapping(text,request,true)} save={async draft=>{const saved=await importMapping(JSON.stringify(draft),request);await loadProfiles();notice='Draft saved. The running profile is unchanged until an explicit restart.';selected=saved.id;}}/>{/key}{/if}
<section class="panel mapping-panel" aria-label="Live mapping inspector">
  <h2>Raw, transformed and output inspector</h2>
  <button class="secondary-button" disabled={status.running} onclick={exportTrace}>Export completed replay trace</button>
  <p>Stop preview before export. Replay with <code>osg-logic replay cockpit-mapping-trace.json</code> to verify script results and output timing without controllers.</p>
  {#each status.scripts??[] as script}<p role="status"><strong>{script.id}</strong>: {script.fault??(script.inflight?'Evaluating':script.started?'Ready':'Waiting for baselines')} · {script.pending} queued · {script.timers} timers</p>{#each script.logs as log}<pre>{log}</pre>{/each}{/each}
  <p>Display refreshes four times per second. Native input, timing and output continue independently. Buttons combine with OR; the first available axis/hat writer in the listed priority wins.</p>
  {#if status.learning}<p role="status">Input learning: {status.learning.phase}. {status.learning.reason}</p>{#if status.learning.candidates.length>1}<p>Candidates: {status.learning.candidates.join(', ')}. Choose the intended control in its assignment.</p>{/if}{/if}
  {#if status.inspector}
    <div class="recording"><table><thead><tr><th>Rule</th><th>Raw / upstream</th><th>Transformed</th><th>Output</th><th>Contribution</th><th>Writer priority / OR</th><th>Next time (µs)</th></tr></thead><tbody>{#each status.inspector.nodes as node}<tr><td>{node.id}</td><td>{node.inputs.map(i=>i.raw ? 'raw '+i.raw.native+' / '+JSON.stringify(i.value) : JSON.stringify(i.value)).join(' · ')}</td><td>{JSON.stringify(node.value)}</td><td>{JSON.stringify(node.output)}</td><td>{JSON.stringify(node.contribution)}</td><td>{node.writers?.join(' → ')??'Intermediate'}</td><td>{node.nextUs??'—'}</td></tr>{/each}</tbody></table></div>
    <div class="mapping-row"><button class="secondary-button" disabled={inspectOffset===0} onclick={()=>{inspectOffset=Math.max(0,inspectOffset-32);void refresh();}}>Previous rules</button><span>{inspectOffset+1}–{Math.min(inspectOffset+32,status.inspector.total)} of {status.inspector.total}</span><button class="secondary-button" disabled={inspectOffset+32>=status.inspector.total} onclick={()=>{inspectOffset+=32;void refresh();}}>Next rules</button></div>
  {:else}<p>Start a recording preview to inspect rule ownership.</p>{/if}
</section>
<section class="panel mapping-panel" aria-label="Recording preview">
  <div class="mapping-row"><h2>Recording preview</h2><span class="quiet-tag">{status.running ? status.ready ? 'Running' : 'Waiting for baseline' : 'Stopped'}</span><button class="secondary-button" disabled={!status.running && !learning} onclick={() => { void request({ command: 'mapping_stop' }).then(() => refresh()).catch(e => { error = String(e); }); }}>Stop mapping</button></div>
  {#if status.fault}<p class="notice amber-notice">{status.fault}</p>{/if}
  <p class="muted">Panel events provide ordered button input and current axes. Hats are unavailable on this route.</p>
  <p class="muted">Preview reports do not send controls to games. The original controllers remain visible.</p>
  <div class="recording"><table><thead><tr><th>Report</th><th>Target</th><th>Reason</th><th>Pressed buttons</th><th>Axes</th><th>Hats</th></tr></thead><tbody>{#each records.slice().reverse() as item (item.cursor)}<tr><td>{item.report.sequence}</td><td>{item.report.target}</td><td>{item.report.reason}</td><td>{item.report.state.buttons.flatMap((pressed, i) => pressed ? [i + 1] : []).join(', ') || '—'}</td><td>{item.report.state.axes.join(', ') || '—'}</td><td>{item.report.state.hats.map(h => h === null ? 'neutral' : String(h)).join(', ') || '—'}</td></tr>{:else}<tr><td colspan="6">Start a preview and move an assigned control.</td></tr>{/each}</tbody></table></div>
</section>
<section class="panel mapping-panel" aria-label="Virtual controllers">
  <div class="mapping-row"><h2>Virtual controllers</h2><button class="secondary-button" disabled={busy} onclick={() => action(async () => { virtual = await request({ command: 'mapping_virtual_inspect' }); })}>Inspect {status.virtualDevices?.backend === 'uinput' ? 'Linux output' : status.virtualDevices?.backend === 'macos-hid' ? 'macOS helper' : 'vJoy'}</button></div>
  <OptionalDependencies only="vjoy" />
  <label class="enable-option"><input type="checkbox" checked={status.virtualDevices?.enabled ?? false} disabled={busy} onchange={event => action(async () => { await request({ command: 'mapping_virtual_enable', enabled: event.currentTarget.checked }); })} />Enable {status.virtualDevices?.backend === 'uinput' ? 'Linux virtual devices' : status.virtualDevices?.backend === 'macos-hid' ? 'macOS virtual devices' : 'installed vJoy'} for this computer</label>
  <p>{virtual?.reason ?? status.virtualDevices?.reason ?? 'Virtual output is optional and disabled by default.'}</p>
  {#each virtual?.slots ?? [] as slot}<div class="virtual-slot"><strong>vJoy slot {slot.id} · {slot.status}</strong><span>{slot.buttons} buttons · {slot.axes.length} axes · {slot.continuousHats} continuous hats · {slot.discreteHats} discrete hats{slot.forceFeedback ? ' · force feedback' : ''}{slot.ownerPid ? ' · owner PID ' + slot.ownerPid : ''}</span></div>{/each}
  {#if profile && ['uinput','macos-hid'].includes(status.virtualDevices?.backend ?? '')}
    <h3>{status.virtualDevices?.backend === 'uinput' ? 'Linux' : 'macOS'} device templates</h3>
    <p>Up to eight axes, 32 buttons and four hats per device. Templates are saved locally. Devices exist only during an explicitly started live session; game bindings after recreation still need qualification.</p>
    <div class="mapping-row">{#each profile.targets as target}<label>{target.name}<select bind:value={slotAssignments[target.id]}><option value={undefined}>Choose a local identity</option>{#each Array.from({length:16},(_,i)=>i+1) as slot}<option value={slot}>Controller {slot}</option>{/each}</select></label>{/each}
    <button class="secondary-button" disabled={busy || status.running || profile.targets.some(t => !slotAssignments[t.id] || t.layout.buttons > 32 || t.layout.axes.length > 8)} onclick={() => action(async () => { if (!profile) return; const bindings = profile.targets.map(target => ({target: target.id, slot: slotAssignments[target.id], axes: target.layout.axes.map((_,i)=>0x30+i), layout:target.layout})); await request({command:'mapping_targets_save',profileId:profile.id,bindings}); notice='Device templates saved. No device was created.'; })}>Save device templates</button></div>
  {:else if profile && virtual}
    <h3>Local target slots</h3>
    <div class="mapping-row">{#each profile.targets as target}<label>{target.name}<select bind:value={slotAssignments[target.id]}><option value={undefined}>Choose a matching slot</option>{#each virtual.slots ?? [] as slot}<option value={slot.id} disabled={slot.buttons !== target.layout.buttons || slot.axes.length !== target.layout.axes.length || slot.continuousHats !== target.layout.hats || slot.discreteHats !== 0 || slot.forceFeedback}>Slot {slot.id} · {slot.buttons} buttons, {slot.axes.length} axes, {slot.continuousHats} hats</option>{/each}</select></label>{/each}
    <button class="secondary-button" disabled={busy || profile.targets.some(t => !slotAssignments[t.id])} onclick={() => action(async () => { const bindings = exactTargetBindings(profile, slotAssignments, virtual?.slots ?? []); await request({ command: 'mapping_targets_save', profileId: profile.id, bindings }); notice = 'Local slots saved. Activation will recheck the actual layouts.'; })}>Save target slots</button></div>
  {/if}
  {#if status.outputRecovery?.required}
    <div class="notice" role="status">
      <strong>Interrupted virtual output needs attention.</strong>
      {#if status.outputRecovery.error}
        <p>{status.outputRecovery.error}</p>
      {:else}
        <p>Inspect output identities {status.outputRecovery.slots?.join(', ')} before starting another live mapping.
        {status.outputRecovery.phase === 'confirmed' ? 'The previous run confirmed ownership; recovery remains subject to driver qualification.' : 'The previous operation was interrupted before its outcome was confirmed. No output will be sent automatically.'}</p>
      {/if}
    </div>
  {/if}
  {#if status.provisioning}
    <div class="notice" role="status"><strong>Controller provisioning: {status.provisioning.state}</strong><p>{status.provisioning.message}</p>
      {#if status.provisioning.active}<button class="secondary-button" disabled={busy} onclick={() => action(async () => { await request({ command: 'mapping_provision_cancel', jobId: status.provisioning?.jobId }); })}>Cancel provisioning</button>{/if}
    </div>
  {/if}
  {#if status.provisioningRecovery?.error}<Notification message={String(status.provisioningRecovery.error)} />{/if}
  {#each status.provisioningRecovery?.pending ?? [] as operation}
    <div class="notice" role="status"><strong>Interrupted layout change for slot {operation.target}</strong>
      <p>{operation.refreshUncertain ? 'Driver refresh completion is uncertain. Inspect the driver before attempting recovery.' : 'The previous layout change needs verified recovery before creating another controller.'}</p>
    <button class="secondary-button" disabled={busy || !status.provisioningQualified || operation.refreshUncertain || status.provisioning?.active} onclick={() => action(async () => { await request({ command: 'mapping_provision_recover', operation: operation.operation }); })}>Recover previous layout</button>
    </div>
  {/each}
  {#if !['uinput','macos-hid'].includes(status.virtualDevices?.backend ?? '')}
  <fieldset disabled={busy || !status.provisioningQualified || status.provisioning?.active}>
      <legend>New controller layout</legend>
      <label>Buttons <input type="number" min="0" max="128" step="1" bind:value={createButtons} /></label>
      <label>Continuous hats <input type="number" min="0" max="4" step="1" bind:value={createHats} /></label>
      <div class="axis-options">{#each axisNames as axis, index}<label><input type="checkbox" bind:group={createAxes} value={index} />{axis}</label>{/each}</div>
      <small>Starter: eight axes, 32 buttons and one hat. Created layouts persist across mapping changes. Force feedback is unavailable.</small>
  </fieldset>
  {/if}
  <div class="mapping-row">
    {#if !['uinput','macos-hid'].includes(status.virtualDevices?.backend ?? '')}
    <label>New vJoy slot<select bind:value={createSlot} disabled={!status.provisioningQualified || busy}>{#each Array.from({ length: 16 }, (_, i) => i + 1) as slot}<option value={slot} disabled={virtual?.slots?.some(s => s.id === slot)}>Slot {slot}</option>{/each}</select></label>
    <button class="secondary-button" disabled={busy || !status.provisioningQualified || !status.virtualDevices?.enabled || status.provisioning?.active || status.running} title={status.provisioningQualified ? 'Create the selected persistent layout' : 'Windows layout validation is incomplete'} onclick={() => action(async () => { await request({ command: 'mapping_virtual_create', slot: createSlot, layout: { axes: createAxes.reduce((mask, index) => mask | (1 << index), 0), buttons: createButtons, hats: createHats } }); })}>Create virtual controller</button>
    {/if}
    <button class="secondary-button" disabled={busy || !profile || !status.virtualDevices?.qualified || status.provisioning?.active || status.outputRecovery?.required} title="Requires qualified native output" onclick={() => action(async () => { if (profile) await request({ command: 'mapping_virtual_start', profileId: profile.id }); })}>Start live output</button>
    <button class="secondary-button" disabled={busy || !status.virtualDevices?.qualified || status.running || !(status.outputRecovery?.required || status.recoveryRequired)} onclick={() => action(async () => { await request({ command: 'mapping_virtual_recover' }); })}>Recover virtual output</button>
    {#if status.outputRecovery?.token && (status.outputRecovery.phase === 'intent' || status.outputRecovery.phase === 'releasing')}
      <button class="secondary-button" disabled={busy || !status.virtualDevices?.qualified || status.running} onclick={() => action(async () => { await request({ command: 'mapping_virtual_acknowledge', token: status.outputRecovery?.token }); notice = 'Recovery record acknowledged. No virtual output was changed.'; })}>Acknowledge inspected output</button>
    {/if}
  </div>
</section>
<style>
  fieldset { margin: 16px 0; padding: 16px; border: 1px solid var(--line); border-radius: 6px; }
  .axis-options { display: grid; grid-template-columns: repeat(auto-fit, minmax(120px, 1fr)); gap: 8px; margin: 12px 0; }
  .axis-options label { display: flex; flex-direction: row; align-items: center; gap: 8px; min-width: 0; }
  .mapping-panel { padding: 24px; margin-bottom: 20px; }
  .mapping-panel h2 { margin: 0 0 12px; }
  .mapping-row { display: flex; flex-wrap: wrap; gap: 14px; align-items: end; margin-bottom: 16px; }
  .mapping-row h2 { margin-right: auto; }
  label { display: grid; gap: 7px; min-width: 160px; }
  select { max-width: 420px; }
  .muted { opacity: .7; font-size: .9rem; }
  .enable-option { display: flex; align-items: center; gap: 10px; margin-bottom: 12px; }
  .import-file { max-width: 480px; }
  fieldset { border: 1px solid var(--border, #2b3946); border-radius: 12px; padding: 16px; margin: 16px 0; }
  .assignment-controls { display: flex; gap: 14px; flex-wrap: wrap; margin-top: 14px; max-height: 360px; overflow: auto; }
  .synthetic-control { display: flex; gap: 10px; align-items: center; }
  .recording { max-height: 340px; overflow: auto; }
  table { width: 100%; border-collapse: collapse; text-align: left; font-size: .9rem; }
  th, td { padding: 10px; border-bottom: 1px solid var(--border, #2b3946); }
  .virtual-slot { display: grid; gap: 5px; padding: 12px 0; }
</style>
