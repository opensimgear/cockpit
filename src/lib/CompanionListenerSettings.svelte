<script lang="ts">
  import { invoke } from '@tauri-apps/api/core';
  import Icon from './Icon.svelte';
  import { notify } from './notifications';
  import Notification from './Notification.svelte';
  import { onMount } from 'svelte';
  import { gatewayRequest } from './gateway';
  import type { CompanionHostState } from './companions';
  let hostState = $state<CompanionHostState>({});
  let address = $state('');
  let port = $state(54437);
  let discovery = $state(true);
  let busy = $state(false);
  let error = $state('');
  let loaded = $state(false);
  let polling = false;
  let disposed = false;
  let requestVersion=0;
  const enabled=$derived(hostState.listener?.configuredEnabled??hostState.listener?.enabled??false);
  const portError = $derived(!Number.isInteger(port) || port < 1024 || port > 65535 ? 'Enter a port from 1024 to 65535.' : '');
  async function exportCertificate() {
    try { if(await invoke('export_companion_certificate'))notify('Public certificate exported. Verify the fingerprint before trusting it on another device.'); }
    catch(e){notify(String(e),'error');}
  }
  async function refresh() {
    if (busy || polling) return;
    polling = true;
    const version=requestVersion;
    try {
      const result=(await gatewayRequest({ command: 'companion_inventory' })).companions ?? {};
      if(disposed || version!==requestVersion || busy)return;
      if (!result.listener) throw new Error('Companion listener settings are unavailable.');
      hostState=result;
      if(!loaded){
        const config=result.listener.config;
        if(config){address=config.advertisedHost;port=Number(config.bind.split(':').at(-1))||54437;discovery=config.discovery;}
        if (!address) address = hostState.interfaces?.find(ip => ip !== '127.0.0.1' && !ip.includes(':')) ?? '127.0.0.1';
        loaded=true;
      }
    } catch (e) { if (!disposed && version === requestVersion) error = String(e); }
    finally { polling = false; }
  }
  async function toggle(enabled: boolean) {
    if (!loaded || busy || enabled && (!address || portError)) return;
    requestVersion++;busy = true; error = '';
    try {
      const expectedSettingsRevision=hostState.listener?.settingsRevision;
      if(!Number.isSafeInteger(expectedSettingsRevision))throw new Error('Reload listener settings before saving.');
      const result = (await gatewayRequest({ command: enabled ? 'companion_enable' : 'companion_disable',
        companion: { expectedSettingsRevision, ...(enabled ? { address, advertisedHost: address, port, discovery } : {}) } })).companions;
      if (disposed) return;
      if (!result?.listener) throw new Error('Companion listener settings could not be saved.');
      hostState = result;
      notify('Preference saved.');
    } catch (e) { if (!disposed) error = String(e); }
    finally { if (!disposed) busy = false; }
  }
  onMount(() => { void refresh(); const timer = setInterval(refresh, 1500); return () => { disposed = true; clearInterval(timer); }; });
</script>

