// Runs against the real dev Cockpit and an attached screen with the new cache firmware.
// Publishes temporary panels through Cockpit, then restores the original assignment.
import test from 'node:test';
import assert from 'node:assert/strict';
import { createRequire } from 'node:module';
import { randomUUID } from 'node:crypto';
import { mkdir, writeFile } from 'node:fs/promises';
import { resolve } from 'node:path';

const require = createRequire(import.meta.url);
const { ws: RawSocket } = createRequire(require.resolve(process.env.OSG_PLAYWRIGHT_MODULE ?? 'playwright'))('playwright-core/lib/utilsBundle');
const artifacts = resolve('.research/companion-image-cache-native');
const sleep = ms => new Promise(resolve => setTimeout(resolve, ms));

async function connectNative() {
  const targets = await (await fetch(`${process.env.OSG_NATIVE_CDP_URL ?? 'http://127.0.0.1:9222'}/json/list`, { signal: AbortSignal.timeout(5000) })).json();
  const endpoint = targets.find(target => target.type === 'page' && /127\.0\.0\.1:1420|localhost:1420/.test(target.url))?.webSocketDebuggerUrl;
  assert.ok(endpoint, 'Start the actual native dev Cockpit with CDP enabled.');
  const socket = new RawSocket(endpoint, { handshakeTimeout: 8000 });
  await new Promise((resolve, reject) => { socket.once('open', resolve); socket.once('error', reject); });
  let sequence = 0;
  const pending = new Map();
  socket.on('error', () => {});
  socket.on('message', bytes => {
    const message = JSON.parse(bytes.toString()), operation = pending.get(message.id);
    if (!operation) return;
    clearTimeout(operation.timer); pending.delete(message.id);
    if (message.error) operation.reject(new Error(message.error.message)); else operation.resolve(message.result);
  });
  socket.on('close', () => { for (const operation of pending.values()) { clearTimeout(operation.timer); operation.reject(new Error('Dev Cockpit disconnected.')); } pending.clear(); });
  const send = (method, params = {}) => new Promise((resolve, reject) => {
    const id = ++sequence, timer = setTimeout(() => { pending.delete(id); reject(new Error(`${method} timed out.`)); }, 30000);
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

test('physical companion shares assets, retains more than two panels and reports switch progress', { timeout: 300000 }, async () => {
  await mkdir(artifacts, { recursive: true });
  const native = await connectNative(), nonce = randomUUID().slice(0, 8);
  const evidence = { actualNativeCockpit: true, physicalCompanion: true, switches: [] };
  let original, companionId, fixtures;
  const request = async value => {
    const {command,...payload}=value;
    const response = await native.evaluate(`window.__TAURI_INTERNALS__.invoke('gateway_request',{request:${JSON.stringify({ command, companion:payload, clientId: 'companion-image-cache-native' })}})`);
    assert.equal(response.ok, true, response.error ?? value.command);
    return response.state;
  };
  const status = async () => (await request({ command: 'companion_status' })).companions;
  const instance = value => value.host.instances.find(row => row.companionId === companionId);
  const device = value => (value.usb.devices ?? [value.usb]).find(row => row.device?.companionId === companionId)?.device;
  async function waitReady(hash, capture) {
    const end = Date.now() + 90000;
    let last, previous;
    while (Date.now() < end) {
      last = await status();
      const peer = device(last), operation = peer?.operation, current = instance(last);
      if (operation?.publicationHash === hash) {
        const key = JSON.stringify(operation);
        if (capture && key !== previous) capture.push({ elapsedMs: Date.now() - (end - 90000), ...operation });
        previous = key;
        assert.notEqual(operation.phase, 'failed', operation.message ?? operation.errorCode);
        if (capture && operation.phase === 'downloading' && !evidence.progressHover) {
          const label = await native.evaluate(`Array.from(document.querySelectorAll('.tile-messages button')).map(button=>button.getAttribute('aria-label')).find(label=>label?.includes('KiB')&&label?.includes('cached'))`);
          if (label) {
            evidence.progressHover = label;
            const screenshot = await native.send('Page.captureScreenshot', { format: 'png' });
            await writeFile(resolve(artifacts, 'downloading-progress.png'), Buffer.from(screenshot.data, 'base64'));
          }
        }
      }
      if (current?.publicationHash === hash && current.armed && current.connected && peer?.publicationHash === hash && operation?.publicationHash === hash && operation.phase === 'ready') {
        assert.ok(Object.values(current.values ?? {}).every(value => value === false), 'Every panel switch must establish a neutral baseline.');
        return { instance: current, device: peer };
      }
      await sleep(120);
    }
    throw new Error(`The screen did not finish loading ${hash}: ${JSON.stringify(device(last))}`);
  }
  async function assign(panel, expectation) {
    const progress = [], started = Date.now();
    await request({ command: 'companion_assign', companionId, instanceId: original.instanceId, publicationHash: panel.publicationHash });
    const ready = await waitReady(panel.publicationHash, progress);
    evidence.switches.push({ name: panel.name, elapsedMs: Date.now() - started, operation: ready.device.operation, progress });
    const operation = ready.device.operation;
    assert.ok(operation.totalBytes > 0);
    assert.equal(operation.completedBytes, operation.totalBytes);
    if (expectation === 'hit') {
      assert.ok(operation.cacheHits > 0, 'Unchanged image assets must come from flash.');
      assert.equal(operation.cacheMisses, 0, 'A cached panel must not download images again.');
      assert.ok(progress.every(row => row.phase !== 'downloading'), 'A complete cache hit must skip downloading.');
    }
    if (operation.cacheMisses > 0) {
      assert.ok(progress.some(row => row.phase === 'downloading' && row.completedBytes > 0), 'The cockpit must receive intermediate download progress.');
    }
    return ready;
  }
  try {
    const before = await status();
    const peer = (before.usb.devices ?? [before.usb]).find(row => row.connected && (process.env.OSG_CACHE_COMPANION_ID ? row.device?.companionId === process.env.OSG_CACHE_COMPANION_ID : row.device?.boardProfile === 'sunton-3248s035r'));
    assert.ok(peer, 'Connect a paired Sunton, or set OSG_CACHE_COMPANION_ID to another connected screen.');
    assert.equal(peer.device.firmwareVersion, before.firmwareVersion, 'Install the newly built firmware through Cockpit first.');
    companionId = peer.device.companionId; original = instance(before);
    assert.ok(original, 'The physical companion must have an assigned panel.');
    assert.ok(peer.device.operation, 'This test requires cache-aware companion firmware.');
    await waitReady(original.publicationHash);
    evidence.companionId = companionId; evidence.boardProfile = peer.device.boardProfile; evidence.firmwareVersion = peer.device.firmwareVersion;
    evidence.original = { instanceId: original.instanceId, publicationHash: original.publicationHash };
    await native.evaluate(`(()=>{const button=Array.from(document.querySelectorAll('nav[aria-label="Main navigation"] button')).find(button=>button.textContent.trim()==='Companions');if(!button)throw new Error('Companions navigation is unavailable.');button.click();})()`);
    await sleep(300);
    await native.evaluate(`Array.from(document.querySelectorAll('button')).find(button=>button.textContent.trim()==='Back to Companions')?.click()`);
    fixtures = await native.evaluate(`(async()=>{
      const {CompanionLibraryStore}=await import('/src/lib/companion-library.ts');
      const {publishDocument}=await import('/packages/companion-designer/src/index.ts');
      const {sendPublication}=await import('/src/lib/companions.ts');
      const library=await new CompanionLibraryStore().load();
      const base=Object.values(library.publications).flat().find(p=>p.document.presentation.widgets.some(w=>w.kind==='image')&&p.document.ui.assets.length===1);
      if(!base)throw new Error('Save an image panel in the real Cockpit library before running this acceptance.');
      const result=[];
      for(const [name,factor] of [['A',1],['Shared',1],['C',0.85],['D',0.7]]){
        const document=structuredClone(base.document);document.ui.id='cache-${nonce}-'+name;document.ui.title='Cache acceptance ${nonce} '+name;
        for(const widget of document.presentation.widgets)if(widget.kind==='image')widget.bounds.width*=factor;
        const publication=await publishDocument(document,base.assets,{optimizeAssets:false});
        await sendPublication(publication,async(command,payload)=>{const reply=await window.__TAURI_INTERNALS__.invoke('gateway_request',{request:{command,companion:payload,clientId:'companion-image-cache-native'}});if(!reply.ok)throw new Error(reply.error);return reply.state.companions;});
        result.push({name,uiId:document.ui.id,publicationHash:publication.publicationHash});
      }
      return result;
    })()`);
    assert.equal(fixtures.length, 4);
    await assign(fixtures[0]);
    await assign(fixtures[1], 'hit');
    await assign(fixtures[2]);
    await assign(fixtures[3]);
    await assign(fixtures[0], 'hit');
    // Wait for a new actual session, so retained ready feedback cannot satisfy the check.
    const previousSession = device(await status())?.sessionId;
    await request({ command: 'companion_usb_disarm', companionId });
    const stopDeadline = Date.now() + 10000;
    let stopped = false;
    while (Date.now() < stopDeadline) {
      const current = await status();
      if (device(current)?.status === 'offline' && !instance(current)?.armed) { stopped = true; break; }
      await sleep(120);
    }
    assert.ok(stopped, 'Explicit disarm must stop the physical panel session.');
    const stoppedAt = Date.now();
    while (Date.now() - stoppedAt < 4200) {
      const current = await status();
      assert.equal(device(current)?.status, 'offline', 'An explicit stop must not trigger an automatic reconnect.');
      assert.equal(instance(current)?.armed, false);
      await sleep(150);
    }
    evidence.explicitStop = { stayedStoppedMs: Date.now() - stoppedAt };
    const reconnectedAt = Date.now();
    await request({ command: 'companion_usb_reconnect', companionId });
    const reconnectDeadline = Date.now() + 30000;
    let reconnected;
    while (Date.now() < reconnectDeadline) {
      const current = await status(), candidate = device(current);
      if (candidate?.sessionId && candidate.sessionId !== previousSession && candidate.operation?.phase === 'ready' && instance(current)?.armed) { reconnected = candidate; break; }
      await sleep(120);
    }
    assert.ok(reconnected, 'Reconnect must establish a fresh session with the same cached pixels.');
    assert.equal(reconnected.operation.cacheMisses, 0);
    assert.ok(reconnected.operation.cacheHits > 0);
    evidence.reconnect = { elapsedMs: Date.now() - reconnectedAt, operation: reconnected.operation };
    const screenshot = await native.send('Page.captureScreenshot', { format: 'png' });
    await writeFile(resolve(artifacts, 'cache-reused.png'), Buffer.from(screenshot.data, 'base64'));
  } catch (error) { evidence.error = error.message; throw error; }
  finally {
    try {
      if (original) {
        await request({ command: 'companion_assign', companionId, instanceId: original.instanceId, publicationHash: original.publicationHash });
        const restored = await waitReady(original.publicationHash);
        evidence.restored = restored.device.operation;
      }
      if (fixtures) await request({ command: 'companion_prune', hashes: [], panelIds: fixtures.map(panel => panel.uiId) });
    } finally {
      await writeFile(resolve(artifacts, 'evidence.json'), JSON.stringify(evidence, null, 2) + '\n');
      native.close();
    }
  }
});
