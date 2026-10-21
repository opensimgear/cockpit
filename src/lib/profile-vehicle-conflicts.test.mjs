import assert from 'node:assert/strict';
import test from 'node:test';
import {readFile} from 'node:fs/promises';
import vm from 'node:vm';
import ts from 'typescript';
import {compile} from 'svelte/compiler';
import {render} from 'svelte/server';
import * as server from 'svelte/internal/server';
import * as svelte from 'svelte';
import {loadModules} from './test-module-loader.mjs';
const modules=await loadModules(['./profile-vehicle-conflicts','./profile-editor']);
const {ProfileVehicleConflictController,profileVehicleConflict}=modules.get('./profile-vehicle-conflicts');
const {blankProfile}=modules.get('./profile-editor'),plain=value=>JSON.parse(JSON.stringify(value));
const diagnostic=(code,message=code)=>({code,message,severity:'error',location:{nodeId:null,resourceId:'base',portId:'button',edgeIds:[]}});
const summary=(diagnostics=[])=>({diagnostics,sectionTotals:{diagnostics:diagnostics.length}});
function fixture(){
 const document=blankProfile('11111111-1111-4111-8111-111111111111','Vehicles');document.eligibility.gameIds=['dcs'];
 document.resources=['base','hornet','viper'].map(id=>({id,name:id,kind:'device',required:false,ports:[]}));
 document.vehicleTabs=[{id:'a',gameId:'dcs',vehicleId:'FA-18C_hornet',vehicleName:'F/A-18C',nodeIds:['hornet']},{id:'b',gameId:'dcs',vehicleId:'F-16C_50',vehicleName:'F-16C',nodeIds:['viper']}];
 return{document,assignments:document.resources.map(resource=>({resourceId:resource.id,sourceId:'same-native-controller',ports:[]}))};
}
const drain=async()=>{for(let i=0;i<12;i++)await Promise.resolve();};
function harness(){
 const scheduled=new Set(),calls=[],pages=[],compiler={async compile(document,assignments){calls.push({document:plain(document),assignments:plain(assignments)});return summary();},async inspect(...args){pages.push(args);return{items:[]};}};
 const controller=new ProfileVehicleConflictController(compiler,(callback,delay)=>{assert.equal(delay,350);scheduled.add(callback);return()=>scheduled.delete(callback);});
 return{controller,compiler,calls,pages,scheduled,async flush(){for(const callback of [...scheduled]){scheduled.delete(callback);callback();}await drain();}};
}
const deferred=()=>{let resolve,reject;const promise=new Promise((yes,no)=>{resolve=yes;reject=no;});return{promise,resolve,reject};};

test('debounced conflict checking captures Base with one vehicle and exact pins without changing the draft',async()=>{
 const h=harness(),value=fixture(),before=plain(value);assert.equal(h.controller.state.status,'idle');assert.equal(h.calls.length,0);
 h.controller.update(value.document,value.assignments,'a',1);assert.equal(h.controller.state.status,'pending');assert.equal(h.calls.length,0);
 h.controller.update(value.document,value.assignments,'b',1);assert.equal(h.scheduled.size,1);await h.flush();
 assert.deepEqual(h.calls[0].document.resources.map(resource=>resource.id),['base','viper']);assert.equal(h.calls[0].document.vehicleTabs,undefined);assert.deepEqual(h.calls[0].assignments.map(pin=>pin.resourceId),['base','viper']);assert.deepEqual(plain(value),before);assert.equal(h.controller.state.status,'ready');
 value.document.editor.viewport.x=300;h.controller.update(value.document,value.assignments,'b',1);assert.equal(h.scheduled.size,0,'Layout-only edits do not repeat a conflict capture.');
 h.controller.update(value.document,value.assignments,'b',2);await h.flush();assert.equal(h.calls.length,2,'Inventory changes refresh physical conflicts.');
 h.controller.update(value.document,value.assignments,'',2);await h.flush();assert.deepEqual(h.calls[2].document.resources.map(resource=>resource.id),['base']);
});

test('conflicts after the first page are shown while offline, backend and readiness noise is omitted',async()=>{
 const h=harness(),value=fixture(),unavailable=diagnostic('resourceUnavailable','Offline controller'),conflict=diagnostic('conflictingEndpointWriters','Two writers'),structural=diagnostic('internalCycle','Cycle');
 h.compiler.compile=async()=>({...summary([unavailable]),sectionTotals:{diagnostics:4}});
 h.compiler.inspect=async(_preview,section,offset,limit)=>{h.pages.push({section,offset,limit});return{items:offset===1?[diagnostic('virtualBackendNotReady'),conflict]:[structural]};};
 h.controller.update(value.document,value.assignments,'a');await h.flush();assert.equal(h.controller.state.status,'ready');assert.deepEqual(plain(h.controller.state.diagnostics.map(item=>item.code)),['conflictingEndpointWriters','internalCycle']);assert.deepEqual(h.pages,[{section:'diagnostics',offset:1,limit:64},{section:'diagnostics',offset:3,limit:64}]);assert.ok(Object.isFrozen(h.controller.state.diagnostics[0].location.edgeIds));
 for(const code of ['duplicatePhysicalSource','duplicatePhysicalInputRoutes','virtualDirectSimulatorConflict','virtualOutputFeedback','ambiguousWriters'])assert.equal(profileVehicleConflict(diagnostic(code)),true);
 assert.equal(profileVehicleConflict({...conflict,severity:'warning'}),false);
});

