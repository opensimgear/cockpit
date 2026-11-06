import assert from 'node:assert/strict';
import test from 'node:test';
import { Client } from '@modelcontextprotocol/sdk/client/index.js';
import { StreamableHTTPClientTransport } from '@modelcontextprotocol/sdk/client/streamableHttp.js';
import { nativePage } from '../../scripts/reactive/cdp.mjs';

test('read-only MCP desktop status matches the native portal snapshot', {
  skip: process.env.OSG_MCP_DESKTOP_STATUS_NATIVE !== '1', timeout: 60000,
}, async () => {
  const page = await nativePage();
  const client = new Client({name:'Desktop status parity fixture',version:'1'}, {capabilities:{}});
  const invoke = (command, args = {}) => page.evaluate(
    `window.__TAURI_INTERNALS__.invoke(${JSON.stringify(command)},${JSON.stringify(args)})`,
  );
  const native = async command => {
    const result = await invoke('gateway_request', {request:{command,clientId:'desktop-status-parity'}});
    assert.equal(result.ok,true,result.error);
    return result.state;
  };
  const settings = () => invoke('mcp_settings_get');
  const configure = async (enabled, port) => invoke('mcp_settings_update', {settings:{
    enabled,port,expectedSettingsRevision:(await settings()).settingsRevision,
  }});
  let initial, credential, enabledByFixture = false;
  try {
    initial = await settings();
    credential = await invoke('mcp_credential_create', {
      name:'Temporary desktop status reader',scope:'read',expectedSettingsRevision:initial.settingsRevision,
    });
    const status = initial.enabled ? initial : await configure(true, initial.port);
    enabledByFixture = !initial.enabled;
    const transport = new StreamableHTTPClientTransport(new URL(status.endpoint), {
      requestInit:{headers:{Authorization:`Bearer ${credential.token}`}},
    });
    await client.connect(transport);
    const descriptor = (await client.listTools()).tools.find(tool => tool.name === 'profile_desktop_status');
    assert.ok(descriptor,'Tool must be discoverable with a read credential');
    assert.equal(descriptor.inputSchema.additionalProperties,false);
    assert.equal(descriptor.annotations.readOnlyHint,true);
    const before = await native('profile_status');
    const {desktop} = await native('profile_desktop_status');
    const reply = await client.callTool({name:'profile_desktop_status',arguments:{}});
    assert.ok(!reply.isError);
    const result = reply.structuredContent;
    assert.equal(result.ok,true);
    for (const field of ['enabled','qualified','state','reason','scope','error']) {
      assert.deepEqual(result.data[field],desktop[field],field);
    }
    for (const field of ['application','recentApplication','applications']) {
      assert.ok(!Object.hasOwn(result.data,field),field);
    }
    if (desktop.runtime === null) assert.equal(result.data.runtime,null);
    assert.deepEqual((await native('profile_status')).profiles.profileSession.owner,before.profiles.profileSession.owner);
  } finally {
    try {
      try { await client.close(); }
      finally {
        if (credential) await invoke('mcp_credential_revoke', {
          id:credential.credential.id,expectedSettingsRevision:(await settings()).settingsRevision,
        });
      }
    } finally {
      try { if (enabledByFixture) await configure(initial.enabled, initial.port); }
      finally { page.close(); }
    }
  }
});
