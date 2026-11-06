// Opt-in passive physical input routing. No virtual or physical output is authored.
import test from 'node:test';
import assert from 'node:assert/strict';
import {mkdir,writeFile} from 'node:fs/promises';
import {nativePage,until} from '../../scripts/reactive/cdp.mjs';

test('physical routing shares capture with preview and survives preview closure and profile replacement',{
 skip:process.env.OSG_PROFILE_SOURCE_WAKES_NATIVE!=='1',timeout:120000,
},async()=>{
 const page=await nativePage(),directory=`.research/reactive/profile-source-wakes-native/${Date.now()}`;
 await mkdir(directory,{recursive:true});
 let profile,owner;
 const request=async value=>{
  const response=await page.evaluate(`window.__TAURI_INTERNALS__.invoke('gateway_request',{request:${JSON.stringify({...value,clientId:'source-watch-native-acceptance'})}})`);
  assert.equal(response.ok,true,`${value.command}: ${response.error}`);return response.state;
 };
 try{
  assert.equal((await request({command:'profile_status'})).profiles.profileSession.owner,null);
  const fixture=await page.evaluate(`(async()=>{
   const {gatewayRequest}=await import('/src/lib/gateway.ts');
   const {blankProfile,addProfileResources}=await import('/src/lib/profile-editor.ts');
   const {controllerSourceId}=await import('/src/lib/controller-inputs.ts');
   const {ProfileInventoryClient}=await import('/src/lib/profile-inventory.ts');
   const resources=(await gatewayRequest({command:'profile_resources_list',offset:0,limit:32})).profileResources.resources;
   const resource=resources.filter(r=>r.resourceType==='host-controller'&&r.availability==='online').sort((a,b)=>b.portCount-a.portCount)[0];
   if(!resource)throw Error('This opt-in check requires an observed physical controller.');
   const draft=addProfileResources(blankProfile(crypto.randomUUID(),'Source wake acceptance '+Date.now()),[],[resource]);
   const details=await new ProfileInventoryClient(async request=>(await gatewayRequest(request)).profileResources).inspectAll(resource.id);
   const observed=details.ports.find(p=>p.current&&p.descriptor.direction==='output'&&p.descriptor.contract.schema.type==='integer');
   if(!observed)throw Error('Physical controller has no current numeric input port.');
   const input=draft.document.resources[0],port=structuredClone(observed.descriptor);input.ports=[port];
   draft.assignments.find(a=>a.resourceId===input.id).ports=[{profilePortId:port.id,observedPortId:observed.descriptor.id,expectedContractHash:observed.contractHash}];
   draft.document.processingNodes=[{id:'mirror',name:'Observed input',operation:{kind:'identity'},
    inputs:[{id:'in',name:'Input',direction:'input',contract:port.contract}],outputs:[{id:'out',name:'Output',direction:'output',contract:port.contract}]}];
   draft.document.bindings=[{id:'physical-mirror',from:{nodeId:input.id,portId:port.id},to:{nodeId:'mirror',portId:'in'}}];
   const saved=(await gatewayRequest({command:'profile_editor_save',...draft,expectedRevision:null})).profiles.document;
   return {profile:saved,sourceId:controllerSourceId(resource),controlId:observed.descriptor.id,inputId:input.id,nodeId:'mirror'};
  })()`);
  profile=fixture.profile;
  assert.equal(profile.virtualOutputs.length,0);assert.equal((profile.desktopOutputs??[]).length,0);
  const apply=async expected=>{
   const state=(await request({command:'profile_apply',id:profile.id,expectedRevision:profile.revision,expectedActiveOwner:expected})).profiles.profileSession;
   assert.equal(state.state,'running',JSON.stringify(state));owner=state.owner;return state;
  };
  const inspect=async()=>{
   const pages=[];
   for(let offset=0;offset<256;offset+=64){
    const result=(await request({command:'profile_runtime_inspect',owner,nodeId:fixture.nodeId,offset,limit:64})).profileInspection;
    pages.push(result);if(offset+64>=result.total)break;
   }
   return pages;
  };
  const previewState=()=>page.evaluate('window.__sourceWakePreview');
  const closePreview=()=>page.evaluate(`(()=>{window.__sourceWakePreviewStop?.();
    delete window.__sourceWakePreviewStop;})()`);
  await page.evaluate(`(async()=>{
    const {observeControllerInputs}=await import('/src/lib/controller-observations.ts');
    const sourceId=${JSON.stringify(fixture.sourceId)}, controlId=${JSON.stringify(fixture.controlId)};
    window.__sourceWakePreview={frames:[],error:null};
    window.__sourceWakePreviewStop=observeControllerInputs(sourceId,value=>{
      const state=window.__sourceWakePreview;
      if(value.status==='error')state.error=value.error;
      if(value.status!=='ready')return;
      const {source,controls,capturedUs}=value.detail;
      const control=controls.find(control=>control.capability.id===controlId);
      if(!control?.value)return;
      state.frames.push({generation:source.generation,sequence:source.sequence,capturedUs,native:control.value.native});
      if(state.frames.length>128)state.frames.shift();
    });
  })()`);
  const previewBefore=await until(previewState,state=>state.frames.length>0);
  assert.equal((await request({command:'profile_status'})).profiles.profileSession.owner,null,
    'Preview alone must leave the profile stopped.');
  const active=await apply(null);
  const output=pages=>pages.flatMap(page=>page.ports).find(port=>port.endpoint.portId==='out');
  const initial=await until(inspect,pages=>output(pages)?.sample.status==='known');
  const axis=output(initial);
  assert.equal(axis.contract.schema.type,'integer');assert.ok(Number.isInteger(axis.sample.value.value));
  assert.ok(axis.sample.value.value>=axis.contract.schema.min&&axis.sample.value.value<=axis.contract.schema.max);
  let latest;
  const shared=await until(async()=>{
    const result=(await request({command:'profile_runtime_inspect',owner,nodeId:fixture.inputId,offset:0,limit:64})).profileInspection;
    const sample=result.ports.find(port=>port.endpoint.portId===fixture.controlId);
    const preview=await previewState();
    assert.equal(preview.error,null);
    latest={sample,frames:preview.frames.slice(-3)};
    const frame=preview.frames.find(frame=>frame.capturedUs===sample?.observedUs
      && frame.native===sample?.sample.value?.value);
    return frame?{frame,sample}:null;
  }).catch(error=>{throw new Error(`No shared capture matched: ${JSON.stringify(latest)}`,{cause:error});});
  assert.equal(shared.frame.generation,previewBefore.frames[0].generation);
  await closePreview();
  const closedPreview=await previewState();
  const quietMs=axis.contract.freshnessMs+250;
  assert.ok(Number.isSafeInteger(quietMs)&&quietMs>250&&quietMs<=10000,'Bounded native freshness window required');
  await new Promise(resolve=>setTimeout(resolve,quietMs));
  const held=await inspect();
  assert.equal(output(held)?.sample.status,'known','Quiet native liveness must survive its scheduled freshness deadline');
  assert.deepEqual(await previewState(),closedPreview,'The closed preview must receive no more frames.');
  assert.equal((await request({command:'profile_status'})).profiles.profileSession.state,'running');
  const previous=owner;
  const replacement=await apply(previous);
  assert.notDeepEqual(owner,previous,'Replacement must acquire a new exact owner.');
  const replaced=await until(inspect,pages=>output(pages)?.sample.status==='known');
  await request({command:'profile_stop'});
  const stopped=(await request({command:'profile_status'})).profiles.profileSession;
  assert.equal(stopped.owner,null);assert.equal(stopped.state,'stopped');
  const late=await page.evaluate(`window.__TAURI_INTERNALS__.invoke('gateway_request',{request:${JSON.stringify({command:'profile_runtime_inspect',clientId:'source-watch-native-acceptance',owner,nodeId:fixture.nodeId,offset:0,limit:64})}})`);
  assert.equal(late.ok,false);
  await writeFile(`${directory}/evidence.json`,JSON.stringify({at:new Date().toISOString(),actualNativeCockpit:true,
   profile:profile.id,sourceId:fixture.sourceId,previewBefore,active,initial,shared,quietMs,held,replacement,replaced,stopped,lateOwnerRejected:true,
   noExternalOutputs:true,previewClosedBeforeReplacement:true,rawInputLatencyQualified:false,virtualLatencyQualified:false},null,2)+'\n');
 }finally{
  await page.evaluate(`(()=>{window.__sourceWakePreviewStop?.();delete window.__sourceWakePreviewStop;
    delete window.__sourceWakePreview;})()`).catch(()=>{});
  try{
   const current=(await request({command:'profile_status'})).profiles.profileSession.owner;
   if(owner&&current?.session===owner.session&&current.generation===owner.generation)await request({command:'profile_stop'});
   if(profile)await request({command:'profile_delete',id:profile.id,expectedRevision:profile.revision});
  }finally{page.close();}
 }
});
