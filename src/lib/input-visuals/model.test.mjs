import assert from 'node:assert/strict';
import test from 'node:test';
import { axisVisual, hatVisual, HAT_CELLS, HAT_DIRECTIONS } from './model.ts';

test('axis geometry uses zero for bipolar fills and the minimum for unipolar fills', () => {
  assert.deepEqual(axisVisual(0, -100, 100), {
    status: 'value', bipolar: true, zero: 50, position: 50, clamped: 0,
    fillStart: 50, fillSize: 0, valueText: '0',
  });
  const negative = axisVisual(-50, -100, 100);
  assert.equal(negative.fillStart, 25);
  assert.equal(negative.fillSize, 25);
  assert.equal(axisVisual(50, -100, 100).fillStart, 50);
  assert.equal(axisVisual(50, -100, 100).fillSize, 25);
  assert.equal(axisVisual(25, 0, 100).position, 25);
  assert.equal(axisVisual(25, 0, 100).bipolar, false);
  assert.equal(axisVisual(15, 10, 20).position, 50);
  assert.equal(axisVisual(-15, -20, -10).position, 50);
  assert.equal(axisVisual(-32768, -32768, 32767).position, 0);
  assert.equal(axisVisual(32767, -32768, 32767).position, 100);
  assert.equal(axisVisual(.25, 0, 1).valueText, '0.25');
});

test('axis clamps geometry without hiding the reported value', () => {
  for (const [value, position] of [[-200, 0], [200, 100]]) {
    const axis = axisVisual(value, -100, 100);
    assert.equal(axis.position, position);
    assert.equal(axis.valueText, String(value));
  }
});

test('missing, invalid and extreme axes always have bounded finite geometry', () => {
  const ranges = [[-100, 100], [0, 100], [10, 20], [-20, -10], [0, 0], [5, 1], [NaN, 1], [0, Infinity], [-Infinity, 0], [-Number.MAX_VALUE, Number.MAX_VALUE]];
  for (const [min, max] of ranges) {
    for (const value of [undefined, null, NaN, Infinity, -Infinity, -Number.MAX_VALUE, 0, Number.MAX_VALUE]) {
      const axis = axisVisual(value, min, max);
      for (const key of ['zero', 'position', 'fillStart', 'fillSize']) {
        assert.ok(Number.isFinite(axis[key]), `${key} must be finite`);
        assert.ok(axis[key] >= 0 && axis[key] <= 100, `${key} must be bounded`);
      }
      assert.ok(Number.isFinite(axis.clamped));
    }
  }
  assert.equal(axisVisual(undefined, -1, 1).status, 'waiting');
  assert.equal(axisVisual(undefined, -1, 1).valueText, 'Waiting for report');
  assert.equal(axisVisual(null, -1, 1).status, 'unknown');
  assert.equal(axisVisual(NaN, -1, 1).valueText, 'Unknown');
  assert.equal(axisVisual(0, 0, 0).status, 'unknown');
});

test('all eight hat directions follow clockwise HID indices', () => {
  for (let direction = 0; direction < 8; direction++) {
    assert.deepEqual(hatVisual(direction), { status: 'value', direction, selected: direction, valueText: HAT_DIRECTIONS[direction] });
    assert.equal(HAT_CELLS.find(cell => cell.value === direction).text, HAT_DIRECTIONS[direction]);
  }
  assert.deepEqual(HAT_CELLS.map(cell => cell.value), [7, 0, 1, 6, -1, 2, 5, 4, 3]);
});

test('hat neutral is explicit, missing reports wait, malformed reports remain unknown', () => {
  for (const value of [null, -1]) assert.deepEqual(hatVisual(value), { status: 'neutral', direction: null, selected: -1, valueText: 'Neutral' });
  assert.deepEqual(hatVisual(undefined), { status: 'waiting', direction: null, selected: null, valueText: 'Waiting for report' });
  for (const value of [-2, 8, 99, .5, NaN, Infinity, -Infinity]) {
    assert.deepEqual(hatVisual(value), { status: 'unknown', direction: null, selected: null, valueText: 'Unknown' });
  }
});
