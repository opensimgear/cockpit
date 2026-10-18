import test from 'node:test';
import assert from 'node:assert/strict';
import { loadModules } from './test-module-loader.mjs';

const modules = await loadModules(['./profiles', './profile-inventory', './gateway-capabilities', './controller-lifecycle', './games', './system-inventory', './system-inventory-providers']);
const { SystemInventoryController, gatewayObservations, correlateSystem, integrationGameId } = modules.get('./system-inventory-providers');
const plain = value => JSON.parse(JSON.stringify(value));
const deferred = () => { let resolve; const promise = new Promise(done => { resolve = done; }); return { promise, resolve }; };
const storage = () => { const values = new Map(); return { getItem: key => values.get(key) ?? null, setItem: (key, value) => values.set(key, value) }; };
const source = (id = 'wheel', extra = {}) => ({ id, name: 'Controller ' + id, role: 'Joystick', backend: 'windows-raw-input', online: true, osgHid: false, baselineReady: true, controlCount: 12, identityConfidence: 'local-path', ...extra });
const usb = (key = 'serial', extra = {}) => ({ key, port: { path: 'COM7', friendlyName: 'USB panel' }, present: true, automatic: true, admissible: true, diagnostics: [], snapshot: { connected: true, port: 'COM7', identity: { moduleId: 'verified-panel', serial: 'serial', profile: 'USB panel', board: 'RP2040' }, claimedBy: null, error: null }, ...extra });
const module = (id = 'panel', extra = {}) => ({ id, name: 'CAN panel', kind: 'Button panel', online: true, active: true, enabled: true, registered: true, published: true, settings: {}, ...extra });
const gateway = (extra = {}) => ({ id: 'gateway-one', port: { backend: 'stm32', serialNumber: 'GW-1', path: 'COM2' }, present: true, busy: false, error: '', state: { backend: 'stm32', protocolVersion: 3, canProtocol: 3, lifecycle: 'automatic', connected: true, running: true, claimedModules: [], modules: [module()] }, ...extra });
const route = (id, transport = 'usb') => ({ id, transport, providerGeneration: '1', attributes: {}, baselineReady: true, writable: true, priority: 1 });
const resource = (id = 'physical-panel', connections = [], extra = {}) => ({ id, name: 'Verified physical panel', resourceType: 'osg-module', availability: 'online', generation: '1', capabilityRevision: 'cap-1', portCount: 0, cachedPortCount: 0, connections, attributes: {}, diagnostics: [], ...extra });
const connection = (scope, key, transport = 'usb') => ({ scope, key, route: route(`${scope}:${key}`, transport) });
const game = (id = 'dcs', extra = {}) => ({ id, name: 'DCS World', provider: 'dcs-bios', installed: true, launchSupported: true, integrationSetup: { state: 'ready', live: 'receiving', message: 'Ready', liveMessage: 'Telemetry observed', connection: { host: '127.0.0.1', port: 7778, secret: 'not-an-endpoint' } }, ...extra });

function harness(options = {}) {
  const state = { controllers: [], usb: [], resources: [], companions: { host: { clients: [], instances: [] } }, games: { catalogue: [] }, native: { virtualDevices: [], hiddenDevices: [], diagnostics: [] }, gateways: [], ...options };
  const requests = [], failures = new Map(), custom = new Map(), changes = [];
  const request = async input => {
    requests.push(plain(input));
    if (failures.has(input.command)) throw failures.get(input.command);
    if (custom.has(input.command)) return custom.get(input.command)(input);
    if (input.command === 'controller_list') return { controllers: { session: 'controllers-1', offset: input.offset, total: state.controllers.length, sources: state.controllers.slice(input.offset, input.offset + input.limit) } };
    if (input.command === 'module_usb_list') return { moduleUsb: { offset: input.offset, total: state.usb.length, modules: state.usb.slice(input.offset, input.offset + input.limit) } };
    if (input.command === 'profile_resources_list') return { profileResources: { session: 'inventory-1', revision: '1', offset: input.offset, total: state.resources.length, resources: state.resources.slice(input.offset, input.offset + input.limit) } };
    if (input.command === 'companion_status') return { companions: state.companions };
    if (input.command === 'game_get_state') return { games: state.games };
    if (input.command === 'system_inventory') return { systemInventory: state.native };
    if (['controller_forget', 'forget', 'companion_revoke'].includes(input.command)) return {};
    throw Error('Unexpected provider mutation: ' + input.command);
  };
  const controller = new SystemInventoryController({ request, gateways: () => state.gateways, storage: options.storage, onChange: snapshot => changes.push(snapshot) });
  return { controller, state, requests, failures, custom, changes, component: id => controller.snapshot.components.find(component => component.id === id) };
}

