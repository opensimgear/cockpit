import test from 'node:test';
import assert from 'node:assert/strict';
import { SharedObservations } from './shared.ts';
import { parseDelivery, uint64, MAX_SNAPSHOT_BYTES, MAX_CHANGE_BYTES } from './protocol.ts';
import { delivery, turn } from './fixtures.mjs';

function setup() {
  const opens = [];
  const transport = { open(topic, observer, signal) { opens.push({ topic, observer, signal }); } };
  return { client: new SharedObservations(transport), opens };
}

test('u64 boundary rejects precision loss, noncanonical decimals and overflow', () => {
  for (const value of ['0', '9007199254740993', '18446744073709551615']) assert.equal(uint64(value), true);
  for (const value of [1, 9007199254740992, '-1', '00', '1.0', '1e3', '18446744073709551616', '', null]) {
    assert.equal(uint64(value), false);
  }
  for (const field of ['generation', 'revision', 'sequence', 'capturedUs']) {
    assert.throws(() => parseDelivery(delivery({ [field]: 9007199254740992 }), 'preferences'));
  }
  assert.throws(() => parseDelivery({ ...delivery(), delivery: 1 }, 'preferences'));
  assert.throws(() => parseDelivery(delivery({ kind: 'future' }), 'preferences'));
  assert.throws(() => parseDelivery(delivery({ kind: new String('snapshot') }), 'preferences'));
  assert.throws(() => parseDelivery(delivery({ continuity: 'future' }), 'preferences'));
  assert.throws(() => parseDelivery(delivery({ topic: 'other' }), 'preferences'));
  assert.throws(() => parseDelivery(delivery({ source: 'x'.repeat(257) }), 'preferences'));
  assert.throws(() => parseDelivery(delivery({ reason: '' }), 'preferences'));
  for (const value of [Infinity, NaN]) {
    assert.throws(() => parseDelivery(delivery({ payload: { nested: [value] } }), 'preferences'));
  }
  assert.equal(parseDelivery(delivery({ payload: { axis: -1.5, range: '18446744073709551615' } }), 'preferences').frame.payload.axis, -1.5);
  assert.equal(parseDelivery(delivery({ payload: 9007199254740992 }), 'preferences').frame.payload, 9007199254740992);
  assert.throws(() => parseDelivery(delivery({ payload: 'x'.repeat(MAX_SNAPSHOT_BYTES) }), 'preferences'));
  assert.throws(() => parseDelivery(delivery({ kind: 'change', continuity: 'continuous', payload: 'x'.repeat(MAX_CHANGE_BYTES) }), 'preferences'));
  // Payload alone fits; the complete wire frame must also fit its kind budget.
  assert.throws(() => parseDelivery(delivery({ kind: 'change', continuity: 'continuous', payload: 'x'.repeat(MAX_CHANGE_BYTES - 100) }), 'preferences'));
});

test('sharing, late state, isolated listeners and ACK ordering', () => {
  const { client, opens } = setup();
  const events = [];
  const first = client.subscribe('preferences', state => { events.push(state.status); if (state.status === 'ready') throw Error('consumer'); });
  const second = client.subscribe('preferences', state => events.push(`second:${state.status}`));
  assert.equal(opens.length, 1);
  opens[0].observer.frame(delivery(), async () => { events.push('ack'); });
  assert.deepEqual(events, ['connecting', 'second:connecting', 'ready', 'second:ready', 'ack']);
  const third = client.subscribe('preferences', state => events.push(`late:${state.status}`));
  assert.equal(events.at(-1), 'late:ready');
  first(); first(); second();
  assert.equal(opens[0].signal.aborted, false);
  third();
  assert.equal(opens[0].signal.aborted, true);
  opens[0].observer.frame(delivery(), async () => assert.fail('disposed ACK'));
  client.close(); client.dispose();
  assert.throws(() => client.subscribe('preferences', () => {}), /disposed/);
});

test('gaps withdraw values and only a fresh baseline restores readiness', () => {
  const { client, opens } = setup();
  const states = [];
  client.subscribe('preferences', state => states.push(state));
  const send = (frame, id) => opens[0].observer.frame(delivery(frame, id), async () => {});
  send({ sequence: '9007199254740993' }, '1');
  send({ kind: 'change', continuity: 'continuous', revision: '2', sequence: '9007199254740995' }, '2');
  assert.equal(states.at(-1).status, 'lost');
  assert.equal(states.at(-1).frame, undefined);
  send({ kind: 'change', continuity: 'continuous', revision: '3', sequence: '9007199254740994' }, '3');
  assert.equal(states.at(-1).status, 'lost');
  send({ revision: '3', sequence: '9007199254740995' }, '4');
  assert.equal(states.at(-1).status, 'ready');
  send({ kind: 'loss', continuity: 'lost', revision: '3', capturedUs: '0', clockId: 'none', payload: undefined, reason: 'overflow' }, '5');
  assert.equal(states.at(-1).status, 'lost');
  assert.equal(states.at(-1).frame.capturedUs, '0');
  send({ revision: '4', sequence: '9007199254740996' }, '6');
  send({ kind: 'change', continuity: 'continuous', revision: '5', generation: '2', sequence: '9007199254740997' }, '7');
  assert.equal(states.at(-1).status, 'lost');
  send({ revision: '5', generation: '2' }, '8');
  assert.equal(states.at(-1).status, 'lost');
  client.close();
});

