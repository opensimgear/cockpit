import assert from 'node:assert/strict';
import { EventEmitter } from 'node:events';
import { PassThrough, Readable } from 'node:stream';
import test from 'node:test';
import { nativeGatewayBridge } from './native-gateway-bridge.mjs';

function fixture(onRequest) {
  const process = new EventEmitter();
  process.stdin = new PassThrough();
  process.stdout = new PassThrough();
  process.kill = () => { process.killed = true; };
  process.stdin.on('data', line => onRequest(JSON.parse(line.toString()), process));
  let handler, starts = 0;
  const httpServer = new EventEmitter();
  nativeGatewayBridge({ spawnProcess: () => { starts++; return process; } }).configureServer({
    httpServer, middlewares: { use(path, callback) { assert.equal(path, '/api/gateway/'); handler = callback; } },
  });
  async function request(payload, origin = 'http://127.0.0.1:1420') {
    const req = Readable.from([Buffer.from(JSON.stringify(payload))]);
    req.method = 'POST'; req.headers = { origin, host: '127.0.0.1:1420', 'x-osg-client': '1' };
    return new Promise(resolve => {
      const res = { statusCode: 200, setHeader() {}, end(body) { resolve({ status: this.statusCode, body: JSON.parse(body) }); } };
      handler(req, res);
    });
  }
  return { request, process, close: () => httpServer.emit('close'), starts: () => starts };
}

test('gateway requests run concurrently and out-of-order replies reach their caller', async () => {
  const incoming = [];
  const bridge = fixture((envelope, process) => {
    incoming.push(envelope);
    if (incoming.length === 2) {
      // Two valid responses can arrive in a single chunk larger than one frame.
      const lines = incoming.toReversed().map(({ requestId, request }) => JSON.stringify({ requestId, response: { ok: true, state: { gatewayId: request.gatewayId, data: 'x'.repeat(40_000) } } }));
      setImmediate(() => process.stdout.write(`${lines.join('\n')}\n`));
    }
  });
  const completed = [];
  const first = bridge.request({ command: 'status', gatewayId: 'stm32:first' }).then(result => { completed.push('first'); return result; });
  const second = bridge.request({ command: 'status', gatewayId: 'stm32:second' }).then(result => { completed.push('second'); return result; });
  const [a, b] = await Promise.all([first, second]);
  assert.equal(a.body.state.gatewayId, 'stm32:first');
  assert.equal(b.body.state.gatewayId, 'stm32:second');
  assert.equal(bridge.starts(), 1);
  assert.deepEqual(completed, ['second', 'first']);
  assert.notEqual(incoming[0].requestId, incoming[1].requestId);
  bridge.close();
  assert.equal(bridge.process.stdin.writableEnded, true);
});

test('unmatched service replies reject every pending request instead of crossing gateways', async () => {
  const incoming = [];
  const bridge = fixture((envelope, process) => {
    incoming.push(envelope);
    if (incoming.length === 2) process.stdout.write(`${JSON.stringify({ requestId: 'unrecognized', response: { ok: true } })}\n`);
  });
  const results = await Promise.all([bridge.request({ command: 'status', gatewayId: 'first' }), bridge.request({ command: 'status', gatewayId: 'second' })]);
  for (const result of results) { assert.equal(result.status, 400); assert.match(result.body.error, /unmatched/); }
  assert.equal(bridge.process.killed, true);
  bridge.close();
});

test('non-local requests cannot start the gateway service', async () => {
  const bridge = fixture(() => assert.fail('No request expected'));
  const result = await bridge.request({ command: 'ports' }, 'https://example.com');
  assert.equal(result.status, 403);
  assert.equal(bridge.starts(), 0);
  bridge.close();
});