test('current inventory includes every category, current transport, diagnostics and unsupported integration without any profile requirement', async () => {
  const h = harness({ controllers: [source()], usb: [usb()], gateways: [gateway()], games: { catalogue: [game(), game('x-plane-12', { installed: false, provider: 'x-plane', integrationSetup: undefined })], error: 'Catalogue warning' }, companions: { host: { clients: [{ companionId: 'tablet', name: 'Tablet', kind: 'mobile', connected: true }, { companionId: 'offline', name: 'Desktop', connected: false }, { name: 'Malformed without identity' }], instances: [{ instanceId: 'panel-1', name: 'Radio', companionId: 'tablet', connected: true, armed: true, sessionId: 'session-1' }, { instanceId: 'saved', name: 'Saved panel', companionId: 'offline', connected: false, armed: false, sessionId: null }, {}] }, usb: { devices: [{ connected: true, port: 'COM10', device: { companionId: 'tablet', name: 'Same tablet', boardProfile: 'ESP32' } }, { connected: false, port: 'COM11', stage: 'Disconnected', device: { name: 'Unpaired USB companion' } }, { connected: false }] }, listener: { lastError: 'Listener warning' } }, native: { virtualDevices: [{ id: 'vjoy-1', name: 'vJoy 1', available: true, status: 'Available', createdByCockpit: true, ownerPid: 123, diagnostics: ['Virtual diagnostic'] }, { id: 'vjoy-2', name: 'vJoy 2', available: false, status: 'Missing', createdByCockpit: null, diagnostics: [] }, { id: 'vjoy-3', name: 'vJoy 3', available: null, status: 'Unknown', createdByCockpit: false, owner: 'External app', diagnostics: [] }], hiddenDevices: [{ id: 'hidden', name: 'Hidden external device', instanceId: 'HID\\external', present: null, hidden: true, configured: true, inverse: true, diagnostics: ['External rule'] }], diagnostics: ['Native inspection warning'] } });
  const snapshot = await h.controller.refresh();
  assert.deepEqual([...new Set(snapshot.components.map(component => component.type))].sort(), ['companion', 'controller', 'gateway', 'host', 'integration', 'osg', 'virtual']);
  assert.equal(h.component('controller:wheel').capture, 'ready');
  assert.equal(h.component('module:gateway-one/panel').presence, 'connected');
  assert.equal(h.component('module:gateway-one/panel').connections[0].transport, 'can');
  assert.equal(h.component('usb:serial').connections[0].transport, 'usb');
  assert.equal(h.component('virtual:vjoy-1').owner, 'PID 123');
  assert.match(h.component('virtual:vjoy-1').status, /Created by Cockpit/);
  assert.equal(h.component('virtual:vjoy-2').forget.allowed, true);
  assert.equal(h.component('virtual:vjoy-3').presence, 'unknown');
  assert.equal(h.component('virtual:vjoy-3').owner, 'External app');
  assert.equal(h.component('integration:dcs').presence, 'connected');
  assert.match(h.component('integration:dcs').connections[0].endpoint, /127\.0\.0\.1.*7778/);
  assert.doesNotMatch(h.component('integration:dcs').connections[0].endpoint, /secret/);
  assert.equal(h.component('integration:x-plane-12').presence, 'unavailable');
  assert.equal(integrationGameId(h.component('integration:dcs')), 'dcs');
  assert.equal(integrationGameId(h.component('controller:wheel')), undefined);
  assert.deepEqual(plain(h.component('companion:tablet').connections.map(edge => edge.transport).sort()), ['network', 'usb']);
  assert.equal(h.component('companion-instance:panel-1').owner, 'session-1');
  assert.equal(h.component('companion-instance:saved').presence, 'offline');
  assert.match(h.component('visibility:hidden').diagnostics.join(' '), /inverse/);
  assert.match(snapshot.diagnostics.join(' '), /Catalogue warning.*Native inspection warning|Native inspection warning.*Catalogue warning/);
  assert.equal(h.requests.some(request => /claim|apply|activate|session|arm|profile_(?!resources)/.test(request.command)), false);
});

