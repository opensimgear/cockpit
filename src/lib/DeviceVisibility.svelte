<script lang="ts">
  import Notification from './Notification.svelte';
  import { onMount } from 'svelte';
  import OptionalDependencies from './OptionalDependencies.svelte';
  import { gatewayRequest, type GatewayRequest } from './gateway';
  import type { HostController } from './controllers';
  import type { VisibilityIntent, VisibilityStatus } from './visibility';
  let { source }: { source: HostController } = $props();
  let status = $state<VisibilityStatus | null>(null);
  let mode = $state<VisibilityIntent['mode']>('visible');
  let executable = $state('');
  let launchExecutable = $state('');
  let launchDirectory = $state('');
  let launchArguments = $state('');
  let processes = $state<{ pid: number; path: string }[]>([]);
  let nextProcess = $state<number | null>(null);
  let busy = $state(false);
  let error = $state('');
  let disposed = false;
  let polling = false;
  let loaded = false;
  async function request(value: GatewayRequest, updateIntent = false) {
    const result = (await gatewayRequest({ ...value, sourceId: source.id })).controllers?.visibility;
    if (!result) throw new Error('Restart the native service to use device visibility settings.');
    if (disposed) return;
    status = result;
    if (!loaded || updateIntent) { mode = result.intent.mode; executable = result.intent.mode === 'hide-from-process' ? result.intent.executable : ''; loaded = true; }
    if (result.processes) { processes = value.offset ? [...processes, ...result.processes] : result.processes; nextProcess = result.nextProcess ?? null; }
  }
  async function action(value: GatewayRequest) {
    busy = true;
    try { await request(value, value.command === 'controller_visibility_save'); error = ''; }
    catch (e) { if (!disposed) error = e instanceof Error ? e.message : String(e); }
    finally { busy = false; }
  }
  async function refresh() {
    if (busy || polling || disposed) return;
    polling = true;
    try { await request({ command: 'controller_visibility_status' }); }
    catch (e) { if (!disposed) error = e instanceof Error ? e.message : String(e); }
    finally { polling = false; }
  }
  onMount(() => { void refresh(); const timer = setInterval(refresh, 1000); return () => { disposed = true; clearInterval(timer); }; });
