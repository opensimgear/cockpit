import assert from 'node:assert/strict';
import test from 'node:test';
import { loadModules } from './test-module-loader.mjs';

const { createMcpSettings, parseMcpStatus, mcpClientConfig, mcpPortError, mcpStatusLabel } = (await loadModules(['./mcp'])).get('./mcp');
const plain = value => JSON.parse(JSON.stringify(value));
const credential = (id = 'client-a', scope = 'read', name = 'My assistant') => ({ id, name, scope, createdAt: 1_792_622_400_000 });
const status = (changes = {}) => ({ settingsRevision: 0, enabled: false, port: 54438, endpoint: 'http://127.0.0.1:54438/mcp', running: false, error: null, credentials: [], protocolVersions: ['2026-07-28', '2025-11-25'], readOnly: false, ...changes });
const token = 'osg_mcp_' + 'a'.repeat(64);
const deferred = () => { let resolve, reject; const promise = new Promise((yes, no) => { resolve = yes; reject = no; }); return { promise, resolve, reject }; };

function host(initial = status()) {
  let current=plain(initial),sequence=0;const calls=[];
  const send=async(command,args)=>{
    calls.push(plain({command,...(args?{args}:{})}));
    if(command==='mcp_settings_get')return plain(current);
    const expected=command==='mcp_settings_update'?args.settings.expectedSettingsRevision:args.expectedSettingsRevision;
    assert.equal(expected,current.settingsRevision);
    if(command==='mcp_settings_update'){
      current={...current,enabled:args.settings.enabled,port:args.settings.port,endpoint:'http://127.0.0.1:'+args.settings.port+'/mcp',running:args.settings.enabled,settingsRevision:current.settingsRevision+1};return plain(current);
    }
    if(command==='mcp_credential_create'){
      const created=credential('created-'+(++sequence),args.scope,args.name);current.credentials.push(created);current.settingsRevision++;return {credential:plain(created),token,status:plain(current)};
    }
    if(command==='mcp_credential_rotate'){
      const original=current.credentials.find(item=>item.id===args.id);assert.ok(original);
      const created=credential('created-'+(++sequence),original.scope,original.name);current.credentials=current.credentials.map(item=>item.id===args.id?created:item);current.settingsRevision++;
      return {credential:plain(created),token,status:plain(current)};
    }
    if(command==='mcp_credential_revoke'){current.credentials=current.credentials.filter(item=>item.id!==args.id);current.settingsRevision++;return plain(current);}
    throw Error('Unexpected command '+command);
  };
  return {calls,send,get current(){return current;}};
}

test('public status validates local endpoint and selects only nonsecret fields', () => {
  const input = status({ credentials: [{ ...credential(), token: 'must-not-survive' }], token: 'must-not-survive' });
  const parsed = parseMcpStatus(input);
  assert.deepEqual(plain(parsed), status({ credentials: [credential()] }));
  assert.doesNotMatch(JSON.stringify(parsed), /must-not-survive/);
  input.credentials[0].name = 'Changed externally';
  assert.equal(parsed.credentials[0].name, 'My assistant');
  for (const invalid of [null, [], status({ endpoint: 'http://192.168.1.2:54438/mcp' }), status({ endpoint: 'http://127.0.0.1:54439/mcp' }), status({ enabled: false, running: true }), status({ credentials: [credential(), credential()] }), status({ credentials: [credential('bad', 'admin')] }), status({ credentials: [{ ...credential(), createdAt: 'today' }] }), status({ port: 1023 }), status({ error: {} }), status({ protocolVersions: [null] })]) assert.throws(() => parseMcpStatus(invalid), /invalid MCP settings/);
  assert.equal(mcpPortError(1024), ''); assert.equal(mcpPortError(65535), '');
  for (const value of [0, -1, 65536, 54438.5, NaN, undefined]) assert.match(mcpPortError(value), /1024 to 65535/);
});

