import assert from 'node:assert/strict';
import test from 'node:test';
import { sourceIdentity, createSourceHistory, editSource, undoSource, redoSource, CompilationFence, moveSiblingLayer } from './generic-widget-editor.ts';

test('layer drops reorder siblings in both directions and keep nested groups intact', () => {
  const nodes = [{ id: 'a' }, { id: 'group', children: [{ id: 'c' }, { id: 'd' }, { id: 'e' }] }, { id: 'b' }];
  assert.equal(moveSiblingLayer(nodes, 'a', 'b', true), true);
  assert.deepEqual(nodes.map(node => node.id), ['group', 'b', 'a']);
  assert.equal(moveSiblingLayer(nodes, 'a', 'group'), true);
  assert.deepEqual(nodes.map(node => node.id), ['a', 'group', 'b']);
  const children = nodes[1].children;
  assert.equal(moveSiblingLayer(nodes, 'c', 'e', true), true);
  assert.deepEqual(children.map(node => node.id), ['d', 'e', 'c']);
  assert.equal(moveSiblingLayer(nodes, 'group', 'b', true), true);
  assert.equal(nodes[2].children, children);
  const unchanged = structuredClone(nodes);
  for (const [id, target, after] of [['missing', 'a'], ['a', 'missing'], ['group', 'c'], ['c', 'group'], ['a', 'a'], ['a', 'b'], ['group', 'b', true]]) {
    assert.equal(moveSiblingLayer(nodes, id, target, after), false);
    assert.deepEqual(nodes, unchanged);
  }
  const history = createSourceHistory(nodes);
  const next = structuredClone(nodes);
  moveSiblingLayer(next, 'group', 'a');
  const edited = editSource(history, next);
  assert.deepEqual(undoSource(edited).present, nodes);
  assert.deepEqual(redoSource(undoSource(edited)).present, next);
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
