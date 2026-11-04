import test from 'node:test';
import assert from 'node:assert/strict';
import { createHash, webcrypto } from 'node:crypto';
import { readFile } from 'node:fs/promises';
import { fileURLToPath } from 'node:url';
import vm from 'node:vm';
import ts from 'typescript';
import { SharedObservations } from './streams/shared.ts';
import { uint64 } from './streams/protocol.ts';
import { loadModules } from './test-module-loader.mjs';

const modules = await loadModules(['./module-usb']);
const file = new URL('./usb-metadata-observations.ts', import.meta.url), exports = {};
const code = ts.transpileModule(await readFile(file, 'utf8'), { fileName: fileURLToPath(file), compilerOptions: {
  module: ts.ModuleKind.CommonJS, target: ts.ScriptTarget.ES2022, inlineSourceMap: true, inlineSources: true,
} }).outputText;
vm.runInNewContext(code, { exports, TextEncoder, structuredClone, crypto: webcrypto, require(name) {
  if (name === './streams/shared.ts') return { SharedObservations };
  if (name === './streams/protocol.ts') return { uint64 };
  if (name === './module-usb.ts') return modules.get('./module-usb');
  throw Error(name);
} }, { filename: fileURLToPath(file) });
const { UsbMetadataObservations, usbMetadataTopic, captureUsbMetadata } = exports;
const serial = '00112233445566778899aabb', key = `${serial}:COM8`;
const route = (changes = {}) => ({ moduleId: `4f534744${serial}`, serial, connectionGeneration: 4, boot: 7, configurationGeneration: 3, ...changes });
const runtime = (changes = {}) => ({ generation: 3, savedGeneration: 2, owner: 4, claimExpiresMs: 100, dirty: false, pendingApply: false, valid: true,
  view: 'active', config: { name: 'USB panel', expanderMask: 0, buttons: [[0, 0, 0]], axes: [], hats: [], encoders: [], selectors: [] },
  layout: { buttons: 1, axes: 0, hats: 0, reportBytes: 1 }, descriptorHex: '0501', revision: 8, revisionExhausted: false, ...changes });
const identity = () => ({ model: 'device-template', board: '0.1.0', profile: 'USB panel', moduleId: route().moduleId, serial, protocol: 3, buttonCount: 1, axisCount: 0, hatCount: 0 });
const payload = (changes = {}) => ({ key, connected: true, port: 'COM8', identity: identity(), route: route(), config: runtime(), observedAtMs: 20, error: null, ...changes });
const frame = (topic, value = payload(), changes = {}) => ({ kind: 'snapshot', continuity: 'baseline', topic, source: 'usb-metadata',
  session: 'broker', clockId: 'usb-metadata:capture', generation: '1', revision: '1', sequence: '1', capturedUs: '1', payload: value, ...changes });
const change = (revision = 2, changes = {}) => ({ kind: 'change', continuity: 'continuous', revision: String(revision), sequence: String(revision), ...changes });
const deferred = () => { let resolve; const promise = new Promise(done => { resolve = done; }); return { promise, resolve }; };
function harness() {
  const opens = [], waiting = [];
  const shared = new SharedObservations({ open(topic, observer, signal) { opens.push({ topic, observer, signal, delivery: 0 }); waiting.splice(0).forEach(resolve => resolve()); } });
  const client = new UsbMetadataObservations(shared);
  const send = (value = payload(), changes = {}, opened = opens.at(-1)) => opened.observer.frame({ subscriptionId: opened.topic,
    delivery: String(++opened.delivery), frame: frame(opened.topic, value, changes) }, async () => {});
  const opened = async (count = 1) => { while (opens.length < count) await new Promise(resolve => waiting.push(resolve)); };
  return { client, opens, send, opened };
}

test('topic hashes the exact endpoint key and rejects invalid Unicode and oversized keys', async () => {
  for (const value of [key, key.toLowerCase(), 'serial:COM8:é', 'serial:COM8:e\u0301', 'serial:😀']) {
    assert.equal(await usbMetadataTopic(value), `usb.metadata.${createHash('sha256').update(value, 'utf8').digest('hex')}`);
  }
  assert.notEqual(await usbMetadataTopic(key), await usbMetadataTopic(key.toLowerCase()));
  for (const value of ['', '\n', '\ud800', '\udc00', 'x'.repeat(2049)]) await assert.rejects(usbMetadataTopic(value));
  await assert.rejects(usbMetadataTopic(key, async () => new ArrayBuffer(1)));
});

