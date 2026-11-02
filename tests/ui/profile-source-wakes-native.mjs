// Opt-in passive physical input routing. No virtual or physical output is authored.
import test from 'node:test';
import assert from 'node:assert/strict';
import {mkdir,writeFile} from 'node:fs/promises';
import {nativePage,until} from '../../scripts/reactive/cdp.mjs';

test('physical input reader activates, survives same-profile replacement and stops without previews',{
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
   return {profile:saved,sourceId:controllerSourceId(resource),nodeId:'mirror'};
  })()`);
  profile=fixture.profile;
  assert.equal(profile.virtualOutputs.length,0);assert.equal((profile.desktopOutputs??[]).length,0);
  // No controller preview subscription is opened by this test.
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
  const active=await apply(null);
  const output=pages=>pages.flatMap(page=>page.ports).find(port=>port.endpoint.portId==='out');
  const initial=await until(inspect,pages=>output(pages)?.sample.status==='known');
  const axis=output(initial);
  assert.equal(axis.contract.schema.type,'integer');assert.ok(Number.isInteger(axis.sample.value.value));
  assert.ok(axis.sample.value.value>=axis.contract.schema.min&&axis.sample.value.value<=axis.contract.schema.max);
  await new Promise(resolve=>setTimeout(resolve,200));
  const held=await inspect();
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
   profile:profile.id,sourceId:fixture.sourceId,active,initial,held,replacement,replaced,stopped,lateOwnerRejected:true,
   noExternalOutputs:true,noPreviewSubscription:true,rawInputLatencyQualified:false,virtualLatencyQualified:false},null,2)+'\n');
 }finally{
  try{
   const current=(await request({command:'profile_status'})).profiles.profileSession.owner;
   if(owner&&current?.session===owner.session&&current.generation===owner.generation)await request({command:'profile_stop'});
   if(profile)await request({command:'profile_delete',id:profile.id,expectedRevision:profile.revision});
  }finally{page.close();}
 }
});
