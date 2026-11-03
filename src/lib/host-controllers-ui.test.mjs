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
const controller = (id = 'physical-pedals', name = 'Desk pedals') => ({ id, name, role: 'pedals', origin: 'host-controller', backend: 'windows-raw-input',
  vendorId: 1, productId: 2, osgHid: false, online: true, generation: '1', sequence: '0', controlCount: 0,
  baselineReady: false, identityConfidence: 'serial' });
const result = current => ({ source: current, sources: [current], controls: [], offset: 0, total: 0, limit: 256 });
const inventory = sources => ({ sources, offset: 0, total: sources.length, limit: 256, backendStatus: 'ready' });

function harness({ seeded = false, detailsOnly = true, error = '', actionError = '' } = {}) {
  let current = controller(), handlers, cleanup, effect, stopEffect, effectKey;
  const calls = [], mounts = [], notices = [], custom = new Map(), inputObservers = [], inventoryObservers = [];
  const injected = source.replace('$effect(() => {', 'registerEffect(() => {').replace('</script>', `
    if (seeded) detail = seededResult;
    error = seededError; actionError = seededActionError;
    capture({mutate,select,get selected(){return selected;},get name(){return name;},get role(){return role;},get error(){return error;},get actionError(){return actionError;},get detail(){return detail;},get inventory(){return inventory;},get inventoryState(){return inventoryState;},get inventoryError(){return inventoryError;},get inventoryFresh(){return inventoryState.status === 'ready';},get visibleSources(){return (inventory.sources ?? []).slice(page,page+32);},setPage(next){page=next;},setDraft(nextName,nextRole){name=nextName;role=nextRole;}});
  </script>`);
  const { js } = compile(injected, { filename: file.pathname, generate: 'server' }), exports = {};
  vm.runInNewContext(ts.transpileModule(js.code, { compilerOptions: { module: ts.ModuleKind.CommonJS, target: ts.ScriptTarget.ES2022 } }).outputText, {
    exports, seeded, seededResult: result(current), seededError: error, seededActionError: actionError,
    capture(value) { handlers = value; },
    registerEffect(callback) { effect = callback; },
    require(name) {
      if (name === 'svelte/internal/server') return server;
      if (name === 'svelte') return { ...svelte, onMount(callback) { mounts.push(callback); } };
      if (name === './Notification.svelte') return { default(_payload, props) { notices.push(props.message); } };
      if (name.endsWith('.svelte')) return { default() {} };
      if (name === './controllers') return controllers;
      if (name === './controller-read-models/shared') return { observeControllerInventory(listener) {
        const observer = { listener, stopped: false }; inventoryObservers.push(observer); listener({ status: 'connecting' });
        return () => { observer.stopped = true; };
      } };
      if (name === './controller-observations') return { observeControllerInputs(id, listener) {
        const observer = { id, listener, stopped: false }; inputObservers.push(observer);
        listener({ status: 'connecting' }); return () => { observer.stopped = true; };
      } };
      if (name === './gateway') return { gatewayRequest: async request => {
        calls.push(plain(request));
        if (custom.has(request.command)) return custom.get(request.command)(request);
        if (request.command === 'controller_update') current = { ...current, name: request.name, role: request.role };
        return { controllers: { ...result(plain(current)), source: plain(current) } };
      } };
      throw Error('Unexpected import ' + name);
    },
  }, { filename: file.pathname });
  const html = render(exports.default, { props: { initialSourceId: current.id, detailsOnly } }).body;
  function flush() {
    if (effectKey === handlers.selected) return;
    stopEffect?.(); effectKey = handlers.selected; stopEffect = effect();
  }
  return { html, handlers, calls, custom, notices, inputObservers, inventoryObservers, flush,
    emitInput(state) { inputObservers.at(-1).listener(state); },
    emitInventory(state) { inventoryObservers.at(-1).listener(state); },
    setCurrent(next) { current = next; },
    async mount() { cleanup = mounts[0](); flush(); await settle(); },
    cleanup() { cleanup?.(); stopEffect?.(); },
  };
}

test('mount subscribes to inventory and selected input independently without controller_list queries', async () => {
  const h = harness();
  await h.mount();
  h.emitInventory({ status: 'ready', value: inventory([controller()]), revision: '1' });
  h.emitInput({ status: 'ready', detail: result(controller()) });
  assert.equal(h.handlers.name, 'Desk pedals');
  assert.equal(h.handlers.role, 'pedals');
  assert.deepEqual(h.calls, []);
  assert.equal(h.inventoryObservers.length, 1);
  assert.equal(h.inputObservers[0].id, 'physical-pedals');
  h.cleanup();
  assert.equal(h.inventoryObservers[0].stopped, true);
});

test('inventory metadata replaces catalogue rows while paging locally in 32-row slices', async () => {
  const h = harness({ detailsOnly: false }), sources = Array.from({ length: 70 }, (_, index) => controller(`controller-${index}`, `Controller ${index}`));
  await h.mount();
  h.emitInventory({ status: 'ready', value: inventory(sources), revision: '1' });
  assert.equal(h.handlers.visibleSources.length, 32);
  assert.equal(h.handlers.visibleSources[0].id, 'controller-0');
  h.handlers.setPage(32);
  assert.equal(h.handlers.visibleSources.length, 32);
  assert.equal(h.handlers.visibleSources[0].id, 'controller-32');
  const replaced = sources.map((source, index) => index === 32 ? { ...source, name: 'Metadata refreshed' } : source);
  h.emitInventory({ status: 'ready', value: inventory(replaced), revision: '2' });
  assert.equal(h.handlers.visibleSources[0].name, 'Metadata refreshed');
  h.handlers.setPage(64);
  assert.equal(h.handlers.visibleSources.length, 6);
  assert.deepEqual(h.calls, []);
  h.cleanup();
});

