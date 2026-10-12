// @ts-nocheck -- Node-only Vite integration; browser sources use the typed client.
import { spawn } from 'node:child_process';
import { createInterface } from 'node:readline';
import { fileURLToPath } from 'node:url';
import { randomUUID } from 'node:crypto';
import { access } from 'node:fs/promises';
import { constants } from 'node:fs';
import { isAbsolute } from 'node:path';

const packagePath = fileURLToPath(new URL('./macos/', import.meta.url));
const binaryPath = fileURLToPath(new URL('./macos/.build/debug/osg-gateway-simulator', import.meta.url));
const commands = new Set(['status', 'start', 'stop', 'input', 'register', 'reconnect', 'presence', 'configure']);

export function gatewaySimulator() {
  let child;
  let launching;
  let sequence = Promise.resolve();
  let closed = false;
  let generation = 0;
  let buildProcess;
  const pending = new Map();
  const claims = new Map();
  let diagnostics = '';

  function failPending(message) {
    for (const { reject, timer } of pending.values()) { clearTimeout(timer); reject(new Error(message)); }
    pending.clear();
    claims.clear();
  }
  function close() {
    closed = true;
    generation++;
    buildProcess?.kill('SIGTERM');
    buildProcess = undefined;
    child?.kill('SIGTERM');
    child = undefined;
    failPending('Gateway simulator stopped.');
  }
  async function launch() {
    if (closed) throw new Error('Cockpit server has closed.');
    if (child) return;
    if (launching) return launching;
    launching = (async () => {
      const launchGeneration = generation;
      if (process.platform !== 'darwin') throw new Error('CoreHID simulation requires macOS 15 or later and Xcode command-line tools.');
      const configuredHelper = process.env.OSG_HID_HELPER;
      if (configuredHelper) {
        if (!isAbsolute(configuredHelper)) throw new Error('OSG_HID_HELPER must be the absolute path to a signed helper.');
        await access(configuredHelper, constants.X_OK);
      } else await new Promise((resolve, reject) => {
        const build = spawn('xcrun', ['swift', 'build', '--package-path', packagePath], { stdio: ['ignore', 'pipe', 'pipe'] });
        buildProcess = build;
        let output = '';
        const timer = setTimeout(() => { build.kill(); reject(new Error('Swift compilation timed out. Run swift build in simulator/macos to inspect diagnostics.')); }, 120_000);
        build.stdout.on('data', data => { output = (output + data).slice(-6000); });
        build.stderr.on('data', data => { output = (output + data).slice(-6000); });
        build.once('error', error => { clearTimeout(timer); reject(error); });
        build.once('exit', code => { buildProcess = undefined; clearTimeout(timer); code === 0 ? resolve() : reject(new Error(`CoreHID helper could not build: ${output.trim()}`)); });
      });
      if (closed || generation !== launchGeneration) throw new Error('Cockpit server closed before the helper could start.');
      diagnostics = '';
      const processHandle = spawn(configuredHelper || binaryPath, [], { stdio: ['pipe', 'pipe', 'pipe'] });
      child = processHandle;
      processHandle.stderr.on('data', data => { diagnostics = (diagnostics + data).slice(-3000); });
      attachResponses(processHandle.stdout);
      processHandle.once('error', error => { if (child === processHandle) { child = undefined; failPending(error.message); } });
      processHandle.once('exit', (code, signal) => {
        if (child === processHandle) { child = undefined; failPending(`CoreHID helper exited (${signal ?? code}). ${diagnostics}`.trim()); }
      });
    })().finally(() => { launching = undefined; });
    return launching;
  }
  function attachResponses(stream) {
      createInterface({ input: stream }).on('line', line => {
        try {
          const response = JSON.parse(line);
          const request = pending.get(response.id);
          if (!request) return;
          clearTimeout(request.timer);
          pending.delete(response.id);
          request.resolve(response);
        } catch { /* Native diagnostics belong on stderr; ignore malformed stdout. */ }
      });
  }
  async function send(request) {
    await launch();
    return new Promise((resolve, reject) => {
      const id = randomUUID();
      const timer = setTimeout(() => {
        const unresponsive = child;
        child = undefined;
        unresponsive?.kill('SIGTERM');
        failPending('CoreHID helper did not respond and was stopped. Retry to start a fresh helper.');
      }, 10_000);
      pending.set(id, { resolve, reject, timer });
      child.stdin.write(`${JSON.stringify({ ...request, id })}\n`, error => {
        if (error) { clearTimeout(timer); pending.delete(id); reject(error); }
      });
    });
  }
  async function handle(request) {
    const { clientId, command, moduleId } = request;
    if (typeof clientId !== 'string' || !/^[\w-]{16,80}$/.test(clientId)) throw new Error('Missing simulator client identity. Reload Cockpit.');
    const now = Date.now();
    for (const [id, lease] of claims) {
      if (now - lease.lastSeen > 15_000) claims.delete(id);
      else if (lease.clientId === clientId) lease.lastSeen = now;
    }
    const decorate = result => {
      if (result.state) result.state.claimedModules = [...claims].filter(([, lease]) => lease.clientId === clientId).map(([id]) => id);
      return result;
    };
    if (command === 'claim') {
      const owner = claims.get(moduleId);
      if (owner && owner.clientId !== clientId) throw new Error('Another Cockpit tab owns this module’s setup session.');
      const result = await send({ command: 'status' });
      const module = result.state?.modules.find(module => module.id === moduleId);
      if (!module?.online) throw new Error('Reconnect this module before starting setup.');
      claims.set(moduleId, { clientId, lastSeen: Date.now() });
      return decorate(result);
    }
    if (command === 'release') {
      if (claims.get(moduleId)?.clientId === clientId) claims.delete(moduleId);
      return decorate(await send({ command: 'status' }));
    }
    if (!commands.has(command)) throw new Error('Unknown simulator command.');
    if (command === 'configure' && claims.get(moduleId)?.clientId !== clientId) throw new Error('Start a setup session before saving this module.');
    if (['register', 'reconnect', 'stop'].includes(command) && claims.size) throw new Error('End active setup sessions before changing the gateway layout.');
    const result = await send(request);
    if (result.ok && command === 'presence' && !request.online) claims.delete(moduleId);
    return decorate(result);
  }
  function install(server) {
    server.httpServer?.once('close', close);
    server.middlewares.use('/api/gateway', async (req, res) => {
      res.setHeader('Content-Type', 'application/json');
      res.setHeader('Cache-Control', 'no-store');
      const host = req.headers.host ?? '';
      const loopback = ['127.0.0.1', '::1', '::ffff:127.0.0.1'].includes(req.socket.remoteAddress);
      if (!loopback || !/^(localhost|127\.0\.0\.1|\[::1\])(?::\d+)?$/.test(host) ||
        (req.headers.origin && req.headers.origin !== `http://${host}`) || req.headers['x-osg-simulator'] !== '1') {
        res.statusCode = 403;
        res.end(JSON.stringify({ ok: false, error: 'The simulator accepts requests from this local Cockpit only.' }));
        return;
      }
      if (req.method !== 'POST' || req.url !== '/' || !req.headers['content-type']?.startsWith('application/json')) {
        res.statusCode = 405; res.end(JSON.stringify({ ok: false, error: 'Use POST application/json.' })); return;
      }
      try {
        let body = '';
        for await (const chunk of req) {
          body += chunk;
          if (Buffer.byteLength(body) > 8192) throw new Error('Simulator request is too large.');
        }
        const request = JSON.parse(body);
        const operation = sequence.then(() => handle(request));
        sequence = operation.catch(() => {});
        res.end(JSON.stringify(await operation));
      } catch (error) {
        res.statusCode = 400;
        res.end(JSON.stringify({ ok: false, error: error instanceof Error ? error.message : 'Simulator request failed.' }));
      }
    });
  }
  return { name: 'opensimgear-corehid-simulator', configureServer: install, configurePreviewServer: install };
}
