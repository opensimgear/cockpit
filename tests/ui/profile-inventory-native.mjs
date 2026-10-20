// Real native Cockpit IPC and a disposable production browser companion.
// No mocked providers, fabricated input frames, direct configuration-file edits or native restart.
import test from 'node:test';
import assert from 'node:assert/strict';
import { createRequire } from 'node:module';
import { randomUUID } from 'node:crypto';
import { mkdir, writeFile } from 'node:fs/promises';
import { resolve } from 'node:path';

const require = createRequire(import.meta.url), modulePath = process.env.OSG_PLAYWRIGHT_MODULE ?? 'playwright';
const { chromium } = require(modulePath);
const { ws: RawSocket } = createRequire(require.resolve(modulePath))('playwright-core/lib/utilsBundle');
const artifacts = resolve('.research/profile-inventory-native');
const sleep = ms => new Promise(resolve => setTimeout(resolve, ms));
const redact = value => String(value).replace(/[A-Za-z0-9_-]{43}/g, '[redacted]');

async function connectNative() {
  let endpoint = process.env.OSG_NATIVE_PAGE_WS;
  if (!endpoint) {
    const response = await fetch(`${process.env.OSG_NATIVE_CDP_URL ?? 'http://127.0.0.1:9222'}/json/list`, { signal: AbortSignal.timeout(5000) });
    endpoint = (await response.json()).find(target => target.type === 'page' && /127\.0\.0\.1:1420|localhost:1420|tauri\.localhost/.test(target.url))?.webSocketDebuggerUrl;
  }
  assert.ok(endpoint, 'The already-running native dev Cockpit must expose its page debugging target.');
  const socket = new RawSocket(endpoint, { handshakeTimeout: 8000 });
  await new Promise((resolve, reject) => { socket.once('open', resolve); socket.once('error', () => reject(new Error('Native page debugging connection failed.'))); });
  let sequence = 0; const pending = new Map();
  socket.on('error', () => {});
  socket.on('message', bytes => {
    const message = JSON.parse(bytes.toString()), operation = pending.get(message.id);
    if (!operation) return;
    clearTimeout(operation.timer); pending.delete(message.id);
    if (message.error) operation.reject(new Error(redact(message.error.message))); else operation.resolve(message.result);
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
      if (result.exceptionDetails) throw new Error(redact(result.exceptionDetails.exception?.description ?? result.exceptionDetails.text));
      return result.result.value;
    },
    close() { socket.close(); },
  };
}
async function until(operation, description, timeout = 15000) {
  const end = Date.now() + timeout; let last;
  while (Date.now() < end) { try { const value = await operation(); if (value) return value; } catch (error) { last = error; } await sleep(150); }
  throw new Error(`${description}${last ? ': ' + redact(last.message) : ''}`);
}

