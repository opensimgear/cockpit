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

const modules = await loadModules(['./profile-editor', './profile-canvas', './profile-diagram', './profile-processing', './profile-port-browser']);
const plain = value => JSON.parse(JSON.stringify(value));
const port = (id, direction, name = id) => ({ id, direction, name, contract: { delivery: 'state', schema: { type: 'boolean' }, freshnessMs: 1000, fallback: null } });
const resource = (id, ports = []) => ({ id, name: id, kind: 'device', required: false, ports });
const profile = () => ({
  schemaVersion: 2, id: '11111111-1111-4111-8111-111111111111', name: 'Wiring gestures', revision: 1,
  eligibility: { gameIds: [] }, resources: [resource('source', [port('saved-fire', 'output')]), resource('target', [port('saved-led', 'input')]), resource('empty')],
  bindings: [], processingNodes: [], virtualOutputs: [], visibility: [], overrides: [], companionAssignments: [],
  editor: { positions: [{ nodeId: 'source', x: 20, y: 40 }, { nodeId: 'target', x: 400, y: 40 }, { nodeId: 'empty', x: 20, y: 350 }], viewport: { x: 0, y: 0, zoom: 1 } }
});
const noRequest = () => { throw new Error('Canvas gestures must not issue inventory or native requests.'); };
const transpile = source => ts.transpileModule(source, { compilerOptions: { module: ts.ModuleKind.CommonJS, target: ts.ScriptTarget.ES2022 } }).outputText;
const iconSource = await readFile(new URL('./Icon.svelte', import.meta.url), 'utf8');
const iconCode = transpile(compile(iconSource, { filename: 'Icon.svelte', generate: 'server' }).js.code);
let canvasSource = await readFile(new URL('./ProfileCanvas.svelte', import.meta.url), 'utf8');
canvasSource = canvasSource.replace('</script>', `captureCanvas({beginWire,wireClick,move,finish,keyboard,lostPointer,choose,blockPorts,setStage(value){stage=value;},setView(value){view=value;},get state(){return{wire,selectedId,suppressWireClick};}});</script>`);
const canvasCode = transpile(compile(canvasSource, { filename: 'ProfileCanvas.svelte', generate: 'server' }).js.code);

// Invoke the production Svelte closures with explicit pointer-capture and hit-test fixtures.
// Actual browser dispatch and hit testing are covered by the running cockpit UI checks.
function harness(props = {}) {
  const calls = [], edits = [], captured = new Set();
  let handlers, hit = element();
  const context = {
    TextEncoder, captureCanvas: value => handlers = value,
    window: { document: { elementFromPoint: () => hit } },
    require(name) {
      if (name === 'svelte/internal/server') return server;
      if (name === 'svelte') return svelte;
      if (name === './Icon.svelte') return { default: Icon };
      const key = name.replace(/\.ts$/, '');
      if (modules.has(key)) return modules.get(key);
      throw new Error(`Unexpected Canvas dependency ${name}`);
    }
  };
  const iconExports = {};
  vm.runInNewContext(iconCode, { ...context, exports: iconExports }, { filename: 'Icon.svelte' });
  const Icon = iconExports.default, canvasExports = {};
  vm.runInNewContext(canvasCode, { ...context, exports: canvasExports }, { filename: 'ProfileCanvas.svelte' });
  const document = profile(), before = plain(document);
  const body = render(canvasExports.default, { props: {
    document, assignments: [], inventory: [], inventoryClient: { inspect: noRequest },
    change: (...args) => edits.push(args), connectBlocks: (...args) => calls.push(['connect', ...args]),
    connectionFrom: id => calls.push(['from', id]), connectionTo: id => calls.push(['to', id]),
    closeConnectionPanel: () => calls.push(['close']), ...props
  } }).body;
  handlers.setStage({
    setPointerCapture: id => captured.add(id), hasPointerCapture: id => captured.has(id), releasePointerCapture: id => captured.delete(id),
    getBoundingClientRect: () => ({ left: 100, top: 50 }), contains: target => target.inside !== false
  });
  return { handlers, body, calls, edits, captured, document, before, setHit(value) { hit = value; } };
}
function element({ nodeId, input, output, disabled = false, inside = true, panel = false } = {}) {
  const value = { disabled, inside, dataset: { connectionTarget: input, connectionSource: output } };
  value.closest = selector => {
    if (selector === '[data-connection-target]') return input ? value : null;
    if (selector === '[data-connection-source]') return output ? value : null;
    return nodeId || panel ? value : null;
  };
  return value;
}
const pointer = (x = 100, y = 100, extra = {}) => ({ button: 0, pointerId: 7, clientX: x, clientY: y, preventDefault() {}, stopPropagation() {}, ...extra });
const click = (detail = 1) => ({ detail, stopPropagation() {} });
const button = (body, attribute, id) => {
  const tag = body.match(new RegExp(`<button[^>]*${attribute}="${id}"[^>]*>`));
  assert.ok(tag, `Missing ${attribute} handle for ${id}`);
  return tag[0];
};