test('client configuration is loopback HTTP JSON with a placeholder until explicitly given a new token', () => {
  const config = JSON.parse(mcpClientConfig(status().endpoint));
  assert.deepEqual(config, { mcpServers: { opensimgear: { type: 'http', url: status().endpoint, headers: { Authorization: 'Bearer YOUR_COCKPIT_TOKEN' } } } });
  assert.equal(JSON.parse(mcpClientConfig(status().endpoint, token)).mcpServers.opensimgear.headers.Authorization, `Bearer ${token}`);
  assert.throws(() => mcpClientConfig('https://remote.example/mcp', token), /local MCP endpoint/);
});

test('browser settings are read-only and opening desktop settings performs only a native read', async () => {
  const h = host(), browser = createMcpSettings(h.send, false);
  await browser.load(); await browser.update(true, 54438); await browser.create('Client', 'write'); await browser.revoke('client-a'); await browser.rotate('client-a');
  assert.deepEqual(h.calls, []); assert.equal(mcpStatusLabel(browser.state), 'Desktop app required');
  const desktop = createMcpSettings(h.send, true);
  assert.equal(mcpStatusLabel(desktop.state), 'Loading…');
  await desktop.load();
  assert.deepEqual(h.calls, [{ command: 'mcp_settings_get' }]);
  assert.equal(desktop.state.loaded, true); assert.equal(mcpStatusLabel(desktop.state), 'Disabled');
});

test('native failure is honest and a failed listener startup retains saved enablement without claiming Listening', async () => {
  const unreadable = createMcpSettings(async () => { throw Error('Native service unavailable'); }, true);
  await unreadable.load(); assert.equal(unreadable.state.loaded, false); assert.equal(mcpStatusLabel(unreadable.state), 'Unavailable'); assert.match(unreadable.state.error, /unavailable/);
  const h = host();
  const settings = createMcpSettings(async (command, args) => command === 'mcp_settings_update' ? status({ enabled: true, error: 'Port already in use' }) : h.send(command, args), true);
  await settings.load(); await settings.update(true, 54438);
  assert.equal(settings.state.status.enabled, true); assert.equal(settings.state.status.running, false);
  assert.equal(mcpStatusLabel(settings.state), 'Unable to start'); assert.equal(settings.state.message, '');
});

test('failed status polling does not present a stale listener as Listening and recovers after a fresh read', async () => {
  const h = host(status({ enabled: true, running: true })); let fail = false;
  const settings = createMcpSettings((command, args) => fail ? Promise.reject(Error('Native IPC unavailable')) : h.send(command, args), true);
  await settings.load(); assert.equal(mcpStatusLabel(settings.state), 'Listening');
  fail = true; await settings.load(true); assert.equal(mcpStatusLabel(settings.state), 'Status unavailable'); assert.match(settings.state.error, /IPC unavailable/);
  fail = false; await settings.load(true); assert.equal(mcpStatusLabel(settings.state), 'Listening'); assert.equal(settings.state.error, '');
});

test('enable and disable use exact settings arguments; invalid ports do not write', async () => {
  const h = host(), settings = createMcpSettings(h.send, true); await settings.load();
  await settings.update(true, 0); assert.equal(h.calls.length, 1); assert.match(settings.state.error, /port/);
  await settings.update(true, 55555); assert.equal(settings.state.status.endpoint, 'http://127.0.0.1:55555/mcp'); assert.equal(mcpStatusLabel(settings.state), 'Listening');
  await settings.update(false, 55555); assert.equal(mcpStatusLabel(settings.state), 'Disabled');
  assert.deepEqual(h.calls.slice(1), [{ command: 'mcp_settings_update', args: { settings: { enabled: true, port: 55555, expectedSettingsRevision: 0 } } }, { command: 'mcp_settings_update', args: { settings: { enabled: false, port: 55555, expectedSettingsRevision: 1 } } }]);
});