test('inventory loss retains and labels cached rows, and freshness gates mutations', async () => {
  const h = harness(); await h.mount();
  h.emitInventory({ status: 'ready', value: inventory([controller()]), revision: '1' });
  h.emitInventory({ status: 'lost', value: inventory([controller()]), revision: '1', error: 'Continuity lost' });
  assert.equal(h.handlers.inventory.sources[0].id, 'physical-pedals');
  assert.equal(h.handlers.inventoryFresh, false);
  assert.match(h.handlers.inventoryState.error, /Continuity lost/);
  await h.handlers.mutate({ command: 'controller_update', sourceId: 'physical-pedals', name: 'Draft', role: 'pedals' });
  assert.deepEqual(h.calls, []);
  h.cleanup();
});

test('opening a tile initializes fields from input observation without saving settings', async () => {
  const h = harness(); await h.mount();
  h.emitInventory({ status: 'ready', value: inventory([controller()]), revision: '1' });
  h.emitInput({ status: 'ready', detail: result(controller()) });
  assert.equal(h.handlers.name, 'Desk pedals');
  assert.equal(h.handlers.role, 'pedals');
  assert.deepEqual(h.calls, []);
});

test('live input preserves a local draft and Save adopts acknowledged fields without replacing inventory', async () => {
  const h = harness(); await h.mount();
  h.emitInventory({ status: 'ready', value: inventory([controller()]), revision: '1' });
  h.emitInput({ status: 'ready', detail: result(controller()) });
  h.handlers.setDraft('Unsaved local name', 'throttle');
  h.setCurrent({ ...controller(), name: 'Provider name', role: 'wheelbase' });
  h.emitInput({ status: 'ready', detail: result(controller()) });
  h.handlers.setDraft('Unsaved local name', 'throttle');
  await h.handlers.mutate({ command: 'controller_update', sourceId: 'physical-pedals', name: h.handlers.name, role: h.handlers.role });
  await settle();
  assert.equal(h.handlers.name, 'Unsaved local name');
  assert.equal(h.handlers.role, 'throttle');
  assert.equal(h.handlers.inventory.sources[0].name, 'Desk pedals');
  assert.equal(h.calls.filter(call => call.command === 'controller_update').length, 1);
  h.cleanup();
});

test('embedded settings keep outer title and navigation in charge', () => {
  const embedded = harness({ seeded: true });
  assert.match(embedded.html, /Controller settings/);
  assert.doesNotMatch(embedded.html, />Close<\/button>/);
  assert.doesNotMatch(embedded.html, />Desk pedals<\/h[123]>/);
  assert.match(embedded.html, /Local name/);
  const standalone = harness({ seeded: true, detailsOnly: false });
  assert.match(standalone.html, />Close<\/button>/);
  assert.match(standalone.html, />Desk pedals<\/h2>/);
});

test('Save failures notify and preserve the current draft', async () => {
  const seeded = harness({ error: 'Inspection unavailable', actionError: 'Save rejected' });
  assert.deepEqual(seeded.notices, ['Inspection unavailable', 'Save rejected']);
  const h = harness(); await h.mount();
  h.emitInventory({ status: 'ready', value: inventory([controller()]), revision: '1' });
  h.handlers.setDraft('Draft survives', 'pedals');
  h.custom.set('controller_update', () => { throw Error('Save rejected'); });
  await h.handlers.mutate({ command: 'controller_update', sourceId: 'physical-pedals', name: 'Draft survives', role: 'pedals' });
  await settle();
  assert.match(h.handlers.actionError, /Save rejected/);
  assert.equal(h.handlers.name, 'Draft survives');
  assert.equal(h.calls.filter(call => call.command === 'controller_update').length, 1);
  h.cleanup();
});

test('input stream remains separate from inventory and late selected-source events are cancelled', async () => {
  const h = harness(); await h.mount();
  h.emitInventory({ status: 'ready', value: inventory([controller(), controller('other-controller', 'Other')]), revision: '1' });
  h.emitInput({ status: 'ready', detail: result(controller()) });
  const oldObserver = h.inputObservers[0], originalDetail = h.handlers.detail;
  h.handlers.select(controller()); h.flush();
  assert.equal(h.handlers.detail, originalDetail);
  assert.equal(h.inputObservers.length, 1);
  const next = controller('other-controller', 'Other');
  h.handlers.select(next); h.flush();
  assert.equal(oldObserver.stopped, true);
  assert.equal(h.handlers.detail, null);
  oldObserver.listener({ status: 'ready', detail: result(controller()) });
  assert.equal(h.handlers.detail, null);
  h.emitInput({ status: 'ready', detail: result(next) });
  assert.equal(h.handlers.detail.source.id, next.id);
  h.emitInput({ status: 'lost', error: 'Continuity lost', detail: result({ ...next, online: false, baselineReady: false }) });
  assert.equal(h.handlers.detail.source.online, false);
  assert.equal(h.handlers.error, 'Continuity lost');
  h.cleanup();
  assert.equal(h.inputObservers.at(-1).stopped, true);
  assert.equal(h.inventoryObservers[0].stopped, true);
});
