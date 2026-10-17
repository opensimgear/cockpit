import assert from 'node:assert/strict';
import test from 'node:test';
import { ModuleUsbClient, usbRoute, usbRoutesEqual } from './module-usb.ts';

const clone = value => JSON.parse(JSON.stringify(value));
const serial = '00112233445566778899aabb';
const key = `${serial}:COM8`;
const route = () => ({ moduleId: `4f534744${serial}`, serial, connectionGeneration: 9007199254740990, boot: 100, configurationGeneration: 1 });
const config = () => ({ name: 'Panel with spaces', expanderMask: 0, buttons: [[0, 0, 0]], axes: [], hats: [], encoders: [], selectors: [] });
const runtime = () => ({ generation: 1, savedGeneration: 1, owner: 0, claimExpiresMs: 0, dirty: false, pendingApply: false, valid: true, view: 'active', config: config(), layout: { buttons: 1, axes: 0, hats: 0, reportBytes: 1 }, descriptorHex: '0501' });
const descriptor = () => ({ id: 'led.boolean.set_enabled', label: 'LED', endpoint: 'led.boolean', semantics: 'setter', readable: true, parameters: [{ id: 'enabled', label: 'Enabled', type: 'boolean' }] });
const module = () => ({ key, port: { path: 'COM8', serialNumber: serial, vendorId: '1209', productId: '0001', manufacturer: '(Standard port types)', friendlyName: 'USB Serial Device (COM8)' }, present: true, automatic: false, admissible: true, diagnostics: [], snapshot: {
  connected: true, port: 'COM8', identity: { model: 'device-template', board: '0.1.0', profile: config().name, moduleId: route().moduleId, serial, protocol: 3, buttonCount: 1, axisCount: 0, hatCount: 0 }, route: route(),
  config: runtime(), draft: null, actions: { boot: 100, route: route(), status: 'ready', catalogueRevision: 1, actions: [descriptor()], states: [{ id: descriptor().id, values: [false] }], ownership: null, error: null },
  events: { ok: true, schemaVersion: 1, boot: 100, route: route(), stream: 1, head: 10, oldest: 1, overflow: 0, buttonCount: 1, held: [false], encoderButtons: [] },
  claimedBy: null, observedAtMs: 1792200000000, error: null,
} });
const selection = () => ({ key, port: module().port, present: true, automatic: false, admissible: true, connected: true, diagnostics: [] });
const compact = () => { const result = module(); for (const field of ['config', 'draft', 'actions', 'events']) delete result.snapshot[field]; return result; };
const successful = calls => new ModuleUsbClient(async request => { calls.push(clone(request)); return { module: module() }; });

test('enumeration and paged summaries preserve generic Windows metadata without connecting or claiming', async () => {
  const calls = [], ports = { ports: [selection()] }, summaries = { offset: 0, total: 2, modules: [compact()] };
  const client = new ModuleUsbClient(async request => { calls.push(request); return request.command === 'module_usb_ports' ? ports : summaries; });
  const observed = await client.ports(), listed = await client.list(0, 2);
  assert.equal(observed.ports[0].automatic, false);
  assert.equal(observed.ports[0].port.friendlyName, 'USB Serial Device (COM8)');
  assert.equal(listed.total, 2); assert.equal(listed.modules.length, 1);
  assert.equal(listed.modules[0].snapshot.config, undefined);
  assert.deepEqual(calls, [{ command: 'module_usb_ports' }, { command: 'module_usb_list', offset: 0, limit: 2 }]);
  ports.ports[0].port.path = 'COM99';
  assert.equal(observed.ports[0].port.path, 'COM8');
  assert.ok(Object.isFrozen(listed.modules[0].snapshot.route));
});

test('Connect is an explicit observed key/path selection followed by native role and route verification', async () => {
  const calls = [], client = successful(calls);
  const connected = await client.connect(selection());
  assert.deepEqual(calls, [{ command: 'module_usb_connect', key, port: 'COM8' }]);
  assert.equal(connected.snapshot.route.connectionGeneration, 9007199254740990);
  assert.ok(Object.isFrozen(connected.snapshot.config.config));
  for (const change of [{ present: false }, { admissible: false }, { key: '' }, { port: { path: '' } }]) await assert.rejects(client.connect({ ...selection(), ...change }), /observed unambiguous/);
  assert.equal(calls.length, 1);
  const rejected = new ModuleUsbClient(async () => { throw Error('USB role/UID handshake did not match'); });
  await assert.rejects(rejected.connect(selection()), /handshake/);
});

