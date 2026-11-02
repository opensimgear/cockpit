import assert from 'node:assert/strict';
import test from 'node:test';
import { deliver, fixture, frame, reply, tick } from './fixture.mjs';

const connectionId = '12345678-1234-1234-1234-123456789012';
const openPath = '/api/observe/';
const subscribePath = '/api/observe/subscribe';
const unsubscribePath = '/api/observe/unsubscribe';
const ackPath = '/api/observe/ack';
const topic = 'preferences.theme';
const token = id => `token-${id}`;
function service(envelope, child) {
  if (envelope.observation?.operation === 'subscribe') {
    deliver(child, envelope.requestId, token(envelope.requestId), '1', frame({ topic: envelope.observation.topic }));
    reply(child, envelope.requestId, { ok: true, subscriptionId: token(envelope.requestId) });
  } else reply(child, envelope.requestId);
}
async function connect(bridge, options) {
  const open = bridge.request(openPath, { connectionId }, options);
  await open.handled;
  assert.deepEqual(JSON.parse(open.res.lines[0]), { type: 'ready', connectionId });
  return open;
}
function subscribe(bridge, requestId = '1', selectedTopic = topic) {
  return bridge.request(subscribePath, { connectionId, requestId, topic: selectedTopic });
}
const ack = (bridge, subscriptionId, delivery = '1') => bridge.request(ackPath, { subscriptionId, delivery });
const cancel = (bridge, requestId = '1') => bridge.request(unsubscribePath, { connectionId, requestId });

test('six topics share one physical stream; early records coexist with ordinary RPC', async () => {
  const bridge = fixture(service);
  const open = await connect(bridge);
  const subscriptions = Array.from({ length: 6 }, (_, n) => subscribe(bridge, String(n + 1), `topic/${n}`));
  const rpc = bridge.request('/api/gateway/', { command: 'status' });
  await Promise.all(subscriptions.map(value => value.handled));
  assert.equal((await rpc.finished).statusCode, 200);
  assert.equal(open.res.lines.length, 7);
  for (let n = 0; n < 6; n++) {
    const value = JSON.parse(open.res.lines[n + 1]);
    assert.equal(value.requestId, String(n + 1));
    assert.equal(value.envelope.frame.topic, `topic/${n}`);
    assert.equal(value.envelope.frame.capturedUs, '18446744073709551615');
    assert.equal((await ack(bridge, value.envelope.subscriptionId).finished).statusCode, 200);
  }
  assert.ok(bridge.commands.filter(value => value.observation).every(value => value.observation.clientId === 'browser-dev'));
  assert.equal(bridge.children.length, 1);
  open.res.abort(); await tick();
  assert.equal(bridge.commands.filter(value => value.observation?.operation === 'unsubscribe').length, 6);
  bridge.close();
});

test('record completion gates exact ACK independently of later buffered records', async () => {
  const bridge = fixture(service);
  const open = await connect(bridge, { slow: true, belowWatermark: true });
  await subscribe(bridge).handled;
  await subscribe(bridge, '2', 'other').handled;
  const firstToken = JSON.parse(open.res.lines[1]).envelope.subscriptionId;
  const secondToken = JSON.parse(open.res.lines[2]).envelope.subscriptionId;
  const first = ack(bridge, firstToken), second = ack(bridge, secondToken);
  await tick();
  assert.equal(bridge.commands.length, 2);
  assert.equal((await ack(bridge, firstToken).finished).statusCode, 400);
  open.res.flush(0); // ready
  open.res.flush(1); // first reader; the other reader remains buffered
  assert.equal((await first.finished).statusCode, 200);
  assert.ok(open.res.writableLength > 0);
  assert.equal(bridge.commands.filter(value => value.observation?.operation === 'acknowledge').length, 1);
  open.res.flush(2);
  assert.equal((await second.finished).statusCode, 200);
  open.res.abort(); await tick(); bridge.close();
});

