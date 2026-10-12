// @ts-nocheck -- Node-only development transport; UI requests use typed gateway.ts.
// Local development: actual browser UI -> Rust service -> production C emulator.
import { spawn } from 'node:child_process';
import { resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

/** @returns {import('vite').Plugin} */
export function nativeEmulatorBridge() {
  let child;
  let buffer = '';
  const pending = [];
  let queue = Promise.resolve();
  function start() {
    if (child) return;
    const workspace = fileURLToPath(new URL('../../', import.meta.url));
    const executable = process.env.OSG_EMULATOR_SERVICE ?? resolve(workspace, '.research/cockpit-native-target/debug/osg-emulator-service');
    child = spawn(executable, [], { stdio: ['pipe', 'pipe', 'inherit'], env: process.env });
    const fail = error => { for (const task of pending.splice(0)) task.reject(error); child = undefined; };
    child.once('error', fail);
    child.stdin.on('error', fail);
    child.once('exit', () => fail(new Error('Native emulator service exited.')));
    child.stdout.on('data', bytes => {
      buffer += bytes.toString('utf8');
      if (buffer.length > 65_536) { fail(new Error('Native service response exceeds its limit.')); return; }
      let newline;
      while ((newline = buffer.indexOf('\n')) >= 0) {
        const line = buffer.slice(0, newline); buffer = buffer.slice(newline + 1);
        const task = pending.shift();
        if (task) { try { task.resolve(JSON.parse(line)); } catch { task.reject(new Error('Native service returned invalid JSON.')); } }
      }
    });
  }
  function request(payload) {
    const operation = queue.then(() => new Promise((resolve, reject) => {
      start(); pending.push({ resolve, reject });
      child.stdin.write(`${JSON.stringify(payload)}\n`, error => { if (error) reject(error); });
    }));
    queue = operation.catch(() => {}); return operation;
  }
  return {
    name: 'osg-native-emulator',
    transformIndexHtml() { return [{ tag: 'script', children: 'window.__OSG_NATIVE_EMULATOR__=true;', injectTo: 'head-prepend' }]; },
    configureServer(server) {
      server.httpServer?.once('close', () => { child?.stdin.end(); });
      server.middlewares.use('/api/native-gateway/', async (req, res) => {
        res.setHeader('Content-Type', 'application/json');
        try {
          const origin = new URL(req.headers.origin ?? 'http://invalid');
          if (req.method !== 'POST' || req.headers['x-osg-simulator'] !== '1' || !['127.0.0.1','localhost'].includes(origin.hostname) || origin.host !== req.headers.host) {
            res.statusCode = 403; res.end(JSON.stringify({ ok: false, error: 'Only the local Cockpit emulator UI may access this service.' })); return;
          }
          let body = ''; for await (const bytes of req) { body += bytes; if (Buffer.byteLength(body)>16_384) throw new Error('Gateway request exceeds 16384 bytes.'); }
          res.end(JSON.stringify(await request(JSON.parse(body))));
        } catch (error) { res.statusCode=400; res.end(JSON.stringify({ok:false,error:error.message})); }
      });
    },
  };
}