test('configuration operations retain exact routes and draft/active separation without implicit claims or Apply', async () => {
  const calls = [], client = successful(calls), selected = module();
  await client.status(key);
  await client.readConfig(selected);
  await client.claim(selected);
  await client.apply(selected, config());
  await client.apply(selected, config(), true);
  await client.save(selected); await client.discard(selected); await client.reset(selected); await client.release(selected);
  assert.deepEqual(calls.map(request => request.command), ['module_usb_status', 'module_usb_config_read', 'module_usb_claim', 'module_usb_config_apply', 'module_usb_config_apply', 'module_usb_config_save', 'module_usb_config_discard', 'module_usb_config_reset', 'module_usb_release']);
  for (const request of calls.slice(1)) { assert.equal(request.key, key); assert.deepEqual(request.route, route()); }
  assert.equal(calls[3].save, false); assert.equal(calls[4].save, true);
  assert.equal(selected.snapshot.draft, null);
  const draft = module(); draft.snapshot.draft = { ...runtime(), view: 'draft', dirty: true, valid: false, layout: null, descriptorHex: '' };
  const read = await new ModuleUsbClient(async () => ({ module: draft })).status(key);
  assert.equal(read.snapshot.config.view, 'active'); assert.equal(read.snapshot.draft.view, 'draft'); assert.equal(read.snapshot.draft.valid, false);
  const offline = module(); offline.snapshot.connected = false; offline.snapshot.route = null; offline.snapshot.events = null;
  const stopped = await new ModuleUsbClient(async () => ({ module: offline })).disconnect(key);
  assert.equal(stopped.snapshot.connected, false); assert.equal(stopped.snapshot.identity.moduleId, route().moduleId);
});

test('route changes require a fresh native snapshot and never silently repair or retry a write', async () => {
  let current = module(); const calls = [];
  const client = new ModuleUsbClient(async request => {
    calls.push(clone(request));
    if ('route' in request && !usbRoutesEqual(request.route, current.snapshot.route)) throw Error('USB route changed; establish a fresh session');
    return { module: clone(current) };
  });
  const original = await client.status(key);
  current.snapshot.route.connectionGeneration++;
  await assert.rejects(client.apply(original, config()), /route changed/);
  assert.equal(calls.filter(request => request.command === 'module_usb_config_apply').length, 1);
  const fresh = await client.status(key);
  await client.readConfig(fresh);
  assert.equal(original.snapshot.route.connectionGeneration, 9007199254740990);
  assert.equal(calls.at(-1).route.connectionGeneration, 9007199254740991);
});

test('lost or malformed mutation replies stay unknown and send exactly once', async () => {
  const calls = [], client = new ModuleUsbClient(async request => { calls.push(request); throw Error('The action reply was lost. The output may have changed; refresh its state before invoking again.'); });
  await assert.rejects(client.invoke(module(), descriptor().id, [true]), /may have changed/);
  await assert.rejects(client.apply(module(), config(), true), /reply was lost/);
  assert.deepEqual(calls.map(request => request.command), ['module_usb_actions_invoke', 'module_usb_config_apply']);
  const malformed = new ModuleUsbClient(async () => ({ module: { ...module(), key: 'other-endpoint' } }));
  await assert.rejects(malformed.save(module()), /result is unknown/);
  const missing = new ModuleUsbClient(async () => ({}));
  await assert.rejects(missing.status(key), /unknown/);
});

test('configuration is checked before JSON cloning and does not change a captured status', async () => {
  const calls = [], client = successful(calls), original = module();
  for (const changed of [{ expanderMask: 256 }, { buttons: [[NaN, 0, 0]] }, { name: '' }, { axes: [[0, 48, 20, 10, 0, 0, 0]] }, { hats: [[0, 0, 1, 2]] }]) await assert.rejects(client.apply(original, { ...config(), ...changed }));
  assert.equal(calls.length, 0);
  assert.deepEqual(original.snapshot.config.config, config());
});