<section class="panel preferences-panel listener-settings" aria-label="Companion listener settings">
  <div class="panel-heading"><h2>Companion listener</h2><span class="quiet-tag" role="status">{busy ? 'Saving…' : !loaded ? 'Loading…' : hostState.listener?.enabled ? 'Listening' : enabled ? 'Waiting to start' : 'Disabled'}</span></div>
  <div class="preference-row">
    <div><strong>Enable companion listener</strong><p>Allow paired Wi-Fi and network companions to connect. Direct USB companions work with this disabled.</p></div>
    <button class="toggle" class:on={enabled} role="switch" aria-label="Enable companion listener" aria-checked={enabled} disabled={busy || !loaded || !enabled && (!address || !!portError)} onclick={() => toggle(!enabled)}><span></span></button>
  </div>
  <div class="preference-row connection-row">
    <div class="connection-copy"><strong>Network connection</strong><p id="listener-connection-help">{enabled ? 'Turn off the listener to change its address, port, or LAN discovery.' : 'Choose the address companions can reach. Network settings are saved when you enable the listener.'}</p></div>
    <div class="connection-fields">
      <label><span class="field-label">LAN address</span><select aria-describedby="listener-connection-help" bind:value={address} disabled={busy || !loaded || enabled}>{#each [...new Set([address, ...(hostState.interfaces ?? [])])].filter(Boolean) as ip}<option value={ip}>{ip}</option>{/each}</select></label>
      <label><span class="field-label">Port</span><input class="text-input" type="number" min="1024" max="65535" step="1" bind:value={port} aria-invalid={!!portError} aria-describedby={portError ? 'listener-port-error' : 'listener-connection-help'} disabled={busy || !loaded || enabled} />{#if portError}<p class="field-error" id="listener-port-error" role="alert">{portError}</p>{/if}</label>
    </div>
  </div>
  <div class="preference-row">
    <div><strong>LAN discovery</strong><p>Advertise Cockpit on your local network so companions can find it automatically.</p>{#if hostState.listener?.enabled && hostState.listener.discovery}<p class="listener-detail">Discovery: {String(hostState.listener.discovery)}</p>{/if}</div>
    <button class="toggle" class:on={discovery} role="switch" aria-label="LAN discovery" aria-checked={discovery} aria-describedby="listener-connection-help" disabled={busy || !loaded || enabled} onclick={() => { discovery = !discovery; }}><span></span></button>
  </div>
  {#if hostState.listener?.enabled && hostState.listener.endpoint}<div class="preference-row"><div><strong>Listener endpoint</strong><p><code>{hostState.listener.endpoint}</code></p></div></div>{/if}
  <div class="preference-row browser-trust">
    <div class="trust-copy">
      <strong>Browser companion HTTPS</strong>
      <p>Browsers on companion devices need to trust Cockpit’s certificate before opening pairing links or installing the browser app. Export the public certificate and verify its fingerprint through a trusted channel before installing it in the device’s certificate trust store.</p>
      <p>Keep the LAN address and port stable for saved browser apps.</p>
      {#if hostState.listener?.caSpkiSha256}<div class="fingerprint"><span>CA public-key fingerprint</span><code>{hostState.listener.caSpkiSha256}</code></div>{/if}
    </div>
    <button class="secondary-button" disabled={busy || !loaded} onclick={exportCertificate}><Icon name="download" size={14} />Export public certificate</button>
  </div>
  {#if hostState.listener?.lastError || error}<Notification message={String(error || hostState.listener?.lastError)} kind="error" />{/if}
</section>
<style>
  .listener-settings { margin-top: 24px; }
  .preference-row p { line-height: 1.6; max-width: 620px; overflow-wrap: anywhere; }
  .toggle:disabled { opacity: .45; cursor: not-allowed; }
  .connection-row, .browser-trust { align-items: flex-start; flex-wrap: wrap; }
  .connection-copy { flex: 1 1 260px; }
  .connection-fields { display: grid; grid-template-columns: minmax(0, 1fr) 110px; gap: 16px; flex: 1 1 300px; max-width: 420px; }
  .connection-fields label { min-width: 0; }
  select { width: 100%; background: var(--input-bg); }
  select:disabled { cursor: not-allowed; opacity: .65; }
  .preference-row .field-error { color: var(--text); font-size: 9px; }
  .trust-copy { flex: 1 1 400px; min-width: 0; }
  .browser-trust .secondary-button { flex-shrink: 0; }
  .fingerprint { display: grid; gap: 7px; margin-top: 14px; }
  .fingerprint > span { color: var(--muted); font-size: 9px; }
  code { font-size: 9px; overflow-wrap: anywhere; }
  .fingerprint code { display: block; padding: 10px 11px; border: 1px solid var(--line); border-radius: 4px; background: var(--input-bg); color: var(--soft); line-height: 1.7; }
  @media (max-width: 520px) { .connection-copy, .connection-fields, .trust-copy { flex-basis: auto; width: 100%; } }
</style>