test('providers traverse every controller, USB and verified-identity resource page', async () => {
  const h = harness({ controllers: Array.from({ length: 67 }, (_, index) => source('wheel-' + index)), usb: Array.from({ length: 35 }, (_, index) => usb('usb-' + index)), resources: Array.from({ length: 65 }, (_, index) => resource('resource-' + index)) });
  await h.controller.refresh();
  for (const [command, expected] of [['controller_list', [0, 32, 64]], ['module_usb_list', [0, 32]], ['profile_resources_list', [0, 32, 64]]]) assert.deepEqual(h.requests.filter(request => request.command === command).map(request => request.offset), expected);
  assert.equal(h.controller.snapshot.components.filter(component => component.type === 'controller').length, 67);
  assert.equal(h.controller.snapshot.components.filter(component => component.type === 'osg').length, 35);
});

test('only native verified identity combines USB, CAN and host HID; ambiguous lookalikes retain independent nodes', async () => {
  const h = harness({ controllers: [source('panel-hid', { osgHid: true }), source('same-name', { name: 'CAN panel' })], usb: [usb(), usb('ambiguous', { port: { path: 'COM8', friendlyName: 'CAN panel' } })], gateways: [gateway()], resources: [resource('physical-panel', [connection('gateway:gateway-one', 'panel', 'can'), connection('standalone-usb', 'serial'), connection('host-controllers', 'panel-hid', 'hid')]), resource('unrelated', [connection('unsupported-scope', 'ambiguous')])] });
  await h.controller.refresh();
  const grouped = h.component('resource:physical-panel');
  assert.equal(grouped.type, 'osg');
  assert.equal(grouped.name, 'CAN panel');
  assert.equal(grouped.connections.length, 3);
  assert.ok(grouped.connections.every(edge => edge.to === grouped.id));
  assert.deepEqual(plain(grouped.targets.map(target => target.kind).sort()), ['controller', 'module', 'usb']);
  assert.equal(h.component('controller:panel-hid'), undefined);
  assert.ok(h.component('controller:same-name'));
  assert.ok(h.component('usb:ambiguous'));
  assert.equal(h.component('resource:unrelated'), undefined);
});

test('hidden PnP collections augment both ordinary and OSG host controllers without duplicate visibility nodes', async () => {
  for (const osgHid of [false, true]) {
    const h = harness({ controllers: [source('hidden-controller', { online: false, osgHid })], native: { virtualDevices: [], hiddenDevices: [{ id: 'pnp', name: 'Native same controller', instanceId: 'HID\\panel', present: true, hidden: true, controllerIds: ['hidden-controller'], diagnostics: ['Matched physical PnP collection'] }], diagnostics: [] } });
    await h.controller.refresh();
    assert.equal(h.component('visibility:pnp'), undefined, `supplemental hidden node must be consumed for osgHid=${osgHid}`);
    const controller = h.component('controller:hidden-controller');
    assert.equal(controller.hidden, true);
    assert.equal(controller.presence, 'connected');
    assert.equal(controller.forget.allowed, false);
    assert.match(controller.diagnostics.join(' '), /PnP/);
    assert.equal(h.controller.snapshot.components.length, 2);
  }
});

test('a connected but unarmed real companion session is not advertised as an active input session', async () => {
  const h = harness({ companions: { host: { clients: [{ companionId: 'display', name: 'Display', connected: true }], instances: [{ instanceId: 'radio', name: 'Radio', companionId: 'display', connected: true, armed: false, sessionId: 'live-session', generation: '1' }] } } });
  await h.controller.refresh();
  const instance = h.component('companion-instance:radio');
  assert.equal(instance.presence, 'connected');
  assert.notEqual(instance.capture, 'ready');
  assert.doesNotMatch(instance.status, /input session active/i);
});

