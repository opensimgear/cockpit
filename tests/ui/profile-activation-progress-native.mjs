// Real dev Cockpit, a native child window and a physical USB screen. No mocked IPC.
// Cleanup observation subscribes to the actual loaded gateway module; it never
// replaces transport or inserts artificial delays into native output cleanup.
import test from 'node:test';
import assert from 'node:assert/strict';
import {createRequire} from 'node:module';
import {randomUUID} from 'node:crypto';
import {mkdir,writeFile} from 'node:fs/promises';
import {resolve} from 'node:path';

const require=createRequire(import.meta.url);
const {ws:RawSocket}=createRequire(require.resolve(process.env.OSG_PLAYWRIGHT_MODULE??'playwright'))('playwright-core/lib/utilsBundle');
const artifacts=resolve('.research/profile-activation-progress-native');
const cdp=process.env.OSG_NATIVE_CDP_URL??'http://127.0.0.1:9222';
const sleep=ms=>new Promise(resolve=>setTimeout(resolve,ms));
const targets=async()=>(await fetch(`${cdp}/json/list`,{signal:AbortSignal.timeout(5000)})).json();
async function connectTarget(target){
  const socket=new RawSocket(target.webSocketDebuggerUrl,{handshakeTimeout:8000});
  await new Promise((resolve,reject)=>{socket.once('open',resolve);socket.once('error',reject);});
  let sequence=0;const pending=new Map();socket.on('error',()=>{});
  socket.on('message',bytes=>{const message=JSON.parse(bytes.toString()),item=pending.get(message.id);if(!item)return;clearTimeout(item.timer);pending.delete(message.id);if(message.error)item.reject(new Error(message.error.message));else item.resolve(message.result);});
  socket.on('close',()=>{for(const item of pending.values()){clearTimeout(item.timer);item.reject(new Error('Native Cockpit window disconnected.'));}pending.clear();});
  const send=(method,params={})=>new Promise((resolve,reject)=>{if(socket.readyState!==1){reject(new Error('Native Cockpit window is closed.'));return;}const id=++sequence,timer=setTimeout(()=>{pending.delete(id);reject(new Error(`${method} timed out.`));},method==='Page.captureScreenshot'?5000:120000);pending.set(id,{resolve,reject,timer});socket.send(JSON.stringify({id,method,params}));});
  return{target,send,async evaluate(expression){const result=await send('Runtime.evaluate',{expression,awaitPromise:true,returnByValue:true});if(result.exceptionDetails)throw new Error(result.exceptionDetails.exception?.description??result.exceptionDetails.text);return result.result.value;},close(){socket.close();}};
}
async function until(operation,label,timeout=20000){const end=Date.now()+timeout;let last;while(Date.now()<end){try{const value=await operation();if(value)return value;}catch(error){if(error.code==='ERR_ASSERTION')throw error;last=error;}await sleep(100);}throw new Error(`${label}${last?`: ${last.message}`:''}`);}

