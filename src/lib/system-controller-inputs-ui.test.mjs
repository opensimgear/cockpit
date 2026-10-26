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
const file = new URL('./SystemControllerInputs.svelte', import.meta.url), source = await readFile(file, 'utf8');
const { js } = compile(source, { filename: file.pathname, generate: 'server' }), exports = {};
vm.runInNewContext(ts.transpileModule(js.code, { compilerOptions: { module: ts.ModuleKind.CommonJS, target: ts.ScriptTarget.ES2022 } }).outputText, {
  exports, require(name) {
    if (name === 'svelte/internal/server') return server;
    if (name === 'svelte') return svelte;
    if (name === './controllers') return controllers;
    throw Error('Unexpected input component import ' + name);
  },
}, { filename: file.pathname });
const html = (controls, online = true) => render(exports.default, { props: { controls, online } }).body;
const control = (id, kind, value, usage = 1) => ({ capability: { id, kind, usage, usagePage: 1,
  reportId: 2, collection: 3, dataIndex: 4, min: -100, max: 100 }, value });

test('input readouts preserve button state and metadata, including missing reports', () => {
  const rendered = html([control('pressed', 'button', { native: 1, normalized: 1 }), control('waiting', 'button', null, 2)]);
  assert.match(rendered, /aria-label="Button 1: Pressed"/);
  assert.match(rendered, /aria-label="Button 2: Waiting for report"/);
  assert.match(rendered, /Report 2 · collection 3 · field 4/);
  assert.match(rendered, /Native 1 · range -100…100/);
  assert.match(rendered, /Native — · range -100…100/);
  assert.match(rendered, /aria-describedby="[^"]+"/);
});

test('axes, hats and diagnostics remain available even without buttons', () => {
  const rendered = html([control('x', 'axis', { native: 20, normalized: 6553 }, 48),
    control('hat', 'hat', { native: 8, normalized: null }, 57), control('raw', 'diagnostic', { native: 7, normalized: null }, 99)]);
  assert.match(rendered, /aria-label="Axes, hats and other inputs"/);
  assert.match(rendered, /aria-label="X"/);
  assert.match(rendered, /6553/); assert.match(rendered, /Neutral/); assert.match(rendered, /Raw 7/);
  assert.doesNotMatch(rendered, /aria-label="Buttons"/);
});

test('empty devices retain the appropriate online and offline guidance', () => {
  assert.match(html([]), /No input controls reported/);
  assert.match(html([], false), /appear when this controller reconnects/);
});
