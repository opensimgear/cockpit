import { authorize, decimal, identity, readBody, validateDelivery } from './protocol.mjs';

/** @typedef {import('node:http').IncomingMessage} HttpRequest */
/** @typedef {import('node:http').ServerResponse} HttpResponse */
/** @typedef {{bytes: number, completed: boolean}} WriteRecord */
/** @typedef {{delivery: string, written: boolean, acknowledging?: boolean, resolve?: () => void, reject?: (error: Error) => void}} Receipt */
/** @typedef {{res: HttpResponse, readers: Map<string, ReaderState>, canceled: Set<string>, seen: Set<string>, highest: bigint, writes: Set<WriteRecord>, id?: string, closing?: boolean}} Connection */
/** @typedef {{connection: Connection, frontendId: string, topic: string, settled: boolean, requestId?: string, token?: string, current?: Receipt, lastDelivery?: string, closing?: boolean, failed?: boolean, unsubscribing?: boolean, unsubscribed?: boolean}} ReaderState */
/** @typedef {{operation: 'subscribe'|'unsubscribe'|'acknowledge', clientId: string, topic?: string, subscriptionId?: string, delivery?: string}} NativeControl */
/** @typedef {{ok?: boolean, error?: string, subscriptionId?: string}} NativeResponse */
/** @typedef {(payload: NativeControl, beforeWrite?: (requestId: string) => void) => Promise<NativeResponse>} NativeRequest */

const clientId = 'browser-dev';
/** @param {unknown} value @returns {value is string} */
const uuid = value => typeof value === 'string' && /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/.test(value);
/** @param {unknown} value @returns {value is string} */
const positive = value => decimal(value) && value !== '0';
const outputLimit = 16 * 1024 * 1024;

/** One physical stream multiplexes bounded, independently acknowledged readers.
 * @param {NativeRequest} request
 * @param {(requestId: string) => boolean} wasIssued
 */
