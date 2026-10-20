// Actual USB companion panel takeover in the already-running native Cockpit.
// Supply OSG_PANEL_SWITCH_PROFILE_ID for a saved, panel-only fixture; no profiles,
// pairings or publications are created or edited by this acceptance.
import test from 'node:test';
import assert from 'node:assert/strict';
import { createRequire } from 'node:module';
import { mkdir, writeFile } from 'node:fs/promises';
import { resolve } from 'node:path';

const require = createRequire(import.meta.url), modulePath = process.env.OSG_PLAYWRIGHT_MODULE ?? 'playwright';
const { ws: RawSocket } = createRequire(require.resolve(modulePath))('playwright-core/lib/utilsBundle');
const artifacts = resolve('.research/profile-companion-panel-switch-native');
const sleep = ms => new Promise(resolve => setTimeout(resolve, ms));

async function connectNative() {
  const targets = await (await fetch(`${process.env.OSG_NATIVE_CDP_URL ?? 'http://127.0.0.1:9222'}/json/list`, { signal: AbortSignal.timeout(5000) })).json();
  const endpoint = process.env.OSG_NATIVE_PAGE_WS ?? targets.find(target => target.type === 'page' && /127\.0\.0\.1:1420|localhost:1420|tauri\.localhost/.test(target.url))?.webSocketDebuggerUrl;
  assert.ok(endpoint, 'The running native dev Cockpit must expose its page debugging target.');
  const socket = new RawSocket(endpoint, { handshakeTimeout: 8000 });
  await new Promise((resolve, reject) => { socket.once('open', resolve); socket.once('error', () => reject(new Error('Native debugging connection failed.'))); });
  let sequence = 0; const pending = new Map();
  socket.on('error', () => {});
  socket.on('message', bytes => {
    const message = JSON.parse(bytes.toString()), operation = pending.get(message.id);
    if (!operation) return;
    clearTimeout(operation.timer); pending.delete(message.id);
    if (message.error) operation.reject(new Error(message.error.message)); else operation.resolve(message.result);
  });
  socket.on('close', () => { for (const operation of pending.values()) { clearTimeout(operation.timer); operation.reject(new Error('Native debugging connection closed.')); } pending.clear(); });
  const send = (method, params = {}) => new Promise((resolve, reject) => {
    const id = ++sequence, timer = setTimeout(() => { pending.delete(id); reject(new Error(`Native CDP ${method} timed out.`)); }, 90000);
    pending.set(id, { resolve, reject, timer }); socket.send(JSON.stringify({ id, method, params }));
  });
  return {
    send,
    async evaluate(expression) {
      const result = await send('Runtime.evaluate', { expression, returnByValue: true, awaitPromise: true });
      if (result.exceptionDetails) throw new Error(result.exceptionDetails.exception?.description ?? result.exceptionDetails.text);
      return result.result.value;
    },
    close() { socket.close(); },
  };
}
async function until(operation, description, timeout = 15000) {
  const end = Date.now() + timeout; let last;
  while (Date.now() < end) { try { const value = await operation(); if (value) return value; } catch (error) { last = error; } await sleep(150); }
  throw new Error(`${description}${last ? ': ' + last.message : ''}`);
}

