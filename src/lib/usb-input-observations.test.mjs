import test from 'node:test';
import assert from 'node:assert/strict';
import { createHash, webcrypto } from 'node:crypto';
import { readFile } from 'node:fs/promises';
import { fileURLToPath } from 'node:url';
import vm from 'node:vm';
import ts from 'typescript';
import { compile } from 'svelte/compiler';
import { render } from 'svelte/server';
import * as server from 'svelte/internal/server';
import * as svelte from 'svelte';
import { SharedObservations } from './streams/shared.ts';
import { uint64 } from './streams/protocol.ts';
import { loadModules } from './test-module-loader.mjs';

const modules = await loadModules(['./module-usb']);
const file = new URL('./usb-input-observations.ts', import.meta.url), exports = {};
const code = ts.transpileModule(await readFile(file, 'utf8'), { fileName: fileURLToPath(file), compilerOptions: {
  module: ts.ModuleKind.CommonJS, target: ts.ScriptTarget.ES2022, inlineSourceMap: true, inlineSources: true,
} }).outputText;
vm.runInNewContext(code, { exports, TextEncoder, structuredClone, crypto: webcrypto, require(name) {
  if (name === './streams/shared.ts') return { SharedObservations };
  if (name === './streams/protocol.ts') return { uint64 };
  if (name === './module-usb.ts') return modules.get('./module-usb');
  throw Error(name);
} }, { filename: fileURLToPath(file) });
const { UsbInputObservations, usbInputsTopic, captureUsbInputs } = exports;
const serial = '00112233445566778899aabb', key = `${serial}:COM8`;
const route = (changes = {}) => ({ moduleId: `4f534744${serial}`, serial, connectionGeneration: 4, boot: 7, configurationGeneration: 3, ...changes });
const state = (changes = {}) => ({ ok: true, schemaVersion: 1, stream: 3, head: 2, oldest: 1, overflow: 0,
  buttonCount: 2, held: [false, true], contactButtonCount: 2, maintainedButtons: [1], encoderButtons: [], selectorButtons: [],
  axisCount: 1, axes: [40000], axisNeutral: [32767], axisUsage: [48], axisAvailable: [true], sampledAtMs: 20, ...changes });
const payload = (changes = {}) => ({ key, route: route(), ready: true, state: state(), error: null, ...changes });
const frame = (topic, value = payload(), changes = {}) => ({ kind: 'snapshot', continuity: 'baseline', topic, source: 'usb-inputs',
  session: 'broker', clockId: 'usb-inputs:capture', generation: '1', revision: '1', sequence: '1', capturedUs: '1', payload: value, ...changes });
const change = (revision = 2, changes = {}) => ({ kind: 'change', continuity: 'continuous', revision: String(revision), sequence: String(revision), ...changes });
const settle = () => new Promise(resolve => setImmediate(resolve));
const plain = value => JSON.parse(JSON.stringify(value));
const deferred = () => { let resolve; const promise = new Promise(done => { resolve = done; }); return { promise, resolve }; };
function harness() {
  const opens = [], waiters = [], observations = new SharedObservations({ open(topic, observer, signal) {
    opens.push({ topic, observer, signal, delivery: 0 }); waiters.splice(0).forEach(resolve => resolve());
  } });
  const client = new UsbInputObservations(observations);
  const send = (value = payload(), changes = {}, open = opens.at(-1)) => open.observer.frame({ subscriptionId: open.topic,
    delivery: String(++open.delivery), frame: frame(open.topic, value, changes) }, async () => {});
  const opened = async (count = 1) => { while (opens.length < count) await new Promise(resolve => waiters.push(resolve)); };
  return { client, opens, send, opened };
}

test('topic hashes exact UTF-8 endpoint keys without case folding or parsing', async () => {
  for (const value of [key, key.toLowerCase(), 'serial:COM8:é', 'serial:COM8:e\u0301', 'serial:😀']) {
    assert.equal(await usbInputsTopic(value), `usb.inputs.${createHash('sha256').update(value, 'utf8').digest('hex')}`);
  }
  assert.notEqual(await usbInputsTopic(key), await usbInputsTopic(key.toLowerCase()));
  for (const value of ['', '\n', '\ud800', '\udc00', 'x'.repeat(2049)]) await assert.rejects(usbInputsTopic(value));
  await assert.rejects(usbInputsTopic(key, async () => new ArrayBuffer(1)));
});

