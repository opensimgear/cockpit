<script lang="ts">
  import { onMount } from 'svelte';
  import { invoke, isTauri } from '@tauri-apps/api/core';
  import Icon from './Icon.svelte';
  import Notification from './Notification.svelte';
  import { createMcpSettings, mcpClientConfig, mcpPortError, mcpStatusLabel, type McpScope, type McpSettingsState } from './mcp';

  let preferences = $state.raw<McpSettingsState>({ native: false, loaded: false, status: null, busy: '', error: '', message: '', secret: null });
  let port = $state(54438);
  let name = $state('');
  let scope = $state<McpScope>('read');
  let initialized = false;
  let observedPort = 54438;
  const settings = createMcpSettings(invoke, isTauri(), next => {
    preferences = next;
    if (next.status) {
      if (!initialized || next.status.enabled || port === observedPort) port = next.status.port;
      initialized = true;
      observedPort = next.status.port;
    }
  });
  const unavailable = $derived(!preferences.native || !preferences.loaded || !!preferences.busy || !!preferences.status?.readOnly);
  const portError = $derived(mcpPortError(port));
  const config = $derived(preferences.status ? mcpClientConfig(preferences.status.endpoint) : '');
  const error = $derived(preferences.error || preferences.status?.error || '');
  const formatDate = (value: number) => new Date(value).toLocaleString();
  const copy = (kind: 'endpoint' | 'config' | 'token') => settings.copy(kind, text => navigator.clipboard.writeText(text));

  onMount(() => {
    preferences = settings.state;
    void settings.load();
    const timer = setInterval(() => { void settings.load(true); }, 5000);
    return () => { clearInterval(timer); settings.dispose(); };
  });

  async function toggle() {
    if (!preferences.status) return;
    await settings.update(!preferences.status.enabled, preferences.status.enabled ? preferences.status.port : port);
    if (settings.state.status) port = settings.state.status.port;
  }
</script>

