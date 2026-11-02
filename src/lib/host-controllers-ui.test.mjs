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

const controllers = (await loadModules(['./controllers'])).get('./controllers');
const file = new URL('./HostControllers.svelte', import.meta.url);
const source = await readFile(file, 'utf8');
const plain = value => JSON.parse(JSON.stringify(value));
const settle = () => new Promise(resolve => setImmediate(resolve));
const controller = () => ({ id: 'physical-pedals', name: 'Desk pedals', role: 'pedals', origin: 'host-controller', backend: 'windows-raw-input',
  vendorId: 1, productId: 2, osgHid: false, online: true, generation: '1', sequence: '2', controlCount: 0,
  baselineReady: true, identityConfidence: 'serial' });
const result = current => ({ source: current, sources: [current], controls: [], offset: 0, total: 0, limit: 32 });

function harness({ seeded = false, detailsOnly = true, error = '', actionError = '' } = {}) {
  let current = controller(), handlers, cleanup, effect, stopEffect, effectKey;
  const calls = [], mounts = [], timers = new Map(), notices = [], custom = new Map(), observers = [];
  const injected = source.replace('$effect(() => {', 'registerEffect(() => {').replace('</script>', `
    if (seeded) detail = seededResult;
    error = seededError; actionError = seededActionError;
    capture({refresh,mutate,select,get selected(){return selected;},get name(){return name;},get role(){return role;},get error(){return error;},get actionError(){return actionError;},get detail(){return detail;},setDraft(nextName,nextRole){name=nextName;role=nextRole;}});
  </script>`);
  const { js } = compile(injected, { filename: file.pathname, generate: 'server' }), exports = {};
  vm.runInNewContext(ts.transpileModule(js.code, { compilerOptions: { module: ts.ModuleKind.CommonJS, target: ts.ScriptTarget.ES2022 } }).outputText, {
    exports, seeded, seededResult: result(current), seededError: error, seededActionError: actionError,
    capture(value) { handlers = value; },
    registerEffect(callback) { effect = callback; },
    setInterval(callback) { const id = timers.size + 1; timers.set(id, callback); return id; },
    clearInterval(id) { timers.delete(id); },
    require(name) {
      if (name === 'svelte/internal/server') return server;
      if (name === 'svelte') return { ...svelte, onMount(callback) { mounts.push(callback); } };
      if (name === './Notification.svelte') return { default(_payload, props) { notices.push(props.message); } };
      if (name.endsWith('.svelte')) return { default() {} };
      if (name === './controllers') return controllers;
      if (name === './controller-observations') return { observeControllerInputs(id, listener) {
        const observer = { id, listener, stopped: false }; observers.push(observer);
        listener({ status: 'connecting' }); return () => { observer.stopped = true; };
      } };
      if (name === './gateway') return { gatewayRequest: async request => {
        calls.push(plain(request));
        if (custom.has(request.command)) return custom.get(request.command)(request);
        if (request.command === 'controller_update') current = { ...current, name: request.name, role: request.role };
        return { controllers: result(plain(current)) };
      } };
      throw Error('Unexpected import ' + name);
    },
  }, { filename: file.pathname });
  const html = render(exports.default, { props: { initialSourceId: current.id, detailsOnly } }).body;
  function flush() {
    if (effectKey === handlers.selected) return;
    stopEffect?.(); effectKey = handlers.selected; stopEffect = effect();
  }
  return { html, handlers, calls, timers, custom, notices, observers, flush,
    emit(state) { observers.at(-1).listener(state); },
    setCurrent(next) { current = next; },
    async mount() { cleanup = mounts[0](); flush(); await settle(); },
    cleanup() { cleanup?.(); stopEffect?.(); },
  };
}

test('opening a tile observes the selected controller and initializes fields without saving settings', async () => {
  const h = harness();
  await h.mount();
  h.emit({ status: 'ready', detail: result(controller()) });
  assert.equal(h.handlers.name, 'Desk pedals');
  assert.equal(h.handlers.role, 'pedals');
  assert.deepEqual(h.calls.map(call => call.command), ['controller_list']);
  assert.equal(h.observers[0].id, 'physical-pedals');
  h.cleanup();
  assert.equal(h.timers.size, 0);
});

