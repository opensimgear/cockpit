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
const modules=await loadModules(['./game-telemetry','./games']);
const {GameTelemetryController}=modules.get('./game-telemetry');
const noRequest=()=>{throw Error('Rendering telemetry must not send a native request.');};
const channel=(overrides={})=>({id:'vehicle.speed',label:'Vehicle speed',category:'physics',type:'number',unit:'m/s',status:'known',value:0,ageMs:50,nativeField:'speedKmh',provider:'shared-memory',description:'Speed in common SI units.',...overrides});
const fixture=(overrides={})=>({schemaVersion:1,games:[{id:'assetto-corsa',name:'Assetto Corsa',status:'live',source:'Native shared memory · acpmf_physics',packet:42,ageMs:50,context:{car:'Abarth 500',track:'Imola'},groups:[{id:'physics',label:'Physics',total:2},{id:'engine',label:'Engine',total:1}],total:3,offset:0,limit:64,channels:[channel(),channel({id:'engine.rpm',label:'Engine speed',category:'engine',unit:'rpm',status:'unknown',value:null,ageMs:null}),channel({id:'vehicle.brake',label:'Brake',unit:'ratio',value:0.25})],...overrides}]});
const filename=new URL('./GameTelemetryInspector.svelte',import.meta.url),source=await readFile(filename,'utf8'),{js}=compile(source,{filename:filename.pathname,generate:'server'}),exports={};
vm.runInNewContext(ts.transpileModule(js.code,{compilerOptions:{module:ts.ModuleKind.CommonJS,target:ts.ScriptTarget.ES2022}}).outputText,{exports,TextEncoder,require(name){if(name==='svelte/internal/server')return server;if(name==='svelte')return svelte;if(modules.has(name))return modules.get(name);if(name==='./gateway')return{gatewayRequest:noRequest};throw Error(`Unexpected telemetry dependency ${name}`);}},{filename:filename.pathname});
async function body(reply=fixture(),query={gameId:'assetto-corsa',offset:0,limit:64},stale=false){const controller=new GameTelemetryController({inspect:async()=>reply});controller.configure(query);controller.setActive(true);await controller.refresh();if(stale)controller.setActive(false);return render(exports.default,{props:{controller}}).body;}

test('incoming telemetry is organized by game, source, canonical channel and category without a profile',async()=>{
  const html=await body();
  for(const text of ['aria-label="Telemetry game"','Assetto Corsa Competizione','Assetto Corsa EVO','X-Plane 12','aria-label="Game telemetry source"','Native shared memory','Last packet','42','50 ms','Abarth 500','Imola','aria-label="Telemetry categories"','Physics','Engine','aria-label="Search game telemetry"','vehicle.speed','Vehicle speed','Native: speedKmh','m/s','provider context'])assert.ok(html.includes(text),text);
  assert.match(html,/>0<\/td>/);assert.ok(html.includes('Unknown'));assert.match(html,/>—<\/td>/);
  assert.ok(html.includes('Native field: speedKmh'));assert.ok(html.includes('Provider: shared-memory'));assert.ok(!html.includes('<pre>'));
});

test('paused and stale sources remain explicit and stale captures never present last values as current',async()=>{
  assert.ok((await body(fixture({status:'paused'}))).includes('Paused'));
  const stale=await body(fixture(),undefined,true);assert.ok(stale.includes('Stale capture'));assert.ok(!/>0\.25<\/td>/.test(stale));assert.ok(stale.includes('Stale'));
  const waiting=await body(fixture({status:'waiting',packet:null,ageMs:null,total:0,groups:[],channels:[]}));
  assert.ok(waiting.includes('Waiting'));assert.ok(waiting.includes('Not observed'));assert.ok(waiting.includes('No incoming channels are available yet'));
});
test('undocumented native units are explicit rather than described as dimensionless',async()=>{
  const html=await body(fixture({channels:[channel({id:'flight.shake',label:'Shake amplitude',unit:null})],total:1}));assert.ok(html.includes('Not specified'));assert.ok(!html.includes('Unitless'));
});

test('filtered pages expose exact returned counts and accessible previous/next controls',async()=>{
  const html=await body(fixture({total:130,offset:64,channels:[channel()]}),{gameId:'assetto-corsa',offset:64,limit:64,group:'physics',search:'speed'});
  assert.ok(html.includes('65–65 of 130'));assert.ok(html.includes('130 matching channels'));assert.ok(html.includes('value="speed"'));
  assert.ok(html.includes('Previous channels'));assert.ok(html.includes('Next channels'));assert.match(html,/aria-pressed="true"[^>]*>Physics/);
  const empty=await body(fixture({total:0,channels:[]}),{gameId:'assetto-corsa',offset:0,limit:64,search:'missing'});assert.ok(empty.includes('No incoming channels match this selection.'));assert.ok(empty.includes('0–0 of 0'));
});

test('the mounted telemetry view polls only its visible active section and cleans up its timer and visibility listener',async()=>{
  let handlers,resolveFirst;const calls=[],timers=new Map(),listeners=new Map(),document={hidden:false,addEventListener:(name,listener)=>listeners.set(name,listener),removeEventListener:(name,listener)=>{if(listeners.get(name)===listener)listeners.delete(name);}};
  const controller=new GameTelemetryController({inspect:query=>{calls.push(query);return calls.length===1?new Promise(resolve=>resolveFirst=resolve):Promise.resolve(fixture());}});
  const instrumented=source.replace('</script>',"captureTelemetry({watchTelemetry,mount(value){mounted=value;},activate(value){active=value;}});</script>");
  const built=compile(instrumented,{filename:filename.pathname,generate:'server'}),local={};
  vm.runInNewContext(ts.transpileModule(built.js.code,{compilerOptions:{module:ts.ModuleKind.CommonJS,target:ts.ScriptTarget.ES2022}}).outputText,{exports:local,TextEncoder,document,window:{setInterval:callback=>{timers.set(1,callback);return 1;},clearInterval:id=>timers.delete(id)},captureTelemetry:value=>handlers=value,require(name){if(name==='svelte/internal/server')return server;if(name==='svelte')return svelte;if(modules.has(name))return modules.get(name);if(name==='./gateway')return{gatewayRequest:noRequest};throw Error(`Unexpected telemetry dependency ${name}`);}});
  const rendered=render(local.default,{props:{controller}}).body;assert.ok(rendered.includes('Incoming game telemetry'));assert.equal(handlers.watchTelemetry(),undefined);assert.equal(calls.length,0);
  handlers.mount(true);const cleanup=handlers.watchTelemetry();assert.equal(calls.length,1);assert.equal(timers.size,1);assert.equal(listeners.size,1);
  document.hidden=true;listeners.get('visibilitychange')();timers.get(1)();assert.equal(controller.state.active,false);assert.equal(calls.length,1);
  resolveFirst(fixture());await new Promise(resolve=>setImmediate(resolve));assert.equal(controller.state.report,null);
  document.hidden=false;listeners.get('visibilitychange')();await new Promise(resolve=>setImmediate(resolve));assert.equal(calls.length,2);assert.equal(controller.state.stale,false);
  cleanup();assert.equal(timers.size,0);assert.equal(listeners.size,0);assert.equal(controller.state.active,false);
  handlers.activate(false);assert.equal(handlers.watchTelemetry(),undefined);assert.equal(calls.length,2);assert.equal(timers.size,0);controller.dispose();
});
