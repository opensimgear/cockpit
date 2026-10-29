// Real SDK client -> loopback MCP -> the running desktop's shared manager.
// CDP is used only by this integration test to inspect native IPC/UI agreement.
import test from 'node:test';
import assert from 'node:assert/strict';
import { randomUUID } from 'node:crypto';
import { mkdir, writeFile } from 'node:fs/promises';
import { resolve } from 'node:path';
import { execFile } from 'node:child_process';
import { promisify } from 'node:util';
import { Client } from '@modelcontextprotocol/sdk/client/index.js';
import { StreamableHTTPClientTransport } from '@modelcontextprotocol/sdk/client/streamableHttp.js';
import { softwareScenarios } from './mcp-software-scenarios.mjs';
import { companionScenarios } from './mcp-companion-scenarios.mjs';
import { uiPreferenceScenarios } from './mcp-ui-preference-scenarios.mjs';

const pause = ms => new Promise(resolve => setTimeout(resolve, ms));
async function until(operation, description, timeout = 20000) {
  const deadline = Date.now() + timeout; let last;
  while (Date.now() < deadline) {
    try { const value = await operation(); if (value) return value; } catch (error) { last = error; }
    await pause(100);
  }
  throw new Error(`${description}${last ? `: ${last.message}` : ''}`);
}
async function nativePage() {
  const targets = await (await fetch(`${process.env.OSG_NATIVE_CDP_URL ?? 'http://127.0.0.1:9227'}/json/list`, { signal: AbortSignal.timeout(5000) })).json();
  const origin = process.env.OSG_NATIVE_DEV_ORIGIN ?? 'http://127.0.0.1:1420';
  const target = targets.find(target => target.type === 'page' && !target.url.includes('profile-activation-window') && (target.url.startsWith(`${origin}/`) || target.url.startsWith('http://tauri.localhost/')));
  assert.ok(target, 'Start the actual native dev Cockpit with WebView debugging before running this acceptance test.');
  const socket = new WebSocket(target.webSocketDebuggerUrl), pending = new Map(); let sequence = 0;
  await new Promise((resolve, reject) => { const timer = setTimeout(() => { socket.close(); reject(new Error('Native debugger connection timed out')); }, 8000); socket.addEventListener('open', () => { clearTimeout(timer); resolve(); }, { once: true }); socket.addEventListener('error', error => { clearTimeout(timer); reject(error); }, { once: true }); });
  socket.addEventListener('message', event => { const message = JSON.parse(event.data), task = pending.get(message.id); if (!task) return; clearTimeout(task.timer); pending.delete(message.id); message.error ? task.reject(new Error(message.error.message)) : task.resolve(message.result); });
  socket.addEventListener('close', () => { for (const task of pending.values()) { clearTimeout(task.timer); task.reject(new Error('Native debugger connection closed')); } pending.clear(); });
  const send = (method, params = {}) => new Promise((resolve, reject) => { if(socket.readyState!==WebSocket.OPEN){reject(new Error('Native debugger connection is closed'));return;} const id = ++sequence, timer = setTimeout(() => { pending.delete(id); reject(new Error(`${method} timed out`)); }, 45000); pending.set(id, { resolve, reject, timer }); socket.send(JSON.stringify({ id, method, params })); });
  return {
    send,
    async evaluate(fn, ...args) { const reply = await send('Runtime.evaluate', { expression: `(${fn.toString()})(...${JSON.stringify(args)})`, awaitPromise: true, returnByValue: true }); if (reply.exceptionDetails) throw new Error(reply.exceptionDetails.exception?.description ?? reply.exceptionDetails.text); return reply.result.value; },
    async invoke(command, args = {}) { return this.evaluate((command, args) => window.__TAURI_INTERNALS__.invoke(command, args), command, args); },
    async invokeMcp(command,args={}) {
      const current=await this.invoke('mcp_settings_get');
      const request=command==='mcp_settings_update'?{settings:{...args.settings,expectedSettingsRevision:current.settingsRevision}}:{...args,expectedSettingsRevision:current.settingsRevision};
      return this.invoke(command,request);
    },
    async native(request) { const response = await this.invoke('gateway_request', { request: { ...request, clientId: 'mcp-native-acceptance-ui' } }); assert.equal(response.ok, true, response.error); return response.state; },
    close() { for (const task of pending.values()) clearTimeout(task.timer); socket.close(); },
  };
}
async function connect(endpoint, token) {
  const client = new Client({ name: 'OpenSimGear live acceptance', version: '1.32.1' }, { capabilities: {} });
  const transport = new StreamableHTTPClientTransport(new URL(endpoint), { requestInit: { headers: { Authorization: `Bearer ${token}` } }, reconnectionOptions: { maxRetries: 0 } });
  await client.connect(transport); return client;
}
async function call(client, name, args = {}) {
  const reply = await client.callTool({ name, arguments: args }, undefined, { timeout: 135000 });
  const result = reply.structuredContent ?? JSON.parse(reply.content.find(part => part.type === 'text').text);
  return { ...result, isError: reply.isError ?? false };
}
async function ok(client, name, args = {}) { const result = await call(client, name, args); assert.equal(result.ok, true, `${name}: ${JSON.stringify(result)}`); return result.data; }
const mutation = args => ({ ...args, operationId: randomUUID() });
async function companionState(page) {
  const status=await page.invoke('persistence_status'),{DatabaseSync}=await import('node:sqlite');
  const database=new DatabaseSync(status.path,{readOnly:true});
  try {
    return database.prepare("SELECT domain,key,revision,hex(payload) AS payload FROM records WHERE domain='secrets' OR (domain='companion' AND key='publications') ORDER BY domain,key").all();
  }finally{database.close();}
}


