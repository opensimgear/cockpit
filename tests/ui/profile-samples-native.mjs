// Opt-in native UI stream acceptance for read-only samples of an exact profile owner.
import test from 'node:test';
import assert from 'node:assert/strict';
import {mkdir,writeFile} from 'node:fs/promises';
import {randomUUID} from 'node:crypto';
import {nativePage,until} from '../../scripts/reactive/cdp.mjs';

test('native profile samples use shared actual-Tauri observations and fence retired owners',{
  skip:process.env.OSG_PROFILE_SAMPLES_NATIVE!=='1',timeout:120000,
},async()=>{
  const page=await nativePage();
  const directory=`.research/reactive/profile-samples-native/${Date.now()}`;
  await mkdir(directory,{recursive:true});
  let profile,owner;
  const request=async value=>{
    const reply=await page.evaluate(`window.__TAURI_INTERNALS__.invoke('gateway_request',{request:${JSON.stringify({...value,clientId:'profile-samples-native-acceptance'})}})`);
    assert.equal(reply.ok,true,`${value.command}: ${reply.error}`);return reply.state;
  };
  const equalOwner=(left,right)=>left?.session===right?.session&&left?.generation===right?.generation;
  try{
    const initial=(await request({command:'profile_status'})).profiles.profileSession;
    assert.equal(initial.owner,null,'Start with no active profile owner.');
    const created=(await request({command:'profile_create',name:`Native samples ${randomUUID()}`})).profiles.document;
    profile=created;
    const boolean={delivery:'state',schema:{type:'boolean'},freshnessMs:1000,fallback:false};
    created.resources=[{id:'debug',name:'Debug input',kind:'debug-controller',required:false,
      ports:[{id:'button-1',name:'Button',direction:'output',contract:boolean}]}];
    created.processingNodes=[{id:'mirror',name:'Boolean mirror',operation:{kind:'identity'},
      inputs:[{id:'in',name:'Input',direction:'input',contract:boolean}],
      outputs:[{id:'out',name:'Output',direction:'output',contract:boolean}]}];
    created.bindings=[{id:'debug-to-mirror',from:{nodeId:'debug',portId:'button-1'},to:{nodeId:'mirror',portId:'in'}}];
    profile=(await request({command:'profile_editor_save',document:created,assignments:[],expectedRevision:created.revision})).profiles.document;
    assert.equal(profile.virtualOutputs.length,0,'The fixture has no virtual outputs.');
    assert.equal((profile.desktopOutputs??[]).length,0,'The fixture has no desktop outputs.');

    const applied=(await request({command:'profile_apply',id:profile.id,expectedRevision:profile.revision,expectedActiveOwner:null})).profiles.profileSession;
    assert.equal(applied.state,'running',JSON.stringify(applied));owner=applied.owner;
    const installed=await page.evaluate(`(async()=>{
      const {observeProfileNodeSamples}=await import('/src/lib/profile-samples/shared.ts');
      window.__profileSampleProbe={first:[],second:[],old:[],stopFirst:null,stopSecond:null,stopOld:null,stopReplacement:null};
      window.__profileSampleProbe.stopFirst=observeProfileNodeSamples(${JSON.stringify(owner)},'mirror',state=>window.__profileSampleProbe.first.push(structuredClone(state)));
      window.__profileSampleProbe.stopSecond=observeProfileNodeSamples(${JSON.stringify(owner)},'mirror',state=>window.__profileSampleProbe.second.push(structuredClone(state)));
      return true;
    })()`);
    assert.equal(installed,true);
    const states=()=>page.evaluate(`({first:window.__profileSampleProbe.first.at(-1),second:window.__profileSampleProbe.second.at(-1)})`);
    const output=state=>state?.value?.ports?.find(port=>port.endpoint.portId==='out')?.sample;

    // Both consumers must receive the native baseline before any explicit debug input.
    const baseline=await until(states,value=>value.first?.status==='ready'&&value.second?.status==='ready'
      &&output(value.first)?.status==='known'&&output(value.first)?.value?.value===false
      &&output(value.second)?.status==='known'&&output(value.second)?.value?.value===false);
    assert.deepEqual(baseline.first.value.owner,owner);
    assert.deepEqual(baseline.second.value.owner,owner);
    assert.equal(baseline.first.value.nodeId,'mirror');

    const streamId=randomUUID();
    await request({command:'profile_debug_controller_input',owner,nodeId:'debug',streamId,sequence:'1',changes:[{portId:'button-1',value:true}]});
    const high=await until(states,value=>output(value.first)?.value?.value===true&&output(value.second)?.value?.value===true);
    assert.equal(output(high.first).value.value,true);
    assert.equal(output(high.second).value.value,true);
    await request({command:'profile_debug_controller_input',owner,nodeId:'debug',streamId,sequence:'2',changes:[{portId:'button-1',value:false}]});
    const low=await until(states,value=>output(value.first)?.status==='known'&&output(value.first)?.value?.value===false
      &&output(value.second)?.status==='known'&&output(value.second)?.value?.value===false);
    assert.equal(output(low.first).value.value,false,'A false sample is preserved as a known Boolean.');
    assert.equal(output(low.second).value.value,false,'The second consumer sees the same false sample.');

    await request({command:'profile_stop'});
    const stopped=await until(states,value=>value.first?.status==='lost'&&value.second?.status==='lost');
    assert.equal(stopped.first.value,undefined,'Loss clears the first cached value.');
    assert.equal(stopped.second.value,undefined,'Loss clears the second cached value.');
    await page.evaluate('window.__profileSampleProbe.stopFirst();window.__profileSampleProbe.stopSecond();');
    // Re-importing the module gives the same frontend singleton and tests that a retired identity cannot reopen.
    const oldInstalled=await page.evaluate(`(async()=>{
      const {observeProfileNodeSamples}=await import('/src/lib/profile-samples/shared.ts');
      window.__profileSampleProbe.stopOld=observeProfileNodeSamples(${JSON.stringify(owner)},'mirror',state=>window.__profileSampleProbe.old.push(structuredClone(state)));
      return true;
    })()`);
    assert.equal(oldInstalled,true);
    const oldRejected=await until(()=>page.evaluate('window.__profileSampleProbe.old.at(-1)'),state=>state?.status==='error');
    assert.match(oldRejected.error,/owner|applied|running|current/i);
    await page.evaluate('window.__profileSampleProbe.stopOld();');

    const retiredOwner=owner;
    const replacement=(await request({command:'profile_apply',id:profile.id,expectedRevision:profile.revision,expectedActiveOwner:null})).profiles.profileSession;
    assert.equal(replacement.state,'running',JSON.stringify(replacement));
    assert.equal(equalOwner(replacement.owner,owner),false,'Restart creates a replacement owner.');
    const replacementInstalled=await page.evaluate(`(async()=>{
      const {observeProfileNodeSamples}=await import('/src/lib/profile-samples/shared.ts');
      window.__profileSampleProbe.replacement=[];
      window.__profileSampleProbe.stopReplacement=observeProfileNodeSamples(${JSON.stringify(replacement.owner)},'mirror',state=>window.__profileSampleProbe.replacement.push(structuredClone(state)));
      return true;
    })()`);
    assert.equal(replacementInstalled,true);owner=replacement.owner;
    const fresh=await until(()=>page.evaluate('window.__profileSampleProbe.replacement.at(-1)'),state=>state?.status==='ready');
    assert.deepEqual(fresh.value.owner,replacement.owner);
    assert.equal(output(fresh)?.status,'known');
    assert.equal(output(fresh)?.value?.value,false);
    const staleInput=await page.evaluate(`window.__TAURI_INTERNALS__.invoke('gateway_request',{request:${JSON.stringify({command:'profile_debug_controller_input',clientId:'profile-samples-native-acceptance',owner:retiredOwner,nodeId:'debug',streamId,sequence:'3',changes:[{portId:'button-1',value:true}]})}})`);
    assert.equal(staleInput.ok,false,'The prior owner cannot write into its replacement.');

    await request({command:'profile_stop'});
    const evidence={at:new Date().toISOString(),actualNativeCockpit:true,profile:profile.id,owner:replacement.owner,
      baselineWithoutExplicitInput:baseline,knownTrueChange:high,knownFalseChange:low,lossAfterStop:stopped,
      retiredOwnerReopenRejected:oldRejected,replacementBaseline:fresh,staleInputRejected:true,
      sharedConsumers:2,noPhysicalOutputs:true,noExternalOutputs:true,simulatorQualified:false,latencyQualified:false};
    await writeFile(`${directory}/evidence.json`,JSON.stringify(evidence,null,2)+'\n');
  }finally{
    try{
      const active=(await request({command:'profile_status'})).profiles.profileSession;
      if(owner&&equalOwner(active.owner,owner))await request({command:'profile_stop'});
      if(profile)await request({command:'profile_delete',id:profile.id,expectedRevision:profile.revision});
    }finally{
      try{await page.evaluate("for(const key of ['stopFirst','stopSecond','stopOld','stopReplacement'])window.__profileSampleProbe?.[key]?.();delete window.__profileSampleProbe;");}finally{page.close();}
    }
  }
});
