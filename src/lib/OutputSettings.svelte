<script lang="ts">
  import { onMount } from 'svelte';
  import { invoke, isTauri } from '@tauri-apps/api/core';
  import OptionalDependencies from './OptionalDependencies.svelte';
  import Icon from './Icon.svelte';
  import { gatewayRequest } from './gateway';
  import {guardUiSettings} from './ui-settings-guard';
  import { desktopStatus, type DesktopStatus } from './profile-desktop-runtime';
  import type { VisibilityStatus } from './visibility';
  import type { NativeSystemInventory } from './system-inventory-providers';

  const settingsRequest=guardUiSettings(gatewayRequest);
  let visibility = $state.raw<VisibilityStatus | null>(null);
  let desktop = $state.raw<DesktopStatus | null>(null);
  let inventory = $state.raw<NativeSystemInventory | null>(null);
  let busy = $state(''), error = $state('');
  let disposed = false;
  const nativeApp = isTauri();
  const vjoyDriver = $derived(inventory?.drivers?.vjoy);
  const hidHideDriver = $derived(inventory?.drivers?.hidhide);
  const virtualIssue = $derived(vjoyDriver?.error || inventory?.diagnostics.find(message => /virtual|vjoy/i.test(message)));
  const virtualSlots = $derived(inventory?.virtualDevices ?? []);

  async function changeEnablement(event: Event, current: () => boolean, save: (enabled: boolean) => Promise<void>) {
    const input = event.currentTarget as HTMLInputElement;
    await save(input.checked);
    input.checked = current();
  }

  async function refresh() {
    if (busy) return;
    busy = 'refresh'; error = '';
    const results = await Promise.allSettled([
      gatewayRequest({ command: 'system_inventory' }).then(result => {
        if (!result.systemInventory) throw new Error('Virtual controller inspection is unavailable.');
        if (!disposed) inventory = result.systemInventory;
      }),
      gatewayRequest({ command: 'controller_visibility_status' }).then(result => {
        if (!result.controllers?.visibility) throw new Error('Device hiding settings are unavailable.');
        if (!disposed) visibility = result.controllers.visibility;
      }),
      settingsRequest({ command: 'profile_desktop_status' }).then(result => {
        const status = desktopStatus(result.desktop);
        if (!disposed) desktop = status;
      }),
    ]);
    if (!disposed) {
      error = results.filter((result): result is PromiseRejectedResult => result.status === 'rejected').map(result => String(result.reason instanceof Error ? result.reason.message : result.reason)).join(' ');
      busy = '';
    }
  }
  async function enableVisibility(enabled: boolean) {
    if (busy || !visibility) return;
    busy = 'visibility'; error = '';
    try {
      const result = (await gatewayRequest({ command: 'controller_visibility_enable', enabled })).controllers?.visibility;
      if (!result) throw new Error('Device hiding settings are unavailable.');
      if (!disposed) visibility = result;
    } catch (reason) { if (!disposed) error = reason instanceof Error ? reason.message : String(reason); }
    finally { if (!disposed) busy = ''; }
  }
  async function enableDesktop(enabled: boolean) {
    if (busy || !desktop) return;
    busy = 'desktop'; error = '';
    try {
      const result = desktopStatus((await settingsRequest({ command: 'profile_desktop_configure', enabled, scope: desktop.scope })).desktop);
      if (!disposed) desktop = result;
    } catch (reason) { if (!disposed) error = reason instanceof Error ? reason.message : String(reason); }
    finally { if (!disposed) busy = ''; }
  }
  async function manageDriver(command: 'open_hidhide' | 'open_vjoy') {
    busy = command; error = '';
    try { await invoke(command); }
    catch (reason) { if (!disposed) error = reason instanceof Error ? reason.message : String(reason); }
    finally { if (!disposed) busy = ''; }
  }
  onMount(() => { void refresh(); return () => { disposed = true; }; });
</script>