test('native USB companion finishes panel loading before activation and restores its original panel on deactivation', { timeout: 180000 }, async () => {
  const profileId = process.env.OSG_PANEL_SWITCH_PROFILE_ID;
  assert.ok(profileId, 'Set OSG_PANEL_SWITCH_PROFILE_ID to an existing panel-only acceptance profile.');
  await mkdir(artifacts, { recursive: true });
  const native = await connectNative(), evidence = { actualNativeCockpit: true, existingUsbCompanion: true, checks: [] };
  let activationAttempted = false, original, companionId;
  const request = async value => {
    const response = await native.evaluate(`window.__TAURI_INTERNALS__.invoke('gateway_request',{request:${JSON.stringify({ ...value, clientId: 'profile-companion-panel-switch-native' })}})`);
    assert.equal(response.ok, true, response.error ?? `Native ${value.command} failed`); return response.state;
  };
  const status = async () => (await request({ command: 'profile_status' })).profiles.profileSession;
  const companions = async () => (await request({ command: 'companion_status' })).companions;
  const profiles = async () => (await request({ command: 'profile_list' })).profiles.profiles;
  const saved = async id => { const value = (await request({ command: 'profile_assignments_get', id })).profiles; return { document: value.document, assignments: value.assignments }; };
  const click = (label, scope = '.profiles-workspace') => until(() => native.evaluate(`(()=>{const root=document.querySelector(${JSON.stringify(scope)});const button=[...(root?.querySelectorAll('button')??[])].find(button=>(button.getAttribute('aria-label')??button.textContent.replace(/\\s+/g,' ').trim())===${JSON.stringify(label)});if(!button||button.disabled)return false;button.scrollIntoView({block:'nearest'});button.click();return true;})()`), `UI action unavailable: ${label}`);
  const capture = async name => { const value = await native.send('Page.captureScreenshot', { format: 'png' }); await writeFile(resolve(artifacts, name), Buffer.from(value.data, 'base64')); };
  const instance = value => value.host.instances.find(row => row.companionId === companionId);
  const assignmentShape = value => value.host.instances.map(row => ({ instanceId: row.instanceId, companionId: row.companionId, publicationHash: row.publicationHash })).sort((a, b) => a.instanceId.localeCompare(b.instanceId));
  try {
    const initialStatus = await status(), list = await profiles(), beforeSaved = await Promise.all(list.map(profile => saved(profile.id)));
    assert.equal(initialStatus.state, 'stopped'); assert.equal(initialStatus.owner, null); assert.equal(initialStatus.fault, null);
    const profile = list.find(profile => profile.id === profileId), authored = beforeSaved.find(row => row.document.id === profileId);
    assert.ok(profile && authored); assert.equal(authored.document.resources.length, 1); assert.equal(authored.document.resources[0].kind, 'companion');
    for (const key of ['bindings', 'processingNodes', 'virtualOutputs', 'desktopOutputs', 'overrides', 'visibility']) assert.equal(authored.document[key]?.length ?? 0, 0, `The fixture must be panel-only: ${key}`);
    assert.equal(authored.document.companionAssignments.length, 1); assert.equal(authored.assignments.length, 1);
    assert.equal(authored.document.resources[0].ports.length, 0); assert.equal(authored.assignments[0].ports.length, 0);
    const inventory = (await request({ command: 'profile_resources_list', offset: 0, limit: 128 })).profileResources;
    const resource = inventory.resources.find(row => row.id === authored.assignments[0].sourceId);
    assert.ok(resource?.attributes?.companionId); companionId = resource.attributes.companionId;
    const beforeCompanions = await companions(); original = instance(beforeCompanions);
    assert.ok(original); assert.notEqual(original.publicationHash, authored.document.companionAssignments[0].publicationHash);
    const initial = await until(async () => { const value = await companions(), row = instance(value); return row?.armed && row.transport === 'usb' && row.publicationHash === original.publicationHash ? value : null; }, 'The exact original USB companion must be armed before takeover', 60000);
    evidence.original = { instanceId: original.instanceId, companionId, publicationHash: original.publicationHash };
    evidence.desiredPublicationHash = authored.document.companionAssignments[0].publicationHash;
    await click('Profiles', 'nav[aria-label="Main navigation"]');
    const started = Date.now(); activationAttempted = true;
    await click(`Activate ${profile.name}`);
    const active = await until(async () => { const value = await status(); return value.fault || (value.owner?.profileId === profileId && value.state === 'running') ? value : null; }, 'The profile must activate after its real USB panel finishes loading', 70000);
    assert.equal(active.fault, null, active.fault ?? 'The actual panel must activate without a fault.');
    evidence.activationMs = Date.now() - started;
    const current = await companions(), acquired = instance(current);
    assert.equal(acquired.publicationHash, evidence.desiredPublicationHash); assert.equal(acquired.armed, true); assert.equal(acquired.connected, true); assert.equal(acquired.transport, 'usb');
    assert.ok(Object.values(acquired.values ?? {}).every(value => value === false), 'The actual peer must supply neutral controls.');
    evidence.active = { profileId: active.owner.profileId, state: active.state, instanceId: acquired.instanceId, publicationHash: acquired.publicationHash, armed: acquired.armed, neutralValues: acquired.values };
    evidence.checks.push('UI activation waits for the actual USB peer to load the desired panel and supply its neutral baseline');
    await until(() => native.evaluate(`Array.from(document.querySelectorAll('.profile-tile.profile-active button')).some(button=>button.getAttribute('aria-label')===${JSON.stringify(`Deactivate ${profile.name}`)})`), 'The activated profile tile must reflect native ownership');
    await capture('desired-panel-active.png');
    const deactivatedAt = Date.now(); await click(`Deactivate ${profile.name}`);
    await until(async () => { const value = await status(); return value.state === 'stopped' && !value.owner && !value.fault ? value : null; }, 'The real Deactivate button must release profile ownership');
    const restored = await until(async () => { const value = await companions(), row = instance(value); return row?.publicationHash === original.publicationHash && row.armed && row.transport === 'usb' ? value : null; }, 'The exact original panel must finish reloading and establish its baseline after deactivation', 70000);
    evidence.restorationMs = Date.now() - deactivatedAt;
    assert.deepEqual(assignmentShape(restored), assignmentShape(initial));
    assert.deepEqual(restored.host.clients.map(row => row.companionId).sort(), initial.host.clients.map(row => row.companionId).sort());
    assert.deepEqual(await profiles(), list); assert.deepEqual(await Promise.all(list.map(profile => saved(profile.id))), beforeSaved);
    evidence.checks.push('UI deactivation restores the exact original panel; every saved graph, local assignment and paired installation remains unchanged');
    await capture('original-panel-restored.png');
  } catch (error) { evidence.error = error.message; await capture('failure.png').catch(() => {}); throw error; }
  finally {
    if (activationAttempted) {
      const current = await status().catch(() => null);
      if (current?.owner?.profileId === profileId) await request({ command: 'profile_stop' });
      else if (current && ['faulted', 'recovery-needed'].includes(current.state)) await request({ command: 'profile_recover' });
      if (original) await until(async () => { const row = instance(await companions()); return row?.publicationHash === original.publicationHash && row.armed; }, 'Restore the original USB panel after acceptance', 70000);
    }
    evidence.finalStatus = await status().catch(() => null);
    await writeFile(resolve(artifacts, 'evidence.json'), JSON.stringify(evidence, null, 2) + '\n'); native.close();
  }
});
