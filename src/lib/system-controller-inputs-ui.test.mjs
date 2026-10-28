import test from 'node:test';
import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import vm from 'node:vm';
import ts from 'typescript';
import { compile } from 'svelte/compiler';
import { render } from 'svelte/server';
import * as server from 'svelte/internal/server';
import * as svelte from 'svelte';
import { loadModules,libModuleName } from './test-module-loader.mjs';

const modules = await loadModules(['./controllers']);
const cache=new Map();
async function component(name){
  if(cache.has(name))return cache.get(name);
  const file=new URL(name,import.meta.url),{js}=compile(await readFile(file,'utf8'),{filename:file.pathname,generate:'server'}),dependencies=new Map();
  for(const match of js.code.matchAll(/from\s+['"]([^'"]+\.svelte)['"]/g)){const key=libModuleName(match[1],file);dependencies.set(key,{default:await component(key)});}
  const exports={};vm.runInNewContext(ts.transpileModule(js.code,{compilerOptions:{module:ts.ModuleKind.CommonJS,target:ts.ScriptTarget.ES2022}}).outputText,{
    exports,require(name){name=libModuleName(name,file);if(name==='svelte/internal/server')return server;if(name==='svelte')return svelte;if(dependencies.has(name))return dependencies.get(name);if(modules.has(name))return modules.get(name);throw Error('Unexpected input component import '+name);},
  },{filename:file.pathname});cache.set(name,exports.default);return exports.default;
}
const Inputs=await component('./SystemControllerInputs.svelte');
const html = (controls, online = true, simple = false) => render(Inputs, { props: { controls, online, simple } }).body;
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
  assert.match(rendered, /aria-label="X: 6553"/);
  assert.match(rendered, /6553/); assert.match(rendered, /Neutral/); assert.match(rendered, /Raw 7/);
  assert.doesNotMatch(rendered, /aria-label="Buttons"/);
});

test('simple profile readout keeps live values and accessible square buttons without detail metadata', () => {
  const rendered = html([
    control('x', 'axis', { native: 20, normalized: 6553 }, 48),
    control('hat', 'hat', { native: 8, normalized: null }, 57),
    control('raw', 'diagnostic', { native: 7, normalized: null }, 99),
    control('pressed', 'button', { native: 1, normalized: 1 }, 1),
    control('waiting', 'button', null, 2),
  ], true, true);
  assert.match(rendered, /aria-label="Axes, hats and other inputs"/);
  assert.match(rendered, /aria-label="X: 6553"/);
  assert.match(rendered, />6553</);
  assert.match(rendered, />Neutral</);
  assert.match(rendered, />Raw 7</);
  assert.match(rendered, /class="button-indicator[^"]*pressed"[^>]*role="img" aria-label="Button 1: Pressed"/);
  assert.match(rendered, /class="button-indicator[^"]*waiting"[^>]*role="img" aria-label="Button 2: Waiting for report"/);
  assert.doesNotMatch(rendered, /<button\b|aria-describedby=/);
  assert.doesNotMatch(rendered, /Report 2|Native 1|range -100/);
});

test('empty devices retain the appropriate online and offline guidance', () => {
  assert.match(html([]), /No input controls reported/);
  assert.match(html([], false), /appear when this controller reconnects/);
});

test('physical hats distinguish a missing report, neutral and malformed normalized values', () => {
  const rendered = html([
    control('waiting', 'hat', null),
    control('neutral', 'hat', { native: 8, normalized: null }),
    control('east', 'hat', { native: 2, normalized: 2 }),
    control('invalid', 'hat', { native: -1, normalized: -1 }),
  ], true, true);
  assert.match(rendered, /aria-label="Hat 1: Waiting for report"/);
  assert.match(rendered, /aria-label="Hat 2: Neutral"/);
  assert.match(rendered, /aria-label="Hat 3: E"/);
  assert.match(rendered, /aria-label="Hat 4: Unknown"/);
  assert.doesNotMatch(rendered, /<button\b/);
});