export function observationBridge(request, wasIssued = () => false) {
  /** @type {Map<string, Connection>} */
  const connections = new Map();
  /** @type {Set<Connection>} */
  const physical = new Set();
  /** @type {Set<ReaderState>} */
  const slots = new Set();
  /** @type {Map<string, ReaderState>} */
  const requests = new Map(), tokens = new Map();
  let pendingBytes = 0;

  /** @param {HttpResponse} res @param {unknown} error @param {number} status */
  function failure(res, error, status = 400) {
    const message = error instanceof Error ? error.message : String(error);
    if (res.writableEnded || res.destroyed) return;
    if (!res.headersSent) {
      res.statusCode = status;
      res.setHeader('Content-Type', 'application/json');
      res.end(JSON.stringify({ ok: false, error: message }));
    } else res.end(`${JSON.stringify({ error: message })}\n`);
  }
  /** @param {ReaderState} state */
  function release(state) {
    slots.delete(state);
    state.connection.readers.delete(state.frontendId);
    if (state.requestId) requests.delete(state.requestId);
    if (state.token) tokens.delete(state.token);
    state.current?.reject?.(new Error('Observation closed.'));
  }
  /** @param {ReaderState} state */
  async function close(state) {
    state.closing = true;
    state.current?.reject?.(new Error('Observation closed.'));
    if (state.failed || (state.settled && (!state.token || state.unsubscribed))) { release(state); return; }
    if (!state.token || state.unsubscribing) return;
    state.unsubscribing = true;
    try { await request({ operation: 'unsubscribe', clientId, subscriptionId: state.token }); }
    catch { /* Native failure also releases every reader. */ }
    finally { state.unsubscribed = true; if (state.settled) release(state); }
  }
  /** @param {Connection} connection @param {unknown} [error] */
  function closeConnection(connection, error) {
    connection.closing = true;
    physical.delete(connection);
    if (connection.id && connections.get(connection.id) === connection) connections.delete(connection.id);
    for (const record of connection.writes) {
      if (!record.completed) { record.completed = true; pendingBytes -= record.bytes; }
    }
    connection.writes.clear();
    for (const state of connection.readers.values()) void close(state);
    if (error) {
      failure(connection.res, error);
      // A stalled socket must release its actual Node output buffers too.
      if (connection.res.writableLength > 0) connection.res.destroy?.();
    }
  }
  /** @param {Connection} connection @param {unknown} value @param {() => void} completed */
  function write(connection, value, completed = () => {}) {
    const line = `${JSON.stringify(value)}\n`;
    const bytes = Buffer.byteLength(line);
    if (pendingBytes + bytes > outputLimit) {
      closeConnection(connection, new Error('Observation output backlog limit reached.'));
      return;
    }
    const record = { bytes, completed: false };
    connection.writes.add(record);
    pendingBytes += bytes;
    connection.res.write(line, error => {
      if (record.completed) return;
      record.completed = true;
      connection.writes.delete(record);
      pendingBytes -= bytes;
      if (error) closeConnection(connection, error);
      else completed();
    });
  }
  /** @param {HttpRequest} req @param {HttpResponse} res */
  async function body(req, res) {
    try { authorize(req); }
    catch (error) { failure(res, error, 403); return; }
    return readBody(req);
  }
  /** @param {Record<string, unknown>} payload */
  function connectionFor(payload) {
    if (!uuid(payload.connectionId)) throw new Error('Unknown observation connection.');
    const connection = connections.get(payload.connectionId);
    if (!connection || connection.closing) throw new Error('Unknown observation connection.');
    return connection;
  }
  /** @param {HttpRequest} req @param {HttpResponse} res */
  async function open(req, res) {
    /** @type {Connection | undefined} */
    let connection;
    try {
      try { authorize(req); } catch (error) { failure(res, error, 403); return; }
      if (physical.size >= 16) throw new Error('Observation HTTP connection limit reached.');
      connection = { res, readers: new Map(), canceled: new Set(), seen: new Set(), highest: 0n, writes: new Set() };
      const opened = connection;
      physical.add(connection);
      res.once('close', () => closeConnection(opened));
      req.once('aborted', () => closeConnection(opened));
      const payload = await readBody(req);
      if (Object.keys(payload).length !== 1 || !uuid(payload.connectionId)) throw new Error('Invalid observation connection ID.');
      if (connections.has(payload.connectionId)) throw new Error('Duplicate observation connection.');
      if (connection.closing) return;
      connection.id = payload.connectionId;
      connections.set(connection.id, connection);
      res.setHeader('Content-Type', 'application/x-ndjson');
      res.setHeader('Cache-Control', 'no-store');
      write(connection, { type: 'ready', connectionId: connection.id });
    } catch (error) {
      if (connection) closeConnection(connection);
      failure(res, error);
    }
  }
  /** @param {HttpRequest} req @param {HttpResponse} res */
  async function subscribe(req, res) {
    /** @type {ReaderState | undefined} */
    let state;
    try {
      const payload = await body(req, res);
      if (!payload) return;
      const connection = connectionFor(payload);
      if (Object.keys(payload).length !== 3 || !positive(payload.requestId) || !identity(payload.topic)) throw new Error('Invalid observation subscription.');
      const counter = BigInt(payload.requestId);
      // Parallel HTTP controls may arrive out of order. Keep a bounded replay
      // window wider than the maximum simultaneous native readers.
      if (connection.seen.has(payload.requestId) || counter + 64n <= connection.highest) throw new Error('Replayed observation subscription.');
      if (counter > connection.highest) {
        connection.highest = counter;
        for (const id of connection.seen) if (BigInt(id) + 64n <= counter) connection.seen.delete(id);
        for (const id of connection.canceled) if (BigInt(id) + 64n <= counter) connection.canceled.delete(id);
      }
      connection.seen.add(payload.requestId);
      if (connection.canceled.delete(payload.requestId)) throw new Error('Observation subscription canceled.');
      if (slots.size >= 64) throw new Error('Observation reader limit reached.');
      state = { connection, frontendId: payload.requestId, topic: payload.topic, settled: false };
      const reader = state;
      slots.add(state);
      connection.readers.set(state.frontendId, state);
      const response = await request({ operation: 'subscribe', clientId, topic: state.topic }, requestId => {
        reader.requestId = requestId;
        requests.set(requestId, reader);
      });
      state.settled = true;
      if (state.failed) throw new Error('Native observation transport closed.');
      if (!response.ok || !identity(response.subscriptionId)) throw new Error(response.error ?? 'Native observation subscription failed.');
      if (state.token && state.token !== response.subscriptionId) throw new Error('Native observation token changed.');
      if (tokens.has(response.subscriptionId) && tokens.get(response.subscriptionId) !== state) throw new Error('Duplicate native observation token.');
      state.token = response.subscriptionId;
      tokens.set(state.token, state);
      if (state.closing) { await close(state); throw new Error('Observation subscription canceled.'); }
      res.end(JSON.stringify({ ok: true, subscriptionId: state.token }));
    } catch (error) {
      if (state) { state.settled = true; void close(state); }
      failure(res, error);
    }
  }
  /** @param {HttpRequest} req @param {HttpResponse} res */
  async function unsubscribe(req, res) {
    try {
      const payload = await body(req, res);
      if (!payload) return;
      const connection = connectionFor(payload);
      if (Object.keys(payload).length !== 2 || !positive(payload.requestId)) throw new Error('Invalid observation cancellation.');
      const state = connection.readers.get(payload.requestId);
      if (state) void close(state);
      else if (!connection.seen.has(payload.requestId) && BigInt(payload.requestId) + 64n > connection.highest) {
        const counter = BigInt(payload.requestId);
        if (counter > connection.highest) {
          connection.highest = counter;
          for (const id of connection.seen) if (BigInt(id) + 64n <= counter) connection.seen.delete(id);
          for (const id of connection.canceled) if (BigInt(id) + 64n <= counter) connection.canceled.delete(id);
        }
        if (!connection.canceled.has(payload.requestId) && connection.canceled.size >= 64) throw new Error('Observation pending cancellation limit reached.');
        connection.canceled.add(payload.requestId);
      }
      res.end(JSON.stringify({ ok: true }));
    } catch (error) { failure(res, error); }
  }
  /** @param {HttpRequest} req @param {HttpResponse} res */
  async function acknowledge(req, res) {
    try {
      const payload = await body(req, res);
      if (!payload) return;
      const state = identity(payload.subscriptionId) ? tokens.get(payload.subscriptionId) : undefined;
      const current = state?.current;
      if (Object.keys(payload).length !== 2 || !identity(payload.subscriptionId) || !decimal(payload.delivery) ||
          !state || state.closing || state.connection.closing || !current || current.delivery !== payload.delivery || current.acknowledging) {
        throw new Error('Unknown observation receipt.');
      }
      current.acknowledging = true;
      if (!current.written) await new Promise((resolve, reject) => { current.resolve = () => resolve(undefined); current.reject = reject; });
      if (state.closing) throw new Error('Observation closed.');
      state.current = undefined;
      const response = await request({ operation: 'acknowledge', clientId, subscriptionId: state.token, delivery: payload.delivery });
      if (response?.ok !== true) throw new Error(response?.error ?? 'Native observation acknowledgement failed.');
      res.end(JSON.stringify(response));
    } catch (error) { failure(res, error); }
  }
  return {
    /** @param {import('vite').ViteDevServer} server */
    configure(server) {
      /** @type {[string, (req: HttpRequest, res: HttpResponse) => Promise<void>][]} */
      const routes = [['ack', acknowledge], ['subscribe', subscribe], ['unsubscribe', unsubscribe]];
      for (const [path, handler] of routes) {
        server.middlewares.use(`/api/observe/${path}`, (req, res) => {
          res.setHeader('Content-Type', 'application/json');
          return handler(req, res);
        });
      }
      server.middlewares.use('/api/observe/', open);
    },
    /** @param {unknown} envelope @param {number} bytes */
    deliver(envelope, bytes) {
      const value = validateDelivery(envelope, bytes);
      const state = requests.get(value.requestId);
      if (!state) {
        if (wasIssued(value.requestId)) return; // Bounded monotonic retired-ID fence.
        throw new Error('Unmatched native observation.');
      }
      if ((state.token && state.token !== value.subscriptionId) || state.topic !== value.frame.topic) throw new Error('Unmatched native observation.');
      if (tokens.has(value.subscriptionId) && tokens.get(value.subscriptionId) !== state) throw new Error('Duplicate native observation token.');
      state.token = value.subscriptionId;
      tokens.set(state.token, state);
      if (state.closing) { void close(state); return; }
      if (state.current) throw new Error('Native observation exceeded one in-flight delivery.');
      if (state.lastDelivery !== undefined && BigInt(value.delivery) <= BigInt(state.lastDelivery)) throw new Error('Native observation receipt was replayed.');
      state.lastDelivery = value.delivery;
      state.current = { delivery: value.delivery, written: false };
      const current = state.current;
      write(state.connection, { requestId: state.frontendId, envelope: {
        subscriptionId: value.subscriptionId, delivery: value.delivery, frame: value.frame,
      } }, () => { current.written = true; current.resolve?.(); });
    },
    /** @param {unknown} error */
    fail(error) {
      for (const state of slots) { state.failed = true; state.closing = true; release(state); }
      for (const connection of physical) closeConnection(connection, error);
    },
  };
}
