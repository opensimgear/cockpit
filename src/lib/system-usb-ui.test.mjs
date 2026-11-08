import test from 'node:test';
import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import vm from 'node:vm';
import ts from 'typescript';
import { compile } from 'svelte/compiler';
import { render } from 'svelte/server';
import * as server from 'svelte/internal/server';
import * as svelte from 'svelte';
import { loadModules } from './test-module-loader.mjs';

const modules = await loadModules(['./gateway-capabilities', './device-actions', './runtime-config', './module-usb']);
const plain = value => JSON.parse(JSON.stringify(value));
const deferred = () => { let resolve, reject; const promise = new Promise((done, fail) => { resolve = done; reject = fail; }); return { promise, resolve, reject }; };
const settle = () => new Promise(done => setImmediate(done));
const serial = '00112233445566778899aabb', key = serial + ':COM8';
const route = () => ({ moduleId: '4f534744' + serial, serial, connectionGeneration: 4, boot: 100, configurationGeneration: 1 });
const config = () => ({ name: 'USB panel', expanderMask: 0, buttons: [[0, 0, 0]], axes: [], hats: [], encoders: [], selectors: [] });
const runtime = () => ({ generation: 1, savedGeneration: 1, owner: 0, claimExpiresMs: 0, dirty: false, pendingApply: false, valid: true, view: 'active', config: config(), layout: { buttons: 1, axes: 0, hats: 0, reportBytes: 1 }, descriptorHex: '0501', revision: 1, revisionExhausted: false });
const action = () => ({ id: 'led.set', label: 'LED', endpoint: 'led', semantics: 'setter', readable: true, parameters: [{ id: 'enabled', label: 'Enabled', type: 'boolean' }] });
const endpoint = () => ({ key, port: { path: 'COM8', serialNumber: serial }, present: true, automatic: false, admissible: true, diagnostics: [], snapshot: {
  connected: true, port: 'COM8', identity: { model: 'device-template', board: 'RP2040', profile: config().name, moduleId: route().moduleId, serial, protocol: 3, buttonCount: 1, axisCount: 0, hatCount: 0 }, route: route(), config: runtime(), draft: null,
  actions: { boot: 100, route: route(), status: 'ready', catalogueRevision: 7, actions: [action()], states: [{ id: action().id, values: [false] }], ownership: null, error: null },
  events: null, claimedBy: null, observedAtMs: 1000, error: null,
} });

