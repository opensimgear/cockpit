import assert from 'node:assert/strict';
import test from 'node:test';
import {profileAxisRanges} from './profile-axis-ranges.mjs';

test('native extrema retain witnesses even when presentation values repeat', () => {
  const ranges = profileAxisRanges();
  ranges.declare({id: 'stick', min: 0, max: 65535});
  ranges.record('stick', {native: 1, normalized: 0}, '10');
  ranges.record('stick', {native: 0, normalized: 0}, '11');
  ranges.record('stick', {native: 65535, normalized: 32767}, '12');
  assert.deepEqual(ranges.evidence(), [{
    id: 'stick',
    declaredNative: {min: 0, max: 65535},
    declaredPresentation: {min: 0, max: 32767},
    samples: 3,
    unavailable: 0,
    minimum: {native: 0, normalized: 0, sequence: '11'},
    maximum: {native: 65535, normalized: 32767, sequence: '12'},
  }]);
});

test('signed axis range is stable and unavailable values do not establish endpoints', () => {
  const ranges = profileAxisRanges();
  ranges.declare({id: 'rudder', min: -128, max: 127});
  ranges.declare({id: 'rudder', min: -128, max: 127});
  ranges.record('rudder', null, '1');
  ranges.record('rudder', {native: 128, normalized: null}, '2');
  assert.equal(ranges.evidence()[0].unavailable, 2);
  assert.equal(ranges.evidence()[0].minimum, null);
  ranges.record('rudder', {native: -128, normalized: -32768}, '3');
  ranges.record('rudder', {native: 127, normalized: 32767}, '4');
  assert.deepEqual(ranges.evidence()[0].declaredPresentation, {min: -32768, max: 32767});
  assert.throws(() => ranges.declare({id: 'rudder', min: -128, max: 128}), /range changed/);
  assert.throws(() => ranges.declare({id: 'bad', min: 1, max: 1}), /Invalid native range/);
  assert.throws(() => ranges.record('missing', null, '5'), /Missing native range/);
  assert.throws(() => ranges.record('rudder', {native: 128, normalized: 32767}, '6'), /Inconsistent/);
  assert.throws(() => ranges.record('rudder', {native: 0, normalized: 0}, '7'), /Inconsistent/);
});
