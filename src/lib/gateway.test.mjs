import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import test from 'node:test';
import vm from 'node:vm';
import ts from 'typescript';
import {loadModules} from './test-module-loader.mjs';
const gameMetadata=(await loadModules(['./games'])).get('./games');

const source = await readFile(new URL('./gateway.ts', import.meta.url), 'utf8');
const compiled = ts.transpileModule(source, { compilerOptions: { module: ts.ModuleKind.CommonJS, target: ts.ScriptTarget.ES2022 } }).outputText;
const first = 'stm32:00112233445566778899aabb';
const second = 'stm32:00112233445566778899aacc';
const state = { gatewayId: first, backend: 'stm32', running: false, needsReconnect: false, connected: true, modules: [] };
const plain = value => JSON.parse(JSON.stringify(value));
const tick = () => new Promise(resolve => setImmediate(resolve));
const deferred = () => { let resolve; const promise = new Promise(done => { resolve = done; }); return { promise, resolve }; };

test('passive telemetry bypasses game setup and keeps profile Stop responsive',async()=>{
 const pending=deferred(),api=transport({invokeResults:async(_command,request)=>{if(request.command==='game_scan')await pending.promise;return{ok:true,state};}});
 const scan=api.gatewayRequest({command:'game_scan'});await tick();
 await api.gatewayRequest({command:'game_telemetry_inspect',gameId:'assetto-corsa',offset:0,limit:64});
 await api.gatewayRequest({command:'profile_stop'});
 assert.deepEqual(api.calls.map(call=>call.args.request.command),['game_scan','game_telemetry_inspect','profile_stop']);
 pending.resolve();await scan;
});

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
      if (name === './games') return gameMetadata;
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

test('automatic control admission metadata reaches native and browser transports while explicit requests remain foreground', async () => {
  const requests = [{ command: 'connect', gatewayId: first, backend: 'stm32', port: 'gateway', background: true }, { command: 'start', gatewayId: first, background: true }, { command: 'connect', gatewayId: first, backend: 'stm32', port: 'gateway' }];
  for (const desktop of [true, false]) {
    const api = transport({ desktop });
    for (const request of requests) await api.gatewayRequest(request);
    assert.deepEqual(api.calls.map(call => desktop ? call.args.request : JSON.parse(call.options.body)), requests.map(request => ({ ...request, clientId: 'test-client-uuid' })));
  }
});

test('activation progress and Cancel bypass blocked profile library reads and use independent native queues',async()=>{
 const blocked=deferred(),progress=deferred();const api=transport({invokeResults:async(_command,request)=>request.command==='profile_list'?blocked.promise:request.command==='profile_activation_current'?progress.promise:{ok:true,state}});
 const library=api.gatewayRequest({command:'profile_list'});await tick();const current=api.gatewayRequest({command:'profile_activation_current'});await tick();
 const cancelled=await api.gatewayRequest({command:'profile_activation_cancel',activationId:'exact-operation'});
 assert.equal(cancelled,state);assert.deepEqual(api.calls.map(call=>call.args.request.command),['profile_list','profile_activation_current','profile_activation_cancel']);
 progress.resolve({ok:true,state});blocked.resolve({ok:true,state});await Promise.all([library,current]);
});

test('cleanup dispatch opens progress with the exact native ID without waiting for rendering and reports completion/failure',async()=>{
 const blocked=deferred(),events=[];
 const api=transport({invokeResults:async()=>blocked.promise});
 const unsubscribe=api.subscribeProfileCleanup(event=>{events.push(plain(event));return new Promise(()=>{});});
 const stopping=api.gatewayRequest({command:'profile_stop'});await tick();
 assert.deepEqual(events,[{type:'start',activationId:'test-client-uuid',phase:'deactivating'}]);
 assert.equal(api.calls[0].args.request.activationId,'test-client-uuid');
 blocked.resolve({ok:true,state});await stopping;
 assert.deepEqual(events[1],{type:'finish',activationId:'test-client-uuid',phase:'deactivating'});unsubscribe();
 const failure=transport({result:{ok:false,error:'Owned output cleanup is uncertain.',state}}),failed=[];
 failure.subscribeProfileCleanup(event=>failed.push(plain(event)));
 await assert.rejects(failure.gatewayRequest({command:'profile_recover',activationId:'exact-recovery'}),/uncertain/);
 assert.equal(failed[0].activationId,'exact-recovery');assert.equal(failed[1].error,'Owned output cleanup is uncertain.');
 const observerFailure=transport();observerFailure.subscribeProfileCleanup(()=>{throw new Error('UI unavailable');});
 assert.equal(await observerFailure.gatewayRequest({command:'profile_stop'}),state);
});