test('action invocation preserves all atomic arguments, display limits and the native catalogue revision', async () => {
  const calls = [], client = successful(calls);
  await client.describeActions(module()); await client.readActions(module());
  const selected = module(); selected.snapshot.actions.actions = [{ ...descriptor(), parameters: [
    { id: 'on', label: 'On', type: 'boolean' }, { id: 'index', label: 'Index', type: 'integer', minimum: -1, maximum: 3 }, { id: 'mode', label: 'Mode', type: 'enum', options: ['auto', 'off'] }, { id: 'color', label: 'Color', type: 'rgb' },
  ] }];
  selected.snapshot.actions.states = [];
  await client.invoke(selected, descriptor().id, [true, 3, 'off', 'Ab0011']);
  assert.deepEqual(calls.at(-1), { command: 'module_usb_actions_invoke', key, route: route(), catalogueRevision: 1, actionId: descriptor().id, values: [true, 3, 'off', 'Ab0011'] });
  for (const values of [[true], ['true', 3, 'off', 'Ab0011'], [true, 4, 'off', 'Ab0011'], [true, 0.5, 'off', 'Ab0011'], [true, 3, 'unknown', 'Ab0011'], [true, 3, 'off', 'bad']]) await assert.rejects(client.invoke(selected, descriptor().id, values), /exact values/);
  const text = module(); text.snapshot.actions.actions = [{ ...descriptor(), parameters: [{ id: 'text', label: 'Text', type: 'string', maxBytes: 8 }], display: { columns: 4, rows: 1, encoding: 'ascii' } }];
  text.snapshot.actions.states = [];
  for (const value of ['', 'ABCD']) await client.invoke(text, descriptor().id, [value]);
  for (const value of ['ABCDE', 'é', '\n', 'x'.repeat(9)]) await assert.rejects(client.invoke(text, descriptor().id, [value]), /exact values/);
  for (const change of [{ status: 'uncertain' }, { status: 'error' }, { status: 'unsupported' }, { route: { ...route(), boot: 101 } }]) await assert.rejects(client.invoke({ ...module(), snapshot: { ...module().snapshot, actions: { ...module().snapshot.actions, ...change } } }, descriptor().id, [true]));
  await assert.rejects(client.invoke(module(), 'not-advertised', []), /exact values/);
});

test('unsupported firmware is explicit and event paging follows next rather than replaying history or inventing hats', async () => {
  const calls = [], client = successful(calls), selected = module();
  await client.eventsState(selected);
  await client.eventsRead(selected, 1, 10);
  const partial = module(); partial.snapshot.events = { ...partial.snapshot.events, head: 30, after: 10, next: 26, more: true, events: Array.from({ length: 16 }, (_, index) => ({ sequence: 11 + index, control: 0, pressed: index % 2 === 0 })) };
  await client.eventsRead(partial, 1, 26);
  assert.equal(calls.at(-1).after, 26); assert.equal(selected.snapshot.events.hats, undefined);
  for (const [stream, after] of [[2, 10], [1, 0], [1, -1], [0, 10]]) await assert.rejects(client.eventsRead(selected, stream, after), /baseline/);
  const unsupported = module(); unsupported.snapshot.events = { boot: 100, route: route(), status: 'unsupported' }; unsupported.snapshot.actions = { boot: 100, route: route(), status: 'unsupported' };
  const read = await new ModuleUsbClient(async () => ({ module: unsupported })).status(key);
  assert.equal(read.snapshot.events.status, 'unsupported'); assert.equal(read.snapshot.events.held, undefined);
  assert.equal(read.snapshot.actions.actions, undefined);
  await assert.rejects(client.eventsRead(read, 1, 10), /baseline/);
});

