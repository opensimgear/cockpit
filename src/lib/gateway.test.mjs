import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import test from 'node:test';
import vm from 'node:vm';
import ts from 'typescript';

const source = await readFile(new URL('./gateway.ts', import.meta.url), 'utf8');
const compiled = ts.transpileModule(source, { compilerOptions: { module: ts.ModuleKind.CommonJS, target: ts.ScriptTarget.ES2022 } }).outputText;
const first = 'stm32:00112233445566778899aabb';
const second = 'stm32:00112233445566778899aacc';
const state = { gatewayId: first, backend: 'stm32', running: false, needsReconnect: false, connected: true, modules: [] };
const plain = value => JSON.parse(JSON.stringify(value));
const tick = () => new Promise(resolve => setImmediate(resolve));
const deferred = () => { let resolve; const promise = new Promise(done => { resolve = done; }); return { promise, resolve }; };

function transport({ desktop = true, result = { ok: true, state }, invokeResults, invokeError, responseOK = true, contentType = 'application/json' } = {}) {
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
          return invokeResults ? invokeResults(command, args.request) : result;
        },
      };
      if (name === '@tauri-apps/plugin-opener') return { openUrl: async url => { calls.push({ kind: 'open', url }); } };
      throw new Error(`Unexpected import: ${name}`);
    },
    crypto: { randomUUID: () => 'test-client-uuid' }, AbortSignal,
    fetch: async (url, options) => {
      calls.push({ kind: 'fetch', url, options });
      return { ok: responseOK, headers: { get: () => contentType }, json: async () => result };
    },
  });
  return { ...exported, calls };
}

test('desktop routes the complete explicitly scoped request through one native manager IPC', async () => {
  const api = transport();
  const request = { command: 'configure', gatewayId: first, moduleId: 'same-device', settings: { label: 'Pedals', debounce: 5, inverted: false } };
  assert.equal(await api.gatewayRequest(request), state);
  await api.gatewayRequest({ command: 'claim', gatewayId: second, moduleId: request.moduleId });
  assert.deepEqual(api.calls, [
    { kind: 'invoke', command: 'gateway_request', args: { request: { ...request, clientId: 'test-client-uuid' } } },
    { kind: 'invoke', command: 'gateway_request', args: { request: { command: 'claim', gatewayId: second, moduleId: request.moduleId, clientId: 'test-client-uuid' } } },
  ]);
});

test('DCS-BIOS profile and lifecycle use the same native request in desktop and browser', async () => {
  const config = { schemaVersion: 1, cataloguePath: '/DCS-BIOS/doc/json', gatewayId: first, moduleId: 'panel',
    button: 0, ledActionId: 'led.boolean.set_enabled', commandHost: '127.0.0.1', commandPort: 7778,
    listenAddress: '0.0.0.0', listenPort: 5010, multicastGroup: '239.255.50.10', multicastInterface: '127.0.0.1', freshnessMs: 1500 };
  const result = { ok: true, state: { running: false, needsReconnect: false, modules: [],
    dcsBios: { configured: true, running: false, config, status: 'stopped' } } };
  for (const desktop of [true, false]) {
    const api = transport({ desktop, result });
    const requests = [{ command: 'dcs_configure', gatewayId: first, dcsConfig: config },
      ...['dcs_status', 'dcs_start', 'dcs_stop'].map(command => ({ command, gatewayId: first }))];
    for (const request of requests) assert.deepEqual(plain((await api.gatewayRequest(request)).dcsBios), result.state.dcsBios);
    assert.deepEqual(api.calls.map(call => desktop ? call.args.request : JSON.parse(call.options.body)),
      requests.map(request => ({ ...request, clientId: 'test-client-uuid' })));
  }
});

test('global DCS requests stay ordered while gateway and inventory requests are slow', async () => {
  const gatewayWaiting = deferred();
  const inventoryWaiting = deferred();
  const dcsWaiting = deferred();
  const api = transport({ invokeResults: async (_command, request) => {
    if (request.command === 'status') await gatewayWaiting.promise;
    if (request.command === 'ports') await inventoryWaiting.promise;
    if (request.command === 'dcs_configure') await dcsWaiting.promise;
    return { ok: true, state };
  } });
  const pendingGateway = api.gatewayRequest({ command: 'status', gatewayId: first });
  const pendingInventory = api.gatewayRequest({ command: 'ports' });
  const pendingConfig = api.gatewayRequest({ command: 'dcs_configure' });
  const pendingStop = api.gatewayRequest({ command: 'dcs_stop' });
  await tick();
  assert.deepEqual(api.calls.map(call => call.args.request.command), ['status', 'ports', 'dcs_configure']);
  dcsWaiting.resolve();
  await Promise.all([pendingConfig, pendingStop]);
  assert.equal(api.calls.at(-1).args.request.command, 'dcs_stop');
  gatewayWaiting.resolve(); inventoryWaiting.resolve();
  await Promise.all([pendingGateway, pendingInventory]);
});

