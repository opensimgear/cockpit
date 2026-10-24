// Actual Cockpit HTTPS/browser app and WSS sessions. No app state, responses or
// credentials are injected. Browser trust is scoped to a CA-verified server key.
import test from 'node:test';
import assert from 'node:assert/strict';
import { createRequire } from 'node:module';
import { randomUUID } from 'node:crypto';
import {verifiedServerKey} from './native/browser-trust.mjs';
import { createServer } from 'node:net';
import { mkdir, readFile, writeFile } from 'node:fs/promises';
import { resolve, join } from 'node:path';
import { importBundle, publishDocument } from '../../packages/companion-designer/src/index.ts';
import { fallbackValue } from '../../packages/companion-ui/src/controller.ts';
import { sendPublication } from '../../src/lib/companions.ts';

const pause = ms => new Promise(resolve => setTimeout(resolve, ms));
async function native() {
  const origin = process.env.OSG_WIDGETS_ORIGIN ?? 'http://127.0.0.1:1422';
  const targets = await (await fetch((process.env.OSG_WIDGETS_CDP ?? 'http://127.0.0.1:9232') + '/json/list', { signal: AbortSignal.timeout(5000) })).json();
  const target = targets.find(target => target.type === 'page' && new URL(target.url).origin === origin);
  assert.ok(target, 'Start the actual widget dev Cockpit.');
  const socket = new WebSocket(target.webSocketDebuggerUrl), pending = new Map(); let sequence = 0;
  await new Promise((resolve, reject) => { socket.addEventListener('open', resolve, { once: true }); socket.addEventListener('error', reject, { once: true }); });
  socket.addEventListener('message', event => {
    const reply = JSON.parse(event.data), operation = pending.get(reply.id); if (!operation) return;
    pending.delete(reply.id); clearTimeout(operation.timer); reply.error ? operation.reject(Error(reply.error.message)) : operation.resolve(reply.result);
  });
  return {
    close: () => socket.close(),
    request: async (command, companion = {}) => {
      const expression = `window.__TAURI_INTERNALS__.invoke('gateway_request',{request:${JSON.stringify({ command, companion, clientId: 'widget-browser-native' })}})`;
      const result = await new Promise((resolve, reject) => {
        const id = ++sequence, timer = setTimeout(() => { pending.delete(id); reject(Error(command + ' timed out')); }, 15000);
        pending.set(id, { resolve, reject, timer }); socket.send(JSON.stringify({ id, method: 'Runtime.evaluate', params: { expression, awaitPromise: true, returnByValue: true } }));
      });
      if (result.exceptionDetails) throw Error(result.exceptionDetails.exception?.description ?? result.exceptionDetails.text);
      const reply = result.result.value; assert.equal(reply.ok, true, reply.error ?? command); return reply.state.companions;
    },
  };
}
async function until(work, message) {
  for (let attempt = 0; attempt < 150; attempt++) { const result = await work(); if (result) return result; await pause(50); }
  throw Error(message);
}
async function freePort() {
  const server = createServer(); await new Promise(resolve => server.listen(0, '127.0.0.1', resolve));
  const port = server.address().port; await new Promise(resolve => server.close(resolve)); return port;
}
test('the live browser companion shares an edited widget publication and isolates source loss', {
  skip: process.env.OSG_WIDGETS_BROWSER_NATIVE !== '1', timeout: 120000,
}, async () => {
  const require = createRequire(import.meta.url), { chromium } = require(process.env.OSG_PLAYWRIGHT_MODULE ?? 'playwright');
  const api = await native(), browsers = [], peers = [], wire = [[], []], errors = [];
  const directory = resolve('.research/widget-browser-native', String(Date.now())); await mkdir(directory, { recursive: true });
  const bundleFile = process.env.OSG_WIDGETS_BUNDLE ?? 'tests/fixtures/legacy-companion/shared-systems-complete.bundle.json';
  const imported = await importBundle(await readFile(bundleFile, 'utf8')), panel = await publishDocument(imported.document, imported.assets);
  const evidence = { actualNativeCockpit: true, actualBrowserApp: true, nativeMobileQualified: false, bundleFile: resolve(bundleFile), publicationHash: panel.publicationHash, capabilityFingerprint: panel.capabilityFingerprint, checks: [] };
  let createdListener = false, published = false, complete = false;
  const instance = async id => (await api.request('companion_status')).host.instances.find(row => row.instanceId === id);
  try {
    const state = await api.request('companion_status');
    if (!state.listener?.enabled) { await api.request('companion_enable', { address: '127.0.0.1', advertisedHost: '127.0.0.1', port: await freePort(), discovery: false }); createdListener = true; }
    const certificate = (await api.request('companion_browser_certificate')).certificatePem;
    const spki = await verifiedServerKey((await api.request('companion_status')).browserUrl, certificate);
    const existed = state.host.publications.some(publication => publication.publicationHash === panel.publicationHash);
    await sendPublication(panel, (command, payload) => api.request(command, payload), { saveAsLatest: false }); published = !existed;
    const widgets = panel.document.presentation.widgets.filter(widget => widget.pageId === panel.document.ui.pages[0].id && widget.kind === 'component');
    // Use the topmost reusable momentary control, including edited instances.
    const controlWidget = widgets.findLast(widget => panel.document.presentation.definitions.find(definition => definition.id === widget.config.definitionId && definition.version === widget.config.definitionVersion).interaction.kind === 'momentary');
    assert.ok(controlWidget); const controlId = controlWidget.controlIds[0];
    const baseline = Object.fromEntries(panel.document.ui.controls.map(control => [control.id, fallbackValue(control)]));
    for (const index of [0, 1]) {
      const browser = await chromium.launch({ headless: true, ...(process.env.OSG_BROWSER_CHANNEL ? { channel: process.env.OSG_BROWSER_CHANNEL } : {}), args: ['--ignore-certificate-errors-spki-list=' + spki] }); browsers.push(browser);
      const context = await browser.newContext({ viewport: { width: 1280, height: 900 } }), page = await context.newPage();
      page.on('pageerror', error => errors.push(error.message));
      page.on('websocket', socket => socket.on('framesent', event => {
        try { const frame = JSON.parse(String(event.payload)); wire[index].push({ type: frame.type, ...(frame.sessionId ? { sessionId: frame.sessionId } : {}), ...(frame.type === 'baseline' ? { values: frame.values } : {}), ...(frame.type === 'input' ? { changes: frame.changes } : {}) }); } catch {}
      }));
      const before = new Set((await api.request('companion_status')).host.clients.map(client => client.companionId));
      const offer = await api.request('companion_pair_offer', { kind: 'browser' });
      await page.goto(offer.pairing.browserUrl, { waitUntil: 'domcontentloaded' });
      const client = await until(async () => (await api.request('companion_status')).host.clients.find(client => !before.has(client.companionId)), 'The actual browser pairing did not reach Cockpit');
      const instanceId = 'browser-widget-' + randomUUID(); peers.push({ companionId: client.companionId, instanceId, page, context });
      await api.request('companion_assign', { companionId: client.companionId, instanceId, publicationHash: panel.publicationHash });
      await page.getByText('Connected · panel active', { exact: true }).waitFor();
      assert.equal(await page.locator('.component-widget').count(), widgets.length);
      assert.equal(await page.locator('.component-widget[data-definition-id="osg-text-instrument"]').count(), 1);
      assert.equal(await page.locator('.component-widget[data-definition-id="osg-power-icon"]').count(), 1);
      const current = await instance(instanceId); assert.equal(current.armed, true); assert.equal(current.publicationHash, panel.publicationHash); assert.deepEqual(current.values, baseline);
      assert.deepEqual(wire[index].findLast(frame => frame.type === 'baseline')?.values, baseline);
      await page.screenshot({ path: join(directory, `browser-${index}.png`), fullPage: true });
    }
    evidence.checks.push('Two independently paired browser processes display the exact same complete source publication with neutral typed baselines.');
    const hit = '[data-widget-id="' + controlWidget.id + '"] .component-hit';
    for (const peer of peers) {
      const target = peer.page.locator(hit); assert.equal(await target.count(), 1);
      const bounds = await target.boundingBox(); assert.ok(bounds);
      await peer.page.mouse.move(bounds.x + bounds.width / 2, bounds.y + bounds.height / 2); await peer.page.mouse.down();
      await until(async () => (await instance(peer.instanceId)).values[controlId] === true, 'Real browser pointer press did not reach its own input instance');
      assert.equal(await peer.page.locator('[data-widget-id="' + controlWidget.id + '"] [data-widget-state]').getAttribute('data-widget-state'), 'pressed');
    }
    await peers[0].context.close();
    const lost = await until(async () => { const current = await instance(peers[0].instanceId); return !current.armed && (current.values === null || current.values[controlId] === false) ? current : false; }, 'Closing one browser did not release its source');
    evidence.sourceLoss = { armed: lost.armed, values: lost.values };
    const retained = await instance(peers[1].instanceId); assert.equal(retained.armed, true); assert.equal(retained.values[controlId], true);
    await peers[1].page.mouse.up();
    await until(async () => (await instance(peers[1].instanceId)).values[controlId] === false, 'The independent browser did not release normally');
    assert.equal(await peers[1].page.locator('[data-widget-id="' + controlWidget.id + '"] [data-widget-state]').getAttribute('data-widget-state'), 'unknown');
    evidence.checks.push('Real held pointer input belongs to each companion; closing one releases only its source while the other stays armed and held. Release returns to unknown feedback.');
    const baselineCount = wire[1].filter(frame => frame.type === 'baseline').length;
    await peers[1].page.reload({ waitUntil: 'domcontentloaded' });
    await peers[1].page.getByText('Connected · panel active', { exact: true }).waitFor();
    assert.ok(wire[1].filter(frame => frame.type === 'baseline').length > baselineCount);
    assert.deepEqual((await instance(peers[1].instanceId)).values, baseline);
    evidence.checks.push('Reload retains normal browser pairing and requires a new neutral session baseline without replaying prior input.');
    assert.deepEqual(errors, []); evidence.wire = wire; evidence.companions = peers.map(({ companionId, instanceId }) => ({ companionId, instanceId }));
    await writeFile(join(directory, 'evidence.json'), JSON.stringify(evidence, null, 2)); console.log(JSON.stringify({ directory, publicationHash: panel.publicationHash, checks: evidence.checks })); complete = true;
  } catch (error) {
    evidence.failure = String(error); evidence.errors = errors; evidence.wire = wire;
    for (const [index, peer] of peers.entries()) if (!peer.page.isClosed()) { await peer.page.screenshot({ path: join(directory, `failure-${index}.png`), fullPage: true }).catch(() => {}); await writeFile(join(directory, `page-${index}.txt`), await peer.page.locator('body').innerText().catch(() => 'Unavailable')); }
    await writeFile(join(directory, 'evidence.json'), JSON.stringify(evidence, null, 2)); throw error;
  } finally {
    for (const browser of browsers) await browser.close().catch(() => {});
    for (const peer of peers) await api.request('companion_revoke', { companionId: peer.companionId }).catch(() => {});
    let cleanup;
    if (published) cleanup = await api.request('companion_status').then(state => api.request('companion_prune', { panelIds: [panel.document.ui.id], hashes: state.host.publications.filter(publication => publication.publicationHash !== panel.publicationHash).map(publication => publication.publicationHash) })).catch(() => undefined);
    if (createdListener) await api.request('companion_disable').catch(() => {});
    api.close();
    if (complete && published) assert.ok(cleanup && !cleanup.host.publications.some(publication => publication.publicationHash === panel.publicationHash), 'Temporary browser publication must leave the bounded native cache after revocation');
  }
});
