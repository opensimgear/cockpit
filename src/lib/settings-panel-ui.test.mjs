import assert from 'node:assert/strict';
import test from 'node:test';
import { readFile } from 'node:fs/promises';
import vm from 'node:vm';
import ts from 'typescript';
import { compile } from 'svelte/compiler';
import { render } from 'svelte/server';
import * as server from 'svelte/internal/server';
import * as svelte from 'svelte';
import { loadModules } from './test-module-loader.mjs';

const modules = await loadModules(['./canvas-navigation', './canvas-styling', './profile-diagram']);
const { createCanvasNavigationStore } = modules.get('./canvas-navigation');
const file = new URL('./SettingsPanel.svelte', import.meta.url);
const { js } = compile(await readFile(file, 'utf8'), { filename: file.pathname, generate: 'server' });
const code = ts.transpileModule(js.code, { compilerOptions: { module: ts.ModuleKind.CommonJS, target: ts.ScriptTarget.ES2022 } }).outputText;
function html(store = createCanvasNavigationStore(null)) {
  const exports = {};
  vm.runInNewContext(code, { exports, TextEncoder, require(name) {
    if (name === 'svelte/internal/server') return server;
    if (name === 'svelte') return svelte;
    if (name.endsWith('.svelte')) return { default: () => {} };
    if (name === './canvas-navigation') return { ...modules.get(name), canvasNavigation: store, setWheelAction: store.setWheelAction };
    if (modules.has(name)) return modules.get(name);
    throw Error(`Settings must not issue native or profile requests while rendering: ${name}`);
  } }, { filename: file.pathname });
  return render(exports.default, { props: { light: false, onToggleTheme() {} } }).body;
}
function general(body) { return body.slice(body.indexOf('id="settings-panel-general"'), body.indexOf('id="settings-panel-styling"')); }

test('General is initially selected and owns the Canvas mouse-wheel preference', () => {
  const body = html(), content = general(body);
  assert.match(body, /id="settings-tab-general"[^>]*aria-selected="true"/);
  assert.doesNotMatch(content.match(/^[^>]*>/)?.[0] ?? '', /\bhidden(?:[\s=>]|$)/);
  assert.match(content, /<section\b[^>]*aria-label="Canvas navigation"[^>]*>/);
  assert.match(content, /<h2>Canvas<\/h2>/);
  const select = content.match(/<select\b([^>]*aria-label="Canvas mouse wheel"[^>]*)>([^]*?)<\/select>/);
  assert.ok(select); assert.match(select[1], /aria-describedby="canvas-wheel-help"/);
  assert.match(select[2], /<option\b[^>]*value="pan"[^>]*selected[^>]*>Scroll \(pan\)<\/option>/);
  assert.match(select[2], /<option\b[^>]*value="zoom"[^>]*>Zoom<\/option>/);
  assert.match(content, /Hold Ctrl\/⌘ to reverse the action/);
  assert.match(content, /Changes are saved automatically/);
  assert.equal((body.match(/aria-label="Canvas mouse wheel"/g) ?? []).length, 1);
});

test('the rendered selection follows the local navigation store across changes', () => {
  const store = createCanvasNavigationStore(null);
  store.setWheelAction('zoom');
  assert.match(general(html(store)), /<option\b[^>]*value="zoom"[^>]*selected[^>]*>Zoom<\/option>/);
  store.setWheelAction('pan');
  assert.match(general(html(store)), /<option\b[^>]*value="pan"[^>]*selected[^>]*>Scroll \(pan\)<\/option>/);
});

test('a navigation storage failure is shown beside the still-usable local setting', () => {
  const store = createCanvasNavigationStore({ getItem() { throw Error('denied'); }, setItem() { throw Error('full'); } });
  const content = general(html(store));
  assert.match(content, /role="alert"[^>]*>Canvas navigation could not be loaded locally/);
  const select = content.match(/<select\b[^>]*aria-label="Canvas mouse wheel"[^>]*>/)?.[0];
  assert.ok(select); assert.doesNotMatch(select, /\bdisabled(?:[\s=>]|$)/);
  store.setWheelAction('zoom');
  assert.match(general(html(store)), /Canvas navigation could not be saved locally/);
});
