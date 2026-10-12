// Run after building the native helper:
// node --test simulator/bridge.test.mjs
// These checks use an isolated helper and in-memory HTTP streams. They do not
// change the inventory or setup sessions in the running Cockpit dev server.
import test from 'node:test';
import assert from 'node:assert/strict';
import { EventEmitter } from 'node:events';
import { Readable } from 'node:stream';
import { access, mkdtemp, writeFile, readFile, rm } from 'node:fs/promises';
import { constants } from 'node:fs';
import { fileURLToPath } from 'node:url';
import { join } from 'node:path';
import { tmpdir } from 'node:os';
import { setTimeout as delay } from 'node:timers/promises';
import { gatewaySimulator } from './bridge.mjs';

const helperPath = fileURLToPath(new URL('./macos/.build/debug/osg-gateway-simulator', import.meta.url));
const clientA = 'test-client-a-0123456789';
const clientB = 'test-client-b-0123456789';
const shifterID = 'OSG-SQ-0042';

test('isolated bridge enforces ownership and reports the real native lifecycle', { skip: process.platform !== 'darwin', timeout: 45_000 }, async () => {
  await access(helperPath, constants.X_OK);
  const previousHelper = process.env.OSG_HID_HELPER;
  process.env.OSG_HID_HELPER = helperPath;
  const server = new EventEmitter();
  let middleware;
  const plugin = gatewaySimulator();
  plugin.configureServer({ httpServer: server, middlewares: { use(path, handler) {
    assert.equal(path, '/api/gateway');
    middleware = handler;
  } } });

  async function request(command, extra = {}, clientId = clientA, headers = {}) {
    const req = Readable.from([Buffer.from(JSON.stringify({ command, clientId, ...extra }))]);
    Object.assign(req, {
      method: 'POST', url: '/', socket: { remoteAddress: '127.0.0.1' },
      headers: { host: '127.0.0.1:5173', origin: 'http://127.0.0.1:5173',
        'content-type': 'application/json', 'x-osg-simulator': '1', ...headers },
    });
    let body;
    const res = { statusCode: 200, setHeader() {}, end(value) { body = value; } };
    await middleware(req, res);
    assert.ok(body, 'middleware must complete the HTTP response');
    return { status: res.statusCode, ...JSON.parse(body) };
  }

  try {
    const deniedOrigin = await request('status', {}, clientA, { origin: 'https://unrelated.example' });
    assert.equal(deniedOrigin.status, 403);
    assert.equal(deniedOrigin.ok, false);
    const deniedHeader = await request('status', {}, clientA, { 'x-osg-simulator': undefined });
    assert.equal(deniedHeader.status, 403);
    const initial = await request('status');
    assert.equal(initial.ok, true);
    assert.equal(initial.state.running, false);
    assert.equal(initial.state.modules.length, 4);
    assert.ok(initial.state.modules.every(module => !module.active && module.registryID === undefined));

    const withoutClaim = await request('configure', { moduleId: shifterID,
      settings: { label: 'Unauthorized', debounce: 10, inverted: false } });
    assert.equal(withoutClaim.status, 400);
    assert.match(withoutClaim.error, /setup session/);
    const offlineClaim = await request('claim', { moduleId: 'OSG-HB-0023' });
    assert.equal(offlineClaim.status, 400);
    const claimed = await request('claim', { moduleId: shifterID });
    assert.deepEqual(claimed.state.claimedModules, [shifterID]);
    const conflicting = await request('claim', { moduleId: shifterID }, clientB);
    assert.equal(conflicting.status, 400);
    assert.match(conflicting.error, /Another Cockpit tab/);
    const conflictingConfigure = await request('configure', { moduleId: shifterID,
      settings: { label: 'Unauthorized', debounce: 10, inverted: false } }, clientB);
    assert.equal(conflictingConfigure.status, 400);
    const blockedLayout = await request('register', { moduleId: 'OSG-BP-0012' });
    assert.equal(blockedLayout.status, 400);
    assert.match(blockedLayout.error, /End active setup sessions/);

    const invalidSettings = await request('configure', { moduleId: shifterID,
      settings: { label: 'Shifter', debounce: -1, inverted: false } });
    assert.equal(invalidSettings.ok, false);
    assert.equal(invalidSettings.state.modules.find(module => module.id === shifterID).settings.debounce, 10);
    const configured = await request('configure', { moduleId: shifterID,
      settings: { label: 'Smoke-test shifter', debounce: 0, inverted: true } });
    assert.equal(configured.ok, true);
    assert.equal(configured.state.modules.find(module => module.id === shifterID).settings.label, 'Smoke-test shifter');
    const otherRelease = await request('release', { moduleId: shifterID }, clientB);
    assert.deepEqual(otherRelease.state.claimedModules, []);
    assert.equal((await request('claim', { moduleId: shifterID }, clientB)).ok, false,
      'a different client cannot release an owned claim');
    const released = await request('release', { moduleId: shifterID });
    assert.deepEqual(released.state.claimedModules, []);
    assert.equal((await request('claim', { moduleId: shifterID }, clientB)).ok, true);
    await request('release', { moduleId: shifterID }, clientB);

    const started = await request('start');
    if (started.ok) {
      assert.equal(started.state.running, true);
      assert.ok(started.state.modules.every(module => module.active && module.registryID));
      const invalidReport = await request('input', { moduleId: 'OSG-PD-0018', axes: [0, 0, 65536] });
      assert.equal(invalidReport.ok, false);
      assert.deepEqual(invalidReport.state.modules.find(module => module.id === 'OSG-PD-0018').axes, [0, 0, 0]);
    } else {
      assert.match(started.error, /macOS denied virtual HID creation|virtual.device|entitlement/);
      assert.equal(started.state.running, false);
      assert.ok(started.state.modules.every(module => !module.active && module.registryID === undefined),
        'a failed CoreHID creation must never be reported as connected');
      const invalidReport = await request('input', { moduleId: 'OSG-PD-0018', axes: [0, 0, 65536] });
      assert.equal(invalidReport.ok, false);
      assert.deepEqual(invalidReport.state.modules.find(module => module.id === 'OSG-PD-0018').axes, [0, 0, 0]);
    }
    const stopped = await request('stop');
    assert.equal(stopped.ok, true);
    assert.equal(stopped.state.running, false);
    assert.ok(stopped.state.modules.every(module => !module.active && module.registryID === undefined));
    const status = await request('status');
    assert.equal(status.ok, true, 'the actual helper remains responsive after rejected commands');
  } finally {
    server.emit('close');
    if (previousHelper === undefined) delete process.env.OSG_HID_HELPER;
    else process.env.OSG_HID_HELPER = previousHelper;
  }
});