test('real MCP client configures the same running dev Cockpit as its UI', { timeout: 360000 }, async () => {
  let page = await nativePage();
  const credentials = [], clients = [], profiles = new Set(), evidence = { client: '@modelcontextprotocol/sdk 1.32.1', hardwareValidation: 'Software acceptance; no physical-device qualification' };
  const artifacts = resolve('.research/mcp-native', String(Date.now())); await mkdir(artifacts, { recursive: true });
  let settings, writer, endpoint, initialCompanionState, initialCompanionCa;
  try {
    if(process.env.OSG_MCP_EXPECT_NO_COMPANIONS){
      initialCompanionState=await until(async()=>{
        const records=await companionState(page);
        return records.some(record=>record.key==='companion-identity')?records:null;
      },'Native startup did not persist its own companion CA');
      initialCompanionCa=(await page.native({command:'companion_browser_certificate'})).companions.certificatePem;
    }
    await until(() => page.evaluate(() => typeof window.__TAURI_INTERNALS__?.invoke === 'function'
      && !!document.querySelector('nav[aria-label="Main navigation"]')), 'The native UI bridge did not finish startup');
    settings = await page.invoke('mcp_settings_get');
    await page.native({ command: 'profile_stop' });
    const write = await page.invokeMcp('mcp_credential_create', { name: 'Live acceptance writer', scope: 'write' }); credentials.push(write.credential.id);
    const read = await page.invokeMcp('mcp_credential_create', { name: 'Live acceptance reader', scope: 'read' }); credentials.push(read.credential.id);
    const status = await page.invokeMcp('mcp_settings_update', { settings: { enabled: true, port: settings.port } }); endpoint = status.endpoint;
    await until(async () => (await page.invoke('mcp_settings_get')).running, 'Native MCP server did not start');
    writer = await connect(endpoint, write.token); clients.push(writer);
    const reader = await connect(endpoint, read.token); clients.push(reader);
    evidence.server = writer.getServerVersion();
    evidence.protocolVersion = writer.transport.protocolVersion;
    const writeTools = (await writer.listTools()).tools, readTools = (await reader.listTools()).tools;
    assert.ok(writeTools.some(tool => tool.name === 'profile_save_prepared')); assert.ok(!readTools.some(tool => tool.name === 'profile_create'));
    assert.ok(readTools.every(tool => tool.annotations.readOnlyHint));
    evidence.tools = { write: writeTools.length, read: readTools.length };
    const integration=await ok(writer,'mcp_settings_get');assert.ok(Number.isSafeInteger(integration.settingsRevision));assert.doesNotMatch(JSON.stringify(integration),/osg_mcp_/);
    const temporary=await ok(writer,'mcp_credential_create',mutation({name:'SDK-managed reader',scope:'read',expectedSettingsRevision:integration.settingsRevision}));credentials.push(temporary.credential.id);
    const replaced=await ok(writer,'mcp_credential_rotate',mutation({id:temporary.credential.id,expectedSettingsRevision:temporary.status.settingsRevision}));credentials.splice(credentials.indexOf(temporary.credential.id),1);credentials.push(replaced.credential.id);
    assert.ok(!replaced.status.credentials.some(c=>c.id===temporary.credential.id));await assert.rejects(()=>connect(endpoint,temporary.token));
    const managed=await connect(endpoint,replaced.token);clients.push(managed);assert.ok((await managed.listTools()).tools.every(tool=>tool.annotations.readOnlyHint));
    const revoked=await ok(writer,'mcp_credential_revoke',mutation({id:replaced.credential.id,expectedSettingsRevision:replaced.status.settingsRevision}));credentials.splice(credentials.indexOf(replaced.credential.id),1);assert.ok(!revoked.credentials.some(c=>c.id===replaced.credential.id));await assert.rejects(()=>managed.listTools());
    evidence.integrationSettings={sameNativeRevision:true,credentialCreation:true,atomicReplacement:true,revocation:true,hashOnlyStatus:true};
    // The pinned JS client negotiates 2025-11-25. Also exercise modern
    // per-request metadata/header semantics directly against the same server.
    const modern = async (method, params, extraHeaders = {}) => {
      const version = extraHeaders['MCP-Protocol-Version'] ?? '2026-07-28';
      return fetch(endpoint, {
        method: 'POST', signal: AbortSignal.timeout(10000),
        headers: { Authorization: `Bearer ${read.token}`, Accept: 'application/json, text/event-stream', 'Content-Type': 'application/json', 'MCP-Protocol-Version': version, 'Mcp-Method': method, ...(params.name || params.uri ? { 'Mcp-Name': params.name ?? params.uri } : {}), ...extraHeaders },
        body: JSON.stringify({ jsonrpc: '2.0', id: randomUUID(), method, params: { ...params, _meta: { 'io.modelcontextprotocol/protocolVersion': version, 'io.modelcontextprotocol/clientInfo': { name: 'Cockpit modern transport acceptance', version: '1' }, 'io.modelcontextprotocol/clientCapabilities': {} } } }),
      });
    };
    const modernTools = await modern('tools/list', {}); assert.equal(modernTools.status, 200);
    assert.equal((await modernTools.json()).result.tools.length, readTools.length);
    const modernStatus = await modern('tools/call', { name: 'profile_status', arguments: {} }); assert.equal(modernStatus.status, 200);
    assert.equal((await modernStatus.json()).result.structuredContent.ok, true);
    const modernResource = await modern('resources/read', { uri: 'cockpit://guide/configuration' }); assert.equal(modernResource.status, 200); assert.ok((await modernResource.json()).result.contents.length);
    const mismatch = await modern('tools/list', {}, { 'Mcp-Method': 'resources/list' }); assert.equal(mismatch.status, 400); assert.equal((await mismatch.json()).error.code, -32020);
    const unsupported = await modern('tools/list', {}, { 'MCP-Protocol-Version': '2099-01-01' }); assert.equal(unsupported.status, 400); assert.ok((await unsupported.json()).error);
    const malformed = await fetch(endpoint, { method: 'POST', signal: AbortSignal.timeout(10000), headers: { Authorization: `Bearer ${read.token}`, Accept: 'application/json, text/event-stream', 'Content-Type': 'application/json', 'MCP-Protocol-Version': '2025-11-25' }, body: '{' }); assert.ok([400, 415].includes(malformed.status)); await malformed.text();
    const oversized = await fetch(endpoint, { method: 'POST', signal: AbortSignal.timeout(10000), headers: { Authorization: `Bearer ${read.token}`, Accept: 'application/json, text/event-stream', 'Content-Type': 'application/json', 'MCP-Protocol-Version': '2025-11-25' }, body: JSON.stringify({ padding: 'x'.repeat(2_097_152) }) }); assert.equal(oversized.status, 413);
    evidence.protocolChecks = { negotiated: evidence.protocolVersion, modern: '2026-07-28', modernToolsResourcesAndCall: true, mismatchedHeaderRejected: true, unsupportedVersionRejected: true, malformedRejected: malformed.status, oversizedRejected: oversized.status };
    await assert.rejects(() => reader.callTool({ name: 'profile_create', arguments: mutation({ name: 'Denied write' }) }));
    assert.equal((await fetch(endpoint)).status, 401);
    const badOrigin = await fetch(endpoint, { method: 'POST', headers: { Authorization: `Bearer ${write.token}`, Origin: 'https://example.invalid', Accept: 'application/json, text/event-stream', 'Content-Type': 'application/json' }, body: JSON.stringify({ jsonrpc: '2.0', id: 1, method: 'tools/list' }) }); assert.equal(badOrigin.status, 403);
    const context = await writer.listResources(); assert.ok(context.resources.some(resource => resource.uri === 'cockpit://schemas/profile'));
    await writer.readResource({ uri: 'cockpit://guide/configuration' });
    const inventory = await ok(writer, 'resources_list', { limit: 1 }); evidence.resourcePage = inventory;
    if (inventory.resources[0]?.portCount > 0) {
      const resourceId = inventory.resources[0].id;
      const inspected = await ok(reader, 'resource_inspect', { resourceId, limit: 1 });
      assert.equal(inspected.ports.length, 1);
      const firstPort = inspected.ports[0];
      const nativePorts = (await page.native({ command: 'profile_resources_inspect', resourceId, offset: 0, limit: 1 })).profileResources;
      assert.deepEqual(inspected.ports, nativePorts.ports);
      const searched = await ok(reader, 'resource_search_ports', { resourceId, search: firstPort.descriptor.id, limit: 1 });
      assert.ok(searched.ports.some(port => port.descriptor.id === firstPort.descriptor.id)); assert.ok(searched.capabilityStamp);
      evidence.resourcePort = { resourceId, portId: firstPort.descriptor.id, nativeMetadataAgrees: true, boundedSearchVerified: true };
    }
    const games = await ok(writer, 'games_list', { limit: 1 }); assert.equal(games.catalogue.length, 1); assert.ok(games.total >= 1); evidence.gamesTotal = games.total;
    const operations = [];
    let operationOffset = 0, operationsTotal;
    do {
      const result = await ok(reader, 'processing_operations_list', { offset: operationOffset, limit: 3 });
      operationsTotal = result.total; operations.push(...result.operations);
      operationOffset = result.nextOffset;
    } while (operationOffset !== null);
    assert.equal(operations.length, operationsTotal); assert.equal(new Set(operations.map(item => item.kind)).size, operationsTotal);
    assert.ok(operations.some(item => item.kind === 'not')); assert.ok(operations.every(item => item.schemaHash && item.parametersSchema));
    const vehicles = await ok(reader, 'game_vehicles_list', { gameId: games.catalogue[0].id, limit: 1 });
    assert.ok(vehicles.catalogueStamp); assert.ok(vehicles.vehicles.length <= 1); assert.ok(vehicles.total >= vehicles.vehicles.length);
    evidence.discovery = { processingOperations: operationsTotal, processingPaginationVerified: true, vehicleCataloguePaged: true };
    await ok(reader, 'companions_list', { limit: 1 }); const companionPanels=await ok(reader, 'companion_panels_list', { limit: 1 });
    if(process.env.OSG_MCP_EXPECT_NO_COMPANIONS){
      assert.deepEqual(companionPanels.panels,[]);
      const listener=await ok(reader,'companion_listener_get');assert.equal(listener.enabled,false);
      assert.equal((await ok(reader,'companion_usb_status')).usb.connected,false);
      assert.ok(!readTools.some(tool=>tool.name==='companion_certificate_get'));
      assert.deepEqual(await companionState(page),initialCompanionState,'Passive MCP reads must preserve startup trust and publication records');evidence.passiveCompanions={emptyCatalogue:true,listenerRead:true,startupTrustPreserved:true};
    }
    const name = `MCP acceptance ${Date.now()}`;
    const prepared = await ok(writer, 'profile_prepare_changes', { name, expectedRevision: null });
    assert.equal(prepared.effects.saved, false); assert.equal(prepared.validation.valid, true); assert.ok(prepared.diff.totalChanges > 0);
    const before = await ok(writer, 'profiles_list'); assert.ok(!before.profiles.some(profile => profile.id === prepared.profileId));
    const saveArgs = mutation({ draftId: prepared.draftId });
    await ok(writer, 'profile_save_prepared', saveArgs); profiles.add(prepared.profileId);
    const saved = await ok(writer, 'profile_get', { id: prepared.profileId }); assert.equal(saved.document.name, name);
    const nativeSaved = (await page.native({ command: 'profile_assignments_get', id: prepared.profileId })).profiles; assert.deepEqual(saved.document, nativeSaved.document);
    assert.equal((await ok(writer, 'profile_status')).profileSession.owner, null);
    await ok(writer, 'profile_save_prepared', saveArgs);
    const freshConnection = await connect(endpoint, write.token); clients.push(freshConnection);
    await ok(freshConnection, 'profile_save_prepared', saveArgs); // same credential across reconnect
    assert.equal((await ok(writer, 'profile_get', { id: prepared.profileId })).document.revision, saved.document.revision);
    const changedPayload = await call(writer, 'profile_save_prepared', { ...saveArgs, draftId: randomUUID() }); assert.equal(changedPayload.error.code, 'operation_conflict');
    const lostDraft = await ok(writer, 'profile_prepare_changes', { name: `MCP lost response ${Date.now()}`, expectedRevision: null });
    profiles.add(lostDraft.profileId);
    const lostArgs = mutation({ draftId: lostDraft.draftId });
    const unreadReply = await modern('tools/call', { name: 'profile_save_prepared', arguments: lostArgs }, { Authorization: `Bearer ${write.token}` });
    assert.equal(unreadReply.status, 200);
    await unreadReply.body.cancel(); // deliberately lose the committed save result
    await writer.close();
    writer = await connect(endpoint, write.token); clients.push(writer); await writer.listTools();
    const reconciled = await ok(writer, 'operation_status', { operationId: lostArgs.operationId });
    assert.equal(reconciled.state, 'completed'); assert.equal(reconciled.result.ok, true);
    const recoveredProfile = await ok(writer, 'profile_get', { id: lostDraft.profileId });
    await ok(writer, 'profile_save_prepared', lostArgs);
    assert.equal((await ok(writer, 'profile_get', { id: lostDraft.profileId })).document.revision, recoveredProfile.document.revision);
    evidence.lostReply = { unreadSaveReplyDiscarded: true, reconnected: true, originalOperationRecovered: true, exactReplayDidNotDuplicate: true };
    const stale = await ok(writer, 'profile_prepare_changes', { profileId: prepared.profileId, expectedRevision: saved.document.revision, edits: [{ kind: 'set_name', name: `${name} AI pending` }] });
    await page.native({ command: 'profile_rename', id: prepared.profileId, expectedRevision: saved.document.revision, name: `${name} UI edit` });
    const staleSave = await call(writer, 'profile_save_prepared', mutation({ draftId: stale.draftId })); assert.equal(staleSave.ok, false); assert.equal(staleSave.error.code, 'revision_conflict');
    const edited = await ok(writer, 'profile_get', { id: prepared.profileId });
    const applyArgs = mutation({ id: prepared.profileId, expectedRevision: edited.document.revision, expectedActiveOwner: null });
    await ok(writer, 'profile_apply', applyArgs);
    const running = await until(async () => { const state = await ok(writer, 'profile_status'); return state.profileSession.owner?.profileId === prepared.profileId && state; }, 'Profile failed to apply', 45000);
    evidence.appliedOwner = running.profileSession.owner;
    assert.deepEqual((await page.native({ command: 'profile_status' })).profiles.profileSession.owner, running.profileSession.owner);
    const wrongOwner = await call(writer, 'profile_apply', mutation({ id: prepared.profileId, expectedRevision: edited.document.revision, expectedActiveOwner: null })); assert.equal(wrongOwner.ok, false);
    const activeDraft = await ok(writer, 'profile_prepare_changes', { profileId: prepared.profileId, expectedRevision: edited.document.revision, edits: [{ kind: 'set_name', name: `${name} saved only` }] });
    await ok(writer, 'profile_save_prepared', mutation({ draftId: activeDraft.draftId }));
    assert.deepEqual((await ok(writer, 'profile_status')).profileSession.owner, running.profileSession.owner, 'Saving an active profile must not replace its applied revision');
    await ok(writer, 'activation_status', { activationId: applyArgs.operationId });
    await ok(writer, 'profile_stop', mutation({})); assert.equal((await ok(writer, 'profile_status')).profileSession.owner, null);
    const local = await ok(writer, 'local_settings_get', { scope: 'selection-mode' });
    const nextMode = local.settings.mode === 'manual' ? 'automatic' : 'manual';
    const localArgs = mutation({ scope: 'selection-mode', expectedSettingsRevision: local.revision, expectedActiveOwner: null, settings: { mode: nextMode } });
    await ok(writer, 'local_settings_save', localArgs);
    assert.equal((await call(writer, 'local_settings_save', mutation({ ...localArgs, operationId: randomUUID(), settings: { mode: local.settings.mode } }))).ok, false, 'Stale local settings must conflict');
    const currentLocal = await ok(writer, 'local_settings_get', { scope: 'selection-mode' });
    await ok(writer, 'local_settings_save', mutation({ scope: 'selection-mode', expectedSettingsRevision: currentLocal.revision, expectedActiveOwner: (await ok(writer, 'profile_status')).profileSession.owner, settings: { mode: local.settings.mode } }));
    await softwareScenarios({ page, writer, connect, endpoint, token: write.token, profiles, evidence, ok, call, mutation });
    if(process.env.OSG_MCP_COMPANION_ACCEPTANCE)await companionScenarios({page,reader,connect,endpoint,ok,call,mutation,evidence});
    if(process.env.OSG_MCP_UI_PREFERENCES_ACCEPTANCE)await uiPreferenceScenarios({page,reader,writer,ok,call,mutation,evidence});
    await page.evaluate(() => [...document.querySelectorAll('button')].find(button => button.textContent.trim() === 'Settings').click());
    await until(() => page.evaluate(() => [...document.querySelectorAll('[role=tab]')].some(button => button.textContent.trim() === 'AI integration')), 'AI integration tab is missing');
    await page.evaluate(() => [...document.querySelectorAll('[role=tab]')].find(button => button.textContent.trim() === 'AI integration').click());
    await until(() => page.evaluate(() => document.querySelector('[aria-label="AI integration settings"]')?.textContent.includes('Listening')), 'Live settings UI does not reflect listener state');
    const ui = await page.evaluate(() => ({ text: document.querySelector('[aria-label="AI integration settings"]').textContent, title: document.title }));
    assert.ok(ui.text.includes('Live acceptance writer')); assert.ok(!ui.text.includes(write.token)); assert.ok(!ui.text.includes(read.token));
    evidence.settingsUi = { credentialsVisible: true, secretsHidden: true };
    const screenshot = await page.send('Page.captureScreenshot', { format: 'png' }); await writeFile(resolve(artifacts, 'mcp-settings.png'), Buffer.from(screenshot.data, 'base64'));
    await page.invokeMcp('mcp_credential_revoke', { id: read.credential.id }); credentials.splice(credentials.indexOf(read.credential.id), 1);
    await assert.rejects(() => reader.listTools());
    await page.invokeMcp('mcp_settings_update', { settings: { enabled: false, port: settings.port } });
    await assert.rejects(() => writer.listTools());
    await page.invokeMcp('mcp_settings_update', { settings: { enabled: true, port: settings.port } });
    writer = await connect(endpoint, write.token); clients.push(writer);
    assert.equal((await ok(writer, 'profile_get', { id: prepared.profileId })).document.name, `${name} saved only`);
    if (process.env.OSG_MCP_RESTART_EXECUTABLE) {
      page.close();
      await promisify(execFile)('powershell.exe', ['-NoProfile', '-NonInteractive', '-ExecutionPolicy', 'Bypass', '-File', resolve('tests/ui/restart-mcp-cockpit.ps1'), '-Executable', process.env.OSG_MCP_RESTART_EXECUTABLE, '-Workspace', process.cwd(), '-ConfigPath', process.env.OSG_MCP_CONFIG_PATH, '-UiPort', String(new URL(process.env.OSG_NATIVE_DEV_ORIGIN).port), '-DebugPort', String(new URL(process.env.OSG_NATIVE_CDP_URL).port)], { timeout: 45000 });
      page = await until(async () => { const candidate = await nativePage(); try { const state = await candidate.invoke('mcp_settings_get'); if (state.running) return candidate; } catch {} candidate.close(); return null; }, 'Restarted native Cockpit did not restore MCP', 45000);
      writer = await connect(endpoint, write.token); clients.push(writer);
      assert.equal((await ok(writer, 'profile_get', { id: prepared.profileId })).document.name, `${name} saved only`);
      assert.equal((await ok(writer, 'profile_status')).profileSession.owner, null, 'Restart must not replay an applied profile');
      assert.equal((await call(writer, 'operation_status', { operationId: saveArgs.operationId })).error.code, 'operation_unknown');
      if(process.env.OSG_MCP_EXPECT_NO_COMPANIONS){
        // Startup may renew the leaf and reseal the identity record; the root stays exact.
        assert.equal((await page.native({command:'companion_browser_certificate'})).companions.certificatePem,initialCompanionCa);
        assert.deepEqual((await companionState(page)).map(record=>[record.domain,record.key]),initialCompanionState.map(record=>[record.domain,record.key]));
      }
      evidence.restart = { settingsRestored: true, credentialRestored: true, savedProfileRestored: true, appliedRunNotReplayed: true, volatileLedgerReconciled: true };
    }
    evidence.completed = true; evidence.checkedAt = new Date().toISOString();
    await writeFile(resolve(artifacts, 'evidence.json'), JSON.stringify(evidence, null, 2));
  } finally {
    for (const client of clients) await client.close().catch(() => {});
    for (const id of profiles) { try { await page.native({ command: 'profile_stop' }); const state = (await page.native({ command: 'profile_get', id })).profiles; await page.native({ command: 'profile_delete', id, expectedRevision: state.document.revision }); } catch {} }
    for (const id of credentials) await page.invokeMcp('mcp_credential_revoke', { id }).catch(() => {});
    if (settings) await page.invokeMcp('mcp_settings_update', { settings: { enabled: settings.enabled, port: settings.port } }).catch(() => {});
    page.close();
  }
});