test('output-only modules and current native axis metadata stay usable without fabricated HID controls', async () => {
  const output = module();
  output.snapshot.identity.buttonCount = 0;
  output.snapshot.config.config.buttons = [];
  output.snapshot.config.layout = { buttons: 0, axes: 0, hats: 0, reportBytes: 0 };
  output.snapshot.config.descriptorHex = '';
  output.snapshot.events = { ...output.snapshot.events, buttonCount: 0, held: [] };
  const client = new ModuleUsbClient(async () => ({ module: output }));
  const current = await client.status(key);
  assert.equal(usbRoute(current).moduleId, route().moduleId);
  await client.invoke(current, descriptor().id, [true]);
  const axis = module(); axis.snapshot.events = { ...axis.snapshot.events, contactButtonCount: 1, maintainedButtons: [], selectorButtons: [], axisCount: 1, axes: [65535], axisNeutral: [32767], axisUsage: [48], axisAvailable: [true], sampledAtMs: 123 };
  const inspected = await new ModuleUsbClient(async () => ({ module: axis })).status(key);
  assert.deepEqual(inspected.snapshot.events.axes, [65535]);
  assert.deepEqual(inspected.snapshot.events.axisNeutral, [32767]);
  assert.equal(inspected.snapshot.events.hats, undefined);
  const missing = module(); missing.snapshot.connected = false; missing.snapshot.route = null;
  await assert.rejects(client.claim(missing), /Refresh and connect/);
});

test('every USB ownership epoch and safe counter is retained exactly', () => {
  assert.equal(usbRoutesEqual(route(), clone(route())), true);
  for (const field of Object.keys(route())) assert.equal(usbRoutesEqual(route(), { ...route(), [field]: typeof route()[field] === 'number' ? route()[field] + 1 : 'different' }), false);
  for (const generation of [NaN, Infinity, 1.5, 0, 9007199254740992]) {
    const selected = module(); selected.snapshot.route.connectionGeneration = generation;
    assert.throws(() => usbRoute(selected), /Refresh and connect/);
  }
  for (const change of [{ present: false }, { admissible: false }]) assert.throws(() => usbRoute({ ...module(), ...change }), /Refresh and connect/);
  const selected = module(); selected.snapshot.config.generation = 2;
  assert.throws(() => usbRoute(selected), /Refresh and connect/);
});

test('malformed inventories and capabilities fail without manufacturing a current route', async () => {
  for (const result of [{}, { ports: [{ ...selection(), present: false }] }, { ports: [{ ...selection(), port: null }] }, { ports: [selection(), selection()] }]) await assert.rejects(new ModuleUsbClient(async () => result).ports(), /Malformed/);
  for (const result of [{ offset: 0, total: 1, modules: [] }, { offset: 1, total: 1, modules: [compact()] }, { offset: 0, total: 129, modules: [compact()] }, { offset: 0, total: 2, modules: [compact(), compact()] }]) await assert.rejects(new ModuleUsbClient(async () => result).list(), /Malformed/);
  const blocked = new ModuleUsbClient(async () => assert.fail('Invalid request reached native'));
  for (const [offset, limit] of [[-1, 16], [129, 16], [0, 0], [0, 33]]) await assert.rejects(blocked.list(offset, limit), /bounded/);
  for (const edit of [value => { value.snapshot.identity.protocol = 4; }, value => { value.snapshot.route.moduleId = 'not-the-physical-device'; }, value => { value.snapshot.actions.actions[0].parameters[0].type = 'opaque'; }, value => { value.snapshot.events.held = []; }, value => { value.snapshot.events.axisCount = 1; }, value => { value.snapshot.config.generation = 0; }]) {
    const invalid = module(); edit(invalid);
    await assert.rejects(new ModuleUsbClient(async () => ({ module: invalid })).status(key), /unknown/);
  }
});

test('USB action captures preserve optional guarded restoration and reject invented policies', async () => {
  for(const ownership of [null,{schemaVersion:1,scope:'endpoint',maxLeaseMs:5000},{schemaVersion:1,scope:'endpoint',maxLeaseMs:5000,restorePolicy:'if-unchanged'}]) {
    const supplied=module();supplied.snapshot.actions.ownership=ownership;
    const captured=await new ModuleUsbClient(async()=>({module:supplied})).status(key);
    assert.deepEqual(captured.snapshot.actions.ownership,ownership);
  }
  for(const restorePolicy of [null,'always',true,1]) {
    const supplied=module();supplied.snapshot.actions.ownership={schemaVersion:1,scope:'endpoint',maxLeaseMs:5000,restorePolicy};
    await assert.rejects(new ModuleUsbClient(async()=>({module:supplied})).status(key),/unknown/);
  }
});
