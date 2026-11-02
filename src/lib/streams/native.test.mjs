import test from 'node:test';
import assert from 'node:assert/strict';
import { createNativeObservationTransport } from './native.ts';
import { delivery, deferred, turn } from './fixtures.mjs';

test('native early channel delivery waits for token and ACK follows delivery', async () => {
  const opening = deferred();
  const calls = [];
  const events = [];
  const transport = createNativeObservationTransport({
    channel(receive) { receive(delivery()); return {}; },
    async invoke(command, args) {
      calls.push([command, args]);
      if (command === 'observation_subscribe') return opening.promise;
    },
  });
  const controller = new AbortController();
  transport.open('preferences', { frame(value, ack) { events.push(value); void ack(); }, error: assert.fail }, controller.signal);
  assert.equal(events.length, 0);
  opening.resolve('token');
  await turn();
  assert.equal(events.length, 1);
  assert.deepEqual(calls.map(([command]) => command), ['observation_subscribe', 'observation_acknowledge']);
  assert.deepEqual(calls[1][1], { subscriptionId: 'token', delivery: '1' });
  controller.abort(); controller.abort();
  assert.equal(calls.filter(([command]) => command === 'observation_unsubscribe').length, 1);
});

test('cancellation while native subscribe is pending closes returned token without callback', async () => {
  const opening = deferred();
  const calls = [];
  let receive;
  const transport = createNativeObservationTransport({
    channel(callback) { receive = callback; return {}; },
    async invoke(command, args) { calls.push([command, args]); if (command === 'observation_subscribe') return opening.promise; },
  });
  const controller = new AbortController();
  transport.open('preferences', { frame: assert.fail, error: assert.fail }, controller.signal);
  receive(delivery());
  controller.abort();
  opening.resolve('token');
  await turn();
  assert.equal(calls.at(-1)[0], 'observation_unsubscribe');
  receive(delivery());
});

test('native bounded ACK race buffers just one next frame then drains', async () => {
  const ackWait = deferred();
  const frames = [];
  const errors = [];
  let receive;
  let count = 0;
  const transport = createNativeObservationTransport({
    channel(callback) { receive = callback; return {}; },
    async invoke(command) {
      if (command === 'observation_subscribe') return 'token';
      if (command === 'observation_acknowledge' && count++ === 0) {
        receive(delivery({ revision: '2' }, '2'));
        return ackWait.promise;
      }
    },
  });
  const controller = new AbortController();
  transport.open('preferences', { frame(frame, ack) { frames.push(frame); void ack(); }, error: value => errors.push(value) }, controller.signal);
  await turn();
  receive(delivery());
  assert.equal(frames.length, 1);
  ackWait.resolve();
  await turn();
  assert.equal(frames.length, 2);
  assert.deepEqual(errors, []);
  controller.abort();
});

test('native failure, bad token, early overflow and stalled receiver are explicit errors', async () => {
  for (const mode of ['failure', 'token', 'early-overflow', 'stalled', 'ack-failure', 'mismatch']) {
    const errors = [];
    let receive;
    const transport = createNativeObservationTransport({
      channel(callback) {
        receive = callback;
        if (mode === 'early-overflow') { callback(delivery()); callback(delivery()); }
        return {};
      },
      async invoke(command) {
        if (command === 'observation_subscribe') {
          if (mode === 'failure') throw Error('open rejected');
          return mode === 'token' ? null : 'token';
        }
        if (command === 'observation_acknowledge' && mode === 'ack-failure') throw Error('ACK rejected');
      },
    });
    const controller = new AbortController();
    transport.open('preferences', { frame(_frame, ack) { if (mode !== 'stalled') void ack(); }, error: value => errors.push(value) }, controller.signal);
    await turn();
    if (mode === 'stalled') { receive(delivery()); receive(delivery({}, '2')); }
    if (mode === 'ack-failure') receive(delivery());
    if (mode === 'mismatch') receive({ ...delivery(), subscriptionId: 'wrong' });
    await turn();
    assert.equal(errors.length, 1, mode);
    controller.abort();
  }
});

test('already aborted native observation performs no work', () => {
  const controller = new AbortController(); controller.abort();
  createNativeObservationTransport({ channel: assert.fail, invoke: assert.fail })
    .open('preferences', { frame: assert.fail, error: assert.fail }, controller.signal);
});