test('an older poll cannot overwrite a confirmed settings mutation', async () => {
  const h = host(), gate = deferred(); let reads = 0;
  const settings = createMcpSettings((command, args) => command === 'mcp_settings_get' && ++reads > 1 ? gate.promise : h.send(command, args), true);
  await settings.load(); const polling = settings.load(true); await settings.update(true, 54439);
  gate.resolve(status()); await polling;
  assert.equal(settings.state.status.port, 54439); assert.equal(settings.state.status.running, true); assert.equal(settings.state.busy, '');
});

test('credential creation captures trimmed name/access and token copy hides the one-time secret', async () => {
  const h = host(), settings = createMcpSettings(h.send, true); await settings.load();
  await settings.create('   ', 'read'); assert.equal(h.calls.length, 1);
  await settings.create('界'.repeat(27), 'read'); assert.equal(h.calls.length, 1);
  await settings.create('  My assistant  ', 'write');
  assert.deepEqual(h.calls.at(-1), { command: 'mcp_credential_create', args: { name: 'My assistant', scope: 'write', expectedSettingsRevision: 0 } });
  assert.equal(settings.state.secret.token, token); assert.equal(settings.state.status.credentials[0].scope, 'write');
  assert.doesNotMatch(JSON.stringify(settings.state.status), new RegExp(token));
  await settings.create('Must save previous token first', 'read'); assert.equal(h.calls.length, 2);
  const copied = []; await settings.copy('token', async text => copied.push(text));
  assert.deepEqual(copied, [token]); assert.equal(settings.state.secret, null); assert.match(settings.state.message, /copied and hidden/);
});

test('copy endpoint retains an outstanding token; client config copies it once and then uses a placeholder', async () => {
  const h = host(), settings = createMcpSettings(h.send, true); await settings.load(); await settings.create('My assistant', 'read');
  const copied = []; await settings.copy('endpoint', async text => copied.push(text)); assert.ok(settings.state.secret);
  await settings.copy('config', async text => copied.push(text)); assert.equal(settings.state.secret, null);
  assert.equal(JSON.parse(copied[1]).mcpServers.opensimgear.headers.Authorization, `Bearer ${token}`);
  await settings.copy('config', async text => copied.push(text)); assert.match(copied[2], /YOUR_COCKPIT_TOKEN/); assert.doesNotMatch(copied[2], new RegExp(token));
});

test('clipboard failure keeps the token available for manual copy and explicit Hide forgets it', async () => {
  const h = host(), settings = createMcpSettings(h.send, true); await settings.load(); await settings.create('My assistant', 'read');
  await settings.copy('token', async () => { throw Error('No clipboard'); });
  assert.equal(settings.state.secret.token, token); assert.match(settings.state.error, /Select and copy/);
  settings.hideSecret(); assert.equal(settings.state.secret, null);
});

test('rotation replaces the credential in one exact-revision native transaction',async()=>{
 const h=host(status({credentials:[credential()]})),settings=createMcpSettings(h.send,true);await settings.load();await settings.rotate('client-a');
 assert.deepEqual(h.calls.slice(1),[{command:'mcp_credential_rotate',args:{id:'client-a',expectedSettingsRevision:0}}]);
 assert.equal(settings.state.status.credentials.length,1);assert.equal(settings.state.secret.credential.id,'created-1');assert.equal(settings.state.error,'');
});

test('failed atomic rotation preserves the acknowledged original and never sends separate writes',async()=>{
 const h=host(status({credentials:[credential()]}));const settings=createMcpSettings((command,args)=>command==='mcp_credential_rotate'?Promise.reject(Error('Disk full')):h.send(command,args),true);
 await settings.load();await settings.rotate('client-a');assert.equal(settings.state.secret,null);assert.equal(settings.state.status.credentials[0].id,'client-a');assert.match(settings.state.error,/Disk full.*reconcile/);
 assert.ok(h.calls.every(call=>!['mcp_credential_create','mcp_credential_revoke'].includes(call.command)));
});

