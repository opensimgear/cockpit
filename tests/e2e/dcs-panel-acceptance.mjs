// Software qualification only: Rust Cockpit, production C nodes and UDP export.
import test from 'node:test';
import assert from 'node:assert/strict';
import { randomBytes } from 'node:crypto';
import { spawn } from 'node:child_process';
import { createSocket } from 'node:dgram';
import { createInterface } from 'node:readline';
import { mkdir, mkdtemp, readFile, writeFile } from 'node:fs/promises';
import { dirname, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { performance as monotonic } from 'node:perf_hooks';

const root = resolve(dirname(fileURLToPath(import.meta.url)), '../..');
const sleep = ms => new Promise(resolve => setTimeout(resolve, ms));
const percentile = (values, quantile) => [...values].sort((a, b) => a - b)[Math.max(0, Math.ceil(values.length * quantile) - 1)];
async function udpPort() {
  const socket = createSocket('udp4');
  await new Promise(resolve => socket.bind(0, '127.0.0.1', resolve));
  const port = socket.address().port;
  await new Promise(resolve => socket.close(resolve));
  return port;
}

// Service responses can complete out of order; simulator admin responses are
// FIFO and have an `id`, rather than the service's requestId envelope.
function processClient(executable, args, env) {
  const child = spawn(executable, args, { stdio: ['pipe', 'pipe', 'pipe'], env });
  const pending = [];
  let stderr = '', failure, sequence = 0;
  child.stderr.on('data', bytes => { stderr = (stderr + bytes).slice(-16384); });
  function fail(error) {
    failure = error;
    for (const item of pending.splice(0)) { clearTimeout(item.timer); item.reject(error); }
  }
  createInterface({ input: child.stdout }).on('line', line => {
    let reply;
    try { reply = JSON.parse(line); } catch { fail(new Error(`Invalid process JSON: ${line}`)); return; }
    const index = pending.findIndex(item => item.requestId === reply.requestId);
    if (index < 0) { fail(new Error(`Unexpected process JSON: ${line}`)); return; }
    const [item] = pending.splice(index, 1);
    clearTimeout(item.timer);
    item.resolve(item.requestId === undefined ? reply : reply.response);
  });
  child.once('error', fail);
  child.stdin.on('error', fail);
  child.once('exit', (code, signal) => fail(new Error(`Process exited (${code ?? signal}): ${stderr}`)));
  function send(line, requestId) {
    if (failure) return Promise.reject(failure);
    return new Promise((resolve, reject) => {
      const timer = setTimeout(() => { fail(new Error(`Process timeout: ${line}\n${stderr}`)); child.kill('SIGKILL'); }, 20000);
      pending.push({ resolve, reject, timer, requestId });
      child.stdin.write(`${line}\n`, error => { if (error) fail(error); });
    });
  }
  return {
    line: line => send(line),
    request(value) {
      const requestId = `radio-${++sequence}`;
      return send(JSON.stringify({ requestId, request: value }), requestId);
    },
    async stop() {
      if (child.exitCode !== null || child.signalCode !== null) return;
      const exited = new Promise(resolve => child.once('exit', resolve));
      child.kill('SIGTERM');
      const timer = setTimeout(() => child.kill('SIGKILL'), 3000);
      await exited; clearTimeout(timer);
      assert.doesNotMatch(stderr, /AddressSanitizer|runtime error:/, 'Native sanitizers remain clean');
    },
  };
}

test('radio peer validates named catalogue commands and bounds its diagnostic history', { timeout: 20000 }, async () => {
  const receive = createSocket('udp4'), send = createSocket('udp4');
  await new Promise(resolve => receive.bind(0, '127.0.0.1', resolve));
  const commandPort = await udpPort();
  const datagrams = [];
  receive.on('message', bytes => { datagrams.push(bytes); if (datagrams.length > 64) datagrams.shift(); });
  const peer = processClient(process.execPath, [resolve(root, 'simulation/dcs-bios/peer.mjs'), '--command-port', String(commandPort),
    '--export-port', String(receive.address().port), '--interval-ms', '10'], process.env);
  let sequence = 0;
  const admin = async command => {
    const reply = await peer.line(`sim ${++sequence} ${command}`);
    assert.equal(reply.ok, true, `${command}: ${reply.error}`); return reply.simulation;
  };
  const wait = async accepts => {
    const deadline = Date.now() + 3000;
    let state;
    do { state = await admin('inspect'); if (accepts(state)) return state; await sleep(10); } while (Date.now() < deadline);
    assert.fail(`Peer state did not settle: ${JSON.stringify(state)}`);
  };
  const transmit = lines => new Promise((resolve, reject) => send.send(Buffer.from(lines), commandPort, '127.0.0.1', error => error ? reject(error) : resolve()));
  try {
    await wait(() => datagrams.length > 0);
    await admin('warning 1');
    await transmit('UNKNOWN 1\nUHF_VOL 65536\nUHF_VOL +0\nUHF_SQUELCH INC\nUHF_TEST 2\n');
    await transmit('UHF_PRESET_SEL INC\nUHF_PRESET_SEL DEC\nUHF_PRESET_SEL 20\nUHF_10MHZ_SEL 9\nUHF_VOL 20000\nUHF_VOL +1024\nUHF_VOL -24\nUHF_SQUELCH 1\nUHF_TEST 1\nUHF_TEST 0\nUFC_MASTER_CAUTION 1\nUFC_MASTER_CAUTION 0\n');
    let state = await wait(value => value.commandCount === 12);
    assert.deepEqual([state.warning, state.preset, state.frequency, state.volume, state.squelch, state.testPressed], [false, '20', '290.000', 21000, 1, 0]);
    assert.ok(state.commands.every(command => command.receivedAtMs > 0 && command.sequence > 0));
    for (let chunk = 0; chunk < 6; chunk++) await transmit('UHF_TEST 0\n'.repeat(50));
    state = await wait(value => value.commandCount === 312);
    assert.equal(state.commands.length, 256);
    assert.equal(state.commands[0].sequence, 57);
    assert.equal(state.commands.at(-1).sequence, 312);
    await admin('frequency 123.450'); await admin('preset 20'); await admin('volume 0');
    await admin('pause 1'); await admin('half_frame 1'); await admin('pause 0');
    state = await wait(value => value.holdingFrame);
    await admin('pause 1'); await admin('complete'); await admin('half_frame 0');
    await admin('fragment 1'); await admin('freeze 1'); await admin('pause 0');
    const frozen = await admin('inspect'); await sleep(30);
    assert.equal((await admin('inspect')).counter, frozen.counter);
    await admin('freeze 0'); await admin('flood 1'); await admin('rate 200');
    await wait(value => value.frequency !== '123.450');
    await admin('flood 0'); await admin('malformed 1'); await admin('malformed 0'); await admin('malformed');
    await admin('frequency_hex ff'); await admin('aircraft NONE');
    assert.equal((await admin('inspect')).aircraft, '');
    await admin('aircraft A-10C_2'); await admin('coalesce 4');
    await wait(() => datagrams.some(bytes => bytes.length > 200));
    const invalid = await peer.line(`sim ${++sequence} rate 1001`);
    assert.equal(invalid.ok, false);
  } finally {
    await peer.stop();
    await Promise.all([receive, send].map(socket => new Promise(resolve => socket.close(resolve))));
  }
});

test('radio profile routes sampled controls and bounded atomic displays across two independent gateways', { timeout: 180000 }, async () => {
  assert.ok(process.env.OSG_E2E_MANIFEST, 'Provision native dependencies with tests/e2e/run.py');
  const launch = JSON.parse(await readFile(process.env.OSG_E2E_MANIFEST, 'utf8'));
  await mkdir(launch.artifacts, { recursive: true });
  const directory = await mkdtemp(resolve(launch.artifacts, 'dcs-radio-'));
  const environment = { ...process.env, OSG_SIMULATION_NAMESPACE: `radio-${randomBytes(8).toString('hex')}`,
    OSG_DCS_BIOS_CONFIG: resolve(directory, 'profile.json') };
  const uids = [randomBytes(12).toString('hex'), randomBytes(12).toString('hex')];
  const gatewayIds = uids.map(uid => `stm32:${uid}`);
  const definition = JSON.parse(await readFile(resolve(launch.deviceRepository, 'simulation/configs/radio-panel.json'), 'utf8'));
  const moduleId = definition.identity;
  const boundary = '😀'.repeat(15) + '波';
  // Calibration is carried by the same runtime definition as board profiles.
  Object.assign(definition.inputs.axes[0], { inverted: true, deadzone: 1000, neutral: 32768 });
  definition.initial_samples = { axes: [2000], closed_contacts: [0] };
  const configPath = resolve(directory, 'radio.json');
  await writeFile(configPath, JSON.stringify(definition));
  const commandPort = await udpPort(), exportPort = await udpPort();
  let service = processClient(launch.serviceBinary, [], environment);
  let peer, adminId = 0;
  const gateways = [], devices = [], ports = [], latency = [[], []];
  const request = (command, index = 0, extra = {}) => service.request({ command, gatewayId: gatewayIds[index],
    clientId: 'dcs-radio-acceptance', ...extra });
  const ok = async (command, index = 0, extra = {}) => {
    const reply = await request(command, index, extra);
    if (!reply.ok) await writeFile(resolve(directory, 'last-error.json'), JSON.stringify({ command, index, reply }, null, 2));
    assert.equal(reply.ok, true, `${command}: ${reply.error}`);
    return reply.state;
  };
  const admin = async (process, command) => {
    const reply = await process.line(`sim ${++adminId} ${command}`);
    assert.equal(reply.ok, true, `${command}: ${reply.error}`);
    return reply.simulation;
  };
  const wait = async (operation, accepts, label, milliseconds = 8000) => {
    const deadline = Date.now() + milliseconds;
    let value;
    do { value = await operation(); if (accepts(value)) return value; await sleep(25); } while (Date.now() < deadline);
    assert.fail(`${label}: ${JSON.stringify(value)}`);
  };
  const status = async () => (await ok('dcs_status')).dcsBios;
  const ready = label => wait(status, state => state.running && state.feedbackFresh && state.missionActive &&
    state.inputs?.length === 6 && state.inputs.every(input => input.ready && !input.needsBaseline && !input.error), label);
  const panel = index => admin(devices[index], 'inspect');
  const values = async index => {
    const native = await panel(index);
    return { displays: native.displays.map(port => port.text), mode: native.actions.states.find(action => action.id === 'led.status.set_mode').values[0] };
  };
  const peerState = () => admin(peer, 'inspect');
  const commandCount = async name => (await peerState()).commands.filter(command => command.line.startsWith(`${name} `)).length;
  const waitCommand = (name, count, label) => wait(() => commandCount(name), value => value === count, label);
  const invoke = async (index, actionId, value) => {
    const state = await ok('status', index);
    return request('action_invoke', index, { moduleId, boot: state.modules.find(module => module.id === moduleId).boot,
      catalogueRevision: 1, actionId, values: [value] });
  };
  const inputs = [
    { id: 'preset-encoder', gatewayId: gatewayIds[0], moduleId, kind: 'encoder', control: 0, command: 'UHF_PRESET_SEL', interface: 'fixed_step' },
    { id: 'volume-axis', gatewayId: gatewayIds[0], moduleId, kind: 'axis', control: 0, command: 'UHF_VOL', interface: 'set_state', deadband: 256 },
    { id: 'squelch-switch', gatewayId: gatewayIds[0], moduleId, kind: 'switch', control: 0, command: 'UHF_SQUELCH', interface: 'set_state' },
    { id: 'test-button', gatewayId: gatewayIds[0], moduleId, kind: 'button', control: 2, command: 'UHF_TEST', interface: 'set_state' },
    { id: 'frequency-encoder', gatewayId: gatewayIds[1], moduleId, kind: 'encoder', control: 0, command: 'UHF_10MHZ_SEL', interface: 'fixed_step' },
    { id: 'caution-button', gatewayId: gatewayIds[1], moduleId, kind: 'button', control: 2, command: 'UFC_MASTER_CAUTION', interface: 'set_state' },
  ];
  const outputs = gatewayIds.flatMap((gatewayId, index) => [
    { id: `frequency-${index}`, gatewayId, moduleId, control: 'UHF_FREQUENCY', actionId: 'display.uhf.frequency.set_text', format: { kind: 'text', unknownText: '???????', staleText: '-------' } },
    { id: `preset-${index}`, gatewayId, moduleId, control: 'UHF_PRESET', actionId: 'display.uhf.preset.set_text', format: { kind: 'text', unknownText: '??', staleText: '--' } },
    { id: `volume-${index}`, gatewayId, moduleId, control: 'UHF_VOL', actionId: 'display.text.set_text', format: { kind: 'fixed', scale: 100 / 65535, decimals: 1, prefix: index ? '音量 ' : 'VOL ', staleText: index ? boundary : 'STALE' } },
    { id: `caution-${index}`, gatewayId, moduleId, control: 'MASTER_CAUTION', actionId: 'led.status.set_mode', format: { kind: 'boolean' } },
  ]);
  const config = { schemaVersion: 2, aircraftModule: 'A-10C', cataloguePath: resolve(root, 'simulation/dcs-bios/catalogue'),
    commandHost: '127.0.0.1', commandPort, listenAddress: '127.0.0.1', listenPort: exportPort,
    multicastGroup: '', multicastInterface: '127.0.0.1', freshnessMs: 500, inputs, outputs };
  const scenarios = [];
  let measured, frozenCounterEvidence;
  try {
    for (const [index, uid] of uids.entries()) {
      gateways.push(processClient(launch.gatewayBinary, ['--identity', uid, '--state-dir', resolve(directory, `gateway-${index}`)], environment));
      const inventory = await wait(() => ok('ports', index), state => state.ports.some(port => port.serialNumber === uid), 'Private gateway discovery');
      ports.push(inventory.ports.find(port => port.serialNumber === uid));
      await ok('connect', index, { backend: 'stm32', port: ports[index].path });
      devices.push(processClient(launch.python, [resolve(launch.deviceRepository, 'simulation/run.py'), '--config', configPath,
        '--gateway', uid, '--state-dir', resolve(directory, `device-${index}`), '--output-dir', launch.deviceBuild, '--no-build'], environment));
      await wait(() => ok('status', index), state => !state.layoutPending && state.modules.some(module => module.id === moduleId && module.online), 'Radio route ready');
      const actions = (await ok('action_describe', index, { moduleId })).moduleActions[moduleId].actions;
      assert.deepEqual(actions.filter(action => action.display).map(action => action.display), [
        { columns: 7, rows: 1, encoding: 'ascii' }, { columns: 2, rows: 1, encoding: 'ascii' }, { columns: 16, rows: 1, encoding: 'utf8' }]);
      const events = (await ok('events_state', index, { moduleId })).moduleEvents[moduleId];
      assert.deepEqual([events.contactButtonCount, events.maintainedButtons, events.encoderButtons, events.axisUsage], [3, [0, 1], [3, 4, 5, 6], [48]]);
      assert.deepEqual(events.axes, [32768], 'Independent event acquisition applies inversion and calibrated deadzone');
    }
    // Native output buffers, not frontend mirrors, prove bytes and cell bounds.
    assert.equal(Buffer.byteLength(boundary), 63);
    assert.equal((await invoke(0, 'display.text.set_text', boundary)).ok, true);
    assert.equal((await values(0)).displays[2], boundary);
    for (const [actionId, value] of [['display.text.set_text', '😀'.repeat(16)], ['display.text.set_text', 'x'.repeat(17)],
      ['display.uhf.frequency.set_text', 'é'], ['display.uhf.frequency.set_text', '123.4500']]) {
      assert.equal((await invoke(0, actionId, value)).ok, false, 'Invalid output never reaches the peripheral');
    }
    assert.equal((await values(1)).displays[2], '', 'Same identity on another gateway remains isolated');
    scenarios.push('advertised physical capabilities', '63-byte UTF-8 boundary', 'ASCII and cell rejection', 'same-identity gateway isolation');

    peer = processClient(process.execPath, [resolve(root, 'simulation/dcs-bios/peer.mjs'), '--command-port', String(commandPort), '--export-port', String(exportPort)], environment);
    await admin(peer, 'warning 1');
    await ok('dcs_configure', 0, { dcsConfig: config });
    assert.equal((await request('dcs_start')).ok, false, 'Active HID input cannot double-control DCS');
    for (let index = 0; index < 2; index++) {
      await ok('disable', index, { moduleId });
      await wait(() => ok('status', index), state => !state.layoutPending && state.modules.some(module => module.id === moduleId && module.online && !module.enabled && !module.published), 'HID removal settles');
    }
    await admin(devices[0], 'direct 1');
    await wait(() => ok('status'), state => state.modules.some(module => module.directUsb), 'Direct USB proof');
    assert.equal((await request('dcs_start')).ok, false, 'Direct USB input cannot double-control DCS');
    await admin(devices[0], 'direct 0');
    await wait(() => ok('status'), state => state.modules.every(module => !module.directUsb), 'CAN input ownership restored');
    await admin(devices[0], 'turn 0 3');
    await admin(devices[0], 'contact 2 1'); await admin(devices[0], 'contact 2 0'); await admin(devices[0], 'contact 2 1');
    await ok('dcs_start');
    await wait(() => values(0), value => value.displays[0] === '251.000' && value.displays[1] === '01' && value.mode === 'on', 'Initial feedback applies full-width strings');
    assert.equal(await commandCount('UHF_PRESET_SEL'), 0, 'Historical encoder detents never replay');
    assert.equal(await commandCount('UHF_TEST'), 0, 'Held startup button never synthesizes a press');
    await admin(devices[0], 'contact 2 0'); await ready('Startup release arms the current event cursor');
    const initialCommands = (await peerState()).commands.map(command => command.line);
    assert.ok(initialCommands.includes('UHF_SQUELCH 1'), 'Maintained switch synchronizes its current startup state');
    assert.ok(initialCommands.includes('UHF_VOL 32768'), 'Available calibrated axis synchronizes its absolute startup state');
    await wait(() => values(0), value => value.displays[2].trim() === 'VOL 50.0', 'Catalogue integer feedback is scaled to the native text port');
    await wait(() => values(1), value => value.displays[2].trim() === '音量 50.0', 'UTF-8 formatting reaches the second native text port');
    const preservation = await Promise.all(devices.map((_, index) => panel(index)));
    const gatewayPreservation = await Promise.all(gateways.map(gateway => admin(gateway, 'inspect')));
    scenarios.push('HID/direct USB exclusion', 'no historical button or encoder replay', 'current switch and calibrated axis baseline');

    // An incomplete export has all string bytes, but lacks the update-counter
    // commit. No driver may see the new string until that last block arrives.
    await admin(peer, 'pause 1');
    await admin(peer, 'frequency 123.450'); await admin(peer, 'preset 20');
    await admin(peer, 'half_frame 1'); await admin(peer, 'pause 0');
    await wait(peerState, state => state.holdingFrame, 'Half frame exported');
    for (let probe = 0; probe < 4; probe++) {
      assert.equal((await values(0)).displays[0], '251.000', 'Incomplete feedback cannot expose a partial display');
      await sleep(15);
    }
    await admin(peer, 'pause 1'); await admin(peer, 'complete');
    await wait(() => values(0), value => value.displays[0] === '123.450' && value.displays[1] === '20', 'Counter atomically commits full strings');
    await admin(peer, 'half_frame 0'); await admin(peer, 'fragment 1'); await admin(peer, 'pause 0');
    await ready('Fragmented complete frames stay fresh');
    await admin(peer, 'frequency é');
    await wait(status, state => state.outputs.some(output => output.id === 'frequency-0' && output.error), 'ASCII formatting failure is visible');
    await wait(() => values(0), value => value.displays[0] === '-------', 'Encoding errors select a valid configured fallback');
    await admin(peer, 'frequency_hex ff');
    await wait(() => values(0), value => value.displays[0] === '???????', 'Malformed UTF-8 selects configured unknown text');
    await admin(peer, 'frequency 251.000'); await admin(peer, 'preset 01'); await admin(peer, 'coalesce 4');
    await wait(() => values(1), value => value.displays[0] === '251.000' && value.displays[1] === '01', 'Coalesced and fragmented frames recover on both gateways');
    scenarios.push('incomplete-frame atomic strings', 'fragmented/coalesced exports', 'invalid encoding feedback');

    // Each burst physically visits all four Gray-code edges; no event injection.
    await admin(devices[0], 'turn 0 20'); await admin(devices[0], 'turn 0 -5');
    await admin(devices[1], 'turn 0 12'); await admin(devices[1], 'turn 0 -3');
    await waitCommand('UHF_PRESET_SEL', 25, 'All rapid preset detents arrive');
    await waitCommand('UHF_10MHZ_SEL', 15, 'Second gateway detents arrive independently');
    const rapidCommands = (await peerState()).commands;
    assert.equal(rapidCommands.filter(command => command.line === 'UHF_PRESET_SEL INC').length, 20);
    assert.equal(rapidCommands.filter(command => command.line === 'UHF_PRESET_SEL DEC').length, 5);
    await admin(devices[0], 'contact 2 1'); await admin(devices[0], 'contact 2 0');
    await waitCommand('UHF_TEST', 2, 'Short momentary press and release survive between polls');
    await admin(devices[0], 'contact 0 0'); await wait(() => peerState(), state => state.squelch === 0, 'Maintained switch transition applies');
    await admin(devices[0], 'axis 0 100'); await wait(() => peerState(), state => state.volume === 65535, 'Inverted calibrated minimum maps to full-scale volume');
    await admin(devices[0], 'axis 0 3900'); await wait(() => peerState(), state => state.volume === 0, 'Inverted calibrated maximum maps to zero');
    const axisCommands = await commandCount('UHF_VOL');
    await admin(devices[0], 'axes_available 0');
    await wait(status, state => state.inputs.some(input => input.id === 'volume-axis' && !input.ready), 'Unavailable ADC is visible');
    assert.equal(await commandCount('UHF_VOL'), axisCommands, 'Unavailable axis never invents a neutral simulator command');
    await admin(devices[0], 'axes_available 1'); await ready('Axis availability restores current absolute synchronization');
    scenarios.push('rapid bidirectional sampled encoders', 'short momentary pair', 'maintained switch transitions', 'independent calibrated unavailable axes');

    // Invalidated cursors must discard motion that happened during stale export.
    const beforeStale = await commandCount('UHF_PRESET_SEL');
    await admin(peer, 'pause 1');
    await wait(status, state => !state.feedbackFresh && state.inputs.every(input => !input.ready), 'Export staleness disarms every target');
    await wait(() => values(0), value => value.displays[0] === '-------' && value.mode === 'off', 'Stale fallback reaches real outputs');
    await wait(() => values(1), value => value.displays[2] === boundary, 'Runtime fallback preserves the full 63-byte, 16-cell UTF-8 boundary');
    await admin(devices[0], 'turn 0 4'); await admin(devices[0], 'contact 2 1'); await admin(devices[0], 'contact 2 0');
    await admin(peer, 'pause 0'); await ready('Recovery establishes fresh baselines before accepting input');
    assert.equal(await commandCount('UHF_PRESET_SEL'), beforeStale, 'Stale-era detents never replay');
    assert.equal(await commandCount('UHF_TEST'), 2, 'Stale-era momentary pair never replays');
    for (const fault of ['freeze', 'malformed']) {
      await admin(peer, `${fault} 1`);
      await wait(status, state => !state.feedbackFresh && state.inputs.every(input => !input.ready), `${fault} export disarms the profile`);
      await admin(devices[0], 'turn 0 1');
      if (fault === 'freeze') {
        await admin(devices[0], 'contact 2 1'); await admin(devices[0], 'contact 2 0');
        await wait(async () => Promise.all([values(0), values(1)]), panels => panels.every((panel, index) =>
          panel.displays[0] === '-------' && panel.displays[1] === '--' && panel.mode === 'off' &&
          (index ? panel.displays[2] === boundary : panel.displays[2].trim() === 'STALE')), 'Frozen counters apply every gateway fallback');
        const before = await peerState(), started = monotonic.now();
        let probes = 0;
        for (;;) {
          const [integration, ...panels] = await Promise.all([status(), values(0), values(1)]);
          assert.equal(integration.feedbackFresh, false, 'Repeated frozen packets never restore feedback freshness');
          assert.ok(integration.inputs.every(input => !input.ready), 'Inputs remain disarmed throughout frozen export');
          for (const [index, panel] of panels.entries()) {
            assert.equal(panel.displays[0], '-------', 'Frequency stale fallback remains applied');
            assert.equal(panel.displays[1], '--', 'Preset stale fallback remains applied');
            assert.equal(panel.mode, 'off', 'LED stale fallback remains applied');
            assert.equal(index ? panel.displays[2] : panel.displays[2].trim(), index ? boundary : 'STALE', 'Text stale fallback remains applied');
          }
          probes++;
          if (monotonic.now() - started >= 2 * config.freshnessMs) break;
          await sleep(25);
        }
        const after = await peerState();
        assert.equal(after.counter, before.counter, 'The peer continues sending the same update counter');
        assert.ok(after.framesSent - before.framesSent >= 10, 'Frozen export continues across both freshness windows');
        assert.equal(await commandCount('UHF_PRESET_SEL'), beforeStale, 'Frozen-era motion remains suppressed throughout the stale interval');
        assert.equal(await commandCount('UHF_TEST'), 2, 'Frozen-era momentary pairs remain suppressed');
        frozenCounterEvidence = { durationMs: monotonic.now() - started, freshnessMs: config.freshnessMs,
          probes, continuingFrames: after.framesSent - before.framesSent, counter: after.counter };
      }
      await admin(peer, `${fault} 0`); await ready(`${fault} recovery establishes a fresh baseline`);
      assert.equal(await commandCount('UHF_PRESET_SEL'), beforeStale, `${fault}-era detents never replay`);
      assert.equal(await commandCount('UHF_TEST'), 2, `${fault}-era momentary pairs never replay`);
    }
    await admin(peer, 'aircraft NONE'); await wait(status, state => !state.missionActive, 'Mission end invalidates bindings');
    await admin(devices[1], 'turn 0 2'); await admin(peer, 'aircraft A-10C_2'); await ready('Installed aircraft alias starts a fresh mission baseline');
    assert.equal(await commandCount('UHF_10MHZ_SEL'), 15, 'Mission-era encoder events never replay');
    await admin(devices[0], 'reboot');
    await ready('A new panel boot establishes a new route and event baseline');
    assert.equal(await commandCount('UHF_PRESET_SEL'), beforeStale, 'Panel boot does not replay historical events');
    scenarios.push('stale text/LED fallback', 'stale-era replay suppression', 'frozen counters and malformed exports',
      'sustained frozen-counter input disarm and fallback', 'mission/alias baseline', 'panel reboot scope');

    // Measure only an uninterrupted run; deliberate 4s lost replies below are
    // transport-fault tests, not normal-path latency samples.
    await ok('dcs_stop'); await ok('dcs_start'); await ready('Performance run establishes baselines');
    await admin(peer, 'fragment 0'); await admin(peer, 'rate 200'); await admin(peer, 'flood 1');
    for (let port = 0; port < 3; port++) await admin(devices[0], `display_delay ${port} 40`);
    const floodStarted = Date.now();
    const frameBefore = (await peerState()).framesSent;
    for (let sample = 0; sample < 20; sample++) for (let index = 0; index < 2; index++) {
      const command = index ? 'UHF_10MHZ_SEL INC' : 'UHF_PRESET_SEL INC';
      const cursor = (await peerState()).commandCount;
      const started = Date.now();
      await admin(devices[index], 'turn 0 1');
      const received = await wait(peerState, state => state.commands.some(item => item.sequence > cursor && item.line === command), 'Input remains responsive under feedback flood');
      latency[index].push(received.commands.find(item => item.sequence > cursor && item.line === command).receivedAtMs - started);
    }
    const floodStopped = Date.now();
    const finalPeer = await admin(peer, 'flood 0');
    await admin(peer, 'rate 30');
    for (let port = 0; port < 3; port++) await admin(devices[0], `display_delay ${port} 0`);
    await wait(() => values(0), value => value.displays[0] === finalPeer.frequency && value.displays[1] === finalPeer.preset, 'Latest flooded outputs converge without queueing history');
    measured = await wait(status, state => state.outputs.length === 8 && state.outputs.every(output => !output.pending && !output.error),
      'Every latest output converges after the sustained flood');
    for (let index = 0; index < 2; index++) {
      const actual = (await panel(index)).actions.states;
      for (const output of measured.outputs.filter(output => output.gatewayId === gatewayIds[index])) {
        assert.deepEqual(actual.find(action => action.id === output.actionId).values, output.desired,
          'The latest desired setter equals the actual native peripheral readback');
      }
    }
    assert.equal(measured.gateways.length, 2);
    for (const [index, gatewayId] of gatewayIds.entries()) {
      const metrics = measured.gateways.find(gateway => gateway.gatewayId === gatewayId);
      assert.ok(metrics, 'Every gateway reports bounded work metrics');
      assert.ok(metrics.queueDepth <= 4 && metrics.queueHighWater <= 4, 'Output queue is bounded by the four configured endpoints');
      assert.ok(metrics.actionWrites > 0 && metrics.throughputPerSecond > 0);
      assert.ok(metrics.inputPollMaxIntervalMs > 0, 'Input poll interval is measured');
      assert.ok(metrics.latencyP95Ms >= 0 && metrics.latencyMaxMs >= metrics.latencyP95Ms);
      assert.ok(percentile(latency[index], .95) <= 250, `Gateway ${index} native physical-to-peer input p95 exceeds 250ms: ${latency[index]}`);
    }
    assert.ok(measured.gateways.some(gateway => gateway.coalesced > 0), 'Sustained fast feedback replaces obsolete pending setters');
    assert.ok(finalPeer.framesSent - frameBefore >= 100, 'Flood runs long enough to exercise bounded replacement');
    scenarios.push('sustained feedback flood', 'latest setter convergence', 'bounded per-gateway queue', 'input latency under slow peripheral callbacks');
    const performance = { qualification: 'native software only; physical hardware and live DCS deferred', floodDurationMs: floodStopped - floodStarted,
      exportedFrames: finalPeer.framesSent - frameBefore, gateways: measured.gateways,
      physicalToPeer: gatewayIds.map((gatewayId, index) => ({ gatewayId, samplesMs: latency[index], p95Ms: percentile(latency[index], .95), maxMs: Math.max(...latency[index]) })) };
    await writeFile(resolve(directory, 'radio-performance.json'), JSON.stringify(performance, null, 2));
    for (let index = 0; index < 2; index++) {
      const current = await panel(index);
      for (const field of ['settingsErases', 'settingsPrograms', 'moduleReconnects', 'descriptorHex']) {
        assert.deepEqual(current[field], preservation[index][field], `Normal DCS bindings preserve device ${field}`);
      }
      const gateway = await admin(gateways[index], 'inspect');
      for (const field of ['registryErases', 'registryPrograms', 'gatewayReconnects', 'descriptorHex']) {
        assert.deepEqual(gateway[field], gatewayPreservation[index][field], `Normal DCS bindings preserve gateway ${field}`);
      }
    }
    scenarios.push('volatile outputs preserve flash, descriptors and configuration');

    // Only invocation acknowledgements are lost; another gateway continues.
    const beforeLost = await panel(0);
    await admin(gateways[0], 'drop_action_reply 1'); await admin(peer, 'frequency 123.450');
    await wait(() => panel(0), state => state.displays[0].text === '123.450', 'Lost setter reply still applies to actual peripheral');
    const independentCount = await commandCount('UFC_MASTER_CAUTION');
    await admin(devices[1], 'contact 2 1'); await admin(devices[1], 'contact 2 0');
    await waitCommand('UFC_MASTER_CAUTION', independentCount + 2, 'Other gateway continues during a lost reply');
    await wait(status, state => state.outputs.some(output => output.gatewayId === gatewayIds[0] && output.error), 'Lost acknowledgement is reported', 12000);
    await admin(gateways[0], 'drop_action_reply 0');
    await ok('connect', 0, { backend: 'stm32', port: ports[0].path });
    await ready('Reconnect refreshes readback and event baselines');
    await wait(status, state => state.outputs.every(output => !output.error && !output.pending), 'Readable setters reconcile after lost acknowledgement');
    const afterLost = await panel(0);
    assert.ok(afterLost.displays[0].writes <= beforeLost.displays[0].writes + 1, 'Equal applied readback prevents a blind repeat of the lost setter');
    scenarios.push('lost setter acknowledgement', 'independent gateway during timeout', 'read-before-reconcile recovery');

    const testBeforeStop = await commandCount('UHF_TEST');
    await admin(devices[0], 'contact 2 1'); await waitCommand('UHF_TEST', testBeforeStop + 1, 'Held press forwarded before Stop');
    await ok('dcs_stop'); await waitCommand('UHF_TEST', testBeforeStop + 2, 'Stop sends exactly one neutral release');
    await admin(devices[0], 'contact 2 0');
    assert.equal(await commandCount('UHF_TEST'), testBeforeStop + 2);
    // Exercise the catalogue's relative interface on a second physical encoder,
    // with the axis binding removed so one command still has one owner.
    const variableConfig = structuredClone(config);
    Object.assign(variableConfig.inputs.find(input => input.id === 'volume-axis'), {
      gatewayId: gatewayIds[1], kind: 'encoder', control: 1, interface: 'variable_step', step: 1024 });
    await ok('dcs_configure', 0, { dcsConfig: variableConfig });
    await admin(peer, 'volume 20000'); await ok('dcs_start'); await ready('Relative encoder profile starts with a fresh baseline');
    const relativeCursor = (await peerState()).commandCount;
    await admin(devices[1], 'turn 1 3');
    await wait(peerState, state => state.commands.filter(command => command.sequence > relativeCursor && command.line === 'UHF_VOL +1024').length === 3,
      'Variable-step interface receives all positive physical detents');
    await wait(peerState, state => state.volume === 23072, 'Relative encoder commands apply the configured step');
    await ok('dcs_stop'); await ok('dcs_configure', 0, { dcsConfig: config });
    scenarios.push('catalogue variable-step encoder');
    await service.stop(); service = processClient(launch.serviceBinary, [], environment);
    const restored = await status();
    assert.equal(restored.running, false, 'A saved profile never automatically starts on service restart');
    assert.equal(restored.config.schemaVersion, 2);
    assert.deepEqual(restored.config.inputs.map(input => input.id), inputs.map(input => input.id));
    scenarios.push('one-time neutral Stop', 'saved V2 profile without automatic start');
    await writeFile(resolve(directory, 'radio-acceptance.json'), JSON.stringify({ passed: true, gatewayIds, moduleId, scenarios, performance, frozenCounterEvidence,
      readback: await Promise.all(devices.map((_, index) => panel(index))) }, null, 2));
  } catch (error) {
    const diagnostics = await Promise.allSettled([status(), ...gatewayIds.map((_, index) => ok('status', index)),
      ...devices.map((_, index) => panel(index)), ...(peer ? [peerState()] : [])]);
    await writeFile(resolve(directory, 'radio-failure.json'), JSON.stringify({ error: String(error), diagnostics }, null, 2));
    throw error;
  } finally {
    const cleanup = await Promise.allSettled([service, peer, ...devices, ...gateways].filter(Boolean).map(process => process.stop()));
    const failures = cleanup.filter(result => result.status === 'rejected');
    assert.equal(failures.length, 0, failures.map(result => String(result.reason)).join('\n'));
  }
});
