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

test('unified profile writes remain ordered without blocking gateway discovery or capture', async () => {
  const waiting = deferred();
  const api = transport({ invokeResults: async (_command, request) => {
    if (request.command === 'profile_save') await waiting.promise;
    return { ok: true, state: { profiles: { document: request.document } } };
  } });
  const document = { id: 'profile', revision: 1 };
  const save = api.gatewayRequest({ command: 'profile_save', document, expectedRevision: 1 });
  const rename = api.gatewayRequest({ command: 'profile_rename', id: 'profile', name: 'Changed', expectedRevision: 2 });
  const inventory = api.gatewayRequest({ command: 'ports' });
  const controllers = api.gatewayRequest({ command: 'controller_list' });
  await tick();
  assert.deepEqual(api.calls.map(call => call.args.request.command), ['profile_save', 'ports', 'controller_list']);
  waiting.resolve();
  await Promise.all([save, rename, inventory, controllers]);
  assert.equal(api.calls.at(-1).args.request.command, 'profile_rename');
  assert.deepEqual(api.calls[0].args.request.document, document);
});

test('read-only applied inspection neither waits for nor blocks profile controls, Apply or saved edits', async () => {
  const pending=deferred();
  const api=transport({invokeResults:async(_command,request)=>{
    if(request.command==='profile_runtime_inspect')await pending.promise;
    return {ok:true,state};
  }});
  const owner={session:'owner-session',generation:1,profileId:'profile',profileRevision:3,gameId:null};
  const inspecting=api.gatewayRequest({command:'profile_runtime_inspect',owner,nodeId:'input',offset:0,limit:32});
  await tick();
  await api.gatewayRequest({command:'profile_stop'});
  await api.gatewayRequest({command:'profile_status'});
  await api.gatewayRequest({command:'profile_save',document:{id:'profile',revision:3},expectedRevision:3});
  await api.gatewayRequest({command:'profile_apply',id:'profile',expectedRevision:4});
  assert.deepEqual(api.calls.map(call=>call.args.request.command),['profile_runtime_inspect','profile_stop','profile_status','profile_save','profile_apply']);
  assert.deepEqual(api.calls[0].args.request.owner,owner);
  pending.resolve();await inspecting;
});
test('learning cached status and priority cancel bypass a blocked start, and controls cancel unsent starts',async()=>{
  for(const command of ['profile_input_learn_cancel','profile_apply','mapping_preview_start','action_invoke']){
    const wait=deferred(),api=transport({invokeResults:async(_command,request)=>{if(request.command==='profile_input_learn_start')await wait.promise;return{ok:true,state};}});
    const fields={resourceId:'physical',generation:'1',capabilityRevision:'sha256:'+'1'.repeat(64)},active=api.gatewayRequest({command:'profile_input_learn_start',...fields});
    const old=api.gatewayRequest({command:'profile_input_learn_start',...fields}),rejected=assert.rejects(old,/learning Start was cancelled/);await tick();
    await api.gatewayRequest({command:'profile_input_learn_status',learnId:'current'});await api.gatewayRequest({command,...(command==='profile_input_learn_cancel'?{learnId:'current'}:{})});
    assert.equal(api.calls.filter(call=>call.args.request.command==='profile_input_learn_start').length,1);wait.resolve();await Promise.all([active,rejected]);
  }
});

test('profile runtime controls and status bypass slow Apply while older unsent Apply is cancelled',async()=>{
  const active=deferred();
  const api=transport({invokeResults:async(_command,request)=>{
    if(request.command==='profile_apply')await active.promise;
    return{ok:true,state:{profiles:{}}};
  }});
  const firstApply=api.gatewayRequest({command:'profile_apply',id:'first',expectedRevision:1});
  const queuedApply=api.gatewayRequest({command:'profile_apply',id:'old-queued',expectedRevision:1});
  const rejected=assert.rejects(queuedApply,/cancelled by Stop, Pause or recovery/);
  await tick();
  await api.gatewayRequest({command:'profile_status'});
  await api.gatewayRequest({command:'profile_stop'});
  await api.gatewayRequest({command:'profile_editor_save',document:{id:'draft'},expectedRevision:1,assignments:[]});
  assert.deepEqual(api.calls.map(call=>call.args.request.command),['profile_apply','profile_status','profile_stop','profile_editor_save']);
  active.resolve();await Promise.all([firstApply,rejected]);
  assert.equal(api.calls.filter(call=>call.args.request.command==='profile_apply').length,1);
});

