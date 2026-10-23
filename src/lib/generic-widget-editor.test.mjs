import assert from 'node:assert/strict';
import test from 'node:test';
import { sourceIdentity, createSourceHistory, editSource, undoSource, redoSource, CompilationFence } from './generic-widget-editor.ts';

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
