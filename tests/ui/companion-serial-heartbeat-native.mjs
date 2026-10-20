// Exercises the real native USB worker's unsolicited status probes during live heartbeats.
// Requires a paired Sunton with an assigned panel; changes no assignments or settings.
import test from 'node:test';
import assert from 'node:assert/strict';
import { createRequire } from 'node:module';
import { mkdir, writeFile } from 'node:fs/promises';
import { resolve } from 'node:path';

const require = createRequire(import.meta.url);
const { ws: RawSocket } = createRequire(require.resolve(process.env.OSG_PLAYWRIGHT_MODULE ?? 'playwright'))('playwright-core/lib/utilsBundle');
const artifacts = resolve('.research/companion-serial-heartbeat-native');
const sleep = ms => new Promise(resolve => setTimeout(resolve, ms));

async function connectNative() {
  const targets = await (await fetch(`${process.env.OSG_NATIVE_CDP_URL ?? 'http://127.0.0.1:9222'}/json/list`, { signal: AbortSignal.timeout(5000) })).json();
  const endpoint = process.env.OSG_NATIVE_PAGE_WS ?? targets.find(target => target.type === 'page' && /127\.0\.0\.1:1420|localhost:1420|tauri\.localhost/.test(target.url) && !target.url.includes('profile-progress'))?.webSocketDebuggerUrl;
  assert.ok(endpoint, 'Start the actual native dev Cockpit with CDP enabled.');
  const socket = new RawSocket(endpoint, { handshakeTimeout: 8000 });
  await new Promise((resolve, reject) => { socket.once('open', resolve); socket.once('error', reject); });
  let sequence = 0; const pending = new Map();
  socket.on('error', () => {});
  socket.on('message', bytes => {
    const message = JSON.parse(bytes.toString()), operation = pending.get(message.id);
    if (!operation) return;
    clearTimeout(operation.timer); pending.delete(message.id);
    if (message.error) operation.reject(new Error(message.error.message)); else operation.resolve(message.result);
  });
  socket.on('close', () => { for (const operation of pending.values()) { clearTimeout(operation.timer); operation.reject(new Error('Dev Cockpit disconnected.')); } pending.clear(); });
  return {
    async evaluate(expression) {
      const result = await new Promise((resolve, reject) => {
        const id = ++sequence, timer = setTimeout(() => { pending.delete(id); reject(new Error('Native status request timed out.')); }, 10000);
        pending.set(id, { resolve, reject, timer });
        socket.send(JSON.stringify({ id, method: 'Runtime.evaluate', params: { expression, returnByValue: true, awaitPromise: true } }));
      });
      if (result.exceptionDetails) throw new Error(result.exceptionDetails.exception?.description ?? result.exceptionDetails.text);
      return result.result.value;
    },
    close() { socket.close(); },
  };
}

