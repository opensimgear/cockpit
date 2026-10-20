// Real native Cockpit, persisted panel library, UI Save, and a physical screen.
import test from 'node:test';
import assert from 'node:assert/strict';
import {createRequire} from 'node:module';
import {randomUUID} from 'node:crypto';
import {mkdir,writeFile} from 'node:fs/promises';
import {resolve} from 'node:path';

const require=createRequire(import.meta.url);
const {ws:RawSocket}=createRequire(require.resolve(process.env.OSG_PLAYWRIGHT_MODULE??'playwright'))('playwright-core/lib/utilsBundle');
const artifacts=resolve('.research/panel-save-active-profile-native');
const sleep=ms=>new Promise(resolve=>setTimeout(resolve,ms));
async function connectNative(){
  const targets=await(await fetch(`${process.env.OSG_NATIVE_CDP_URL??'http://127.0.0.1:9222'}/json/list`,{signal:AbortSignal.timeout(5000)})).json();
  const endpoint=targets.find(target=>target.type==='page'&&/127\.0\.0\.1:1420|localhost:1420/.test(target.url))?.webSocketDebuggerUrl;
  assert.ok(endpoint,'Start the real native dev Cockpit with CDP enabled.');
  const socket=new RawSocket(endpoint,{handshakeTimeout:8000});
  await new Promise((resolve,reject)=>{socket.once('open',resolve);socket.once('error',reject);});
  let sequence=0;const pending=new Map();
  socket.on('error',()=>{});
  socket.on('message',bytes=>{const message=JSON.parse(bytes.toString()),item=pending.get(message.id);if(!item)return;clearTimeout(item.timer);pending.delete(message.id);if(message.error)item.reject(new Error(message.error.message));else item.resolve(message.result);});
  socket.on('close',()=>{for(const item of pending.values()){clearTimeout(item.timer);item.reject(new Error('Native Cockpit disconnected.'));}pending.clear();});
  const send=(method,params={})=>new Promise((resolve,reject)=>{const id=++sequence,timer=setTimeout(()=>{pending.delete(id);reject(new Error(`${method} timed out.`));},120000);pending.set(id,{resolve,reject,timer});socket.send(JSON.stringify({id,method,params}));});
  return{send,async evaluate(expression){const result=await send('Runtime.evaluate',{expression,awaitPromise:true,returnByValue:true});if(result.exceptionDetails)throw new Error(result.exceptionDetails.exception?.description??result.exceptionDetails.text);return result.result.value;},close(){socket.close();}};
}
async function until(operation,label,timeout=20000){const end=Date.now()+timeout;let last;while(Date.now()<end){try{const value=await operation();if(value)return value;}catch(error){if(error.code==='ERR_ASSERTION')throw error;last=error;}await sleep(120);}throw new Error(`${label}${last?`: ${last.message}`:''}`);}

