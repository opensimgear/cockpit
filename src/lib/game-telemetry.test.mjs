import assert from 'node:assert/strict';
import test from 'node:test';
import {loadModules} from './test-module-loader.mjs';
const modules=await loadModules(['./game-telemetry']);
const {GameTelemetryClient,GameTelemetryController,telemetryAge,telemetryValue,telemetryStatus,preferredTelemetryGame}=modules.get('./game-telemetry');
const plain=value=>JSON.parse(JSON.stringify(value));
const channel=(overrides={})=>({id:'vehicle.speed',label:'Vehicle speed',category:'physics',type:'number',unit:'m/s',status:'known',value:0,ageMs:0,nativeField:'speedKmh',provider:'shared-memory',...overrides});
const game=(overrides={})=>({id:'assetto-corsa',name:'Assetto Corsa',status:'live',source:'Native shared memory',packet:42,ageMs:0,context:{car:'abarth500',track:'imola'},groups:[{id:'physics',label:'Physics',total:1}],total:1,offset:0,limit:64,channels:[channel()],...overrides});
const report=(overrides={})=>({schemaVersion:1,games:[game(overrides)]});
const deferred=()=>{let resolve,reject;const promise=new Promise((yes,no)=>{resolve=yes;reject=no;});return{promise,resolve,reject};};
const tick=()=>new Promise(resolve=>setImmediate(resolve));

test('telemetry captures preserve canonical IDs, native context and zero without mutating the reply',async()=>{
  const calls=[],reply=report(),client=new GameTelemetryClient(async request=>{calls.push(plain(request));return reply;});
  const result=await client.inspect({gameId:'assetto-corsa',offset:0,limit:64,group:'physics',search:'speed'});
  assert.deepEqual(calls,[{command:'game_telemetry_inspect',gameId:'assetto-corsa',offset:0,limit:64,group:'physics',search:'speed'}]);
  assert.equal(result.games[0].channels[0].id,'vehicle.speed');assert.equal(result.games[0].channels[0].nativeField,'speedKmh');
  assert.equal(telemetryValue(result.games[0].channels[0]),'0');reply.games[0].channels[0].value=100;
  assert.equal(result.games[0].channels[0].value,0);assert.ok(Object.isFrozen(result.games[0].channels[0]));
});

test('telemetry rejects malformed, duplicate, nonfinite and impossible native pages',async()=>{
  const malformed=[{schemaVersion:1,games:[]},report({channels:[channel({value:Infinity})]}),report({channels:[channel(),channel()],total:2}),report({offset:2,total:1}),report({channels:[channel()],limit:0}),report({channels:[channel({value:{raw:1}})]}),report({groups:[{id:'physics',label:'Physics',total:1},{id:'physics',label:'Again',total:1}]}),{schemaVersion:1,games:[game(),game()]}];
  for(const reply of malformed){const client=new GameTelemetryClient(async()=>reply);await assert.rejects(()=>client.inspect({offset:0,limit:64}),/invalid or incomplete/);}
  await assert.rejects(()=>new GameTelemetryClient(async()=>report()).inspect({gameId:'assetto-corsa-evo',offset:0,limit:64}),/invalid or incomplete/);
});

test('formatting keeps unavailable samples separate from zero, false, arrays and units',()=>{
  assert.equal(telemetryValue(channel()),'0');assert.equal(telemetryValue(channel({value:false,type:'boolean',unit:null})),'False');
  assert.equal(telemetryValue(channel({value:'',type:'string',unit:null})),'Empty');
  assert.equal(telemetryValue(channel({status:'unknown',value:0})),'—');assert.equal(telemetryValue(channel(),true),'—');assert.equal(telemetryValue(channel({value:null})),'—');
  assert.equal(telemetryValue(channel({value:[0,-1.25,true,null]})),'[0, -1.25, True, —]');
  assert.equal(telemetryAge(null),'Not observed');assert.equal(telemetryAge(0),'0 ms');assert.equal(telemetryAge(1250),'1.3 s');assert.equal(telemetryStatus('paused'),'Paused');
  assert.equal(preferredTelemetryGame([game({status:'not-installed'}),game({id:'assetto-corsa-evo',status:'paused'})]),'assetto-corsa-evo');
});

test('passive telemetry only captures while active and never overlaps a request',async()=>{
  const pending=deferred(),calls=[];let now=100;
  const controller=new GameTelemetryController({inspect:async query=>{calls.push(plain(query));return pending.promise;}},()=>now);
  assert.equal(await controller.refresh(),false);assert.equal(calls.length,0);
  controller.setActive(true);const read=controller.refresh();assert.equal(controller.state.loading,true);
  assert.equal(await controller.refresh(),false);assert.equal(calls.length,1);
  pending.resolve(report());assert.equal(await read,true);assert.equal(controller.state.updatedAt,100);assert.equal(controller.state.stale,false);
  controller.setActive(false);assert.equal(controller.state.stale,true);assert.equal(await controller.refresh(),false);assert.equal(calls.length,1);
  controller.dispose();controller.setActive(true);assert.equal(await controller.refresh(),false);assert.equal(calls.length,1);
});

test('changing game or filter fences an older capture and queues exactly one current page',async()=>{
  const requests=[],first=deferred(),second=deferred();
  const controller=new GameTelemetryController({inspect:query=>{requests.push(plain(query));return requests.length===1?first.promise:second.promise;}});
  controller.setActive(true);const old=controller.refresh();
  controller.configure({gameId:'assetto-corsa-evo',offset:64,limit:64,search:' rpm ',group:'engine'});
  controller.configure({gameId:'assetto-corsa-competizione',offset:0,limit:64,search:' brake ',group:'physics'});
  assert.equal(await controller.refresh(),false);assert.equal(requests.length,1);assert.equal(controller.state.report,null);
  first.resolve(report());assert.equal(await old,false);await tick();assert.equal(requests.length,2);
  assert.deepEqual(requests[1],{gameId:'assetto-corsa-competizione',offset:0,limit:64,search:'brake',group:'physics'});assert.equal(controller.state.report,null);
  second.resolve(report({id:'assetto-corsa-competizione',name:'Assetto Corsa Competizione'}));await tick();
  assert.equal(controller.state.report.games[0].id,'assetto-corsa-competizione');assert.equal(controller.state.loading,false);assert.equal(controller.state.stale,false);
});

test('hide, resume and dispose fence late replies; failures retain only an explicitly stale capture',async()=>{
  const first=deferred(),next=deferred();let count=0;
  const controller=new GameTelemetryController({inspect:()=>++count===1?first.promise:next.promise});
  controller.setActive(true);const old=controller.refresh();controller.setActive(false);controller.setActive(true);
  first.resolve(report());assert.equal(await old,false);await tick();assert.equal(count,2);assert.equal(controller.state.report,null);
  next.resolve(report());await tick();assert.equal(controller.state.stale,false);
  controller.dispose();assert.equal(controller.state.report,null);assert.equal(await controller.refresh(),false);
  let failing=false;const failure=new GameTelemetryController({inspect:async()=>{if(failing)throw Error('Source disconnected');return report();}});
  failure.setActive(true);await failure.refresh();failing=true;assert.equal(await failure.refresh(),false);
  assert.match(failure.state.error,/Source disconnected/);assert.equal(failure.state.stale,true);assert.equal(telemetryValue(failure.state.report.games[0].channels[0],failure.state.stale),'—');
});