async function harness() {
  let current = endpoint(), handlers, mountedCleanup;
  const calls = [], mounts = [], custom = new Map(), timers = new Map();
  const document = Object.assign(new EventTarget(), { hidden: false });
  const visibility = {};
  const visibilitySource = await readFile(new URL('./visible-observation.ts', import.meta.url), 'utf8');
  vm.runInNewContext(ts.transpileModule(visibilitySource, { compilerOptions: { module: ts.ModuleKind.CommonJS } }).outputText,
    { exports: visibility, document });
  const inputListeners = new Set(), metadataListeners = new Set();
  const subscribe = listeners => (selectedKey, listener) => {
    assert.equal(selectedKey, key);
    listeners.add(listener);
    return () => listeners.delete(listener);
  };
  const publishMetadata = value => metadataListeners.forEach(listener => listener({ status: 'ready', value: {
    key: value.key, connected: value.snapshot.connected, port: value.snapshot.port, identity: value.snapshot.identity, route: value.snapshot.route,
    config: value.snapshot.config, observedAtMs: value.snapshot.observedAtMs, error: value.snapshot.error,
  } }));
  const gatewayRequest = async request => {
    calls.push(plain(request));
    if (custom.has(request.command)) return custom.get(request.command)(request);
    if (request.command === 'module_usb_ports') return { moduleUsb: { ports: [{ key: 'unrelated', port: { path: 'COM9' }, present: true, automatic: false, admissible: true, connected: false, diagnostics: [] }, { key, port: current.port, present: true, automatic: false, admissible: true, connected: current.snapshot.connected, diagnostics: [] }] } };
    if (request.command === 'module_usb_claim') { current.snapshot.claimedBy = 'operator'; current.snapshot.config.owner = 4; current.snapshot.config.revision++; }
    if (request.command === 'module_usb_release') { current.snapshot.claimedBy = null; current.snapshot.config.owner = 0; current.snapshot.config.revision++; }
    if (request.command === 'module_usb_config_apply') {
      current.snapshot.config.config = plain(request.config);
      current.snapshot.route.configurationGeneration++;
      current.snapshot.config.generation = current.snapshot.route.configurationGeneration;
      current.snapshot.config.revision++;
      current.snapshot.actions.route = plain(current.snapshot.route);
    }
    if (request.command === 'module_usb_actions_invoke') current.snapshot.actions.states[0].values = plain(request.values);
    if (['module_usb_claim', 'module_usb_release', 'module_usb_config_apply'].includes(request.command)) publishMetadata(current);
    return { moduleUsb: { module: plain(current) } };
  };
  const file = new URL('./SystemUsbDetails.svelte', import.meta.url);
  const source = (await readFile(file, 'utf8')).replaceAll('\r\n', '\n')
    .replace('$effect(watchDetails);', 'onMount(watchDetails);')
    .replace('</script>', `capture({run,client,connect,claim,release,configure,invoke,get module(){return module;},get editing(){return editing;},get busy(){return busy;},get error(){return error;},get draft(){return draft;},setDraft(value){draft=value;}});</script>`);
  assert.match(source, /onMount\(watchDetails\)/, 'The server harness must mount the visible readers.');
  const { js } = compile(source, { filename: file.pathname, generate: 'server' }), exports = {};
  vm.runInNewContext(ts.transpileModule(js.code, { compilerOptions: { module: ts.ModuleKind.CommonJS, target: ts.ScriptTarget.ES2022 } }).outputText, {
    exports, document, setInterval(callback) { const id = timers.size + 1; timers.set(id, callback); return id; }, clearInterval(id) { timers.delete(id); }, capture(value) { handlers = value; }, require(name) {
      if (name === 'svelte/internal/server') return server;
      if (name === 'svelte') return { ...svelte, onMount(callback) { mounts.push(callback); } };
      if (name.endsWith('.svelte')) return { default() {} };
      if (name === './gateway') return { gatewayRequest };
      if (name === './usb-input-observations') return { observeUsbInputs: subscribe(inputListeners) };
      if (name === './usb-metadata-observations') return { observeUsbMetadata: subscribe(metadataListeners) };
      if (name === './visible-observation') return visibility;
      const normalized = name.replace(/\.ts$/, '');
      if (modules.has(normalized)) return modules.get(normalized);
      throw Error('Unexpected component import ' + name);
    },
  }, { filename: file.pathname });
  const html = render(exports.default, { props: { endpointKey: key } }).body;
  return { html, handlers, calls, custom, timers, document, inputListeners, metadataListeners, get current() { return current; }, setCurrent(value) { current = value; publishMetadata(current); }, publishMetadata,
    loseMetadata() { metadataListeners.forEach(listener => listener({ status: 'lost', error: 'USB metadata source lost' })); },
    async mount() { mountedCleanup = mounts.map(mount => mount()); await settle(); publishMetadata(current); }, cleanup() { mountedCleanup?.forEach(stop => stop?.()); }, refresh() { return handlers.run(() => handlers.client.status(key), false); } };
}

test('USB detail rendering and mounting inspect the exact endpoint without claiming, applying or dispatching output', async () => {
  const h = await harness();
  assert.deepEqual(h.calls, []);
  assert.ok(h.html.includes('Configuration changes require an explicit setup claim'));
  await h.mount();
  assert.deepEqual(h.calls, [{ command: 'module_usb_status', key }]);
  assert.equal(h.handlers.editing, false);
  assert.equal(await h.handlers.configure('config_apply', config()), false);
  assert.equal(h.calls.length, 1);
  h.cleanup();
  await settle();
  assert.equal(h.calls.length, 1);
});

