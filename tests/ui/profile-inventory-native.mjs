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

test('native unpair removes paired and assigned companion resources from an open component palette', { timeout: 120000 }, async () => {
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
  const palette = () => native.evaluate(`(()=>{const palette=document.querySelector('[aria-label="Component palette"]');return palette?{rows:[...palette.querySelectorAll('button[data-resource-id]')].map(button=>({id:button.dataset.resourceId,name:button.querySelector('strong').textContent,group:button.closest('section').getAttribute('aria-label'),status:button.querySelector('small').textContent.trim(),disabled:button.disabled}))}:null;})()`);
  const capture = async name => { const result = await native.send('Page.captureScreenshot', { format: 'png' }); await writeFile(resolve(artifacts, name), Buffer.from(result.data, 'base64')); };
  const peerId = async () => peerPage?.evaluate(() => { const value = localStorage.getItem('osg-browser-credential-v2'); return value ? JSON.parse(value).companionId : null; });
  try {
    assert.equal(await native.evaluate('typeof window.__TAURI_INTERNALS__?.invoke'), 'function');
    assert.equal(await native.evaluate('document.visibilityState'), 'visible', 'The native component palette must remain visible for its real polling effect.');
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
    const component = await until(async () => (await resources()).find(item => item.resourceType === 'companion' && item.attributes.componentKind === 'paired-companion' && item.attributes.companionId === companionId), 'The fresh paired companion did not appear before panel assignment');
    assert.ok(!(await companions()).host.instances.some(instance => instance.companionId === companionId), 'Paired companion discovery must not create a panel assignment.');
    checks.push('A fresh production browser appears as a paired companion component before any panel is assigned');
    await request({ command: 'companion_assign', companion: { companionId, instanceId, publicationHash: publication.publicationHash } });
    const resource = await until(async () => {
      for (const item of (await resources()).filter(item => item.resourceType === 'companion' && item.id !== component.id && !initialResources.some(old => old.id === item.id))) {
        const details = (await request({ command: 'profile_resources_inspect', resourceId: item.id, offset: 0, limit: 1 })).profileResources;
        if (details.evidence.some(evidence => evidence.kind === 'companion-instance' && evidence.value === instanceId)) return item;
      }
    }, 'The native inventory did not discover the temporary assigned companion');
    await until(async () => (await resources()).find(item => item.id === resource.id)?.availability === 'online', 'The real browser did not establish its neutral native panel session');
    console.info('Temporary assigned companion discovered by native inventory.');
    checks.push('An independent instance of an existing publication is observed without replacing the paired component identity');

    await click('Profiles', 'nav[aria-label="Main navigation"]');
    await until(() => native.evaluate(`!![...document.querySelectorAll('main h1')].find(heading=>heading.textContent==='Profiles')`), 'The native Profile list did not open');
    await click('Add'); draftOpen = true;
    const current = await resources();
    const visiblePalette = await until(async () => { const value = await palette(); return value?.rows.some(row => row.id === component.id && !row.disabled) ? value : null; }, 'The native component palette did not receive the temporary paired companion');
    assert.equal(visiblePalette.rows.some(row => row.id === resource.id), false, 'Legacy per-panel instances must remain absent from the companion palette.');
    const originalIds = visiblePalette.rows.filter(row => row.group === 'Companions').map(row => row.id).sort();
    const reference = await native.send('Runtime.evaluate', { expression: `document.querySelector('[aria-label="Component palette"]')`, returnByValue: false });
    const paletteObjectId = reference.result.objectId;
    await capture('paired-inventory.png');

    await context.close(); context = null;
    const offline = await until(async () => (await resources()).find(item => item.id === resource.id)?.availability === 'offline', 'Disconnect should retain the paired companion as offline');
    assert.equal(offline, true);
    const disconnected = await companions();
    assert.ok(disconnected.host.clients.some(client => client.companionId === companionId));
    assert.ok(disconnected.host.instances.some(instance => instance.instanceId === instanceId));
    await until(async () => (await resources()).find(item => item.id === component.id)?.availability === 'offline', 'Disconnect should retain the paired component as offline');
    const offlineComponent = await until(async () => (await palette())?.rows.find(row => row.id === component.id && row.status === 'Offline'), 'The open palette did not reflect the paired companion going offline');
    assert.ok(offlineComponent && !offlineComponent.disabled, 'An offline paired companion must remain addable.');
    checks.push('Closing only the temporary browser retains its assignment and offline paired component, which remains addable');

    await request({ command: 'companion_revoke', companion: { companionId } });
    await until(async () => { const state = await companions(); return !state.host.clients.some(client => client.companionId === companionId) && !state.host.instances.some(instance => instance.instanceId === instanceId); }, 'The exact temporary companion did not unpair');
    await until(async () => !(await resources()).some(item => item.id === resource.id || item.id === component.id), 'The unpaired companion or its assigned instance remained in the native resource cache');
    const expectedIds = originalIds.filter(id => id !== component.id);
    const refreshed = await until(async () => { const value = await palette(); return value && JSON.stringify(value.rows.filter(row => row.group === 'Companions').map(row => row.id).sort()) === JSON.stringify(expectedIds) ? value : null; }, 'The existing open palette did not automatically remove the unpaired companion Add button');
    const connected = await native.send('Runtime.callFunctionOn', { objectId: paletteObjectId, functionDeclaration: `function(){return this.isConnected&&this===document.querySelector('[aria-label="Component palette"]');}`, returnByValue: true });
    assert.equal(connected.result.value, true, 'The palette must update in place without closing or navigating.');
    await capture('unpaired-inventory.png');
    console.info('Unpair removed both native resources and refreshed the same open palette.');
    checks.push('Unpair removes both exact native resources and its Add button from the same open component palette');
    const final = await companions(), profilesAfter = (await request({ command: 'profile_list' })).profiles.profiles;
    assert.deepEqual(final.host.clients.map(client => client.companionId).sort(), before.clients);
    assert.deepEqual(final.host.instances.map(instance => instance.instanceId).sort(), before.instances);
    assert.deepEqual(profilesAfter, before.profiles);
    assert.deepEqual(errors, []);
    await writeFile(resolve(artifacts, 'evidence.json'), JSON.stringify({ actualNativeCockpit: true, rawCdp: true, productionBrowser: true, companionId, instanceId, resourceId: resource.id, componentResourceId: component.id, pairedBeforeAssignment: true, legacyInstanceHiddenFromPalette: true, offlineComponentAddable: true, beforeResourceCount: initialResources.length, pairedResourceCount: current.length, finalPaletteResourceCount: refreshed.rows.length, paletteUpdatedInPlace: true, preservedExistingIdentities: before, checks, errors }, null, 2) + '\n');
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
