import assert from 'node:assert/strict';
import test from 'node:test';
import { ProfileRuntimeClient, nativeProfileStatus,nativeProfileActivation } from './profile-runtime.ts';
const id='00000000-0000-4000-8000-000000000001',run='11111111-1111-4111-8111-111111111111';
const stopped=()=>({state:'stopped',appliedSnapshot:null,owner:null,degradation:[],fault:null});
const running=()=>({...stopped(),state:'running',appliedSnapshot:{profileId:id,revision:3,semanticHash:`sha256:${'a'.repeat(64)}`},owner:{session:run,generation:4,profileId:id,profileRevision:3,gameId:null}});

test('activation progress and cancellation bind exact operation identity without ordinary stop authority',async()=>{
  const calls=[],progress={activationId:run,profileId:id,phase:'preparing',canCancel:true,cancelRequested:false,message:'Checking devices.',companions:[],outcome:null,error:null};
  const client=new ProfileRuntimeClient(async request=>{calls.push(request);return request.command==='profile_apply'?running():progress;});
  await client.apply(id,3,undefined,null,run);const snapshot=await client.activationStatus(run);await client.cancelActivation(run);
  assert.deepEqual(calls,[{command:'profile_apply',id,expectedRevision:3,expectedActiveOwner:null,activationId:run},{command:'profile_activation_status',activationId:run},{command:'profile_activation_cancel',activationId:run}]);
  progress.message='changed';assert.equal(snapshot.message,'Checking devices.');assert.ok(Object.isFrozen(snapshot));
  for(const patch of [{activationId:id},{phase:['preparing']},{phase:'ready'},{outcome:['active']},{profileId:'other'},{companions:Array(9).fill({})}])assert.throws(()=>nativeProfileActivation({...progress,...patch},run),/progress is unavailable/);
  await assert.rejects(client.activationStatus('old'),/exact profile activation/);assert.throws(()=>client.apply(id,3,undefined,null,'old'),/exact saved/);
});

test('cleanup progress retains native phases, origin and actual cleanup steps',()=>{
  for(const [phase,origin]of[['deactivating','deactivation'],['recovering','recovery']]){
    const wire={activationId:run,profileId:id,phase,origin,step:'unhiding-peripherals',steps:[{step:'releasing-bindings',message:'Releasing keyboard and mouse bindings'},{step:'unhiding-peripherals',message:'Unhiding peripherals'}],canCancel:false,cancelRequested:false,message:'Unhiding peripherals',companions:[],outcome:null,error:null};
    const value=nativeProfileActivation(wire,run);wire.steps[0].message='changed';
    assert.equal(value.phase,phase);assert.equal(value.origin,origin);assert.equal(value.message,'Unhiding peripherals');assert.equal(value.steps[0].message,'Releasing keyboard and mouse bindings');assert.ok(Object.isFrozen(value.steps));
  }
});

test('native runtime snapshots retain exact applied authority and never share mutable transport state',()=>{
  const wire=running(),captured=nativeProfileStatus(wire);wire.owner.generation=7;wire.appliedSnapshot.revision=9;wire.degradation.push('later');
  assert.equal(captured.owner.generation,4);assert.equal(captured.appliedSnapshot.revision,3);assert.deepEqual(captured.degradation,[]);
  assert.throws(()=>{captured.owner.profileRevision=99;},TypeError);
  for(const state of ['stopped','faulted','recovery-needed','transitioning'])assert.equal(nativeProfileStatus({...stopped(),state}).state,state);
  assert.equal(nativeProfileStatus({...running(),state:'degraded',degradation:['Game telemetry is stale.']}).state,'degraded');
  assert.equal(nativeProfileStatus({...running(),state:'awaiting-game'}).state,'awaiting-game');
});
test('explicit controls send saved identity and revision without document, resource tokens, telemetry or inferred context',async()=>{
  const calls=[],client=new ProfileRuntimeClient(async request=>{calls.push(request);return stopped();});
  await client.status();await client.apply(id,3);await client.apply(id,3,'dcs');await client.stop();await client.recover();
  assert.deepEqual(calls,[{command:'profile_status'},{command:'profile_apply',id,expectedRevision:3},{command:'profile_apply',id,expectedRevision:3,gameId:'dcs'},{command:'profile_stop'},{command:'profile_recover'}]);
});