test('Connect resolves the selected observed key and path, and missing endpoints surface an error without selecting another port', async () => {
  const h = await harness();
  await h.handlers.connect();
  assert.deepEqual(h.calls, [{ command: 'module_usb_ports' }, { command: 'module_usb_connect', key, port: 'COM8' }]);
  const absent = await harness();
  absent.custom.set('module_usb_ports', () => ({ moduleUsb: { ports: [] } }));
  await absent.handlers.connect();
  assert.equal(absent.calls.length, 1);
  assert.match(absent.handlers.error, /exact USB endpoint is no longer present/);
});

test('actual setup handlers claim once, configure exact current routes, preserve Save intent and explicitly release', async () => {
  const h = await harness(); await h.mount(); h.calls.length = 0;
  await h.handlers.claim();
  assert.equal(h.handlers.editing, true);
  const edited = { ...config(), name: 'Edited panel' };
  await h.handlers.configure('config_apply', edited);
  await h.handlers.configure('config_apply_save', edited);
  await h.handlers.configure('config_save'); await h.handlers.configure('config_discard'); await h.handlers.configure('config_reset');
  assert.equal(await h.handlers.configure('scan'), false);
  await h.handlers.release();
  assert.equal(h.handlers.editing, false);
  assert.deepEqual(h.calls.map(request => request.command), ['module_usb_claim', 'module_usb_config_apply', 'module_usb_config_apply', 'module_usb_config_save', 'module_usb_config_discard', 'module_usb_config_reset', 'module_usb_release']);
  assert.equal(h.calls[1].save, false); assert.equal(h.calls[2].save, true);
  assert.equal(h.calls.every(request => request.key === key && request.route.moduleId === route().moduleId), true);
  assert.equal(h.calls.at(-1).route.configurationGeneration, 3);
});

test('own public metadata arriving before its claim receipt preserves a valid local claim', async () => {
  const h = await harness(); await h.mount();
  await h.handlers.claim();
  assert.equal(h.handlers.editing, true);
  assert.equal(h.handlers.module.snapshot.claimedBy, 'operator');
  assert.equal(h.handlers.module.snapshot.config.revision, h.current.snapshot.config.revision);
  h.cleanup();
});

test('a command receipt waits for its public owner revision before enabling setup', async () => {
  const h = await harness(); await h.mount();
  const receipt = plain(h.current); receipt.snapshot.claimedBy = 'operator'; receipt.snapshot.config.owner = 4; receipt.snapshot.config.revision++;
  h.custom.set('module_usb_claim', async () => ({ moduleUsb: { module: receipt } }));
  await h.handlers.claim();
  assert.equal(h.handlers.editing, false);
  h.publishMetadata(receipt);
  assert.equal(h.handlers.editing, true);
  h.cleanup();
});

test('a lagging public revision cannot roll back a newer command receipt display', async () => {
  const h = await harness(); await h.mount(); await h.handlers.claim();
  const receipt = plain(h.current); receipt.snapshot.config.config.name = 'Receipt config'; receipt.snapshot.route.configurationGeneration++;
  receipt.snapshot.config.generation = receipt.snapshot.route.configurationGeneration; receipt.snapshot.config.revision++;
  receipt.snapshot.actions.route = plain(receipt.snapshot.route);
  h.custom.set('module_usb_config_apply', async () => ({ moduleUsb: { module: receipt } }));
  await h.handlers.configure('config_apply', receipt.snapshot.config.config);
  assert.equal(h.handlers.module.snapshot.config.revision, receipt.snapshot.config.revision);
  assert.equal(h.handlers.module.snapshot.config.config.name, 'Receipt config');
  assert.equal(h.handlers.module.snapshot.route.configurationGeneration, 2);
  assert.equal(h.handlers.editing, false, 'editing waits for the receipt revision to become public');
  h.publishMetadata(receipt);
  assert.equal(h.handlers.editing, true);
  h.cleanup();
});

test('a lower token and reset revision from a new route source replaces the retired receipt', async () => {
  const h = await harness(); await h.mount();
  const old = plain(h.current); old.snapshot.route.connectionGeneration = 900; old.snapshot.config.revision = 900;
  old.snapshot.actions.route = plain(old.snapshot.route); h.setCurrent(old);
  const next = endpoint(); next.snapshot.route.connectionGeneration = 1; next.snapshot.config.revision = 1;
  next.snapshot.actions.route = plain(next.snapshot.route); h.setCurrent(next);
  assert.equal(h.handlers.module.snapshot.route.connectionGeneration, 1);
  assert.equal(h.handlers.module.snapshot.config.revision, 1);
  assert.equal(h.handlers.module.snapshot.actions, null);
  assert.equal(h.handlers.module.snapshot.events, null);
  h.cleanup();
});

