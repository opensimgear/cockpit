import assert from 'node:assert/strict';
import {randomUUID} from 'node:crypto';
import {readMcpEvents} from './mcp-vjoy-observation.mjs';

const uri = 'cockpit://observations/games.telemetry';
const monotonicMs = () => Number(process.hrtime.bigint()) / 1e6;

export async function openMcpTelemetryTiming(cdp, channel) {
  const invoke = (command, args = {}) => cdp.evaluate(
    `window.__TAURI_INTERNALS__.invoke(${JSON.stringify(command)},${JSON.stringify(args)})`);
  const initial = await invoke('mcp_settings_get');
  const abort = new AbortController();
  let stream, credential, enabledHere = false;
  const status = () => invoke('mcp_settings_get');
  const configure = (enabled, current) => invoke('mcp_settings_update', {settings:{
    enabled, port:initial.port, expectedSettingsRevision:current.settingsRevision,
  }});
  const close = async () => {
    abort.abort();
    await stream?.close().catch(() => {});
    try {
      if (credential) await invoke('mcp_credential_revoke', {
        id:credential.credential.id, expectedSettingsRevision:(await status()).settingsRevision,
      });
    } finally {
      if (enabledHere) await configure(initial.enabled, await status());
    }
  };
  try {
    credential = await invoke('mcp_credential_create', {name:'DCS telemetry timing fixture',
      scope:'read', expectedSettingsRevision:initial.settingsRevision});
    let current = await status();
    if (!current.enabled) {
      current = await configure(true, current);
      enabledHere = true;
    }
    assert.ok(current.running && current.endpoint, 'The local MCP listener must be running');
    const version = '2026-07-28';
    const post = (method, params) => fetch(current.endpoint, {
      method:'POST',
      signal:abort.signal,
      headers:{Authorization:`Bearer ${credential.token}`, Accept:'application/json, text/event-stream',
        'Content-Type':'application/json', 'MCP-Protocol-Version':version, 'Mcp-Method':method,
        ...(params.name ? {'Mcp-Name':params.name} : {})},
      body:JSON.stringify({jsonrpc:'2.0', id:randomUUID(), method, params:{...params, _meta:{
        'io.modelcontextprotocol/protocolVersion':version,
        'io.modelcontextprotocol/clientInfo':{name:'Cockpit DCS timing fixture', version:'1'},
        'io.modelcontextprotocol/clientCapabilities':{},
      }}}),
    });
    const frameOf = event => event.params?._meta?.['opensimgear.com/observation'];
    const readFrame = async () => frameOf(await stream.next(event => event.params?.uri === uri && frameOf(event)));
    const startTimeout = setTimeout(() => abort.abort(new Error('MCP telemetry baseline did not arrive')), 5000);
    let previous;
    try {
      const response = await post('subscriptions/listen', {notifications:{resourceSubscriptions:[uri]}});
      assert.equal(response.status, 200);
      assert.match(response.headers.get('content-type'), /text\/event-stream/);
      stream = readMcpEvents(response);
      await stream.next(event => event.method === 'notifications/subscriptions/acknowledged');
      previous = await readFrame();
      assert.equal(previous.kind, 'snapshot');
    } finally { clearTimeout(startTimeout); }

    return {close, async sample({submittedMs, packetSequence}, value) {
      const timeout = setTimeout(() => abort.abort(new Error(`MCP telemetry did not report marker ${value}`)), 2500);
      let reads = 0;
      try {
        for (;;) {
          const notice = await readFrame();
          const requestedMs = monotonicMs();
          assert.notEqual(notice.kind, 'loss');
          assert.equal(notice.generation, previous.generation);
          assert.ok(BigInt(notice.revision) > BigInt(previous.revision));
          previous = notice;
          reads++;
          const response = await post('tools/call', {name:'game_telemetry_inspect',
            arguments:{gameId:'dcs', search:channel, offset:0, limit:8}});
          assert.equal(response.status, 200);
          const result = (await response.json()).result?.structuredContent;
          assert.equal(result?.ok, true, JSON.stringify(result?.error));
          const row = result.data.games[0]?.channels.find(row => row.id === channel);
          if (row?.status !== 'known' || row.value !== value) continue;
          const receivedMs = monotonicMs();
          assert.ok(submittedMs <= requestedMs && requestedMs <= receivedMs);
          return {packetSequence, value, reads, submittedMs, requestedMs, receivedMs,
            notificationRevision:notice.revision};
        }
      } finally { clearTimeout(timeout); }
    }};
  } catch (error) {
    await close();
    throw error;
  }
}