test('new explicit Apply waits for prior Save and priority control completion without replaying cancelled requests',async()=>{
  for(const command of ['profile_stop','profile_pause','profile_recover','mapping_stop','integration_stop','dcs_stop','controller_visibility_restore']){
    const saved=deferred(),stopping=deferred();
    const api=transport({invokeResults:async(_name,request)=>{
      if(request.command==='profile_editor_save')await saved.promise;
      if(request.command===command)await stopping.promise;
      return{ok:true,state};
    }});
    const save=api.gatewayRequest({command:'profile_editor_save',document:{id:'draft'},expectedRevision:1,assignments:[]});
    const old=api.gatewayRequest({command:'profile_apply',id:'cancelled',expectedRevision:2});
    const rejected=assert.rejects(old,/cancelled by Stop, Pause or recovery/);
    const stop=api.gatewayRequest({command,...(command==='profile_pause'?{paused:true}:{})});
    const fresh=api.gatewayRequest({command:'profile_apply',id:'fresh',expectedRevision:2});
    await api.gatewayRequest({command:'profile_status'});
    assert.deepEqual(api.calls.map(call=>call.args.request.command),['profile_editor_save',command,'profile_status']);
    saved.resolve();await Promise.all([save,rejected]);await tick();
    assert.equal(api.calls.some(call=>call.args.request.id==='fresh'),false);
    stopping.resolve();await Promise.all([stop,fresh]);
    assert.deepEqual(api.calls.filter(call=>call.args.request.command==='profile_apply').map(call=>call.args.request.id),['fresh']);
  }
});

test('compiler previews follow profile edits without blocking independent inventory or activating', async () => {
  const waiting = deferred();
  const api = transport({ invokeResults: async (_command, request) => {
    if (request.command === 'profile_save') await waiting.promise;
    return { ok: true, state: { profileCompilation: { compilationId: 'preview' } } };
  } });
  const save = api.gatewayRequest({ command: 'profile_save', document: { id: 'profile', revision: 1 }, expectedRevision: 1 });
  const compile = api.gatewayRequest({ command: 'profile_compile', document: { id: 'profile', revision: 2 } });
  const inspect = api.gatewayRequest({ command: 'profile_compilation_inspect', compilationId: 'preview', section: 'diagnostics', offset: 0, limit: 32 });
  const inventory = api.gatewayRequest({ command: 'profile_resources_list', offset: 0, limit: 32 });
  await tick();
  assert.deepEqual(api.calls.map(call => call.args.request.command), ['profile_save', 'profile_resources_list']);
  waiting.resolve();
  await Promise.all([save, compile, inspect, inventory]);
  assert.deepEqual(api.calls.slice(-2).map(call => call.args.request.command), ['profile_compile', 'profile_compilation_inspect']);
  assert.ok(api.calls.every(call => call.args.request.command !== 'profile_apply'));
});

test('profile resource inspection and token checks use their own queue without activating profiles', async () => {
  const waiting = deferred();
  const api = transport({ invokeResults: async (_command, request) => {
    if (request.command === 'profile_save') await waiting.promise;
    return { ok: true, state: { profileResources: { valid: true } } };
  } });
  const save = api.gatewayRequest({ command: 'profile_save', document: { id: 'profile', revision: 1 }, expectedRevision: 1 });
  const inventory = api.gatewayRequest({ command: 'profile_resources_list', offset: 0, limit: 32 });
  const resourceToken = { inventorySession: 'i', resourceId: 'r', generation: '9007199254740999', capabilityRevision: 'c', routeId: 'route', providerSession: 'p', providerGeneration: 'g', portId: 'axis:0', contractHash: 'hash', access: 'read' };
  const validate = api.gatewayRequest({ command: 'profile_resources_validate', resourceToken, portId: 'axis:0', access: 'read' });
  const capture = api.gatewayRequest({ command: 'controller_list' });
  await tick();
  assert.deepEqual(api.calls.map(call => call.args.request.command), ['profile_save', 'profile_resources_list', 'controller_list', 'profile_resources_validate']);
  assert.deepEqual(api.calls.at(-1).args.request.resourceToken, resourceToken);
  waiting.resolve();
  await Promise.all([save, inventory, validate, capture]);
  assert.ok(api.calls.every(call => call.args.request.command !== 'profile_apply'));
});