test('uncertain rotation is reconciled by reading without retrying or claiming either token',async()=>{
 const h=host(status({credentials:[credential()]}));const settings=createMcpSettings(async(command,args)=>{const reply=await h.send(command,args);if(command==='mcp_credential_rotate')throw Error('Disconnected after commit');return reply;},true);
 await settings.load();await settings.rotate('client-a');assert.equal(settings.state.secret,null);assert.match(settings.state.error,/reconcile/);assert.equal(h.current.credentials.length,1);assert.equal(h.current.credentials[0].id,'created-1');
 await settings.load(true);assert.equal(settings.state.status.credentials[0].id,'created-1');assert.equal(settings.state.secret,null);assert.equal(h.calls.filter(call=>call.command==='mcp_credential_rotate').length,1);
});

test('revoke removes access and clears the displayed token if it belongs to that credential', async () => {
  const h = host(), settings = createMcpSettings(h.send, true); await settings.load(); await settings.create('My assistant', 'write');
  const id = settings.state.secret.credential.id; await settings.revoke('unknown'); assert.equal(h.calls.length, 2);
  await settings.revoke(id); assert.equal(settings.state.secret, null); assert.deepEqual(plain(settings.state.status.credentials), []); assert.match(settings.state.message, /revoked/);
});

test('malformed creation and uncertain writes are not retried; refreshing reconciles public metadata', async () => {
  const h = host(); let creates = 0;
  const settings = createMcpSettings(async (command, args) => {
    if (command === 'mcp_credential_create') { creates++; await h.send(command, args); throw Error('Connection lost after commit'); }
    return h.send(command, args);
  }, true);
  await settings.load(); await settings.create('My assistant', 'read'); assert.equal(creates, 1); assert.equal(settings.state.secret, null); assert.match(settings.state.error, /not retried automatically/);
  await settings.load(); assert.equal(creates, 1); assert.equal(settings.state.status.credentials.length, 1);
  const malformed = createMcpSettings(async command => command === 'mcp_settings_get' ? status() : { credential: credential(), token: 'short', status: status({ credentials: [credential()] }) }, true);
  await malformed.load(); await malformed.create('My assistant', 'read'); assert.equal(malformed.state.secret, null); assert.match(malformed.state.error, /valid new credential/);
});

test('external revocation clears the secret and disposal prevents late replies or notifications', async () => {
  const h = host(), settings = createMcpSettings(h.send, true); await settings.load(); await settings.create('My assistant', 'read');
  await h.send('mcp_credential_revoke', { id: settings.state.secret.credential.id, expectedSettingsRevision: h.current.settingsRevision }); await settings.load(true); assert.equal(settings.state.secret, null);
  const gate = deferred(), changes = [], pending = createMcpSettings(() => gate.promise, true, next => changes.push(next));
  const reading = pending.load(); const before = changes.length; pending.dispose(); gate.resolve(status()); await reading;
  assert.equal(changes.length, before); assert.equal(pending.state.status, null); assert.equal(pending.state.secret, null);
});

test('leaving during atomic replacement never publishes the secret or sends a second mutation',async()=>{
 const h=host(status({credentials:[credential()]})),gate=deferred();const settings=createMcpSettings((command,args)=>command==='mcp_credential_rotate'?gate.promise:h.send(command,args),true);
 await settings.load();const replacing=settings.rotate('client-a');settings.dispose();gate.resolve({credential:credential('new-client'),token,status:status({settingsRevision:1,credentials:[credential('new-client')]})});
 await replacing;assert.ok(h.calls.every(call=>!['mcp_credential_create','mcp_credential_revoke'].includes(call.command)));assert.equal(settings.state.secret,null);
});

test('a corrupt native settings file permits reads but cannot trigger preference or credential writes', async () => {
  const h = host(status({ readOnly: true, error: 'Repair the settings file', credentials: [credential()] }));
  const settings = createMcpSettings(h.send, true); await settings.load(); await settings.update(true, 54438); await settings.create('Client', 'read'); await settings.rotate('client-a'); await settings.revoke('client-a');
  assert.deepEqual(h.calls, [{ command: 'mcp_settings_get' }]); assert.equal(settings.state.status.readOnly, true);
});
