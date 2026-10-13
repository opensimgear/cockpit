// Ordinary Rust discovery/serial protocol, independently running production C nodes.
import test from 'node:test';
import assert from 'node:assert/strict';
import { randomBytes } from 'node:crypto';
import { spawn } from 'node:child_process';
import { createInterface } from 'node:readline';
import { mkdir, mkdtemp, readFile, writeFile } from 'node:fs/promises';
import { resolve } from 'node:path';
const sleep = ms => new Promise(resolve => setTimeout(resolve, ms));

function processClient(executable, args = [], env = process.env) {
  const child = spawn(executable, args, { stdio: ['pipe', 'pipe', 'pipe'], env });
  const pending = [];
  let nextRequestId = 0;
  let stderr = '';
  let failure;
  child.stderr.on('data', bytes => { stderr = (stderr + bytes).slice(-8192); });
  function fail(error) {
    failure = error;
    for (const task of pending.splice(0)) { clearTimeout(task.timer); task.reject(error); }
  }
  createInterface({ input: child.stdout }).on('line', line => {
    let response;
    try { response = JSON.parse(line); } catch { fail(new Error(`Invalid process JSON: ${line}`)); return; }
    const index = pending.findIndex(task => task.requestId === response.requestId);
    if (index < 0) { fail(new Error(`Unexpected process output: ${line}`)); return; }
    const [task] = pending.splice(index, 1);
    clearTimeout(task.timer);
    task.resolve(task.requestId === undefined ? response : response.response);
  });
  child.once('error', fail);
  child.stdin.on('error', fail);
  child.once('exit', (code, signal) => fail(new Error(`Process exited (${code ?? signal}): ${stderr}`)));
  function send(value, requestId) {
      if (failure) return Promise.reject(failure);
      return new Promise((resolve, reject) => {
        const timer = setTimeout(() => {
          const error = new Error(`Process response timed out: ${value}\n${stderr}`);
          fail(error); child.kill('SIGKILL');
        }, 20_000);
        pending.push({ resolve, reject, timer, requestId });
        child.stdin.write(`${value}\n`, error => { if (error) fail(error); });
      });
  }
  return {
    child,
    assertRunning() { if (failure) throw failure; },
    line: value => send(value),
    request(value) {
      const requestId = `acceptance-${++nextRequestId}`;
      return send(JSON.stringify({ requestId, request: value }), requestId);
    },
    async stop(signal = 'SIGTERM') {
      if (child.exitCode !== null || child.signalCode !== null) return;
      const exited = new Promise(resolve => child.once('exit', resolve));
      child.kill(signal);
      const timeout = setTimeout(() => child.kill('SIGKILL'), 3000);
      await exited;
      clearTimeout(timeout);
    },
  };
}