test('standalone USB endpoints have independent queues and Disconnect cancels older unsent writes', async () => {
  const reading = deferred();
  const api = transport({ invokeResults: async (_command, request) => {
    if (request.key === 'physical-a:COM8' && request.command === 'module_usb_status') await reading.promise;
    return { ok: true, state: { moduleUsb: { module: { key: request.key } } } };
  } });
  const route = { moduleId: 'module', serial: 'serial', connectionGeneration: 5, boot: 2, configurationGeneration: 4 };
  const read = api.gatewayRequest({ command: 'module_usb_status', key: 'physical-a:COM8' });
  await tick();
  const write = api.gatewayRequest({ command: 'module_usb_config_apply', key: 'physical-a:COM8', route, config: {}, save: true });
  const cancelled = assert.rejects(write, /cancelled by Disconnect/);
  const disconnect = api.gatewayRequest({ command: 'module_usb_disconnect', key: 'physical-a:COM8' });
  await api.gatewayRequest({ command: 'module_usb_status', key: 'physical-b:COM9' });
  await api.gatewayRequest({ command: 'module_usb_ports' });
  await api.gatewayRequest({ command: 'profile_resources_list', offset: 0, limit: 32 });
  assert.deepEqual(api.calls.map(call => call.args.request.command), ['module_usb_status', 'module_usb_status', 'module_usb_ports', 'profile_resources_list']);
  reading.resolve(); await Promise.all([read, cancelled, disconnect]);
  assert.equal(api.calls.at(-1).args.request.command, 'module_usb_disconnect');
  assert.ok(api.calls.every(call => call.args.request.command !== 'module_usb_config_apply'));
  await api.gatewayRequest({ command: 'module_usb_config_read', key: 'physical-a:COM8', route });
  assert.deepEqual(api.calls.at(-1).args.request.route, route);
});

test('a new explicit USB Connect waits for Disconnect and captures the observed endpoint key and path', async () => {
  const stopping = deferred();
  const api = transport({ invokeResults: async (_command, request) => {
    if (request.command === 'module_usb_disconnect') await stopping.promise;
    return { ok: true, state: { moduleUsb: {} } };
  } });
  const stop = api.gatewayRequest({ command: 'module_usb_disconnect', key: 'physical-a:COM8' });
  const connect = api.gatewayRequest({ command: 'module_usb_connect', key: 'physical-a:COM8', port: 'COM8' });
  await tick();
  await api.gatewayRequest({ command: 'module_usb_connect', key: 'physical-b:COM9', port: 'COM9' });
  assert.deepEqual(api.calls.map(call => call.args.request.key), ['physical-a:COM8', 'physical-b:COM9']);
  stopping.resolve(); await Promise.all([stop, connect]);
  assert.deepEqual(api.calls.at(-1).args.request, { command: 'module_usb_connect', key: 'physical-a:COM8', port: 'COM8', clientId: 'test-client-uuid' });
});

test('a lost USB action reply is surfaced once and does not poison a later explicit read', async () => {
  const api = transport({ invokeResults: async (_command, request) => request.command === 'module_usb_actions_invoke'
    ? { ok: false, error: 'Reply lost; output state is unknown', state: { moduleUsb: { module: { snapshot: { error: 'unknown' } } } } }
    : { ok: true, state: { moduleUsb: {} } } });
  await assert.rejects(api.gatewayRequest({ command: 'module_usb_actions_invoke', key: 'physical-a:COM8', route: {}, catalogueRevision: 4, actionId: 'led.set', values: [true] }), /state is unknown/);
  await api.gatewayRequest({ command: 'module_usb_actions_state', key: 'physical-a:COM8', route: {} });
  assert.deepEqual(api.calls.map(call => call.args.request.command), ['module_usb_actions_invoke', 'module_usb_actions_state']);
});

