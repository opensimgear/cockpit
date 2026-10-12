import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import test from 'node:test';
import vm from 'node:vm';
import ts from 'typescript';
const source = await readFile(new URL('./gateway-automation.ts', import.meta.url), 'utf8');
const exported = {};
vm.runInNewContext(ts.transpileModule(source, { compilerOptions: { module: ts.ModuleKind.CommonJS, target: ts.ScriptTarget.ES2022 } }).outputText, { exports: exported });
const { gatewayCandidates, automaticGatewayAction: action } = exported;
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
