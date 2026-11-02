import { EventEmitter } from 'node:events';
import { PassThrough, Readable } from 'node:stream';
import { nativeGatewayBridge } from '../native-gateway-bridge.mjs';

export const tick = () => new Promise(resolve => setImmediate(resolve));
export function frame(overrides = {}) {
  return { kind: 'snapshot', topic: 'preferences.theme', source: 'preferences', session: 'session',
    generation: '1', revision: '1', capturedUs: '18446744073709551615', clockId: 'clock',
    continuity: 'baseline', payload: { theme: 'dark' }, ...overrides };
}
export function reply(child, requestId, response = { ok: true }) {
  child.stdout.write(`${JSON.stringify({ requestId, response })}\n`);
}
export function deliver(child, requestId, subscriptionId = 'token', delivery = '1', value = frame()) {
  child.stdout.write(`${JSON.stringify({ observation: { requestId, subscriptionId, delivery, frame: value } })}\n`);
}

export function fixture(onRequest) {
  const handlers = new Map();
  const children = [];
  const commands = [];
  const httpServer = new EventEmitter();
  nativeGatewayBridge({ spawnProcess() {
    const child = new EventEmitter();
    child.stdin = new PassThrough(); child.stdout = new PassThrough();
    child.kill = () => { child.killed = true; };
    child.stdin.on('data', bytes => {
      const envelope = JSON.parse(bytes.toString());
      commands.push(envelope);
      onRequest(envelope, child);
    });
    children.push(child);
    return child;
  } }).configureServer({ httpServer, middlewares: { use(path, callback) { handlers.set(path, callback); } } });

  function request(path, body, { slow = false, belowWatermark = false, headers = {}, method = 'POST' } = {}) {
    const req = Readable.from([Buffer.from(JSON.stringify(body))]);
    req.headers = { origin: 'http://127.0.0.1:1420', host: '127.0.0.1:1420',
      'x-osg-client': '1', 'content-type': 'application/json', ...headers };
    req.method = method;
    const res = new EventEmitter();
    res.statusCode = 200; res.headers = {}; res.lines = []; res.headersSent = false; res.writableLength = 0;
    res.writeCallbacks = [];
    res.setHeader = (key, value) => { res.headers[key.toLowerCase()] = value; };
    res.write = (line, callback) => {
      res.headersSent = true; res.lines.push(line);
      if (slow) { res.writableLength += Buffer.byteLength(line); res.writeCallbacks.push({ bytes: Buffer.byteLength(line), callback }); }
      else queueMicrotask(() => callback?.());
      return !slow || belowWatermark;
    };
    let finish;
    const finished = new Promise(resolve => { finish = resolve; });
    res.end = body => { if (body) res.lines.push(body); res.writableEnded = true; finish(res); };
    res.flush = index => {
      const record = res.writeCallbacks[index];
      if (!record) return;
      res.writeCallbacks[index] = undefined;
      res.writableLength -= record.bytes;
      record.callback?.();
    };
    res.drain = () => { for (let n = 0; n < res.writeCallbacks.length; n++) res.flush(n); res.emit('drain'); };
    res.abort = () => { res.destroyed = true; res.emit('close'); };
    res.destroy = () => { res.writableLength = 0; res.abort(); };
    const handled = handlers.get(path)(req, res);
    return { req, res, finished, handled };
  }
  return { request, commands, children, close: () => httpServer.emit('close') };
}
