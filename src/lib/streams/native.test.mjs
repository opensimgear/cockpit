import test from 'node:test';
import assert from 'node:assert/strict';
import { createNativeObservationTransport } from './native.ts';
import { controllerChannelDecoder } from './controller-channel.ts';
import { delivery, deferred, turn } from './fixtures.mjs';

test('compact native controller snapshots restore exact values and require a fresh descriptor baseline after loss', () => {
  const expand = controllerChannelDecoder();
  const topic = 'controllers.input.source';
  const baseline = delivery({topic, source:'controller-input', kind:'snapshot',
    payload:{source:{sequence:'1'}, cursor:'1', capturedUs:'10', identity:{path:'usb'},
      controls:[{capability:{id:'axis'},value:{native:0,normalized:0}}, {capability:{id:'hat'},value:null}]}});
  const compact = delivery({topic, source:'controller-input', kind:'snapshot', revision:'2',
    payload:{source:{sequence:'9'}, cursor:'9', capturedUs:'19', controllerSamples:[[32768,24576],null]}}, '2');
  assert.throws(() => expand(compact), /Invalid compact controller snapshot/);
  expand(baseline);
  baseline.frame.payload.controls[0].capability.id = 'changed by consumer';
  const restored = expand(compact);
  assert.deepEqual(restored.frame.payload.controls, [
    {capability:{id:'axis'}, value:{native:32768,normalized:24576}},
    {capability:{id:'hat'}, value:null},
  ]);
  assert.deepEqual(restored.frame.payload.source, {sequence:'9'});
  assert.equal(restored.frame.payload.cursor, '9');
  restored.frame.payload.controls[0].capability.id = 'changed after expansion';
  restored.frame.payload.identity.path = 'changed after expansion';
  const next = expand(compact).frame.payload;
  assert.equal(next.controls[0].capability.id, 'axis');
  assert.equal(next.identity.path, 'usb');
  assert.throws(() => expand({...compact,frame:{...compact.frame,generation:'2'}}), /Invalid compact controller snapshot/);
  assert.throws(() => expand({...compact,frame:{...compact.frame,payload:{...compact.frame.payload,
    controllerSamples:[['invalid',0],null]}}}), /Invalid compact controller sample/);
  expand(delivery({topic,source:'controller-input',kind:'loss'}));
  assert.throws(() => expand(compact), /Invalid compact controller snapshot/);
});

test('compact snapshot expansion isolates nested descriptor fields', () => {
  const expand = controllerChannelDecoder();
  const topic = 'controllers.input.source';
  expand(delivery({topic, source:'controller-input', kind:'snapshot',
    payload:{controls:[{capability:{id:'axis', extension:{value:'original'}}, value:null}]}}));
  const compact = delivery({topic, source:'controller-input', kind:'snapshot', revision:'2',
    payload:{source:{sequence:'2'}, cursor:'2', capturedUs:'20', controllerSamples:[[1,1]]}}, '2');
  expand(compact).frame.payload.controls[0].capability.extension.value = 'changed';
  assert.equal(expand(compact).frame.payload.controls[0].capability.extension.value, 'original');
});

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
