import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import test from 'node:test';
import vm from 'node:vm';
import ts from 'typescript';

const compile = async name => ts.transpileModule(await readFile(new URL(name, import.meta.url), 'utf8'), {
  compilerOptions: { module: ts.ModuleKind.CommonJS, target: ts.ScriptTarget.ES2022 },
}).outputText;
const capabilities = {};
vm.runInNewContext(await compile('./gateway-capabilities.ts'), { exports: capabilities });
const requireCapabilities = name => { if (name.replace(/\.ts$/,'') === './gateway-capabilities') return capabilities; throw new Error(`Unexpected import ${name}`); };
const actions = {};
vm.runInNewContext(await compile('./device-actions.ts'), { exports: actions, require: requireCapabilities });
const automation = {};
vm.runInNewContext(await compile('./gateway-automation.ts'), { exports: automation, require: requireCapabilities });
class GatewayError extends Error { constructor(message, state) { super(message); this.state = state; } }
const exported = {};
vm.runInNewContext(await compile('./gateway-workspace.ts'), {
  exports: exported, require(name) {
    name=name.replace(/\.ts$/,'');
    if (name === './gateway-capabilities') return capabilities;
    if (name === './gateway-automation') return automation;
    if (name === './device-actions') return actions;
    if (name === './gateway') return { GatewayError, gatewayRequest: () => { throw new Error('A fake transport is required.'); } };
    throw new Error(`Unexpected import ${name}`);
  },
});
const { GatewayWorkspace, gatewayIdentity } = exported;
const plain = value => JSON.parse(JSON.stringify(value));
const tick = () => new Promise(resolve => setImmediate(resolve));
const deferred = () => { let resolve; const promise = new Promise(done => { resolve = done; }); return { promise, resolve }; };
const firstPort = { path: 'tcp:127.0.0.1:1234/00112233445566778899aabb', manufacturer: 'OpenSimGear', friendlyName: 'OpenSimGear CAN gateway', serialNumber: '00112233445566778899aabb', backend: 'stm32' };
const secondPort = { ...firstPort, path: 'tcp:127.0.0.1:1235/00112233445566778899aacc', serialNumber: '00112233445566778899aacc' };
const first = gatewayIdentity(firstPort), second = gatewayIdentity(secondPort);
const module = { id: 'same-module-id', online: true, active: true, registered: true, canConfigure: false, buttons: [1], axes: [42000], hats: [2] };
function state(port) { return { gatewayId: gatewayIdentity(port), backend: port.backend, gatewaySerial: port.serialNumber,
  port: port.path, connected: false, running: false, needsReconnect: false, modules: [{ ...module }], claimedModules: [] }; }
function fixture() {
  const f = { ports: [firstPort, secondPort], states: new Map([[first, state(firstPort)], [second, state(secondPort)]]),
    calls: [], changes: [], now: 0, intercept: undefined };
  f.transport = async request => {
    f.calls.push(plain(request));
    if (f.intercept) { const result = await f.intercept(request); if (result !== undefined) return result; }
    if (request.command === 'ports') return { connected: false, running: false, needsReconnect: false, modules: [], ports: f.ports };
    const next = plain(f.states.get(request.gatewayId));
    assert.ok(next, `Unknown scope ${request.gatewayId}`);
    if (request.command === 'connect') { next.connected = true; next.port = request.port; }
    if (request.command === 'disconnect') { next.connected = false; next.running = false; }
    if (request.command === 'claim') next.claimedModules = [request.moduleId];
    if (request.command === 'release') next.claimedModules = [];
    if (request.command === 'start') next.running = true;
    if (request.command === 'config_read') next.moduleConfigs = { ...next.moduleConfigs, [request.moduleId]: { config: { name: request.gatewayId }, generation: 1 } };
    f.states.set(request.gatewayId, next);
    return next;
  };
  f.workspace = new GatewayWorkspace({ request: f.transport, now: () => f.now, onChange: (entries, error) => f.changes.push({ entries, error }) });
  f.entry = id => f.workspace.entries.find(entry => entry.id === id);
  return f;
}

