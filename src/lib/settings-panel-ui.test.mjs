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
const { createCanvasStylingStore } = modules.get('./canvas-styling');
const file = new URL('./SettingsPanel.svelte', import.meta.url);
const { js } = compile(await readFile(file, 'utf8'), { filename: file.pathname, generate: 'server' });
const code = ts.transpileModule(js.code, { compilerOptions: { module: ts.ModuleKind.CommonJS, target: ts.ScriptTarget.ES2022 } }).outputText;
function html(store = createCanvasNavigationStore(null), styling = createCanvasStylingStore(null)) {
  const exports = {};
  vm.runInNewContext(code, { exports, TextEncoder, require(name) {
    if (name === 'svelte/internal/server') return server;
    if (name === 'svelte') return svelte;
    if (name.endsWith('.svelte')) return { default: () => {} };
    if (name === './canvas-navigation') return { ...modules.get(name), canvasNavigation: store, setWheelAction: store.setWheelAction };
    if (name === './canvas-styling') return { ...modules.get(name), canvasStyling: styling, setArrowStyle: styling.setArrowStyle, setCornerRadius: styling.setCornerRadius, setCollapseParallelWires: styling.setCollapseParallelWires };
    if (modules.has(name)) return modules.get(name);
    throw Error(`Settings must not issue native or profile requests while rendering: ${name}`);
  } }, { filename: file.pathname });
  return render(exports.default, { props: { light: false, onToggleTheme() {} } }).body;
}
function general(body) { return body.slice(body.indexOf('id="settings-panel-general"'), body.indexOf('id="settings-panel-styling"')); }
function styling(body) { return body.slice(body.indexOf('id="settings-panel-styling"'), body.indexOf('id="settings-panel-outputs"')); }

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

test('Styling owns the enabled collapse preference with its signal-count explanation', () => {
  const body = html(), content = styling(body);
  assert.match(content, /Collapse parallel connections/);
  assert.match(content, /Share trunks at common input or output points, with a signal count and branches to each block\./);
  const checkbox = content.match(/<input\b[^>]*type="checkbox"[^>]*>/)?.[0];
  assert.ok(checkbox);
  assert.match(checkbox, /aria-labelledby="arrow-collapse-label"/);
  assert.match(checkbox, /aria-describedby="arrow-collapse-help"/);
  assert.match(checkbox, /\bchecked(?:[\s=>]|$)/);
  assert.doesNotMatch(general(body), /Collapse parallel connections/);
});

test('collapse changes stay local and independent of arrow style, radius, and navigation', () => {
  const navigation = createCanvasNavigationStore(null), store = createCanvasStylingStore(null);
  store.setArrowStyle('orthogonal');
  store.setCornerRadius(17);
  store.setCollapseParallelWires(false);
  let content = styling(html(navigation, store));
  let checkbox = content.match(/<input\b[^>]*type="checkbox"[^>]*>/)?.[0];
  assert.ok(checkbox); assert.doesNotMatch(checkbox, /\bchecked(?:[\s=>]|$)/);
  assert.match(content, /<option\b[^>]*value="orthogonal"[^>]*selected[^>]*>/);
  assert.match(content, /aria-label="Arrow corner radius"[^>]*value="17"/);
  assert.match(general(html(navigation, store)), /<option\b[^>]*value="pan"[^>]*selected[^>]*>/);
  store.setCollapseParallelWires(true);
  content = styling(html(navigation, store));
  checkbox = content.match(/<input\b[^>]*type="checkbox"[^>]*>/)?.[0];
  assert.match(checkbox, /\bchecked(?:[\s=>]|$)/);
});