test('a disconnected public snapshot with no config withdraws the old connection despite its old revision', async () => {
  const h = await harness(); await h.mount();
  const disconnected = plain(h.current); disconnected.snapshot.connected = false; disconnected.snapshot.route = null;
  disconnected.snapshot.identity = null; disconnected.snapshot.config = null; disconnected.snapshot.claimedBy = null;
  h.publishMetadata(disconnected);
  assert.equal(h.handlers.module.snapshot.connected, false);
  assert.equal(h.handlers.module.snapshot.route, null);
  assert.equal(h.handlers.module.snapshot.actions, null);
  assert.equal(h.handlers.module.snapshot.events, null);
  h.cleanup();
});

test('a late old command receipt cannot revive a retired route after a lower-token source arrives', async () => {
  const h = await harness(); await h.mount();
  const oldReceipt = plain(h.current); oldReceipt.snapshot.route.connectionGeneration = 900; oldReceipt.snapshot.config.revision = 900;
  oldReceipt.snapshot.actions.route = plain(oldReceipt.snapshot.route); h.setCurrent(oldReceipt);
  const pending = deferred(); h.custom.set('module_usb_status', () => pending.promise);
  const refreshing = h.refresh();
  const next = endpoint(); next.snapshot.route.connectionGeneration = 1; next.snapshot.config.revision = 1;
  next.snapshot.actions.route = plain(next.snapshot.route); h.setCurrent(next);
  oldReceipt.snapshot.claimedBy = 'stale-private-owner';
  pending.resolve({ moduleUsb: { module: oldReceipt } }); await refreshing;
  assert.equal(h.handlers.module.snapshot.route.connectionGeneration, 1);
  assert.equal(h.handlers.module.snapshot.config.revision, 1);
  assert.equal(h.handlers.module.snapshot.claimedBy, null);
  assert.equal(h.handlers.module.snapshot.actions, null);
  assert.equal(h.handlers.module.snapshot.events, null);
  h.cleanup();
});

test('a newer public revision withdraws editing without replacing the draft or borrowing authority', async () => {
  const h = await harness(); await h.mount(); await h.handlers.claim();
  const draft = { ...config(), name: 'Unsaved draft' }; h.handlers.setDraft(draft);
  h.current.snapshot.config.revision++;
  h.publishMetadata(h.current);
  assert.equal(h.handlers.editing, false);
  assert.equal(h.handlers.draft, draft);
  assert.equal(h.handlers.module.snapshot.claimedBy, null);
  h.cleanup();
});

test('an external same-owner revision during a command cannot be authorized by its older receipt', async () => {
  const h = await harness(); await h.mount(); await h.handlers.claim();
  const pending = deferred(), oldReceipt = plain(h.current), draft = { ...config(), name: 'Unsaved while request waits' };
  h.handlers.setDraft(draft); h.custom.set('module_usb_config_apply', () => pending.promise);
  const applying = h.handlers.configure('config_apply', config());
  h.current.snapshot.config.revision++;
  h.publishMetadata(h.current);
  assert.equal(h.handlers.editing, true, 'the private decision waits while its command is pending');
  pending.resolve({ moduleUsb: { module: oldReceipt } }); await applying;
  assert.equal(h.handlers.editing, false);
  assert.equal(h.handlers.module.snapshot.claimedBy, null);
  assert.equal(h.handlers.draft, draft);
  h.cleanup();
});

test('a rejected command reconciles newer public ownership after leaving busy state', async () => {
  const h = await harness(); await h.mount(); await h.handlers.claim();
  const pending = deferred(); h.custom.set('module_usb_config_apply', () => pending.promise);
  const applying = h.handlers.configure('config_apply', config());
  h.current.snapshot.config.revision++; h.publishMetadata(h.current);
  pending.reject(Error('command reply rejected')); assert.equal(await applying, false);
  assert.equal(h.handlers.busy, false);
  assert.equal(h.handlers.editing, false);
  assert.equal(h.handlers.module.snapshot.claimedBy, null);
  h.cleanup();
});

