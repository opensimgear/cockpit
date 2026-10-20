// Real running dev Cockpit and native Windows drivers. No transport interception,
// fake acknowledgements, navigation or reload. Every authored control is neutral.
import test from 'node:test';
import assert from 'node:assert/strict';
import {createRequire} from 'node:module';
import {randomUUID} from 'node:crypto';
import {mkdir,readFile,writeFile} from 'node:fs/promises';
import {dirname,resolve} from 'node:path';

const enabled=process.env.OSG_PROFILE_OUTPUTS_NATIVE==='1';
const artifacts=resolve('.research/profile-outputs-native');
const clone=value=>JSON.parse(JSON.stringify(value));
const delay=ms=>new Promise(resolve=>setTimeout(resolve,ms));
async function until(read,accept,label,timeout=20_000){
  const deadline=Date.now()+timeout;let value;
  do{value=await read();if(accept(value))return value;await delay(250);}while(Date.now()<deadline);
  const summary=value?.virtualDevices?{virtualDevices:value.virtualDevices.map(conciseSlot),physical:value.hiddenDevices.map(conciseHidden),diagnostics:value.diagnostics}:value;
  assert.fail(`${label}: ${JSON.stringify(summary)}`);
}
const neutralNode=contract=>({id:'neutral',name:'Neutral acceptance control',operation:{kind:'constant',value:false},inputs:[],outputs:[{id:'out',name:'Released',direction:'output',contract:clone(contract)}]});
const conciseSession=value=>({state:value.state,profileId:value.owner?.profileId??null,generation:value.owner?.generation??null,fault:value.fault,degradation:value.degradation});
const conciseSlot=value=>({id:value.id,status:value.status,ownerPid:value.ownerPid??null,available:value.available});
const conciseHidden=value=>({controllerId:value.controllerId,hidden:value.hidden,configured:value.configured});