test('physical Sunton retains one healthy USB session through ninety seconds of status probes and heartbeats', { timeout: 170000 }, async () => {
  await mkdir(artifacts, { recursive: true });
  const native = await connectNative();
  const evidence = { actualNativeCockpit: true, physicalCompanion: true, sampleIntervalMs: 150, samples: [], sampleCount: 0, maxStatusAgeMs: 0 };
  let companionId;
  const request = async (command, companion = {}) => {
    const response = await native.evaluate(`window.__TAURI_INTERNALS__.invoke('gateway_request',{request:${JSON.stringify({ command, companion, clientId: 'companion-serial-heartbeat-native' })}})`);
    assert.equal(response.ok, true, response.error ?? command);
    return response.state.companions;
  };
  const link = state => (state.usb.devices ?? [state.usb]).find(row => row.device?.companionId === companionId);
  const instance = state => state.host.instances.find(row => row.companionId === companionId);
  const assignments = state => state.host.instances.map(({ instanceId, companionId, publicationHash }) => ({ instanceId, companionId, publicationHash })).sort((a, b) => a.instanceId.localeCompare(b.instanceId));
  try {
    const before = await request('companion_status');
    const selected = (before.usb.devices ?? [before.usb]).find(row => row.connected && (process.env.OSG_HEARTBEAT_COMPANION_ID ? row.device?.companionId === process.env.OSG_HEARTBEAT_COMPANION_ID : row.device?.boardProfile === 'sunton-3248s035r'));
    assert.ok(selected, 'Connect the paired Sunton, or set OSG_HEARTBEAT_COMPANION_ID to the exact USB companion.');
    companionId = selected.device.companionId;
    assert.ok(before.host.clients.some(row => row.companionId === companionId), 'The selected companion must be paired.');
    const original = instance(before), previousSession = selected.device.sessionId;
    assert.ok(original, 'Assign a panel to the physical companion before acceptance.');
    assert.equal(original.profileOwned, false, 'Deactivate the profile before testing its companion connection.');
    evidence.companionId = companionId; evidence.boardProfile = selected.device.boardProfile; evidence.firmwareVersion = selected.device.firmwareVersion;
    evidence.assignment = { instanceId: original.instanceId, publicationHash: original.publicationHash };
    const started = Date.now();
    await request('companion_usb_reconnect', { companionId });
    let ready;
    while (Date.now() - started < 60000) {
      const state = await request('companion_status'), peer = link(state)?.device, current = instance(state);
      if (peer?.sessionId && peer.sessionId !== previousSession) {
        assert.notEqual(peer.operation?.phase, 'failed', peer.statusMessage ?? peer.operation?.errorCode);
        if (peer.status === 'live-armed' && peer.operation?.phase === 'ready' && peer.publicationHash === original.publicationHash && current?.connected && current.armed && current.sessionId === peer.sessionId && current.generation === peer.generation && current.transport === 'usb') { ready = { peer, current }; break; }
      }
      await sleep(150);
    }
    assert.ok(ready, 'One explicit reconnect must establish a fresh actual USB session and neutral baseline.');
    evidence.reconnectMs = Date.now() - started;
    evidence.sessionId = ready.peer.sessionId; evidence.generation = ready.peer.generation; evidence.initialOperation = ready.peer.operation;
    const soakStarted = Date.now(); let lastRecorded = -1000, latest;
    do {
      latest = await request('companion_status');
      const usb = link(latest), peer = usb?.device, current = instance(latest), elapsedMs = Date.now() - soakStarted;
      const sample = { elapsedMs, connected: usb?.connected, status: peer?.status, statusAgeMs: usb?.statusAgeMs, sessionId: peer?.sessionId, phase: peer?.operation?.phase, errorCode: peer?.operation?.errorCode };
      evidence.sampleCount++;
      if (elapsedMs - lastRecorded >= 1000 || !usb?.connected || peer?.status !== 'live-armed' || peer?.operation?.phase === 'failed') { evidence.samples.push(sample); lastRecorded = elapsedMs; }
      assert.equal(usb?.connected, true, 'The actual USB link must remain connected.');
      assert.ok(Number.isFinite(usb.statusAgeMs) && usb.statusAgeMs >= 0 && usb.statusAgeMs <= 5000, 'Nonce-bound full USB status must remain fresh within five seconds.');
      evidence.maxStatusAgeMs = Math.max(evidence.maxStatusAgeMs, usb.statusAgeMs);
      assert.notEqual(peer.operation?.phase, 'failed', peer.statusMessage ?? peer.operation?.errorCode);
      assert.equal(peer.status, 'live-armed', peer.statusMessage ?? 'The physical screen lost its live session.');
      assert.equal(peer.sessionId, ready.peer.sessionId, 'A reconnect must not conceal a failed heartbeat.');
      assert.equal(peer.generation, ready.peer.generation);
      assert.equal(current?.sessionId, ready.peer.sessionId); assert.equal(current?.armed, true); assert.equal(current?.connected, true);
      assert.equal(current?.publicationHash, original.publicationHash); assert.equal(current?.transport, 'usb');
      if (elapsedMs >= 90000) { evidence.observedMs = elapsedMs; break; }
      await sleep(150);
    } while (true);
    assert.deepEqual(assignments(latest), assignments(before), 'No companion assignment, including other screens, may change.');
    evidence.finalOperation = link(latest).device.operation; evidence.passed = true;
  } catch (error) { evidence.error = error.message; throw error; }
  finally {
    await writeFile(resolve(artifacts, 'evidence.json'), JSON.stringify(evidence, null, 2) + '\n');
    native.close();
  }
});
