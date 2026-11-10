import test from 'node:test';
import assert from 'node:assert/strict';
import {calibrateControllerClock} from './controller-clock.mjs';

const sample = {id: 'controllers:fixture', nowUs: '1000'};

test('qualification uses the shortest round trip and includes timestamp rounding', async () => {
  let requests = 0;
  const ticks = [0, 4, 4, 5, ...Array.from({length: 36}, (_, i) => 6 + i * 2)];
  const clock = await calibrateControllerClock(assert.fail, async (command, args) => {
    assert.equal(command, 'gateway_request');
    assert.deepEqual(args, {request: {command: 'qualification_controller_clock'}});
    requests++;
    return sample;
  }, () => ticks.shift());
  assert.equal(clock.method, 'qualification');
  assert.equal(requests, 21);
  assert.equal(clock.id, sample.id);
  assert.equal(clock.rtt, 1);
  assert.equal(clock.offset, 3.5);
  assert.equal(clock.uncertainty, 0.601);
});

test('unavailable qualification retains the inventory calibration', async () => {
  let count = 0, time = 0;
  const clock = await calibrateControllerClock(async request => {
    assert.deepEqual(request, {command: 'controller_list', offset: 0});
    count++;
    return {controllers: {captureClock: sample}};
  }, async () => { throw Error('Desktop qualification is disabled.'); }, () => time++);
  assert.equal(count, 20);
  assert.equal(clock.method, 'inventory');
  assert.equal(clock.id, sample.id);
});

test('a changed native clock invalidates the bracket', async () => {
  let requests = 0, time = 0;
  await assert.rejects(calibrateControllerClock(assert.fail, async () =>
    ++requests < 3 ? sample : {...sample, id: 'controllers:replacement'}, () => time++),
  /Invalid controller capture clock sample/);
});