test('epoch replacement rejects old callbacks and accepts new service session', async () => {
  const { client, opens } = setup();
  const seen = [];
  const off = client.subscribe('preferences', state => seen.push(state));
  opens[0].observer.frame(delivery(), async () => {});
  off();
  client.subscribe('preferences', state => seen.push(state));
  const length = seen.length;
  opens[0].observer.error('late error');
  opens[0].observer.frame(delivery(), async () => assert.fail('stale ack'));
  assert.equal(seen.length, length);
  opens[1].observer.frame(delivery({ session: 'new', generation: '0', revision: '0' }), async () => {});
  assert.equal(seen.at(-1).frame.session, 'new');
  opens[1].observer.frame(delivery({ kind: 'change', continuity: 'continuous', session: 'new', generation: '0', revision: '1' }, '2'), async () => { throw Error('ack failed'); });
  await turn();
  assert.equal(seen.at(-1).status, 'error');
  assert.equal(opens[1].signal.aborted, true);
  client.close();
});

test('invalid entire envelopes never partially update cache, duplicate delivery ignored', () => {
  const { client, opens } = setup();
  const states = [];
  client.subscribe('preferences', state => states.push(state));
  opens[0].observer.frame(delivery(), async () => {});
  opens[0].observer.frame(delivery({ payload: 'late' }), async () => {});
  assert.equal(states.length, 2);
  opens[0].observer.frame({ ...delivery(), subscriptionId: 'wrong' }, async () => assert.fail('invalid ack'));
  assert.equal(states.at(-1).status, 'error');
  assert.equal(states.at(-1).frame, undefined);
  client.close();
});

test('listener disposal during delivery fences later callbacks and ACK', () => {
  const { client, opens } = setup();
  client.subscribe('preferences', state => { if (state.status === 'ready') client.close(); });
  client.subscribe('preferences', state => { if (state.status === 'ready') assert.fail('after disposal'); });
  opens[0].observer.frame(delivery(), async () => assert.fail('after disposal ACK'));
});

test('backend wire generation replacement loss authorizes new baseline and fences retired generation', () => {
  const { client, opens } = setup();
  const states = [];
  client.subscribe('preferences', state => states.push(state));
  const send = (frame, id) => opens[0].observer.frame(delivery(frame, id), async () => {});
  send({}, '1');
  // Wire snapshots have no sequence; the first subsequent change anchors it.
  send({ kind: 'change', continuity: 'continuous', revision: '2', sequence: '1' }, '2');
  assert.equal(states.at(-1).status, 'ready');
  send({ kind: 'loss', continuity: 'lost', generation: '2', revision: '3', capturedUs: '0', clockId: 'none', payload: undefined, reason: 'generation-replaced' }, '3');
  assert.equal(states.at(-1).status, 'lost');
  send({ generation: '2', revision: '3' }, '4');
  assert.equal(states.at(-1).status, 'ready');
  assert.equal(states.at(-1).frame.generation, '2');
  send({ kind: 'change', continuity: 'continuous', generation: '2', revision: '4', sequence: '1' }, '5');
  assert.equal(states.at(-1).status, 'ready');
  const count = states.length;
  send({ kind: 'change', continuity: 'continuous', revision: '999', sequence: '2' }, '6');
  send({ kind: 'loss', continuity: 'lost', revision: '999', reason: 'overflow' }, '7');
  send({ revision: '999' }, '8');
  assert.equal(states.length, count);
  send({ kind: 'loss', continuity: 'lost', generation: '2', revision: '5', reason: 'baseline-reset' }, '9');
  send({ generation: '2', revision: '5' }, '10');
  assert.equal(states.at(-1).status, 'ready');
  client.close();
});

test('one listener cannot mutate the shared frame or payload seen by later listeners', () => {
  const { client, opens } = setup();
  client.subscribe('preferences', state => {
    if (state.frame) { state.frame.revision = '999'; state.frame.payload.theme = 'mutated'; }
  });
  const seen = [];
  client.subscribe('preferences', state => seen.push(state));
  const input = delivery();
  opens[0].observer.frame(input, async () => {});
  input.frame.payload.theme = 'external mutation';
  assert.equal(seen.at(-1).frame.revision, '1');
  assert.equal(seen.at(-1).frame.payload.theme, 'dark');
  client.subscribe('preferences', state => {
    assert.equal(state.frame.payload.theme, 'dark');
    assert.equal(state.frame.revision, '1');
  });
  client.close();
});

test('read-only JSON delivery shares frozen state while mutable listeners keep independent copies', () => {
  const {client, opens} = setup();
  const states = [];
  client.subscribe('preferences', state => { if (state.frame) state.frame.payload.items[0].value = 99; });
  client.subscribeReadonly('preferences', state => states.push(state));
  const input = delivery({payload:{items:[{value:1}]}});
  opens[0].observer.frame(input, async () => {});
  const first = states.at(-1);
  assert.throws(() => { first.frame.payload.items[0].value = 2; }, TypeError);
  assert.throws(() => first.frame.payload.items.push({value:3}), TypeError);
  input.frame.payload.items[0].value = 10;
  client.subscribeReadonly('preferences', state => states.push(state));
  assert.equal(states.at(-1), first);
  assert.equal(first.frame.payload.items[0].value, 1);
  client.close();
});

test('read-only delivery clones non-JSON objects with mutable internal state', () => {
  const {client, opens} = setup();
  const seen = [];
  client.subscribeReadonly('preferences', state => state.frame?.payload.date.setUTCFullYear(2000));
  client.subscribeReadonly('preferences', state => { if (state.frame) seen.push(state.frame.payload.date.getUTCFullYear()); });
  opens[0].observer.frame(delivery({payload:{date:new Date('2026-01-01T00:00:00Z')}}), async () => {});
  assert.deepEqual(seen, [2026]);
  client.close();
});
