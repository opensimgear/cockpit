import test from 'node:test';
import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import vm from 'node:vm';
import ts from 'typescript';

const exported = {};
vm.runInNewContext(ts.transpileModule(await readFile(new URL('./dcs-bios.ts', import.meta.url), 'utf8'), {
  compilerOptions: { module: ts.ModuleKind.CommonJS, target: ts.ScriptTarget.ES2022 },
}).outputText, { exports: exported }, { filename: new URL('./dcs-bios.ts', import.meta.url).pathname });
const { cautionLedActions, defaultDcsConfig, dcsConfigEqual, dcsConfigError, dcsStartError, dcsStatusLabel, dcsFeedbackLabel } = exported;
const plain = value => JSON.parse(JSON.stringify(value));
const gatewayId = 'stm32:00112233445566778899aabb';
const module = { id: 'panel', online: true, boot: 7, buttonCount: 4, enabled: false, published: false, directUsb: false };
const boolean = { id: 'led.boolean.set_enabled', label: 'LED 2', readable: true, semantics: 'setter', parameters: [{ type: 'boolean' }] };
const board = { ...boolean, id: 'led.status.set_mode', parameters: [{ type: 'enum', options: ['auto', 'on', 'off'] }] };
const snapshot = { boot: 7, status: 'ready', actions: [boolean, board] };
const state = { gatewayId, connected: true, backend: 'stm32', modules: [module], moduleActions: { panel: snapshot } };
const config = { ...defaultDcsConfig(gatewayId, module.id, null, [boolean]), cataloguePath: '/DCS-BIOS/doc/json' };
const stopped = { configured: true, running: false, config, aircraft: null, missionActive: false, feedbackFresh: false,
  feedbackAgeMs: null, status: 'stopped', error: null, commandsSent: 0, eventOverflow: 0, ledValue: null };

test('LED choices include only readable boolean setters and the current board mode setter', () => {
  assert.deepEqual(plain(cautionLedActions(snapshot)).map(action => action.id), [boolean.id, board.id]);
  const rejected = [
    { ...boolean, readable: false }, { ...boolean, semantics: 'oneShot' },
    { ...boolean, parameters: [] }, { ...boolean, parameters: [{ type: 'boolean' }, { type: 'boolean' }] },
    { ...boolean, parameters: [{ type: 'rgb' }] }, { ...board, id: 'other.enum' },
    { ...board, parameters: [{ type: 'enum', options: ['auto', 'on'] }] },
    { ...board, parameters: [{ type: 'enum' }] },
  ];
  assert.deepEqual(plain(cautionLedActions({ ...snapshot, actions: rejected })), []);
  for (const status of ['unknown', 'unsupported', 'error', 'uncertain']) assert.deepEqual(plain(cautionLedActions({ ...snapshot, status })), []);
  assert.deepEqual(plain(cautionLedActions()), []);
});

test('defaults select this device and preserve saved settings only for the matching binding', () => {
  const defaults = defaultDcsConfig(gatewayId, module.id);
  assert.equal(defaults.commandHost, '127.0.0.1');
  assert.equal(defaults.commandPort, 7778);
  assert.equal(defaults.listenAddress, '0.0.0.0');
  assert.equal(defaults.listenPort, 5010);
  assert.equal(defaults.multicastGroup, '239.255.50.10');
  assert.equal(defaults.multicastInterface, '127.0.0.1');
  assert.equal(defaults.freshnessMs, 1500);
  assert.equal(defaults.ledActionId, '');
  assert.equal(defaults.button, 0);
  const saved = { ...config, button: 3, ledActionId: board.id, commandPort: 8888 };
  assert.deepEqual(plain(defaultDcsConfig(gatewayId, module.id, saved, [boolean])), saved);
  const changed = defaultDcsConfig('different-gateway', 'different-panel', saved, [boolean]);
  assert.equal(changed.cataloguePath, saved.cataloguePath);
  assert.equal(changed.commandPort, 8888);
  assert.equal(changed.gatewayId, 'different-gateway');
  assert.equal(changed.moduleId, 'different-panel');
  assert.equal(changed.button, 0);
  assert.equal(changed.ledActionId, boolean.id);
  assert.equal(saved.button, 3);
});

test('saved comparisons check values rather than object key order', () => {
  assert.ok(dcsConfigEqual(config, { ...config }));
  assert.ok(dcsConfigEqual(config, Object.fromEntries(Object.entries(config).reverse())));
  assert.equal(dcsConfigEqual(config), false);
  assert.equal(dcsConfigEqual(config, null), false);
  for (const field of Object.keys(config)) assert.equal(dcsConfigEqual(config, { ...config, [field]: 'changed' }), false, field);
});