test('native game file selection returns the absolute path or cancellation and does not change installations', async () => {
  const path = String.raw`D:\Games\X-Plane 12\X-Plane.exe`;
  const api = transport({invokeResults: command => command === 'pick_game_executable' ? path : null});
  assert.equal(await api.pickGameExecutable('x-plane-12', path), path);
  assert.deepEqual(api.calls,[{kind:'invoke',command:'pick_game_executable',args:{gameId:'x-plane-12',currentPath:path}}]);
  const cancelled=transport({invokeResults:()=>null});assert.equal(await cancelled.pickGameExecutable('x-plane-11'),null);
  assert.deepEqual(cancelled.calls[0].args,{gameId:'x-plane-11',currentPath:null});
  const browser=transport({desktop:false});await assert.rejects(browser.pickGameExecutable('x-plane-12'),/desktop app/);assert.equal(browser.calls.length,0);
  const failed=transport({invokeError:new Error('Picker failed')});await assert.rejects(failed.pickGameExecutable('dcs'),/Picker failed/);
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
test('mapping stop and driver inspection cannot wait behind an uploading profile', async () => {
  const upload = deferred();
  const api = transport({ invokeResults: async (_command, request) => {
    if (request.command === 'mapping_import_chunk') await upload.promise;
    return { ok: true, state: { mappings: { running: false } } };
  } });
  const blocked = api.gatewayRequest({ command: 'mapping_import_chunk', uploadId: 'u', offset: 0, bytes: [1] });
  await tick();
  await api.gatewayRequest({ command: 'mapping_stop' });
  await api.gatewayRequest({ command: 'mapping_virtual_inspect' });
  assert.deepEqual(api.calls.map(c => c.args.request.command), ['mapping_import_chunk', 'mapping_stop', 'mapping_virtual_inspect']);
  upload.resolve();
  await blocked;
});

test('companion instances have independent queues and Stop cancels an older queued Start', async () => {
  const inspection = deferred();
  const api = transport({ invokeResults: async (_command, request) => {
    if (request.companion?.instanceId === 'panel-a' && request.companion?.action === 'inspect') await inspection.promise;
    return { ok: true, state: { mappings: { companionRoute: { running: false } } } };
  } });
  const inspect = api.gatewayRequest({ command: 'mapping_companion_route', companion: { action: 'inspect', instanceId: 'panel-a' } });
  const start = api.gatewayRequest({ command: 'mapping_companion_route', companion: { action: 'start', instanceId: 'panel-a', mode: 'recording' } });
  const rejectedStart = assert.rejects(start, /cancelled by Stop/);
  await tick();
  await api.gatewayRequest({ command: 'mapping_companion_route', companion: { action: 'inspect', instanceId: 'panel-b' } });
  await api.gatewayRequest({ command: 'mapping_companion_route', companion: { action: 'stop', instanceId: 'panel-a' } });
  await api.gatewayRequest({ command: 'controller_list' });
  assert.deepEqual(api.calls.map(call => call.args.request.companion?.action ?? call.args.request.command), ['inspect', 'inspect', 'stop', 'controller_list']);
  inspection.resolve(); await inspect; await rejectedStart;
  await api.gatewayRequest({ command: 'mapping_companion_route', companion: { action: 'start', instanceId: 'panel-a', mode: 'recording' } });
  assert.equal(api.calls.at(-1).args.request.companion.action, 'start');
});

test('companion priority Stop does not bypass ordering for saves or cancel another instance Start', async () => {
  const blocked = deferred();
  const api = transport({ invokeResults: async (_command, request) => {
    if (request.command === 'mapping_companion_route' && request.companion.action === 'save') await blocked.promise;
    return { ok: true, state: { mappings: {} } };
  } });
  const save = api.gatewayRequest({ command: 'mapping_companion_route', companion: { action: 'save', instanceId: 'panel-a' } });
  const reconcile = api.gatewayRequest({ command: 'mapping_companion_route', companion: { action: 'reconcile', instanceId: 'panel-a' } });
  await tick();
  await api.gatewayRequest({ command: 'mapping_companion_route', companion: { action: 'stop', instanceId: 'panel-a' } });
  await api.gatewayRequest({ command: 'mapping_companion_route', companion: { action: 'start', instanceId: 'panel-b', mode: 'recording' } });
  assert.deepEqual(api.calls.map(call => call.args.request.companion.action), ['save', 'stop', 'start']);
  blocked.resolve(); await Promise.all([save, reconcile]);
  assert.equal(api.calls.at(-1).args.request.companion.action, 'reconcile');
});

test('a new explicit companion Start waits for its prior priority Stop to finish', async () => {
  const stopping = deferred();
  const api = transport({ invokeResults: async (_command, request) => {
    if (request.companion?.action === 'stop') await stopping.promise;
    return { ok: true, state: { mappings: {} } };
  } });
  const stop = api.gatewayRequest({ command: 'mapping_companion_route', companion: { action: 'stop', instanceId: 'panel' } });
  const start = api.gatewayRequest({ command: 'mapping_companion_route', companion: { action: 'start', instanceId: 'panel', mode: 'recording' } });
  await tick();
  assert.deepEqual(api.calls.map(call => call.args.request.companion.action), ['stop']);
  stopping.resolve(); await Promise.all([stop, start]);
  assert.deepEqual(api.calls.map(call => call.args.request.companion.action), ['stop', 'start']);
});
