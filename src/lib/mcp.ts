/** Settings for the MCP server in this running desktop. Tokens stay in memory. */
export type McpScope = 'read' | 'write';
export type McpCredential = { id: string; name: string; scope: McpScope; createdAt: number };
export type McpStatus = {
  settingsRevision: number; enabled: boolean; port: number; endpoint: string; running: boolean; error: string | null;
  credentials: McpCredential[]; protocolVersions: string[];
  readOnly?: boolean;
};
export type McpSecret = { credential: McpCredential; token: string };
export type McpSettingsState = {
  native: boolean; loaded: boolean; status: McpStatus | null; busy: string;
  error: string; message: string; secret: McpSecret | null;
  statusCurrent?: boolean;
};
export type McpInvoke = (command: string, args?: Record<string, unknown>) => Promise<unknown>;
export type McpClipboard = (text: string) => Promise<void>;

const object = (value: unknown): value is Record<string, unknown> => !!value && typeof value === 'object' && !Array.isArray(value);
const text = (value: unknown, max: number): value is string => typeof value === 'string' && value.length > 0 && value.length <= max;
const credential = (value: unknown): value is McpCredential => object(value)
  && text(value.id, 128) && text(value.name, 80) && (value.scope === 'read' || value.scope === 'write')
  && typeof value.createdAt === 'number' && Number.isSafeInteger(value.createdAt) && value.createdAt >= 0 && value.createdAt <= 8_640_000_000_000_000;

export function mcpPortError(port: number): string {
  return Number.isInteger(port) && port >= 1024 && port <= 65535 ? '' : 'Enter a port from 1024 to 65535.';
}

export function parseMcpStatus(value: unknown): McpStatus {
  if (!object(value) || !Number.isSafeInteger(value.settingsRevision) || Number(value.settingsRevision) < 0 || typeof value.enabled !== 'boolean' || typeof value.running !== 'boolean'
    || typeof value.port !== 'number' || mcpPortError(value.port)
    || value.endpoint !== `http://127.0.0.1:${value.port}/mcp`
    || !(value.error === null || typeof value.error === 'string' && value.error.length <= 16_384)
    || !Array.isArray(value.credentials) || value.credentials.length > 256 || !value.credentials.every(credential)
    || new Set(value.credentials.map(item => item.id)).size !== value.credentials.length
    || !Array.isArray(value.protocolVersions) || value.protocolVersions.length > 16 || !value.protocolVersions.every(item => text(item, 80))
    || value.readOnly !== undefined && typeof value.readOnly !== 'boolean'
    || value.running && !value.enabled) throw new Error('Cockpit returned invalid MCP settings. Refresh to read the current server state.');
  // Select public fields explicitly; secret-bearing responses never enter status.
  return {
    settingsRevision: value.settingsRevision as number, enabled: value.enabled, port: value.port, endpoint: value.endpoint as string, running: value.running, error: value.error as string | null,
    credentials: value.credentials.map(item => ({ id: item.id, name: item.name, scope: item.scope, createdAt: item.createdAt })),
    protocolVersions: [...value.protocolVersions] as string[],
    readOnly: value.readOnly === true,
  };
}

export function mcpClientConfig(endpoint: string, token = 'YOUR_COCKPIT_TOKEN'): string {
  if (!/^http:\/\/127\.0\.0\.1:[0-9]+\/mcp$/.test(endpoint)) throw new Error('Read the local MCP endpoint before copying client configuration.');
  return JSON.stringify({ mcpServers: { opensimgear: { type: 'http', url: endpoint, headers: { Authorization: `Bearer ${token}` } } } }, null, 2);
}

export function mcpStatusLabel(state: McpSettingsState): string {
  if (!state.native) return 'Desktop app required';
  if (!state.loaded) return state.error ? 'Unavailable' : 'Loading…';
  if (state.busy === 'settings') return 'Saving…';
  if (state.statusCurrent === false) return 'Status unavailable';
  if (state.status?.running) return 'Listening';
  if (state.status?.error) return 'Unable to start';
  return state.status?.enabled ? 'Waiting to start' : 'Disabled';
}