test('captures exact public shape, freezes a detached copy and rejects private or stale fields', async () => {
  const topic = await usbMetadataTopic(key), capture = value => captureUsbMetadata(frame(topic, value), topic, key);
  const original = payload(), copied = capture(original); original.config.config.buttons[0][0] = 12;
  assert.equal(copied.config.config.buttons[0][0], 0); assert.equal(Object.isFrozen(copied.config.config.buttons[0]), true);
  assert.equal(capture(payload({ connected: false, route: null, config: null })).connected, false);
  assert.equal(capture(payload({ connected: false, route: null, config: runtime() })).config.revision, 8);
  for (const broken of [payload({ claimedBy: null }), payload({ draft: null }), payload({ actions: null }), payload({ events: null }), payload({ key: 'other' }),
    payload({ route: { ...route(), unknown: true } }), payload({ connected: 1 }), payload({ observedAtMs: -1 }), payload({ error: 'x'.repeat(4097) }),
    payload({ identity: { ...identity(), private: true } }), payload({ config: runtime({ revision: 0 }) }), payload({ config: runtime({ revisionExhausted: true }) }),
    payload({ config: runtime({ owner: -1 }) }), payload({ config: runtime({ view: 'draft' }) }), payload({ config: runtime({ claimedBy: 'secret' }) }),
    payload({ config: runtime({ revision: 4294967296 }) }), payload({ config: runtime({ config: { ...runtime().config, private: true } }) }),
    payload({ connected: true, route: null, config: runtime({ revision: 0 }) }), payload({ connected: true, config: null })]) assert.throws(() => capture(broken));
  for (const changes of [{ topic: 'usb.metadata.invalid' }, { source: 'usb-inputs' }, { kind: 'loss' }]) assert.throws(() => captureUsbMetadata(frame(topic, payload(), changes), topic, key));
});

test('subscribers share immutable exact-key metadata and loss requires a current snapshot', async () => {
  const h = harness(), first = [], second = [], stopA = h.client.observe(key, value => first.push(value)), stopB = h.client.observe(key, value => second.push(value));
  await h.opened(); assert.equal(h.opens.length, 1); h.send(); assert.equal(first.at(-1).status, 'ready'); assert.equal(first.at(-1).value, second.at(-1).value);
  h.send(payload({ config: runtime({ revision: 9, config: { ...runtime().config, name: 'Updated' } }) }), change());
  assert.equal(second.at(-1).value.config.config.name, 'Updated'); stopA(); assert.equal(h.opens[0].signal.aborted, false);
  h.send(undefined, { kind: 'loss', continuity: 'lost', generation: '2', revision: '3', sequence: '3', reason: 'source unavailable' });
  assert.equal(second.at(-1).status, 'lost'); assert.equal(second.at(-1).value, undefined);
  h.send(payload(), { kind: 'change', continuity: 'continuous', generation: '2', revision: '4', sequence: '4' }); assert.equal(second.at(-1).status, 'lost');
  h.send(payload(), { kind: 'snapshot', continuity: 'baseline', generation: '2', revision: '5', sequence: '5' }); assert.equal(second.at(-1).status, 'ready');
  stopB(); assert.equal(h.opens[0].signal.aborted, true);
});

test('routes, sessions, generation boundaries and invalid observations cannot revive stale metadata', async () => {
  let emit; const ready = deferred(), direct = new UsbMetadataObservations({ subscribe(_topic, listener) { emit = listener; ready.resolve(); return () => {}; } });
  const seen = [], stop = direct.observe(key, value => seen.push(value)); await ready.promise; const topic = await usbMetadataTopic(key);
  const send = (value, changes = {}) => emit({ status: 'ready', frame: frame(topic, value, changes) });
  send(payload()); const current = seen.at(-1);
  send(payload({ route: route({ connectionGeneration: 5 }) }), change()); assert.equal(seen.at(-1).status, 'lost');
  send(payload({ route: route({ connectionGeneration: 5 }) }), { kind: 'snapshot', continuity: 'baseline', revision: '3', sequence: '3' });
  assert.equal(seen.at(-1).status, 'ready');
  const recovered = seen.at(-1); emit({ status: 'lost', frame: frame(topic, null, { kind: 'loss', continuity: 'lost', generation: '2', revision: '5', clockId: 'new-clock' }) });
  assert.equal(seen.at(-1).status, 'lost');
  send(payload(), { generation: '1', revision: '4', clockId: 'usb-metadata:capture' }); assert.equal(seen.at(-1).status, 'lost');
  send(payload({ route: route({ connectionGeneration: 5 }) }), { generation: '2', revision: '5', clockId: 'new-clock' });
  assert.equal(seen.at(-1).status, 'ready'); assert.notEqual(seen.at(-1), recovered); stop();
});

test('disposal during hashing and listener failures remain isolated', async () => {
  let resolve; const digest = () => new Promise(done => { resolve = done; });
  const topics = [], client = new UsbMetadataObservations({ subscribe(topic) { topics.push(topic); return () => {}; } }, digest), stop = client.observe(key, () => { throw Error('view'); });
  stop(); resolve(new ArrayBuffer(32)); await new Promise(done => setImmediate(done)); assert.deepEqual(topics, []);
  const errors = [], invalid = new UsbMetadataObservations(); invalid.observe('', value => errors.push(value)); assert.equal(errors[0].status, 'error');
});