test('explicit disconnect suspends automatic reconnect for only that gateway until explicit connect', async () => {
  const f=fixture(); await f.workspace.refresh();
  await f.workspace.disconnect(first);
  assert.equal(f.entry(first).autoConnectPaused,true); assert.equal(f.entry(first).state.connected,false);
  f.calls.length=0; await f.workspace.refresh();
  assert.equal(f.calls.some(c=>c.gatewayId===first),false); assert.ok(f.calls.some(c=>c.gatewayId===second));
  await f.workspace.retry(first); assert.equal(f.entry(first).autoConnectPaused,false); assert.equal(f.entry(first).state.connected,true);
});

test('background test readbacks remain gateway-ordered without blocking the setup UI with busy state', async () => {
  const f = fixture(); await f.workspace.refresh();
  const waiting = deferred();
  f.intercept = request => request.command === 'events_state' ? waiting.promise : undefined;
  const read = f.workspace.request(first, { command: 'events_state', moduleId: module.id }, true);
  await tick();
  assert.equal(f.entry(first).busy, false);
  const action = f.workspace.request(first, { command: 'debug_invoke', moduleId: module.id, debugOperation: 'axis', debugArgs: [0,4095] });
  assert.equal(f.entry(first).busy, true);
  assert.equal(f.calls.at(-1).command, 'events_state');
  waiting.resolve(f.states.get(first)); await Promise.all([read,action]); await tick();
  assert.equal(f.calls.at(-1).command, 'debug_invoke'); assert.equal(f.entry(first).busy, false);
});

test('Pico device-contract firmware receives runtime hydration without legacy start', async () => {
  const f = fixture(), serialNumber = 'OSG-PICO-0123456789ABCDEF';
  const port = { path: '/dev/cu.usbmodemDebug', serialNumber, backend: 'pico', vendorId: 'cafe', productId: '4010' };
  const id = gatewayIdentity(port);
  f.ports = [port];
  f.states.set(id, { ...state(port), protocolVersion: 3, canProtocol: 3, lifecycle: 'automatic', modules: [{ ...module, canConfigure: true }] });
  await f.workspace.refresh();
  assert.ok(f.calls.some(request => request.gatewayId === id && request.command === 'config_read'));
  assert.ok(!f.calls.some(request => request.command === 'start'));
});

test('two same-backend gateways both connect and retain separate instances of the same module ID', async () => {
  const f = fixture();
  f.states.get(first).modules[0].name = 'First pedals';
  f.states.get(second).modules[0].name = 'Second pedals';
  await f.workspace.refresh();
  assert.equal(f.workspace.entries.length, 2);
  assert.ok(f.workspace.entries.every(entry => entry.state.connected));
  assert.equal(f.entry(first).state.modules[0].name, 'First pedals');
  assert.equal(f.entry(second).state.modules[0].name, 'Second pedals');
  assert.deepEqual(f.calls.filter(call => call.command === 'connect').map(call => call.gatewayId).sort(), [first, second]);
  assert.ok(!f.calls.some(call => call.command === 'disconnect'));
});

test('identity survives alias and endpoint changes, without creating another connection', async () => {
  const f = fixture();
  assert.equal(gatewayIdentity({ ...firstPort, serialNumber: firstPort.serialNumber.toUpperCase() }), first);
  f.ports = [{ ...firstPort, path: '/dev/tty.usbmodemOSG' }, { ...firstPort, path: '/dev/cu.usbmodemOSG' }];
  await f.workspace.refresh();
  assert.equal(f.workspace.entries.length, 1);
  assert.equal(f.entry(first).port.path, '/dev/cu.usbmodemOSG');
  f.ports = [{ ...firstPort, path: 'tcp:127.0.0.1:9876/00112233445566778899aabb' }];
  await f.workspace.refresh();
  assert.equal(f.workspace.entries.length, 1);
  assert.equal(f.entry(first).port.path, f.ports[0].path);
  assert.equal(f.calls.filter(call => call.command === 'connect').length, 1);
});

