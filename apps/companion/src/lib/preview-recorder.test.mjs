import assert from 'node:assert/strict';
import test from 'node:test';
import { createPreviewRecorder } from './preview-recorder.mjs';
const button = (value = true) => ({ id: 'trigger', kind: 'button', value });
const axis = (value = 1) => ({ id: 'x', kind: 'axis', value });
const hat = (value = 0) => ({ id: 'hat', kind: 'hat', value });
test('disarmed preview rejects input and duplicate pointer ownership', () => {
  const recorder = createPreviewRecorder();
  assert.equal(recorder.press(1, 'trigger', [button()]), false);
  recorder.arm();
  assert.equal(recorder.press(1.1, 'trigger', [button()]), false);
  assert.equal(recorder.press(1, 'trigger', []), false);
  assert.equal(recorder.press(1, 'trigger', [button()]), true);
  assert.equal(recorder.press(1, 'other', [axis()]), false);
  assert.equal(recorder.press(2, 'trigger', [axis()]), false);
  assert.equal(recorder.press(2, 'other', [button()]), false);
  assert.equal(recorder.press(2, 'other', [axis(), axis()]), false);
  assert.equal(recorder.move(99, [axis()]), false);
  assert.equal(recorder.release(99), false);
});
test('multitouch axis and held button have independent releases and ordered records', () => {
  let now = 10;
  const recorder = createPreviewRecorder({ clock: () => now++, limit: 3 });
  recorder.arm();
  assert.equal(recorder.press(1, 'pad', [axis(-0.5), { id: 'y', kind: 'axis', value: 0.5 }]), true);
  recorder.press(2, 'trigger', [button()]);
  assert.equal(recorder.snapshot().activePointers, 2);
  assert.equal(recorder.move(1, [axis(0.75)]), true);
  assert.equal(recorder.move(1, [button(false)]), false);
  assert.equal(recorder.move(1, []), false);
  assert.equal(recorder.move(1, [axis(3)]), false);
  assert.equal(recorder.release(1), true);
  assert.deepEqual(recorder.snapshot().values, { x: 0, y: 0, trigger: true });
  recorder.release(2, 'Lost pointer capture');
  const snapshot = recorder.snapshot();
  assert.equal(snapshot.values.trigger, false);
  assert.equal(snapshot.records.length, 3);
  assert.deepEqual(snapshot.records.map((entry) => entry.sequence), [3, 4, 5]);
  snapshot.records[0].changes[0].value = 99;
  assert.notEqual(recorder.snapshot().records[0].changes[0].value, 99);
  assert.equal(snapshot.totalChanges, 7);
});
test('lifecycle deactivation clears every pointer and returns axes/buttons/hat to fallback', () => {
  for (const reason of ['Window lost focus', 'Page hidden', 'Page unloading', 'Preview stopped']) {
    const recorder = createPreviewRecorder();
    recorder.arm();
    recorder.press(1, 'axis', [axis()]);
    recorder.press(2, 'button', [button()]);
    recorder.press(3, 'hat', [hat(6)]);
    const snapshot = recorder.disarm(reason);
    assert.equal(snapshot.armed, false);
    assert.equal(snapshot.activePointers, 0);
    assert.deepEqual(snapshot.values, { x: 0, trigger: false, hat: -1 });
    assert.equal(snapshot.lastReason, reason);
    assert.equal(recorder.move(1, [axis()]), false);
    assert.equal(recorder.release(2), false);
    recorder.arm();
    assert.deepEqual(recorder.snapshot().values, { x: 0, trigger: false, hat: -1 });
  }
});
test('invalid control values and type changes are rejected without state changes', () => {
  const recorder = createPreviewRecorder();
  recorder.arm();
  for (const change of [axis(NaN), axis(Infinity), axis(-2), button(1), hat(0.1), hat(8), hat(-2), { id: '', kind: 'button', value: true }, { id: 'x', kind: 'unknown', value: 1 }]) {
    assert.equal(recorder.press(1, 'bad', [change]), false);
  }
  recorder.press(1, 'axis', [axis(0)]);
  recorder.move(1, [axis(0)]);
  assert.equal(recorder.snapshot().records.length, 1);
  recorder.release(1);
  assert.equal(recorder.press(1, 'type-change', [{ id: 'x', kind: 'button', value: true }]), false);
  recorder.arm();
  recorder.press(1, 'hat', [hat()]);
  assert.equal(recorder.release(1, 'Pointer cancelled'), true);
  assert.equal(recorder.snapshot().values.hat, -1);
  recorder.disarm();
  assert.equal(recorder.snapshot().lastReason, 'Preview stopped');
});
