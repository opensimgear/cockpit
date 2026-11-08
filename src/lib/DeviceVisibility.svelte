<script lang="ts">
  import Icon from './Icon.svelte';
  import Notification from './Notification.svelte';
  import { onMount } from 'svelte';
  import { gatewayRequest, type GatewayRequest } from './gateway';
  import { SharedObservations } from './streams/shared';
  import { visibleObservation } from './visible-observation';
  import type { HostController } from './controllers';
  import type { HidHideSelfAccess, VisibilityIntent, VisibilityStatus } from './visibility';
  const startupAccessLabels: Record<HidHideSelfAccess['status'], string> = {
    configured: 'Configured automatically', unchanged: 'Already configured', absent: 'Not installed',
    unsupported: 'Unsupported installation', blocked: 'Blocked', error: 'Setup error', 'not-requested': 'Not requested',
  };
  let { source }: { source: HostController } = $props();
  let status = $state<VisibilityStatus | null>(null);
  let mode = $state<VisibilityIntent['mode']>('visible');
  let executable = $state('');
  let processes = $state<{ pid: number; path: string }[]>([]);
  let nextProcess = $state<number | null>(null);
  let busy = $state(false);
  let error = $state('');
  let disposed = false;
  let refreshing = false;
  let refreshPending = false;
  let requestSequence = 0;
  let visibilityEpoch = 0;
  let visibilityBaselineReady = false;
  let inventoryBaselineReady = false;
  function intentExecutable(intent: VisibilityIntent) {
    return intent.mode === 'hide-from-process' ? intent.executable : '';
  }
  async function request(value: GatewayRequest, updateIntent = false, passiveEpoch?: number) {
    const sequence = ++requestSequence;
    const current = () => !disposed && sequence === requestSequence &&
      (passiveEpoch === undefined || (!document.hidden && passiveEpoch === visibilityEpoch));
    let result: VisibilityStatus | undefined;
    try {
      result = (await gatewayRequest({ ...value, sourceId: source.id })).controllers?.visibility;
    } catch (reason) {
      if (current()) throw reason;
      return;
    }
    if (!current()) return;
    if (!result) throw new Error('Restart the native service to use device visibility settings.');
    const previous = status?.intent;
    status = result;
    if (!previous || updateIntent || (mode === previous.mode && executable === intentExecutable(previous))) {
      mode = result.intent.mode;
      executable = intentExecutable(result.intent);
    }
    if (result.processes) { processes = value.offset ? [...processes, ...result.processes] : result.processes; nextProcess = result.nextProcess ?? null; }
  }
  async function action(value: GatewayRequest) {
    busy = true;
    try { await request(value, value.command === 'controller_visibility_save'); error = ''; }
    catch (e) { if (!disposed) error = e instanceof Error ? e.message : String(e); }
    finally { busy = false; if (refreshPending) void refresh(); }
  }
  async function refresh() {
    if (refreshing) return;
    refreshing = true;
    try {
      while (refreshPending && visibilityBaselineReady && inventoryBaselineReady && !busy && !disposed && !document.hidden) {
        refreshPending = false;
        try { await request({ command: 'controller_visibility_status' }, false, visibilityEpoch); }
        catch (e) { if (!disposed) error = e instanceof Error ? e.message : String(e); }
      }
    } finally { refreshing = false; }
  }
  onMount(() => {
    const observations = new SharedObservations();
    const retire = () => {
      visibilityEpoch++;
      visibilityBaselineReady = false;
      inventoryBaselineReady = false;
      refreshPending = false;
    };
    const visibility = visibleObservation(observations, 'controllers.visibility', state => {
      if (state.status === 'ready' || state.status === 'lost' || state.status === 'error') {
        visibilityBaselineReady = true;
        refreshPending = true;
        void refresh();
      }
    }, retire);
    const inventory = visibleObservation(observations, 'system.inventory', state => {
      if (state.status === 'ready' || state.status === 'error' || state.status === 'lost') {
        inventoryBaselineReady = true;
        refreshPending = true;
        void refresh();
      }
    }, retire);
    visibility.setVisible(true);
    inventory.setVisible(true);
    return () => {
      disposed = true;
      visibility.close();
      inventory.close();
      observations.close();
    };
  });
