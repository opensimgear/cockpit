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
  let current = controller(), handlers, cleanup;
  const calls = [], mounts = [], timers = new Map(), notices = [], custom = new Map();
  const injected = source.replace('</script>', `
    if (seeded) detail = seededResult;
    error = seededError; actionError = seededActionError;
    capture({refresh,mutate,select,get name(){return name;},get role(){return role;},get error(){return error;},get actionError(){return actionError;},get detail(){return detail;},setDraft(nextName,nextRole){name=nextName;role=nextRole;}});
  </script>`);
  const { js } = compile(injected, { filename: file.pathname, generate: 'server' }), exports = {};
  vm.runInNewContext(ts.transpileModule(js.code, { compilerOptions: { module: ts.ModuleKind.CommonJS, target: ts.ScriptTarget.ES2022 } }).outputText, {
    exports, seeded, seededResult: result(current), seededError: error, seededActionError: actionError,
    capture(value) { handlers = value; },
    setInterval(callback) { const id = timers.size + 1; timers.set(id, callback); return id; },
    clearInterval(id) { timers.delete(id); },
    require(name) {
      if (name === 'svelte/internal/server') return server;
      if (name === 'svelte') return { ...svelte, onMount(callback) { mounts.push(callback); } };
      if (name === './Notification.svelte') return { default(_payload, props) { notices.push(props.message); } };
      if (name.endsWith('.svelte')) return { default() {} };
      if (name === './controllers') return controllers;
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
  return { html, handlers, calls, timers, custom, notices,
    setCurrent(next) { current = next; },
    async mount() { cleanup = mounts[0](); await settle(); },
    cleanup() { cleanup?.(); },
  };
}

test('opening a tile initializes local fields from the inspected controller without saving settings', async () => {
  const h = harness();
  await h.mount();
  assert.equal(h.handlers.name, 'Desk pedals');
  assert.equal(h.handlers.role, 'pedals');
  assert.deepEqual(h.calls.map(call => call.command), ['controller_list', 'controller_inspect']);
  assert.equal(h.calls[1].sourceId, 'physical-pedals');
  h.cleanup();
  assert.equal(h.timers.size, 0);
});

test('live polling preserves a local draft and successful Save adopts the acknowledged fields', async () => {
  const h = harness();
  await h.mount();
  h.handlers.setDraft('Unsaved local name', 'throttle');
  h.setCurrent({ ...controller(), name: 'Provider name', role: 'wheelbase' });
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

test('live inputs collect every control page without exposing pagination', async () => {
  const h = harness(), controls = Array.from({ length: 70 }, (_, id) => ({ capability: { id: String(id), kind: 'button' }, value: null }));
  h.custom.set('controller_inspect', request => ({ controllers: { ...result(controller()), offset: request.offset, total: controls.length,
    controls: controls.slice(request.offset, request.offset + 32) } }));
  await h.mount();
  assert.deepEqual(h.calls.filter(call => call.command === 'controller_inspect').map(call => call.offset), [0, 32, 64]);
  assert.equal(h.handlers.detail.controls.length, 70);
  assert.equal(h.handlers.detail.controls.at(-1).capability.id, '69');
  assert.doesNotMatch(h.html, /Previous controls|Next controls/);
  await h.handlers.mutate({ command: 'controller_update', sourceId: controller().id, name: 'Updated', role: 'panel' });
  assert.equal(h.handlers.detail.controls.length, 70);
  h.cleanup();
});

test('a changed or incomplete control page cannot replace the complete displayed controls', async () => {
  for (const change of [page => ({ ...page, controls: [] }), page => ({ ...page, source: { ...page.source, generation: '2' } })]) {
    const h = harness(); await h.mount();
    const previous = h.handlers.detail;
    h.custom.set('controller_inspect', request => {
      const page = { ...result(controller()), offset: request.offset, total: 2, controls: [{ capability: { id: String(request.offset) }, value: null }] };
      return { controllers: request.offset ? change(page) : page };
    });
    await h.handlers.refresh();
    assert.equal(h.handlers.detail, previous);
    assert.match(h.handlers.error, /inputs changed while loading/);
    h.cleanup();
  }
});
