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

const modules = await loadModules(['./system-inventory', './system-topology', './system-device-icons', './canvas-navigation']);
const { systemStateLabel } = modules.get('./system-inventory');
const file = new URL('./SystemTopology.svelte', import.meta.url), source = await readFile(file, 'utf8');
const { js } = compile(source, { filename: file.pathname, generate: 'server' }), exports = {};
vm.runInNewContext(ts.transpileModule(js.code, { compilerOptions: { module: ts.ModuleKind.CommonJS, target: ts.ScriptTarget.ES2022 } }).outputText, {
  exports, require(name) {
    if (name === 'svelte/internal/server') return server;
    if (name === 'svelte') return svelte;
    if (name.endsWith('.svelte')) return { default() {} };
    if (modules.has(name)) return modules.get(name);
    throw Error('Unexpected topology import ' + name);
  },
}, { filename: file.pathname });
const component = (id, type, extra = {}) => ({ id, name: id, type, role: type, presence: 'connected', hidden: null, enabled: null, owner: null, capture: 'unknown', status: '', diagnostics: [], connections: [], targets: [], evidence: [], references: [], firstSeen: 1, lastSeen: 1, observedAt: 1, provider: 'fixture', stale: false, forget: { allowed: false, reason: 'Connected', effect: '' }, ...extra });

test('graph subtext shows panels and inputs while complete health remains accessible', () => {
  const components = [
    component('Busy virtual', 'virtual', { status: 'busy · Creation provenance unknown', owner: 'External feeder' }),
    component('Hidden pedals', 'controller', { hidden: true, enabled: false }),
    component('Stale gateway', 'gateway', { presence: 'unknown', stale: true, status: 'Service unavailable', diagnostics: ['Last provider request failed'] }),
    component('Integration', 'integration', { presence: 'unavailable', status: 'Integration needs attention' }),
    component('Controller', 'controller', { status: '24 controls' }),
    component('Companion', 'companion', { presence: 'offline', status: 'Paired installation · Panel: Dashboard' }),
  ];
  const html = render(exports.default, { props: { components, selectedId: 'Busy virtual', onselect() {} } }).body;
  for (const component of components) {
    const escapedId = component.id.replace(/[.*+?^${}()|[\]\\]/g, '\\$&');
    const node = html.match(new RegExp(`<button[^>]*data-component-id="${escapedId}"[^]*?<\\/button>`))?.[0];
    assert.ok(node, component.id);
    const label = systemStateLabel(component);
    assert.ok(node.includes(label), label);
    assert.ok(node.includes(`title="${label}"`), 'bounded visible state has a complete tooltip');
    assert.ok(node.includes(`, ${label}"`), 'accessible name includes the complete shared state');
    const subtext = node.replace(/<!--[^]*?-->/g, '').match(/class="node-status[^]*?<span[^>]*>([^<]*)<\/span>/)?.[1];
    assert.equal(subtext, component.id === 'Controller' ? '24 inputs' : component.id === 'Companion' ? 'Panel: Dashboard' : '');
  }
  assert.ok(html.includes('Last provider request failed'));
});