test('native profile acquires and restores vJoy and HidHide, then owns a neutral desktop endpoint',{skip:!enabled,timeout:180_000},async()=>{
  assert.equal(process.platform,'win32');
  const require=createRequire(import.meta.url);
  const {chromium}=require(process.env.OSG_PLAYWRIGHT_MODULE??'playwright');
  await mkdir(artifacts,{recursive:true});
  const browser=await until(async()=>{
    try{return await chromium.connectOverCDP(process.env.OSG_NATIVE_CDP_URL??'http://127.0.0.1:9222');}
    catch(error){if(!String(error).includes('ECONNREFUSED'))throw error;return null;}
  },value=>!!value,'running native Cockpit CDP after rebuild',30_000);
  const page=browser.contexts().flatMap(context=>context.pages()).find(page=>/127\.0\.0\.1:1420|localhost:1420|tauri\.localhost/.test(page.url()));
  assert.ok(page,'Start the native dev Cockpit with CDP enabled before running this acceptance.');
  const originalUrl=page.url(),commands=[],created=new Map(),evidence={observedAt:new Date().toISOString(),nativeSubmission:'Only neutral false constants; no key-down, mouse button, wheel or pointer action is authored.'};
  const configRoot=process.env.OSG_DCS_BIOS_CONFIG?dirname(resolve(process.env.OSG_DCS_BIOS_CONFIG)):resolve(process.env.APPDATA,'OpenSimGear/Cockpit');
  const native=async request=>{
    commands.push(request.command);
    const response=await page.evaluate(request=>{
      if(!window.__TAURI_INTERNALS__?.invoke)throw Error('The existing page is not native Cockpit.');
      return window.__TAURI_INTERNALS__.invoke('gateway_request',{request:{...request,clientId:'profile-outputs-native-check'}});
    },request);
    assert.equal(response.ok,true,`${request.command}: ${response.error}`);return response.state;
  };
  const status=async()=>(await native({command:'profile_status'})).profiles.profileSession;
  const inventory=async()=>(await native({command:'system_inventory'})).systemInventory;
  const save=async(document,assignments=[])=>{
    const result=(await native({command:'profile_editor_save',document,assignments,expectedRevision:null})).profiles;
    created.set(result.document.id,result.document.revision);assert.equal(result.saveApplication.state,'inactive');return result.document;
  };
  let selectedBefore,sourceId,intentBefore;
  try{
    const initial=(await native({command:'profile_status'})).profiles;selectedBefore=initial.session.selectedProfileId;
    evidence.initial=conciseSession(initial.profileSession);
    if(initial.profileSession.state==='recovery-needed')await native({command:'profile_recover'});
    await native({command:'profile_stop'});
    assert.equal((await status()).owner,null);
    const list=(await native({command:'profile_list'})).profiles.profiles;
    const base=list.find(profile=>/MH-6J.*hidden pedals/i.test(profile.name));
    assert.ok(base,'The existing MH-6J profile supplies the actual driver layout and pedals assignment.');
    const document=(await native({command:'profile_get',id:base.id})).profiles.document;
    const assignments=(await native({command:'profile_assignments_get',id:base.id})).profiles.assignments;
    const policy=document.visibility.find(value=>value.mode==='hidden-while-active');assert.ok(policy);
    const pedals=document.resources.find(value=>value.id===policy.resourceId);assert.ok(pedals);
    const assignment=assignments.find(value=>value.resourceId===pedals.id);assert.ok(assignment);
    let physical;
    for(let offset=0;offset<4096;offset+=32){
      const result=(await native({command:'profile_resources_list',offset,limit:32})).profileResources;
      physical=result.resources.find(value=>value.id===assignment.sourceId)??physical;
      if(physical||offset+32>=result.total)break;
    }
    assert.ok(physical);assert.equal(physical.availability,'online');
    sourceId=physical.connections.find(value=>value.scope==='host-controllers')?.key;assert.ok(sourceId);
    const visibility=(await native({command:'controller_visibility_status',sourceId})).controllers.visibility;
    assert.equal(visibility.qualified,true);assert.equal(visibility.selfAccess.ready,true);
    intentBefore=visibility.intent;
    await native({command:'controller_visibility_enable',enabled:true});
    const desktopPreferences=(await native({command:'profile_desktop_status'})).desktop;
    assert.equal(desktopPreferences.qualified,true);
    await native({command:'profile_desktop_configure',enabled:true,scope:desktopPreferences.scope});
    if(visibility.intent.mode!=='hide-all')await native({command:'controller_visibility_save',sourceId,intent:{mode:'hide-all'}});
    const before=await inventory();
    const hiddenBefore=before.hiddenDevices.find(value=>value.controllerId===sourceId||value.controllerIds?.includes(sourceId));assert.ok(hiddenBefore);
    const output=clone(document.virtualOutputs[0]);assert.ok(output);output.required=true;
    const buttonId=output.layout.buttons[0],button=output.ports.find(value=>value.id===buttonId);assert.ok(button);
    const virtualDocument={...clone(document),id:randomUUID(),revision:1,name:`Native output acceptance ${Date.now()}`,eligibility:{gameIds:[]},
      resources:[{...clone(pedals),required:true}],bindings:[{id:'neutral-vjoy',from:{nodeId:'neutral',portId:'out'},to:{nodeId:output.id,portId:buttonId}}],
      processingNodes:[neutralNode(button.contract)],desktopOutputs:[],virtualOutputs:[output],visibility:[clone(policy)],overrides:[],companionAssignments:[],editor:{positions:[],viewport:{x:0,y:0,zoom:1}}};
    const saved=await save(virtualDocument,[clone(assignment)]);
    const compiled=(await native({command:'profile_compile',document:saved,assignments:[clone(assignment)]})).profileCompilation;
    assert.equal(compiled.readiness,'ready',JSON.stringify(compiled.diagnostics));
    assert.equal(compiled.requirements.virtualDevices,true);assert.equal(compiled.requirements.visibility,true);
    await native({command:'profile_apply',id:saved.id,expectedRevision:saved.revision});
    const run=await until(status,value=>value.owner?.profileId===saved.id&&['running','degraded'].includes(value.state),'vJoy/HidHide active owner');
    const active=await until(inventory,value=>value.virtualDevices.some(slot=>slot.status==='busy'&&slot.ownerPid),'native vJoy driver ownership');
    const busy=active.virtualDevices.find(slot=>slot.status==='busy'&&slot.ownerPid);
    // HidHide exposes one exclusive control handle. The owning native worker
    // verifies every mutation by driver readback before committing its position.
    // Inspect that exact committed owner rather than opening a competing handle.
    const manifest=JSON.parse(await readFile(resolve(configRoot,'profile-session-v1.json'),'utf8'));
    assert.equal(manifest.active.owner.profileId,saved.id);
    const receipt=manifest.active.receipts.find(value=>value.kind==='controller-visibility');assert.ok(receipt);
    const visibilityJournalPath=resolve(configRoot,`game-visibility-${receipt.state.visibilityOwner}.json`);
    const record=JSON.parse(await readFile(visibilityJournalPath,'utf8'));
    const owningWorker=(await native({command:'controller_visibility_status',sourceId})).controllers.visibility.job;
    assert.equal(owningWorker.active,true);assert.equal(owningWorker.phase,'active');assert.equal(owningWorker.sessionId,record.id);
    assert.equal(record.clean,false);assert.equal(record.pending,null);assert.equal(record.restoring,false);
    assert.equal(record.position,['applications','inverse','active','hidden'].filter(field=>JSON.stringify(record.plan.before[field])!==JSON.stringify(record.plan.after[field])).length);
    assert.equal(record.plan.after.active,true);
    assert.ok(record.plan.selections.some(value=>value.instance.toLowerCase()===hiddenBefore.instanceId.toLowerCase()));
    assert.ok(record.plan.after.hidden.some(value=>value.toLowerCase()===hiddenBefore.instanceId.toLowerCase()));
    const hidden={controllerId:sourceId,hidden:true,configured:true,verifiedBy:'owning native worker driver readback',committedPosition:record.position};
    assert.equal(before.virtualDevices.find(slot=>slot.id===busy.id)?.status,'free');
    await native({command:'profile_stop'});
    const restored=await until(inventory,value=>value.virtualDevices.find(slot=>slot.id===busy.id)?.status==='free'&&value.hiddenDevices.some(value=>(value.controllerId===sourceId||value.controllerIds?.includes(sourceId))&&value.hidden===hiddenBefore.hidden&&value.configured===hiddenBefore.configured),'vJoy release and exact visibility restoration');
    const cleanRecord=JSON.parse(await readFile(visibilityJournalPath,'utf8'));assert.equal(cleanRecord.clean,true);assert.equal(cleanRecord.position,0);
    assert.equal((await status()).owner,null);
    evidence.controller={sourceProfileId:base.id,sourceId,requiredVirtual:true,requiredPhysical:true,preflight:compiled.readiness,run:conciseSession(run),
      before:{slot:conciseSlot(before.virtualDevices.find(slot=>slot.id===busy.id)),physical:conciseHidden(hiddenBefore)},
      active:{slot:conciseSlot(busy),physical:hidden},
      stopped:{slot:conciseSlot(restored.virtualDevices.find(slot=>slot.id===busy.id)),physical:conciseHidden(restored.hiddenDevices.find(value=>value.controllerId===sourceId||value.controllerIds?.includes(sourceId)))}};
    const preferences=(await native({command:'profile_desktop_status'})).desktop;assert.equal(preferences.qualified,true);
    await native({command:'profile_desktop_configure',enabled:true,scope:preferences.scope});
    const blank=JSON.parse(await readFile(new URL('../fixtures/profiles/blank.json',import.meta.url),'utf8'));
    const contract={delivery:'state',schema:{type:'boolean'},freshnessMs:1000,fallback:false};
    const desktopDocument={...blank,id:randomUUID(),name:`Native neutral desktop acceptance ${Date.now()}`,processingNodes:[neutralNode(contract)],
      desktopOutputs:[{id:'desktop',name:'Neutral keyboard output',required:true,scope:{kind:'desktop-wide'},ports:[{id:'hold',name:'Released key',direction:'input',contract}],actions:[{portId:'hold',action:{kind:'key-hold',key:44}}]}],
      bindings:[{id:'neutral-desktop',from:{nodeId:'neutral',portId:'out'},to:{nodeId:'desktop',portId:'hold'}}]};
    const desktopSaved=await save(desktopDocument);
    const desktopPreflight=(await native({command:'profile_compile',document:desktopSaved,assignments:[]})).profileCompilation;
    assert.equal(desktopPreflight.readiness,'ready',JSON.stringify(desktopPreflight.diagnostics));
    await native({command:'profile_apply',id:desktopSaved.id,expectedRevision:desktopSaved.revision});
    const desktopRun=await until(status,value=>value.owner?.profileId===desktopSaved.id&&['running','degraded'].includes(value.state),'desktop active owner');
    const desktopActive=await until(async()=>(await native({command:'profile_desktop_status'})).desktop,value=>value.runtime?.backend?.runs?.length===1,'native desktop backend acquisition');
    assert.equal(desktopActive.runtime.scope.kind,'desktop-wide');assert.equal(desktopActive.runtime.backend.qualified,true);
    assert.deepEqual(desktopActive.runtime.backend.held,[]);assert.deepEqual(desktopActive.runtime.backend.possibleHeld,[]);assert.equal(desktopActive.runtime.backend.uncertain,false);
    const inspected=(await native({command:'profile_runtime_inspect',owner:desktopRun.owner,nodeId:'neutral',offset:0,limit:32})).profileInspection;
    assert.ok(inspected.ports.some(port=>port.endpoint.portId==='out'&&port.sample.status==='known'&&port.sample.value.value===false));
    await native({command:'profile_stop'});
    const desktopStopped=(await native({command:'profile_desktop_status'})).desktop;
    assert.equal((await status()).owner,null);assert.equal(desktopStopped.runtime.state,'stopped');assert.equal(desktopStopped.runtime.backend.cleanupComplete,true);
    assert.deepEqual(desktopStopped.runtime.backend.held,[]);assert.deepEqual(desktopStopped.runtime.backend.possibleHeld,[]);
    evidence.desktop={run:conciseSession(desktopRun),active:{state:desktopActive.state,scope:desktopActive.runtime.scope,backend:desktopActive.runtime.backend},
      neutralSample:inspected.ports.find(port=>port.endpoint.portId==='out').sample,stopped:{state:desktopStopped.state,backend:desktopStopped.runtime.backend}};
    assert.equal(page.url(),originalUrl);
  }catch(error){evidence.error=error instanceof Error?error.message:String(error);throw error;}
  finally{
    const cleanup=[];
    try{
      await native({command:'profile_stop'});
      if((await status()).state==='recovery-needed')await native({command:'profile_recover'});
      if(sourceId&&intentBefore)await native({command:'controller_visibility_save',sourceId,intent:intentBefore});
      if(selectedBefore)await native({command:'profile_select',id:selectedBefore,selectionSource:'manual'});
      for(const[id,revision]of created){await native({command:'profile_delete',id,expectedRevision:revision});cleanup.push(id);}
      const finalStatus=await status(),desktop=(await native({command:'profile_desktop_status'})).desktop,visibility=(await native({command:'controller_visibility_status'})).controllers.visibility;
      evidence.cleanup={deletedProfiles:cleanup.length,session:conciseSession(finalStatus),desktopEnabled:desktop.enabled,hidingEnabled:visibility.enabled,urlUnchanged:page.url()===originalUrl};
    }catch(error){evidence.cleanupError=error instanceof Error?error.message:String(error);}
    evidence.commands=commands;await writeFile(resolve(artifacts,'evidence.json'),JSON.stringify(evidence,null,2)+'\n');
    await browser.close();
    assert.equal(evidence.cleanupError,undefined,evidence.cleanupError);
  }
});