test('a claim receipt spanning metadata loss and recovery is released and never accepted', async () => {
  const h = await harness(); await h.mount();
  const pending = deferred(); h.custom.set('module_usb_claim', () => pending.promise);
  const claiming = h.handlers.claim(); h.loseMetadata(); h.publishMetadata(h.current);
  const claimed = plain(h.current); claimed.snapshot.claimedBy = 'operator'; claimed.snapshot.config.owner = 4; claimed.snapshot.config.revision++;
  pending.resolve({ moduleUsb: { module: claimed } }); await claiming;
  assert.equal(h.handlers.editing, false);
  assert.notEqual(h.handlers.module.snapshot.claimedBy, 'operator');
  assert.deepEqual(h.calls.map(request => request.command), ['module_usb_status', 'module_usb_claim', 'module_usb_release']);
  h.cleanup();
});

test('metadata loss withdraws the current connection view and preserves the unsaved draft', async () => {
  const h = await harness(); await h.mount(); await h.handlers.claim();
  const draft = { ...config(), name: 'Retained offline draft' }; h.handlers.setDraft(draft);
  h.loseMetadata();
  assert.equal(h.handlers.module, undefined);
  assert.equal(h.handlers.editing, false);
  assert.equal(h.handlers.draft, draft);
  h.cleanup();
});

test('an uncertain Apply reply is surfaced once and leaves the last confirmed module intact without replay', async () => {
  const h = await harness(); await h.mount(); await h.handlers.claim(); h.calls.length = 0;
  const confirmed = h.handlers.module;
  h.custom.set('module_usb_config_apply', () => { throw Error('Apply reply lost; state unknown'); });
  assert.equal(await h.handlers.configure('config_apply_save', config()), false);
  assert.equal(h.calls.length, 1);
  assert.equal(h.handlers.module, confirmed);
  assert.match(h.handlers.error, /reply lost/);
  assert.equal(h.handlers.busy, false);
});

test('busy Apply rejects duplicate handler dispatch and live action replies do not overwrite an unsaved configuration draft', async () => {
  const h = await harness(); await h.mount(); await h.handlers.claim();
  const pending = deferred(); h.custom.set('module_usb_config_apply', () => pending.promise); h.calls.length = 0;
  const first = h.handlers.configure('config_apply', config());
  assert.equal(await h.handlers.configure('config_apply', config()), false);
  assert.equal(h.calls.length, 1);
  pending.resolve({ moduleUsb: { module: h.current } }); await first;
  const draft = { ...config(), name: 'Unsaved local draft' }; h.handlers.setDraft(draft); h.calls.length = 0;
  await h.handlers.invoke(action(), [true]);
  assert.equal(h.handlers.draft, draft);
  assert.deepEqual(h.calls.map(request => request.command), ['module_usb_actions_invoke']);
  assert.equal(h.calls[0].catalogueRevision, 7);
});

test('unmount releases an established setup claim with its current route and failed release remains visible until navigation', async () => {
  const h = await harness(); await h.mount(); await h.handlers.claim();
  h.custom.set('module_usb_release', () => { throw Error('Writer release reply unavailable'); }); h.calls.length = 0;
  await h.handlers.release();
  assert.equal(h.handlers.editing, true);
  assert.match(h.handlers.error, /release reply/);
  assert.equal(h.calls.length, 1);
  h.custom.delete('module_usb_release'); h.cleanup(); await settle();
  assert.equal(h.calls.filter(request => request.command === 'module_usb_release').length, 2);
  assert.deepEqual(h.calls.at(-1).route, route());
});

