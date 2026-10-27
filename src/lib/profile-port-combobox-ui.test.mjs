import assert from 'node:assert/strict';
import test from 'node:test';
import { readFile } from 'node:fs/promises';
import vm from 'node:vm';
import ts from 'typescript';
import { compile } from 'svelte/compiler';
import { render } from 'svelte/server';
import * as server from 'svelte/internal/server';
import * as svelte from 'svelte';

const source = await readFile(new URL('./ProfilePortCombobox.svelte', import.meta.url), 'utf8');

async function view(props = {}) {
  const components = new Map();
  let handlers;
  async function component(name) {
    if (components.has(name)) return components.get(name);
    const filename = new URL(name, import.meta.url);
    let text = await readFile(filename, 'utf8');
    if (name === './ProfilePortCombobox.svelte') {
      text = text.replace('</script>', `capture({get matches(){return matches},input,select,keydown,get value(){return value},get query(){return query},get activeIndex(){return activeIndex}});</script>`);
    }
    const { js } = compile(text, { filename: filename.pathname, generate: 'server' });
    const dependencies = new Map();
    for (const match of js.code.matchAll(/from\s+['"]([^'"]+\.svelte)['"]/g)) {
      dependencies.set(match[1], { default: await component(match[1]) });
    }
    const exports = {};
    vm.runInNewContext(ts.transpileModule(js.code, {
      compilerOptions: { module: ts.ModuleKind.CommonJS, target: ts.ScriptTarget.ES2022 },
    }).outputText, {
      exports,
      TextEncoder,
      capture(value) { handlers = value; },
      require(dependency) {
        if (dependency === 'svelte/internal/server') return server;
        if (dependency === 'svelte') return svelte;
        if (dependencies.has(dependency)) return dependencies.get(dependency);
        throw new Error(`Unexpected combobox dependency ${dependency}`);
      },
    }, { filename: filename.pathname });
    components.set(name, exports.default);
    return exports.default;
  }
  const Component = await component('./ProfilePortCombobox.svelte');
  const body = render(Component, { props: { label: 'Input', options: [], ...props } }).body;
  return { body, handlers };
}

const options = [
  { value: 'port-a', label: 'Port Alpha' },
  { value: 'port-b', label: 'Port Beta' },
];

test('None is first in actual matches, survives filtering, and does not duplicate empty options', async () => {
  const h = await view({ options: [{ value: '', label: 'Legacy empty' }, ...options], allowEmpty: true, query: 'beta' });
  assert.deepEqual(Array.from(h.handlers.matches, option => option.value), ['', 'port-b']);
  h.handlers.input({ currentTarget: { value: 'beta' } });
  assert.equal(h.handlers.activeIndex, 1);
});

test('typing then Enter selects the real match; choosing None emits the empty value', async () => {
  const selected = [];
  const h = await view({ options, allowEmpty: true, query: 'beta', onselect: value => selected.push(value) });
  h.handlers.input({ currentTarget: { value: 'beta' } });
  let prevented = false;
  h.handlers.keydown({ key: 'Enter', preventDefault() { prevented = true; } });
  assert.equal(prevented, true);
  assert.deepEqual(selected, ['port-b']);
  h.handlers.select(h.handlers.matches[0]);
  assert.deepEqual(selected, ['port-b', '']);
  assert.equal(h.handlers.value, '');
  assert.equal(h.handlers.query, '');
});

test('empty value keeps the placeholder blank even when options contain a legacy empty label', async () => {
  const h = await view({ options: [{ value: '', label: 'Legacy empty' }, ...options], allowEmpty: true, value: '' });
  assert.match(h.body, /placeholder="Search ports by name or ID"/);
  assert.match(h.body, /value=""/);
  assert.doesNotMatch(h.body, /value="Legacy empty"/);
});

test('empty removable fields show X only when both the flag and callback are present', async () => {
  const onclear = () => {};
  const shown = await view({ onclear, showClearWhenEmpty: true, clearLabel: 'Remove Input 3 input' });
  assert.match(shown.body, /aria-label="Remove Input 3 input"/);
  assert.match(shown.body, /class="[^"]*\bclear-assignment\b/);
  const noCallback = await view({ showClearWhenEmpty: true });
  assert.doesNotMatch(noCallback.body, /class="[^"]*\bclear-assignment\b/);
  const flagOff = await view({ onclear });
  assert.doesNotMatch(flagOff.body, /class="[^"]*\bclear-assignment\b/);
});

test('disabled selection does not emit a selection callback', async () => {
  const selected = [];
  const h = await view({ options, disabled: true, onselect: value => selected.push(value) });
  h.handlers.select(options[0]);
  assert.deepEqual(selected, []);
  assert.equal(h.handlers.value, '');
});