test('gateway states keep presence, input ownership, availability and conservative forget capability independent', () => {
  const entries = [gateway({ error: 'Gateway error', state: { ...gateway().state, claimedModules: ['claimed'], reconnecting: true, modules: [module('claimed', { online: false, outputOnly: true, settings: { label: 'Output label' } }), module('direct', { directUsb: true, online: false, enabled: false, published: true }), module('offline', { online: false, boot: 42 })] } }), gateway({ id: 'unavailable', present: true, busy: true, port: { backend: 'pico' }, state: null }), gateway({ id: 'missing', present: false, port: { backend: 'pico' }, state: { connected: false, modules: [module('old', { online: false })] } })];
  const observations = gatewayObservations(entries, 20);
  const get = id => observations.find(component => component.id === id);
  assert.equal(get('gateway:gateway-one').status, 'Reconnecting');
  assert.deepEqual(plain(get('gateway:gateway-one').diagnostics), ['Gateway error']);
  assert.equal(get('module:gateway-one/claimed').owner, 'Setup session');
  assert.equal(get('module:gateway-one/claimed').capture, 'unavailable');
  assert.equal(get('module:gateway-one/claimed').name, 'Output label');
  assert.equal(get('module:gateway-one/direct').owner, 'Direct USB input owner');
  assert.equal(get('module:gateway-one/direct').enabled, false);
  assert.match(get('module:gateway-one/direct').status, /local attachment unconfirmed/);
  assert.match(get('module:gateway-one/offline').status, /Neutral controller retained/);
  assert.equal(get('gateway:unavailable').presence, 'unavailable');
  assert.equal(get('gateway:unavailable').status, 'Working');
  assert.equal(get('gateway:missing').forget.allowed, true);
  assert.equal(get('module:missing/old').presence, 'offline');
  assert.equal(get('module:missing/old').connections[0].transport, 'software');
});

test('failed providers retain stale unknown records while successful empty providers prove absence', async () => {
  const h = harness({ controllers: [source()], usb: [usb()], games: { catalogue: [game()] } });
  await h.controller.refresh();
  h.failures.set('controller_list', Error('Controller service disconnected'));
  h.failures.set('system_inventory', 'Native inspection unavailable');
  h.state.usb = [];
  await h.controller.refresh();
  assert.equal(h.component('controller:wheel').presence, 'unknown');
  assert.equal(h.component('controller:wheel').stale, true);
  assert.equal(h.component('controller:wheel').connections[0].current, false);
  assert.equal(h.component('controller:wheel').forget.allowed, false);
  assert.equal(h.component('usb:serial').presence, 'offline');
  assert.equal(h.component('usb:serial').stale, false);
  assert.equal(h.component('integration:dcs').presence, 'connected');
  assert.match(h.controller.snapshot.diagnostics.join(' '), /Controller service disconnected/);
  h.failures.clear();
  await h.controller.refresh();
  assert.equal(h.component('controller:wheel').presence, 'connected');
  assert.equal(h.component('controller:wheel').stale, false);
});

test('temporarily unavailable identity evidence preserves previously proved grouping without duplicate remembered devices', async () => {
  const h = harness({ usb: [usb()], gateways: [gateway()], resources: [resource('physical-panel', [connection('standalone-usb', 'serial'), connection('gateway:gateway-one', 'panel', 'can')])] });
  await h.controller.refresh();
  h.failures.set('profile_resources_list', Error('Identity provider restarting'));
  await h.controller.refresh();
  assert.equal(h.controller.snapshot.components.filter(component => component.type === 'osg').length, 1);
  assert.equal(h.component('resource:physical-panel').presence, 'connected');
  assert.match(h.controller.snapshot.diagnostics.join(' '), /Identity provider restarting/);
});