test('bridge recovers from a crashed or unresponsive child and does not relaunch after close', { skip: process.platform !== 'darwin', timeout: 20_000 }, async () => {
  // A deliberately faulty child exercises bridge supervision only. Native HID
  // creation and permissions are checked by the separate real-helper test.
  const directory = await mkdtemp(join(tmpdir(), 'osg-bridge-test-'));
  const markerPath = join(directory, 'first-launch');
  const pidPath = join(directory, 'pid');
  const faultyHelper = join(directory, 'faulty-helper.mjs');
  const fixture = `#!/usr/bin/env node
import { createInterface } from 'node:readline';
import { existsSync, writeFileSync } from 'node:fs';
const marker = ${JSON.stringify(markerPath)};
writeFileSync(${JSON.stringify(pidPath)}, String(process.pid));
const first = !existsSync(marker);
if (first) writeFileSync(marker, 'started');
createInterface({ input: process.stdin }).on('line', line => {
  const request = JSON.parse(line);
  if (first) process.exit(17);
  if (request.command === 'input') return;
  process.stdout.write(JSON.stringify({ id: request.id, ok: true,
    state: { running: false, needsReconnect: false, modules: [{ id: 'OSG-SQ-0042', online: true }] } }) + '\\n');
});
`;
  await writeFile(faultyHelper, fixture, { mode: 0o700 });
  const previousHelper = process.env.OSG_HID_HELPER;
  process.env.OSG_HID_HELPER = faultyHelper;
  const server = new EventEmitter();
  let middleware;
  gatewaySimulator().configureServer({ httpServer: server, middlewares: { use(_path, handler) { middleware = handler; } } });
  const request = async command => {
    const req = Readable.from([Buffer.from(JSON.stringify({ command, clientId: clientA, moduleId: shifterID }))]);
    Object.assign(req, { method: 'POST', url: '/', socket: { remoteAddress: '127.0.0.1' },
      headers: { host: '127.0.0.1:5173', origin: 'http://127.0.0.1:5173',
        'content-type': 'application/json', 'x-osg-simulator': '1' } });
    let body;
    const res = { statusCode: 200, setHeader() {}, end(value) { body = value; } };
    await middleware(req, res);
    return { status: res.statusCode, ...JSON.parse(body) };
  };
  try {
    const crashed = await request('status');
    assert.equal(crashed.ok, false);
    assert.match(crashed.error, /exited/);
    assert.equal((await request('status')).ok, true, 'the next request must relaunch a crashed helper');
    assert.deepEqual((await request('claim')).state.claimedModules, [shifterID]);
    const timedOut = await request('input');
    assert.equal(timedOut.ok, false);
    assert.match(timedOut.error, /did not respond/);
    const recovered = await request('status');
    assert.equal(recovered.ok, true, 'a timed-out child must be replaced');
    assert.deepEqual(recovered.state.claimedModules, [], 'a failed helper invalidates its setup sessions');
    server.emit('close');
    const closed = await request('status');
    assert.equal(closed.ok, false);
    assert.match(closed.error, /stopped|closed/);
  } finally {
    server.emit('close');
    await delay(50);
    // Ensure that a failing assertion cannot leave the deliberate faulty child.
    const pid = Number(await readFile(pidPath, 'utf8').catch(() => '0'));
    if (pid > 0) { try { process.kill(pid, 'SIGKILL'); } catch {} }
    await rm(directory, { recursive: true, force: true });
    if (previousHelper === undefined) delete process.env.OSG_HID_HELPER;
    else process.env.OSG_HID_HELPER = previousHelper;
  }
});
