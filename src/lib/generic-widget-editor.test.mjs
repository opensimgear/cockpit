import assert from 'node:assert/strict';
import test from 'node:test';
import { sourceIdentity, createSourceHistory, editSource, undoSource, redoSource, CompilationFence, moveLayer, resizeSelection } from './generic-widget-editor.ts';

test('eight resize handles keep opposite edges fixed, allow nonuniform corners and clamp crossed edges', () => {
  const original = { x: 10, y: 20, width: 100, height: 50 };
  for (const [handle, expected] of Object.entries({nw:[20,25,90,45],n:[10,25,100,45],ne:[10,25,110,45],e:[10,20,110,50],se:[10,20,110,55],s:[10,20,100,55],sw:[20,20,90,55],w:[20,20,90,50]})) {
    const resized = resizeSelection(original, handle, 10, 5);
    assert.deepEqual(Object.values(resized), expected, handle);
    if (handle.includes('w')) assert.equal(resized.x + resized.width, original.x + original.width);
    if (handle.includes('n')) assert.equal(resized.y + resized.height, original.y + original.height);
  }
  assert.notEqual(resizeSelection(original, 'se', 20, 5).width / 55, 2);
  assert.deepEqual(resizeSelection(original, 'nw', 200, 200), { x:109, y:69, width:1, height:1 });
  assert.deepEqual(resizeSelection(original, 'se', -200, -200), { x:10, y:20, width:1, height:1 });
  assert.deepEqual(original, { x:10, y:20, width:100, height:50 });
});

test('layer drops reorder siblings in both directions and keep nested groups intact', () => {
  const nodes = [{ id: 'a' }, { id: 'group', children: [{ id: 'c' }, { id: 'd' }, { id: 'e' }] }, { id: 'b' }];
  assert.equal(moveLayer(nodes, 'a', 'b', 'after'), true);
  assert.deepEqual(nodes.map(node => node.id), ['group', 'b', 'a']);
  assert.equal(moveLayer(nodes, 'a', 'group'), true);
  assert.deepEqual(nodes.map(node => node.id), ['a', 'group', 'b']);
  const children = nodes[1].children;
  assert.equal(moveLayer(nodes, 'c', 'e', 'after'), true);
  assert.deepEqual(children.map(node => node.id), ['d', 'e', 'c']);
  assert.equal(moveLayer(nodes, 'group', 'b', 'after'), true);
  assert.equal(nodes[2].children, children);
  const unchanged = structuredClone(nodes);
  for (const [id, target, placement] of [['missing', 'a'], ['a', 'missing'], ['group', 'c'], ['a', 'a'], ['a', 'b'], ['group', 'b', 'after']]) {
    assert.equal(moveLayer(nodes, id, target, placement), false);
    assert.deepEqual(nodes, unchanged);
  }
  const history = createSourceHistory(nodes);
  const next = structuredClone(nodes);
  moveLayer(next, 'group', 'a');
  const edited = editSource(history, next);
  assert.deepEqual(undoSource(edited).present, nodes);
  assert.deepEqual(redoSource(undoSource(edited)).present, next);
});

test('layer drops change parents, support empty groups and scene, and reject invalid ancestry without mutation', () => {
  const nodes = [{ id: 'a' }, { id: 'left', children: [{ id: 'nested', children: [{ id: 'child' }] }] }, { id: 'empty', children: [] }];
  assert.equal(moveLayer(nodes, 'a', 'empty', 'inside'), true);
  assert.deepEqual(nodes[1].children.map(n => n.id), ['a']);
  assert.equal(moveLayer(nodes, 'nested', 'a', 'before'), true);
  assert.deepEqual(nodes[1].children.map(n => n.id), ['nested', 'a']);
  const unchanged = structuredClone(nodes);
  for (const [id, target, placement] of [['nested', 'child', 'inside'], ['empty', 'child', 'after'], ['a', 'child', 'inside'], ['a', 'a', 'inside'], ['a', 'missing', 'inside']]) {
    assert.equal(moveLayer(nodes, id, target, placement), false);
    assert.deepEqual(nodes, unchanged);
  }
  assert.equal(moveLayer(nodes, 'child', 'left', 'inside'), true);
  assert.equal(moveLayer(nodes, 'child', '', 'root'), true);
  assert.equal(nodes.at(-1).id, 'child');
  assert.equal(moveLayer(nodes, 'child', '', 'root'), false);
  const before = structuredClone(nodes);
  assert.equal(moveLayer(nodes, 'left', 'nested', 'inside', 1), false);
  assert.deepEqual(nodes, before);
  let history = createSourceHistory(nodes);
  const next = structuredClone(nodes);moveLayer(next, 'child', 'nested', 'inside');history = editSource(history, next);
  assert.deepEqual(undoSource(history).present, nodes);
  assert.deepEqual(redoSource(undoSource(history)).present, next);
});

test('source history is immutable, bounded and clears redo after a branch', () => {
  const source = { title: 'Toggle', layers: [{ x: 0 }] };
  let history = createSourceHistory(source);
  source.layers[0].x = 100;
  assert.equal(history.present.layers[0].x, 0);
  assert.equal(editSource(history, history.present), history);
  assert.equal(undoSource(history), history);
  assert.equal(redoSource(history), history);
  for (let i = 1; i <= 100; i++) history = editSource(history, { title: 'Toggle', layers: [{ x: i }] });
  assert.equal(history.past.length, 80);
  history = undoSource(history);
  assert.equal(history.present.layers[0].x, 99);
  assert.equal(redoSource(history).present.layers[0].x, 100);
  history = editSource(history, { title: 'Other', layers: [{ x: 42 }] });
  assert.equal(history.future.length, 0);
  assert.equal(history.present.title, 'Other');
});

test('compile identity is canonical and refuses unserializable values', () => {
  assert.equal(sourceIdentity({ z: [false, null, 1], a: 'hello' }), sourceIdentity({ a: 'hello', z: [false, null, 1] }));
  assert.notEqual(sourceIdentity({ x: 1 }), sourceIdentity({ x: 2 }));
  for (const invalid of [undefined, NaN, Infinity, { x: undefined }]) assert.throws(() => sourceIdentity(invalid), /serializable/);
});

test('compile cancellation rejects stale completion even when worker ignores abort', async () => {
  const fence = new CompilationFence();
  let firstDone, firstSignal;
  const first = fence.run('first', signal => { firstSignal = signal; return new Promise(resolve => firstDone = resolve); });
  assert.equal(fence.get('first'), undefined);
  await fence.run('second', async () => ({ pixels: 'new' }));
  assert.equal(firstSignal.aborted, true);
  firstDone({ pixels: 'old' });
  assert.equal(await first, undefined);
  assert.deepEqual(fence.get('second'), { pixels: 'new' });
  assert.equal(fence.get('first'), undefined);
  fence.invalidate();
  assert.equal(fence.get('second'), undefined);
});

test('compile errors surface only for the current source', async () => {
  const fence = new CompilationFence();
  await assert.rejects(fence.run('live', async () => { throw Error('Rasterizer failed'); }), /Rasterizer failed/);
  let rejectOld;
  const old = fence.run('old', () => new Promise((_, reject) => rejectOld = reject));
  fence.invalidate(); rejectOld(Error('Cancelled'));
  assert.equal(await old, undefined);
});