test('claims, saves, and releases route to an exact gateway despite identical module IDs', async () => {
  const f = fixture(); await f.workspace.refresh();
  await f.workspace.request(second, { command: 'claim', moduleId: module.id });
  assert.deepEqual(plain(f.entry(second).state.claimedModules), [module.id]);
  assert.deepEqual(plain(f.entry(first).state.claimedModules), []);
  await f.workspace.request(first, { command: 'config_apply_save', moduleId: module.id, config: { name: 'First only' } });
  await f.workspace.request(second, { command: 'release', moduleId: module.id });
  assert.deepEqual(f.calls.filter(call => ['claim', 'config_apply_save', 'release'].includes(call.command)).map(call => [call.gatewayId, call.command]),
    [[second, 'claim'], [first, 'config_apply_save'], [second, 'release']]);
  await assert.rejects(f.workspace.request(first, { command: 'claim', gatewayId: second, moduleId: module.id }), /different gateway/);
});

test('a slow gateway does not block later refreshes or actions for another gateway', async () => {
  const f = fixture(); await f.workspace.refresh();
  const blocked = deferred();
  f.intercept = request => request.gatewayId === first && request.command === 'status' ? blocked.promise : undefined;
  const slowRefresh = f.workspace.refresh(); await tick();
  const firstPolls = f.calls.filter(call => call.gatewayId === first && call.command === 'status').length;
  await f.workspace.refresh();
  await f.workspace.request(second, { command: 'claim', moduleId: module.id });
  assert.equal(f.calls.filter(call => call.gatewayId === first && call.command === 'status').length, firstPolls);
  assert.ok(f.calls.filter(call => call.gatewayId === second && call.command === 'status').length >= 2);
  assert.equal(f.entry(second).state.claimedModules[0], module.id);
  blocked.resolve(f.states.get(first)); await slowRefresh;
});

test('an action waits for its own poll, blocks further same-gateway polls, and keeps busy until finished', async () => {
  const f = fixture(); await f.workspace.refresh();
  const blocked = deferred();
  f.intercept = request => request.gatewayId === first && request.command === 'status' ? blocked.promise : undefined;
  const poll = f.workspace.refresh(); await tick();
  const claim = f.workspace.request(first, { command: 'claim', moduleId: module.id });
  assert.equal(f.entry(first).busy, true);
  await f.workspace.refresh();
  assert.equal(f.calls.some(call => call.command === 'claim'), false);
  blocked.resolve(f.states.get(first));
  await Promise.all([poll, claim]); await tick();
  assert.equal(f.entry(first).busy, false);
  assert.equal(f.entry(first).state.claimedModules[0], module.id);
  assert.equal(f.calls.filter(call => call.gatewayId === first && call.command === 'status').length, 1);
});

test('a failed connection backs off individually, while retry restores it immediately', async () => {
  const f = fixture(); let fail = true;
  f.intercept = request => { if (fail && request.gatewayId === first) throw new Error('First gateway unavailable.'); };
  await f.workspace.refresh();
  assert.equal(f.entry(first).state, null);
  assert.equal(f.entry(second).state.connected, true);
  assert.match(f.entry(first).error, /First gateway unavailable/);
  await f.workspace.refresh();
  assert.equal(f.calls.filter(call => call.gatewayId === first).length, 1);
  assert.ok(f.calls.some(call => call.gatewayId === second && call.command === 'status'));
  fail = false; await f.workspace.retry(first);
  assert.equal(f.entry(first).state.connected, true);
  assert.equal(f.entry(first).error, '');
});

test('transport failure clears only that gateway live inputs and preserves its last-known registration', async () => {
  const f = fixture(); await f.workspace.refresh();
  f.ports = [secondPort];
  f.intercept = request => { if (request.gatewayId === first) throw new Error('Closed socket.'); };
  await f.workspace.refresh();
  assert.equal(f.entry(first).present, false);
  assert.equal(f.entry(first).state.connected, false);
  assert.equal(f.entry(first).state.modules[0].online, false);
  assert.equal(f.entry(first).state.modules[0].registered, true);
  assert.deepEqual(plain(f.entry(first).state.modules[0].axes), []);
  assert.equal(f.entry(second).state.connected, true);
  assert.deepEqual(plain(f.entry(second).state.modules[0].axes), [42000]);
  f.intercept = undefined; f.ports = [firstPort, secondPort];
  await f.workspace.refresh();
  assert.equal(f.entry(first).state.connected, true);
});