test('typed multi-gateway profiles and address-free catalogue pages use the same desktop and browser API', async () => {
  const dcsConfig = { schemaVersion: 2, aircraftModule: 'A-10C', cataloguePath: '/installed/doc/json', inputs: [
    { id: 'tuning', gatewayId: first, moduleId: 'radio', kind: 'encoder', control: 0, command: 'VHFAM_FREQ1', interface: 'fixed_step' },
    { id: 'volume', gatewayId: second, moduleId: 'axes', kind: 'axis', control: 0, command: 'VHFAM_VOL', interface: 'set_state', deadband: 128 },
  ], outputs: [{ id: 'frequency', gatewayId: first, moduleId: 'radio', control: 'VHFAM_FREQ', actionId: 'display.radio.set_text', format: { kind: 'text', overflow: 'truncate' } }] };
  const catalogue = { cataloguePath: dcsConfig.cataloguePath, aircraftModule: 'A-10C', kind: 'all', query: 'VHFAM', offset: 0, total: 1,
    controls: [{ id: 'VHFAM_FREQ', label: 'Radio frequency', category: 'VHF AM', inputs: [], outputs: [{ type: 'string', maxLength: 7 }] }] };
  const result = { ok: true, state: { modules: [], dcsBios: { config: dcsConfig, catalogue } } };
  const requests = [{ command: 'dcs_configure', dcsConfig }, { command: 'dcs_catalogue', cataloguePath: dcsConfig.cataloguePath,
    aircraftModule: 'A-10C', kind: 'all', query: 'VHFAM', offset: 0, limit: 32 }];
  for (const desktop of [true,false]) {
    const api = transport({ desktop, result });
    for (const request of requests) assert.deepEqual(plain((await api.gatewayRequest(request)).dcsBios), result.state.dcsBios);
    assert.deepEqual(api.calls.map(call => desktop ? call.args.request : JSON.parse(call.options.body)), requests.map(request => ({ ...request, clientId: 'test-client-uuid' })));
  }
});

test('discovery is one aggregate request and never changes or disconnects either connection', async () => {
  const api = transport();
  await api.gatewayRequest({ command: 'connect', gatewayId: first, backend: 'stm32', port: 'COM1' });
  await api.gatewayRequest({ command: 'connect', gatewayId: second, backend: 'stm32', port: 'COM2' });
  await api.gatewayRequest({ command: 'ports' });
  await api.gatewayRequest({ command: 'status', gatewayId: first });
  assert.deepEqual(api.calls.map(call => [call.command, call.args.request.command, call.args.request.gatewayId]), [
    ['gateway_request', 'connect', first], ['gateway_request', 'connect', second],
    ['gateway_request', 'ports', undefined], ['gateway_request', 'status', first],
  ]);
});

test('a slow gateway preserves its own ordering while another gateway and discovery progress', async () => {
  const waiting = deferred();
  const api = transport({ invokeResults: async (_command, request) => {
    if (request.gatewayId === first && request.command === 'claim') await waiting.promise;
    return { ok: true, state: { ...state, gatewayId: request.gatewayId } };
  } });
  const claim = api.gatewayRequest({ command: 'claim', gatewayId: first, moduleId: 'same-device' });
  const save = api.gatewayRequest({ command: 'config_save', gatewayId: first, moduleId: 'same-device' });
  await api.gatewayRequest({ command: 'status', gatewayId: second });
  await api.gatewayRequest({ command: 'ports' });
  assert.deepEqual(api.calls.map(call => call.args.request.command), ['claim', 'status', 'ports']);
  waiting.resolve();
  await Promise.all([claim, save]);
  assert.equal(api.calls.at(-1).args.request.command, 'config_save');
});

test('a failed request preserves its returned state and does not poison that gateway queue', async () => {
  let fail = true;
  const api = transport({ invokeResults: () => fail
    ? { ok: false, state, error: 'Setup access is already claimed.' }
    : { ok: true, state } });
  await assert.rejects(api.gatewayRequest({ command: 'claim', gatewayId: first, moduleId: 'same-device' }), error => {
    assert.ok(error instanceof api.GatewayError);
    assert.equal(error.message, 'Setup access is already claimed.');
    assert.equal(error.state, state);
    return true;
  });
  fail = false;
  assert.equal(await api.gatewayRequest({ command: 'status', gatewayId: first }), state);
});