test('partial native inspection keeps omitted devices unknown while retaining affirmative current observations', async () => {
  const virtual = id => ({ id, name: id, available: true, status: 'Available', createdByCockpit: null, diagnostics: [] });
  const hidden = id => ({ id, name: id, instanceId: 'HID\\' + id, present: true, hidden: true, diagnostics: [] });
  const h = harness({ native: { virtualDevices: [virtual('current'), virtual('omitted')], hiddenDevices: [hidden('hidden')], diagnostics: [] } });
  await h.controller.refresh();
  h.state.native = { virtualDevices: [virtual('current')], hiddenDevices: [], diagnostics: ['HidHide inspection was incomplete; permission unavailable.'] };
  await h.controller.refresh();
  assert.equal(h.component('virtual:current').presence, 'connected');
  assert.equal(h.component('virtual:omitted').presence, 'unknown');
  assert.equal(h.component('virtual:omitted').stale, true);
  assert.equal(h.component('virtual:omitted').forget.allowed, false);
  assert.equal(h.component('visibility:hidden').presence, 'unknown');
  assert.equal(h.component('visibility:hidden').connections.every(connection => !connection.current), true);
  assert.match(h.controller.snapshot.diagnostics.join(' '), /incomplete/);
  h.state.native = { virtualDevices: [virtual('current')], hiddenDevices: [], diagnostics: [] };
  await h.controller.refresh();
  assert.equal(h.component('virtual:omitted').presence, 'offline');
  assert.equal(h.component('visibility:hidden').presence, 'offline');
});

test('malformed, changed-session and truncated pages fail one provider without discarding other categories', async () => {
  const badPages = [
    request => ({ controllers: { session: 'a', offset: request.offset + 1, total: 1, sources: [source()] } }),
    request => ({ controllers: { session: 'a', offset: request.offset, total: 1, sources: [] } }),
    request => ({ controllers: { session: 'a', offset: request.offset, total: 5000, sources: [source()] } }),
    request => ({ controllers: { session: request.offset ? 'changed' : 'first', offset: request.offset, total: 2, sources: [source('at-' + request.offset)] } }),
    request => ({ controllers: { session: 'a', offset: request.offset, total: 1, sources: [source('first'), source('overflow')] } }),
  ];
  for (const bad of badPages) {
    const h = harness({ usb: [usb()] }); h.custom.set('controller_list', bad);
    await h.controller.refresh();
    assert.equal(h.controller.snapshot.components.filter(component => component.type === 'controller').length, 0);
    assert.ok(h.component('usb:serial'));
    assert.match(h.controller.snapshot.diagnostics.join(' '), /Controller inventory/);
  }
  const h = harness();
  h.custom.set('module_usb_list', request => ({ moduleUsb: { offset: request.offset, total: 2, modules: request.offset ? [] : [usb()] } }));
  await h.controller.refresh();
  assert.match(h.controller.snapshot.diagnostics.join(' '), /USB inventory/);
});

test('missing capabilities are surfaced as diagnostics instead of silently treating providers as empty', async () => {
  const h = harness();
  for (const command of ['controller_list', 'module_usb_list', 'companion_status', 'game_get_state', 'system_inventory', 'profile_resources_list']) h.custom.set(command, () => ({}));
  await h.controller.refresh();
  assert.equal(h.controller.snapshot.components.length, 1);
  assert.equal(h.controller.snapshot.diagnostics.length, 6);
  assert.match(h.controller.snapshot.diagnostics.join(' '), /Identity correlation/);
});

test('refresh coalesces concurrent reads, publishes once, and ignores late provider responses after disposal', async () => {
  const h = harness({ controllers: [source()] }), gate = deferred();
  h.custom.set('controller_list', () => gate.promise);
  const first = h.controller.refresh(), second = h.controller.refresh();
  assert.equal(first, second);
  h.controller.dispose();
  gate.resolve({ controllers: { session: 'one', offset: 0, total: 1, sources: [source()] } });
  await first;
  assert.equal(h.changes.length, 0);
  assert.equal(h.controller.snapshot.components.length, 0);
  const reads = h.requests.length;
  await h.controller.refresh();
  assert.equal(h.requests.length, reads);
});

