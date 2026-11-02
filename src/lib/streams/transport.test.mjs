import test from 'node:test';
import assert from 'node:assert/strict';
import { SharedObservations } from './shared.ts';
import { createObservationTransport } from './transport.ts';
import { delivery, turn } from './fixtures.mjs';

test('default native transport uses the actual Tauri Channel protocol and releases callbacks', async t => {
  const previous = { window: globalThis.window, native: globalThis.isTauri };
  t.after(() => { globalThis.window = previous.window; globalThis.isTauri = previous.native; });
  const callbacks = new Map(), calls = [], removed = [];
  globalThis.isTauri = true;
  globalThis.window = { __TAURI_INTERNALS__: {
    transformCallback(callback) { callbacks.set(1, callback); return 1; },
    unregisterCallback(id) { removed.push(id); callbacks.delete(id); },
    async invoke(command, args) {
      calls.push([command, args]);
      if (command === 'observation_subscribe') return 'token';
      if (command === 'observation_unsubscribe') callbacks.get(1)?.({ index: 1, end: true });
    },
  } };
  const client = new SharedObservations(createObservationTransport());
  const values = [];
  const stop = client.subscribe('preferences', state => values.push(state));
  await turn();
  const channel = calls[0][1].channel;
  assert.equal(channel.id, 1);
  callbacks.get(1)({ index: 0, message: delivery() });
  await turn();
  assert.equal(values.at(-1).status, 'ready');
  assert.equal(calls[1][0], 'observation_acknowledge');
  stop(); await turn();
  assert.equal(calls.at(-1)[0], 'observation_unsubscribe');
  assert.deepEqual(removed, [1]);
  client.close();
});

test('default browser transport opens a streaming request and aborts it on last disposal', async t => {
  const previous = { fetch: globalThis.fetch, native: globalThis.isTauri };
  t.after(() => { globalThis.fetch = previous.fetch; globalThis.isTauri = previous.native; });
  globalThis.isTauri = false;
  const calls = [];
  let cancelled = false;
  let stream;
  globalThis.fetch = async (url, args) => {
    calls.push([url, args]);
    if (url.endsWith('/ack')) return new Response('{"ok":true}');
    if (url.endsWith('/unsubscribe')) return new Response('{"ok":true}');
    const payload=JSON.parse(args.body);
    if (url.endsWith('/subscribe')) {
      stream.enqueue(new TextEncoder().encode(JSON.stringify({requestId:payload.requestId,envelope:delivery()})+'\n'));
      return new Response('{"ok":true,"subscriptionId":"token"}');
    }
    return new Response(new ReadableStream({
      start(controller) { stream=controller;controller.enqueue(new TextEncoder().encode(JSON.stringify({type:'ready',connectionId:payload.connectionId})+'\n')); },
      cancel() { cancelled = true; },
    }), { headers: { 'content-type': 'application/x-ndjson' } });
  };
  const client = new SharedObservations(createObservationTransport());
  const values = [];
  const stop = client.subscribe('preferences', state => values.push(state));
  await turn(); await turn();
  assert.equal(values.at(-1).status, 'ready');
  assert.equal(calls[0][0], '/api/observe/');
  assert.equal(typeof JSON.parse(calls[0][1].body).connectionId,'string');
  assert.equal(calls[1][0], '/api/observe/subscribe');
  assert.equal(JSON.parse(calls[1][1].body).topic,'preferences');
  assert.equal(calls[2][0], '/api/observe/ack');
  stop(); await turn();
  assert.equal(calls[0][1].signal.aborted, true);
  assert.equal(cancelled, true);
  client.close();
});