test('both wire directions preserve output-to-input routing, world coordinates and the exact profile', () => {
  for (const direction of ['output', 'input']) {
    const h = harness(), origin = direction === 'output' ? 'source' : 'target';
    h.handlers.setView({ x: 20, y: 30, zoom: .5 });
    h.handlers.beginWire(pointer(), origin, direction);
    assert.equal(h.captured.has(7), true);
    h.handlers.move(pointer(320, 180));
    assert.deepEqual(plain(h.handlers.state.wire.end), { x: 400, y: 200 });
    assert.equal(h.handlers.state.wire.moved, true);
    h.setHit(direction === 'output' ? element({ nodeId: 'target', input: 'target' }) : element({ nodeId: 'source', output: 'source' }));
    h.handlers.finish(pointer(320, 180));
    assert.deepEqual(h.calls, [['connect', 'source', 'target']]);
    assert.equal(h.handlers.state.wire, null);
    assert.equal(h.captured.size, 0);
    h.handlers.wireClick(click(), origin, direction);
    assert.deepEqual(h.calls, [['connect', 'source', 'target']], 'The native follow-up click must not replace the paired mapping.');
    assert.equal(h.edits.length, 0);
    assert.deepEqual(h.document, h.before, 'Dragging only opens the chooser; exact ports are saved later.');
  }
});

test('wrong-side, same-block, disabled and off-canvas wire releases remain inert', () => {
  for (const direction of ['output', 'input']) {
    const origin = direction === 'output' ? 'source' : 'target';
    const invalid = [
      element({ nodeId: 'other', ...(direction === 'output' ? { output: 'other' } : { input: 'other' }) }),
      element({ nodeId: origin, ...(direction === 'output' ? { input: origin } : { output: origin }) }),
      element({ nodeId: 'other', disabled: true, ...(direction === 'output' ? { input: 'other' } : { output: 'other' }) }),
      element({ inside: false }), element({ panel: true })
    ];
    for (const target of invalid) {
      const h = harness();
      h.handlers.beginWire(pointer(), origin, direction);h.handlers.move(pointer(300, 100));h.setHit(target);h.handlers.finish(pointer(300, 100));
      h.handlers.wireClick(click(), origin, direction);
      assert.deepEqual(h.calls, []);
      assert.equal(h.captured.size, 0);
      assert.equal(h.handlers.state.wire, null);
      assert.equal(h.edits.length, 0);
    }
  }
});

test('pointer cancellation, Escape and lost capture never open the connection chooser', () => {
  for (const cancel of ['pointer', 'escape', 'capture']) {
    const h = harness();h.handlers.beginWire(pointer(), 'target', 'input');h.handlers.move(pointer(300, 100));
    h.setHit(element({ nodeId: 'source', output: 'source' }));
    if (cancel === 'pointer') h.handlers.finish(pointer(300, 100), true);
    if (cancel === 'escape') h.handlers.keyboard({ key: 'Escape', preventDefault() {} });
    if (cancel === 'capture') { h.captured.delete(7);h.handlers.lostPointer(pointer()); }
    h.handlers.finish(pointer(300, 100));h.handlers.wireClick(click(), 'target', 'input');
    assert.deepEqual(h.calls, []);assert.equal(h.handlers.state.wire, null);assert.equal(h.captured.size, 0);
  }
  const h = harness();h.handlers.beginWire(pointer(0, 0, { button: 2 }), 'source', 'output');
  assert.equal(h.handlers.state.wire, null);assert.equal(h.captured.size, 0);
});