test('late identity-facade response cannot publish a disposed inventory', async () => {
  const h = harness({ controllers: [source()] }), gate = deferred(), entered = deferred();
  h.custom.set('profile_resources_list', () => { entered.resolve(); return gate.promise; });
  const pending = h.controller.refresh();
  await entered.promise;
  h.controller.dispose();
  gate.resolve({ profileResources: { session: 's', revision: '1', offset: 0, total: 0, resources: [] } });
  await pending;
  assert.equal(h.changes.length, 0);
});

test('restart restores remembered components conservatively and a fresh observation restores current state', async () => {
  const disk = storage(), first = harness({ storage: disk, controllers: [source()] });
  await first.controller.refresh();
  const firstSeen = first.component('controller:wheel').firstSeen;
  first.controller.dispose();
  const second = harness({ storage: disk, controllers: [source()] });
  assert.equal(second.component('controller:wheel').presence, 'unknown');
  assert.equal(second.component('controller:wheel').forget.allowed, false);
  await second.controller.refresh();
  assert.equal(second.component('controller:wheel').presence, 'connected');
  assert.equal(second.component('controller:wheel').firstSeen, firstSeen);
});

test('forget refreshes presence immediately and refuses a controller that reconnected since the displayed snapshot', async () => {
  const h = harness({ controllers: [source('wheel', { online: false })] });
  await h.controller.refresh();
  assert.equal(h.component('controller:wheel').forget.allowed, true);
  h.state.controllers = [source()];
  await assert.rejects(h.controller.forget('controller:wheel'), /connected|Disconnect/i);
  assert.equal(h.requests.some(request => request.command === 'controller_forget'), false);
});

test('eligible forget dispatches only its provider action and durable tombstones suppress stale rediscovery', async () => {
  for (const [state, id, command] of [
    [{ controllers: [source('wheel', { online: false })] }, 'controller:wheel', 'controller_forget'],
    [{ companions: { host: { clients: [{ companionId: 'tablet', name: 'Tablet', connected: false }], instances: [] } } }, 'companion:tablet', 'companion_revoke'],
    [{ gateways: [gateway({ state: { ...gateway().state, modules: [module('panel', { online: false })] } })] }, 'module:gateway-one/panel', 'forget'],
  ]) {
    const h = harness({ ...state, storage: storage() });
    await h.controller.refresh();
    await h.controller.forget(id);
    assert.equal(h.component(id), undefined);
    assert.equal(h.requests.filter(request => request.command === command).length, 1);
    await h.controller.refresh();
    assert.equal(h.component(id), undefined, 'unchanged offline provider record must remain forgotten');
  }
});

test('local-only forget leaves unavailable providers untouched and failed native forget retains the visible record', async () => {
  const h = harness({ usb: [usb('old-usb', { present: false, snapshot: { connected: false, identity: null, claimedBy: null, error: 'Removed' } })] });
  await h.controller.refresh();
  await h.controller.forget('usb:old-usb');
  assert.equal(h.component('usb:old-usb'), undefined);
  assert.equal(h.requests.some(request => /forget|revoke/.test(request.command)), false);
  const failure = harness({ controllers: [source('wheel', { online: false })] });
  failure.failures.set('controller_forget', Error('Native removal refused'));
  await assert.rejects(failure.controller.forget('controller:wheel'), /Native removal refused/);
  assert.ok(failure.component('controller:wheel'));
});

test('identity grouping combines independent hidden, ownership and readiness evidence without weakening removal safety', () => {
  const sample = gatewayObservations([gateway({ state: { ...gateway().state, modules: [module('panel', { online: false })] } })], 1).find(component => component.type === 'osg');
  const grouped = correlateSystem([sample, { ...sample, id: 'usb:serial', targets: [{ kind: 'usb', id: 'serial' }], presence: 'connected', owner: 'External writer', hidden: true, capture: 'ready', evidence: ['USB proof'], diagnostics: ['USB diagnostic'] }], [resource('same', [connection('gateway:gateway-one', 'panel'), connection('standalone-usb', 'serial')])]);
  assert.equal(grouped.length, 1);
  assert.equal(grouped[0].hidden, true);
  assert.equal(grouped[0].owner, 'External writer');
  assert.equal(grouped[0].capture, 'ready');
  assert.equal(grouped[0].forget.allowed, false);
});
