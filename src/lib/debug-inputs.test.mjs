import assert from 'node:assert/strict';
import test from 'node:test';
import { loadModules } from './test-module-loader.mjs';
const modules = await loadModules(['./gateway-capabilities', './debug-inputs']);
const { supportsDeviceContracts } = modules.get('./gateway-capabilities');
const { DebugInputQueue, debugReady, debugDefinition, rawAxisValue, debugPresetLabels } = modules.get('./debug-inputs');
const plain = value => JSON.parse(JSON.stringify(value));
const turn = () => new Promise(resolve => setImmediate(resolve));
const deferred = () => { let done; const promise = new Promise(resolve => { done = resolve; }); return { promise, done }; };
const definition = { name: 'Panel', buttons: [[0,0,0],[1,1,0]], axes: [[0,48,0,4095,0,0,0]], hats: [], encoders: [], selectors: [], expanderMask: 0 };
const device = { id: 'panel', boot: 9, online: true, definition };
const module = { id: 'panel', boot: 9, online: true, enabled: false, published: false };
const state = { backend: 'pico', protocolVersion: 3, canProtocol: 3, connected: true, debug: { devices: [device] } };

test('only negotiated device contracts enable the production path on Pico', () => {
  assert.equal(supportsDeviceContracts(state), true);
  assert.equal(supportsDeviceContracts({ backend: 'stm32' }), true);
  for (const change of [{ protocolVersion: 1 }, { canProtocol: 2 }, { backend: 'unknown' }]) assert.equal(supportsDeviceContracts({ ...state, ...change }), false);
  assert.equal(supportsDeviceContracts(null), false); assert.equal(supportsDeviceContracts(undefined), false);
  assert.ok(debugPresetLabels.radio); assert.ok(debugPresetLabels['master-caution']);
});
test('input controls require matching debug boot while HID and setup ownership are independent', () => {
  assert.equal(debugReady({ ...state, claimedModules: ['panel'] }, module), true);
  assert.equal(debugReady(state, { ...module, online: false }), true, 'Lifecycle controls can restore offline devices');
  for (const change of [{ connected: false }, { reconnecting: true }, { layoutPending: true }, { debug: undefined }, { debug: { devices: [] } }]) assert.equal(debugReady({ ...state, ...change }, module), false);
  assert.equal(debugReady(state, { ...module, boot: 10 }), false);
  assert.equal(debugReady({ ...state, debug: { devices: [{ ...device, online: false, boot: 10 }] } }, module), true,
    'An offline reboot updates debug boot before registry heartbeat; lifecycle controls must restore it with the new boot');
  assert.equal(debugReady(state, undefined), false); assert.equal(debugReady(null, module), false);
  assert.equal(debugDefinition(state, device), definition);
  const config = { ...definition, axes: [] };
  assert.equal(debugDefinition({ ...state, moduleConfigs: { panel: { view: 'active', config } } }, device), config);
  assert.equal(debugDefinition({ ...state, moduleConfigs: { panel: { view: 'draft', config } } }, device), definition);
});
test('raw ADC fields validate whole 12-bit samples including meaningful zero', () => {
  for (const value of ['0', 0, '4095', 2048]) assert.equal(rawAxisValue(value), Number(value));
  for (const value of ['', ' ', '1.5', '-1', '4096', 'NaN', Infinity]) assert.equal(rawAxisValue(value), undefined);
});
test('pending axis updates coalesce by channel but preserve every discrete edge and detent', async () => {
  const blocked = deferred(), calls = [];
  const queue = new DebugInputQueue({ moduleId: 'panel', boot: 9 }, async request => {
    calls.push(plain(request)); if (calls.length === 1) return blocked.promise; return true;
  });
  const pending = [queue.enqueue('axis', [0,100]), queue.enqueue('axis', [0,200]), queue.enqueue('axis', [1,300]), queue.enqueue('axis', [0,400]),
    queue.press(2), queue.enqueue('axis', [0,500]), queue.release(2), queue.enqueue('turn', [0,-1]), queue.enqueue('turn', [0,1])];
  assert.equal(calls.length, 1); blocked.done(true); assert.ok((await Promise.all(pending)).every(Boolean));
  assert.deepEqual(calls.map(call => [call.debugOperation, call.debugArgs]), [['axis',[0,100]],['axis',[0,400]],['axis',[1,300]],['contact',[2,1]],['axis',[0,500]],['contact',[2,0]],['turn',[0,-1]],['turn',[0,1]]]);
  assert.ok(calls.every(call => call.moduleId === 'panel' && call.boot === 9 && call.command === 'debug_invoke'));
});
test('duplicate holds and cancellation release only contacts actually pressed', async () => {
  const calls = [], queue = new DebugInputQueue({ moduleId: 'panel', boot: 9 }, async request => { calls.push(plain(request)); return true; });
  assert.equal(await queue.release(3), false);
  const first = queue.press(3), duplicate = queue.press(3);
  assert.equal(await duplicate, false); assert.equal(await first, true);
  await queue.press(4); assert.deepEqual(Array.from(await queue.releaseHeld()), [true,true]);
  assert.deepEqual(Array.from(await queue.releaseHeld()), []);
  assert.deepEqual(calls.map(call => call.debugArgs), [[3,1],[4,1],[3,0],[4,0]]);
});
test('reset discards unsent axis updates and closes with neutral after in-flight discrete events', async () => {
  const blocked = deferred(), calls = [], queue = new DebugInputQueue({ moduleId: 'panel', boot: 9 }, async request => { calls.push(plain(request)); return calls.length === 1 ? blocked.promise : true; });
  const press = queue.press(0), axis = queue.enqueue('axis', [0,4095]), contact = queue.enqueue('contact', [1,1]), closed = queue.close();
  assert.equal(await axis, false);
  assert.equal(await queue.enqueue('turn', [0,1]), false); assert.equal(await queue.press(3), false);
  blocked.done(true); assert.deepEqual(await Promise.all([press,contact,closed]), [true,true,true]);
  assert.deepEqual(calls.map(call => call.debugOperation), ['contact','contact','neutral']);
  assert.equal(await queue.release(0), false);
});
test('a failed request does not swallow its result or stall the following release', async () => {
  let calls = 0;
  const queue = new DebugInputQueue({ moduleId: 'panel', boot: 9 }, async () => { if (++calls === 1) throw new Error('Disconnected'); return true; });
  assert.equal(await queue.press(0), false); assert.equal(await queue.release(0), true); await turn();
  assert.equal(await queue.reset(), true);
});

test('closing a replaced or disconnected boot discards queued work without resetting its replacement', async () => {
  const waiting = deferred(), calls = [];
  const queue = new DebugInputQueue({ moduleId: 'panel', boot: 9 }, async request => { calls.push(plain(request)); return waiting.promise; });
  const press = queue.press(0), axis = queue.enqueue('axis', [0,2000]), release = queue.release(0);
  assert.equal(await queue.close(false), false);
  assert.deepEqual(await Promise.all([axis,release]), [false,false]);
  assert.equal(await queue.enqueue('neutral'), false); waiting.done(true); await press;
  assert.deepEqual(calls.map(call => call.debugOperation), ['contact']);
});
