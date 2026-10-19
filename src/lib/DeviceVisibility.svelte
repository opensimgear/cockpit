<script lang="ts">
  import Notification from './Notification.svelte';
  import { onMount } from 'svelte';
  import OptionalDependencies from './OptionalDependencies.svelte';
  import { gatewayRequest, type GatewayRequest } from './gateway';
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
<section class="visibility" aria-label="Device visibility">
  <h3>Device visibility</h3>
  <OptionalDependencies only="hidhide" />
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
    <div class="visibility-toggle"><strong>Enable optional visibility controls on this computer</strong><button class="toggle" class:on={status.enabled} role="switch" aria-label="Enable optional visibility controls on this computer" aria-checked={status.enabled} disabled={busy || status.job?.active} onclick={() => action({ command: 'controller_visibility_enable', enabled: !status?.enabled })}><span></span></button></div>
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
      <button class="secondary-button" disabled={busy || (!status.job?.active && (!status.qualified || !status.recovery.required || !status.recovery.id))} onclick={() => action({ command: 'controller_visibility_restore', jobId: status?.job?.jobId, sessionId: status?.recovery.id })}>Restore visibility</button>
    </div>
    {#if status.global.error}<p class="notice">Visibility setup: {status.global.error}</p>{/if}
    {#if status.global.inspected && !status.global.error}<p>Last inspected policy: cloaking {status.global.active ? 'on' : 'off'}, {status.global.inverse ? 'selected applications blocked' : 'listed applications exempt'}. {status.global.hiddenCount} configured hidden devices.</p>{/if}
    {#if status.effective}<p>This controller {status.effective.hidden ? 'has an active global HidHide rule' : status.effective.configured ? 'has an inactive HidHide rule' : 'has no configured HidHide rule'} in the inspected state. Visible does not remove externally managed rules.</p>{/if}
    {#if status.global.applications?.length}<details><summary>{status.global.inverse ? 'Blocked executable paths' : 'Existing application exceptions'} ({status.global.applicationCount})</summary><ul>{#each status.global.applications as path}<li>{path}</li>{/each}</ul>{#if (status.global.applicationCount ?? 0) > 16}<p>First 16 paths shown.</p>{/if}</details>{/if}
    {#if status.job}<p role="status">{status.job.phase}: {status.job.message}</p>{/if}
    {#if status.launch?.job?.active}
      <h4>Previous isolated application</h4>
      <p role="status">{status.launch.job.phase}: {status.launch.job.message}</p>
      <button class="secondary-button" disabled={busy} onclick={() => action({ command: 'controller_visibility_restore', jobId: status?.launch?.job?.jobId })}>Stop retained isolated application</button>
    {/if}
    {#if status.recovery.required && !status.job?.active}<p class="notice">An interrupted visibility operation needs explicit recovery. {status.recovery.error ?? 'No hiding is reapplied automatically.'}</p>{/if}
  {/if}
</section>
<style>
  .visibility { margin: 22px 0; padding: 18px; border: 1px solid var(--line); border-radius: 7px; background: var(--subtle); min-width: 0; }
  h3 { font-size: 12px; font-weight: 570; margin: 0 0 10px; }
  h4 { font-size: 11px; font-weight: 570; margin: 14px 0 8px; }
  p { font-size: 10px; line-height: 1.7; color: var(--muted); margin: 8px 0; overflow-wrap: anywhere; }
  .fields, .actions { display: flex; flex-wrap: wrap; gap: 12px; align-items: end; margin: 14px 0; }
  label { display: flex; flex-direction: column; gap: 8px; min-width: min(100%, 200px); flex: 1; font-size: 10px; color: var(--soft); }
  select, input { width: 100%; min-width: 0; padding: 10px 11px; font-size: 10px; color: var(--text); background: var(--input-bg); border: 1px solid var(--line); border-radius: 5px; }
  .visibility-toggle { display: flex; justify-content: space-between; align-items: center; gap: 16px; margin: 16px 0; padding: 14px 0; border-top: 1px solid var(--line); border-bottom: 1px solid var(--line); }
  .visibility-toggle strong { font-size: 10px; line-height: 1.6; font-weight: 500; color: var(--soft); }
  .hint { font-size: 9px; }
  .notice { display: block; font-size: 10px; line-height: 1.7; margin: 12px 0; background: var(--surface); }
  .startup-access { margin: 16px 0; padding: 12px 14px; background: var(--surface); border: 1px solid var(--line); border-radius: 6px; }
  .startup-access.attention { border-left: 3px solid var(--muted); }
  .startup-access p { margin: 6px 0; }
  .startup-access code { overflow-wrap: anywhere; }
  .startup-access strong { color: var(--soft); font-weight: 500; }
  details { margin: 14px 0; font-size: 10px; color: var(--soft); line-height: 1.7; }
  summary { cursor: pointer; }
  ul { padding-left: 18px; }
  li { overflow-wrap: anywhere; color: var(--muted); }
  code { font-size: 9px; }
  @media (max-width: 600px) { .visibility { padding: 14px; } .fields > button { width: 100%; } .actions > button { white-space: normal; } }
</style>