</script>
<section class="visibility" aria-label="Device visibility">
  <h3>Device visibility</h3>
  <p>These are local visibility preferences and inspection tools. Saving a selection does not hide a device. Author and apply device hiding in Profiles.</p>
  {#if error}<Notification message={String(error)} kind="error" />{/if}
  {#if status}
    {#if status.selfAccess}
      <div class="startup-access" class:attention={['unsupported', 'blocked', 'error'].includes(status.selfAccess.status)} role="status" aria-label="Startup HidHide access" data-self-access-status={status.selfAccess.status}>
        <p><strong>Startup HidHide access: {startupAccessLabels[status.selfAccess.status] ?? 'Not confirmed'}</strong></p>
        <p>{status.selfAccess.message}</p>
        {#if status.selfAccess.ready}<p>Access was verified at startup.</p>{/if}
        {#if status.selfAccess.changed === null}<p>Startup access may have changed; its final state could not be verified.</p>{/if}
        {#if status.selfAccess.executable}<p class="hint">Capture executable: <code>{status.selfAccess.executable}</code></p>{/if}
        <p class="hint">Startup result only; later HidHide changes can affect capture. Hidden-device rules are separate from Cockpit access.</p>
      </div>
    {/if}
    <p class="notice">{status.reason}</p>
    {#if status.settingsError}<Notification message={'Saved settings need attention: ' + String(status.settingsError)} kind="error" />{/if}
    <p>Device hiding is {status.enabled ? 'enabled' : 'disabled'} on this computer. Manage HidHide setup in Settings.</p>
    <div class="fields">
      <label>Saved policy<select bind:value={mode} disabled={busy || status.job?.active}>{#each status.modes as option}<option value={option.mode} disabled={!option.supported}>{option.label}{option.supported ? '' : ' · unavailable'}</option>{/each}</select></label>
      {#if mode === 'hide-from-process'}
        <label>Executable path<input bind:value={executable} maxlength="4096" placeholder="C:\Games\Game.exe" disabled={busy || status.job?.active} /></label>
        <button class="secondary-button" disabled={busy} onclick={() => action({ command: 'controller_visibility_processes', offset: 0 })}><Icon name="search" size={14} />Choose a running application</button>
        {#if processes.length}<label>Running executables<select value={executable} onchange={event => executable = event.currentTarget.value}><option value="">Choose an executable path</option>{#each processes as process}<option value={process.path}>{process.path} (PID {process.pid})</option>{/each}</select></label>{/if}
        {#if nextProcess != null}<button class="secondary-button" disabled={busy} onclick={() => action({ command: 'controller_visibility_processes', offset: nextProcess ?? 0 })}><Icon name="plus" size={14} />More applications</button>{/if}
      {/if}
    </div>
    <p class="hint">{status.modes.find(option => option.mode === mode)?.reason}</p>
    <div class="actions">
      <button class="secondary-button" disabled={busy || status.job?.active || (mode === 'hide-from-process' && !executable.trim())} onclick={() => action({ command: 'controller_visibility_save', intent: mode === 'hide-from-process' ? { mode, executable: executable.trim() } : { mode } })}><Icon name="save" size={14} />Save visibility selection</button>
      <button class="secondary-button" disabled={busy || status.job?.active} onclick={() => action({ command: 'controller_visibility_inspect' })}><Icon name="search" size={14} />{status.platform === 'windows' ? 'Inspect HidHide' : 'Inspect visibility capabilities'}</button>
      <button class="secondary-button" disabled={busy || (!status.job?.active && (!status.qualified || !status.recovery.required || !status.recovery.id))} onclick={() => action({ command: 'controller_visibility_restore', jobId: status?.job?.jobId, sessionId: status?.recovery.id })}><Icon name="refresh" size={14} />Restore visibility</button>
    </div>
    {#if status.global.error}<p class="notice">Visibility setup: {status.global.error}</p>{/if}
    {#if status.global.inspected && !status.global.error}<p>Last inspected policy: cloaking {status.global.active ? 'on' : 'off'}, {status.global.inverse ? 'selected applications blocked' : 'listed applications exempt'}. {status.global.hiddenCount} configured hidden devices.</p>{/if}
    {#if status.effective}<p>This controller {status.effective.hidden ? 'has an active global HidHide rule' : status.effective.configured ? 'has an inactive HidHide rule' : 'has no configured HidHide rule'} in the inspected state. Visible does not remove externally managed rules.</p>{/if}
    {#if status.global.applications?.length}<details><summary>{status.global.inverse ? 'Blocked executable paths' : 'Existing application exceptions'} ({status.global.applicationCount})</summary><ul>{#each status.global.applications as path}<li>{path}</li>{/each}</ul>{#if (status.global.applicationCount ?? 0) > 16}<p>First 16 paths shown.</p>{/if}</details>{/if}
    {#if status.job}<p role="status">{status.job.phase}: {status.job.message}</p>{/if}
    {#if status.launch?.job?.active}
      <h4>Previous isolated application</h4>
      <p role="status">{status.launch.job.phase}: {status.launch.job.message}</p>
      <button class="secondary-button" disabled={busy} onclick={() => action({ command: 'controller_visibility_restore', jobId: status?.launch?.job?.jobId })}><Icon name="close" size={14} />Stop retained isolated application</button>
    {/if}
    {#if status.recovery.required && !status.job?.active}<p class="notice">An interrupted visibility operation needs explicit recovery. {status.recovery.error ?? 'No hiding is reapplied automatically.'}</p>{/if}
  {/if}
</section>
<style>
  .visibility { margin: var(--ui-section-gap) 0; padding: var(--ui-panel-padding); border: 1px solid var(--line); border-radius: 7px; background: var(--subtle); min-width: 0; }
  h3 { font-size: var(--ui-heading-size); font-weight: 570; margin: 0 0 10px; }
  h4 { font-size: var(--ui-heading-size); font-weight: 570; margin: 14px 0 8px; }
  p { font-size: var(--ui-text-size); line-height: var(--ui-line-height); color: var(--muted); margin: 8px 0; overflow-wrap: anywhere; }
  .fields, .actions { display: flex; flex-wrap: wrap; gap: 12px; align-items: end; margin: 14px 0; }
  label { display: flex; flex-direction: column; gap: 8px; min-width: min(100%, 200px); flex: 1; font-size: var(--ui-text-size); color: var(--soft); }
  select, input { min-height: var(--ui-control-height); width: 100%; min-width: 0; padding: 10px 11px; font-size: var(--ui-text-size); color: var(--text); background: var(--input-bg); border: 1px solid var(--line); border-radius: 5px; }
  .hint { font-size: var(--ui-text-size); }
  .notice { display: block; font-size: var(--ui-text-size); line-height: var(--ui-line-height); margin: 12px 0; background: var(--surface); }
  .startup-access { margin: 16px 0; padding: 12px 14px; background: var(--surface); border: 1px solid var(--line); border-radius: 6px; }
  .startup-access.attention { border-left: 3px solid var(--muted); }
  .startup-access p { margin: 6px 0; }
  .startup-access code { overflow-wrap: anywhere; }
  .startup-access strong { color: var(--soft); font-weight: 500; }
  details { margin: 14px 0; font-size: var(--ui-text-size); color: var(--soft); line-height: var(--ui-line-height); }
  summary { cursor: pointer; }
  ul { padding-left: 18px; }
  li { overflow-wrap: anywhere; color: var(--muted); }
  code { font-size: var(--ui-text-size); }
  @media (max-width: 600px) { .visibility { padding: 14px; } .fields > button { width: 100%; } .actions > button { white-space: normal; } }
</style>
