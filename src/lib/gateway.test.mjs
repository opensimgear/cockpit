import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import test from 'node:test';
import vm from 'node:vm';
import ts from 'typescript';

const source = await readFile(new URL('./gateway.ts', import.meta.url), 'utf8');
const compiled = ts.transpileModule(source, { compilerOptions: { module: ts.ModuleKind.CommonJS, target: ts.ScriptTarget.ES2022 } }).outputText;
const state = { running: false, needsReconnect: false, connected: true, modules: [] };
const plain = value => JSON.parse(JSON.stringify(value));

function transport({ desktop = true, result = { ok: true, state }, invokeError, responseOK = true, contentType = 'application/json' } = {}) {
  const calls = [];
  const exported = {};
  vm.runInNewContext(compiled, {
    exports: exported,
    require(name) {
      if (name === '@tauri-apps/api/core') return {
        isTauri: () => desktop,
        invoke: async (command, args) => {
          calls.push({ kind: 'invoke', command, ...(args ? { args: plain(args) } : {}) });
          if (invokeError) throw invokeError;
          return result;
        },
      };
      if (name === '@tauri-apps/plugin-opener') return { openUrl: async url => { calls.push({ kind: 'open', url }); } };
      throw new Error(`Unexpected import: ${name}`);
    },
    crypto: { randomUUID: () => 'test-client-uuid' },
    AbortSignal,
    fetch: async (url, options) => {
      calls.push({ kind: 'fetch', url, options });
      return { ok: responseOK, headers: { get: () => contentType }, json: async () => result };
    },
  });
  return { ...exported, calls };
}

test('desktop Pico requests use native IPC with the complete request and stable owner ID', async () => {
  const api = transport();
  const request = { command: 'input', moduleId: 'OSG-PD-01', buttons: [], axes: [65535, 123, 0] };
  assert.equal(await api.gatewayRequest(request), state);
  await api.gatewayRequest({ command: 'claim', moduleId: request.moduleId });
  assert.deepEqual(api.calls, [
    { kind: 'invoke', command: 'pico_gateway', args: { request: { ...request, clientId: 'test-client-uuid' } } },
    { kind: 'invoke', command: 'pico_gateway', args: { request: { command: 'claim', moduleId: request.moduleId, clientId: 'test-client-uuid' } } },
  ]);
});

test('desktop CoreHID requests use its native supervisor', async () => {
  const api = transport();
  await api.gatewayRequest({ command: 'status' }, 'corehid');
  assert.equal(api.calls[0].command, 'corehid_gateway');
  assert.ok(api.calls.every(call => call.kind === 'invoke'));
});

test('native gateway command errors preserve returned state for UI reconciliation', async () => {
  const api = transport({ result: { ok: false, state, error: 'Setup access is already claimed.' } });
  await assert.rejects(api.gatewayRequest({ command: 'claim', moduleId: 'OSG-PD-01' }), error => {
    assert.ok(error instanceof api.GatewayError);
    assert.equal(error.message, 'Setup access is already claimed.');
    assert.equal(error.state, state);
    return true;
  });
});

test('native IPC failure surfaces an error without falling back to HTTP', async () => {
  const api = transport({ invokeError: 'Native gateway service is unavailable.' });
  await assert.rejects(api.gatewayRequest({ command: 'status' }), /Native gateway service is unavailable/);
  assert.deepEqual(api.calls.map(call => call.kind), ['invoke']);
});

test('claim release during desktop unload uses IPC and the same owner ID', async () => {
  const api = transport();
  api.releaseOnExit('OSG-SQ-01', 'pico');
  await new Promise(resolve => setImmediate(resolve));
  assert.deepEqual(api.calls, [{ kind: 'invoke', command: 'pico_gateway', args: { request: { command: 'release', moduleId: 'OSG-SQ-01', clientId: 'test-client-uuid' } } }]);
});

test('explicit browser mode retains both bridge routes and unload keepalive', async () => {
  const api = transport({ desktop: false });
  await api.gatewayRequest({ command: 'ports' }, 'pico');
  await api.gatewayRequest({ command: 'status' }, 'corehid');
  api.releaseOnExit('OSG-SQ-01', 'pico');
  assert.deepEqual(api.calls.map(call => call.url), ['/api/pico-gateway/', '/api/gateway/', '/api/pico-gateway/']);
  assert.ok(api.calls.every(call => call.kind === 'fetch'));
  assert.equal(api.calls[2].options.keepalive, true);
  assert.deepEqual(JSON.parse(api.calls[2].options.body), { command: 'release', moduleId: 'OSG-SQ-01', clientId: 'test-client-uuid' });
});

test('desktop links open the packaged guide or external browser without navigating the main app', async () => {
  const api = transport();
  let prevented = 0;
  const event = href => ({ preventDefault: () => { prevented++; }, currentTarget: { getAttribute: () => href } });
  await api.openCockpitLink(event('/pico-flashing.html'));
  await api.openCockpitLink(event('https://www.opensimgear.org'));
  assert.equal(prevented, 2);
  assert.deepEqual(api.calls, [
    { kind: 'invoke', command: 'open_pico_flashing_guide' },
    { kind: 'open', url: 'https://www.opensimgear.org' },
  ]);
});

test('browser links retain standard browser navigation', () => {
  const api = transport({ desktop: false });
  let prevented = false;
  assert.equal(api.openCockpitLink({ preventDefault: () => { prevented = true; } }), undefined);
  assert.equal(prevented, false);
  assert.deepEqual(api.calls, []);
});
