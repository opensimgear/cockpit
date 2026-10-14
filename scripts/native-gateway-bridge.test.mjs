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

test('bounded multi-device profile and catalogue payloads survive the ordinary browser bridge', async () => {
  const gatewayId = index => `stm32:${(index % 4 + 1).toString(16).padStart(24,'0')}`;
  const moduleId = index => (index % 8 + 1).toString(16).padStart(32,'0');
  const profile = { schemaVersion: 2, aircraftModule: 'A-10C', cataloguePath: '/' + '\\'.repeat(4094),
    commandHost: '127.0.0.1', commandPort: 7778, listenAddress: '0.0.0.0', listenPort: 5010,
    multicastGroup: '', multicastInterface: '127.0.0.1', freshnessMs: 1500,
    inputs: Array.from({ length: 16 }, (_, i) => ({ id: `input-${i}`.padEnd(31,'x'), gatewayId: gatewayId(i), moduleId: moduleId(i),
      kind: 'button', control: Math.floor(i / 8), command: `COMMAND_${i}`.padEnd(64,'X'), interface: 'set_state', pressedValue: 65535, releasedValue: 0 })),
    outputs: Array.from({ length: 16 }, (_, i) => ({ id: `output-${i}`.padEnd(31,'x'), gatewayId: gatewayId(i), moduleId: moduleId(i),
      control: `FEEDBACK_${i}`.padEnd(64,'X'), output: 0, actionId: `display-${Math.floor(i / 8)}.set_text`, format: {
        kind: 'text', prefix: '"'.repeat(16), suffix: '\\'.repeat(16), unknownText: '"'.repeat(63), staleText: '\\'.repeat(63), align: 'left', overflow: 'truncate' } })),
  };
  const payload = { command: 'dcs_configure', dcsConfig: profile };
  assert.ok(Buffer.byteLength(JSON.stringify(payload)) > 16_384, 'Normal bounded fields exceed the old request limit');
  const controls = Array.from({ length: 64 }, (_, i) => ({ id: `CONTROL_${i}`.padEnd(64,'X'), label: 'L'.repeat(63), category: 'C'.repeat(63),
    inputs: ['set_state','fixed_step','variable_step'].map(interfaceName => ({ interface: interfaceName, maxValue: 65535 })),
    outputs: Array.from({ length: 8 }, () => ({ type: 'string', maxLength: 63, maxValue: 65535 })) }));
  const result = { ok: true, state: { modules: [], dcsBios: { configured: true, running: false, config: profile,
    catalogue: { cataloguePath: profile.cataloguePath, aircraftModule: 'A-10C', kind: 'all', query: '', offset: 0, total: controls.length, controls } } } };
  assert.ok(Buffer.byteLength(JSON.stringify(result)) > 65_536, 'Combined profile and catalogue exceed the old response limit');
  const bridge = fixture((envelope, process) => {
    assert.deepEqual(envelope.request, payload);
    process.stdout.write(`${JSON.stringify({ requestId: envelope.requestId, response: result })}\n`);
  });
  const response = await bridge.request(payload);
  assert.equal(response.status, 200); assert.deepEqual(response.body, result);
  bridge.close();
});

test('oversized browser requests are rejected before starting a native service', async () => {
  const bridge = fixture(() => assert.fail('Oversized input must not reach native transport'));
  const result = await bridge.request({ command: 'dcs_configure', dcsConfig: { cataloguePath: 'x'.repeat(70_000) } });
  assert.equal(result.status, 400); assert.match(result.body.error, /request exceeds/); assert.equal(bridge.starts(), 0);
  bridge.close();
});

test('oversized native responses fail the bounded stream and reject the pending caller', async () => {
  const bridge = fixture((envelope, process) => process.stdout.write(`${JSON.stringify({ requestId: envelope.requestId,
    response: { ok: true, state: { modules: [], invalid: 'x'.repeat(140_000) } } })}\n`));
  const result = await bridge.request({ command: 'dcs_status' });
  assert.equal(result.status, 400); assert.match(result.body.error, /response exceeds/); assert.equal(bridge.process.killed, true);
  bridge.close();
});