test('pending cancellation disposes an early token and ignores one retired late frame', async () => {
  let pending;
  const bridge = fixture((envelope, child) => {
    if (envelope.observation?.operation === 'subscribe' && envelope.observation.topic === 'other') service(envelope, child);
    else if (envelope.observation?.operation === 'subscribe') pending = { envelope, child };
    else reply(child, envelope.requestId);
  });
  const open = await connect(bridge);
  const subscription = subscribe(bridge);
  await tick();
  assert.equal((await cancel(bridge).finished).statusCode, 200);
  deliver(pending.child, pending.envelope.requestId);
  await tick();
  reply(pending.child, pending.envelope.requestId, { ok: true, subscriptionId: 'token' });
  assert.equal((await subscription.finished).statusCode, 400);
  assert.equal(open.res.lines.length, 1);
  await subscribe(bridge, '2', 'other').handled;
  deliver(pending.child, pending.envelope.requestId);
  assert.equal(pending.child.killed, undefined, 'late canceled frame must not kill healthy multiplexed readers');
  assert.equal((await ack(bridge, 'token').finished).statusCode, 400);
  const healthyToken = JSON.parse(open.res.lines[1]).envelope.subscriptionId;
  assert.equal((await ack(bridge, healthyToken).finished).statusCode, 200);
  open.res.abort(); bridge.close();
});

test('physical close cancels pending opens after their token reply without restarting', async () => {
  let pending;
  const bridge = fixture((envelope, child) => {
    if (envelope.observation?.operation === 'subscribe') pending = { envelope, child };
    else reply(child, envelope.requestId);
  });
  const open = await connect(bridge);
  const subscription = subscribe(bridge);
  await tick(); open.res.abort();
  reply(pending.child, pending.envelope.requestId, { ok: true, subscriptionId: 'token' });
  assert.equal((await subscription.finished).statusCode, 400);
  assert.equal(bridge.commands.at(-1).observation.operation, 'unsubscribe');
  assert.equal(bridge.children.length, 1);
  bridge.close();
});

test('cancellation arriving before subscribe prevents native allocation; out-of-order counters work', async () => {
  const bridge = fixture(service);
  const open = await connect(bridge);
  assert.equal((await cancel(bridge, '3').finished).statusCode, 200);
  await subscribe(bridge, '2').handled;
  await subscribe(bridge, '1').handled;
  assert.equal((await subscribe(bridge, '3').finished).statusCode, 400);
  assert.equal((await subscribe(bridge, '2').finished).statusCode, 400);
  assert.equal(bridge.commands.length, 2);
  open.res.abort(); await tick(); bridge.close();
});

test('late lower subscription IDs remain canceled and obsolete cancellations release their budget', async () => {
  const bridge = fixture(service);
  const open = await connect(bridge);
  await subscribe(bridge, '2').handled;
  assert.equal((await cancel(bridge, '1').finished).statusCode, 200);
  assert.equal((await subscribe(bridge, '1').finished).statusCode, 400);
  for (let id = 3; id <= 200; id++) assert.equal((await cancel(bridge, String(id)).finished).statusCode, 200);
  await subscribe(bridge, '201').handled;
  assert.equal((await cancel(bridge, '202').finished).statusCode, 200);
  assert.equal((await cancel(bridge, '202').finished).statusCode, 200);
  assert.equal((await subscribe(bridge, '202').finished).statusCode, 400);
  assert.equal(bridge.commands.filter(value => value.observation?.operation === 'subscribe').length, 2);
  open.res.abort(); await tick(); bridge.close();
});

test('unknown tokens, connections, forged scope and malformed controls never reach native', async () => {
  const bridge = fixture(service);
  for (const payload of [
    { subscriptionId: 'unknown', delivery: '1' },
    { subscriptionId: 'token', delivery: 1 },
    { subscriptionId: 'token', delivery: '18446744073709551616' },
    { subscriptionId: 'token', delivery: '1', clientId: 'victim' },
  ]) assert.equal((await bridge.request(ackPath, payload).finished).statusCode, 400);
  assert.equal((await subscribe(bridge).finished).statusCode, 400);
  assert.equal((await bridge.request(openPath, { connectionId, topic }).finished).statusCode, 400);
  assert.equal(bridge.children.length, 0);
  bridge.close();
});

test('all observation endpoints require same local origin, POST, marker and JSON', async () => {
  const bridge = fixture(() => assert.fail('Unauthorized native request'));
  const cases = [
    { headers: { origin: 'https://example.com' } }, { headers: { origin: 'http://localhost:1420' } },
    { headers: { origin: 'invalid' } }, { headers: { 'x-osg-client': '0' } },
    { headers: { 'content-type': 'text/plain' } }, { method: 'GET' },
  ];
  for (const path of [openPath, subscribePath, unsubscribePath, ackPath]) for (const options of cases) {
    assert.equal((await bridge.request(path, { topic }, options).finished).statusCode, 403);
  }
  assert.equal(bridge.children.length, 0);
  bridge.close();
});