test('a successful claim arriving after navigation is immediately released using the returned route', async () => {
  const h = await harness(); await h.mount();
  const pending = deferred(); h.custom.set('module_usb_claim', () => pending.promise); h.calls.length = 0;
  const claiming = h.handlers.claim(); h.cleanup();
  const claimed = endpoint(); claimed.snapshot.claimedBy = 'operator'; claimed.snapshot.config.owner = 4;
  pending.resolve({ moduleUsb: { module: claimed } }); await claiming; await settle();
  assert.deepEqual(h.calls.map(request => request.command), ['module_usb_claim', 'module_usb_release']);
  assert.deepEqual(h.calls.at(-1).route, claimed.snapshot.route);
});

test('changed or lost setup ownership on Refresh ends local editing and prevents configuration without a new claim', async () => {
  for (const change of ['generation', 'ownership']) {
    const h = await harness(); await h.mount(); await h.handlers.claim();
    const fresh = plain(h.current);
    if (change === 'generation') fresh.snapshot.route.connectionGeneration++;
    else { fresh.snapshot.claimedBy = null; fresh.snapshot.config.owner = 0; }
    fresh.snapshot.actions.route = plain(fresh.snapshot.route); h.setCurrent(fresh); h.calls.length = 0;
    await h.refresh();
    assert.equal(h.handlers.editing, false, change);
    assert.equal(await h.handlers.configure('config_apply', config()), false);
    assert.deepEqual(h.calls.map(request => request.command), ['module_usb_status']);
  }
});

test('an Apply completed after navigation releases any returned writer using the newly acknowledged route', async () => {
  const h = await harness(); await h.mount(); await h.handlers.claim();
  const pending = deferred(); h.custom.set('module_usb_config_apply', () => pending.promise); h.calls.length = 0;
  const applying = h.handlers.configure('config_apply', config()); h.cleanup();
  const applied = endpoint(); applied.snapshot.claimedBy = 'operator'; applied.snapshot.config.owner = 4;
  applied.snapshot.route.configurationGeneration = 2; applied.snapshot.config.generation = 2; applied.snapshot.actions.route = plain(applied.snapshot.route);
  pending.resolve({ moduleUsb: { module: applied } }); await applying; await settle();
  assert.equal(h.calls.filter(request => request.command === 'module_usb_config_apply').length, 1);
  const releases = h.calls.filter(request => request.command === 'module_usb_release');
  assert.equal(releases.at(-1).route.configurationGeneration, 2);
});

test('USB details retire both readers while hidden and resume from a fresh baseline without losing the draft', async () => {
  const h = await harness();
  h.document.hidden = true; await h.mount();
  assert.equal(h.inputListeners.size, 0);
  assert.equal(h.metadataListeners.size, 0);
  assert.deepEqual(h.calls, []);
  const show = async visible => {
    h.document.hidden = !visible;
    h.document.dispatchEvent(new Event('visibilitychange'));
    await settle();
  };
  await show(true); h.publishMetadata(h.current);
  await h.handlers.claim();
  h.handlers.setDraft({ ...config(), name: 'Unsaved USB draft' });
  h.calls.length = 0;
  assert.equal(h.timers.size, 0);
  assert.equal(h.inputListeners.size, 1);
  assert.equal(h.metadataListeners.size, 1);
  await show(false);
  assert.equal(h.inputListeners.size, 0);
  assert.equal(h.metadataListeners.size, 0);
  assert.equal(h.handlers.editing, false);
  const replacement = plain(h.current);
  replacement.snapshot.route.connectionGeneration++;
  replacement.snapshot.config.config.name = 'Current USB configuration';
  h.setCurrent(replacement);
  await show(true);
  assert.equal(h.handlers.editing, false);
  h.publishMetadata(replacement);
  assert.equal(h.inputListeners.size, 1);
  assert.equal(h.metadataListeners.size, 1);
  assert.equal(h.handlers.module.snapshot.config.config.name, 'Current USB configuration');
  assert.equal(h.handlers.module.snapshot.route.connectionGeneration, replacement.snapshot.route.connectionGeneration);
  assert.equal(h.handlers.draft.name, 'Unsaved USB draft');
  assert.equal(h.handlers.editing, false);
  assert.deepEqual(h.calls, []);
  h.cleanup();
  assert.equal(h.inputListeners.size, 0);
  assert.equal(h.metadataListeners.size, 0);
  assert.equal(h.timers.size, 0);
});
