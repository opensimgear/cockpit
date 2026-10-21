// Actual native Cockpit plus a disposable production browser companion. Input
// baselines come from that browser; this test never fabricates provider frames,
// edits configuration files, restarts Cockpit, or qualifies a real simulator.
import test from 'node:test';
import assert from 'node:assert/strict';
import {createRequire} from 'node:module';
import {mkdir,writeFile} from 'node:fs/promises';
import {resolve} from 'node:path';
import {randomUUID} from 'node:crypto';

const artifacts=resolve('.research/profile-offline-peripherals-native');
const delay=ms=>new Promise(resolve=>setTimeout(resolve,ms));
const redact=value=>String(value).replace(/[A-Za-z0-9_-]{43}/g,'[redacted]');
async function until(read,label,timeout=30_000){const end=Date.now()+timeout;let last;while(Date.now()<end){try{const value=await read();if(value)return value;}catch(error){if(error.code==='ERR_ASSERTION')throw error;last=redact(error.message);}await delay(150);}throw Error(`${label}${last?`: ${last}`:''}`);}
async function connectNative(){
 const targets=await(await fetch(`${process.env.OSG_NATIVE_CDP_URL??'http://127.0.0.1:9222'}/json/list`,{signal:AbortSignal.timeout(5000)})).json();
 const target=targets.find(target=>target.type==='page'&&/127\.0\.0\.1:1420|localhost:1420|tauri\.localhost/.test(target.url)&&!target.url.includes('profile-activation-window'));
 assert.ok(target,'Start the actual native dev Cockpit with CDP enabled.');
 const socket=new WebSocket(target.webSocketDebuggerUrl),pending=new Map();let sequence=0;
 try{await new Promise((resolve,reject)=>{socket.addEventListener('open',resolve,{once:true});socket.addEventListener('error',reject,{once:true});});}catch(error){socket.close();throw error;}
 socket.addEventListener('message',event=>{const message=JSON.parse(event.data),entry=pending.get(message.id);if(!entry)return;clearTimeout(entry.timer);pending.delete(message.id);message.error?entry.reject(Error(redact(message.error.message))):entry.resolve(message.result);});
 socket.addEventListener('close',()=>{for(const entry of pending.values()){clearTimeout(entry.timer);entry.reject(Error('Native Cockpit window closed.'));}pending.clear();});
 const send=(method,params={})=>new Promise((resolve,reject)=>{if(socket.readyState!==1){reject(Error('Native Cockpit window is closed.'));return;}const id=++sequence,timer=setTimeout(()=>{pending.delete(id);reject(Error(`CDP ${method} timed out`));},20_000);pending.set(id,{resolve,reject,timer});socket.send(JSON.stringify({id,method,params}));});
 return{send,async evaluate(expression){const reply=await send('Runtime.evaluate',{expression,awaitPromise:true,returnByValue:true});if(reply.exceptionDetails)throw Error(redact(reply.exceptionDetails.exception?.description??reply.exceptionDetails.text));return reply.result.value;},close(){socket.close();}};
}
const port=(id,direction,contract)=>({id,name:id,direction,contract});
const identity=(id,contract)=>({id,name:id,operation:{kind:'identity'},inputs:[port('in','input',contract)],outputs:[port('out','output',contract)]});
const base=()=>({id:'base-marker',name:'Independent Base marker',operation:{kind:'constant',value:100},inputs:[],outputs:[port('out','output',{delivery:'state',schema:{type:'integer',min:0,max:10000,unit:null},freshnessMs:1000,fallback:null})]});
const instanceShape=instance=>instance?Object.fromEntries(['instanceId','sourceId','companionId','publicationHash','profileOwned','connected','armed','generation','sessionId'].map(key=>[key,instance[key]??null])):null;