test('two consumers share one immutable capture and dispose independently without commands', async () => {
  const h = harness(), first = [], second = [];
  const a = h.client.observe(key, value => first.push(value)), b = h.client.observe(key, value => second.push(value));
  await h.opened(); assert.equal(h.opens.length, 1); h.send();
  assert.equal(first.at(-1).status, 'ready'); assert.equal(first.at(-1).value, second.at(-1).value);
  assert.equal(Object.isFrozen(first.at(-1).value.state.held), true);
  assert.equal(Object.isFrozen(first.at(-1).value.route), true);
  a(); a(); assert.equal(h.opens[0].signal.aborted, false);
  h.send(payload({ state: state({ held: [true, true] }) }), change());
  assert.equal(second.at(-1).value.state.held[0], true);
  b(); assert.equal(h.opens[0].signal.aborted, true);
  const fresh = [], off = h.client.observe(key, value => fresh.push(value));
  assert.equal(fresh.at(-1).value, undefined); await h.opened(2); assert.equal(h.opens.length, 2); off();
});

test('loss withdraws input and requires a current baseline while old frames cannot restore it', async () => {
  const h = harness(), values = [], off = h.client.observe(key, value => values.push(value)); await h.opened(); h.send();
  h.send(undefined, { kind: 'loss', continuity: 'lost', generation: '2', revision: '2', sequence: '2', reason: 'USB input unavailable' });
  assert.equal(values.at(-1).status, 'lost'); assert.equal(values.at(-1).value, undefined);
  h.send(payload(), change(3, { generation: '2' })); assert.equal(values.at(-1).status, 'lost');
  h.send(payload({ state: state({ axes: [20] }) }), { generation: '2', revision: '4', sequence: '4' });
  const current = values.at(-1); assert.equal(current.value.state.axes[0], 20);
  h.send(payload(), { generation: '1', revision: '2', sequence: '2' }); assert.equal(values.at(-1), current); off();
});

test('unavailable replacements and malformed payloads never retain healthy levels', async () => {
  const h = harness(), values = [], off = h.client.observe(key, value => values.push(value)); await h.opened(); h.send();
  h.send(payload({ ready: false, state: null, error: 'Input sample expired.' }), change());
  assert.equal(values.at(-1).value.ready, false); assert.equal(values.at(-1).value.state, null);
  h.send(payload({ key: `${key}:other` }), change(3)); assert.equal(values.at(-1).status, 'error'); assert.equal(values.at(-1).value, undefined);
  h.send(payload(), change(4)); assert.equal(values.at(-1).status, 'lost');
  h.send(payload(), { revision: '5', sequence: '5' }); assert.equal(values.at(-1).status, 'ready'); off();
});

test('complete metadata replacements validate exact keys, route identity, bounds and no replay pages', async () => {
  const topic = await usbInputsTopic(key), capture = value => captureUsbInputs(frame(topic, value), topic, key);
  const original = payload(), copied = capture(original); original.state.held[0] = true; assert.equal(copied.state.held[0], false);
  assert.equal(capture(payload({ route: null, ready: false, state: null })).ready, false);
  for (const broken of [payload({ extra: true }), payload({ key: 'different' }), payload({ route: null }), payload({ ready: 1 }), payload({ error: 'bad' }),
    payload({ ready: false }), payload({ error: 'x'.repeat(4097) }), payload({ route: route({ boot: 0 }) }), payload({ route: route({ serial: 'bad' }) }),
    payload({ route: route({ moduleId: 'invented' }) }), payload({ route: route({ connectionGeneration: 0 }) }), payload({ route: route({ configurationGeneration: 4 }) }),
    payload({ route: route({ unknown: true }) })]) assert.throws(() => capture(broken));
  for (const patch of [{ ok: false }, { schemaVersion: 2 }, { head: -1 }, { oldest: 4 }, { overflow: 4294967296 }, { stream: 0 },
    { held: [false] }, { held: [0, 1] }, { buttonCount: 129 }, { contactButtonCount: 3 }, { maintainedButtons: [1, 1] }, { maintainedButtons: [2] },
    { encoderButtons: [0] }, { selectorButtons: [[0, 1]] }, { axisCount: 9 }, { axes: [65536] }, { axes: [] }, { axisNeutral: [-1] },
    { axisUsage: [47] }, { axisAvailable: [1] }, { sampledAtMs: null }, { events: [] }, { after: 0 }, { next: 0 }, { more: false }]) {
    assert.throws(() => capture(payload({ state: state(patch) })), JSON.stringify(patch));
  }
  for (const patch of [{ topic: 'usb.inputs.wrong' }, { source: 'games' }, { kind: 'loss' }]) assert.throws(() => captureUsbInputs(frame(topic, payload(), patch), topic, key));
  const full = state({ buttonCount: 7, held: Array(7).fill(false), contactButtonCount: 2, encoderButtons: [2, 3], selectorButtons: [[4, 5, 6]],
    axisCount: 0, axes: [], axisNeutral: [], axisUsage: [], axisAvailable: [] });
  assert.equal(capture(payload({ state: full })).state.buttonCount, 7);
});

