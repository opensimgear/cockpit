// Opt-in: actual native worker, authored timer, no simulator or physical outputs.
import test from 'node:test';
import assert from 'node:assert/strict';
import {mkdir,writeFile} from 'node:fs/promises';
import {randomUUID} from 'node:crypto';
import {nativePage,until} from '../../scripts/reactive/cdp.mjs';

test('native routing expires an authored pulse without another input or visible preview',{
  skip:process.env.OSG_PROFILE_DEADLINES_NATIVE!=='1',timeout:120000,
},async()=>{
  const page=await nativePage();
  const directory=`.research/reactive/profile-deadlines-native/${Date.now()}`;
  await mkdir(directory,{recursive:true});
  let profile,owner;
  const request=async value=>{
    const reply=await page.evaluate(`window.__TAURI_INTERNALS__.invoke('gateway_request',{request:${JSON.stringify({...value,clientId:'profile-deadline-acceptance'})}})`);
    assert.equal(reply.ok,true,`${value.command}: ${reply.error}`);return reply.state;
  };
  try{
    const initial=(await request({command:'profile_status'})).profiles.profileSession;
    assert.equal(initial.owner,null,'Start with no active output owner.');
    assert.equal(initial.state,'stopped');
    profile=(await request({command:'profile_create',name:`Deadline acceptance ${Date.now()}`})).profiles.document;
    const contract={delivery:'state',schema:{type:'boolean'},freshnessMs:1000,fallback:false};
    profile.resources=[{id:'source',name:'Authored button',kind:'debug-controller',required:false,
      ports:[{id:'button-1',name:'Button 1',direction:'output',contract}]}];
    profile.processingNodes=[{id:'pulse',name:'Authored pulse',operation:{kind:'builtin',rule:{kind:'pulse',durationUs:50000,repeatUs:0},inputs:['in']},
      inputs:[{id:'in',name:'Input',direction:'input',contract}],outputs:[{id:'out',name:'Output',direction:'output',contract}]}];
    profile.bindings=[{id:'button-pulse',from:{nodeId:'source',portId:'button-1'},to:{nodeId:'pulse',portId:'in'}}];
    profile=(await request({command:'profile_editor_save',document:profile,assignments:[],expectedRevision:profile.revision})).profiles.document;
    const applied=(await request({command:'profile_apply',id:profile.id,expectedRevision:profile.revision,expectedActiveOwner:null})).profiles.profileSession;
    assert.equal(applied.state,'running',JSON.stringify(applied));owner=applied.owner;
    const inspect=async()=>(await request({command:'profile_runtime_inspect',owner,nodeId:'pulse',offset:0,limit:64})).profileInspection;
    const value=page=>page.ports.find(port=>port.endpoint.portId==='out').sample;
    assert.equal(value(await inspect()).value.value,false);
    const streamId=randomUUID(),start=performance.now();
    await request({command:'profile_debug_controller_input',owner,nodeId:'source',streamId,sequence:'1',changes:[{portId:'button-1',value:true}]});
    const high=await until(inspect,page=>value(page).status==='known'&&value(page).value.value===true);
    // No read, input, output or UI preview supplies the expiration work.
    await new Promise(resolve=>setTimeout(resolve,100));
    const low=await inspect(),elapsedMs=performance.now()-start;
    assert.equal(value(low).status,'known');assert.equal(value(low).value.value,false);
    await new Promise(resolve=>setTimeout(resolve,100));
    const held=await inspect();assert.equal(value(held).value.value,false,'A held level must not retrigger this pulse.');
    await request({command:'profile_stop'});
    const stale=await page.evaluate(`window.__TAURI_INTERNALS__.invoke('gateway_request',{request:${JSON.stringify({command:'profile_debug_controller_input',clientId:'profile-deadline-acceptance',owner,nodeId:'source',streamId,sequence:'2',changes:[{portId:'button-1',value:false}]})}})`);
    assert.equal(stale.ok,false,'Stop must fence late old-owner input.');
    await writeFile(`${directory}/evidence.json`,JSON.stringify({at:new Date().toISOString(),actualNativeCockpit:true,profile:profile.id,owner,
      noPhysicalOutputs:true,simulatorQualified:false,latencyQualified:false,authoredDurationUs:50000,high,low,held,elapsedMs,staleOwnerRejected:true},null,2)+'\n');
  }finally{
    try{
      const current=(await request({command:'profile_status'})).profiles.profileSession.owner;
      if(owner&&current?.session===owner.session&&current.generation===owner.generation)await request({command:'profile_stop'});
      if(profile)await request({command:'profile_delete',id:profile.id,expectedRevision:profile.revision});
    }finally{page.close();}
  }
});