test('physical connections cap at16 and native pending readers cap at64', async () => {
  const bridge = fixture(() => {});
  const opens = [];
  for (let n = 0; n < 16; n++) {
    const value = bridge.request(openPath, { connectionId: `12345678-1234-1234-1234-${String(n).padStart(12, '0')}` });
    await value.handled; opens.push(value);
  }
  assert.equal((await bridge.request(openPath, { connectionId }).finished).statusCode, 400);
  opens[0].res.abort();
  const open = await connect(bridge);
  const readers = Array.from({ length: 64 }, (_, n) => subscribe(bridge, String(n + 1)));
  await tick();
  assert.equal(bridge.commands.length, 64);
  assert.equal((await subscribe(bridge, '65').finished).statusCode, 400);
  bridge.children[0].emit('exit');
  await Promise.all(readers.map(value => value.handled));
  assert.equal(open.res.writableEnded, true);
  assert.equal(bridge.children.length, 1);
  bridge.close();
});

test('native ACK rejection is an HTTP error; native exit closes streams and permits fresh physical connection', async () => {
  const bridge = fixture((envelope, child) => {
    if (envelope.observation?.operation === 'acknowledge') reply(child, envelope.requestId, { ok: false, error: 'receipt rejected' });
    else service(envelope, child);
  });
  const open = await connect(bridge);
  await subscribe(bridge).handled;
  const subscriptionId = JSON.parse(open.res.lines[1]).envelope.subscriptionId;
  const result = await ack(bridge, subscriptionId).finished;
  assert.equal(result.statusCode, 400);
  assert.match(result.lines.join(''), /receipt rejected/);
  bridge.children[0].emit('exit');
  assert.match(open.res.lines.at(-1), /exited/);
  const next = await connect(bridge);
  await subscribe(bridge).handled;
  assert.equal(bridge.children.length, 2);
  next.res.abort(); await tick(); bridge.close();
});

test('invalid or future observation IDs fail transport; issued retired IDs do not', async () => {
  for (const invalid of [frame({ capturedUs: 42 }), frame({ revision: '01' }), frame({ topic: 'another-topic' }),
    frame({ unexpected: true }), frame({ payload: 'x'.repeat(1048576) }), frame()]) {
    const bridge = fixture(service);
    const open = await connect(bridge);
    await subscribe(bridge).handled;
    deliver(bridge.children[0], bridge.commands[0].requestId, token(bridge.commands[0].requestId), '2', invalid);
    assert.equal(bridge.children[0].killed, true);
    bridge.close();
  }
  const bridge = fixture(service);
  const open = await connect(bridge);
  await subscribe(bridge).handled;
  deliver(bridge.children[0], 'observation:999', 'future');
  assert.equal(bridge.children[0].killed, true);
  assert.equal(open.res.writableEnded, true);
  bridge.close();
});

test('physical output has a global16MiB budget and closed stalled sockets release buffers', async () => {
  const bridge = fixture((envelope, child) => {
    if (envelope.observation?.operation === 'subscribe') {
      deliver(child, envelope.requestId, token(envelope.requestId), '1', frame({ payload: 'x'.repeat(1047000) }));
      reply(child, envelope.requestId, { ok: true, subscriptionId: token(envelope.requestId) });
    } else reply(child, envelope.requestId);
  });
  const open = await connect(bridge, { slow: true });
  for (let n = 1; n <= 17; n++) await subscribe(bridge, String(n)).handled;
  assert.equal(open.res.destroyed, true);
  assert.equal(open.res.writableLength, 0);
  assert.match(open.res.lines.at(-1), /backlog/);
  assert.equal(bridge.children[0].killed, undefined);
  bridge.close();
});

test('closing a stalled physical stream rejects pending ACK and never forwards it', async () => {
  const bridge = fixture(service);
  const open = await connect(bridge, { slow: true });
  await subscribe(bridge).handled;
  const subscriptionId = JSON.parse(open.res.lines[1]).envelope.subscriptionId;
  const pending = ack(bridge, subscriptionId);
  await tick(); open.res.abort();
  assert.equal((await pending.finished).statusCode, 400);
  await tick();
  assert.equal(bridge.commands.filter(value => value.observation?.operation === 'acknowledge').length, 0);
  bridge.close();
});