test('clicks and empty-canvas releases open the matching side once and retain keyboard access', () => {
  for (const direction of ['output', 'input']) {
    for (const moved of [false, true]) {
      const h = harness(), origin = direction === 'output' ? 'source' : 'target', callback = direction === 'output' ? 'from' : 'to';
      h.handlers.beginWire(pointer(), origin, direction);if (moved) h.handlers.move(pointer(300, 100));
      h.handlers.finish(pointer(300, 100));h.handlers.wireClick(click(), origin, direction);
      assert.deepEqual(h.calls, [[callback, origin]]);
      h.handlers.wireClick(click(0), origin, direction);
      assert.deepEqual(h.calls, [[callback, origin], [callback, origin]], 'Keyboard activation remains available after a wire gesture.');
      h.handlers.choose('empty');assert.deepEqual(h.calls.at(-1), ['close']);assert.equal(h.handlers.state.selectedId, 'empty');
    }
  }
});

test('blocks show discovered input and output counts while loading/error handles and read-only state remain clear', () => {
  const discoveredPorts = { source: [port('fire', 'output', 'Fire trigger'), port('axis', 'output', 'Brake axis'), port('switch', 'output', 'Third output'), port('led', 'input', 'Backlight')], target: [] };
  const h = harness({ discoveredPorts });
  assert.match(button(h.body, 'data-connection-source', 'source'), /Outputs for source \(3\)/);
  assert.match(button(h.body, 'data-connection-target', 'source'), /Inputs \/ actions for source \(1\)/);
  for (const name of ['Fire trigger', 'Brake axis', 'Third output', 'Backlight', '+1 more']) assert.ok(!h.body.includes(name));
  assert.ok(!h.body.includes('saved-fire'));assert.ok(!h.body.includes('saved-led'), 'A completed empty discovery replaces the saved preview.');
  assert.match(button(h.body, 'data-connection-target', 'target'), /disabled/);assert.match(button(h.body, 'data-connection-source', 'empty'), /disabled/);
  assert.match(button(h.body, 'data-connection-target', 'target'), /Inputs \/ actions for target \(0\)/);
  assert.match(button(h.body, 'data-connection-source', 'empty'), /Outputs for empty \(0\)/);assert.deepEqual(h.document, h.before);
  const waiting = harness({ discoveryBusy: ['empty'] });
  assert.ok(waiting.body.includes('Discovering inputs and outputs'));assert.doesNotMatch(button(waiting.body, 'data-connection-target', 'empty'), /disabled/);
  const failed = harness({ discoveryErrors: { empty: 'Provider unavailable' } });
  assert.ok(failed.body.includes('Port discovery failed'));assert.doesNotMatch(button(failed.body, 'data-connection-source', 'empty'), /disabled/, 'Failed discovery must allow opening its retry UI.');
  const locked = harness({ readOnly: true, discoveredPorts });
  for (const id of ['source', 'target', 'empty']) for (const attribute of ['data-connection-source', 'data-connection-target']) assert.match(button(locked.body, attribute, id), /disabled/);
  locked.handlers.beginWire(pointer(), 'source', 'output');locked.handlers.wireClick(click(0), 'source', 'output');
  assert.equal(locked.handlers.state.wire, null);assert.deepEqual(locked.calls, []);assert.equal(locked.captured.size, 0);
});

test('port discovery falls back to intrinsic processing, virtual and desktop contracts, and the connection panel replaces node inspection', () => {
  const h = harness(), input = port('value', 'input'), output = port('result', 'output');
  assert.deepEqual(plain(h.handlers.blockPorts({ processing: { inputs: [input], outputs: [output] } }, 'output')), [output]);
  assert.deepEqual(plain(h.handlers.blockPorts({ processing: { inputs: [input], outputs: [output] } }, 'input')), [input]);
  for (const kind of ['virtual', 'desktop']) {
    assert.deepEqual(plain(h.handlers.blockPorts({ [kind]: { ports: [input] } }, 'input')), [input]);
    assert.deepEqual(plain(h.handlers.blockPorts({ [kind]: { ports: [input] } }, 'output')), []);
  }
  const panel = harness({ selectedId: 'source', connectionOpen: true, connectionPanel: svelte.createRawSnippet(() => ({ render: () => '<p>Exact port choices</p>' })) });
  assert.ok(panel.body.includes('aria-label="Exact connection editor"'));assert.ok(panel.body.includes('Exact port choices'));assert.ok(!panel.body.includes('aria-label="Selected block inspector"'));
  const inspected = harness({ selectedId: 'source' });assert.ok(inspected.body.includes('aria-label="Selected block inspector"'));assert.ok(!inspected.body.includes('aria-label="Exact connection editor"'));
});