export function createMcpSettings(send: McpInvoke, native: boolean, changed: (state: McpSettingsState) => void = () => {}) {
  let state: McpSettingsState = { native, loaded: false, status: null, busy: '', error: '', message: '', secret: null, statusCurrent: false };
  let disposed = false, polling = false, epoch = 0, readError = '';
  const publish = (patch: Partial<McpSettingsState>) => {
    if (disposed) return;
    state = { ...state, ...patch };
    changed(state);
  };
  const ready = () => !disposed && native && state.loaded && !!state.status && !state.busy;
  const writable = () => ready() && !state.status?.readOnly;
  const failed = (error: unknown) => (error instanceof Error ? error.message : String(error)).slice(0, 2048);
  const readStatus = (value: unknown) => parseMcpStatus(value);

  async function load(silent = false) {
    if (disposed || !native || state.busy || polling) return;
    polling = true;
    const version = epoch;
    if (!silent) publish({ busy: 'load', error: '', message: '' });
    try {
      const status = readStatus(await send('mcp_settings_get'));
      if (disposed || epoch !== version) return;
      const error = silent && state.error !== readError ? state.error : '';
      readError = '';
      publish({ status, loaded: true, statusCurrent: true, error, secret: state.secret && status.credentials.some(item => item.id === state.secret?.credential.id) ? state.secret : null });
    } catch (error) {
      if (!disposed && epoch === version) {
        const patch: Partial<McpSettingsState> = { statusCurrent: false };
        if (!silent || !state.error || state.error === readError) { readError = failed(error); patch.error = readError; }
        publish(patch);
      }
    } finally {
      polling = false;
      if (epoch === version && state.busy === 'load') publish({ busy: '' });
    }
  }

  async function update(enabled: boolean, port: number) {
    if (!writable()) return;
    const validation = mcpPortError(port);
    if (validation) { publish({ error: validation, message: '' }); return; }
    epoch++;
    publish({ busy: 'settings', error: '', message: '' });
    try {
      const status = readStatus(await send('mcp_settings_update', { settings: { enabled, port, expectedSettingsRevision: state.status!.settingsRevision } }));
      publish({ status, statusCurrent: true, message: status.error ? '' : 'MCP settings saved.' });
    } catch (error) { publish({ statusCurrent: false, error: `${failed(error)} Refresh to check whether the settings were saved before trying again.` }); }
    finally { publish({ busy: '' }); }
  }

  async function issue(name: string, scope: McpScope, replacementId?: string): Promise<{ status: McpStatus; secret: McpSecret }> {
    const expectedSettingsRevision = state.status!.settingsRevision;
    const result = replacementId
      ? await send('mcp_credential_rotate', { id: replacementId, expectedSettingsRevision })
      : await send('mcp_credential_create', { name, scope, expectedSettingsRevision });
    if (!object(result) || !credential(result.credential) || !text(result.token, 1024) || /\s/.test(result.token) || result.token.length < 32) throw new Error('Cockpit did not return a valid new credential. Refresh the credential list before creating another.');
    const status = readStatus(result.status), issued = result.credential;
    if (!status.credentials.some(item => item.id === issued.id && item.name === name && item.scope === scope)
      || issued.name !== name || issued.scope !== scope) throw new Error('The new credential does not match the requested access. Refresh the credential list.');
    return { status, secret: { credential: { ...issued }, token: result.token } };
  }

  async function create(name: string, scope: McpScope) {
    if (!writable() || state.secret) return;
    const captured = name.trim();
    if (!captured || new TextEncoder().encode(captured).length > 80 || !['read', 'write'].includes(scope)) { publish({ error: 'Enter a short client name and choose its access. The encoded name may contain up to 80 bytes.', message: '' }); return; }
    epoch++;
    publish({ busy: 'create', error: '', message: '' });
    try {
      const issued = await issue(captured, scope);
      publish({ status: issued.status, statusCurrent: true, secret: issued.secret, message: 'Save this token in your MCP client. It is shown only once.' });
    } catch (error) { publish({ error: `${failed(error)} Refresh to reconcile the credential list; creation is not retried automatically.` }); }
    finally { publish({ busy: '' }); }
  }

  async function revoke(id: string) {
    if (!writable() || !state.status?.credentials.some(item => item.id === id)) return;
    epoch++;
    publish({ busy: `revoke:${id}`, error: '', message: '' });
    try {
      const status = readStatus(await send('mcp_credential_revoke', { id, expectedSettingsRevision: state.status!.settingsRevision }));
      if (status.credentials.some(item => item.id === id)) throw new Error('The credential is still present.');
      publish({ status, statusCurrent: true, secret: state.secret?.credential.id === id ? null : state.secret, message: 'Credential revoked. Its token can no longer connect.' });
    } catch (error) { publish({ error: `${failed(error)} Refresh to check whether access was revoked.` }); }
    finally { publish({ busy: '' }); }
  }

  async function rotate(id: string) {
    if (!writable() || state.secret) return;
    const original = state.status?.credentials.find(item => item.id === id);
    if (!original) return;
    epoch++;
    publish({ busy: `rotate:${id}`, error: '', message: '' });
    try {
      const issued = await issue(original.name, original.scope, id);
      if (issued.status.credentials.some(item => item.id === id)) throw new Error('The original credential is still present.');
      publish({ status: issued.status, statusCurrent: true, secret: issued.secret, message: 'Token replaced. Save the new token in your MCP client.' });
    } catch (error) {
      publish({ error: `${failed(error)} Refresh to reconcile credential replacement; it is not retried automatically.` });
    } finally { publish({ busy: '' }); }
  }

  function hideSecret() {
    publish({ secret: null, message: 'Token hidden. Create a replacement if you did not save it.' });
  }

  async function copy(kind: 'endpoint' | 'config' | 'token', clipboard: McpClipboard) {
    if (!ready()) return;
    const secret = state.secret;
    if (kind === 'token' && !secret) return;
    const endpoint = state.status!.endpoint;
    const value = kind === 'endpoint' ? endpoint : kind === 'token' ? secret!.token : mcpClientConfig(endpoint, secret?.token);
    publish({ busy: 'copy', error: '', message: '' });
    try {
      await clipboard(value);
      publish({ ...(kind === 'endpoint' ? {} : { secret: null }), message: kind === 'endpoint' ? 'Endpoint copied.' : kind === 'token' ? 'Token copied and hidden.' : secret ? 'Client configuration copied with the token. Token hidden.' : 'Client configuration copied. Replace YOUR_COCKPIT_TOKEN with your saved token.' });
    } catch { publish({ error: 'Clipboard access failed. Select and copy the text manually; the token remains available until you hide it or leave this section.' }); }
    finally { publish({ busy: '' }); }
  }

  return { get state() { return state; }, load, update, create, revoke, rotate, hideSecret, copy,
    dispose() { disposed = true; epoch++; state = { ...state, secret: null }; },
  };
}
