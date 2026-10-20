// Recover an existing fault in the actual running dev Cockpit. No mocked IPC or test fault.
// Run only after capturing the incident: this deliberately acknowledges native recovery.
import test from 'node:test';
import assert from 'node:assert/strict';
import { createRequire } from 'node:module';
import { mkdir, writeFile } from 'node:fs/promises';
import { resolve } from 'node:path';

const require = createRequire(import.meta.url), modulePath = process.env.OSG_PLAYWRIGHT_MODULE ?? 'playwright';
const { ws: RawSocket } = createRequire(require.resolve(modulePath))('playwright-core/lib/utilsBundle');
const artifacts = resolve('.research/profile-recovery-message-native');
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

test('native activation fault appears once with an inline Recover action and clears after recovery', { timeout: 45000 }, async () => {
  await mkdir(artifacts, { recursive: true });
  const native = await connectNative();
  const request = async value => {
    const response = await native.evaluate(`window.__TAURI_INTERNALS__.invoke('gateway_request',{request:${JSON.stringify({ ...value, clientId: 'profile-recovery-message-native' })}})`);
    assert.equal(response.ok, true, response.error ?? `Native ${value.command} failed`); return response.state;
  };
  const status = async () => (await request({ command: 'profile_status' })).profiles.profileSession;
  const profiles = async () => (await request({ command: 'profile_list' })).profiles.profiles;
  const companionIds = async () => (await request({ command: 'companion_status' })).companions.host.clients.map(client => client.companionId).sort();
  const capture = async name => { const value = await native.send('Page.captureScreenshot', { format: 'png' }); await writeFile(resolve(artifacts, name), Buffer.from(value.data, 'base64')); };
  try {
    assert.equal(await native.evaluate('typeof window.__TAURI_INTERNALS__?.invoke'), 'function');
    const before = await status(), profilesBefore = await profiles(), companionsBefore = await companionIds();
    assert.ok(['faulted', 'recovery-needed'].includes(before.state) && before.fault, 'Run against an existing captured native activation fault; this test never fabricates one.');
    const fault = before.fault;
    const banner = await until(() => native.evaluate(`(() => {
      const root = document.querySelector('[aria-label="Runtime profile status"]');
      if (!root) return null;
      const alerts = [...root.querySelectorAll('[role="alert"]')];
      const message = alerts.find(node => node.textContent.includes(${JSON.stringify(fault)}));
      const button = [...root.querySelectorAll('button')].find(node => node.textContent.trim() === 'Recover');
      if (!message || !button || button.disabled) return null;
      const text = [...message.childNodes].find(node => node.nodeType === Node.ELEMENT_NODE && node.textContent.trim() === ${JSON.stringify(fault)});
      const range = document.createRange();
      if (text) range.selectNodeContents(text); else range.selectNodeContents(message);
      const m = range.getBoundingClientRect(), b = button.getBoundingClientRect();
      return { alerts: alerts.map(node => node.textContent.trim()), occurrences: root.textContent.split(${JSON.stringify(fault)}).length - 1,
        sameBanner: message.contains(button), buttonLabel: button.textContent.trim(), oldButton: root.textContent.includes('Recover outputs'),
        message: { x: m.x, y: m.y, width: m.width, height: m.height }, button: { x: b.x, y: b.y, width: b.width, height: b.height },
        centerDifference: Math.abs(m.y + m.height / 2 - b.y - b.height / 2) };
    })()`), 'The native fault did not render with its new recovery banner');
    assert.equal(banner.occurrences, 1, 'The activation fault must appear exactly once.');
    assert.equal(banner.alerts.length, 1, 'There must be one runtime error banner.');
    assert.equal(banner.sameBanner, true, 'Recover belongs inside the fault banner.');
    assert.equal(banner.oldButton, false);
    assert.ok(banner.button.x >= banner.message.x + banner.message.width - 1, 'Recover must sit to the right of the message.');
    assert.ok(banner.centerDifference < Math.max(8, banner.message.height / 2), 'Recover must align with the message on the same row.');
    await capture('fault-once-inline-recover.png');
    await native.evaluate(`(() => {
      const root = document.querySelector('[aria-label="Runtime profile status"]');
      window.__recoveryMessageEvidence = [];
      window.__recoveryMessageObserver = new MutationObserver(() => {
        const button = [...root.querySelectorAll('button')].find(node => node.textContent.trim() === 'Recover');
        window.__recoveryMessageEvidence.push({ disabled: !!button?.disabled, recovering: root.textContent.includes('Recovering outputs…'), alerts: root.querySelectorAll('[role="alert"]').length });
      });
      window.__recoveryMessageObserver.observe(root, { subtree: true, childList: true, characterData: true, attributes: true, attributeFilter: ['disabled'] });
      [...root.querySelectorAll('button')].find(node => node.textContent.trim() === 'Recover').click();
    })()`);
    const after = await until(async () => { const value = await status(); return value.state === 'stopped' && !value.fault ? value : null; }, 'The actual native Recover action did not clear the fault');
    await until(() => native.evaluate(`(() => { const root = document.querySelector('[aria-label="Runtime profile status"]'); return !root?.querySelector('[role="alert"]') && ![...(root?.querySelectorAll('button') ?? [])].some(button => button.textContent.trim() === 'Recover'); })()`), 'The recovered runtime retained its fault or Recover action');
    const pending = await native.evaluate('window.__recoveryMessageEvidence');
    assert.ok(pending.some(value => value.recovering && value.disabled), 'The real recovery must show pending feedback and disable its duplicate action.');
    assert.deepEqual(await profiles(), profilesBefore, 'Recovery must preserve saved profiles.');
    assert.deepEqual(await companionIds(), companionsBefore, 'Recovery must preserve paired companions.');
    await capture('recovery-complete.png');
    await writeFile(resolve(artifacts, 'evidence.json'), JSON.stringify({ actualNativeCockpit: true, rawCdp: true, existingIncident: true, nativeBefore: { state: before.state, fault }, nativeAfter: { state: after.state, fault: after.fault }, banner, pending, savedProfilesPreserved: true, pairedCompanionsPreserved: true }, null, 2) + '\n');
  } catch (error) { await capture('failure.png').catch(() => {}); throw error; }
  finally { await native.evaluate('window.__recoveryMessageObserver?.disconnect();delete window.__recoveryMessageObserver;delete window.__recoveryMessageEvidence').catch(() => {}); native.close(); }
});
