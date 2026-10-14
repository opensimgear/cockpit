import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import test from 'node:test';
import vm from 'node:vm';
import ts from 'typescript';

const capabilities = {};
vm.runInNewContext(ts.transpileModule(await readFile(new URL('./gateway-capabilities.ts', import.meta.url), 'utf8'), {
  compilerOptions: { module: ts.ModuleKind.CommonJS, target: ts.ScriptTarget.ES2022 },
}).outputText, { exports: capabilities });
const require = name => { if (name === './gateway-capabilities') return capabilities; throw new Error(`Unexpected import ${name}`); };
const exported = {};
const source = await readFile(new URL('./controller-lifecycle.ts', import.meta.url), 'utf8');
vm.runInNewContext(ts.transpileModule(source, { compilerOptions: { module: ts.ModuleKind.CommonJS, target: ts.ScriptTarget.ES2022 } }).outputText, { exports: exported, require });
const { automaticControllers, canChangeController, controllerWarning, controllerStatus, orderedModules } = exported;
const module = { id: 'pedals', enabled: true, published: true, descriptorIndex: 0, online: true, profileMatch: true };
const state = { backend: 'stm32', protocolVersion: 3, lifecycle: 'automatic', connected: true, modules: [module] };

test('Pico protocol-3 debug gateways support the same automatic controller lifecycle', () => {
  const pico = { ...state, backend: 'pico', canProtocol: 3 };
  assert.equal(automaticControllers(pico), true);
  assert.equal(canChangeController(pico, module, 'disable'), true);
  assert.equal(canChangeController({ ...pico, canProtocol: 2 }, module, 'disable'), false);
});

test('enable, disable and offline removal remain independent of CAN presence', () => {
  assert.equal(canChangeController(state, module, 'disable'), true);
  assert.equal(canChangeController(state, module, 'forget'), false);
  const ghost = { ...module, online: false };
  assert.equal(canChangeController(state, ghost, 'disable'), true);
  assert.equal(canChangeController(state, ghost, 'forget'), true);
  const disabled = { ...module, enabled: false, published: false };
  assert.equal(canChangeController(state, disabled, 'enable'), true);
  assert.equal(canChangeController(state, { ...disabled, online: false }, 'enable'), true);
  assert.equal(canChangeController(state, disabled, 'disable'), false);
});

test('confirmation rechecks a returning device, setup ownership and layout transitions', () => {
  const ghost = { ...module, online: false };
  assert.equal(canChangeController(state, ghost, 'forget'), true);
  assert.equal(canChangeController(state, { ...ghost, online: true }, 'forget'), false);
  for (const changed of [{ connected: false }, { reconnecting: true }, { layoutPending: true }, { claimedModules: ['pedals'] }])
    for (const action of ['enable', 'disable', 'forget', 'update-profile'])
      assert.equal(canChangeController({ ...state, ...changed }, ghost, action), false);
  assert.equal(canChangeController(state, undefined, 'forget'), false);
});

test('warnings apply to removing a published collection, including an offline ghost', () => {
  for (const action of ['disable', 'forget', 'update-profile']) {
    assert.equal(controllerWarning({ ...module, online: false }, action), 'This reconnects the gateway and may require rebinding remaining controllers in your games.');
    assert.equal(controllerWarning({ ...module, enabled: false, published: false }, action), undefined);
  }
  assert.equal(controllerWarning(module, 'enable'), undefined);
});

test('published descriptor order is kept and remembered disabled records follow it', () => {
  const first = { ...module, id: 'first', descriptorIndex: 0 };
  const third = { ...module, id: 'third', descriptorIndex: 1 };
  const disabled = { ...module, id: 'disabled', enabled: false, published: false, descriptorIndex: -1 };
  const otherDisabled = { ...disabled, id: 'other' };
  const input = [disabled, third, otherDisabled, first];
  assert.deepEqual(Array.from(orderedModules({ ...state, modules: input }), item => item.id), ['first', 'third', 'disabled', 'other']);
  assert.deepEqual(input.map(item => item.id), ['disabled', 'third', 'other', 'first']);
});

test('ghost, disabled and profile-change labels reflect the remembered lifecycle', () => {
  assert.equal(controllerStatus(module), 'Enabled');
  assert.equal(controllerStatus({ ...module, online: false }), 'Offline · controller retained');
  assert.equal(controllerStatus({ ...module, enabled: false, published: false }), 'Disabled · remembered');
  assert.equal(controllerStatus({ ...module, enabled: false, published: true }), 'Disable pending · controller retained');
  assert.equal(controllerStatus({ ...module, published: false }), 'Enabling');
  assert.equal(controllerStatus({ ...module, profileMatch: false }), 'Profile changed');
  assert.equal(canChangeController(state, { ...module, profileMatch: false }, 'update-profile'), true);
});

test('failed or pending disable keeps the prior published-controller warning', () => {
  const pending = { ...module, enabled: false, published: true };
  for (const transition of [{ layoutPending: true }, { layoutPending: false, layoutError: 'flash_commit' }]) {
    const changed = { ...state, ...transition, modules: [pending] };
    assert.equal(controllerStatus(changed.modules[0]), 'Disable pending · controller retained');
    assert.match(controllerWarning(changed.modules[0], 'forget'), /may require rebinding/);
  }
});

test('older STM32 and Pico adapters do not advertise automatic lifecycle controls', () => {
  for (const legacy of [{ ...state, protocolVersion: 2 }, { ...state, lifecycle: undefined }, { ...state, backend: 'pico' }]) {
    assert.equal(automaticControllers(legacy), false);
    assert.equal(canChangeController(legacy, module, 'disable'), false);
    assert.equal(orderedModules(legacy), legacy.modules);
  }
});


test('output-only inventory supports offline forgetting without empty controller controls', () => {
  const output={...module,enabled:false,published:false,outputOnly:true};
  for(const action of ['enable','disable','update-profile']) assert.equal(canChangeController(state,output,action),false);
  assert.equal(controllerStatus(output),'Output device · no controller');
  assert.equal(canChangeController(state,{...output,online:false},'forget'),true);
});