test('tab changes, hidden editor and disposal suppress stale successful or failed native results',async()=>{
 const h=harness(),value=fixture(),pending=[];h.compiler.compile=async()=>{const wait=deferred();pending.push(wait);return wait.promise;};
 h.controller.update(value.document,value.assignments,'a');await h.flush();h.controller.update(value.document,value.assignments,'b');await h.flush();
 pending[1].resolve(summary([diagnostic('duplicatePhysicalSource','Current vehicle')]));await drain();pending[0].resolve(summary([diagnostic('conflictingEndpointWriters','Old vehicle')]));await drain();assert.equal(h.controller.state.diagnostics[0].message,'Current vehicle');
 h.controller.update(value.document,value.assignments,'a');await h.flush();h.controller.update(value.document,value.assignments,'a',0,false);pending[2].reject(new Error('Late unavailable error'));await drain();assert.equal(h.controller.state.status,'idle');assert.equal(h.controller.state.error,'');assert.equal(h.controller.state.diagnostics.length,0);
 h.controller.update(value.document,value.assignments,'a');h.controller.dispose();assert.equal(h.scheduled.size,0);h.controller.update(value.document,value.assignments,'b');assert.equal(h.scheduled.size,0);
});

test('compiler and paging errors produce a clear unavailable result and can refresh after inventory changes',async()=>{
 const h=harness(),value=fixture();h.compiler.compile=async()=>{throw new Error('Native service disconnected');};h.controller.update(value.document,value.assignments,'a');await h.flush();assert.equal(h.controller.state.status,'error');assert.match(h.controller.state.error,/Native service disconnected/);
 h.compiler.compile=async()=>({...summary(),sectionTotals:{diagnostics:1}});h.controller.update(value.document,value.assignments,'a',1);await h.flush();assert.match(h.controller.state.error,/incomplete/);
 h.compiler.compile=async()=>summary([diagnostic('invalidCapture','Native inventory capture is invalid')]);h.controller.update(value.document,value.assignments,'a',2);await h.flush();assert.match(h.controller.state.error,/inventory capture is invalid/);
 h.compiler.compile=async()=>summary();h.controller.update(value.document,value.assignments,'a',3);await h.flush();assert.equal(h.controller.state.status,'ready');
 h.controller.update(value.document,value.assignments,'missing');assert.match(h.controller.state.error,/existing vehicle/);
});

test('vehicle conflict panel renders scoped results, clean checks and failures without native calls during render',async()=>{
 const source=await readFile(new URL('./ProfileVehicleConflicts.svelte',import.meta.url),'utf8');
 const {js}=compile(source,{filename:'ProfileVehicleConflicts.svelte',generate:'server'}),exports={},value=fixture();
 let current={status:'ready',diagnostics:[diagnostic('duplicatePhysicalSource','Same physical controller is assigned twice')],error:''};
 class Preview{get state(){return current;}subscribe(listener){listener(current);return()=>{};}dispose(){}}
 vm.runInNewContext(ts.transpileModule(js.code,{compilerOptions:{module:ts.ModuleKind.CommonJS,target:ts.ScriptTarget.ES2022}}).outputText,{exports,require(name){if(name==='svelte/internal/server')return server;if(name==='svelte')return svelte;if(name==='./profile-vehicle-conflicts')return{ProfileVehicleConflictController:Preview};throw Error(name);}});
 const compiler={compile(){throw Error('Rendering cannot compile.');},inspect(){throw Error('Rendering cannot inspect.');}},props={...value,activeVehicleTabId:'a',compiler};
 let body=render(exports.default,{props}).body;assert.ok(body.includes('Base + F/A-18C conflicts'));assert.ok(body.includes('Same physical controller is assigned twice'));assert.ok(body.includes('base · button'));assert.ok(!body.includes('viper'));
 current={status:'ready',diagnostics:[],error:''};body=render(exports.default,{props}).body;assert.ok(body.includes('No conflicts found'));
 current={status:'error',diagnostics:[],error:'Native service disconnected'};body=render(exports.default,{props}).body;assert.ok(body.includes('Conflict check unavailable'));assert.ok(body.includes('Native service disconnected'));assert.ok(!render(exports.default,{props:{...props,active:false}}).body.includes('Vehicle configuration conflicts'));
});
