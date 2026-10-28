import assert from 'node:assert/strict';
import test from 'node:test';
import { readFile } from 'node:fs/promises';
import vm from 'node:vm';
import ts from 'typescript';
import { compile } from 'svelte/compiler';
import { render } from 'svelte/server';
import * as server from 'svelte/internal/server';
import * as svelte from 'svelte';
import { loadModules, libModuleName } from '../test-module-loader.mjs';

const modules = await loadModules(['./profile-canvas']);
const transpile = source => ts.transpileModule(source, {
  compilerOptions: { module: ts.ModuleKind.CommonJS, target: ts.ScriptTarget.ES2022 },
}).outputText;
const cache = new Map();

async function component(name) {
  const key = libModuleName(name, import.meta.url);
  if (cache.has(key)) return cache.get(key);
  const filename = new URL(name, import.meta.url);
  const { js } = compile(await readFile(filename, 'utf8'), { filename: filename.pathname, generate: 'server' });
  const dependencies = new Map();
  for (const match of js.code.matchAll(/from\s+['"]([^'"]+\.svelte)['"]/g)) {
    const dependency = libModuleName(match[1], filename);
    dependencies.set(dependency, { default: await component(dependency) });
  }
  const exports = {};
  vm.runInNewContext(transpile(js.code), {
    exports,
    TextEncoder,
    require(specifier) {
      const dependency = libModuleName(specifier, filename);
      if (dependency === 'svelte/internal/server') return server;
      if (dependency === 'svelte') return svelte;
      if (dependencies.has(dependency)) return dependencies.get(dependency);
      const key = dependency.replace(/\.ts$/, '');
      if (modules.has(key)) return modules.get(key);
      throw new Error(`Unexpected component dependency ${specifier}`);
    },
  }, { filename: filename.pathname });
  cache.set(key, exports.default);
  return exports.default;
}

await component('../Icon.svelte');
const source = await readFile(new URL('./ProfileIntegrationPorts.svelte', import.meta.url), 'utf8');
function harness({ ports = [], busy = false, error = '', readOnly = false, query = '', page = 0, direction = 'output', duringRender = () => {} } = {}) {
  const seeded = source.replace("let query=$state(''),page=$state(0),filterInput:HTMLInputElement;",
    `let query=$state(${JSON.stringify(query)}),page=$state(${page}),filterInput:HTMLInputElement;`)
    .replace('</script>', `captureIntegrationPorts({filter,clear,choose,setPage(value){page=value;},get state(){return{query,page};}});</script>`);
  assert.notEqual(seeded, source, 'The component state seed must match production source.');
  const compiled = compile(seeded, { filename: new URL('./ProfileIntegrationPorts.svelte', import.meta.url).pathname, generate: 'server' });
  const dependencies = new Map();
  const filename = new URL('./ProfileIntegrationPorts.svelte', import.meta.url);
  for (const match of compiled.js.code.matchAll(/from\s+['"]([^'"]+\.svelte)['"]/g)) {
    const dependency = libModuleName(match[1], filename);
    dependencies.set(dependency, { default: cache.get(dependency) });
  }
  const captured = {};
  const exports = {};
  vm.runInNewContext(transpile(compiled.js.code), {
    exports,
    TextEncoder,
    captureIntegrationPorts: value => { Object.assign(captured, value); duringRender(value); },
    require(specifier) {
      const dependency = libModuleName(specifier, filename);
      if (dependency === 'svelte/internal/server') return server;
      if (dependency === 'svelte') return svelte;
      if (dependencies.has(dependency)) return dependencies.get(dependency);
      const key = dependency.replace(/\.ts$/, '');
      if (modules.has(key)) return modules.get(key);
      throw new Error(`Unexpected component dependency ${specifier}`);
    },
  }, { filename: filename.pathname });
  const selected = [], retried = [];
  const body = render(exports.default, { props: {
    ports, busy, error, readOnly, direction,
    select: port => selected.push(port),
    retry: () => retried.push('retry'),
  } }).body;
  return { body, handlers: captured, selected, retried };
}

const state = (unit = null) => ({ delivery: 'state', schema: { type: 'number', min: 0, max: 100, unit }, freshnessMs: 1000, fallback: null });
const port = (id, name = id, direction = 'output', unit = null) => ({
  id, name, direction, contract: state(unit), key: `choice:${id}`, profilePortId: null,
  sourceId: 'game-source', observed: null, current: true,
});