test('form rejects missing paths, controls, ports, timing and address fields', () => {
  assert.equal(dcsConfigError(config), '');
  for (const value of ['', '   ', '/path\n/json', '/path\u007f/json']) assert.match(dcsConfigError({ ...config, cataloguePath: value }), /folder/);
  assert.match(dcsConfigError({ ...config, schemaVersion: 2 }), /version/);
  for (const field of ['gatewayId', 'moduleId']) assert.match(dcsConfigError({ ...config, [field]: '' }), /device/);
  for (const button of [-1, 128, 0.5, NaN, undefined]) assert.match(dcsConfigError({ ...config, button }), /button/);
  assert.match(dcsConfigError({ ...config, ledActionId: '' }), /LED/);
  for (const field of ['commandPort', 'listenPort']) {
    for (const port of [0, 65536, 0.5, NaN, undefined]) assert.match(dcsConfigError({ ...config, [field]: port }), /UDP ports/);
    for (const port of [1, 65535]) assert.equal(dcsConfigError({ ...config, [field]: port }), '');
  }
  for (const freshnessMs of [99, 10001, NaN, 100.5, undefined]) assert.match(dcsConfigError({ ...config, freshnessMs }), /freshness/);
  for (const freshnessMs of [100, 10000]) assert.equal(dcsConfigError({ ...config, freshnessMs }), '');
  for (const field of ['commandHost', 'listenAddress', 'multicastInterface']) {
    for (const address of ['', ' ', 'localhost 2', '\n']) assert.match(dcsConfigError({ ...config, [field]: address }), /address/);
  }
  assert.equal(dcsConfigError({ ...config, multicastGroup: '' }), '');
  for (const multicastGroup of [' ', 'group\n']) assert.match(dcsConfigError({ ...config, multicastGroup }), /multicast group/);
});

test('start rechecks identity, device route and ownership without mutating the controller', () => {
  assert.equal(dcsStartError(state, module, config), '');
  for (const change of [{ connected: false }, { backend: 'pico' }]) assert.match(dcsStartError({ ...state, ...change }, module, config), /Reconnect/);
  for (const changedModule of [undefined, { ...module, online: false }, { ...module, boot: 0 }]) assert.match(dcsStartError(state, changedModule, config), /Reconnect/);
  assert.match(dcsStartError(null, module, config), /Reconnect/);
  assert.match(dcsStartError({ ...state, gatewayId: 'other' }, module, config), /different device/);
  assert.match(dcsStartError(state, { ...module, id: 'other' }, config), /different device/);
  for (const change of [{ reconnecting: true }, { layoutPending: true }]) assert.match(dcsStartError({ ...state, ...change }, module, config), /Wait/);
  assert.match(dcsStartError(state, { ...module, directUsb: true }, config), /direct USB/);
  for (const change of [{ enabled: true }, { enabled: undefined }, { published: true }, { published: undefined }]) assert.match(dcsStartError(state, { ...module, ...change }, config), /Disable/);
  assert.match(dcsStartError({ ...state, claimedModules: ['panel'] }, module, config), /setup session/);
  assert.deepEqual(module, { id: 'panel', online: true, boot: 7, buttonCount: 4, enabled: false, published: false, directUsb: false });
});

test('start requires a current boot catalogue, existing button and usable LED', () => {
  assert.match(dcsStartError(state, module, { ...config, button: 4 }), /outside/);
  assert.match(dcsStartError(state, { ...module, buttonCount: 0 }, config), /outside/);
  assert.match(dcsStartError(state, module, { ...config, ledActionId: 'missing' }), /Discover/);
  assert.match(dcsStartError({ ...state, moduleActions: {} }, module, config), /Discover/);
  for (const change of [{ boot: 8 }, { status: 'uncertain' }]) assert.match(dcsStartError({ ...state, moduleActions: { panel: { ...snapshot, ...change } } }, module, config), /Discover/);
  assert.equal(dcsStartError(state, module, { ...config, ledActionId: board.id }), '');
  assert.match(dcsStartError(state, module, { ...config, commandPort: NaN }), /ports/);
});

test('status and feedback labels distinguish stopped, waiting, fresh, stale and errors', () => {
  assert.equal(dcsStatusLabel(), 'Loading status…');
  assert.equal(dcsStatusLabel(stopped), 'Stopped');
  assert.equal(dcsStatusLabel({ ...stopped, error: 'broken' }), 'Needs attention');
  assert.equal(dcsStatusLabel({ ...stopped, status: 'error' }), 'Needs attention');
  assert.equal(dcsStatusLabel({ ...stopped, running: true, status: 'waiting' }), 'Waiting for DCS');
  assert.equal(dcsStatusLabel({ ...stopped, running: true, status: 'active', feedbackFresh: true }), 'Active');
  assert.equal(dcsStatusLabel({ ...stopped, running: true, status: 'active' }), 'Waiting for DCS');
  assert.equal(dcsStatusLabel({ ...stopped, running: true, status: 'stale' }), 'Feedback stale');
  assert.equal(dcsFeedbackLabel(), 'Unknown');
  assert.equal(dcsFeedbackLabel(stopped), 'Unknown');
  assert.equal(dcsFeedbackLabel({ ...stopped, running: true }), 'Waiting for feedback');
  assert.equal(dcsFeedbackLabel({ ...stopped, running: true, feedbackFresh: true, feedbackAgeMs: 25 }), 'Fresh · 25 ms');
  assert.equal(dcsFeedbackLabel({ ...stopped, running: true, feedbackAgeMs: 1501 }), 'Stale · 1501 ms');
});
