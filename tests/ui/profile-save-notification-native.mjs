// Actual native dev Cockpit, UI saves and passive DOM observation. No mocked IPC or notification overrides.
import test from 'node:test';
import assert from 'node:assert/strict';
import { createRequire } from 'node:module';
import { randomUUID } from 'node:crypto';
import { mkdir, writeFile } from 'node:fs/promises';
import { resolve } from 'node:path';

const require = createRequire(import.meta.url), modulePath = process.env.OSG_PLAYWRIGHT_MODULE ?? 'playwright';
const { ws: RawSocket } = createRequire(require.resolve(modulePath))('playwright-core/lib/utilsBundle');
const artifacts = resolve('.research/profile-save-notification-native');
const sleep = ms => new Promise(resolve => setTimeout(resolve, ms));

async function connectNative() {
  const targets = await (await fetch(`${process.env.OSG_NATIVE_CDP_URL ?? 'http://127.0.0.1:9222'}/json/list`, { signal: AbortSignal.timeout(5000) })).json();
  const endpoint = process.env.OSG_NATIVE_PAGE_WS ?? targets.find(target => target.type === 'page' && /127\.0\.0\.1:1420|localhost:1420|tauri\.localhost/.test(target.url))?.webSocketDebuggerUrl;
  assert.ok(endpoint, 'The running native dev Cockpit must expose its page debugging target.');
  const socket = new RawSocket(endpoint, { handshakeTimeout: 8000 });
  await new Promise((resolve, reject) => { socket.once('open', resolve); socket.once('error', () => reject(new Error('Native page debugging connection failed.'))); });
  let sequence = 0; const pending = new Map();
  socket.on('error', () => {});
  socket.on('message', bytes => {
    const message = JSON.parse(bytes.toString()), operation = pending.get(message.id);
    if (!operation) return;
    clearTimeout(operation.timer); pending.delete(message.id);
    if (message.error) operation.reject(new Error(message.error.message)); else operation.resolve(message.result);
  });
  socket.on('close', () => { for (const operation of pending.values()) { clearTimeout(operation.timer); operation.reject(new Error('Native page debugging connection closed.')); } pending.clear(); });
  const send = (method, params = {}) => new Promise((resolve, reject) => {
    const id = ++sequence, timer = setTimeout(() => { pending.delete(id); reject(new Error(`Native CDP ${method} timed out.`)); }, 12000);
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
  while (Date.now() < end) { try { const value = await operation(); if (value) return value; } catch (error) { last = error; } await sleep(100); }
  throw new Error(`${description}${last ? ': ' + last.message : ''}`);
}

// Each newly created countdown span marks either a new toast or a timer reset on
// an existing deduplicated toast, so polling cannot hide a replay by reusing DOM.
const observeToasts = `(() => {
  window.__saveToastObserver?.disconnect();
  window.__saveToastEvidence = [];
  const seen = new WeakSet(), started = Date.now();
  const record = () => {
    for (const toast of document.querySelectorAll('[aria-label="Notifications"] .toast')) {
      const message = toast.querySelector('p')?.textContent.trim(), timer = toast.querySelector('.toast-countdown-fill');
      if (!message?.startsWith('Profile saved') || !timer || seen.has(timer)) continue;
      seen.add(timer); window.__saveToastEvidence.push({ message, elapsedMs: Date.now() - started, at: new Date().toISOString() });
    }
  };
  window.__saveToastObserver = new MutationObserver(record);
  window.__saveToastObserver.observe(document, { subtree: true, childList: true });
  record();
})()`;

test('one real save shows one toast across inventory polling and navigation, while another save notifies again', { timeout: 90000 }, async () => {
  await mkdir(artifacts, { recursive: true });
  const native = await connectNative(), name = `Save notification acceptance ${randomUUID().slice(0, 8)}`;
  let profileId, draftOpen = false;
  const request = async value => {
    const response = await native.evaluate(`window.__TAURI_INTERNALS__.invoke('gateway_request',{request:${JSON.stringify({ ...value, clientId: 'profile-save-notification-native' })}})`);
    assert.equal(response.ok, true, response.error ?? `Native ${value.command} failed`); return response.state;
  };
  const profiles = async () => (await request({ command: 'profile_list' })).profiles.profiles;
  const saved = async () => (await request({ command: 'profile_assignments_get', id: profileId })).profiles.document;
  const companionIds = async () => (await request({ command: 'companion_status' })).companions.host.clients.map(client => client.companionId).sort();
  const click = (label, scope = '.profiles-workspace') => until(() => native.evaluate(`(() => { const button = [...(document.querySelector(${JSON.stringify(scope)})?.querySelectorAll('button') ?? [])].find(button => (button.getAttribute('aria-label') ?? button.textContent.replace(/\\s+/g, ' ').trim()) === ${JSON.stringify(label)}); if (!button || button.disabled) return false; button.click(); return true; })()`), `Native UI action unavailable: ${label}`);
  const inputName = value => native.evaluate(`(() => { const input = document.querySelector('.profile-name-input'); if (!input) return false; input.value = ${JSON.stringify(value)}; input.dispatchEvent(new Event('input', { bubbles: true })); input.dispatchEvent(new KeyboardEvent('keydown', { key: 'Enter', bubbles: true, cancelable: true })); return true; })()`);
  const visibleToasts = () => native.evaluate(`Array.from(document.querySelectorAll('[aria-label="Notifications"] .toast p')).map(node => node.textContent.trim()).filter(message => message.startsWith('Profile saved'))`);
  const evidence = () => native.evaluate('window.__saveToastEvidence');
  const capture = async name => { const value = await native.send('Page.captureScreenshot', { format: 'png' }); await writeFile(resolve(artifacts, name), Buffer.from(value.data, 'base64')); };
  let profilesBefore, companionsBefore;
  try {
    assert.equal(await native.evaluate('typeof window.__TAURI_INTERNALS__?.invoke'), 'function');
    const status = (await request({ command: 'profile_status' })).profiles.profileSession;
    assert.equal(status.owner, null, 'Run after any diagnostic activation is deactivated.');
    assert.equal(status.state, 'stopped', 'Run after any diagnostic fault is recovered.');
    profilesBefore = await profiles(); companionsBefore = await companionIds();
    await click('Profiles', 'nav[aria-label="Main navigation"]');
    await until(() => native.evaluate(`!![...document.querySelectorAll('.profiles-workspace h1')].find(heading => heading.textContent === 'Profiles')`), 'Open the saved profile list before this test');
    await native.send('Input.dispatchMouseEvent', { type: 'mouseMoved', x: 20, y: 20 });
    await native.evaluate(`for (const toast of document.querySelectorAll('[aria-label="Notifications"] .toast')) if (toast.querySelector('p')?.textContent.trim().startsWith('Profile saved')) toast.querySelector('button').click()`);
    await native.evaluate(observeToasts);
    await click('Add'); draftOpen = true;
    await click('Edit profile name'); assert.equal(await inputName(name), true);
    await click('Save profile');
    profileId = await until(async () => (await profiles()).find(profile => profile.name === name)?.id, 'The temporary profile was not saved'); draftOpen = false;
    const first = await saved();
    assert.equal(first.resources.length, 0); assert.equal(first.bindings.length, 0);
    await until(async () => (await visibleToasts()).length === 1, 'The actual save did not show its toast');
    await capture('one-save-toast.png');
    await sleep(13200); // Four real 3-second inventory polls, beyond the 6-second toast lifetime.
    assert.deepEqual(await visibleToasts(), [], 'The original save toast must expire without a polling reset.');
    const firstEvents = await evidence(); assert.equal(firstEvents.length, 1, 'One actual save must start exactly one notification countdown.');
    assert.equal((await saved()).revision, first.revision, 'Inventory polls must not save the profile.');

    await click('Overview', 'nav[aria-label="Main navigation"]');
    await click('Profiles', 'nav[aria-label="Main navigation"]');
    await sleep(3300);
    assert.equal((await evidence()).length, 1, 'Returning to Profiles must not replay its retained save message.');
    assert.deepEqual(await visibleToasts(), []);
    await native.send('Page.reload', { ignoreCache: true });
    await until(() => native.evaluate(`typeof window.__TAURI_INTERNALS__?.invoke === 'function' && !!document.querySelector('nav[aria-label="Main navigation"]')`), 'Native frontend did not remount');
    await native.evaluate(observeToasts);
    await click('Profiles', 'nav[aria-label="Main navigation"]');
    if (await native.evaluate(`!!document.querySelector('.profiles-workspace .back-profiles')`)) await click('← Back to Profiles');
    await click(`Edit ${name}`);
    await sleep(3300);
    assert.deepEqual(await evidence(), [], 'Remounting the real frontend must not replay a previous save.');
    assert.deepEqual(await visibleToasts(), []);
    assert.equal((await saved()).revision, first.revision, 'Navigation and frontend remount must not save the profile.');

    await click('Edit profile name'); assert.equal(await inputName(`${name} updated`), true);
    await click('Save profile');
    const second = await until(async () => { const value = await saved(); return value.name === `${name} updated` && value.revision > first.revision ? value : null; }, 'The second actual edit was not saved');
    await until(async () => (await visibleToasts()).length === 1, 'A second actual save must produce its own toast');
    await sleep(7200);
    const secondEvents = await evidence(); assert.equal(secondEvents.length, 1, 'The second save must produce one fresh countdown without polling resets.');
    assert.deepEqual(await visibleToasts(), []);
    assert.equal((await saved()).revision, second.revision);
    assert.deepEqual((await profiles()).filter(profile => profile.id !== profileId), profilesBefore, 'Only the temporary profile may change.');
    assert.deepEqual(await companionIds(), companionsBefore, 'Notification acceptance must preserve paired companions.');
    await capture('save-toast-expired-without-replay.png');
    await writeFile(resolve(artifacts, 'evidence.json'), JSON.stringify({ actualNativeCockpit: true, rawCdp: true, passiveToastObservation: true, profileId, initialRevision: first.revision, secondRevision: second.revision, firstEvents, secondEvents, pollingWaitMs: 13200, navigationWaitMs: 3300, remountWaitMs: 3300, secondSaveWaitMs: 7200, nativeSavesOnlyForExplicitEdits: true, preExistingProfilesPreserved: true, pairedCompanionsPreserved: true }, null, 2) + '\n');
  } catch (error) { await capture('failure.png').catch(() => {}); throw error; }
  finally {
    try {
      if (profileId) { const current = await saved(); await request({ command: 'profile_delete', id: profileId, expectedRevision: current.revision }); }
      else if (draftOpen) await click('Cancel creation');
      if (profileId) { await native.send('Page.reload', { ignoreCache: true }); await until(() => native.evaluate(`typeof window.__TAURI_INTERNALS__?.invoke === 'function'`), 'Native frontend did not finish cleanup remount'); }
      if (profilesBefore) assert.deepEqual(await profiles(), profilesBefore);
      if (companionsBefore) assert.deepEqual(await companionIds(), companionsBefore);
    } finally {
      await native.evaluate('window.__saveToastObserver?.disconnect();delete window.__saveToastObserver;delete window.__saveToastEvidence').catch(() => {});
      native.close();
    }
  }
});
