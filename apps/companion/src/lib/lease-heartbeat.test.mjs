import assert from 'node:assert/strict';
import test from 'node:test';
import { startLeaseHeartbeat } from './lease-heartbeat.ts';

test('lease renewal does not overlap and stops after cancellation', async t => {
  t.mock.timers.enable({ apis: ['setTimeout'] });
  let sent = 0, finish;
  const stop = startLeaseHeartbeat(
    () => { sent++; return new Promise(resolve => finish = resolve); },
    () => true,
    assert.fail,
    () => 0,
  );
  t.mock.timers.tick(99);
  assert.equal(sent, 0);
  t.mock.timers.tick(1);
  assert.equal(sent, 1);
  t.mock.timers.tick(400);
  assert.equal(sent, 1);
  finish();
  await Promise.resolve();
  t.mock.timers.tick(100);
  assert.equal(sent, 2);
  stop();
  finish();
  await Promise.resolve();
  t.mock.timers.tick(1000);
  assert.equal(sent, 2);
});

test('retired leases never send and errors only reach the current owner', async t => {
  t.mock.timers.enable({ apis: ['setTimeout'] });
  let current = true;
  const errors = [];
  const reason = new Error('renewal rejected');
  const renew = async () => { throw reason; };
  const start = () => startLeaseHeartbeat(renew, () => current, error => errors.push(error), () => 0);
  const stop = start();
  current = false;
  t.mock.timers.tick(100);
  await Promise.resolve();
  assert.deepEqual(errors, []);
  stop();

  current = true;
  const stopCurrent = start();
  t.mock.timers.tick(100);
  await Promise.resolve();
  assert.deepEqual(errors, [reason]);
  t.mock.timers.tick(1000);
  assert.deepEqual(errors, [reason]);
  stopCurrent();
});
