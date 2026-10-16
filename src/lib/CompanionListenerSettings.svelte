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
  let loaded=false;
  let requestVersion=0;
  const enabled=$derived(hostState.listener?.configuredEnabled??hostState.listener?.enabled??false);
  async function exportCertificate() {
    try { if(await invoke('export_companion_certificate'))notify('Public certificate exported. Verify the fingerprint before trusting it on another device.'); }
    catch(e){notify(String(e),'error');}
  }
  async function refresh() {
    if (busy) return;
    const version=requestVersion;
    try {
      const result=(await gatewayRequest({ command: 'companion_status' })).companions ?? {};
      if(version!==requestVersion || busy)return;
      hostState=result;
      if(!loaded && result.listener){
        loaded=true;
        const config=result.listener.config;
        if(config){address=config.advertisedHost;port=Number(config.bind.split(':').at(-1))||54437;discovery=config.discovery;}
      }
      if (!address) address = hostState.interfaces?.find(ip => ip !== '127.0.0.1' && !ip.includes(':')) ?? '127.0.0.1';
    } catch (e) { error = String(e); }
  }
  async function toggle(enabled: boolean) {
    requestVersion++;busy = true; error = '';
    try {
      hostState = (await gatewayRequest({ command: enabled ? 'companion_enable' : 'companion_disable',
        companion: enabled ? { address, advertisedHost: address, port, discovery } : {} })).companions ?? {};
    } catch (e) { error = String(e); }
    finally { busy = false; }
  }
  onMount(() => { void refresh(); const timer = setInterval(refresh, 1500); return () => clearInterval(timer); });
</script>

<section class="panel preferences-panel listener-settings" aria-label="Companion listener settings">
  <div class="panel-heading"><h2>Companion listener</h2><span role="status">{hostState.listener?.enabled ? 'Listening' : enabled?'Waiting to start':'Disabled'}</span></div>
  <div class="preference-row">
    <div><strong>Enable companion listener</strong><p>Allow paired Wi-Fi and network companions to connect. Direct USB companions work with this disabled.</p></div>
    <input type="checkbox" aria-label="Enable companion listener" checked={enabled} disabled={busy} onchange={event => toggle(event.currentTarget.checked)} />
  </div>
  <div class="fields">
    <label>LAN address<select bind:value={address} disabled={busy || enabled}>{#each [...new Set([address, ...(hostState.interfaces ?? [])])].filter(Boolean) as ip}<option value={ip}>{ip}</option>{/each}</select></label>
    <label>Port<input type="number" min="1024" max="65535" bind:value={port} disabled={busy || enabled} /></label>
    <label class="check"><input type="checkbox" bind:checked={discovery} disabled={busy || enabled} />LAN discovery</label>
  </div>
  {#if hostState.listener?.endpoint}<p>Endpoint: <code>{hostState.listener.endpoint}</code></p>{/if}
  <div class="browser-trust"><strong>Browser companion HTTPS</strong><p>Browsers on other devices must trust Cockpit’s certificate before opening pairing links or installing the browser app. Export the public certificate, transfer it to the device, and verify the fingerprint through a trusted channel before installing it in that device’s certificate trust store. Keep the listener address and port stable for saved browser apps.</p>{#if hostState.listener?.caSpkiSha256}<p>CA public-key fingerprint: <code>{hostState.listener.caSpkiSha256}</code></p>{/if}<button onclick={exportCertificate}><Icon name="download" size={16} />Export public certificate</button></div>
  {#if hostState.listener?.discovery}<p>Discovery: {String(hostState.listener.discovery)}</p>{/if}
  {#if hostState.listener?.lastError || error}<Notification message={String(error || hostState.listener?.lastError)} kind="error" />{/if}
</section>
<style>
  .browser-trust{padding:16px}.browser-trust p{padding:0}.browser-trust button{display:inline-flex;align-items:center;gap:8px;padding:9px 12px;border:1px solid var(--border);border-radius:6px;background:var(--panel);color:var(--text);cursor:pointer}
  .listener-settings{margin-top:24px}.fields{display:flex;gap:16px;padding:16px;flex-wrap:wrap;align-items:end}
  label{display:flex;flex-direction:column;gap:6px;font-size:13px}.check{flex-direction:row;align-items:center}
  input,select{font:inherit;padding:8px;background:var(--panel);color:var(--text);border:1px solid var(--border);border-radius:5px}
  p{font-size:13px;padding:0 16px;overflow-wrap:anywhere}.preference-row p{padding:0}
</style>