test('renders the complete output catalogue as paged choices without adding declarations', () => {
  const ports = [...Array.from({ length: 40 }, (_, index) => port(`out-${index}`, `Output ${index}`)), port('input-speed', 'Input speed', 'input')];
  const { body } = harness({ ports });

  assert.equal((body.match(/class="port-tile/g) ?? []).length, 10);
  assert.match(body, /aria-label="Integration outputs"/);
  assert.match(body, /Output 0/);
  assert.match(body, /Output 9/);
  assert.doesNotMatch(body, /Output 10|Input speed/);
  assert.match(body, /1–10 \/ 40/);
});

test('input catalogue page shows automatic unpinned inputs without output choices', () => {
  const action = { delivery: 'action', semantics: 'setter', endpoint: 'light', parameters: [], readable: false, display: null, ownership: null };
  const ports = [port('state-output', 'Aircraft state'), { ...port('set-light', 'Set light', 'input'), contract: action }];
  const { body } = harness({ ports, direction: 'input' });
  assert.match(body, /aria-label="Integration inputs"/);
  assert.match(body, /Set light/);
  assert.doesNotMatch(body, /Aircraft state/);
  assert.match(body, /Filter inputs/);
  assert.match(body, /action · setter/);
});

test('input quick filter finds later catalogue entries beyond the first page', () => {
  const ports = [...Array.from({ length: 36 }, (_, index) => port(`in-${index}`, `Input ${index}`, 'input')),
    port('late-id', 'Tail command', 'input'), port('output-late', 'Tail command output', 'output')];
  const byName = harness({ ports, direction: 'input', query: 'Tail command' }).body;
  assert.match(byName, /Tail command/);
  assert.doesNotMatch(byName, /Input 0|Tail command output/);
  const byId = harness({ ports, direction: 'input', query: 'late-id' }).body;
  assert.match(byId, /Tail command/);
  assert.doesNotMatch(byId, /Tail command output/);
});

test('an output quick filter finds later-page names, IDs, and units while excluding input choices', () => {
  const ports = [...Array.from({ length: 36 }, (_, index) => port(`out-${index}`, `Output ${index}`)),
    port('late-id', 'Tail channel', 'output', 'km/h'), port('input-late', 'Tail channel input', 'input', 'km/h')];
  const byName = harness({ ports, query: 'Tail channel' }).body;
  assert.match(byName, /Tail channel/);
  assert.doesNotMatch(byName, /Output 0|Tail channel input/);
  const byId = harness({ ports, query: 'late-id' }).body;
  assert.match(byId, /Tail channel/);
  assert.doesNotMatch(byId, /Tail channel input/);
  const byUnit = harness({ ports, query: 'KM/H' }).body;
  assert.match(byUnit, /Tail channel/);
  assert.doesNotMatch(byUnit, /Tail channel input/);
});

test('loading and errors stay visible, retry is disabled while busy, and browsing remains passive', () => {
  const ports = [port('out-0')];
  const snapshots = [];
  const { body, selected, retried } = harness({ ports, busy: true, error: 'Catalogue refresh failed', readOnly: true, query: 'out', page: 4,
    duringRender(handlers) {
      handlers.filter({ currentTarget: { value: 'updated' } });
      snapshots.push(handlers.state.page);
      handlers.setPage(3);
      handlers.clear();
      snapshots.push(handlers.state.page);
      handlers.choose(ports[0]);
    },
  });

  assert.match(body, /Loading outputs…/);
  assert.match(body, /role="alert"/);
  assert.match(body, /Catalogue refresh failed/);
  assert.match(body, /class="retry[^\"]*" disabled=""/);
  assert.match(body, /class="port-tile[^"]*"[^>]*disabled/);
  assert.deepEqual(snapshots, [0, 0]);
  assert.deepEqual(selected, []);
  assert.deepEqual(retried, []);
  const retryEnabled = harness({ ports, error: 'Catalogue refresh failed' }).body;
  assert.match(retryEnabled, /class="retry[^\"]*"/);
  assert.doesNotMatch(retryEnabled.match(/<button[^>]*class="retry[^>]*>/)?.[0] ?? '', /disabled/);
});

test('select passes the exact choice once when enabled and is blocked while busy or read-only', () => {
  const choice = port('exact-output', 'Exact output');
  const enabled = harness({ ports: [choice] });
  enabled.handlers.choose(choice);
  assert.equal(enabled.selected.length, 1);
  assert.equal(enabled.selected[0], choice);

  const busy = harness({ ports: [choice], busy: true });
  busy.handlers.choose(choice);
  assert.deepEqual(busy.selected, []);
  const readOnly = harness({ ports: [choice], readOnly: true });
  readOnly.handlers.choose(choice);
  assert.deepEqual(readOnly.selected, []);
  const failed = harness({ ports: [choice], error: 'Catalogue changed; refresh failed.' });
  failed.handlers.choose(choice);
  assert.deepEqual(failed.selected, []);
  assert.match(failed.body, /class="port-tile[^"]*"[^>]*disabled/);
});


test('browsing pages does not select or persist anything', () => {
  const ports = Array.from({ length: 40 }, (_, index) => port(`input-${index}`, `Command ${index}`, 'input'));
  const { handlers, selected } = harness({ ports, direction: 'input' });
  handlers.setPage(1);
  assert.deepEqual(selected, []);
});