test('new configuration is hydrated independently, while a claim blocks hydration only on its own gateway', async () => {
  const f = fixture();
  for (const state of f.states.values()) state.modules[0].canConfigure = true;
  f.states.get(first).claimedModules = [module.id];
  await f.workspace.refresh();
  assert.equal(f.entry(first).state.moduleConfigs, undefined);
  assert.equal(f.entry(second).state.moduleConfigs[module.id].config.name, second);
  await f.workspace.request(first, { command: 'release', moduleId: module.id });
  await f.workspace.refresh();
  assert.equal(f.entry(first).state.moduleConfigs[module.id].config.name, first);
  assert.equal(f.entry(second).state.moduleConfigs[module.id].config.name, second);
});

test('a command rejection reconciles only its own returned state and preserves another gateway setup', async () => {
  const f = fixture(); await f.workspace.refresh();
  await f.workspace.request(second, { command: 'claim', moduleId: module.id });
  f.intercept = request => { if (request.command === 'claim' && request.gatewayId === first)
    throw new GatewayError('Another client owns setup.', { ...f.states.get(first), claimedModules: [] }); };
  await assert.rejects(f.workspace.request(first, { command: 'claim', moduleId: module.id }), /Another client owns setup/);
  assert.equal(f.entry(first).state.connected, true);
  assert.match(f.entry(first).error, /Another client/);
  assert.equal(f.entry(second).state.claimedModules[0], module.id);
});

test('a response from another scope never replaces the requested gateway state', async () => {
  const f = fixture(); await f.workspace.refresh();
  f.intercept = request => request.gatewayId === first ? f.states.get(second) : undefined;
  await assert.rejects(f.workspace.request(first, { command: 'status' }), /different gateway/);
  assert.equal(f.entry(first).state.gatewayId, first);
  assert.equal(f.entry(first).state.connected, false);
  assert.equal(f.entry(second).state.connected, true);
});

test('inventory failure preserves existing connections and reports a separate discovery error', async () => {
  const f = fixture(); await f.workspace.refresh();
  f.intercept = request => { if (request.command === 'ports') throw new Error('Enumeration unavailable.'); };
  await f.workspace.refresh();
  assert.equal(f.workspace.entries.length, 2);
  assert.ok(f.workspace.entries.every(entry => entry.state.connected && !entry.error));
  assert.match(f.workspace.inventoryError, /Enumeration unavailable/);
  assert.notEqual(f.changes[0].entries, f.changes.at(-1).entries);
  assert.notEqual(f.changes.at(-2).entries[0].state, f.changes.at(-1).entries[0].state);
});

test('dispose prevents late transport completions from publishing UI updates', async () => {
  const f = fixture(); const blocked = deferred();
  f.intercept = request => request.command === 'ports' ? blocked.promise : undefined;
  const refresh = f.workspace.refresh();
  f.workspace.dispose(); const count = f.changes.length;
  blocked.resolve({ ports: f.ports }); await refresh;
  assert.equal(f.changes.length, count);
  assert.equal(f.workspace.entries.length, 0);
});

test('automatic controller publication is observed without client registration or Apply and disabled devices remain configurable', async () => {
  const f = fixture();
  for (const current of f.states.values()) {
    Object.assign(current, { protocolVersion: 3, lifecycle: 'automatic', running: true, layoutPending: false });
    Object.assign(current.modules[0], { enabled: false, published: false, descriptorIndex: -1, canConfigure: true });
  }
  await f.workspace.refresh();
  assert.equal(f.entry(first).state.modules[0].enabled, false);
  assert.ok(f.entry(first).state.moduleConfigs[module.id]);
  assert.ok(f.calls.some(call => call.command === 'config_read' && call.gatewayId === first));
  assert.ok(f.calls.every(call => !['register', 'apply', 'reconnect', 'enable'].includes(call.command)));
  const current = f.states.get(first);
  current.modules.push({ ...current.modules[0], id: 'new-device', enabled: true, published: true, descriptorIndex: 0 });
  await f.workspace.refresh();
  assert.equal(f.entry(first).state.modules.find(item => item.id === 'new-device').enabled, true);
  assert.equal(f.entry(first).state.modules.find(item => item.id === module.id).enabled, false);
});