test('retired standalone authoring, synthetic and companion feedback routes never reach desktop or HTTP transport',async()=>{
 for(const desktop of[true,false]){const api=transport({desktop});for(const command of['mapping_status','mapping_start','mapping_stop','mapping_import','mapping_virtual_inspect','mapping_companion_route','integration_get_state','integration_start','integration_inject','dcs_status','dcs_catalogue','dcs_start','companion_feedback_routes','companion_feedback','companion_game_capture','controller_visibility_apply','controller_visibility_launch'])await assert.rejects(api.gatewayRequest({command}),/retired/);assert.equal(api.calls.length,0);}
});

test('local visibility preferences, inspection, enablement and exact restoration still use the ordinary native boundary on both transports',async()=>{
 const requests=[{command:'controller_visibility_status',sourceId:'physical'},{command:'controller_visibility_save',sourceId:'physical',intent:{mode:'hide-from-process',executable:'C:\\Games\\Game.exe'}},{command:'controller_visibility_enable',sourceId:'physical',enabled:true},{command:'controller_visibility_inspect',sourceId:'physical'},{command:'controller_visibility_processes',sourceId:'physical',offset:32},{command:'controller_visibility_restore',sourceId:'physical',jobId:'native-job',sessionId:'native-session'}];
 for(const desktop of[true,false]){const api=transport({desktop});for(const request of requests)await api.gatewayRequest(request);assert.deepEqual(api.calls.map(call=>desktop?call.args.request:JSON.parse(call.options.body)),requests.map(request=>({...request,clientId:'test-client-uuid'})));}
});