test('offline required companions leave Base running and rebind automatically when the same real peer returns',{
 skip:process.env.OSG_OFFLINE_PERIPHERALS_NATIVE!=='1',timeout:240_000,
},async()=>{
 assert.equal(process.platform,'win32');await mkdir(artifacts,{recursive:true});
 const require=createRequire(import.meta.url),{chromium}=require(process.env.OSG_PLAYWRIGHT_MODULE??'playwright');
 const native=await connectNative(),checks=[],cleanupErrors=[],browserErrors=[],commands=[],instanceId=randomUUID();
 const evidence={actualNativeCockpit:true,productionBrowserCompanion:true,authoredProviderFrames:false,directConfigurationWrites:false,actualSimulatorQualified:false,checks,cleanupErrors,browserErrors};
 let browser,context,peerPage,companionId,profileId,resourceId,browserUrl,before,activationAttempted=false;
 const request=async value=>{commands.push(value.command);const reply=await native.evaluate(`window.__TAURI_INTERNALS__.invoke('gateway_request',{request:${JSON.stringify({...value,clientId:'profile-offline-peripherals-native'})}})`);assert.equal(reply.ok,true,`${value.command}: ${redact(reply.error??'Native request failed')}`);return reply.state;};
 const status=async()=>(await request({command:'profile_status'})).profiles;
 const companions=async()=>(await request({command:'companion_status'})).companions;
 const instance=state=>state.host.instances.find(instance=>instance.instanceId===instanceId);
 const resources=()=>until(async()=>{const all=[];let first;do{const page=(await request({command:'profile_resources_list',offset:all.length,limit:128})).profileResources;if(!first)first=page;else if(page.revision!==first.revision||page.total!==first.total)return false;all.push(...page.resources);}while(all.length<first.total);return all;},'Native resource inventory did not settle');
 const ports=async id=>{const all=[];let page;do{page=(await request({command:'profile_resources_inspect',resourceId:id,offset:all.length,limit:128})).profileResources;all.push(...page.ports);}while(all.length<page.total);return all;};
 const inspect=async owner=>(await request({command:'profile_runtime_inspect',owner,offset:0,limit:64})).profileInspection;
 const sample=(inspection,nodeId)=>inspection.ports.find(port=>port.endpoint.nodeId===nodeId&&port.endpoint.portId==='out');
 const known=(value,expected)=>value?.sample.status==='known'&&value.sample.value.kind==='scalar'&&(expected===undefined||value.sample.value.value===expected);
 const checkpoint=async stage=>{evidence.stage=stage;await writeFile(resolve(artifacts,'evidence.json'),JSON.stringify(evidence,null,2)+'\n');console.info(stage);};
 const capture=async name=>{const value=await native.send('Page.captureScreenshot',{format:'png',captureBeyondViewport:false});await writeFile(resolve(artifacts,name),Buffer.from(value.data,'base64'));};
 const openPeer=async url=>{peerPage=await context.newPage();peerPage.on('pageerror',error=>browserErrors.push(redact(error.message)));await peerPage.goto(url,{timeout:20_000});await peerPage.bringToFront();return peerPage;};
 const credentialId=()=>peerPage.evaluate(()=>{const raw=localStorage.getItem('osg-browser-credential-v2');return raw?JSON.parse(raw).companionId:null;});
 const cleanup=async(label,action)=>{try{await action();}catch(error){cleanupErrors.push(`${label}: ${redact(error.message??error)}`);}};
 try{
  const initial=await status(),initialCompanions=await companions();
  assert.equal(initial.profileSession.owner,null,'Run after the current profile is deactivated.');assert.equal(initial.profileSession.state,'stopped','Finish any existing recovery before acceptance.');
  assert.equal(initialCompanions.listener.enabled,true,'The existing companion listener must be enabled.');
  assert.equal(await native.evaluate('document.visibilityState'),'visible','Show the running Cockpit before testing its visible degraded status.');
  before={selectedProfileId:initial.session.selectedProfileId,selectionMode:initial.session.selectionMode,view:await native.evaluate(`document.querySelector('.sidebar button.current')?.textContent.trim()`),clients:initialCompanions.host.clients.map(client=>client.companionId).sort(),instances:initialCompanions.host.instances.map(instance=>instance.instanceId).sort()};
  await native.evaluate(`[...document.querySelectorAll('.sidebar button')].find(button=>button.textContent.trim()==='Overview').click()`);
  await request({command:'profile_selection_mode',mode:'manual'});
  const offered=(await request({command:'companion_pair_offer',companion:{kind:'browser'}})).companions;
  browser=await chromium.launch({headless:true,...(process.env.OSG_BROWSER_CHANNEL?{channel:process.env.OSG_BROWSER_CHANNEL}:{channel:'msedge'})});
  context=await browser.newContext({ignoreHTTPSErrors:true,viewport:{width:1280,height:900}});await openPeer(offered.pairing.browserUrl);
  companionId=await until(credentialId,'The production browser did not retain its approved paired identity');assert.ok(!before.clients.includes(companionId),'The disposable context must create a new identity.');
  browserUrl=new URL(peerPage.url());browserUrl.hash='';browserUrl.search='';browserUrl=browserUrl.href;
  const component=await until(async()=>(await resources()).find(resource=>resource.resourceType==='companion'&&resource.attributes.componentKind==='paired-companion'&&resource.attributes.companionId===companionId),'The real paired companion component is missing');resourceId=component.id;
  const catalogue=(await request({command:'profile_companion_catalogue'})).profileCompanionAuthoring;
  let panel,observed;
  for(const candidate of catalogue.panels.filter(panel=>panel.panelId&&panel.controlCount>0).sort((left,right)=>left.assetCount-right.assetCount)){
   let offset=0;
   do{const page=(await request({command:'profile_companion_inspect',resourceId,publicationHash:candidate.publicationHash,offset,limit:64})).profileCompanionAuthoring;observed=page.ports.find(port=>port.descriptor.direction==='output'&&port.descriptor.contract.delivery==='state'&&port.descriptor.contract.schema.type==='boolean');if(observed){panel=candidate;break;}offset+=page.ports.length;if(offset>=page.total||!page.ports.length)break;}while(true);
   if(panel)break;
  }
  assert.ok(panel&&observed,'An existing retained panel with a stable panel ID and boolean state control is required.');
  await request({command:'companion_assign',companion:{companionId,instanceId,publicationHash:panel.publicationHash}});
  const online=await until(async()=>{const state=await companions(),current=instance(state);return current?.connected&&current.armed&&current.publicationHash===panel.publicationHash&&(await resources()).find(resource=>resource.id===resourceId)?.availability==='online'?current:false;},'The actual browser did not provide its original panel baseline');
  observed=await until(async()=>(await ports(resourceId)).find(port=>port.current&&port.descriptor.id===observed.descriptor.id&&port.contractHash===observed.contractHash),'The paired source did not expose its exact live retained port contract');
  evidence.peer={companionId,instanceId,resourceId,panelId:panel.panelId,publicationHash:panel.publicationHash,observed,online:instanceShape(online)};
  await checkpoint('Production browser paired and exact online contracts captured');

  await peerPage.close();peerPage=undefined;
  await until(async()=>(await resources()).find(resource=>resource.id===resourceId)?.availability==='offline','Closing the page did not retain its companion as offline');
  const offlineBefore=instanceShape(instance(await companions()));assert.ok(offlineBefore);assert.equal(offlineBefore.connected,false);assert.equal(offlineBefore.profileOwned,false);
  const retained=(await ports(resourceId)).find(port=>port.descriptor.id===observed.descriptor.id&&port.contractHash===observed.contractHash);assert.ok(retained,'Disconnect lost the exact retained contract.');assert.equal(retained.current,false);
  const created=(await request({command:'profile_create',name:`Offline companion ${randomUUID()}`})).profiles.document;profileId=created.id;
  const document={...created,resources:[{id:'offline-peer',name:'Disposable offline companion',kind:'companion',required:true,ports:[{...observed.descriptor,id:'control',name:'Real peer control'}]}],processingNodes:[base(),identity('peer-value',observed.descriptor.contract)],bindings:[{id:'peer-control',from:{nodeId:'offline-peer',portId:'control'},to:{nodeId:'peer-value',portId:'in'}}],companionAssignments:[{resourceId:'offline-peer',panelId:panel.panelId,publicationHash:panel.publicationHash}]};
  const assignments=[{resourceId:'offline-peer',sourceId:resourceId,ports:[{profilePortId:'control',observedPortId:observed.descriptor.id,expectedContractHash:observed.contractHash}]}];
  const saved=(await request({command:'profile_editor_save',document,assignments,expectedRevision:created.revision})).profiles;
  const compiled=(await request({command:'profile_compile',document:saved.document,assignments})).profileCompilation;evidence.compilation=compiled;
  assert.equal(compiled.readiness,'ready',JSON.stringify(compiled.diagnostics));
  const compiledSources=(await request({command:'profile_compilation_inspect',compilationId:compiled.compilationId,section:'sources',offset:0,limit:32})).profileCompilation;evidence.compiledSources=compiledSources;
  const compiledPeer=compiledSources.items.find(source=>source.endpoint.nodeId==='offline-peer');assert.ok(compiledPeer);assert.equal(compiledPeer.readiness,'unavailable');assert.equal(compiledPeer.token,null);assert.equal(compiledPeer.route,null);
  activationAttempted=true;await request({command:'profile_apply',id:profileId,expectedRevision:saved.document.revision});
  const degraded=await until(async()=>{const state=(await status()).profileSession;assert.ok(!['faulted','recovery-needed'].includes(state.state),state.fault??state.state);return state.owner?.profileId===profileId&&state.state==='degraded'?state:false;},'The required offline companion blocked Base activation');
  assert.ok(degraded.degradation.some(message=>/offline-peer|Disposable offline companion/.test(message)&&/offline|unavailable|inactive/.test(message)),JSON.stringify(degraded.degradation));
  const offlineInspection=await until(async()=>{const inspection=await inspect(degraded.owner);return known(sample(inspection,'base-marker'),100)?inspection:false;},'Independent Base constant did not run while the peer was offline');
  assert.equal(known(sample(offlineInspection,'peer-value')),false,'An absent peer must not invent a known input baseline.');
  assert.deepEqual(instanceShape(instance(await companions())),offlineBefore,'Offline Apply must not acquire or change the retained native companion assignment.');
  evidence.offline={status:degraded,inspection:offlineInspection,assignment:offlineBefore};checks.push('A required offline companion activates Degraded with identified inactive controls while independent Base remains known at 100; no offline companion ownership is acquired');
  evidence.offline.visibleStatus=await until(()=>native.evaluate(`(()=>{const badge=document.querySelector('.profile-status'),warnings=document.querySelector('[aria-label="Unavailable profile branches"]');return badge?.textContent.includes('Degraded')&&warnings?.textContent.includes('Disposable offline companion')?{badge:badge.textContent,warnings:warnings.textContent}:false;})()`),'The visible Cockpit did not mark the profile Degraded and identify its offline peripheral');
  await capture('offline-base-active.png');await checkpoint('Required offline companion skipped; actual Base is running');

  await openPeer(browserUrl);assert.equal(await until(credentialId,'Saved browser pairing was not reused'),companionId);
  const recovered=await until(async()=>{const state=(await status()).profileSession;assert.ok(!['faulted','recovery-needed'].includes(state.state),state.fault??state.state);return state.owner?.profileId===profileId&&state.state==='running'&&state.owner.generation>degraded.owner.generation?state:false;},'Returning the same production peer did not rebind automatically',60_000);
  const recoveredInspection=await until(async()=>{const inspection=await inspect(recovered.owner);return known(sample(inspection,'base-marker'),100)&&known(sample(inspection,'peer-value'))?inspection:false;},'Automatic rebind did not acquire the real returning peer baseline');
  assert.equal(typeof sample(recoveredInspection,'peer-value').sample.value.value,'boolean');
  const reboundInstance=instance(await companions());assert.equal(reboundInstance.connected,true);assert.equal(reboundInstance.armed,true);assert.equal(reboundInstance.profileOwned,true);assert.notEqual(reboundInstance.sessionId,online.sessionId,'The restored profile must acquire a fresh native companion session.');
  evidence.recovered={status:recovered,inspection:recoveredInspection,assignment:instanceShape(reboundInstance)};
  evidence.recovered.visibleStatus=await until(()=>native.evaluate(`(()=>{const badge=document.querySelector('.profile-status');return badge?.textContent.includes('Offline companion')&&!badge.textContent.includes('Degraded')&&!document.querySelector('[aria-label="Unavailable profile branches"]')?badge.textContent:false;})()`),'The visible Cockpit did not clear its degraded warning after recovery');
  for(let index=0;index<12;index++){await delay(250);assert.deepEqual((await status()).profileSession.owner,recovered.owner,'Unchanged connected inventory must not repeatedly reapply the profile.');}
  checks.push('The same retained browser credential reconnects through production transport, automatically acquires a fresh owner and known real baseline, and remains Running without repeated ownership generations');
  await capture('same-peer-rebound.png');await checkpoint('Same paired browser returned and native profile rebind is stable');

  await request({command:'profile_stop'});await until(async()=>(await status()).profileSession.owner===null,'Stop did not release the active owner');
  await peerPage.close();peerPage=undefined;await until(async()=>(await resources()).find(resource=>resource.id===resourceId)?.availability==='offline','The stopped peer did not disconnect');
  await openPeer(browserUrl);assert.equal(await until(credentialId,'Pairing was lost after Stop'),companionId);
  await until(async()=>(await resources()).find(resource=>resource.id===resourceId)?.availability==='online','The stopped peer did not reconnect');
  for(let index=0;index<12;index++){await delay(250);assert.equal((await status()).profileSession.owner,null,'A later peer reconnect must not revive a stopped profile.');}
  checks.push('Stop fences later disconnect/reconnect activity and leaves no active profile owner');await checkpoint('Stop preserved while the same companion disconnected and returned');
  assert.deepEqual(browserErrors,[]);
 }catch(error){evidence.error=redact(error.message??error);await capture('failure.png').catch(()=>{});throw error;}
 finally{
  if(!companionId&&peerPage&&!peerPage.isClosed())companionId=await credentialId().catch(()=>null);
  if(before)await cleanup('Stop only the temporary profile',async()=>{const state=(await status()).profileSession;assert.ok(!state.owner||state.owner.profileId===profileId,'A different profile became active during acceptance.');if(state.owner?.profileId===profileId)await request({command:'profile_stop'});if(activationAttempted&&(await status()).profileSession.state==='recovery-needed')await request({command:'profile_recover'});});
  if(before?.selectedProfileId)await cleanup('Restore selected profile',()=>request({command:'profile_select',id:before.selectedProfileId,selectionSource:'manual'}));
  if(profileId)await cleanup('Delete temporary profile',async()=>{const current=(await request({command:'profile_get',id:profileId})).profiles.document;await request({command:'profile_delete',id:profileId,expectedRevision:current.revision});});
  if(context)await cleanup('Close disposable browser context',()=>context.close());
  if(browser)await cleanup('Close disposable browser',()=>browser.close());
  if(companionId)await cleanup('Revoke only the disposable companion',async()=>{assert.ok(before&&!before.clients.includes(companionId));await request({command:'companion_revoke',companion:{companionId}});});
  if(before)await cleanup('Restore selection policy and verify existing identities',async()=>{await request({command:'profile_selection_mode',mode:before.selectionMode});const final=await companions();assert.deepEqual(final.host.clients.map(client=>client.companionId).sort(),before.clients);assert.deepEqual(final.host.instances.map(instance=>instance.instanceId).sort(),before.instances);assert.equal((await status()).profileSession.owner,null);});
  if(before?.view)await cleanup('Restore navigation',()=>native.evaluate(`(()=>{const button=[...document.querySelectorAll('.sidebar button')].find(button=>button.textContent.trim()===${JSON.stringify(before.view)});if(!button)throw Error('Original navigation is missing');button.click();})()`));
  evidence.commands=commands;try{await writeFile(resolve(artifacts,'evidence.json'),JSON.stringify(evidence,null,2)+'\n');}finally{native.close();}
  assert.deepEqual(cleanupErrors,[]);
 }
});
