<script lang="ts">
  import { onMount } from 'svelte';
  import { gatewayRequest } from './gateway';
  import { controlLabel, type ControllerControl, type HostController } from './controllers';
  import { capturedKind, compatibleControl, directProfile, importMapping, savedAssignments, type MappingAssignment, type MappingKind, type MappingProfile, type MappingRequest, type MappingResult, type MappingValue, type RecordedMapping } from './mappings';
  let slotAssignments = $state<Record<string, number>>({});
  let profiles = $state<MappingProfile[]>([]);
  let selected = $state('');
  let sources = $state<HostController[]>([]);
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
  const profile = $derived(profiles.find(p => p.id === selected));
  const creatorControls = $derived(controls[newSource]?.filter(c => capturedKind(c)) ?? []);
  async function request(value: MappingRequest): Promise<MappingResult> {
    const result = (await gatewayRequest(value)).mappings;
    if (!result) throw new Error('Restart the native Cockpit service to use controller mappings.');
    return result;
  }
  async function loadControls(id: string) {
    if (controls[id]) return;
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
    const all: HostController[] = [];
    for (let offset = 0; offset < 256; offset += 32) {
      const result = (await gatewayRequest({ command: 'controller_list', offset })).controllers;
      if (!result?.sources) throw new Error('Native controller discovery is unavailable.');
      all.push(...result.sources);
      if (offset + 32 >= result.total) break;
    }
    sources = all;
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
    selected = id; assignments = []; notice = '';
    const item = profiles.find(p => p.id === id);
    if (!item) return;
    try { const saved = await savedAssignments(id, request); assignments = item.roles.map(role => { const local = saved.find(a => a.role === role.id); if (!local) throw new Error("Missing role assignment."); return { ...local, controls: role.controls.map(control => { const binding = local.controls.find(c => c.control === control.id); if (!binding) throw new Error("Missing control assignment."); return binding; }) }; }); }
    catch { assignments = item.roles.map(r => ({ role: r.id, source: { origin: r.origin, id: r.origin === 'synthetic' ? r.id : '' }, controls: r.controls.map(c => ({ control: c.id, sourceControl: r.origin === 'synthetic' ? c.id : '' })) })); notice = 'Assign each source and control, then save the local assignments.'; }
    const targets = await request({ command: 'mapping_targets_get', profileId: id });
    slotAssignments = Object.fromEntries((targets.bindings ?? []).map(binding => [binding.target, binding.slot]));
    for (const assignment of assignments) if (assignment.source.origin === 'host-controller' && assignment.source.id) await loadControls(assignment.source.id);
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
      status = await request({ command: 'mapping_status' });
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
    const result = directProfile(name, kind, { origin: newSource === 'synthetic' ? 'synthetic' : 'host-controller', id: newSource === 'synthetic' ? 'preview-input' : newSource }, newSource === 'synthetic' ? 'control' : newControl);
    const saved = await importMapping(JSON.stringify(result.profile), request);
    await request({ command: 'mapping_assignments_save', profileId: saved.id, assignments: result.assignments });
    await loadProfiles(); await selectProfile(saved.id); notice = 'Profile and local assignment saved. Start a recording preview when ready.';
  }
  async function chooseSource(index: number, id: string) {
    const item = assignments[index];
    assignments[index] = { ...item, source: { ...item.source, id }, controls: item.controls.map(c => ({ ...c, sourceControl: '' })) };
    if (id) await loadControls(id);
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

<div class="page-heading"><div><div class="eyebrow">YOUR INPUTS, YOUR LAYOUT</div><h1>Controller mappings</h1><p>Assign physical controllers or synthetic inputs, then preview complete output reports.</p></div><span class="quiet-tag">Experimental</span></div>
{#if error}<p class="notice amber-notice" role="alert">{error}</p>{/if}
{#if notice}<p class="notice" role="status">{notice}</p>{/if}
{#if status.storageError}<p class="notice amber-notice" role="alert">Saved mapping settings need repair: {status.storageError}</p>{/if}
<section class="panel mapping-panel" aria-label="Create a direct mapping">
  <h2>New direct mapping</h2>
  <div class="mapping-row">
    <label>Name<input class="text-input" bind:value={name} maxlength="80" /></label>
    <label>Input source<select bind:value={newSource} onchange={() => action(async () => { newControl = ''; if (newSource !== 'synthetic') await loadControls(newSource); })}><option value="synthetic">Synthetic preview input</option>{#each sources.filter(s => s.online) as source}<option value={source.id}>{source.name}</option>{/each}</select></label>
    {#if newSource === 'synthetic'}<label>Control<select bind:value={syntheticKind}><option value="button">Button</option><option value="axis">Bipolar axis</option><option value="hat">Hat</option></select></label>
    {:else}<label>Control<select bind:value={newControl}><option value="">Choose a control</option>{#each creatorControls as control}<option value={control.capability.id}>{controlLabel(control)} · {control.capability.reportId}:{control.capability.dataIndex}</option>{/each}</select></label>{/if}
    <button class="primary-button" disabled={busy || !name.trim() || (newSource !== 'synthetic' && !newControl)} onclick={() => action(create)}>Save direct mapping</button>
    <button class="secondary-button" disabled={busy} onclick={() => action(loadSources)}>Refresh sources</button>
  </div>
  <label class="import-file">Import profile JSON<input type="file" accept=".json,application/json" disabled={busy} onchange={event => { const file = event.currentTarget.files?.[0]; if (file) void action(async () => { if (file.size > 65536) throw new Error('Mapping files are limited to 65536 bytes.'); const saved = await importMapping(await file.text(), request); await loadProfiles(); await selectProfile(saved.id); }); event.currentTarget.value = ''; }} /></label>
  <p class="muted">Saving and importing never start a mapping, change controller layouts, or hide devices.</p>
</section>
<section class="panel mapping-panel" aria-label="Mapping assignments">
  <div class="mapping-row"><h2>Saved profiles</h2><select aria-label="Saved mapping profile" value={selected} disabled={busy} onchange={event => action(() => selectProfile(event.currentTarget.value))}><option value="">Choose a profile</option>{#each profiles as item}<option value={item.id}>{item.name}</option>{/each}</select></div>
  {#if profile}
    <p>{profile.targets.length} logical targets · {profile.nodes.length} nodes</p>
    {#each profile.roles as role, index}
      {@const assigned = assignments[index]}
      {#if assigned}<fieldset><legend>{role.name} · {role.origin}</legend>
        {#if role.origin === 'host-controller'}<label>Local controller<select value={assigned.source.id} disabled={busy} onchange={event => action(() => chooseSource(index, event.currentTarget.value))}><option value="">Choose a controller</option>{#each sources as source}<option value={source.id}>{source.name}{source.online ? '' : ' (offline)'}</option>{/each}</select></label>
        {:else}<p>{role.origin === 'synthetic' ? 'Synthetic inputs are controlled below during a recording preview.' : 'Panel-event sources await game-integration qualification.'}</p>{/if}
        <div class="assignment-controls">{#each role.controls as control, controlIndex}
          {#if role.origin === 'host-controller'}<label>{control.id}<select aria-label={'Source control for ' + role.name + ' ' + control.id} bind:value={assignments[index].controls[controlIndex].sourceControl} disabled={busy}><option value="">Choose matching control</option>{#each (controls[assigned.source.id] ?? []).filter(c => compatibleControl(control.kind, c)) as native}<option value={native.capability.id}>{controlLabel(native)} · {native.capability.reportId}:{native.capability.dataIndex}</option>{/each}</select></label>
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
<section class="panel mapping-panel" aria-label="Recording preview">
  <div class="mapping-row"><h2>Recording preview</h2><span class="quiet-tag">{status.running ? status.ready ? 'Running' : 'Waiting for baseline' : 'Stopped'}</span><button class="secondary-button" disabled={!status.running} onclick={() => { void request({ command: 'mapping_stop' }).then(() => refresh()).catch(e => { error = String(e); }); }}>Stop mapping</button></div>
  {#if status.fault}<p class="notice amber-notice">{status.fault}</p>{/if}
  <p class="muted">Preview reports do not send controls to games. The original controllers remain visible.</p>
  <div class="recording"><table><thead><tr><th>Report</th><th>Target</th><th>Reason</th><th>Pressed buttons</th><th>Axes</th><th>Hats</th></tr></thead><tbody>{#each records.slice().reverse() as item (item.cursor)}<tr><td>{item.report.sequence}</td><td>{item.report.target}</td><td>{item.report.reason}</td><td>{item.report.state.buttons.flatMap((pressed, i) => pressed ? [i + 1] : []).join(', ') || '—'}</td><td>{item.report.state.axes.join(', ') || '—'}</td><td>{item.report.state.hats.map(h => h === null ? 'neutral' : String(h)).join(', ') || '—'}</td></tr>{:else}<tr><td colspan="6">Start a preview and move an assigned control.</td></tr>{/each}</tbody></table></div>
</section>
<section class="panel mapping-panel" aria-label="Virtual controllers">
  <div class="mapping-row"><h2>Virtual controllers</h2><button class="secondary-button" disabled={busy} onclick={() => action(async () => { virtual = await request({ command: 'mapping_virtual_inspect' }); })}>Inspect vJoy</button></div>
  <label class="enable-option"><input type="checkbox" checked={status.virtualDevices?.enabled ?? false} disabled={busy} onchange={event => action(async () => { await request({ command: 'mapping_virtual_enable', enabled: event.currentTarget.checked }); })} />Enable installed vJoy for this computer</label>
  <p>{virtual?.reason ?? status.virtualDevices?.reason ?? 'Virtual output is optional and disabled by default.'}</p>
  {#each virtual?.slots ?? [] as slot}<div class="virtual-slot"><strong>vJoy slot {slot.id} · {slot.status}</strong><span>{slot.buttons} buttons · {slot.axes.length} axes · {slot.continuousHats} continuous hats · {slot.discreteHats} discrete hats{slot.forceFeedback ? ' · force feedback' : ''}{slot.ownerPid ? ' · owner PID ' + slot.ownerPid : ''}</span></div>{/each}
  {#if profile && virtual}
    <h3>Local target slots</h3>
    <div class="mapping-row">{#each profile.targets as target}<label>{target.name}<select bind:value={slotAssignments[target.id]}><option value={undefined}>Choose a matching slot</option>{#each virtual.slots ?? [] as slot}<option value={slot.id} disabled={slot.buttons !== target.layout.buttons || slot.axes.length !== target.layout.axes.length || slot.continuousHats !== target.layout.hats || slot.discreteHats !== 0 || slot.forceFeedback}>Slot {slot.id} · {slot.buttons} buttons, {slot.axes.length} axes, {slot.continuousHats} hats</option>{/each}</select></label>{/each}
    <button class="secondary-button" disabled={busy || profile.targets.some(t => !slotAssignments[t.id])} onclick={() => action(async () => { const bindings = profile.targets.map(target => { const slot = virtual?.slots?.find(s => s.id === slotAssignments[target.id]); if (!slot) throw new Error('Inspect the selected slot first.'); return { target: target.id, slot: slot.id, axes: slot.axes.map(a => a.usage), layout: target.layout }; }); await request({ command: 'mapping_targets_save', profileId: profile.id, bindings }); notice = 'Local slots saved. Activation will recheck the actual layouts.'; })}>Save target slots</button></div>
  {/if}
  <div class="mapping-row"><button class="secondary-button" disabled title="Windows layout validation is incomplete">Create virtual controller</button><button class="secondary-button" disabled title="Windows output validation is incomplete">Start live output</button><button class="secondary-button" disabled title="Windows recovery validation is incomplete">Recover virtual output</button></div>
</section>
<style>
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