test('native IPC failure surfaces without an HTTP fallback', async () => {
  const api = transport({ invokeError: 'Native gateway service is unavailable.' });
  await assert.rejects(api.gatewayRequest({ command: 'ports' }), /Native gateway service is unavailable/);
  assert.deepEqual(api.calls.map(call => call.kind), ['invoke']);
});

test('desktop unload releases only the claimed gateway with the same owner ID', async () => {
  const api = transport();
  api.releaseOnExit('same-device', second);
  await tick();
  assert.deepEqual(api.calls, [{ kind: 'invoke', command: 'gateway_request', args: {
    request: { command: 'release', moduleId: 'same-device', gatewayId: second, clientId: 'test-client-uuid' },
  } }]);
});

test('browser forwards the same scoped contract and unload uses keepalive', async () => {
  const api = transport({ desktop: false });
  await api.gatewayRequest({ command: 'ports' });
  await api.gatewayRequest({ command: 'connect', gatewayId: first, backend: 'stm32', port: 'COM1' });
  await api.gatewayRequest({ command: 'claim', gatewayId: second, moduleId: 'same-device' });
  api.releaseOnExit('same-device', second);
  assert.ok(api.calls.every(call => call.url === '/api/gateway/' && call.kind === 'fetch'));
  assert.deepEqual(api.calls.map(call => JSON.parse(call.options.body)), [
    { command: 'ports', clientId: 'test-client-uuid' },
    { command: 'connect', gatewayId: first, backend: 'stm32', port: 'COM1', clientId: 'test-client-uuid' },
    { command: 'claim', gatewayId: second, moduleId: 'same-device', clientId: 'test-client-uuid' },
    { command: 'release', gatewayId: second, moduleId: 'same-device', clientId: 'test-client-uuid' },
  ]);
  assert.equal(api.calls.at(-1).options.keepalive, true);
});

test('debug input and exit neutral carry the exact gateway, device and boot with browser keepalive', async () => {
  const gatewayId = 'pico:OSG-PICO-0123456789ABCDEF';
  for (const desktop of [true,false]) {
    const api = transport({ desktop });
    const press = { command: 'debug_invoke', gatewayId, moduleId: 'panel', boot: 9, debugOperation: 'contact', debugArgs: [2,1] };
    await api.gatewayRequest(press);
    api.resetDebugOnExit('panel', 9, gatewayId); await tick();
    assert.deepEqual(api.calls.map(call => desktop ? call.args.request : JSON.parse(call.options.body)), [
      { ...press, clientId: 'test-client-uuid' },
      { command: 'debug_invoke', gatewayId, moduleId: 'panel', boot: 9, debugOperation: 'neutral', debugArgs: [], clientId: 'test-client-uuid' },
    ]);
    if (!desktop) assert.ok(api.calls.every(call => call.options.keepalive));
  }
});

test('browser rejects a missing gateway bridge without treating HTML as state', async () => {
  const api = transport({ desktop: false, contentType: 'text/html' });
  await assert.rejects(api.gatewayRequest({ command: 'ports' }), /pnpm web:dev/);
});

test('desktop documentation links open externally; browser links retain browser navigation', async () => {
  const api = transport();
  let prevented = 0;
  const event = href => ({ preventDefault: () => { prevented++; }, currentTarget: { getAttribute: () => href } });
  await api.openCockpitLink(event('https://www.opensimgear.org'));
  assert.equal(prevented, 1);
  assert.deepEqual(api.calls, [{ kind: 'open', url: 'https://www.opensimgear.org' }]);
  const browser = transport({ desktop: false });
  assert.equal(browser.openCockpitLink(event('https://www.opensimgear.org')), undefined);
  assert.equal(prevented, 1);
  assert.deepEqual(browser.calls, []);
});

test('host controller reads bypass a blocked gateway and inventory queue', async () => {
  const blocked = deferred();
  const api = transport({ invokeResults: async (_command, request) => {
    if (request.command === 'ports') await blocked.promise;
    return { ok: true, state: { ...state, controllers: { sources: [], total: 0 } } };
  } });
  const inventory = api.gatewayRequest({ command: 'ports' });
  const controllers = await api.gatewayRequest({ command: 'controller_list' });
  assert.equal(controllers.controllers.total, 0);
  blocked.resolve(); await inventory;
});