test('real native progress covers physical activation, exact cancellation, recovery and deactivation',{timeout:300000},async()=>{
  await mkdir(artifacts,{recursive:true});
  const mainTarget=(await targets()).find(target=>target.type==='page'&&/127\.0\.0\.1:1420|localhost:1420/.test(target.url)&&!target.url.includes('profile-activation-window'));
  assert.ok(mainTarget,'Start the real native dev Cockpit with CDP enabled.');
  const native=await connectTarget(mainTarget),nonce=randomUUID().slice(0,8),profileName=`Activation progress ${nonce}`;
  const companionId=process.env.OSG_ACTIVATION_COMPANION_ID??'40efcae8-07ab-4422-a69a-80d08029a700';
  const evidence={actualNativeCockpit:true,physicalCompanion:true,checks:[],activation:[]};
  const checkpoint=async stage=>{evidence.stage=stage;await writeFile(resolve(artifacts,'evidence.json'),JSON.stringify(evidence,null,2)+'\n');console.log(stage);};
  let original,fixture,profileId,probeId,child,originalMode,activationId;
  const request=async(command,payload={})=>{const value={command,clientId:'profile-activation-progress-native',...(command.startsWith('companion_')?{companion:payload}:payload)};const reply=await native.evaluate(`window.__TAURI_INTERNALS__.invoke('gateway_request',{request:${JSON.stringify(value)}})`);assert.equal(reply.ok,true,reply.error??command);return reply.state;};
  const profileStatus=async()=>(await request('profile_status')).profiles.profileSession;
  const companions=async()=>(await request('companion_status')).companions;
  const peer=state=>(state.usb?.devices??[state.usb]).find(row=>row?.device?.companionId===companionId)?.device;
  const instance=state=>state.host.instances.find(row=>row.companionId===companionId);
  const click=(label,scope='.profiles-workspace',surface=native)=>until(()=>surface.evaluate(`(()=>{const root=document.querySelector(${JSON.stringify(scope)}),button=[...(root?.querySelectorAll('button')??[])].find(button=>(button.getAttribute('aria-label')??button.textContent.replace(/\\s+/g,' ').trim())===${JSON.stringify(label)});if(!button||button.disabled)return false;button.click();return true;})()`),`Missing UI action ${label}`);
  const screenshot=async(name,surface=native)=>{const image=await surface.send('Page.captureScreenshot',{format:'png'});await writeFile(resolve(artifacts,name),Buffer.from(image.data,'base64'));};
  const hasTile=name=>native.evaluate(`!![...document.querySelectorAll('.profiles-workspace .profile-row')].find(button=>button.getAttribute('aria-label')===${JSON.stringify(`Edit ${name}`)})`);
  const ready=async hash=>until(async()=>{const state=await companions(),device=peer(state),current=instance(state);if(device?.operation?.publicationHash===hash)assert.notEqual(device.operation.phase,'failed',device.operation.message??device.operation.errorCode);return current?.publicationHash===hash&&current.connected&&current.armed&&device?.sessionId===current.sessionId&&device?.publicationHash===hash&&device.operation?.phase==='ready'?{instance:current,device}:null;},'Physical companion did not reach a fresh ready baseline',90000);
  const saved=async id=>(await request('profile_assignments_get',{id})).profiles;
  const deleteProfile=async id=>{const current=await saved(id);await request('profile_delete',{id,expectedRevision:current.document.revision});};
  const openProgress=async()=>{
    const target=await until(async()=>{const target=(await targets()).find(target=>target.type==='page'&&target.url.includes('profile-activation-window'));if(target)return target;const problem=await native.evaluate(`document.querySelector('.profiles-workspace .runtime-error-banner')?.innerText.trim()??''`);assert.ok(!problem,`Activation failed before its progress window opened: ${problem}`);return null;},'Native activation progress window did not open',15000);
    const surface=await connectTarget(target);
    await until(()=>surface.evaluate(`(async()=>{const state=await window.__TAURI_INTERNALS__.invoke('profile_activation_window_state');return state?.progress.profileName===${JSON.stringify(profileName)}&&!!document.querySelector('main[aria-label="Profile operation progress"] [role="status"]')?.textContent.trim();})()`),'Progress window did not load the target profile');
    return surface;
  };
  const cleanupProgress=async(label,phase)=>{
    await native.evaluate(`(async()=>{const loaded=performance.getEntriesByType('resource').map(entry=>entry.name).find(name=>new URL(name).pathname==='/src/lib/gateway.ts');if(!loaded)throw new Error('The running app did not load the actual gateway module.');const {subscribeProfileCleanup}=await import(loaded);window.__profileCleanupEvidence={events:[],unsubscribe:subscribeProfileCleanup(event=>window.__profileCleanupEvidence.events.push({at:Date.now(),...event}))};})()`);
    const observation={phase,snapshots:[]};evidence[`${phase}Observation`]=observation;let finished=false,surface,lastTarget;
    const collect=(async()=>{
      while(!finished){
        try{
          const target=(await targets()).find(target=>target.type==='page'&&target.url.includes('profile-activation-window'));
          if(target&&target.id!==lastTarget){surface?.close();surface=await connectTarget(target);lastTarget=target.id;}
          if(target&&surface){
            const snapshot=await surface.evaluate(`(async()=>{const state=await window.__TAURI_INTERNALS__.invoke('profile_activation_window_state'),main=document.querySelector('main'),button=main?.querySelector('button'),status=main?.querySelector('[role="status"]');return state?.progress.phase===${JSON.stringify(phase)}&&button&&status?{state,text:status.textContent.trim(),button:button.textContent.trim(),disabled:button.disabled,children:[...main.children].map(node=>node.tagName),headings:main.querySelectorAll('h1,h2,h3,.profile-name,.activation-heading,.progress-track,footer').length,width:innerWidth,height:innerHeight,overflow:document.documentElement.scrollWidth>innerWidth||document.documentElement.scrollHeight>innerHeight}:null;})()`);
            if(snapshot&&!observation.snapshots.some(row=>row.text===snapshot.text)){
              observation.snapshots.push(snapshot);
              if(observation.snapshots.length===1)await screenshot(`native-${phase}-progress.png`,surface).catch(()=>{});
            }
          }
        }catch(error){observation.lastObservationError=error.message;}
        await sleep(20);
      }
    })();
    try{
      await click(label);
      const call=await until(()=>native.evaluate(`window.__profileCleanupEvidence.events.find(event=>event.type==='start'&&event.phase===${JSON.stringify(phase)})??null`),`Actual ${phase} was not dispatched`);
      const operationId=call.activationId;assert.match(operationId,/^[0-9a-f-]{36}$/);observation.operationId=operationId;
      observation.terminal=await until(async()=>{const state=(await request('profile_activation_status',{activationId:operationId})).profileActivation;assert.equal(state.canCancel,false,'Output cleanup must remain non-cancellable.');return ['succeeded','failed'].includes(state.phase)?state:null;},`Native ${phase} did not finish`,90000);
      assert.equal(observation.terminal.phase,'succeeded',observation.terminal.error??observation.terminal.message);
      assert.equal(observation.terminal.outcome,'inactive');assert.equal(observation.terminal.origin,phase==='deactivating'?'deactivation':'recovery');
      assert.ok(observation.terminal.steps.length,'Native cleanup must retain its actual step history.');
      if(phase==='deactivating')assert.ok(observation.terminal.steps.some(step=>/companion|panel/i.test(step.message)),`Cleanup must report restoring the physical companion: ${JSON.stringify(observation.terminal.steps)}`);
      // Cleanup can finish before the OS child is ready. Its absence at that
      // instant is not proof of completion; first observe the actual window.
      await until(()=>observation.snapshots.length,`The native ${phase} window did not render live cleanup status`,15000);
      await until(async()=>!(await targets()).some(target=>target.type==='page'&&target.url.includes('profile-activation-window')),`${phase} progress window remained after cleanup`,15000);
      observation.events=await native.evaluate('window.__profileCleanupEvidence.events');
      assert.ok(observation.events.some(event=>event.type==='finish'&&event.activationId===operationId&&!event.error),`Actual ${phase} did not finish its exact cleanup request.`);
      assert.ok(observation.snapshots.length,`The native ${phase} window did not render live cleanup status.`);
      for(const snapshot of observation.snapshots){assert.equal(snapshot.state.progress.operationId,operationId);assert.equal(snapshot.state.progress.canCancel,false);assert.equal(snapshot.disabled,true);assert.equal(snapshot.button,'Cleaning up…');assert.deepEqual(snapshot.children,['P','BUTTON']);assert.equal(snapshot.headings,0);assert.equal(snapshot.overflow,false);assert.ok(snapshot.width>=580&&snapshot.width<=620);assert.ok(snapshot.height>=60&&snapshot.height<=90);assert.ok(snapshot.text);}
      return observation;
    }finally{
      finished=true;await collect;surface?.close();
      await native.evaluate(`(()=>{window.__profileCleanupEvidence?.unsubscribe();delete window.__profileCleanupEvidence;})()`).catch(()=>{});
    }
  };
  try{
    const initial=await profileStatus();assert.equal(initial.owner,null,'Run acceptance with no active profile.');assert.equal(initial.fault,null);
    const before=await companions();original=instance(before);assert.ok(original,'The physical companion must have its ordinary panel assigned first.');
    assert.ok(peer(before)?.operation,'This acceptance requires cache-aware companion firmware.');await ready(original.publicationHash);
    evidence.original={instanceId:original.instanceId,publicationHash:original.publicationHash};
    const policy=(await request('profile_automatic_status')).profiles.automatic;originalMode=policy.mode;
    if(originalMode!=='manual')await request('profile_selection_mode',{mode:'manual'});
    await click('Profiles','nav[aria-label="Main navigation"]');
    await until(()=>native.evaluate(`document.querySelectorAll('.profiles-workspace .profile-row').length>0`),'Saved profile list did not appear');
    const heading=await native.evaluate(`(()=>{const root=document.querySelector('.profiles-workspace'),buttons=[...root.querySelectorAll('.page-actions button')];return{actions:buttons.map(button=>({text:button.textContent.trim(),icon:!!button.querySelector('svg')})),count:root.querySelector('.list-tools')?.textContent.trim(),refresh:[...root.querySelectorAll('button')].some(button=>button.textContent.trim()==='Refresh profiles')};})()`);
    const index=heading.actions.findIndex(button=>button.text==='Import profile');assert.ok(index>=0);assert.equal(heading.actions[index+1]?.text,'Add profile');assert.equal(heading.actions[index].icon,true);assert.match(heading.count,/^\d+ profiles?$/);assert.equal(heading.refresh,false);evidence.heading=heading;
    const probeName=`Automatic refresh ${nonce}`,probe=(await request('profile_create',{name:probeName})).profiles.document;probeId=probe.id;
    const createdAt=Date.now();await until(()=>hasTile(probeName),'Externally created profile did not appear automatically',10000);evidence.autoCreateMs=Date.now()-createdAt;
    await deleteProfile(probeId);probeId=undefined;const deletedAt=Date.now();await until(async()=>!await hasTile(probeName),'Externally deleted profile did not disappear automatically',10000);evidence.autoDeleteMs=Date.now()-deletedAt;
    evidence.checks.push('The real list observes external create/delete automatically; Import with an icon is immediately left of Add; count contains profiles only');
    await screenshot('automatic-list.png');
    fixture=await native.evaluate(`(async()=>{
      const {CompanionLibraryStore}=await import('/src/lib/companion-library.ts');
      const {publishDocument,encodeBase64}=await import('/packages/companion-designer/src/index.ts');
      const {sendPublication}=await import('/src/lib/companions.ts');
      const library=await new CompanionLibraryStore().load(),base=Object.values(library.publications).flat().find(panel=>panel.document.ui.assets.length===1&&panel.document.presentation.widgets.some(widget=>widget.kind==='image'));
      if(!base)throw new Error('Save an image panel in the Cockpit library before running this acceptance.');
      const document=structuredClone(base.document);document.ui.id='activation-progress-${nonce}';document.ui.title='Activation screen ${nonce}';
      // A unique source digest guarantees a cold raster, even on a warm persistent cache.
      const canvas=new OffscreenCanvas(360,220),ctx=canvas.getContext('2d'),gradient=ctx.createLinearGradient(0,0,360,220);gradient.addColorStop(0,'#183840');gradient.addColorStop(1,'#679b73');ctx.fillStyle=gradient;ctx.fillRect(0,0,360,220);ctx.fillStyle='#fff';ctx.font='26px sans-serif';ctx.fillText('Activation ${nonce}',16,100);
      const blob=await canvas.convertToBlob({type:'image/png'}),bytes=new Uint8Array(await blob.arrayBuffer()),digest=[...new Uint8Array(await crypto.subtle.digest('SHA-256',bytes))].map(value=>value.toString(16).padStart(2,'0')).join('');
      const asset=document.ui.assets[0];asset.sha256=digest;asset.sizeBytes=bytes.length;asset.mime='image/png';
      const publication=await publishDocument(document,[{assetId:asset.id,base64:encodeBase64(bytes)}],{optimizeAssets:false});
      await sendPublication(publication,async(command,payload)=>{const reply=await window.__TAURI_INTERNALS__.invoke('gateway_request',{request:{command,companion:payload,clientId:'profile-activation-progress-native'}});if(!reply.ok)throw new Error(reply.error);return reply.state.companions;});
      return{panelId:document.ui.id,publicationHash:publication.publicationHash,assetDigest:digest};
    })()`);evidence.fixture=fixture;
    const inventory=(await request('profile_resources_list',{offset:0,limit:128})).profileResources,source=inventory.resources.find(resource=>resource.resourceType==='companion'&&resource.attributes.companionId===companionId&&resource.attributes.componentKind==='paired-companion');assert.ok(source,'Paired physical companion is absent from native inventory.');
    const created=(await request('profile_create',{name:profileName})).profiles.document;profileId=created.id;evidence.profileId=profileId;
    const document={...created,eligibility:{gameIds:[]},resources:[{id:'screen',name:'Physical activation screen',kind:'companion',required:true,ports:[]}],bindings:[],processingNodes:[],virtualOutputs:[],desktopOutputs:[],visibility:[],overrides:[],companionAssignments:[{resourceId:'screen',panelId:fixture.panelId,publicationHash:fixture.publicationHash}],editor:{positions:[{nodeId:'screen',x:80,y:80}],viewport:{x:0,y:0,zoom:1}}};
    await request('profile_editor_save',{document,expectedRevision:created.revision,assignments:[{resourceId:'screen',sourceId:source.id,ports:[]}]});
    await until(()=>hasTile(profileName),'Saved activation fixture did not appear automatically',10000);
    await ready(original.publicationHash);await checkpoint('Activating the cold physical panel');await click(`Activate ${profileName}`);child=await openProgress();
    const snapshot=await child.evaluate(`window.__TAURI_INTERNALS__.invoke('profile_activation_window_state')`);
    assert.ok(snapshot?.progress?.operationId);assert.equal(snapshot.progress.profileName,profileName);evidence.window=snapshot;
    const compact=await child.evaluate(`(()=>{const main=document.querySelector('main'),status=main.querySelector('[role="status"]'),button=main.querySelector('button'),text=status.getBoundingClientRect(),action=button.getBoundingClientRect();return{width:innerWidth,height:innerHeight,children:[...main.children].map(element=>element.tagName),buttons:main.querySelectorAll('button').length,label:button.textContent.trim(),headings:main.querySelectorAll('h1,h2,h3,.profile-name,.activation-heading,.progress-track,footer').length,overflow:document.documentElement.scrollWidth>innerWidth||document.documentElement.scrollHeight>innerHeight,status:{left:text.left,right:text.right,top:text.top,bottom:text.bottom},button:{left:action.left,right:action.right,top:action.top,bottom:action.bottom}};})()`);
    assert.ok(compact.width>=580&&compact.width<=620,`Expected compact 600px progress width; got ${compact.width}`);assert.ok(compact.height>=60&&compact.height<=90,`Expected compact 76px progress height; got ${compact.height}`);assert.deepEqual(compact.children,['P','BUTTON']);assert.equal(compact.buttons,1);assert.equal(compact.label,'Cancel');assert.equal(compact.headings,0);assert.equal(compact.overflow,false);assert.ok(compact.status.right<compact.button.left);assert.ok(compact.button.right<=compact.width&&compact.button.top>=0&&compact.button.bottom<=compact.height);assert.ok(compact.status.top>=0&&compact.status.bottom<=compact.height);evidence.compactWindow=compact;
    activationId=snapshot.progress.operationId;
    const pending=(await request('profile_activation_status',{activationId})).profileActivation;
    assert.equal(pending.activationId,activationId);assert.equal(pending.profileId,profileId);assert.ok(['preparing','activating'].includes(pending.phase));evidence.pending=pending;
    await until(async()=>{const progress=(await request('profile_activation_status',{activationId})).profileActivation,operation=progress.companions.find(target=>target.publicationHash===fixture.publicationHash)?.operation;if(!operation)return false;evidence.activation.push({at:Date.now(),phase:progress.phase,operation});assert.notEqual(operation.phase,'failed',operation.message??operation.errorCode);return operation.phase==='downloading'&&operation.cacheMisses>0;},'The cold physical panel did not report a download',60000);
    evidence.windowDownload=await until(()=>child.evaluate(`(()=>{const text=document.querySelector('main [role="status"]')?.innerText??'';return text.includes('Physical activation screen')&&/download.*(?:asset|image)/i.test(text)&&/(?:KiB|MiB|\\bbytes\\b)/.test(text)?text:null;})()`),'Native child did not display current physical asset-download progress',12000);
    await screenshot('native-progress-downloading.png',child);await checkpoint('Physical image downloading; navigating away from Profiles');
    await click('Overview','nav[aria-label="Main navigation"]');
    await until(()=>native.evaluate(`document.querySelector('nav[aria-label="Main navigation"] button.current')?.textContent.trim()==='Overview'&&!!document.querySelector('.overview-grid')&&!!document.querySelector('.profiles-workspace')?.closest('[hidden]')`),'Overview did not replace the visible Profiles page');
    evidence.navigationProgress=await until(()=>child.evaluate(`(async()=>{const state=await window.__TAURI_INTERNALS__.invoke('profile_activation_window_state'),message=document.querySelector('main [role="status"]')?.innerText??'';if(state?.progress.operationId!==${JSON.stringify(activationId)})return null;return message!==${JSON.stringify(evidence.windowDownload)}&&/download.*(?:asset|image)/i.test(message)&&/(?:KiB|MiB|\\bbytes\\b)/.test(message)?{operationId:state.progress.operationId,message}:null;})()`),'Native download progress stopped updating after leaving Profiles',6000);
    assert.equal(evidence.navigationProgress.operationId,activationId);
    await screenshot('native-progress-on-overview.png',child);await screenshot('overview-during-activation.png');
    evidence.checks.push('Leaving Profiles for Overview keeps the same native progress window alive and its physical download status continues to advance');
    await checkpoint('Progress continues on Overview; requesting cancellation');
    const cancelledAt=Date.now();await click('Cancel','main[aria-label="Profile operation progress"]',child);
    const cancelView=await child.evaluate(`({text:document.querySelector('main')?.innerText,disabled:document.querySelector('main button')?.disabled})`).catch(()=>null);evidence.cancelView=cancelView;
    if(cancelView){assert.match(cancelView.text,/Cancelling|cleanup/i);assert.equal(cancelView.disabled,true);}
    const terminal=await until(async()=>{const state=(await request('profile_activation_status',{activationId})).profileActivation;return state&&['cancelled','failed','succeeded'].includes(state.phase)?state:null;},'Cancelled activation did not reach a terminal native result',90000);evidence.cancelled={elapsedMs:Date.now()-cancelledAt,terminal};await checkpoint('Native cancellation reached a terminal result');
    assert.equal(terminal.phase,'cancelled','Cancellation while downloading must prevent publishing an active profile.');assert.equal(terminal.cancelRequested,true);assert.ok(['inactive','recovery-needed'].includes(terminal.outcome));
    await until(async()=>!(await targets()).some(target=>target.type==='page'&&target.url.includes('profile-activation-window')),'Progress window remained open after cleanup',20000);child.close();child=undefined;
    const afterCancel=await profileStatus();assert.equal(afterCancel.owner,null);evidence.cancelled.profileStatus=afterCancel;
    await click('Profiles','nav[aria-label="Main navigation"]');await until(()=>native.evaluate(`!!document.querySelector('.profiles-workspace')&&!document.querySelector('.profiles-workspace').closest('[hidden]')`),'Profiles did not reappear after cancellation');
    await screenshot('cancellation-result.png');await checkpoint('Cancellation outcome captured before recovery');
    if(['faulted','recovery-needed'].includes(afterCancel.state)){
      evidence.recoveryMessage=await until(()=>native.evaluate(`(()=>{const root=document.querySelector('.profiles-workspace'),button=[...root.querySelectorAll('button')].find(button=>button.textContent.trim()==='Recover');return button&&!button.disabled?root.querySelector('.runtime-error-banner')?.innerText.trim():null;})()`),'Profiles did not offer Recover after cancellation cleanup');
      assert.match(evidence.recoveryMessage,/cancel|cleanup|recover/i);assert.ok(!evidence.recoveryMessage.includes('newer native control'));
      evidence.recoveryProgress=await cleanupProgress('Recover','recovering');
    }
    const recovered=await until(async()=>{const state=await profileStatus();return state.owner===null&&state.fault===null&&state.state==='stopped'?state:null;},'Recovery did not release the cancelled profile cleanly');assert.equal(recovered.owner,null);assert.equal(recovered.fault,null);await ready(original.publicationHash);
    const visible=await native.evaluate(`document.querySelector('.profiles-workspace')?.innerText??''`);assert.ok(!visible.includes('Profile preparation was cancelled by a newer native control.'));
    evidence.checks.push('An actual native progress window reports the cold physical screen; Cancel is disabled during safe cleanup and ends the exact activation without false active ownership');
    // Retry through the actual list, then verify successful completion closes its window.
    await ready(original.publicationHash);await checkpoint('Retrying physical activation');await click(`Activate ${profileName}`);child=await openProgress();
    const retry=await child.evaluate(`window.__TAURI_INTERNALS__.invoke('profile_activation_window_state')`);assert.ok(retry?.progress?.operationId);assert.notEqual(retry.progress.operationId,activationId);activationId=retry.progress.operationId;evidence.retry=retry;
    await screenshot('native-progress-retry.png',child);
    await until(async()=>{const state=await profileStatus();assert.equal(state.fault,null,state.fault??'Profile activation failed.');return state.owner?.profileId===profileId&&state.state==='running';},'Retried physical-screen profile did not activate',90000);
    const active=await ready(fixture.publicationHash);evidence.active={operation:active.device.operation,sessionId:active.device.sessionId};
    assert.ok(Object.values(active.instance.values??{}).every(value=>value===false),'Successful activation must establish a fresh neutral input baseline.');
    await until(async()=>!(await targets()).some(target=>target.type==='page'&&target.url.includes('profile-activation-window')),'Success did not close the native progress window',15000);child.close();child=undefined;
    await screenshot('activation-success.png');evidence.deactivationProgress=await cleanupProgress(`Deactivate ${profileName}`,'deactivating');await until(async()=>(await profileStatus()).owner===null,'Deactivate did not release the profile');await ready(original.publicationHash);
    evidence.checks.push('An explicit retry activates the physical panel, closes native progress on success and Deactivate restores the original panel');
    assert.ok(evidence.recoveryProgress,'Cold-download cancellation must exercise the actual Recover progress path.');
    evidence.checks.push('Actual Deactivate and Recover each open the same compact native window, report recorded cleanup steps, disable cancellation, close the exact operation and restore the ordinary physical panel');
  }catch(error){evidence.error=error.message;await checkpoint(`Acceptance failed: ${error.message}`);await screenshot('failure.png').catch(()=>{});if(child)await screenshot('failure-window.png',child).catch(()=>{});throw error;}
  finally{
    try{
      await checkpoint('Cleaning up the acceptance fixtures');
      if(activationId){const current=(await request('profile_activation_status',{activationId})).profileActivation;if(['preparing','activating','cancelling'].includes(current.phase)){await request('profile_activation_cancel',{activationId});await until(async()=>{const state=(await request('profile_activation_status',{activationId})).profileActivation;return ['cancelled','failed','succeeded'].includes(state.phase);},'Fixture activation did not finish during cleanup',90000);}}
      let status=await profileStatus().catch(()=>null);assert.ok(!status?.owner||status.owner.profileId===profileId,'Do not stop a profile activated outside this acceptance.');
      if(status?.owner?.profileId===profileId||status?.state==='transitioning')await request('profile_stop');
      status=await profileStatus().catch(()=>null);if(status&&['faulted','recovery-needed'].includes(status.state))await request('profile_recover');
      if(original){const state=await companions(),current=instance(state);if(current?.publicationHash!==original.publicationHash)await request('companion_assign',{companionId,instanceId:original.instanceId,publicationHash:original.publicationHash});await ready(original.publicationHash);}
      if(probeId)await deleteProfile(probeId);if(profileId)await deleteProfile(profileId);
      if(fixture)await request('companion_prune',{hashes:[],panelIds:[fixture.panelId]});
      if(originalMode&&originalMode!=='manual')await request('profile_selection_mode',{mode:originalMode});
      evidence.finalStatus=await profileStatus();assert.equal(evidence.finalStatus.owner,null);assert.equal(evidence.finalStatus.fault,null);
    }catch(error){evidence.cleanupError=error.message;throw error;}
    finally{await writeFile(resolve(artifacts,'evidence.json'),JSON.stringify(evidence,null,2)+'\n');child?.close();native.close();}
  }
});
