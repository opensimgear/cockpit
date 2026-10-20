<script lang="ts">
  import { onMount } from 'svelte';
  import { invoke, isTauri } from '@tauri-apps/api/core';
  import OptionalDependencies from './OptionalDependencies.svelte';
  import { gatewayRequest } from './gateway';
  import { desktopStatus, type DesktopStatus } from './profile-desktop-runtime';
  import type { VisibilityStatus } from './visibility';
  import type { NativeSystemInventory } from './system-inventory-providers';

  let visibility = $state.raw<VisibilityStatus | null>(null);
  let desktop = $state.raw<DesktopStatus | null>(null);
  let inventory = $state.raw<NativeSystemInventory | null>(null);
  let busy = $state(''), error = $state('');
  let disposed = false;
  const nativeApp = isTauri();
  const virtualIssue = $derived(inventory?.diagnostics.find(message => /virtual|vjoy/i.test(message)));
  const virtualSlots = $derived(inventory?.virtualDevices ?? []);

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
      gatewayRequest({ command: 'profile_desktop_status' }).then(result => {
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
      const result = desktopStatus((await gatewayRequest({ command: 'profile_desktop_configure', enabled, scope: desktop.scope })).desktop);
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

<section class="panel output-settings" aria-label="Controller and desktop output settings">
  <div class="panel-heading"><h2>Controller and desktop output</h2><button class="text-button" disabled={!!busy} onclick={refresh}>Refresh output setup</button></div>
  <p>Set up this computer once, then choose mappings and device hiding in Profiles.</p>
  <div class="output-row">
    <div><h3>vJoy virtual controllers</h3><p>Virtual controller output is enabled on Windows. Install vJoy, configure its controller layout, then add a virtual controller block to a profile.</p>
      {#if inventory}
        <p role="status">{virtualIssue ?? (virtualSlots.length ? `${virtualSlots.length} configured virtual controller${virtualSlots.length === 1 ? '' : 's'} detected.` : 'No configured vJoy controllers detected. Set up vJoy and configure a controller layout.')}</p>
        {#if virtualSlots.length}<ul>{#each virtualSlots as slot (slot.id)}<li>{slot.name}: {slot.status}{slot.owner ? ` · ${slot.owner}` : ''}</li>{/each}</ul>{/if}
      {:else}<p>Refresh output setup to inspect the installed driver.</p>{/if}
    </div>
    {#if nativeApp}<button class="secondary-button" disabled={!!busy} onclick={() => manageDriver('open_vjoy')}>Configure vJoy</button>{/if}
  </div>
  <div class="output-row">
    <div><h3>HidHide device hiding</h3><p>Allow profiles to hide their physical controllers while running. Saved device policies remain in each controller’s visibility settings.</p>
      {#if visibility?.selfAccess}<p role="status">{visibility.selfAccess.message}</p>{/if}
      {#if visibility?.settingsError}<p class="error" role="alert">{visibility.settingsError}</p>{/if}
    </div>
    <label class="check"><input type="checkbox" checked={visibility?.enabled ?? false} disabled={!!busy || !visibility || !visibility.qualified || !!visibility.settingsError} onchange={event => enableVisibility(event.currentTarget.checked)} />Enable device hiding on this computer</label>
    {#if nativeApp}<button class="secondary-button" disabled={!!busy} onclick={() => manageDriver('open_hidhide')}>Manage HidHide</button>{/if}
  </div>
  <div class="output-row">
    <div><h3>Keyboard and mouse output</h3><p>Allow profiles to send keyboard shortcuts, mouse buttons and pointer movement. Choose the application scope in each profile’s keyboard and mouse block.</p>
      {#if desktop}<p role="status">{desktop.qualified ? (desktop.enabled ? 'Desktop output enabled.' : 'Desktop output disabled.') : desktop.reason}</p>{/if}
      {#if desktop?.error}<p class="error" role="alert">{desktop.error}</p>{/if}
    </div>
    <label class="check"><input type="checkbox" checked={desktop?.enabled ?? false} disabled={!!busy || !desktop || !desktop.qualified || !!desktop.error} onchange={event => enableDesktop(event.currentTarget.checked)} />Enable desktop output on this computer</label>
  </div>
  <OptionalDependencies />
  {#if !nativeApp}<p>Driver installers and the HidHide configuration app open from the native Cockpit desktop app.</p>{/if}
  {#if error}<p class="error" role="alert">{error}</p>{/if}
</section>

<style>
  .output-settings{padding:22px;margin-bottom:22px}.panel-heading{display:flex;align-items:center;justify-content:space-between;gap:14px}h2{font-size:12px}h3{font-size:11px;font-weight:550;margin:0 0 8px}p,li{font-size:10px;line-height:1.8;color:var(--muted)}.output-row{display:flex;flex-wrap:wrap;align-items:center;gap:16px;padding:18px 0;border-top:1px solid var(--line)}.output-row>div{flex:1;min-width:min(100%,280px)}.output-row p{margin:6px 0}.check{display:flex;align-items:center;gap:8px;font-size:10px}.check input{accent-color:var(--accent)}ul{padding-left:18px;margin:8px 0}.error{color:#d3a078}button{font-size:10px}
</style>