<section class="output-settings" aria-label="Controller and desktop output settings">
  <header class="output-header">
    <div><h2>Output setup</h2><p>Set up this computer once. Choose mappings and device hiding in Profiles.</p></div>
    <button class="secondary-button" disabled={!!busy} onclick={refresh}><Icon name="refresh" size={14} />{busy === 'refresh' ? 'Refreshing…' : 'Refresh'}</button>
  </header>
  {#if error}<p class="setup-error" role="alert"><Icon name="alert" size={16} /><span>{error}</span></p>{/if}
  <section class="panel output-card" aria-label="Virtual controllers">
    <div class="card-heading">
      <span class="output-icon"><Icon name="gamepad" size={21} /></span>
      <div class="card-title"><h3>Virtual controllers</h3><span>vJoy{#if vjoyDriver?.installed && vjoyDriver.version}<span class="driver-version">{' · '}{vjoyDriver.version}</span>{/if}</span></div>
      <span class="setup-status" class:attention={!!virtualIssue} role="status">{inventory ? vjoyDriver?.installed === false ? 'Not installed' : virtualIssue ? 'Needs attention' : virtualSlots.length ? `${virtualSlots.length} configured` : 'Not configured' : busy === 'refresh' ? 'Checking…' : 'Not inspected'}</span>
    </div>
    <div class="card-body">
      <p>Publish virtual controller inputs on Windows. Configure a layout here, then add a virtual controller block to a profile.</p>
      {#if virtualIssue}<p class="status-note">{virtualIssue}</p>{/if}
      {#if inventory && vjoyDriver?.installed !== false && virtualSlots.length}
        <ul class="virtual-slots" aria-label="Configured virtual controllers">{#each virtualSlots as slot (slot.id)}<li><strong>{slot.name}</strong><span>{slot.status}{slot.owner ? ` · ${slot.owner}` : ''}</span></li>{/each}</ul>
      {:else if inventory && vjoyDriver?.installed !== false}<p class="status-note">No configured vJoy controllers detected. Configure a controller layout in the vJoy app.</p>{/if}
    </div>
    {#if nativeApp && inventory?.drivers?.supported && vjoyDriver?.installed === false}<div class="card-footer"><OptionalDependencies only="vjoy" disabled={!!busy} oncomplete={refresh} /></div>
    {:else if nativeApp}<div class="card-footer"><span>Open the vJoy configuration app. Windows may ask for administrator approval.</span><button class="secondary-button" disabled={!!busy} onclick={() => manageDriver('open_vjoy')}><Icon name="settings" size={14} />Configure</button></div>{/if}
  </section>
  <section class="panel output-card" aria-label="Device hiding">
    <div class="card-heading">
      <span class="output-icon"><Icon name="eye-off" size={21} /></span>
      <div class="card-title"><h3>Device hiding</h3><span>HidHide{#if hidHideDriver?.installed && hidHideDriver.version}<span class="driver-version">{' · '}{hidHideDriver.version}</span>{/if}</span></div>
      <label class="output-toggle"><span class="visually-hidden">Enable device hiding on this computer</span><input type="checkbox" role="switch" aria-describedby="device-hiding-help" checked={visibility?.enabled ?? false} disabled={!!busy || !visibility || !visibility.qualified || !!visibility.settingsError} onchange={event => changeEnablement(event, () => visibility?.enabled ?? false, enableVisibility)} /><span class="toggle" class:on={visibility?.enabled ?? false} aria-hidden="true"><span></span></span></label>
    </div>
    <div class="card-body">
      <p id="device-hiding-help">Allow profiles to hide their physical controllers while running. Saved device policies remain in each controller’s visibility settings.</p>
      {#if visibility?.selfAccess}<p class="status-note" role="status">{visibility.selfAccess.message}</p>{/if}
      {#if visibility && !visibility.qualified}<p class="status-note">{visibility.reason}</p>{/if}
      {#if visibility?.settingsError}<p class="setup-error" role="alert"><Icon name="alert" size={14} /><span>{visibility.settingsError}</span></p>{/if}
      {#if hidHideDriver?.error}<p class="status-note">{hidHideDriver.error}</p>{/if}
    </div>
    {#if nativeApp && inventory?.drivers?.supported && hidHideDriver?.installed === false}<div class="card-footer"><OptionalDependencies only="hidhide" disabled={!!busy} oncomplete={refresh} /></div>
    {:else if nativeApp}<div class="card-footer"><span>Inspect driver setup and existing device rules.</span><button class="secondary-button" disabled={!!busy} onclick={() => manageDriver('open_hidhide')}><Icon name="settings" size={14} />Manage</button></div>{/if}
  </section>
  <section class="panel output-card" aria-label="Keyboard and mouse">
    <div class="card-heading">
      <span class="output-icon"><Icon name="keyboard" size={21} /></span>
      <div class="card-title"><h3>Keyboard and mouse</h3><span>Desktop output</span></div>
      <label class="output-toggle"><span class="visually-hidden">Enable desktop output on this computer</span><input type="checkbox" role="switch" aria-describedby="desktop-output-help" checked={desktop?.enabled ?? false} disabled={!!busy || !desktop || !desktop.qualified || !!desktop.error} onchange={event => changeEnablement(event, () => desktop?.enabled ?? false, enableDesktop)} /><span class="toggle" class:on={desktop?.enabled ?? false} aria-hidden="true"><span></span></span></label>
    </div>
    <div class="card-body">
      <p id="desktop-output-help">Allow profiles to send keyboard shortcuts, mouse buttons and pointer movement. Choose the application scope in each profile’s keyboard and mouse block.</p>
      {#if desktop && !desktop.qualified}<p class="status-note">{desktop.reason}</p>{/if}
      {#if desktop?.error}<p class="setup-error" role="alert"><Icon name="alert" size={14} /><span>{desktop.error}</span></p>{/if}
    </div>
  </section>
  {#if !nativeApp}<p class="browser-note"><Icon name="info" size={14} /><span>Driver installers and configuration apps open from the native Cockpit desktop app.</span></p>{/if}
</section>

<style>
  .output-settings { display: grid; gap: 20px; margin-bottom: 24px; }
  .output-header { display: flex; align-items: flex-start; justify-content: space-between; gap: 20px; }
  h2 { font-size: 14px; font-weight: 570; margin: 0 0 8px; }
  .output-header p, .card-body p { color: var(--muted); font-size: 10px; line-height: 1.8; }
  .output-header button { flex-shrink: 0; }
  .card-heading { display: flex; align-items: center; gap: 12px; padding: 20px 22px 14px; }
  .output-icon { display: grid; place-items: center; width: 36px; height: 36px; flex-shrink: 0; border: 1px solid var(--line); border-radius: 8px; color: var(--accent); background: var(--input-bg); }
  .card-title { flex: 1; min-width: 0; }
  h3 { font-size: 12px; font-weight: 550; margin: 0 0 5px; }
  .card-title > span { color: var(--muted); font-size: 9px; }
  .driver-version { color: var(--soft); }
  .setup-status { display: inline-flex; align-items: center; gap: 6px; padding: 5px 8px; border: 1px solid var(--line); border-radius: 4px; color: var(--muted); background: var(--input-bg); font-size: 9px; white-space: nowrap; }
  .setup-status.attention { color: #d3a078; }
  .card-body { padding: 0 22px 20px; }
  .card-body .status-note { margin-top: 12px; color: var(--soft); }
  .virtual-slots { list-style: none; margin: 16px 0 0; padding: 0; border: 1px solid var(--line); border-radius: 5px; background: var(--input-bg); }
  .virtual-slots li { display: flex; align-items: center; justify-content: space-between; gap: 12px; padding: 10px 12px; font-size: 10px; line-height: 1.6; overflow-wrap: anywhere; }
  .virtual-slots li + li { border-top: 1px solid var(--line); }
  .virtual-slots strong { font-weight: 500; color: var(--soft); }
  .virtual-slots li > span { color: var(--muted); text-align: right; }
  .card-footer { display: flex; align-items: center; justify-content: space-between; gap: 16px; padding: 14px 22px; border-top: 1px solid var(--line); }
  .card-footer > span { color: var(--muted); font-size: 9px; line-height: 1.7; }
  .card-footer button { flex-shrink: 0; }
  .output-toggle { display: grid; position: relative; place-items: center; width: 32px; height: 32px; flex-shrink: 0; cursor: pointer; }
  .output-toggle input { position: absolute; inset: 0; width: 100%; height: 100%; margin: 0; opacity: 0; cursor: pointer; }
  .output-toggle .toggle { pointer-events: none; }
  .output-toggle input:focus-visible + .toggle { outline: 2px solid var(--accent); outline-offset: 4px; }
  .output-toggle input:disabled { cursor: not-allowed; }
  .output-toggle:has(input:disabled) .toggle { opacity: .45; }
  .visually-hidden { position: absolute; width: 1px; height: 1px; padding: 0; margin: -1px; overflow: hidden; clip-path: inset(50%); white-space: nowrap; }
  .setup-error, .browser-note { display: flex; align-items: flex-start; gap: 9px; font-size: 10px; line-height: 1.8; overflow-wrap: anywhere; }
  .setup-error { color: #d3a078; }
  .card-body .setup-error { margin-top: 12px; }
  .setup-error :global(svg), .browser-note :global(svg) { margin-top: 2px; flex-shrink: 0; }
  .browser-note { color: var(--muted); }
  @media(max-width:520px) { .output-header { flex-wrap: wrap; gap: 12px; } .card-heading { padding: 18px 16px 14px; gap: 9px; flex-wrap: wrap; } .output-icon { width: 30px; height: 30px; } .card-title { flex-basis: calc(100% - 48px); } .card-heading > .setup-status { margin-left: 39px; } .output-toggle { margin-left: auto; } .card-body { padding: 0 16px 18px; } .card-footer { flex-wrap: wrap; padding: 14px 16px; } .card-footer button { margin-left: auto; } .virtual-slots li { align-items: flex-start; flex-direction: column; gap: 4px; } .virtual-slots li > span { text-align: left; } }
</style>
