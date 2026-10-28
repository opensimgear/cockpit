import test from 'node:test';
import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import vm from 'node:vm';
import ts from 'typescript';
import { compile } from 'svelte/compiler';
import { render } from 'svelte/server';
import * as server from 'svelte/internal/server';
import * as svelte from 'svelte';
import { loadModules, libModuleName } from '../test-module-loader.mjs';

const modules = await loadModules(['./input-visuals/model']);
const cache = new Map();
async function component(name) {
  if (cache.has(name)) return cache.get(name);
  const file = new URL(`../${name.slice(2)}`, import.meta.url);
  const { js } = compile(await readFile(file, 'utf8'), { filename: file.pathname, generate: 'server' });
  const dependencies = new Map();
  for (const match of js.code.matchAll(/from\s+['"]([^'"]+\.svelte)['"]/g)) {
    const key = libModuleName(match[1], file);
    dependencies.set(key, { default: await component(key) });
  }
  const exports = {};
  vm.runInNewContext(ts.transpileModule(js.code, {
    compilerOptions: { module: ts.ModuleKind.CommonJS, target: ts.ScriptTarget.ES2022 },
  }).outputText, { exports, require(name) {
    name = libModuleName(name, file);
    if (name === 'svelte/internal/server') return server;
    if (name === 'svelte') return svelte;
    if (dependencies.has(name)) return dependencies.get(name);
    if (modules.has(name)) return modules.get(name);
    throw Error('Unexpected visual component import ' + name);
  } }, { filename: file.pathname });
  cache.set(name, exports.default);
  return exports.default;
}
const Axis = await component('./input-visuals/Axis.svelte');
const Hat = await component('./input-visuals/Hat.svelte');
const axis = props => render(Axis, { props: { label: 'Throttle', controlId: 'axis-id', min: -100, max: 100, value: 50, ...props } }).body;
const hat = props => render(Hat, { props: { label: 'View', controlId: 'hat-id', value: 0, ...props } }).body;
const tags = (html, name) => [...html.matchAll(new RegExp(`<${name}\\b[^>]*>`, 'g'))].map(match => match[0]);

test('axis readout and slider share frame, fill, marker and zero-reference geometry', () => {
  const readonly = axis({});
  const interactive = axis({ interactive: true });
  for (const html of [readonly, interactive]) {
    assert.match(html, /data-control-id="axis-id"/);
    assert.match(html, /title="Throttle"/);
    assert.match(html, />50<\/span>/);
    assert.match(html, /class="fill[^\"]*" style="left: 50%; width: 25%;"/);
    assert.match(html, /class="zero[^\"]*" style="left: 50%;"/);
    assert.match(html, /class="marker[^\"]*" style="left: 75%;"/);
  }
  assert.doesNotMatch(readonly, /<input\b/);
  assert.match(readonly, /role="img" aria-label="Throttle: 50"/);
  assert.match(interactive, /type="range" min="-100" max="100" step="1" value="50"/);
  assert.match(interactive, /aria-label="Throttle" aria-valuetext="50"/);
});

test('axis slider clamps its native value and marker while retaining report text and disabled state', () => {
  for (const [value, position, clamped] of [[-200, 0, -100], [200, 100, 100]]) {
    const html = axis({ value, interactive: true, disabled: true });
    assert.match(html, new RegExp(`class="marker[^\"]*" style="left: ${position}%;"`));
    assert.match(html, new RegExp(`value="${clamped}" disabled="" aria-label="Throttle" aria-valuetext="${value}"`));
  }
  assert.doesNotMatch(axis({ value: 25, min: 0, max: 100 }), /class="zero/);
  for (const [value, text] of [[undefined, 'Waiting for report'], [null, 'Unknown'], [NaN, 'Unknown']]) {
    const html = axis({ value });
    assert.match(html, new RegExp(`aria-label="Throttle: ${text}"`));
    assert.doesNotMatch(html, /class="marker|class="fill/);
  }
});

test('hat readout and interactive pad render the same nine cells for every direction and neutral', () => {
  for (const [value, text] of [[0, 'N'], [1, 'NE'], [2, 'E'], [3, 'SE'], [4, 'S'], [5, 'SW'], [6, 'W'], [7, 'NW'], [-1, 'Neutral'], [null, 'Neutral']]) {
    const readonly = hat({ value });
    const interactive = hat({ value, interactive: true });
    assert.match(readonly, new RegExp(`role="img" aria-label="View: ${text}"`));
    assert.match(interactive, new RegExp(`role="group" aria-label="View: ${text}"`));
    assert.equal(tags(readonly, 'span').filter(tag => /class="cell/.test(tag)).length, 9);
    assert.equal(tags(readonly, 'span').filter(tag => /class="cell[^\"]*selected/.test(tag)).length, 1);
    const buttons = tags(interactive, 'button');
    assert.equal(buttons.length, 9);
    assert.equal(buttons.filter(tag => /aria-pressed="true"/.test(tag)).length, 1);
    assert.match(buttons.find(tag => /aria-pressed="true"/.test(tag)), new RegExp(`aria-label="View: ${text}"`));
    assert.doesNotMatch(readonly, /<button\b|<input\b|<select\b/);
  }
});

test('waiting and unknown hats select no direction; disabled controls retain accessible state', () => {
  for (const [value, text] of [[undefined, 'Waiting for report'], [8, 'Unknown'], [.5, 'Unknown'], [NaN, 'Unknown']]) {
    for (const interactive of [false, true]) {
      const html = hat({ value, interactive, disabled: true });
      assert.match(html, new RegExp(`aria-label="View: ${text}"`));
      assert.doesNotMatch(html, /class="cell[^\"]*selected|aria-pressed="true"/);
      if (interactive) assert.equal(tags(html, 'button').filter(tag => / disabled="" /.test(tag)).length, 9);
    }
  }
  const selected = tags(hat({ value: 2, interactive: true, disabled: true }), 'button').find(tag => /aria-pressed="true"/.test(tag));
  assert.match(selected, /disabled="" aria-label="View: E" aria-pressed="true"/);
});
