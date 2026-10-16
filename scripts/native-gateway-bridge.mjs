// @ts-nocheck -- Node-only development transport; UI requests use typed gateway.ts.
// A local browser forwards to the same native gateway services as the desktop app.
import { spawn } from 'node:child_process';
import { isAbsolute, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { StringDecoder } from 'node:string_decoder';
import { randomUUID } from 'node:crypto';

const profileRequestLimit = 524_288 + 65_536;
const responseEnvelopeLimit = 2_097_152;

/** @returns {import('vite').Plugin} */
export function nativeGatewayBridge({ spawnProcess = spawn } = {}) {
  let child;
  let buffer = '';
  let fail;
  const pending = new Map();
  const endpoint = '/api/gateway/';
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
        const task = envelope && typeof envelope === 'object' ? pending.get(envelope.requestId) : undefined;
        if (!task || !envelope.response || typeof envelope.response !== 'object' || Array.isArray(envelope.response)) {
          failProcess(new Error('Native service returned an unmatched response.')); return;
        }
        if (!task.profile && Buffer.byteLength(line) > 131_072) {
          failProcess(new Error('Native service response exceeds its limit.')); return;
        }
        pending.delete(envelope.requestId);
        clearTimeout(task.timer);
        task.resolve(envelope.response);
      }
      if (Buffer.byteLength(buffer) > responseEnvelopeLimit) failProcess(new Error('Native service response exceeds its limit.'));
    });
  }
  function request(payload) {
    return new Promise((resolve, reject) => {
      try {
        start();
        const requestChild = child;
        const failRequestProcess = fail;
        const timer = setTimeout(() => failRequestProcess(new Error('Native gateway service did not reply.')), 130_000);
        const requestId = randomUUID();
        pending.set(requestId, { resolve, reject, timer, profile: typeof payload?.command === 'string' && payload.command.startsWith('profile_') });
        requestChild.stdin.write(`${JSON.stringify({ requestId, request: payload })}\n`, error => { if (error) failRequestProcess(error); });
      } catch (error) { reject(error); }
    });
  }
  return {
    name: 'osg-native-gateway',
    configureServer(server) {
      server.httpServer?.once('close', () => { child?.stdin.end(); });
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
            if (bodySize > profileRequestLimit) throw new Error('Profile request exceeds its size limit.');
            chunks.push(bytes);
          }
          const payload = JSON.parse(Buffer.concat(chunks).toString('utf8'));
          const profile = typeof payload?.command === 'string' && payload.command.startsWith('profile_');
          if (!profile && bodySize > 65_536) throw new Error('Gateway request exceeds 65536 bytes.');
          res.end(JSON.stringify(await request(payload)));
        } catch (error) {
          res.statusCode = 400;
          res.end(JSON.stringify({ ok: false, error: error.message }));
        }
      });
    },
  };
}
