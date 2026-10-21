import test from 'node:test';
import assert from 'node:assert/strict';
import {summarize} from './assetto-live.mjs';

const configuration={minSpeed:0};
function live(speed,rpm=1000){
  const values={rpm,gear:1,speed_kmh:speed,max_rpm:9000,rev_ratio:rpm/9000};
  return {channels:Object.fromEntries(Object.entries(values).map(([id,value])=>[id,{status:'known',value}])),vehicle:{id:'actual_car'},processVerified:true,detected:true};
}
function withdrawn(){
  const row=live(0);for(const sample of Object.values(row.channels)){sample.status='stale';sample.value=null;}row.vehicle=null;row.processVerified=false;return row;
}

test('driving-runtime exit allows the original Assetto Corsa menu to remain',()=>{
  const rows=[withdrawn(),withdrawn()];
  const runtime=summarize(rows,'exit',configuration);
  assert.equal(runtime.passed,true);assert.equal(runtime.exitScope,'driving-runtime');assert.equal(runtime.applicationDetected,true);
  const application=summarize(rows,'exit',{...configuration,'require-game-closed':true});
  assert.equal(application.passed,false);assert.equal(application.exitScope,'whole-application');
  rows[1].processVerified=true;
  assert.equal(summarize(rows,'exit',configuration).passed,false);
});

test('idle physics jitter does not qualify as material telemetry change or driving',()=>{
  const idle=[live(0.001,1000),live(0.002,1001),live(0.0015,1000)];
  const summary=summarize(idle,'capture',{...configuration,minSpeed:5,'require-change':true});
  assert.equal(summary.passed,false);assert.ok(summary.problems.some(message=>message.includes('Speed did not reach 5')));assert.ok(summary.problems.some(message=>message.includes('No material')));
  assert.equal(summarize([live(0),live(10),live(20)],'capture',{...configuration,minSpeed:5,'require-change':true,'require-all':true}).passed,true);
});

test('unavailable observations reject a return of known telemetry',()=>{
  assert.equal(summarize([withdrawn(),withdrawn()],'pause',configuration).passed,true);
  assert.equal(summarize([withdrawn(),withdrawn(),live(5)],'stale',configuration).passed,false);
});

test('new live setup retires the old process owner before setup and check',async()=>{
  const {verifyLiveSetup}=await import('./assetto-live.mjs');
  const requests=[];let active=true;
  const native=async request=>{requests.push(request.command);if(request.command==='profile_stop')active=false;if(request.command.startsWith('game_integration_'))assert.equal(active,false);};
  const game={integrationSetup:{state:'ready',verified:true}},evidence={};
  await verifyLiveSetup(native,'assetto-corsa',async()=>({game}),async check=>check(),evidence);
  assert.deepEqual(requests,['profile_selection_mode','profile_stop','game_integration_setup','game_integration_check']);
  assert.equal(evidence.checkedSetup.verified,true);
});

test('reapply observations keep the live channel and motion requirements',()=>{
  assert.equal(summarize([live(5),live(20)],'reapply',{...configuration,minSpeed:5,'require-all':true,'require-change':true}).passed,true);
  assert.equal(summarize([withdrawn(),withdrawn()],'reapply',{...configuration,'require-all':true}).passed,false);
});

test('requiring all channels needs sustained redline delivery rather than one lucky sample',()=>{
  const rows=[live(5),live(10)];rows[1].channels.max_rpm={status:'unknown',value:null};rows[1].channels.rev_ratio={status:'unknown',value:null};
  assert.equal(summarize(rows,'capture',configuration).passed,true);
  assert.equal(summarize(rows,'capture',{...configuration,'require-all':true}).passed,false);
});

test('requiring all channels also checks redline withdrawal after pause or process exit',()=>{
  const rows=[withdrawn(),withdrawn()];for(const row of rows){row.channels.max_rpm={status:'known',value:9000};row.channels.rev_ratio={status:'known',value:0.1};}
  assert.equal(summarize(rows,'pause',configuration).passed,true);
  assert.equal(summarize(rows,'pause',{...configuration,'require-all':true}).passed,false);
  assert.equal(summarize(rows,'exit',{...configuration,'require-all':true}).passed,false);
});

test('speed-gated capture waits for motion and all requested live channels',async()=>{
  const {readyForObservation}=await import('./assetto-live.mjs');
  const config={mode:'observe',minSpeed:5,'wait-speed':true,'require-all':true};
  assert.equal(readyForObservation(live(0),config),false);
  assert.equal(readyForObservation(live(4.999),config),false);
  assert.equal(readyForObservation(live(5),config),true);
  const missing=live(10);missing.channels.rpm={status:'stale',value:null};
  assert.equal(readyForObservation(missing,config),false);
  const redline=live(10);redline.channels.max_rpm={status:'unknown',value:null};
  assert.equal(readyForObservation(redline,config),false);
  assert.equal(readyForObservation(redline,{...config,'require-all':false}),true);
  assert.equal(readyForObservation(live(0),{...config,'wait-speed':false}),true);
});
