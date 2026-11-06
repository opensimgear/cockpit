// Opt-in native gateway regression for per-source baseline delivery.
import test from 'node:test';
import assert from 'node:assert/strict';
import {mkdir,writeFile} from 'node:fs/promises';
import {randomUUID} from 'node:crypto';
import {nativePage,until} from '../../scripts/reactive/cdp.mjs';

test('native profile baselines stay scoped to their source',{
  skip:process.env.OSG_PROFILE_BASELINE_SCOPE_NATIVE!=='1',timeout:120000,
},async()=>{
  const page=await nativePage();
  const directory=`.research/reactive/profile-baseline-scope-native/${Date.now()}`;
  await mkdir(directory,{recursive:true});
  let profile,owner;
  const request=async value=>{
    const reply=await page.evaluate(`window.__TAURI_INTERNALS__.invoke('gateway_request',{request:${JSON.stringify({...value,clientId:'profile-baseline-scope-acceptance'})}})`);
    assert.equal(reply.ok,true,`${value.command}: ${reply.error}`);return reply.state;
  };
  try{
    const initial=(await request({command:'profile_status'})).profiles.profileSession;
    assert.equal(initial.owner,null,'Start with no active output owner.');
    profile=(await request({command:'profile_create',name:`Baseline scope acceptance ${Date.now()}`})).profiles.document;
    const button={delivery:'state',schema:{type:'boolean'},freshnessMs:1000,fallback:false};
    const count={delivery:'state',schema:{type:'integer',min:0,max:1000,unit:null},freshnessMs:1000,fallback:0};
    profile.resources=[
      {id:'source-a',name:'Source A',kind:'debug-controller',required:false,ports:[{id:'button-1',name:'Button 1',direction:'output',contract:button}]},
      {id:'source-b',name:'Source B',kind:'debug-controller',required:false,ports:[{id:'button-1',name:'Button 1',direction:'output',contract:button}]},
    ];
    profile.processingNodes=[
      {id:'counter',name:'Source A callback counter',operation:{kind:'script',source:'fn on_start(ctx) { transaction(#{count:1}, #{out: scalar(1)}, []) } fn on_event(ctx) { transaction(#{count:ctx.state.count+1}, #{out: scalar(ctx.state.count+1)}, []) }',parameters:{}},
        inputs:[{id:'input',name:'Input',direction:'input',contract:button}],outputs:[{id:'out',name:'Count',direction:'output',contract:count}]},
      {id:'pulse',name:'Source B pulse',operation:{kind:'builtin',rule:{kind:'pulse',durationUs:500000,repeatUs:0},inputs:['input']},
        inputs:[{id:'input',name:'Input',direction:'input',contract:button}],outputs:[{id:'out',name:'Output',direction:'output',contract:button}]},
    ];
    profile.bindings=[
      {id:'a-counter',from:{nodeId:'source-a',portId:'button-1'},to:{nodeId:'counter',portId:'input'}},
      {id:'b-pulse',from:{nodeId:'source-b',portId:'button-1'},to:{nodeId:'pulse',portId:'input'}},
    ];
    profile=(await request({command:'profile_editor_save',document:profile,assignments:[],expectedRevision:profile.revision})).profiles.document;
    const applied=(await request({command:'profile_apply',id:profile.id,expectedRevision:profile.revision,expectedActiveOwner:null})).profiles.profileSession;
    assert.equal(applied.state,'running',JSON.stringify(applied));owner=applied.owner;
    const inspect=async nodeId=>(await request({command:'profile_runtime_inspect',owner,nodeId,offset:0,limit:64})).profileInspection;
    const sample=(page,portId='out')=>page.ports.find(port=>port.endpoint.portId===portId).sample;
    const aStarted=await until(()=>inspect('counter'),page=>sample(page).status==='known');
    assert.equal(sample(aStarted).value.value,1,'Only Source A startup may enqueue its callback.');
    const bBaseline=await until(()=>inspect('pulse'),page=>sample(page).status==='known');
    assert.equal(sample(bBaseline).value.value,false,'Source B fallback must not start a pulse during activation.');
    await new Promise(resolve=>setTimeout(resolve,100));
    const settledA=await inspect('counter');
    assert.equal(sample(settledA).value.value,1,'Source B baseline must not create an extra callback on Source A.');

    const streamA=randomUUID(),streamB=randomUUID();
    await request({command:'profile_debug_controller_input',owner,nodeId:'source-b',streamId:streamB,sequence:'1',changes:[{portId:'button-1',value:true}]});
    const bPulse=await until(()=>inspect('pulse'),page=>sample(page).value?.value===true);
    const afterB=await inspect('counter');
    assert.equal(sample(afterB).value.value,1,'A Source B event must not reach Source A callback.');
    await request({command:'profile_debug_controller_input',owner,nodeId:'source-a',streamId:streamA,sequence:'1',changes:[{portId:'button-1',value:true}]});
    const afterA=await until(()=>inspect('counter'),page=>sample(page).status==='known'&&sample(page).value.value===2);
    await new Promise(resolve=>setTimeout(resolve,100));
    const heldA=await inspect('counter');
    assert.equal(sample(heldA).value.value,2,'Holding Source A must not retrigger its callback.');
    for(let sequence=2;sequence<=17;sequence++){
      await request({command:'profile_debug_controller_input',owner,nodeId:'source-a',streamId:streamA,sequence:String(sequence),changes:[{portId:'button-1',value:true}]});
    }
    await new Promise(resolve=>setTimeout(resolve,100));
    const repeatedA=await inspect('counter');
    assert.equal(sample(repeatedA).value.value,2,'Repeated reports of a held level must not retrigger its callback.');
    await request({command:'profile_debug_controller_input',owner,nodeId:'source-a',streamId:streamA,sequence:'18',changes:[{portId:'button-1',value:false}]});
    const releasedA=await until(()=>inspect('counter'),page=>sample(page).status==='known'&&sample(page).value.value===3);

    await request({command:'profile_stop'});
    const stale=await page.evaluate(`window.__TAURI_INTERNALS__.invoke('gateway_request',{request:${JSON.stringify({command:'profile_debug_controller_input',clientId:'profile-baseline-scope-acceptance',owner,nodeId:'source-a',streamId:streamA,sequence:'19',changes:[{portId:'button-1',value:true}]})}})`);
    assert.equal(stale.ok,false,'Stop must reject input from the prior owner.');
    await writeFile(`${directory}/evidence.json`,JSON.stringify({at:new Date().toISOString(),actualNativeCockpit:true,profile:profile.id,owner,
      noPhysicalOutputs:true,simulatorQualified:false,latencyQualified:false,aStarted,settledA,bBaseline,bPulse,afterB,afterA,heldA,repeatedA,releasedA,repeatedHeldReports:16,staleOwnerRejected:true},null,2)+'\n');
  }finally{
    try{
      const current=(await request({command:'profile_status'})).profiles.profileSession.owner;
      if(owner&&current?.session===owner.session&&current.generation===owner.generation)await request({command:'profile_stop'});
      if(profile)await request({command:'profile_delete',id:profile.id,expectedRevision:profile.revision});
    }finally{page.close();}
  }
});