<section class="panel preferences-panel mcp-settings" aria-label="AI integration settings">
  <div class="panel-heading"><h2>AI integration</h2><span class="quiet-tag" role="status">{mcpStatusLabel(preferences)}</span></div>
  <div class="preference-row">
    <div><strong>Enable MCP server</strong><p>Connect an AI assistant to this running Cockpit to inspect resources and configure profiles. Clients connect from this computer and need a token with the access you choose.</p></div>
    <button class="toggle" class:on={preferences.status?.enabled ?? false} role="switch" aria-label="Enable MCP server" aria-checked={preferences.status?.enabled ?? false} disabled={unavailable || !preferences.status?.enabled && !!portError} onclick={() => { void toggle(); }}><span></span></button>
  </div>
  {#if !preferences.native}
    <div class="preference-row"><p>Open the Cockpit desktop app to manage AI connections.</p></div>
  {:else}
    <div class="preference-row connection-row">
      <div class="connection-copy"><strong>Local connection</strong><p id="mcp-port-help">Turn off the server before changing its port. The endpoint accepts connections from this computer only. Port changes are saved when you enable the server.</p>{#if preferences.status}<p class="endpoint"><code>{preferences.status.endpoint}</code></p>{/if}</div>
      <div class="connection-controls"><label><span class="field-label">Port</span><input class="text-input" type="number" min="1024" max="65535" step="1" bind:value={port} aria-invalid={!!portError} aria-describedby={portError ? 'mcp-port-error' : 'mcp-port-help'} disabled={unavailable || preferences.status?.enabled} />{#if portError}<p class="field-error" id="mcp-port-error" role="alert">{portError}</p>{/if}</label><button class="secondary-button" disabled={unavailable} onclick={() => { void copy('endpoint'); }}><Icon name="copy" size={14} />Copy endpoint</button></div>
    </div>
    {#if preferences.status?.enabled && !preferences.status.running && !preferences.status.error}<div class="preference-row"><p role="status">The server is enabled but has not started listening. Refresh to check its current preferences.</p></div>{/if}
    <div class="preference-row credential-form">
      <div class="connection-copy"><strong>Client access</strong><p id="mcp-access-help">Create a separate token for each assistant. Read only allows inspection and preparation. Configure also allows saving, applying profiles, and supported device settings.</p></div>
      <form onsubmit={event => { event.preventDefault(); void settings.create(name, scope); }}>
        <label><span class="field-label">Client name</span><input class="text-input" aria-describedby="mcp-access-help" autocomplete="off" maxlength="80" placeholder="My assistant" bind:value={name} disabled={unavailable || !!preferences.secret} /></label>
        <label><span class="field-label">Access</span><select aria-describedby="mcp-access-help" bind:value={scope} disabled={unavailable || !!preferences.secret}><option value="read">Read only</option><option value="write">Configure</option></select></label>
        <button class="secondary-button" type="submit" disabled={unavailable || !!preferences.secret || !name.trim()}><Icon name="plus" size={14} />{preferences.busy === 'create' ? 'Creating…' : 'Create token'}</button>
      </form>
    </div>
    {#if preferences.secret}
      <div class="preference-row secret-row">
        <div class="secret-copy"><strong>New token for {preferences.secret.credential.name}</strong><p id="mcp-secret-help">Save this token in your client now. Copying it or its client configuration hides it. Leaving this section also hides it.</p><textarea class="text-input secret-value" aria-label="New MCP token" aria-describedby="mcp-secret-help" readonly autocomplete="off" spellcheck="false" value={preferences.secret.token}></textarea></div>
        <div class="secret-actions"><button class="secondary-button" disabled={unavailable} onclick={() => { void copy('token'); }}><Icon name="copy" size={14} />Copy token</button><button class="secondary-button" disabled={unavailable} onclick={() => { void copy('config'); }}><Icon name="copy" size={14} />Copy client configuration</button><button class="secondary-button" disabled={unavailable} onclick={() => settings.hideSecret()}>Hide token</button></div>
      </div>
    {/if}
    {#if preferences.status?.credentials.length}
      <div class="credential-list" aria-label="MCP credentials">
        {#each preferences.status.credentials as credential (credential.id)}
          <div class="preference-row credential-row"><div><strong>{credential.name}</strong><p>{credential.scope === 'read' ? 'Read only' : 'Configure'} · Created {formatDate(credential.createdAt)}</p><p class="credential-id">ID: {credential.id}</p></div><div class="credential-actions"><button class="secondary-button" aria-label={`Replace token for ${credential.name}`} disabled={unavailable || !!preferences.secret} onclick={() => { void settings.rotate(credential.id); }}><Icon name="refresh" size={14} />{preferences.busy === `rotate:${credential.id}` ? 'Replacing…' : 'Replace token'}</button><button class="secondary-button" aria-label={`Revoke token for ${credential.name}`} disabled={unavailable} onclick={() => { void settings.revoke(credential.id); }}><Icon name="close" size={14} />{preferences.busy === `revoke:${credential.id}` ? 'Revoking…' : 'Revoke'}</button></div></div>
        {/each}
      </div>
    {:else if preferences.loaded}<div class="preference-row"><p>No clients have access yet. Create a token to connect your first assistant.</p></div>{/if}
    {#if preferences.status}
      <div class="preference-row client-config-row"><div class="client-config"><strong>Client configuration</strong><p id="mcp-config-help">Use these HTTP connection settings in a compatible MCP client. Replace YOUR_COCKPIT_TOKEN with your saved token. Replacing a token immediately revokes the original when successful.</p><textarea class="text-input config-value" aria-label="MCP client configuration" aria-describedby="mcp-config-help" readonly spellcheck="false" value={config}></textarea>{#if preferences.status.protocolVersions.length}<p class="protocols">Protocol versions: {preferences.status.protocolVersions.join(', ')}</p>{/if}</div><button class="secondary-button" disabled={unavailable} onclick={() => { void copy('config'); }}><Icon name="copy" size={14} />Copy configuration</button></div>
    {/if}
    {#if error}<div class="settings-result"><Notification message={error} kind="error" /><button class="secondary-button" disabled={!!preferences.busy} onclick={() => { void settings.load(); }}>Refresh status</button></div>
    {:else if preferences.message}<div class="preference-row"><p role="status">{preferences.message}</p></div>{/if}
  {/if}
</section>

<style>
  .mcp-settings { margin-top: 24px; }
  .preference-row p { line-height: 1.6; max-width: 620px; overflow-wrap: anywhere; }
  .toggle:disabled { opacity: .45; cursor: not-allowed; }
  .connection-row,.credential-form,.secret-row,.client-config-row { align-items: flex-start; flex-wrap: wrap; }
  .connection-copy { flex: 1 1 280px; }
  .connection-controls { display: flex; align-items: flex-end; gap: 12px; flex-wrap: wrap; }
  .connection-controls label { width: 110px; }
  label { display: grid; gap: 8px; min-width: 0; }
  .credential-form form { display: grid; grid-template-columns: minmax(120px,1fr) 130px; gap: 12px; flex: 1 1 320px; }
  .credential-form form button { justify-self: start; grid-column: 1 / -1; }
  select { width: 100%; background: var(--input-bg); }
  select:disabled { cursor: not-allowed; opacity: .65; }
  .field-error { font-size: 9px; }
  code,.credential-id,.protocols { font-size: 9px; overflow-wrap: anywhere; }
  .secret-copy,.client-config { flex: 1 1 440px; min-width: 0; }
  textarea { display: block; width: 100%; resize: vertical; margin-top: 14px; font-family: var(--font-mono,monospace); line-height: 1.7; font-size: 10px; padding: 10px 11px; }
  .secret-value { min-height: 72px; }
  .config-value { min-height: 250px; }
  .secret-actions,.credential-actions { display: flex; gap: 8px; flex-wrap: wrap; }
  .secret-actions { flex: 1 1 160px; }
  .credential-row { gap: 12px; flex-wrap: wrap; }
  .credential-row > div:first-child { min-width: 0; flex: 1 1 200px; }
  .client-config-row > button { flex-shrink: 0; }
  .settings-result { padding: 0 24px 20px; }
  @media(max-width:520px) { .connection-copy,.connection-controls,.credential-form form,.secret-copy,.client-config { flex-basis: 100%; } .credential-form form { grid-template-columns: 1fr; } .credential-form form button { grid-column: auto; } }
</style>