test('Cockpit starts first, discovers three JSON devices and recovers saved configuration across independent restarts', { timeout: 120_000 }, async () => {
  assert.ok(process.env.OSG_E2E_MANIFEST, 'Run tests/e2e/run.py to provision external components first');
  const launch = JSON.parse(await readFile(process.env.OSG_E2E_MANIFEST, 'utf8'));
  const executable = launch.gatewayBinary;
  const serviceExecutable = launch.serviceBinary;
  const environment = { ...process.env };
  const uid = randomBytes(12).toString('hex');
  const gatewayId = `stm32:${uid}`;
  const identities = Array.from({ length: 3 }, () => randomBytes(16).toString('hex'));
  const moduleId = identities[0];
  await mkdir(launch.artifacts, { recursive: true });
  const stateDirectory = await mkdtemp(resolve(launch.artifacts, 'cockpit-'));
  const definitions = [];
  for (const [index, name] of ['pedals', 'button-panel', 'shifter'].entries()) {
    const definition = JSON.parse(await readFile(resolve(launch.deviceRepository, 'simulation/configs', `${name}.json`), 'utf8'));
    definition.identity = identities[index];
    const path = resolve(stateDirectory, `${name}.json`);
    await writeFile(path, JSON.stringify(definition, null, 2));
    definitions.push({ path, definition });
  }
  let service = processClient(serviceExecutable, [], environment);
  let gateway;
  let device;
  const peers = [];
  let adminId = 0;
  let clientId = 'standalone-runtime-acceptance-client';
  const request = (command, extra = {}) => service.line(JSON.stringify({ command,
    ...(command === 'ports' ? {} : { gatewayId }), ...extra, clientId }));
  const ok = async (command, extra) => {
    const response = await request(command, extra);
    assert.equal(response.ok, true, `${command} ${extra?.moduleId ?? ''}: ${response.error}`);
    return response.state;
  };
  const admin = async (process, action) => {
    const response = await process.line(`sim ${++adminId} ${action}`);
    assert.equal(response.ok, true, response.error);
    return response.simulation;
  };
  const wait = async (operation, accepts, description) => {
    let result;
    for (let attempt = 0; attempt < 60; attempt++) {
      result = await operation();
      if (accepts(result)) return result;
      await sleep(100);
    }
    assert.fail(`${description}: ${JSON.stringify(result)}`);
  };
  const startGateway = () => processClient(executable, ['--identity', uid, '--state-dir', resolve(stateDirectory, 'gateway')], environment);
  const startDevice = (index = 0) => processClient(launch.python, [resolve(launch.deviceRepository, 'simulation/run.py'),
    '--config', definitions[index].path, '--gateway', uid, '--state-dir', resolve(stateDirectory, `device-${index}`),
    '--output-dir', launch.deviceBuild, '--no-build'], environment);
  try {
    let state = await ok('ports');
    assert.ok(!state.ports.some(port => port.serialNumber === uid), 'Cockpit never fabricates a gateway before its process starts');
    gateway = startGateway();
    state = await wait(() => { gateway.assertRunning(); return ok('ports'); }, state => state.ports.some(port => port.serialNumber === uid), 'Gateway advertises independently');
    let port = state.ports.find(port => port.serialNumber === uid);
    assert.equal(port.backend, 'stm32');
    assert.ok(port.path.startsWith('tcp:127.0.0.1:'), 'An advertised loopback transport is discovered without emulator configuration');
    assert.ok(port.path.endsWith(`/${uid}`), 'The local endpoint is bound to the advertised gateway identity');
    state = await ok('connect', { backend: 'stm32', port: port.path });
    assert.equal(state.gatewaySerial, uid);
    assert.deepEqual(state.modules, [], 'The gateway starts without an invented device');

    device = startDevice();
    state = await wait(() => { device.assertRunning(); return ok('status'); }, state => state.modules.some(module => module.id === moduleId && module.online), 'Separately started device is discovered over CAN');
    assert.equal(state.protocolVersion, 3);
    assert.equal(state.lifecycle, 'automatic');
    assert.equal(state.modules.find(module => module.id === moduleId).enabled, true);
    peers.push(startDevice(1), startDevice(2));
    state = await wait(() => ok('status'), state => state.modules.length === 3 && state.modules.every(module => module.online), 'Three independent JSON devices join the gateway');
    for (const [index, id] of identities.entries()) {
      state = await ok('config_read', { moduleId: id });
      const active = state.moduleConfigs[id].config;
      assert.equal(active.name, definitions[index].definition.name);
      assert.equal(active.axes.length, definitions[index].definition.inputs.axes?.length ?? 0);
      assert.equal(active.buttons.length, definitions[index].definition.inputs.buttons?.length ?? 0);
    }
    await ok('claim', { moduleId });
    const before = await admin(gateway, 'inspect');
    const config = {
      name: 'Independent controller é', expanderMask: 1,
      buttons: [[0, 0, 1], [1, 0, 1], [24, 1, 0]],
      axes: [[0, 48, 100, 3900, 0, 1, 1000], [1, 49, 0, 4095, 32768, 0, 1200]],
      hats: [[2, 3, 4, 5]], encoders: [[6, 7]], selectors: [[8, 9]],
    };
    state = await ok('config_apply_save', { moduleId, config });
    assert.deepEqual(state.moduleConfigs[moduleId].config, config);
    assert.equal(state.moduleConfigs[moduleId].generation, state.moduleConfigs[moduleId].savedGeneration);
    assert.equal(state.moduleConfigs[moduleId].pendingApply, false);
    assert.equal((await admin(gateway, 'inspect')).descriptorHex, before.descriptorHex, 'Module settings do not publish a pending gateway layout');
    await ok('release', { moduleId });
    state = await ok('update-profile', { moduleId });
    assert.equal(state.layoutPending, false);
    assert.equal(state.modules.find(module => module.id === moduleId).profileMatch, true);
    assert.equal(state.modules.find(module => module.id === moduleId).reportId, 1);
    assert.equal(state.modules.filter(module => module.enabled && module.published && module.profileMatch).length, 3);

    await admin(device, 'contact 0 1');
    await admin(device, 'axis 0 100');
    await admin(device, 'axis 1 4095');
    await admin(device, 'contact 2 1');
    state = await wait(() => ok('status'), state => {
      const module = state.modules.find(module => module.id === moduleId);
      return module?.buttons.includes(1) && module.axes[0] === 65535 && module.axes[1] === 65535 && module.hats[0] === 0;
    }, 'Real module inputs reach the registered gateway report');
    const moduleState = await admin(device, 'inspect');
    const gatewayState = await admin(gateway, 'inspect');
    assert.equal(gatewayState.gatewayReports[0].hex, moduleState.moduleReportHex);
    assert.ok(gatewayState.descriptorHex.length > 0);

    const boot = state.modules.find(module => module.id === moduleId).boot;
    await device.stop('SIGKILL');
    state = await wait(() => ok('status'), state => !state.modules.find(module => module.id === moduleId)?.online, 'Device loss becomes a retained neutral ghost controller');
    const offline = state.modules.find(module => module.id === moduleId);
    assert.equal(state.modules.filter(module => module.online).length, 2, 'The other devices keep running independently');
    assert.equal(offline.descriptorIndex, 0);
    assert.equal(offline.enabled, true);
    assert.equal(offline.published, true);
    assert.deepEqual(offline.buttons, []);
    assert.deepEqual(offline.axes, [0, 32768]);
    assert.deepEqual(offline.hats, [8]);
    device = startDevice();
    state = await wait(() => ok('status'), state => {
      const module = state.modules.find(module => module.id === moduleId);
      return module?.online && module.boot > boot && module.profileMatch;
    }, 'Device restart restores saved configuration with a fresh boot and lease');
    state = await ok('config_read', { moduleId });
    assert.deepEqual(state.moduleConfigs[moduleId].config, config);

    const survivingOrder = state.modules.filter(module => module.id !== moduleId).sort((a, b) => a.descriptorIndex - b.descriptorIndex).map(module => ({ id: module.id, reportId: module.reportId }));
    state = await ok('disable', { moduleId });
    assert.equal(state.modules.find(module => module.id === moduleId).enabled, false);
    assert.equal(state.modules.find(module => module.id === moduleId).published, false);
    assert.equal(state.modules.find(module => module.id === moduleId).reportId, null);
    await sleep(1100);
    state = await ok('status');
    assert.equal(state.modules.find(module => module.id === moduleId).enabled, false, 'Repeated announcements cannot re-enable a remembered disabled device');
    state = await ok('config_read', { moduleId });
    assert.deepEqual(state.moduleConfigs[moduleId].config, config, 'Disabled online devices remain configurable');
    await ok('claim', { moduleId });
    state = await ok('config_apply_save', { moduleId, config });
    assert.deepEqual(state.moduleConfigs[moduleId].config, config);
    await ok('release', { moduleId });
    const epoch = state.epoch;
    await gateway.stop('SIGKILL');
    await wait(() => request('status'), response => response.state?.connected === false, 'Gateway process loss closes the native connection');
    state = await wait(() => ok('ports'), state => !state.ports.some(port => port.serialNumber === uid), 'Dead gateway advertisements are ignored');
    gateway = startGateway();
    state = await wait(() => ok('ports'), state => state.ports.some(port => port.serialNumber === uid), 'Restarted gateway is discovered through ordinary inventory');
    port = state.ports.find(port => port.serialNumber === uid);
    state = await ok('connect', { backend: 'stm32', port: port.path });
    assert.ok(state.epoch > epoch);
    state = await wait(() => ok('status'), state => {
      const module = state.modules.find(module => module.id === moduleId);
      return module?.online && module.enabled === false && module.published === false;
    }, 'Explicit disable survives gateway restart and continuing announcements');
    assert.equal(state.layoutPending, false);
    state = await ok('config_read', { moduleId });
    assert.deepEqual(state.moduleConfigs[moduleId].config, config);
    state = await ok('enable', { moduleId });
    assert.equal(state.modules.find(module => module.id === moduleId).descriptorIndex, 2, 'Explicit re-enable appends after surviving controllers');
    assert.deepEqual(state.modules.filter(module => module.id !== moduleId).sort((a, b) => a.descriptorIndex - b.descriptorIndex).map(module => ({ id: module.id, reportId: module.reportId })), survivingOrder);

    // Two fresh Cockpit processes used to reuse the same initial request IDs.
    // The device keeps its immutable completed-request cache across app exits.
    service.child.stdin.end();
    await new Promise(resolve => service.child.once('exit', resolve));
    service = processClient(serviceExecutable, [], environment);
    clientId = 'crashed-cockpit-writer-acceptance';
    await ok('connect', { backend: 'stm32', port: port.path });
    await ok('claim', { moduleId });
    const writer = (await admin(device, 'inspect')).module.owner;
    assert.ok(writer > 0);
    await service.stop('SIGKILL');
    assert.equal((await admin(device, 'inspect')).module.owner, writer, 'Crashing Cockpit preserves the firmware writer lease');

    service = processClient(serviceExecutable, [], environment);
    clientId = 'restarted-cockpit-writer-acceptance';
    state = await ok('connect', { backend: 'stm32', port: port.path });
    assert.equal(state.gatewaySerial, uid);
    assert.equal(state.modules.find(module => module.id === moduleId).profileMatch, true);
    const started = performance.now();
    const contested = await request('claim', { moduleId });
    assert.equal(contested.ok, false);
    assert.match(contested.error, /Another configuration client owns this module/);
    assert.ok(performance.now() - started < 2000, 'A new app session receives the firmware ownership error without a cached-request timeout');
    await admin(device, `direct_config release ${writer}`);
    state = await ok('claim', { moduleId });
    assert.ok(state.claimedModules.includes(moduleId), 'The restarted Cockpit can claim after the previous writer releases access');
    assert.notEqual((await admin(device, 'inspect')).module.owner, writer, 'The restarted session uses a different writer identity');
    await ok('release', { moduleId });
    assert.equal((await admin(device, 'inspect')).module.owner, 0);
    state = await ok('config_read', { moduleId });
    assert.deepEqual(state.moduleConfigs[moduleId].config, config, 'The new app session reads the saved configuration while both firmware processes keep running');

    const onlineForget = await request('forget', { moduleId });
    assert.equal(onlineForget.ok, false, 'Remembered online devices cannot be forgotten');
    await device.stop();
    state = await wait(() => ok('status'), state => state.modules.find(module => module.id === moduleId)?.online === false, 'Device disconnect is ghosted before manual removal');
    state = await ok('forget', { moduleId });
    assert.equal(state.modules.some(module => module.id === moduleId), false, 'Manual removal deletes the offline remembered identity');
    device = startDevice();
    state = await wait(() => ok('status'), state => state.modules.find(module => module.id === moduleId)?.published, 'A forgotten identity arriving again is automatically enabled as a new entry');
    assert.equal(state.modules.find(module => module.id === moduleId).descriptorIndex, 2);
    state = await ok('config_read', { moduleId });
    assert.deepEqual(state.moduleConfigs[moduleId].config, config, 'Forgetting a gateway record does not erase the module configuration');

    await admin(device, 'contact 0 1');
    await admin(device, 'axis 0 100');
    await admin(device, 'axis 1 4095');
    await admin(device, 'contact 2 1');
    await wait(() => ok('status'), state => {
      const module = state.modules.find(module => module.id === moduleId);
      return module?.buttons.includes(1) && module.axes[0] === 65535 && module.hats[0] === 0;
    }, 'Held inputs are present before disabling the controller');
    state = await ok('disable', { moduleId });
    const disabled = state.modules.find(module => module.id === moduleId);
    assert.deepEqual(disabled.buttons, [], 'Disabled inventory never retains held buttons');
    assert.deepEqual(disabled.axes, [0, 32768]);
    assert.deepEqual(disabled.hats, [8]);
    for (const id of identities.slice(1)) state = await ok('disable', { moduleId: id });
    assert.equal(state.modules.length, 3, 'Disabling all controllers preserves remembered inventory');
    assert.ok(state.modules.every(module => !module.enabled && !module.published && module.reportId === null && module.descriptorIndex === -1));
    const emptyLayout = await admin(gateway, 'inspect');
    assert.deepEqual(emptyLayout.gatewayReports, [], 'A zero-controller layout has no permanent joystick collection');
    assert.equal(emptyLayout.descriptorHex, '', 'The zero-controller layout has no joystick report descriptor');
    state = await ok('config_read', { moduleId });
    assert.equal(state.connected, true, 'CDC configuration remains available with zero enabled controllers');
    assert.deepEqual(state.moduleConfigs[moduleId].config, config);
    const reconnects = emptyLayout.gatewayReconnects;
    await sleep(1100);
    state = await ok('status');
    assert.ok(state.modules.every(module => !module.enabled && !module.published), 'Continuing announcements preserve an empty controller layout');
    assert.equal((await admin(gateway, 'inspect')).gatewayReconnects, reconnects);
    state = await ok('enable', { moduleId });
    assert.equal(state.modules.find(module => module.id === moduleId).descriptorIndex, 0, 'The first enabled controller appends to an empty layout');
    assert.equal((await admin(gateway, 'inspect')).gatewayReports.length, 1);

    service.child.stdin.end();
    await new Promise(resolve => service.child.once('exit', resolve));
    assert.doesNotThrow(() => process.kill(gateway.child.pid, 0), 'Closing Cockpit leaves the independently started gateway running');
    assert.doesNotThrow(() => process.kill(device.child.pid, 0), 'Closing Cockpit leaves the independently started device running');
    assert.ok((await admin(gateway, 'inspect')).gatewayReports.length);
    assert.equal((await admin(device, 'inspect')).module.config.name, config.name);
    await writeFile(resolve(stateDirectory, 'result.json'), JSON.stringify({ ok: true, gateway: uid, devices: identities,
      savedConfig: config, reportHex: gatewayState.gatewayReports[0].hex,
      checks: ['cockpit-first', 'automatic-device-enrollment', 'three-json-devices', 'configuration-save', 'explicit-profile-refresh', 'live-inputs',
        'offline-ghost', 'disabled-configurable', 'disabled-announcements', 'disabled-gateway-restart', 'append-reenable', 'offline-forget',
        'forgotten-auto-arrival', 'held-input-disable-neutrality', 'zero-controllers', 'enable-from-empty-layout',
        'device-restart', 'gateway-restart', 'cockpit-crash-reopen', 'independent-lifetimes'] }, null, 2));
    console.log(`Evidence: ${stateDirectory}`);
  } finally {
    await service.stop();
    await device?.stop();
    for (const peer of peers) await peer.stop();
    await gateway?.stop();
  }
});

