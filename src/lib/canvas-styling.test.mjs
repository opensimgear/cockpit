import assert from 'node:assert/strict';
import test from 'node:test';
import { loadModules } from './test-module-loader.mjs';

const { createCanvasStylingStore, canvasStyling, CANVAS_STYLING_STORAGE_KEY } = (await loadModules(['./canvas-styling'])).get('./canvas-styling');
const plain = value => JSON.parse(JSON.stringify(value));
function current(store) {
  let value;
  store.subscribe(next => { value = next; })();
  return plain(value);
}
function memory(saved = null) {
  const writes = [];
  return { writes, getItem: key => { assert.equal(key, CANVAS_STYLING_STORAGE_KEY); return saved; }, setItem: (key, value) => { assert.equal(key, CANVAS_STYLING_STORAGE_KEY); saved = value; writes.push(value); } };
}

test('canvas styling works without browser globals and starts with smooth connections', () => {
  assert.deepEqual(current(canvasStyling), { arrowStyle: 'smooth', cornerRadius: 24, collapseParallelWires: true, storageError: null });
  const store = createCanvasStylingStore(null);
  store.setArrowStyle('orthogonal');
  store.setCornerRadius(35);
  assert.deepEqual(current(store), { arrowStyle: 'orthogonal', cornerRadius: 35, collapseParallelWires: true, storageError: null });
});

test('restored styling validates the style and clamps finite corner radii', () => {
  assert.deepEqual(current(createCanvasStylingStore(memory('{"arrowStyle":"orthogonal","cornerRadius":57}'))), { arrowStyle: 'orthogonal', cornerRadius: 57, collapseParallelWires: true, storageError: null });
  assert.deepEqual(current(createCanvasStylingStore(memory('{"arrowStyle":"unknown","cornerRadius":999}'))), { arrowStyle: 'smooth', cornerRadius: 100, collapseParallelWires: true, storageError: null });
  assert.equal(current(createCanvasStylingStore(memory('{"cornerRadius":-5}'))).cornerRadius, 0);
  assert.equal(current(createCanvasStylingStore(memory('{"cornerRadius":"10"}'))).cornerRadius, 24);
  assert.equal(current(createCanvasStylingStore(memory('{"cornerRadius":1e1000}'))).cornerRadius, 24);
});

test('unsupported stored structures use defaults and malformed storage reports its failure', () => {
  for (const value of [null, 'null', '[]', 'true', '42']) {
    assert.deepEqual(current(createCanvasStylingStore(memory(value))), { arrowStyle: 'smooth', cornerRadius: 24, collapseParallelWires: true, storageError: null });
  }
  assert.match(current(createCanvasStylingStore(memory('{'))).storageError, /could not be loaded/);
  assert.match(current(createCanvasStylingStore({ getItem() { throw new Error('blocked'); }, setItem() {} })).storageError, /could not be loaded/);
});

test('each preference update preserves the other and saves only preference data', () => {
  const storage = memory(), store = createCanvasStylingStore(storage);
  store.setCornerRadius(19);
  store.setArrowStyle('orthogonal');
  store.setArrowStyle('smooth');
  store.setCollapseParallelWires(false);
  store.setCornerRadius(37);
  store.setArrowStyle('orthogonal');
  assert.deepEqual(storage.writes.map(JSON.parse), [
    { arrowStyle: 'smooth', cornerRadius: 19, collapseParallelWires: true },
    { arrowStyle: 'orthogonal', cornerRadius: 19, collapseParallelWires: true },
    { arrowStyle: 'smooth', cornerRadius: 19, collapseParallelWires: true },
    { arrowStyle: 'smooth', cornerRadius: 19, collapseParallelWires: false },
    { arrowStyle: 'smooth', cornerRadius: 37, collapseParallelWires: false },
    { arrowStyle: 'orthogonal', cornerRadius: 37, collapseParallelWires: false },
  ]);
  assert.deepEqual(current(createCanvasStylingStore(storage)), current(store));
});

test('invalid changes are ignored, radii stay bounded, and unchanged settings do not write', () => {
  const storage = memory(), store = createCanvasStylingStore(storage);
  store.setArrowStyle('smooth');
  store.setCornerRadius(24);
  store.setCollapseParallelWires(true);
  for (const value of [null, undefined, 'false', 0, {}, []]) store.setCollapseParallelWires(value);
  store.setArrowStyle('invalid');
  for (const value of [NaN, Infinity, -Infinity]) store.setCornerRadius(value);
  assert.equal(storage.writes.length, 0);
  store.setCornerRadius(-3);
  assert.equal(current(store).cornerRadius, 0);
  store.setCornerRadius(200);
  assert.equal(current(store).cornerRadius, 100);
});

test('a failed save preserves session styling, exposes the problem, and can recover', () => {
  let blocked = true;
  const storage = memory();
  const store = createCanvasStylingStore({ getItem: storage.getItem, setItem(...args) { if (blocked) throw new Error('full'); storage.setItem(...args); } });
  store.setArrowStyle('orthogonal');
  assert.equal(current(store).arrowStyle, 'orthogonal');
  assert.match(current(store).storageError, /could not be saved/);
  store.setCornerRadius(61);
  assert.equal(current(store).cornerRadius, 61);
  blocked = false;
  store.setCornerRadius(61);
  assert.deepEqual(current(store), { arrowStyle: 'orthogonal', cornerRadius: 61, collapseParallelWires: true, storageError: null });
  assert.deepEqual(JSON.parse(storage.writes.at(-1)), { arrowStyle: 'orthogonal', cornerRadius: 61, collapseParallelWires: true });
});

test('subscribers receive immutable complete snapshots and can unsubscribe', () => {
  const store = createCanvasStylingStore(null), seen = [];
  const unsubscribe = store.subscribe(value => { assert.equal(Object.isFrozen(value), true); seen.push(plain(value)); });
  store.setArrowStyle('orthogonal');
  store.setCornerRadius(0);
  store.setCollapseParallelWires(false);
  unsubscribe();
  store.setCornerRadius(20);
  assert.deepEqual(seen, [
    { arrowStyle: 'smooth', cornerRadius: 24, collapseParallelWires: true, storageError: null },
    { arrowStyle: 'orthogonal', cornerRadius: 24, collapseParallelWires: true, storageError: null },
    { arrowStyle: 'orthogonal', cornerRadius: 0, collapseParallelWires: true, storageError: null },
    { arrowStyle: 'orthogonal', cornerRadius: 0, collapseParallelWires: false, storageError: null },
  ]);
});

test('collapsed connection preference restores only boolean values and defaults old settings to enabled', () => {
  for (const value of [null, 'false', 0, [], {}]) {
    assert.equal(current(createCanvasStylingStore(memory(JSON.stringify({ collapseParallelWires: value })))).collapseParallelWires, true);
  }
  for (const value of [true, false]) {
    assert.equal(current(createCanvasStylingStore(memory(JSON.stringify({ collapseParallelWires: value })))).collapseParallelWires, value);
  }
});