test('live observations preserve a local draft and successful Save adopts the acknowledged fields', async () => {
  const h = harness();
  await h.mount();
  h.emit({ status: 'ready', detail: result(controller()) });
  h.handlers.setDraft('Unsaved local name', 'throttle');
  h.setCurrent({ ...controller(), name: 'Provider name', role: 'wheelbase' });
  h.emit({ status: 'ready', detail: result(controller()) });
  await h.handlers.refresh();
  assert.equal(h.handlers.name, 'Unsaved local name');
  assert.equal(h.handlers.role, 'throttle');
  await h.handlers.mutate({ command: 'controller_update', sourceId: 'physical-pedals', name: h.handlers.name, role: h.handlers.role });
  await settle();
  assert.equal(h.handlers.name, 'Unsaved local name');
  assert.equal(h.handlers.role, 'throttle');
  assert.equal(h.calls.filter(call => call.command === 'controller_update').length, 1);
  h.cleanup();
});

test('embedded controller settings keep the outer device title and navigation in charge', () => {
  const embedded = harness({ seeded: true });
  assert.match(embedded.html, /Controller settings/);
  assert.doesNotMatch(embedded.html, />Close<\/button>/);
  assert.doesNotMatch(embedded.html, />Desk pedals<\/h[123]>/);
  assert.match(embedded.html, /Local name/);
  const standalone = harness({ seeded: true, detailsOnly: false });
  assert.match(standalone.html, />Close<\/button>/);
  assert.match(standalone.html, />Desk pedals<\/h2>/);
});

test('embedded provider and Save failures reach notifications and keep the existing draft', async () => {
  const seeded = harness({ error: 'Inspection unavailable', actionError: 'Save rejected' });
  assert.deepEqual(seeded.notices, ['Inspection unavailable', 'Save rejected']);
  const h = harness(); await h.mount();
  h.handlers.setDraft('Draft survives', 'pedals');
  h.custom.set('controller_update', () => { throw Error('Save rejected'); });
  await h.handlers.mutate({ command: 'controller_update', sourceId: 'physical-pedals', name: 'Draft survives', role: 'pedals' });
  await settle();
  assert.match(h.handlers.actionError, /Save rejected/);
  assert.equal(h.handlers.name, 'Draft survives');
  assert.equal(h.calls.filter(call => call.command === 'controller_update').length, 1);
  h.cleanup();
});

test('one complete stream update supplies all controls and metadata Save cannot replace newer input', async () => {
  const h = harness(), controls = Array.from({ length: 70 }, (_, id) => ({ capability: { id: String(id), kind: 'button' }, value: null }));
  await h.mount();
  h.emit({ status: 'ready', detail: { ...result(controller()), controls, total: controls.length } });
  assert.equal(h.handlers.detail.controls.length, 70);
  assert.equal(h.handlers.detail.controls.at(-1).capability.id, '69');
  assert.doesNotMatch(h.html, /Previous controls|Next controls/);
  let resolve;
  h.custom.set('controller_update', () => new Promise(done => { resolve = done; }));
  const save = h.handlers.mutate({ command: 'controller_update', sourceId: controller().id, name: 'Updated', role: 'panel' });
  const newer = { ...result(controller()), controls: [{ ...controls[0], value: { native: 1, normalized: 1 } }], total: 1 };
  h.emit({ status: 'ready', detail: newer });
  resolve({ controllers: result(controller()) });
  await save;
  assert.equal(h.handlers.detail, newer);
  assert.equal(h.handlers.detail.controls[0].value.native, 1);
  assert.ok(h.calls.every(call => call.command !== 'controller_inspect'));
  h.cleanup();
});

test('selection change disposes the stream, fences late events and loss removes available values', async () => {
  const h = harness(); await h.mount();
  const first = h.observers[0];
  h.emit({ status: 'ready', detail: result(controller()) });
  const quietDetail = h.handlers.detail;
  h.handlers.select(controller()); h.flush();
  assert.equal(h.handlers.detail, quietDetail);
  assert.equal(h.observers.length, 1);
  const next = { ...controller(), id: 'other-controller', name: 'Other' };
  h.handlers.select(next); h.flush();
  assert.equal(first.stopped, true);
  assert.equal(h.handlers.detail, null);
  first.listener({ status: 'ready', detail: result(controller()) });
  assert.equal(h.handlers.detail, null);
  h.emit({ status: 'ready', detail: result(next) });
  assert.equal(h.handlers.detail.source.id, next.id);
  h.emit({ status: 'lost', error: 'Continuity lost', detail: result({ ...next, online: false, baselineReady: false }) });
  assert.equal(h.handlers.detail.source.online, false);
  assert.equal(h.handlers.error, 'Continuity lost');
  await h.handlers.refresh();
  assert.equal(h.handlers.error, 'Continuity lost');
  h.cleanup();
  assert.equal(h.observers.at(-1).stopped, true);
});
