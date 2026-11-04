import { errorMessage, parseDelivery, uint64, type DeliveryEnvelope, type ObservationObserver, type ObservationTransport } from './protocol.ts';
import { readObservationLines } from './browser/ndjson.ts';

export interface BrowserObservationDependencies {
  fetch?: typeof fetch;
  endpoint?: string;
  connectionId?: () => string;
}
interface Entry {
  topic: string;
  requestId: string;
  observer: ObservationObserver;
  signal: AbortSignal;
  controller: AbortController;
  stop: () => void;
  active: boolean;
  opening: boolean;
  token?: string;
  delivery?: bigint;
  inFlight: boolean;
  acknowledging: boolean;
  pending?: DeliveryEnvelope;
}
interface Connection {
  id: string;
  controller: AbortController;
  reader?: ReadableStreamDefaultReader<Uint8Array>;
  ready: boolean;
  active: boolean;
  entries: Map<string, Entry>;
  closing: number;
}
const object = (value: unknown): value is Record<string, unknown> => typeof value === 'object' && value !== null && !Array.isArray(value);
// Leave room in the bridge's 64-reader budget for another browser context.
const MAX_CONTEXT_READERS = 32;
let sharedBrowserTransport: ObservationTransport | undefined;

/** Defaults share one context connection; injected dependencies create an isolated transport. */
export function createBrowserObservationTransport(dependencies?: BrowserObservationDependencies): ObservationTransport {
  if (dependencies === undefined) return sharedBrowserTransport ??= createBrowserObservationTransport({});
  const request = dependencies.fetch ?? ((...args: Parameters<typeof fetch>) => globalThis.fetch(...args));
  const endpoint = dependencies.endpoint ?? '/api/observe/';
  const root = endpoint.replace(/\/$/, '');
  const headers = { 'Content-Type': 'application/json', 'x-osg-client': '1' };
  let connection: Connection | undefined;
  let nextRequest = 1n;
  const post = (path: string, body: unknown, signal?: AbortSignal) => request(path, {
    method: 'POST', headers, body: JSON.stringify(body), ...(signal ? { signal } : {}),
  });

  function close(current: Connection, message?: string) {
    if (!current.active) return;
    current.active = false;
    if (connection === current) connection = undefined;
    current.controller.abort();
    void current.reader?.cancel().catch(() => {});
    const entries = [...current.entries.values()];
    current.entries.clear();
    for (const entry of entries) {
      entry.active = false;
      entry.controller.abort();
      entry.signal.removeEventListener('abort', entry.stop);
      entry.pending = undefined;
    }
    if (message) for (const entry of entries) {
      try { entry.observer.error(message); } catch { /* Isolate consumers. */ }
    }
  }

  function remove(current: Connection, entry: Entry, message?: string) {
    if (!entry.active) return;
    entry.active = false;
    entry.controller.abort();
    entry.pending = undefined;
    current.entries.delete(entry.requestId);
    entry.signal.removeEventListener('abort', entry.stop);
    if (entry.opening) {
      if (current.closing >= 16) close(current, 'Observation cancellation budget exceeded');
      else {
        current.closing++;
        void post(`${root}/unsubscribe`, { connectionId: current.id, requestId: entry.requestId }, current.controller.signal)
          .catch(() => {}).finally(() => { current.closing--; });
      }
    }
    if (!current.entries.size) close(current);
    if (message) entry.observer.error(message);
  }

  function receive(current: Connection, entry: Entry, envelope: DeliveryEnvelope) {
    if (!entry.active || !current.active) return;
    try {
      if (!entry.token || entry.inFlight) {
        if (entry.pending || (entry.inFlight && !entry.acknowledging)) {
          throw new Error('Observation exceeded one in-flight delivery');
        }
        entry.pending = envelope;
        return;
      }
      if (envelope.subscriptionId !== entry.token) throw new Error('Observation subscription mismatch');
      const delivery = BigInt(envelope.delivery);
      if (entry.delivery !== undefined && delivery !== entry.delivery + 1n) throw new Error('Observation delivery gap');
      entry.delivery = delivery;
      entry.inFlight = true;
      let acknowledgment: Promise<void> | undefined;
      entry.observer.frame(envelope, () => {
        if (acknowledgment) return acknowledgment;
        if (!entry.active || !current.active) return Promise.resolve();
        entry.acknowledging = true;
        acknowledgment = (async () => {
          try {
            const result = await post(`${root}/ack`, {
              subscriptionId: envelope.subscriptionId, delivery: envelope.delivery,
            }, entry.controller.signal);
            if (!result.ok) throw new Error(`Observation acknowledge failed (${result.status})`);
            if (!entry.active || !current.active) return;
            entry.inFlight = false;
            entry.acknowledging = false;
            if (entry.pending) {
              const next = entry.pending;
              entry.pending = undefined;
              receive(current, entry, next);
            }
          } catch (error) {
            if (entry.active && current.active) remove(current, entry, errorMessage(error));
          }
        })();
        return acknowledgment;
      });
      if (entry.active && !acknowledgment) throw new Error('Observation delivery was not acknowledged');
    } catch (error) { remove(current, entry, errorMessage(error)); }
  }

  function subscribe(current: Connection, entry: Entry) {
    if (!entry.active || !current.active || !current.ready || entry.opening) return;
    entry.opening = true;
    void (async () => {
      try {
        const result = await post(`${root}/subscribe`, {
          connectionId: current.id, requestId: entry.requestId, topic: entry.topic,
        }, entry.controller.signal);
        if (!result.ok) throw new Error(`Observation subscribe failed (${result.status})`);
        const reply: unknown = await result.json();
        if (!object(reply) || reply.ok !== true || typeof reply.subscriptionId !== 'string'
            || !reply.subscriptionId || new TextEncoder().encode(reply.subscriptionId).length > 256) {
          throw new Error('Invalid observation subscription token');
        }
        if (!entry.active || !current.active) return;
        entry.token = reply.subscriptionId;
        if (entry.pending) {
          const early = entry.pending;
          entry.pending = undefined;
          receive(current, entry, early);
        }
      } catch (error) {
        if (entry.active && current.active) remove(current, entry, errorMessage(error));
      }
    })();
  }

  function line(current: Connection, value: unknown) {
    if (!object(value)) throw new Error('Invalid observation multiplex record');
    if (!current.ready) {
      if (value.type !== 'ready' || value.connectionId !== current.id || Object.keys(value).length !== 2) {
        throw new Error('Invalid observation connection readiness');
      }
      current.ready = true;
      for (const entry of current.entries.values()) subscribe(current, entry);
      return;
    }
    if (Object.hasOwn(value, 'type') || !uint64(value.requestId) || value.requestId === '0'
        || Object.keys(value).length !== 2 || !Object.hasOwn(value, 'envelope')) {
      throw new Error('Invalid observation multiplex delivery');
    }
    if (BigInt(value.requestId) >= nextRequest) throw new Error('Unknown future observation request');
    const entry = current.entries.get(value.requestId);
    if (!entry) return; // Retired counters need no unbounded tombstone set.
    try { receive(current, entry, parseDelivery(value.envelope, entry.topic)); }
    catch (error) { remove(current, entry, errorMessage(error)); }
  }

  function start(): Connection {
    const current: Connection = {
      id: (dependencies!.connectionId ?? (() => crypto.randomUUID()))(),
      controller: new AbortController(), ready: false, active: true, entries: new Map(), closing: 0,
    };
    connection = current;
    void (async () => {
      try {
        const result = await post(endpoint, { connectionId: current.id }, current.controller.signal);
        if (!current.active) { void result.body?.cancel().catch(() => {}); return; }
        if (!result.ok) throw new Error(`Observation open failed (${result.status})`);
        if (result.headers.get('content-type')?.split(';')[0].trim().toLowerCase() !== 'application/x-ndjson') {
          throw new Error('Observation response is not NDJSON');
        }
        if (!result.body) throw new Error('Observation response has no body');
        current.reader = result.body.getReader();
        await readObservationLines(current.reader, current.controller.signal, value => line(current, value));
      } catch (error) {
        if (current.active) close(current, errorMessage(error));
      } finally {
        current.reader?.releaseLock();
      }
    })();
    return current;
  }

  return { open(topic, observer, signal) {
    if (signal.aborted) return;
    if (connection && connection.entries.size >= MAX_CONTEXT_READERS) { observer.error('Observation subscription limit exceeded'); return; }
    if (nextRequest > 18446744073709551615n) { observer.error('Observation request counter exhausted'); return; }
    const current = connection ?? start();
    const entry: Entry = {
      topic, requestId: String(nextRequest++), observer, signal, controller: new AbortController(), active: true, opening: false,
      inFlight: false, acknowledging: false, stop: () => remove(current, entry),
    };
    current.entries.set(entry.requestId, entry);
    signal.addEventListener('abort', entry.stop, { once: true });
    if (signal.aborted) entry.stop(); else subscribe(current, entry);
  } };
}