test('route replacement needs a fresh snapshot and clock/session changes cannot revive old input', async () => {
  let emit; const opened = deferred(), observations = { subscribe(_topic, listener) { emit = listener; opened.resolve(); return () => {}; } };
  const client = new UsbInputObservations(observations), seen = [], off = client.observe(key, value => seen.push(value)); await opened.promise;
  const topic = await usbInputsTopic(key), send = (value, changes = {}) => emit({ status: 'ready', frame: frame(topic, value, changes) });
  send(payload()); const replacement = payload({ route: route({ connectionGeneration: 5 }) });
  send(replacement, change()); assert.equal(seen.at(-1).status, 'lost'); assert.equal(seen.at(-1).value, undefined);
  send(replacement, { revision: '3' }); assert.equal(seen.at(-1).value.route.connectionGeneration, 5);
  send(payload(), change(4, { clockId: 'old-clock' })); assert.equal(seen.at(-1).status, 'error');
  send(replacement, { revision: '4', clockId: 'same-generation-replacement' }); assert.equal(seen.at(-1).status, 'error');
  send(replacement, change(4, { generation: '2', clockId: 'new-capture' })); assert.equal(seen.at(-1).status, 'error');
  emit({ status: 'lost', frame: frame(topic, null, { kind: 'loss', continuity: 'lost', generation: '2', revision: '5', clockId: 'new-capture' }) });
  const newest = payload({ route: route({ connectionGeneration: 6 }) });
  send(newest, { generation: '2', revision: '5', clockId: 'new-capture' });
  const current = seen.at(-1); assert.equal(current.status, 'ready'); assert.equal(current.value.route.connectionGeneration, 6);
  send(replacement, { revision: '3', clockId: 'usb-inputs:capture' }); assert.equal(seen.at(-1), current);
  send(payload(), { revision: '4', session: 'retired-broker' }); assert.equal(seen.at(-1).status, 'error');
  off(); send(payload()); assert.equal(seen.at(-1).status, 'error');
});

test('disposal while hashing prevents registration and isolates callbacks and transport failures', async () => {
  let resolve; const digest = () => new Promise(done => { resolve = done; });
  const opens = [], client = new UsbInputObservations({ subscribe(topic) { opens.push(topic); return () => {}; } }, digest);
  const off = client.observe(key, () => { throw Error('view'); }); off(); resolve(new ArrayBuffer(32)); await settle(); assert.deepEqual(opens, []);
  const failed = [], failedState = deferred(), broken = new UsbInputObservations({ subscribe() { throw Error('transport unavailable'); } });
  broken.observe(key, value => { failed.push(value); if (value.status === 'error') failedState.resolve(); }); await failedState.promise; assert.equal(failed.at(-1).status, 'error');
  const h = harness(), seen = [], one = h.client.observe(key, () => { throw Error('view'); }), two = h.client.observe(key, value => seen.push(value));
  await h.opened(); h.send(); assert.equal(seen.at(-1).status, 'ready');
  h.opens[0].observer.error('Stream disconnected'); assert.equal(seen.at(-1).status, 'error'); assert.equal(seen.at(-1).value, undefined); one(); two();
  const invalid = []; new UsbInputObservations().observe('', value => invalid.push(value)); assert.equal(invalid[0].status, 'error');
});

