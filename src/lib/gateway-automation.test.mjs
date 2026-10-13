import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import test from 'node:test';
import vm from 'node:vm';
import ts from 'typescript';
const source = await readFile(new URL('./gateway-automation.ts', import.meta.url), 'utf8');
const exported = {};
vm.runInNewContext(ts.transpileModule(source, { compilerOptions: { module: ts.ModuleKind.CommonJS, target: ts.ScriptTarget.ES2022 } }).outputText, { exports: exported });
const { gatewayCandidates, automaticGatewayAction: action, nextConfigurationRead } = exported;
const port = { path: '/dev/cu.usbmodemOSG', vendorId: 'cafe', productId: '4010', serialNumber: 'OSG-PICO-0123456789ABCDEF' };
const state = { connected: false, running: false, modules: [{ online: true, registered: true }], ports: [port] };
const options = { paused: false, setupActive: false, failedPorts: new Set() };
test('discovery requires VID, PID, and serial prefix; names alone do not match', () => {
  assert.equal(gatewayCandidates([port]).length, 1);
  for (const changed of [{vendorId:'1209'}, {productId:'0000'}, {serialNumber:'unrelated'}])
    assert.equal(gatewayCandidates([{...port,...changed}]).length, 0);
  assert.equal(gatewayCandidates([{path:'unrelated',manufacturer:'OpenSimGear'}]).length, 0);
});
test('only one candidate is connected, without probing unrelated serial devices', () => {
  assert.equal(action(state,options).port,port.path);
  assert.equal(action({...state,ports:[port,{...port,path:'other'}]},options).command,'connect');
  assert.equal(action({...state,ports:[]},options).command,undefined);
});
test('start requires a connected gateway with registered online modules', () => {
  assert.equal(action({...state,connected:true},options).command,'start');
  for(const modules of [[],[{online:false,registered:true}],[{online:true,registered:false}]])
    assert.equal(action({...state,connected:true,modules},options).command,undefined);
  assert.equal(action({...state,connected:true,running:true},options).command,undefined);
});
test('manual pause, setup ownership, pending layout, and reconnect block automatic start', () => {
  const connected = {...state,connected:true};
  assert.equal(action(connected,{...options,paused:true}).command,undefined);
  assert.equal(action(connected,{...options,setupActive:true}).command,undefined);
  for(const changed of [{claimedModules:['module']},{needsReconnect:true},{reconnecting:true}])
    assert.equal(action({...connected,...changed},options).command,undefined);
});
test('a failed attachment is not automatically probed repeatedly', () => {
  assert.equal(action(state,{...options,failedPorts:new Set([port.path])}).command,undefined);
  assert.equal(action(state,options).command,'connect');
});
test('macOS call-in and call-out aliases count as one gateway regardless of enumeration order', () => {
  const tty = { ...port, path: '/dev/tty.usbmodemOSG' };
  for (const ports of [[tty, port], [port, tty]]) {
    assert.equal(gatewayCandidates(ports).length, 1);
    assert.equal(action({ ...state, ports }, options).port, port.path);
  }
  assert.equal(action({ ...state, ports: [tty] }, options).port, tty.path);
});
test('multiple gateways connect deterministically and failed candidates do not block others', () => {
  const other = { ...port, path: '/dev/cu.usbmodemZZZ', serialNumber: 'OSG-PICO-OTHER123' };
  assert.equal(action({ ...state, ports: [other, port] }, options).port, port.path);
  assert.equal(action({ ...state, ports: [other, port] }, { ...options, failedPorts: new Set([port.path]) }).port, other.path);
});

const physicalPort = { path: '/dev/cu.usbmodemCAN', manufacturer: 'OpenSimGear', friendlyName: 'CAN Gateway Rev A', serialNumber: '00112233445566778899aabb', vendorId: '1209', productId: '1234' };
test('CAN discovery requires gateway product metadata and a stable UID without an invented VID', () => {
  assert.equal(gatewayCandidates([physicalPort])[0].backend, 'stm32');
  for (const changed of [{ manufacturer: 'Other' }, { manufacturer: 'OpenSimGear clone' }, { friendlyName: 'Generic Device' }, { serialNumber: 'OSG-PICO-12345678' }, { serialNumber: '0'.repeat(24) }, { serialNumber: 'f'.repeat(24) }])
    assert.equal(gatewayCandidates([{ ...physicalPort, ...changed }]).length, 0);
  assert.equal(action({ ...state, ports: [port, physicalPort] }, options).backend, 'stm32');
});
test('physical gateways never receive simulator start or automatic USB apply commands', () => {
  for (const changed of [{ running: false }, { needsReconnect: true }, { modules: [] }]) {
    const result = action({ ...state, backend: 'stm32', connected: true, ...changed }, options);
    assert.equal(result.command, undefined);
  }
});

test('automatic gateways observe firmware-owned controller updates without manual Apply', () => {
  for (const layoutPending of [false, true]) {
    const result = action({ ...state, backend: 'stm32', connected: true, lifecycle: 'automatic', layoutPending }, options);
    assert.equal(result.command, undefined);
    assert.doesNotMatch(result.reason, /Apply|register/);
  }
});

test('configuration discovery reads one new device at a time and resumes after cache invalidation', () => {
  const first = { id: 'first', online: true, canConfigure: true };
  const second = { ...first, id: 'second' };
  const current = { ...state, connected: true, backend: 'stm32', modules: [first, second] };
  assert.equal(nextConfigurationRead(current, false), 'first');
  current.moduleConfigs = { first: { config: { name: 'Pedals' } } };
  assert.equal(nextConfigurationRead(current, false), 'second');
  current.moduleConfigs.second = { config: { name: 'Panel' } };
  assert.equal(nextConfigurationRead(current, false), undefined);
  delete current.moduleConfigs.first;
  assert.equal(nextConfigurationRead(current, false), 'first');
});

test('configuration discovery skips unavailable routes and stays idle during setup or reconnect', () => {
  const candidate = { id: 'readable', online: true, canConfigure: true };
  const current = { ...state, connected: true, backend: 'stm32', modules: [
    { ...candidate, id: 'offline', online: false },
    { ...candidate, id: 'legacy', canConfigure: false },
    { ...candidate, id: 'direct-usb', directUsb: true }, candidate,
  ] };
  assert.equal(nextConfigurationRead(current, false), 'readable');
  assert.equal(nextConfigurationRead(current, true), undefined);
  for (const change of [{ connected: false }, { reconnecting: true }, { layoutPending: true }, { backend: 'pico' }])
    assert.equal(nextConfigurationRead({ ...current, ...change }, false), undefined);
});