</script>
<section class="visibility">
  <h3>Device visibility</h3>
  <OptionalDependencies only="hidhide" />
  <p>Visibility and input suppression are independent of virtual controllers. Saving a selection does not apply it.</p>
  {#if error}<Notification message={String(error)} kind="error" />{/if}
  {#if status}
    <p class="notice">{status.reason}</p>
    {#if status.settingsError}<Notification message={'Saved settings need attention: ' + String(status.settingsError)} kind="error" />{/if}
    <label class="check"><input type="checkbox" checked={status.enabled} disabled={busy || status.job?.active} onchange={event => action({ command: 'controller_visibility_enable', enabled: event.currentTarget.checked })} />Enable optional visibility controls on this computer</label>
    <div class="fields">
      <label>Saved policy<select bind:value={mode} disabled={busy || status.job?.active}>{#each status.modes as option}<option value={option.mode} disabled={!option.supported}>{option.label}{option.supported ? '' : ' · unavailable'}</option>{/each}</select></label>
      {#if mode === 'hide-from-process'}
        <label>Executable path<input bind:value={executable} maxlength="4096" placeholder="C:\Games\Game.exe" disabled={busy || status.job?.active} /></label>
        <button class="secondary-button" disabled={busy} onclick={() => action({ command: 'controller_visibility_processes', offset: 0 })}>Choose a running application</button>
        {#if processes.length}<label>Running executables<select value={executable} onchange={event => executable = event.currentTarget.value}><option value="">Choose an executable path</option>{#each processes as process}<option value={process.path}>{process.path} (PID {process.pid})</option>{/each}</select></label>{/if}
        {#if nextProcess != null}<button class="secondary-button" disabled={busy} onclick={() => action({ command: 'controller_visibility_processes', offset: nextProcess ?? 0 })}>More applications</button>{/if}
      {/if}
    </div>
    <p class="hint">{status.modes.find(option => option.mode === mode)?.reason}</p>
    <div class="actions">
      <button class="secondary-button" disabled={busy || status.job?.active || (mode === 'hide-from-process' && !executable.trim())} onclick={() => action({ command: 'controller_visibility_save', intent: mode === 'hide-from-process' ? { mode, executable: executable.trim() } : { mode } })}>Save visibility selection</button>
      <button class="secondary-button" disabled={busy || status.job?.active} onclick={() => action({ command: 'controller_visibility_inspect' })}>{status.platform === 'windows' ? 'Inspect HidHide' : 'Inspect visibility capabilities'}</button>
      <button class="secondary-button" disabled={busy || !status.qualified || !status.enabled || status.job?.active || !source.online || !source.baselineReady || status.intent.mode === 'visible'} onclick={() => action({ command: 'controller_visibility_apply', sourceIds: [source.id] })}>Apply visibility</button>
      {#if status.selectedCount > 1}<button class="secondary-button" disabled={busy || !status.qualified || !status.enabled || status.job?.active || status.selectedCount > 16} onclick={() => action({ command: 'controller_visibility_apply', sourceIds: status?.selected ?? [] })}>Apply all saved selections</button>{/if}
      <button class="secondary-button" disabled={busy || (!status.job?.active && (!status.qualified || !status.recovery.required || !status.recovery.id))} onclick={() => action({ command: 'controller_visibility_restore', jobId: status?.job?.jobId, sessionId: status?.recovery.id })}>Restore visibility</button>
    </div>
    {#if status.global.error}<p class="notice">Visibility setup: {status.global.error}</p>{/if}
    {#if status.global.inspected && !status.global.error}<p>Last inspected policy: cloaking {status.global.active ? 'on' : 'off'}, {status.global.inverse ? 'selected applications blocked' : 'listed applications exempt'}. {status.global.hiddenCount} configured hidden devices.</p>{/if}
    {#if status.effective}<p>This controller {status.effective.hidden ? 'has an active global HidHide rule' : status.effective.configured ? 'has an inactive HidHide rule' : 'has no configured HidHide rule'} in the inspected state. Visible does not remove externally managed rules.</p>{/if}
    {#if status.global.applications?.length}<details><summary>{status.global.inverse ? 'Blocked executable paths' : 'Existing application exceptions'} ({status.global.applicationCount})</summary><ul>{#each status.global.applications as path}<li>{path}</li>{/each}</ul>{#if (status.global.applicationCount ?? 0) > 16}<p>First 16 paths shown.</p>{/if}</details>{/if}
    {#if status.job}<p role="status">{status.job.phase}: {status.job.message}</p>{/if}
    {#if status.platform === 'linux' && status.launch}
      <h4>Launch an isolated application</h4>
      <p class="notice">{status.launch.reason}</p>
      <p class="hint">The new application and its children receive a fixed device view with this controller and its sibling input interfaces excluded. Host applications keep their access. Network, desktop brokers and display sharing are unavailable; files in the application directory are read-only and session data is temporary. A device change stops the child and requires an explicit new launch.</p>
      <div class="fields">
        <label>Application directory<input bind:value={launchDirectory} maxlength="4096" placeholder="/opt/application" disabled={busy || status.launch.job?.active} /></label>
        <label>Executable<input bind:value={launchExecutable} maxlength="4096" placeholder="/opt/application/program" disabled={busy || status.launch.job?.active} /></label>
        <label>Arguments (one per line)<textarea bind:value={launchArguments} maxlength="16384" disabled={busy || status.launch.job?.active}></textarea></label>
      </div>
      <div class="actions">
        <button class="secondary-button" disabled={busy || !status.launch.qualified || !status.enabled || status.job?.active || status.launch.job?.active || !source.online || !source.baselineReady || !launchExecutable.trim() || !launchDirectory.trim()} onclick={() => action({ command: 'controller_visibility_launch', sourceIds: [source.id], launch: { launchKind: 'direct-child', executable: launchExecutable.trim(), directory: launchDirectory.trim(), arguments: launchArguments ? launchArguments.split('\n') : [] } })}>Launch isolated application</button>
        <button class="secondary-button" disabled={busy || !status.launch.job?.active} onclick={() => action({ command: 'controller_visibility_restore', jobId: status?.launch?.job?.jobId })}>Stop isolated application</button>
      </div>
      {#if status.launch.job}<p role="status">{status.launch.job.phase}: {status.launch.job.message}</p>{/if}
    {/if}
    {#if status.recovery.required && !status.job?.active}<p class="notice">An interrupted visibility operation needs explicit recovery. {status.recovery.error ?? 'No hiding is reapplied automatically.'}</p>{/if}
  {/if}
</section>
<style>
  .visibility { margin: 24px 0; padding: 20px; border: 1px solid var(--line); border-radius: 8px; }
  .fields, .actions { display: flex; flex-wrap: wrap; gap: 12px; align-items: end; margin: 14px 0; }
  label { display: flex; flex-direction: column; gap: 8px; min-width: 200px; flex: 1; }
  .check { flex-direction: row; align-items: center; }
  select, textarea, input:not([type=checkbox]) { padding: 9px; color: inherit; background: var(--input-bg); border: 1px solid var(--line); border-radius: 5px; }
  .hint { font-size: 12px; opacity: .7; }
  li { overflow-wrap: anywhere; }
</style>