test('entry admission and source boundaries are bounded even with an injected transport', async () => {
  const client = new UsbInputObservations({ subscribe() { return () => {}; } }, () => new Promise(() => {}));
  const stops = []; for (let index = 0; index < 128; index++) stops.push(client.observe(`${key}:${index}`, () => {}));
  const denied = []; client.observe('overflow', value => denied.push(value)); assert.equal(denied[0].status, 'error'); stops.forEach(stop => stop());
  let emit; const opened = deferred(), seen = [], direct = new UsbInputObservations({ subscribe(_topic, listener) { emit = listener; opened.resolve(); return () => {}; } });
  const off = direct.observe(key, value => seen.push(value)); await opened.promise; const topic = await usbInputsTopic(key);
  emit({ status: 'ready', frame: frame(topic, payload(), change()) }); assert.equal(seen.at(-1).status, 'lost');
  emit({ status: 'ready', frame: frame(topic) }); emit({ status: 'ready', frame: frame(topic, payload(), { revision: '0' }) }); assert.equal(seen.at(-1).status, 'ready');
  emit({ status: 'ready', frame: frame(topic, payload(), change(2, { generation: '2' })) }); assert.equal(seen.at(-1).status, 'lost'); off();
});

test('USB view subscribes passively, removes the acquisition timer and keeps pushed preview out of the draft', async () => {
  const componentFile = new URL('./SystemUsbDetails.svelte', import.meta.url), source = await readFile(componentFile, 'utf8');
  assert.doesNotMatch(source, /setInterval|eventsState|module_usb_events_state/);
  const mounts = [], calls = [], observations = []; let handlers, emit, stopped = false;
  const module = { key, snapshot: { connected: true, route: route(), config: null, claimedBy: null } };
  // SSR does not execute effects; schedule this exact subscription as a mount hook.
  const instrumented = source.replace('$effect(() => observeUsbInputs(endpointKey, state => { inputObservation = state; }));',
    'onMount(() => observeUsbInputs(endpointKey, state => { inputObservation = state; }));')
    .replace('</script>', 'capture({get module(){return module;},get inputObservation(){return inputObservation;},get draft(){return draft;},setDraft(value){draft=value;}});</script>');
  const { js } = compile(instrumented, { filename: componentFile.pathname, generate: 'server' }), output = {};
  vm.runInNewContext(ts.transpileModule(js.code, { compilerOptions: { module: ts.ModuleKind.CommonJS, target: ts.ScriptTarget.ES2022 } }).outputText,
    { exports: output, capture(value) { handlers = value; }, require(name) {
      if (name === 'svelte/internal/server') return server;
      if (name === 'svelte') return { ...svelte, onMount(callback) { mounts.push(callback); } };
      if (name.endsWith('.svelte')) return { default() {} };
      if (name === './gateway') return { gatewayRequest() { throw Error('unexpected command'); } };
      if (name === './usb-input-observations') return { observeUsbInputs(selected, listener) { observations.push(selected); emit = listener; return () => { stopped = true; }; } };
      if (name === './module-usb') return { ...modules.get('./module-usb'), ModuleUsbClient: class { async status(selected) { calls.push(selected); return module; } } };
      const normalized = name.replace(/\.ts$/, ''); if (modules.has(normalized)) return modules.get(normalized);
      throw Error(name);
    } }, { filename: componentFile.pathname });
  const html = render(output.default, { props: { endpointKey: key } }).body;
  assert.match(html, /Waiting for a USB input baseline/);
  const cleanup = mounts.map(mount => mount()); await settle();
  assert.deepEqual(calls, [key]); assert.deepEqual(observations, [key]);
  const draft = { name: 'Unsaved local USB configuration' }; handlers.setDraft(draft); const confirmed = handlers.module;
  emit({ status: 'ready', value: payload() }); assert.equal(handlers.inputObservation.value.state.held[1], true);
  assert.equal(handlers.module, confirmed); assert.equal(handlers.draft, draft); assert.deepEqual(calls, [key]);
  emit({ status: 'lost', error: 'USB source lost' }); assert.equal(handlers.inputObservation.value, undefined); assert.equal(handlers.draft, draft);
  cleanup.forEach(stop => stop?.()); assert.equal(stopped, true);
});