test('retained cleanup forwards exact native references while status, cancellation and profile Stop bypass slow cleanup; cancelled unsent recovery never restarts a job',async()=>{
 const waiting=deferred(),cancelled=deferred(),reference={operation:'native-operation',recordHash:'native-record-hash',target:2,refreshUncertain:true};
 const api=transport({invokeResults:async(_name,request)=>{if(request.command==='profile_retired_recovery_provision_recover')await waiting.promise;if(request.command==='profile_retired_recovery_provision_cancel')await cancelled.promise;return{ok:true,state:{retiredRecovery:{required:true}}};}});
 const recovering=api.gatewayRequest({command:'profile_retired_recovery_provision_recover',reference}),old=api.gatewayRequest({command:'profile_retired_recovery_provision_recover',reference}),rejected=assert.rejects(old,/cancelled before dispatch/);await tick();const cancel=api.gatewayRequest({command:'profile_retired_recovery_provision_cancel',jobId:'exact-native-job'});await api.gatewayRequest({command:'profile_retired_recovery_status'});await api.gatewayRequest({command:'profile_stop'});const fresh=api.gatewayRequest({command:'profile_retired_recovery_provision_recover',reference});
 assert.deepEqual(api.calls.map(v=>v.args.request.command),['profile_retired_recovery_provision_recover','profile_retired_recovery_provision_cancel','profile_retired_recovery_status','profile_stop']);assert.deepEqual(api.calls[0].args.request.reference,reference);waiting.resolve();await Promise.all([recovering,rejected]);await tick();assert.equal(api.calls.filter(v=>v.args.request.command.endsWith('_provision_recover')).length,1);cancelled.resolve();await Promise.all([cancel,fresh]);assert.equal(api.calls.filter(v=>v.args.request.command.endsWith('_provision_recover')).length,2);
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
test('applied topology metadata is an independent read lane and cannot delay samples, edits or emergency controls',async()=>{
  const pending=deferred(),api=transport({invokeResults:async(_command,request)=>{if(request.command==='profile_runtime_topology')await pending.promise;return{ok:true,state};}});
  const owner={session:'owner-session',generation:1,profileId:'profile',profileRevision:3,gameId:null};
  const reading=api.gatewayRequest({command:'profile_runtime_topology',owner,section:'edges',offset:0,limit:64});await tick();
  await api.gatewayRequest({command:'profile_runtime_inspect',owner,nodeId:'source',offset:0,limit:32});await api.gatewayRequest({command:'profile_stop'});await api.gatewayRequest({command:'profile_save',document:{id:'profile',revision:3},expectedRevision:3});
  assert.deepEqual(api.calls.map(call=>call.args.request.command),['profile_runtime_topology','profile_runtime_inspect','profile_stop','profile_save']);assert.deepEqual(api.calls[0].args.request,{command:'profile_runtime_topology',owner,section:'edges',offset:0,limit:64,clientId:'test-client-uuid'});pending.resolve();await reading;
});
test('learning cached status and priority cancel bypass a blocked start, and controls cancel unsent starts',async()=>{
  for(const command of ['profile_input_learn_cancel','profile_apply','profile_game_launch','profile_selection_mode','profile_game_rule_save','profile_game_rule_remove','profile_retired_recovery_recover','action_invoke']){
    const wait=deferred(),api=transport({invokeResults:async(_command,request)=>{if(request.command==='profile_input_learn_start')await wait.promise;return{ok:true,state};}});
    const fields={resourceId:'physical',generation:'1',capabilityRevision:'sha256:'+'1'.repeat(64)},active=api.gatewayRequest({command:'profile_input_learn_start',...fields});
    const old=api.gatewayRequest({command:'profile_input_learn_start',...fields}),rejected=assert.rejects(old,/learning Start was cancelled/);await tick();
    await api.gatewayRequest({command:'profile_input_learn_status',learnId:'current'});await api.gatewayRequest({command,...(command==='profile_input_learn_cancel'?{learnId:'current'}:{})});
    assert.equal(api.calls.filter(call=>call.args.request.command==='profile_input_learn_start').length,1);wait.resolve();await Promise.all([active,rejected]);
  }
});
test('automatic status is an independent read while rule and saved-profile writes retain storage ordering',async()=>{
  const waiting=deferred(),api=transport({invokeResults:async(_command,request)=>{if(request.command==='profile_game_rule_save')await waiting.promise;return{ok:true,state:{profiles:{automatic:{mode:'manual'}}}};}});
  const rule={command:'profile_game_rule_save',gameId:'dcs',profileId:'11111111-1111-4111-8111-111111111111',priority:32767,enabled:true};const saving=api.gatewayRequest(rule),mode=api.gatewayRequest({command:'profile_selection_mode',mode:'automatic'});await tick();await api.gatewayRequest({command:'profile_automatic_status'});await api.gatewayRequest({command:'profile_status'});assert.deepEqual(api.calls.map(call=>call.args.request.command),['profile_game_rule_save','profile_automatic_status','profile_status']);assert.deepEqual(api.calls[0].args.request,{...rule,clientId:'test-client-uuid'});waiting.resolve();await Promise.all([saving,mode]);assert.equal(api.calls.at(-1).args.request.command,'profile_selection_mode');
});

test('profile runtime controls and status bypass slow Apply while older unsent Apply is cancelled',async()=>{
  const active=deferred();
  const api=transport({invokeResults:async(_command,request)=>{
    if(request.command==='profile_apply')await active.promise;
    return{ok:true,state:{profiles:{}}};
  }});
  const firstApply=api.gatewayRequest({command:'profile_apply',id:'first',expectedRevision:1});
  const queuedApply=api.gatewayRequest({command:'profile_apply',id:'old-queued',expectedRevision:1});
  const rejected=assert.rejects(queuedApply,/cancelled by deactivation or recovery/);
  await tick();
  await api.gatewayRequest({command:'profile_status'});
  await api.gatewayRequest({command:'profile_stop'});
  await api.gatewayRequest({command:'profile_editor_save',document:{id:'draft'},expectedRevision:1,assignments:[]});
  assert.deepEqual(api.calls.map(call=>call.args.request.command),['profile_apply','profile_status','profile_stop','profile_editor_save']);
  active.resolve();await Promise.all([firstApply,rejected]);
  assert.equal(api.calls.filter(call=>call.args.request.command==='profile_apply').length,1);
});

test('new explicit Apply waits for prior Save and priority control completion without replaying cancelled requests',async()=>{
  for(const command of ['profile_stop','profile_recover','controller_visibility_restore']){
    const saved=deferred(),stopping=deferred();
    const api=transport({invokeResults:async(_name,request)=>{
      if(request.command==='profile_editor_save')await saved.promise;
      if(request.command===command)await stopping.promise;
      return{ok:true,state};
    }});
    const save=api.gatewayRequest({command:'profile_editor_save',document:{id:'draft'},expectedRevision:1,assignments:[]});
    const old=api.gatewayRequest({command:'profile_apply',id:'cancelled',expectedRevision:2});
    const rejected=assert.rejects(old,/cancelled by deactivation or recovery/);
    const stop=api.gatewayRequest({command});
    const fresh=api.gatewayRequest({command:'profile_apply',id:'fresh',expectedRevision:2});
    await api.gatewayRequest({command:'profile_status'});
    assert.deepEqual(api.calls.map(call=>call.args.request.command),['profile_editor_save',command,'profile_status']);
    saved.resolve();await Promise.all([save,rejected]);await tick();
    assert.equal(api.calls.some(call=>call.args.request.id==='fresh'),false);
    stopping.resolve();await Promise.all([stop,fresh]);
    assert.deepEqual(api.calls.filter(call=>call.args.request.command==='profile_apply').map(call=>call.args.request.id),['fresh']);
  }
});

test('Stop bypasses an editor Save while queued saves still persist without dispatching cancelled Apply',async()=>{
  const pending=deferred(),api=transport({invokeResults:async(_name,request)=>{
    if(request.command==='profile_editor_save'&&request.document.id==='active')await pending.promise;
    return{ok:true,state};
  }});
  const active=api.gatewayRequest({command:'profile_editor_save',document:{id:'active'},expectedRevision:1,assignments:[]});
  const queued=api.gatewayRequest({command:'profile_editor_save',document:{id:'inactive'},expectedRevision:1,assignments:[]});
  const applying=api.gatewayRequest({command:'profile_apply',id:'cancelled',expectedRevision:2}),cancelled=assert.rejects(applying,/cancelled by deactivation/);
  await tick();await api.gatewayRequest({command:'profile_stop'});await api.gatewayRequest({command:'profile_status'});
  assert.deepEqual(api.calls.map(call=>call.args.request.command),['profile_editor_save','profile_stop','profile_status']);
  pending.resolve();await Promise.all([active,queued,cancelled]);
  assert.deepEqual(api.calls.filter(call=>call.args.request.command==='profile_editor_save').map(call=>call.args.request.document.id),['active','inactive']);
});

test('capturing a draft application leaves pending Apply and learning requests eligible for native admission',async()=>{
  const pending=deferred(),api=transport({invokeResults:async(_name,request)=>{
    if(request.command==='profile_apply'||request.command==='profile_input_learn_start')await pending.promise;
    return{ok:true,state};
  }});
  const active=api.gatewayRequest({command:'profile_apply',id:'active',expectedRevision:1});
  const queued=api.gatewayRequest({command:'profile_apply',id:'next',expectedRevision:1});
  const learning=api.gatewayRequest({command:'profile_input_learn_start',resourceId:'input'});
  const nextLearning=api.gatewayRequest({command:'profile_input_learn_start',resourceId:'input'});
  await tick();await api.gatewayRequest({command:'profile_desktop_capture_application'});
  pending.resolve();await Promise.all([active,queued,learning,nextLearning]);
  assert.equal(api.calls.filter(call=>call.args.request.command==='profile_apply').length,2);
  assert.equal(api.calls.filter(call=>call.args.request.command==='profile_input_learn_start').length,2);
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

test('native integration folder selection forwards the location kind without changing provider setup', async () => {
  const path = String.raw`C:\Users\Pilot\Saved Games\DCS`;
  const api = transport({invokeResults: command => command === 'pick_game_integration_folder' ? path : null});
  assert.equal(await api.pickGameIntegrationFolder('saved-games', path), path);
  assert.deepEqual(api.calls,[{kind:'invoke',command:'pick_game_integration_folder',args:{kind:'saved-games',currentPath:path}}]);
  const cancelled=transport({invokeResults:()=>null});assert.equal(await cancelled.pickGameIntegrationFolder('catalogue'),null);
  assert.deepEqual(cancelled.calls[0].args,{kind:'catalogue',currentPath:null});
  const browser=transport({desktop:false});await assert.rejects(browser.pickGameIntegrationFolder('saved-games'),/desktop app/);assert.equal(browser.calls.length,0);
  const failed=transport({invokeError:new Error('Picker failed')});await assert.rejects(failed.pickGameIntegrationFolder('catalogue'),/Picker failed/);
});

test('generic executable and tile image pickers forward only their local selection context',async()=>{
  const path=String.raw`C:\Games\game.exe`,image=String.raw`C:\Pictures\tile.png`,id='custom-11111111-1111-4111-8111-111111111111';
  const api=transport({invokeResults:command=>command==='pick_game_image'?image:path});
  assert.equal(await api.pickGameExecutable('',path),path);assert.equal(await api.pickGameExecutable(id),path);assert.equal(await api.pickGameImage(image),image);
  assert.deepEqual(api.calls.map(call=>[call.command,call.args]),[['pick_game_executable',{gameId:'',currentPath:path}],['pick_game_executable',{gameId:id,currentPath:null}],['pick_game_image',{currentPath:image}]]);
  const cancelled=transport({invokeResults:()=>null});assert.equal(await cancelled.pickGameImage(),null);
  const browser=transport({desktop:false});await assert.rejects(browser.pickGameImage(),/image path/);assert.equal(browser.calls.length,0);
});

test('native game catalogue replies update selectable custom games without enabling unsupported providers',async()=>{
  const id='custom-11111111-1111-4111-8111-111111111111',custom={id,name:'Custom game',provider:null,installed:false,launchSupported:false};
  const api=transport({result:{ok:true,state:{games:{catalogue:[...gameMetadata.supportedGames,custom]}}}});
  await api.gatewayRequest({command:'game_get_state'});assert.equal(gameMetadata.isRegisteredGameId(id),true);assert.equal(gameMetadata.gameHasIntegration(custom),false);
  gameMetadata.updateGameCatalogue(gameMetadata.supportedGames);
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

test('firmware lookup and download stay ordered while companion status and USB reads remain responsive', async () => {
  const lookup=deferred(),download=deferred();
  const api=transport({invokeResults:async(_command,request)=>{
    if(request.command==='companion_firmware_check')await lookup.promise;
    if(request.command==='companion_usb_flash')await download.promise;
    return {ok:true,state:{companions:{firmwareChecked:true}}};
  }});
  const checking=api.gatewayRequest({command:'companion_firmware_check'});
  const flashing=api.gatewayRequest({command:'companion_usb_flash',companion:{profile:'sunton-3248s035r',port:'COM5'}});
  await tick();
  await api.gatewayRequest({command:'companion_status'});
  await api.gatewayRequest({command:'companion_usb_ports'});
  assert.deepEqual(api.calls.map(call=>call.args.request.command),['companion_firmware_check','companion_status','companion_usb_ports']);
  lookup.resolve();await checking;await tick();
  await api.gatewayRequest({command:'companion_usb_status'});
  assert.deepEqual(api.calls.map(call=>call.args.request.command),['companion_firmware_check','companion_status','companion_usb_ports','companion_usb_flash','companion_usb_status']);
  download.resolve();await flashing;
});
test('passive companion panel metadata bypasses blocked profile saves without delaying emergency controls', async () => {
  const saving=deferred(),metadata=deferred();
  const api=transport({invokeResults:async(_command,request)=>{
    if(request.command==='profile_editor_save')await saving.promise;
    if(request.command==='profile_companion_catalogue')await metadata.promise;
    return{ok:true,state:{profileCompanionAuthoring:{schemaVersion:1,panels:[]}}};
  }});
  const save=api.gatewayRequest({command:'profile_editor_save'});await tick();
  const catalogue=api.gatewayRequest({command:'profile_companion_catalogue'});await tick();
  await api.gatewayRequest({command:'profile_stop'});
  assert.deepEqual(api.calls.map(call=>call.args.request.command),['profile_editor_save','profile_companion_catalogue','profile_stop']);
  metadata.resolve();await catalogue;
  await api.gatewayRequest({command:'profile_companion_inspect',resourceId:'native-instance',publicationHash:'sha256:captured',offset:0,limit:32});
  assert.equal(api.calls.at(-1).args.request.resourceId,'native-instance');
  assert.equal(api.calls.at(-1).args.request.publicationHash,'sha256:captured');
  saving.resolve();await save;
});

test('native profile launch waits for saved edits and shares Apply ordering while Stop and status stay independent',async()=>{
  const saved=deferred(),launch=deferred(),api=transport({invokeResults:async(_command,request)=>{if(request.command==='profile_save')await saved.promise;if(request.command==='profile_game_launch')await launch.promise;return{ok:true,state};}});
  const saving=api.gatewayRequest({command:'profile_save',document:{id:'profile',revision:1},expectedRevision:1});
  const starting=api.gatewayRequest({command:'profile_game_launch',gameId:'dcs'});
  const queued=api.gatewayRequest({command:'profile_game_launch',gameId:'x-plane-12'}),cancelled=assert.rejects(queued,/launch was cancelled/);
  await tick();assert.deepEqual(api.calls.map(call=>call.args.request.command),['profile_save']);saved.resolve();await saving;await tick();
  await api.gatewayRequest({command:'profile_status'});await api.gatewayRequest({command:'profile_stop'});
  assert.deepEqual(api.calls.map(call=>call.args.request.command),['profile_save','profile_game_launch','profile_status','profile_stop']);
  assert.deepEqual(api.calls[1].args.request,{command:'profile_game_launch',gameId:'dcs',clientId:'test-client-uuid'});
  launch.resolve();await Promise.all([starting,cancelled]);
});