test('activation serializes an explicit inactive guard or independent exact active-owner guard',async()=>{
  const calls=[],client=new ProfileRuntimeClient(async request=>{calls.push(request);return stopped();}),expected=running().owner;
  await client.apply(id,3,undefined,null);await client.apply(id,3,'dcs',expected);expected.generation=99;expected.gameId='x-plane-12';
  assert.deepEqual(calls,[{command:'profile_apply',id,expectedRevision:3,expectedActiveOwner:null},{command:'profile_apply',id,expectedRevision:3,gameId:'dcs',expectedActiveOwner:running().owner}]);assert.notEqual(calls[1].expectedActiveOwner,expected);
});
test('invalid requests never reach native transport and a native rejection is not retried or converted to success',async()=>{
  let calls=0;const client=new ProfileRuntimeClient(async()=>{calls++;throw new Error('required resource missing');});
  for(const [target,revision,game]of[['wrong',1], [id,0],[id,Infinity],[id,Number.MAX_SAFE_INTEGER+1],[id,1,'wrong'],[id,1,null]])assert.throws(()=>client.apply(target,revision,game),/exact saved/);
  assert.equal(calls,0);
  for(const expectedOwner of [false,'active',{},[],{...running().owner,session:'old'},{...running().owner,generation:0},{...running().owner,generation:1.5},{...running().owner,generation:Number.MAX_SAFE_INTEGER+1},{...running().owner,profileId:'wrong'},{...running().owner,profileRevision:0},{...running().owner,gameId:'wrong'},{...running().owner,gameId:undefined}])assert.throws(()=>client.apply(id,3,undefined,expectedOwner),/exact saved/);
  assert.equal(calls,0,'A malformed confirmation must never reach native transport.');
  await assert.rejects(client.apply(id,1),/required resource missing/);assert.equal(calls,1);
});
test('malformed and inconsistent native evidence cannot become running UI state',()=>{
  const invalid=[null,[],{}, {...stopped(),state:'ready'}, {...stopped(),state:'paused'},{...stopped(),fault:''},{...stopped(),degradation:['\0']},{...stopped(),degradation:Array(1025).fill('error')}, {...stopped(),appliedSnapshot:{profileId:id,revision:0,semanticHash:'hash'}},{...running(),owner:{...running().owner,gameId:'unknown'}},{...running(),owner:{...running().owner,generation:0}},{...running(),owner:{...running().owner,session:'old'}},{...running(),appliedSnapshot:{...running().appliedSnapshot,semanticHash:'hash'}},{...running(),state:'stopped'},{...running(),appliedSnapshot:null},{...running(),owner:null},{...running(),owner:{...running().owner,profileRevision:2}},{...running(),owner:{...running().owner,profileId:run}}];
  for(const value of invalid)assert.throws(()=>nativeProfileStatus(value),/malformed|inconsistent/,JSON.stringify(value));
  assert.equal(nativeProfileStatus({...running(),owner:{...running().owner,gameId:'dcs'}}).owner.gameId,'dcs');
});
test('control responses require complete native status and do not imply simulator confirmation',async()=>{
  const client=new ProfileRuntimeClient(async()=>({state:'running'}));await assert.rejects(client.status(),/malformed/);
  const status=nativeProfileStatus({...stopped(),state:'recovery-needed',fault:'Inspect owned receipts.\nNo workers restarted.'});assert.equal(status.owner,null);assert.equal(status.appliedSnapshot,null);
});

test('held launch status retains exact owner without accepting coerced states or game identities',()=>{
  for(const value of [{...running(),state:['running']},{...running(),owner:{...running().owner,gameId:['dcs']}},{...running(),state:'awaiting-game',owner:null}])assert.throws(()=>nativeProfileStatus(value),/malformed|inconsistent/);
});