test('native unpair removes cached companion resources and refreshes an open Profile inventory picker', { timeout: 120000 }, async () => {
  await mkdir(artifacts, { recursive: true });
  const native = await connectNative(), errors = [], instanceId = randomUUID(), checks = [];
  let browser, context, peerPage, companionId, draftOpen = false, before;
  const request = async value => {
    const result = await native.evaluate(`window.__TAURI_INTERNALS__.invoke('gateway_request',{request:${JSON.stringify({ ...value, clientId: 'profile-unpair-native' })}})`);
    assert.equal(result.ok, true, `${value.command}: ${redact(result.error ?? 'Native request failed')}`); return result.state;
  };
  const companions = async () => (await request({ command: 'companion_status' })).companions;
  const resources = async () => {
    const all = []; let first;
    do {
      const page = (await request({ command: 'profile_resources_list', offset: all.length, limit: 128 })).profileResources;
      if (!first) first = page; else assert.equal(page.revision, first.revision, 'Inventory changed while paging; retry the read.');
      all.push(...page.resources);
    } while (all.length < first.total);
    return all;
  };
  const click = (label, scope = 'main') => until(() => native.evaluate(`(()=>{const root=document.querySelector(${JSON.stringify(scope)});const button=[...(root?.querySelectorAll('button')??[])].find(button=>(button.getAttribute('aria-label')??button.textContent.replace(/\\s+/g,' ').trim())===${JSON.stringify(label)});if(!button||button.disabled)return false;button.scrollIntoView({block:'center'});button.click();return true;})()`), `Native UI action did not become available: ${label}`);
  const picker = () => native.evaluate(`(()=>{const picker=document.querySelector('[aria-label="Add resource blocks"]');return picker?{rows:[...picker.querySelectorAll('.inventory-choices label')].map(label=>({name:label.querySelector('strong').textContent,kind:label.querySelector('small').textContent,checked:label.querySelector('input').checked})),selection:picker.querySelector(':scope > button:last-child')?.textContent,disabled:picker.querySelector(':scope > button:last-child')?.disabled}:null;})()`);
  const capture = async name => { const result = await native.send('Page.captureScreenshot', { format: 'png' }); await writeFile(resolve(artifacts, name), Buffer.from(result.data, 'base64')); };
  const peerId = async () => peerPage?.evaluate(() => { const value = localStorage.getItem('osg-browser-credential-v2'); return value ? JSON.parse(value).companionId : null; });
  try {
    assert.equal(await native.evaluate('typeof window.__TAURI_INTERNALS__?.invoke'), 'function');
    assert.equal(await native.evaluate('document.visibilityState'), 'visible', 'The native Profile picker must remain visible for its real polling effect.');
    const profileState = (await request({ command: 'profile_status' })).profiles;
    assert.equal(profileState.profileSession.owner, null, 'Run this acceptance with no profile active.');
    const initial = await companions(), initialResources = await resources(), profilesBefore = (await request({ command: 'profile_list' })).profiles.profiles;
    console.info('Connected to actual native Cockpit IPC; baseline inventory read.');
    before = { clients: initial.host.clients.map(client => client.companionId).sort(), instances: initial.host.instances.map(instance => instance.instanceId).sort(), profiles: profilesBefore };
    assert.equal(initial.listener.enabled, true, 'The existing companion listener must already be enabled.');
    const assigned = new Set(initial.host.instances.map(instance => instance.publicationHash));
    const publication = initial.host.publications.find(publication => !assigned.has(publication.publicationHash)) ?? initial.host.publications[0];
    assert.ok(publication?.publicationHash, 'An existing published panel is required; this test never edits panel publications.');
    const offered = (await request({ command: 'companion_pair_offer', companion: { kind: 'browser' } })).companions;
    browser = await chromium.launch({ headless: true, ...(process.env.OSG_BROWSER_CHANNEL ? { channel: process.env.OSG_BROWSER_CHANNEL } : process.platform === 'win32' ? { channel: 'msedge' } : {}) });
    context = await browser.newContext({ ignoreHTTPSErrors: true, viewport: { width: 1280, height: 900 } });
    peerPage = await context.newPage(); peerPage.on('pageerror', error => errors.push(redact(error.message)));
    try { await peerPage.goto(offered.pairing.browserUrl, { timeout: 15000 }); } catch { throw new Error('The temporary production browser companion could not load its native pairing page.'); }
    companionId = await until(peerId, 'The actual browser did not complete pairing');
    console.info('Temporary production browser companion paired.');
    assert.ok(!before.clients.includes(companionId), 'The disposable context must create a new companion identity.');
    await until(async () => (await companions()).host.clients.some(client => client.companionId === companionId), 'The native host did not retain the temporary paired identity');
    await request({ command: 'companion_assign', companion: { companionId, instanceId, publicationHash: publication.publicationHash } });
    const resource = await until(async () => {
      for (const item of (await resources()).filter(item => item.resourceType === 'companion' && !initialResources.some(old => old.id === item.id))) {
        const details = (await request({ command: 'profile_resources_inspect', resourceId: item.id, offset: 0, limit: 1 })).profileResources;
        if (details.evidence.some(evidence => evidence.kind === 'companion-instance' && evidence.value === instanceId)) return item;
      }
    }, 'The native inventory did not discover the temporary assigned companion');
    await until(async () => (await resources()).find(item => item.id === resource.id)?.availability === 'online', 'The real browser did not establish its neutral native panel session');
    console.info('Temporary assigned companion discovered by native inventory.');
    checks.push('A fresh production browser pairs and an independent instance of an existing publication is observed by the actual native inventory');

    await click('Profiles', 'nav[aria-label="Main navigation"]');
    await until(() => native.evaluate(`!![...document.querySelectorAll('main h1')].find(heading=>heading.textContent==='Profiles')`), 'The native Profile list did not open');
    await click('Add profile'); draftOpen = true;
    await until(() => native.evaluate(`!![...document.querySelectorAll('main button')].find(button=>button.textContent.trim()==='Add from inventory')`), 'The unsaved native Profile editor did not open');
    await click('Add from inventory');
    const current = await resources(), index = current.findIndex(item => item.id === resource.id);
    const visiblePicker = await until(async () => { const value = await picker(); return value?.rows.length === current.length && value.rows[index]?.name === resource.name ? value : null; }, 'The open native inventory picker did not receive the temporary resource');
    const originalNames = visiblePicker.rows.filter(row => row.kind.startsWith('Companion ·')).map(row => row.name).sort();
    const reference = await native.send('Runtime.evaluate', { expression: `document.querySelector('[aria-label="Add resource blocks"]')`, returnByValue: false });
    const pickerObjectId = reference.result.objectId;
    await native.evaluate(`(()=>{const input=document.querySelectorAll('[aria-label="Add resource blocks"] .inventory-choices label')[${index}].querySelector('input');input.click();return input.checked;})()`);
    await until(async () => (await picker())?.selection?.trim() === 'Add 1 selected resources', 'The temporary inventory checkbox did not select');
    await capture('paired-inventory.png');

    await context.close(); context = null;
    const offline = await until(async () => (await resources()).find(item => item.id === resource.id)?.availability === 'offline', 'Disconnect should retain the paired companion as offline');
    assert.equal(offline, true);
    const disconnected = await companions();
    assert.ok(disconnected.host.clients.some(client => client.companionId === companionId));
    assert.ok(disconnected.host.instances.some(instance => instance.instanceId === instanceId));
    checks.push('Closing only the temporary browser retains its paired assignment and offline inventory resource');

    await request({ command: 'companion_revoke', companion: { companionId } });
    await until(async () => { const state = await companions(); return !state.host.clients.some(client => client.companionId === companionId) && !state.host.instances.some(instance => instance.instanceId === instanceId); }, 'The exact temporary companion did not unpair');
    await until(async () => !(await resources()).some(item => item.id === resource.id), 'The unpaired companion remained in the native resource cache');
    const expectedNames = [...originalNames]; expectedNames.splice(expectedNames.indexOf(resource.name), 1);
    const refreshed = await until(async () => { const value = await picker(); return value && JSON.stringify(value.rows.filter(row => row.kind.startsWith('Companion ·')).map(row => row.name).sort()) === JSON.stringify(expectedNames) && value.selection?.trim() === 'Add 0 selected resources' && value.disabled ? value : null; }, 'The existing open picker did not automatically remove the unpaired resource and stale selection');
    const connected = await native.send('Runtime.callFunctionOn', { objectId: pickerObjectId, functionDeclaration: `function(){return this.isConnected&&this===document.querySelector('[aria-label="Add resource blocks"]');}`, returnByValue: true });
    assert.equal(connected.result.value, true, 'The picker must update in place without closing or navigating.');
    await capture('unpaired-inventory.png');
    console.info('Unpair removed the native resource and refreshed the same open picker.');
    checks.push('Unpair removes the exact native resource; the same open Profile picker updates automatically and clears its stale selection');
    const final = await companions(), profilesAfter = (await request({ command: 'profile_list' })).profiles.profiles;
    assert.deepEqual(final.host.clients.map(client => client.companionId).sort(), before.clients);
    assert.deepEqual(final.host.instances.map(instance => instance.instanceId).sort(), before.instances);
    assert.deepEqual(profilesAfter, before.profiles);
    assert.deepEqual(errors, []);
    await writeFile(resolve(artifacts, 'evidence.json'), JSON.stringify({ actualNativeCockpit: true, rawCdp: true, productionBrowser: true, companionId, instanceId, resourceId: resource.id, beforeResourceCount: initialResources.length, pairedResourceCount: current.length, finalResourceCount: refreshed.rows.length, pickerUpdatedInPlace: true, staleSelectionCleared: true, preservedExistingIdentities: before, checks, errors }, null, 2) + '\n');
  } catch (error) { await capture('failure.png').catch(() => {}); throw error; }
  finally {
    try {
      if (!companionId && peerPage && !peerPage.isClosed()) companionId = await peerId().catch(() => null);
      if (context) await context.close();
      if (browser) await browser.close();
      if (companionId) {
        assert.ok(before && !before.clients.includes(companionId), 'Cleanup must never revoke a pre-existing identity.');
        if ((await companions()).host.clients.some(client => client.companionId === companionId)) await request({ command: 'companion_revoke', companion: { companionId } });
      }
      if (draftOpen) await click('Cancel creation');
    } finally { native.close(); }
  }
});
