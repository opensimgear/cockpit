// @ts-nocheck -- Node-only development transport; UI requests use typed gateway.ts.
// A local browser forwards to the same native gateway services as the desktop app.
import { spawn } from 'node:child_process';
import { isAbsolute, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { StringDecoder } from 'node:string_decoder';
import { randomUUID } from 'node:crypto';
import { observationBridge } from './observations/bridge.mjs';

const profileRequestLimit = 524_288 + 65_536;
const editorRequestLimit = 2_097_152;
const responseEnvelopeLimit = 16_777_216;

/** @returns {import('vite').Plugin} */
export function nativeGatewayBridge({ spawnProcess = spawn } = {}) {
  let child;
  let buffer = '';
  let fail;
  const pending = new Map();
  let observationCounter = 0n;
  const endpoint = '/api/gateway/';
  const observations = observationBridge(
    (payload, beforeWrite) => request(payload, { observation: true, beforeWrite }),
    id => /^observation:[1-9][0-9]{0,19}$/.test(id) && BigInt(id.slice(12)) <= observationCounter,
  );
  function start() {
    if (child) return;
    const repository = fileURLToPath(new URL('../', import.meta.url));
    const executable = process.env.OSG_GATEWAY_SERVICE ?? resolve(repository, '.e2e/build/cockpit/debug/osg-gateway-service');
    if (!isAbsolute(executable)) throw new Error('The native gateway service must use an absolute executable path.');
    buffer = '';
    const processChild = spawnProcess(executable, [], { stdio: ['pipe', 'pipe', 'inherit'], env: process.env });
    const decoder = new StringDecoder('utf8');
    child = processChild;
    const failProcess = error => {
      if (child !== processChild) return;
      for (const task of pending.values()) { clearTimeout(task.timer); task.reject(error); }
      pending.clear();
      observations.fail(error);
      child = undefined;
      processChild.kill();
    };
    fail = failProcess;
    processChild.once('error', failProcess);
    processChild.stdin.on('error', failProcess);
    processChild.once('exit', () => failProcess(new Error('Native gateway service exited.')));
    processChild.stdout.on('data', bytes => {
      if (child !== processChild) return;
      buffer += decoder.write(bytes);
      let newline;
      while ((newline = buffer.indexOf('\n')) >= 0) {
        const line = buffer.slice(0, newline); buffer = buffer.slice(newline + 1);
        if (Buffer.byteLength(line) > responseEnvelopeLimit) { failProcess(new Error('Native service response exceeds its limit.')); return; }
        let envelope;
        try { envelope = JSON.parse(line); }
        catch { failProcess(new Error('Native service returned invalid JSON.')); return; }
        if (envelope && Object.hasOwn(envelope, 'observation')) {
          try { observations.deliver(envelope, Buffer.byteLength(line)); }
          catch (error) { failProcess(error); return; }
          continue;
        }
        const task = envelope && typeof envelope === 'object' ? pending.get(envelope.requestId) : undefined;
        if (!task || !envelope.response || typeof envelope.response !== 'object' || Array.isArray(envelope.response)) {
          failProcess(new Error('Native service returned an unmatched response.')); return;
        }
        if (Buffer.byteLength(line) > task.responseLimit) {
          failProcess(new Error('Native service response exceeds its limit.')); return;
        }
        pending.delete(envelope.requestId);
        clearTimeout(task.timer);
        task.resolve(envelope.response);
      }
      if (Buffer.byteLength(buffer) > responseEnvelopeLimit) failProcess(new Error('Native service response exceeds its limit.'));
    });
  }
  function request(payload, { observation = false, beforeWrite } = {}) {
    return new Promise((resolve, reject) => {
      try {
        start();
        const requestChild = child;
        const failRequestProcess = fail;
        if (observation && payload.operation === 'subscribe' && observationCounter === 18446744073709551615n) {
          throw new Error('Observation request counter exhausted.');
        }
        const timer = setTimeout(() => failRequestProcess(new Error('Native gateway service did not reply.')), 130_000);
        const requestId = observation && payload.operation === 'subscribe'
          ? `observation:${++observationCounter}` : randomUUID();
        const command = typeof payload?.command === 'string' ? payload.command : '';
        const responseLimit = command.startsWith('game_') ? responseEnvelopeLimit : command.startsWith('profile_') ? 2_097_152 : 131_072;
        pending.set(requestId, { resolve, reject, timer, responseLimit });
        beforeWrite?.(requestId);
        const envelope = observation ? { requestId, observation: payload } : { requestId, request: payload };
        requestChild.stdin.write(`${JSON.stringify(envelope)}\n`, error => { if (error) failRequestProcess(error); });
      } catch (error) { reject(error); }
    });
  }
  return {
    name: 'osg-native-gateway',
    configureServer(server) {
      observations.configure(server);
      server.httpServer?.once('close', () => {
        const error = new Error('Development transport closed.');
        observations.fail(error);
        child?.stdin.end();
        if (child) fail(error);
      });
      server.middlewares.use(endpoint, async (req, res) => {
        res.setHeader('Content-Type', 'application/json');
        try {
          const origin = new URL(req.headers.origin ?? 'http://invalid');
          if (req.method !== 'POST' || req.headers['x-osg-client'] !== '1' || !['127.0.0.1','localhost'].includes(origin.hostname) || origin.host !== req.headers.host) {
            res.statusCode = 403;
            res.end(JSON.stringify({ ok: false, error: 'Only the local Cockpit development UI may access this service.' }));
            return;
          }
          const chunks = [];
          let bodySize = 0;
          for await (const bytes of req) {
            bodySize += bytes.length;
            if (bodySize > editorRequestLimit) throw new Error('Profile request exceeds its size limit.');
            chunks.push(bytes);
          }
          const payload = JSON.parse(Buffer.concat(chunks).toString('utf8'));
          const profile = typeof payload?.command === 'string' && payload.command.startsWith('profile_');
          const remembered = payload?.command === 'ui_preferences_save' && payload.key === 'remembered';
          if (profile && payload.command !== 'profile_editor_save' && bodySize > profileRequestLimit) throw new Error('Profile request exceeds its size limit.');
          if (!profile && !remembered && bodySize > 65_536) throw new Error('Gateway request exceeds 65536 bytes.');
          res.end(JSON.stringify(await request(payload)));
        } catch (error) {
          res.statusCode = 400;
          res.end(JSON.stringify({ ok: false, error: error.message }));
        }
      });
    },
  };
}