test('saved panels have one current choice and UI Save refreshes an active profile on the actual USB screen',{timeout:300000},async()=>{
  await mkdir(artifacts,{recursive:true});const native=await connectNative(),nonce=randomUUID().slice(0,8);
  const evidence={actualNativeCockpit:true,physicalCompanion:true,checks:[]};
  const companionId=process.env.OSG_PANEL_SAVE_COMPANION_ID??'40efcae8-07ab-4422-a69a-80d08029a700';
  const profileName=`Panel save profile ${nonce}`,newTitle=`Panel save updated ${nonce}`,newLabel=`SAVED ${nonce}`;
  let original,fixture,profileId;
  const request=async(command,payload={})=>{const request={command,clientId:'panel-save-active-profile-native',...(command.startsWith('companion_')?{companion:payload}:payload)};const reply=await native.evaluate(`window.__TAURI_INTERNALS__.invoke('gateway_request',{request:${JSON.stringify(request)}})`);assert.equal(reply.ok,true,reply.error??command);return reply.state;};
  const companions=async()=>(await request('companion_status')).companions;
  const profileStatus=async()=>(await request('profile_status')).profiles.profileSession;
  const saved=async()=>(await request('profile_assignments_get',{id:profileId})).profiles;
  const peer=value=>(value.usb?.devices??[value.usb]).find(row=>row?.device?.companionId===companionId)?.device;
  const instance=value=>value.host.instances.find(row=>row.companionId===companionId);
  const click=(label,scope='body')=>until(()=>native.evaluate(`(()=>{const root=document.querySelector(${JSON.stringify(scope)});const button=[...(root?.querySelectorAll('button')??[])].find(button=>(button.getAttribute('aria-label')??button.textContent.replace(/\\s+/g,' ').trim())===${JSON.stringify(label)});if(!button||button.disabled)return false;button.click();return true;})()`),`Missing UI action ${label}`);
  const screenshot=async name=>{const image=await native.send('Page.captureScreenshot',{format:'png'});await writeFile(resolve(artifacts,name),Buffer.from(image.data,'base64'));};
  const ready=async(hash,previousSession)=>until(async()=>{const state=await companions(),current=instance(state),device=peer(state);if(device?.operation?.publicationHash===hash)assert.notEqual(device.operation.phase,'failed',device.operation.message??device.operation.errorCode);if(current?.publicationHash===hash&&current.armed&&current.connected&&device?.publicationHash===hash&&device.operation?.phase==='ready'&&(!previousSession||device.sessionId!==previousSession)){assert.ok(Object.values(current.values??{}).every(value=>value===false),'A new panel must have a fresh neutral input baseline.');return{instance:current,device};}return null;},'Actual USB panel did not become ready',90000);
  try{
    const initial=await profileStatus();assert.equal(initial.owner,null,'Run this acceptance with no active profile.');assert.equal(initial.fault,null);
    const before=await companions();original=instance(before);assert.ok(original,'Assign a panel to the physical companion first.');
    assert.equal(peer(before)?.boardProfile,'sunton-3248s035r');
    assert.equal(peer(before)?.firmwareVersion,'2026.10.20.1');
    await ready(original.publicationHash);
    evidence.original={instanceId:original.instanceId,publicationHash:original.publicationHash};
    fixture=await native.evaluate(`(async()=>{
      const {CompanionLibraryStore}=await import('/src/lib/companion-library.ts');
      const {publishDocument}=await import('/packages/companion-designer/src/index.ts');
      const {sendPublication}=await import('/src/lib/companions.ts');
      const store=new CompanionLibraryStore(),snapshot=await store.load();
      const base=Object.values(snapshot.publications).flat().find(panel=>panel.publicationHash===${JSON.stringify(original.publicationHash)})??Object.values(snapshot.publications).flat().find(panel=>panel.document.presentation.widgets.some(widget=>widget.kind==='image'));
      if(!base||!base.document.presentation.widgets.some(widget=>widget.kind==='image'))throw new Error('The saved source panel needs an image so cache reuse can be verified.');
      const button=base.document.presentation.widgets.find(widget=>widget.kind==='button');if(!button)throw new Error('The source panel needs a real button so a visible screen change can be verified.');
      const document=structuredClone(base.document),draftId=crypto.randomUUID();document.ui.id='panel-save-${nonce}';document.ui.title='Panel save old ${nonce}';
      const old=await publishDocument(document,base.assets,{optimizeAssets:false});
      document.ui.title='Panel save current ${nonce}';const latest=await publishDocument(document,base.assets,{optimizeAssets:false});
      const send=async(command,payload)=>{const reply=await window.__TAURI_INTERNALS__.invoke('gateway_request',{request:{command,companion:payload,clientId:'panel-save-active-profile-native'}});if(!reply.ok)throw new Error(reply.error);return reply.state.companions;};
      await sendPublication(old,send);await sendPublication(latest,send);
      snapshot.library.drafts.push({id:draftId,name:document.ui.title,document,assets:base.assets});snapshot.publications[draftId]=[old,latest];await store.save(snapshot);
      return{draftId,panelId:document.ui.id,title:document.ui.title,oldHash:old.publicationHash,publicationHash:latest.publicationHash,button:{id:button.id,label:button.label,page:document.ui.pages.findIndex(page=>page.id===button.pageId)}};
    })()`);
    evidence.fixture=fixture;
    const inventory=(await request('profile_resources_list',{offset:0,limit:128})).profileResources;
    const source=inventory.resources.find(resource=>resource.resourceType==='companion'&&resource.attributes.companionId===companionId&&resource.attributes.componentKind==='paired-companion')??inventory.resources.find(resource=>resource.resourceType==='companion'&&resource.attributes.companionId===companionId);
    assert.ok(source,'The physical paired companion must be present in native profile inventory.');
    const created=(await request('profile_create',{name:profileName})).profiles.document;profileId=created.id;
    const document={...created,eligibility:{gameIds:[]},resources:[{id:'screen',name:'Acceptance physical screen',kind:'companion',required:true,ports:[]}],bindings:[],processingNodes:[],virtualOutputs:[],desktopOutputs:[],visibility:[],overrides:[],companionAssignments:[{resourceId:'screen',panelId:fixture.panelId,publicationHash:fixture.oldHash}],editor:{positions:[{nodeId:'screen',x:80,y:80}],viewport:{x:0,y:0,zoom:1}}};
    await request('profile_editor_save',{document,expectedRevision:created.revision,assignments:[{resourceId:'screen',sourceId:source.id,ports:[]}]});
    const normalized=(await saved()).document.companionAssignments.find(row=>row.resourceId==='screen');
    assert.deepEqual(normalized,{resourceId:'screen',panelId:fixture.panelId,publicationHash:fixture.publicationHash},'Saving a stable panel assignment must resolve its latest saved contents.');
    evidence.normalizedAssignment=normalized;
    // Reload the real UI so its persisted designer library includes the fixtures.
    await native.send('Page.reload');
    await until(()=>native.evaluate(`!!window.__TAURI_INTERNALS__&&!!document.querySelector('nav[aria-label="Main navigation"]')`),'Cockpit UI did not reload');
    await click('Profiles','nav[aria-label="Main navigation"]');await click('Refresh profiles','.profiles-workspace');await click(`Edit ${profileName}`,'.profiles-workspace');
    await until(()=>native.evaluate(`(()=>{const button=document.querySelector('[data-node-id="screen"] .block-heading');if(!button)return false;button.click();return true;})()`),'Profile companion block was not available');
    await click('Refresh panels','section[aria-label="Profile companion panel selection"]');
    const picker=await until(()=>native.evaluate(`(()=>{const root=document.querySelector('section[aria-label="Profile companion panel selection"]');const select=[...(root?.querySelectorAll('label')??[])].find(label=>label.firstChild?.textContent.trim()==='Panel')?.querySelector('select');if(!select||select.options.length<2)return null;return{options:[...select.options].map(option=>({value:option.value,text:option.textContent})),selected:select.value,text:root.innerText};})()`),'Latest saved panel picker did not load');
    assert.equal(picker.options.filter(option=>option.value===fixture.publicationHash).length,1);
    assert.equal(picker.options.filter(option=>option.value===fixture.oldHash).length,0);
    assert.equal(picker.selected,fixture.publicationHash,'The stable panel identity must select its latest saved contents.');
    assert.ok(picker.options.every(option=>!option.text.includes('sha256:')&&!/revision/i.test(option.text)));
    assert.ok(!picker.text.includes(fixture.oldHash)&&!picker.text.includes(fixture.publicationHash),'Transport hashes must remain outside visible editor text.');
    evidence.picker=picker;evidence.checks.push('One latest saved choice per panel, with stable identity and no revision/hash labels');
    await native.evaluate(`document.querySelector('section[aria-label="Profile companion panel selection"]').scrollIntoView({block:'center'})`);
    await screenshot('latest-panel-picker.png');
    await click('← Back to Profiles','.profiles-workspace');await click(`Activate ${profileName}`,'.profiles-workspace');
    await until(async()=>{const state=await profileStatus();assert.equal(state.fault,null,state.fault??'Profile activation failed.');return state.owner?.profileId===profileId&&state.state==='running'?state:null;},'Temporary panel-only profile did not activate',90000);
    const active=await ready(fixture.publicationHash);evidence.active={profileId,operation:active.device.operation,sessionId:active.device.sessionId};
    await click('Companions','nav[aria-label="Main navigation"]');
    await native.evaluate(`Array.from(document.querySelectorAll('button')).find(button=>button.textContent.trim()==='Back to Companions')?.click()`);
    await click(`Edit ${fixture.title}`,'.designer');await click('Edit panel name','section[aria-label="Panel editor"]');
    await native.evaluate(`(()=>{const input=document.querySelector('input[aria-label="Panel name"]');if(!input)throw new Error('Panel name field is missing');Object.getOwnPropertyDescriptor(HTMLInputElement.prototype,'value').set.call(input,${JSON.stringify(newTitle)});input.dispatchEvent(new Event('input',{bubbles:true}));input.dispatchEvent(new KeyboardEvent('keydown',{key:'Enter',bubbles:true}));})()`);
    if(fixture.button.page)await native.evaluate(`document.querySelectorAll('[aria-label="Panel pages"] .page-select')[${fixture.button.page}].click()`);
    await until(()=>native.evaluate(`(()=>{const button=[...document.querySelectorAll('button.edit-hit')].find(button=>button.getAttribute('aria-label')===${JSON.stringify(`Select or move ${fixture.button.label}`)});if(!button)return false;button.focus();button.dispatchEvent(new KeyboardEvent('keydown',{key:'Enter',bubbles:true}));return true;})()`),'Actual button widget could not be selected');
    await until(()=>native.evaluate(`(()=>{const label=[...document.querySelectorAll('aside[aria-label="Widget properties"] label')].find(label=>label.firstChild?.textContent.trim()==='Label'),input=label?.querySelector('input');if(!input)return false;Object.getOwnPropertyDescriptor(HTMLInputElement.prototype,'value').set.call(input,${JSON.stringify(newLabel)});input.dispatchEvent(new Event('change',{bubbles:true}));return true;})()`),'Button label property could not be edited');
    const savedAt=Date.now();await click('Save','section[aria-label="Panel editor"]');
    const changed=await until(async()=>{const result=await saved(),assignment=result.document.companionAssignments.find(row=>row.resourceId==='screen');return assignment?.panelId===fixture.panelId&&assignment.publicationHash!==fixture.publicationHash?{assignment,revision:result.document.revision}:null;},'Saving the panel did not update its saved profile assignment',90000);
    const updated=await ready(changed.assignment.publicationHash,active.device.sessionId),current=await profileStatus();
    assert.equal(current.owner?.profileId,profileId);assert.equal(current.state,'running');assert.equal(current.fault,null);
    assert.ok(updated.device.operation.cacheHits>0,'Changing only the panel title must reuse its image assets.');assert.equal(updated.device.operation.cacheMisses,0);
    assert.equal(updated.device.operation.completedBytes,updated.device.operation.totalBytes);
    assert.equal(updated.instance.profileOwned,true);
    const rendered=await native.evaluate(`(async()=>{const {CompanionLibraryStore}=await import('/src/lib/companion-library.ts');const snapshot=await new CompanionLibraryStore().load(),panel=snapshot.publications[${JSON.stringify(fixture.draftId)}].find(panel=>panel.publicationHash===${JSON.stringify(changed.assignment.publicationHash)});return{title:panel.document.ui.title,button:panel.document.presentation.widgets.find(widget=>widget.id===${JSON.stringify(fixture.button.id)}).label};})()`);
    assert.equal(rendered.title,newTitle);assert.equal(rendered.button,newLabel);
    evidence.updated={elapsedMs:Date.now()-savedAt,assignment:changed.assignment,operation:updated.device.operation,sessionId:updated.device.sessionId,profileRevision:changed.revision};
    evidence.updated.rendered=rendered;
    evidence.checks.push('Actual panel Save changes a visible button label, updates the active profile, reconnects the physical screen, establishes a fresh neutral baseline and reuses unchanged image assets');
    await screenshot('active-profile-panel-saved.png');
  }catch(error){evidence.error=error.message;await screenshot('failure.png').catch(()=>{});throw error;}
  finally{
    try{
      let current=await profileStatus().catch(()=>null);
      assert.ok(!current?.owner||current.owner.profileId===profileId,'Do not stop a profile started outside this acceptance.');
      if(current?.owner?.profileId===profileId)await request('profile_stop');
      current=await profileStatus().catch(()=>null);
      if(current&&['faulted','recovery-needed'].includes(current.state))await request('profile_recover');
      if(original){const restored=await ready(original.publicationHash);evidence.restored=restored.device.operation;}
      if(profileId){const current=await saved();await request('profile_delete',{id:profileId,expectedRevision:current.document.revision});}
      if(fixture){
        await native.evaluate(`(async()=>{const {CompanionLibraryStore}=await import('/src/lib/companion-library.ts');await new CompanionLibraryStore().deletePanel(${JSON.stringify(fixture.draftId)},[]);})()`);
        await request('companion_prune',{hashes:[],panelIds:[fixture.panelId]});
      }
      evidence.finalStatus=await profileStatus();assert.equal(evidence.finalStatus.owner,null);assert.equal(evidence.finalStatus.fault,null);
    }catch(error){evidence.cleanupError=error.message;throw error;}
    finally{await writeFile(resolve(artifacts,'evidence.json'),JSON.stringify(evidence,null,2)+'\n');native.close();}
  }
});