test('one Cockpit serves simultaneous gateways with isolated device identities, configuration and recovery', { timeout: 120_000 }, async () => {
  assert.ok(process.env.OSG_E2E_MANIFEST, 'Run tests/e2e/run.py to provision external components first');
  const launch = JSON.parse(await readFile(process.env.OSG_E2E_MANIFEST, 'utf8'));
  const moduleId = randomBytes(16).toString('hex');
  const uids = Array.from({ length: 2 }, () => randomBytes(12).toString('hex'));
  const gatewayIds = uids.map(uid => `stm32:${uid}`);
  await mkdir(launch.artifacts, { recursive: true });
  const stateDirectory = await mkdtemp(resolve(launch.artifacts, 'cockpit-multi-gateway-'));
  const definition = JSON.parse(await readFile(resolve(launch.deviceRepository, 'simulation/configs/pedals.json'), 'utf8'));
  definition.identity = moduleId;
  const definitionPath = resolve(stateDirectory, 'device.json');
  await writeFile(definitionPath, JSON.stringify(definition, null, 2));
  const service = processClient(launch.serviceBinary);
  const gateways = [];
  const devices = [];
  let adminId = 0;
  const clientId = 'simultaneous-gateways-acceptance-client';
  const request = (command, index, extra = {}) => service.request({ command, clientId,
    ...(index === undefined ? {} : { gatewayId: gatewayIds[index] }), ...extra });
  const ok = async (command, index, extra) => {
    const response = await request(command, index, extra);
    assert.equal(response.ok, true, response.error);
    if (index !== undefined) assert.equal(response.state.gatewayId, gatewayIds[index]);
    return response.state;
  };
  const admin = async (process, action) => {
    const response = await process.line(`sim ${++adminId} ${action}`);
    assert.equal(response.ok, true, response.error);
    return response.simulation;
  };
  const wait = async (operation, accepts, description) => {
    let result;
    for (let attempt = 0; attempt < 60; attempt++) {
      result = await operation();
      if (accepts(result)) return result;
      await sleep(100);
    }
    assert.fail(`${description}: ${JSON.stringify(result)}`);
  };
  const startGateway = index => processClient(launch.gatewayBinary, ['--identity', uids[index],
    '--state-dir', resolve(stateDirectory, `gateway-${index}`)]);
  const startDevice = index => processClient(launch.python, [resolve(launch.deviceRepository, 'simulation/run.py'),
    '--config', definitionPath, '--gateway', uids[index], '--state-dir', resolve(stateDirectory, `device-${index}`),
    '--output-dir', launch.deviceBuild, '--no-build']);
  const configurations = [
    { name: 'Gateway A pedals', expanderMask: 0, buttons: [[0, 0, 0]],
      axes: [[0, 48, 0, 4095, 0, 0, 0]], hats: [], encoders: [], selectors: [] },
    { name: 'Gateway B pedals', expanderMask: 0, buttons: [[1, 0, 0]],
      axes: [[1, 49, 0, 4095, 32768, 0, 0], [2, 50, 0, 4095, 0, 0, 0]],
      hats: [], encoders: [], selectors: [] },
  ];
  const module = state => state.modules.find(value => value.id === moduleId);
  try {
    const before = await ok('ports');
    assert.ok(before.ports.every(port => !gatewayIds.includes(port.gatewayId)));
    gateways.push(startGateway(0), startGateway(1));
    let inventory = await wait(() => {
      gateways.forEach(process => process.assertRunning());
      return ok('ports');
    }, state => gatewayIds.every(id => state.ports.some(port => port.gatewayId === id)), 'Both running gateways appear in one discovery inventory');
    for (const index of [0, 1]) {
      const port = inventory.ports.find(port => port.gatewayId === gatewayIds[index]);
      assert.equal(port.serialNumber, uids[index]);
      assert.equal(port.backend, 'stm32');
      const state = await ok('connect', index, { backend: 'stm32', port: port.path });
      assert.equal(state.gatewaySerial, uids[index]);
      assert.deepEqual(state.modules, []);
      devices.push(startDevice(index));
    }
    const unscoped = await request('config_read', undefined, { moduleId });
    assert.equal(unscoped.ok, false, 'A command without a gateway identity cannot target either connected device');
    assert.match(unscoped.error, /gateway/i);
    for (const index of [0, 1]) {
      await wait(() => { devices[index].assertRunning(); return ok('status', index); },
        state => state.connected && module(state)?.online, `Gateway ${index} discovers its own device with the shared identity`);
      await ok('claim', index, { moduleId });
      const state = await ok('config_apply_save', index, { moduleId, config: configurations[index] });
      assert.deepEqual(state.moduleConfigs[moduleId].config, configurations[index]);
      await ok('release', index, { moduleId });
      const applied = await ok('update-profile', index, { moduleId });
      assert.equal(module(applied).reportId, 1, 'Each gateway owns its own HID report numbering');
      assert.equal(module(applied).profileMatch, true);
      assert.equal(applied.layoutPending, false);
    }
    await admin(devices[0], 'contact 0 1');
    await admin(devices[0], 'axis 0 4095');
    await admin(devices[1], 'contact 1 0');
    await admin(devices[1], 'axis 1 0');
    await admin(devices[1], 'axis 2 4095');
    await wait(() => ok('status', 0), state => module(state)?.buttons.includes(1)
      && module(state).axes[0] === 65535, 'Gateway A receives only its own device inputs');
    await wait(() => ok('status', 1), state => module(state)?.buttons.length === 0
      && module(state).axes[0] === 0 && module(state).axes[1] === 65535, 'Gateway B receives a different report for the same module identity');
    const reports = [];
    for (const index of [0, 1]) {
      const firmware = await admin(gateways[index], 'inspect');
      const device = await admin(devices[index], 'inspect');
      assert.equal(firmware.gatewayReports[0].hex, device.moduleReportHex);
      reports.push(firmware.gatewayReports[0].hex);
      const state = await ok('config_read', index, { moduleId });
      assert.deepEqual(state.moduleConfigs[moduleId].config, configurations[index], 'Identical module IDs use gateway-scoped configuration caches');
      await ok('claim', index, { moduleId });
    }
    assert.notEqual(reports[0], reports[1]);
    const ownerB = (await admin(devices[1], 'inspect')).module.owner;
    assert.ok(ownerB > 0);
    const blocked = await request('disconnect', 0);
    assert.equal(blocked.ok, false, 'Gateway A enforces its active configuration session');
    assert.match(blocked.error, /End setup sessions/);
    let stateB = await ok('status', 1);
    assert.equal(stateB.connected, true);
    assert.ok(stateB.claimedModules.includes(moduleId), 'Gateway A errors preserve gateway B configuration ownership');
    assert.equal((await admin(devices[1], 'inspect')).module.owner, ownerB);

    // The browser bridge uses correlated requests so one stalled CDC endpoint
    // must not serialize work for another gateway behind it.
    gateways[0].child.kill('SIGSTOP');
    const stalled = request('status', 0);
    let stalledCompleted = false;
    stalled.finally(() => { stalledCompleted = true; }).catch(() => {});
    try {
      await sleep(100);
      stateB = await Promise.race([ok('status', 1), sleep(1500).then(() => {
        throw new Error('Gateway B was blocked behind an unresponsive gateway A');
      })]);
      assert.equal(stateB.connected, true);
      assert.equal(stalledCompleted, false, 'Gateway B replies independently while gateway A is still paused');
    } finally {
      gateways[0].child.kill('SIGCONT');
    }
    const resumed = await stalled;
    assert.equal(resumed.ok, true, resumed.error);

    await ok('release', 0, { moduleId });
    const disconnected = await ok('disconnect', 0);
    assert.equal(disconnected.connected, false, 'Gateway A can disconnect while gateway B has an active setup session');
    stateB = await ok('status', 1);
    assert.equal(stateB.connected, true);
    assert.ok(stateB.claimedModules.includes(moduleId));
    assert.equal((await admin(devices[1], 'inspect')).module.owner, ownerB);
    inventory = await ok('ports');
    const portA = inventory.ports.find(port => port.gatewayId === gatewayIds[0]);
    await ok('connect', 0, { backend: 'stm32', port: portA.path });
    const stateA = await ok('status', 0);
    assert.equal(module(stateA).profileMatch, true);
    const epochA = stateA.epoch;
    const epochB = stateB.epoch;

    await gateways[0].stop('SIGKILL');
    let failedPeerSettled = false;
    const failedPeer = wait(() => request('status', 0), response => response.state?.connected === false,
      'Only the failed gateway connection closes').finally(() => { failedPeerSettled = true; });
    // Each real Cockpit session polls independently. Keep B's UI and writer
    // lease active while A spends its recovery deadline rediscovering its UID.
    while (!failedPeerSettled) {
      stateB = await Promise.race([ok('status', 1), sleep(1500).then(() => {
        throw new Error('Gateway B was blocked while gateway A recovered');
      })]);
      assert.equal(stateB.connected, true);
      assert.equal(stateB.epoch, epochB);
      assert.ok(stateB.claimedModules.includes(moduleId));
      await sleep(200);
    }
    await failedPeer;
    stateB = await ok('status', 1);
    assert.equal(stateB.connected, true);
    assert.equal(stateB.epoch, epochB);
    assert.ok(stateB.claimedModules.includes(moduleId), 'Gateway B keeps its writer lease after gateway A crashes');
    assert.equal((await admin(devices[1], 'inspect')).module.owner, ownerB);
    await admin(devices[1], 'contact 1 1');
    await wait(() => ok('status', 1), state => module(state)?.buttons.includes(1), 'Gateway B inputs continue while gateway A is absent');
    configurations[1] = { ...configurations[1], name: 'Gateway B stays writable' };
    stateB = await ok('config_apply_save', 1, { moduleId, config: configurations[1] });
    assert.deepEqual(stateB.moduleConfigs[moduleId].config, configurations[1]);
    inventory = await wait(() => ok('ports'), state => !state.ports.some(port => port.gatewayId === gatewayIds[0]), 'Discovery removes the stopped gateway');
    assert.ok(inventory.ports.some(port => port.gatewayId === gatewayIds[1]), 'Discovery retains the still-running gateway');

    gateways[0] = startGateway(0);
    inventory = await wait(() => ok('ports'), state => state.ports.some(port => port.gatewayId === gatewayIds[0]), 'Restarted gateway retains its stable gateway identity');
    const restoredPort = inventory.ports.find(port => port.gatewayId === gatewayIds[0]);
    await ok('connect', 0, { backend: 'stm32', port: restoredPort.path });
    const restored = await wait(() => ok('status', 0), state => state.epoch > epochA
      && module(state)?.online && module(state).enabled && module(state).published && module(state).profileMatch, 'Gateway A restores its own remembered controllers independently');
    assert.equal(restored.layoutPending, false);
    for (const index of [0, 1]) {
      const state = await ok('config_read', index, { moduleId });
      assert.equal(state.connected, true);
      assert.deepEqual(state.moduleConfigs[moduleId].config, configurations[index]);
    }
    stateB = await ok('status', 1);
    assert.equal(stateB.epoch, epochB);
    assert.ok(stateB.claimedModules.includes(moduleId));
    await ok('release', 1, { moduleId });
    assert.equal((await admin(devices[1], 'inspect')).module.owner, 0);
    await writeFile(resolve(stateDirectory, 'result.json'), JSON.stringify({ ok: true, gateways: gatewayIds,
      sharedDeviceIdentity: moduleId, configurations, reports,
      checks: ['single-service-two-gateways', 'aggregate-discovery', 'same-device-identity-isolation',
        'independent-configuration-and-hid-layouts', 'independent-live-inputs', 'scoped-errors-and-writer-leases',
        'concurrent-requests-with-stalled-peer', 'independent-disconnect', 'one-gateway-crash-and-recovery',
        'uninterrupted-peer-configuration'] }, null, 2));
    console.log(`Evidence: ${stateDirectory}`);
  } finally {
    await service.stop();
    for (const device of devices) await device.stop();
    for (const gateway of gateways) await gateway.stop();
  }
});
