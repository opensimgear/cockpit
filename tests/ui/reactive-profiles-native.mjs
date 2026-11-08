// Opt-in real native read-model acceptance; no profile is activated.
import test from 'node:test';
import assert from 'node:assert/strict';
import {mkdir,writeFile,readFile} from 'node:fs/promises';
import {createHash} from 'node:crypto';
import {browserPage,nativePage,until} from '../../scripts/reactive/cdp.mjs';

const browserUrl=process.env.OSG_REACTIVE_PROFILES_BROWSER_URL;
const transport=browserUrl?'browser-dev':'native';
const requestProfile=async(page,value)=>{
 const body=JSON.stringify({...value,clientId:'independent-profile-readmodel-acceptance'});
 const result=await page.evaluate(browserUrl
  ?`fetch('/api/gateway/',{method:'POST',headers:{'Content-Type':'application/json','x-osg-client':'1'},body:${JSON.stringify(body)}}).then(response=>response.json())`
  :`window.__TAURI_INTERNALS__.invoke('gateway_request',{request:${body}})`);
 assert.equal(result.ok,true,`${value.command}: ${result.error}`);return result.state.profiles;
};

test(transport+' profile streams update independent consumers and preserve dirty authoring',{
 skip:!browserUrl&&process.env.OSG_REACTIVE_PROFILES_NATIVE!=='1',timeout:90000,
},async()=>{
 const page=browserUrl?await browserPage(browserUrl):await nativePage(),calls=[];let id,revision,recording=false;
 const unlisten=page.listen(event=>{
  if(recording&&event.method==='Network.requestWillBeSent'){
   const request=event.params.request;
   if(/ipc\.localhost|ipc:\/\//.test(request.url)||new URL(request.url).pathname==='/api/gateway/'){
    try{const body=JSON.parse(request.postData);calls.push({url:request.url,command:(body.request??body).command,topic:body.topic});}catch{}
   }
  }
 });
 const request=value=>requestProfile(page,value);
 try{
  await page.call('Network.enable');
  await page.evaluate(`(async()=>{
   const {ProfileReadModels,observeProfileLibrary,observeProfileRuntime}=await import('/src/lib/profile-read-models/shared.ts');
   const {ProfileEditorStore}=await import('/src/lib/profile-editor.ts');
   const {gatewayRequest}=await import('/src/lib/gateway.ts');
   const state=window.__profileReadModelAcceptance={library:[],runtime:[],other:[],reads:[],saves:[]};
   const api={list:()=>{throw Error('Passive library must not query profile_list');},
    getAssignments:async id=>{state.reads.push(id);return(await gatewayRequest({command:'profile_assignments_get',id})).profiles;},
    editorSave:()=>{throw Error('Passive changes must not save');}};
   state.store=new ProfileEditorStore(api,{listAll:async()=>[]});
   state.stops=[observeProfileLibrary(value=>{state.library.push(value);state.store.observeLibrary(value);}),
    observeProfileRuntime(value=>{state.runtime.push(value);state.store.observeRuntime(value);}),
    new ProfileReadModels().observeLibrary(value=>state.other.push(value)),
    state.store.subscribeSaveNotifications(value=>state.saves.push(value))];
  })()`);
  await until(()=>page.evaluate(`(()=>{const s=window.__profileReadModelAcceptance;return [s.library,s.runtime,s.other].every(list=>list.at(-1)?.status==='ready')})()`));
  const baseline=await page.evaluate(`(()=>{const s=window.__profileReadModelAcceptance;return {library:s.library.at(-1),runtime:s.runtime.at(-1),other:s.other.at(-1)}})()`);
  assert.deepEqual(baseline.library.value,baseline.other.value);
  assert.equal(baseline.runtime.value.profileSession.owner,null,'This fixture requires an idle native session.');
  recording=true;
  const created=await request({command:'profile_create',name:'Stream acceptance '+Date.now()});
  id=created.document.id;
  revision=created.document.revision;
  await until(()=>page.evaluate(`(()=>{const s=window.__profileReadModelAcceptance;return [s.library,s.other].every(list=>list.at(-1)?.value?.profiles.some(row=>row.id===${JSON.stringify(id)}))})()`));
  await page.evaluate(`window.__profileReadModelAcceptance.store.open(${JSON.stringify(id)})`);
  const before=await page.evaluate(`window.__profileReadModelAcceptance.reads.length`);
  const renamed=await request({command:'profile_rename',id,name:'Stream clean refresh',expectedRevision:revision});revision=renamed.document.revision;
  await until(()=>page.evaluate(`window.__profileReadModelAcceptance.store.state.editor?.document.name==='Stream clean refresh'`));
  assert.equal(await page.evaluate(`window.__profileReadModelAcceptance.reads.length`),before+1,'One pinned read per changed clean row.');
  await page.evaluate(`(()=>{const s=window.__profileReadModelAcceptance;const doc=structuredClone(s.store.state.editor.document);doc.name='Unsaved local draft';s.store.update(doc);s.dirtyEpoch=s.store.editorEpoch;})()`);
  const dirtyReads=await page.evaluate(`window.__profileReadModelAcceptance.reads.length`);
  const saved=await request({command:'profile_rename',id,name:'Stream remote change',expectedRevision:revision});revision=saved.document.revision;
  await until(()=>page.evaluate(`(()=>{const s=window.__profileReadModelAcceptance;return [s.library,s.other].every(list=>list.at(-1)?.value?.profiles.find(row=>row.id===${JSON.stringify(id)})?.name==='Stream remote change')})()`));
  const dirty=await page.evaluate(`(()=>{const s=window.__profileReadModelAcceptance;return {name:s.store.state.editor.document.name,epoch:s.store.editorEpoch,dirtyEpoch:s.dirtyEpoch,reads:s.reads.length,saves:s.saves}})()`);
  assert.equal(dirty.name,'Unsaved local draft');assert.equal(dirty.epoch,dirty.dirtyEpoch);assert.equal(dirty.reads,dirtyReads);assert.deepEqual(dirty.saves,[]);
  // Cover two of the former 3s workspace refresh cycles.
  await new Promise(resolve=>setTimeout(resolve,7000));
  const forbidden=calls.filter(call=>['profile_list','profile_status','profile_automatic_status','profile_game_candidates'].includes(call.command));
  assert.deepEqual(forbidden,[],'Profile library, runtime and automatic surfaces must use observations while idle.');
  await request({command:'profile_delete',id,expectedRevision:revision});id=null;
  await until(()=>page.evaluate(`(()=>{const s=window.__profileReadModelAcceptance;return [s.library,s.other].every(list=>!list.at(-1)?.value?.profiles.some(row=>row.name==='Stream remote change'))})()`));
  await mkdir('.research/reactive',{recursive:true});
  const executablePath=browserUrl?process.env.OSG_GATEWAY_SERVICE:process.env.OSG_DESKTOP_NATIVE_APP??'src-tauri/target/debug/osg-cockpit.exe';
  const executableSha256=executablePath?createHash('sha256').update(await readFile(executablePath)).digest('hex'):null;
  await writeFile(`.research/reactive/profiles-${transport}.json`,JSON.stringify({at:new Date().toISOString(),transport,actualNativeGateway:true,
   executableSha256,baseline,dirty,calls,
   independentConsumers:true,noExternalOutputs:true,hardwareLatencyQualified:false},null,2)+'\n');
 }finally{
  recording=false;
  if(id)await request({command:'profile_delete',id,expectedRevision:revision}).catch(()=>{});
  await page.evaluate(`(()=>{const s=window.__profileReadModelAcceptance;s?.stops.forEach(stop=>stop());delete window.__profileReadModelAcceptance;})()`).catch(()=>{});
  unlisten();await page.close();
 }
});

test(transport+' saves and reopens a real profile editor after background changes',{
 skip:!browserUrl&&process.env.OSG_REACTIVE_PROFILES_NATIVE!=='1',timeout:90000,
},async()=>{
 const page=browserUrl?await browserPage(browserUrl):await nativePage();
 const name='Editor reopen '+Date.now();
 const gateway=browserUrl?'/api/gateway/':'/gateway_request';
 const calls=[];
 let id,activationRequested=false;
 const unlisten=page.listen(({method,params})=>{
  if(method!=='Network.requestWillBeSent'||!params.request.postData
    ||new URL(params.request.url).pathname!==gateway)return;
  const body=JSON.parse(params.request.postData);
  calls.push((body.request??body).command);
 });
 const click=(selector,label)=>until(()=>page.evaluate(`(()=>{
  const button=[...document.querySelectorAll(${JSON.stringify(selector)})].find(node=>node.checkVisibility()&&!node.disabled
   &&[node.title,node.getAttribute('aria-label'),node.textContent.trim()].includes(${JSON.stringify(label)}));
  if(!button)return false;
  button.click();return true;
 })()`));
 const navigate=label=>click('nav[aria-label="Main navigation"] button',label);
 const editorName=()=>page.evaluate(`document.querySelector('.profiles-workspace button[aria-label="Edit profile name"]')?.textContent.trim()`);
 const renameInEditor=async value=>{
  const editing=await page.evaluate(`!!document.querySelector('input[aria-label="Profile name"]')`);
  if(!editing)await click('.profiles-workspace button','Edit profile name');
  await until(()=>page.evaluate(`(()=>{
   const input=document.querySelector('input[aria-label="Profile name"]');
   if(!input||input.disabled)return false;
   input.value=${JSON.stringify(value)};
   input.dispatchEvent(new Event('input',{bubbles:true}));
   input.dispatchEvent(new KeyboardEvent('keydown',{key:'Enter',bubbles:true,cancelable:true}));
   return true;
  })()`));
  await until(editorName,current=>current===value);
 };
 const profileRow=value=>page.evaluate(`window.__editorReopen?.state.value?.profiles.find(row=>row.name===${JSON.stringify(value)})`);
 const currentOwner=()=>page.evaluate('window.__editorReopen?.runtime.value?.profileSession.owner');
 try{
  await page.call('Network.enable',{maxPostDataSize:65536});
  await page.evaluate(`(async()=>{
   const {observeProfileLibrary,observeProfileRuntime}=await import('/src/lib/profile-read-models/shared.ts');
   const probe=window.__editorReopen={};
   const stopLibrary=observeProfileLibrary(state=>probe.state=state);
   const stopRuntime=observeProfileRuntime(state=>probe.runtime=state);
   probe.stop=()=>{stopRuntime();stopLibrary();};
  })()`);
  await until(()=>page.evaluate("window.__editorReopen?.state.status==='ready'"));

  await navigate('Profiles');
  await click('.profiles-workspace button','Add');
  await renameInEditor(name);
  await page.evaluate(`document.querySelector('details[aria-label="Controllers"]').open=true`);
  await click('[aria-label="Component palette"] button','Add debug controller');
  await click('.profiles-workspace button','Save');
  const created=await until(()=>profileRow(name));
  id=created.id;

  const visible=name+' visible';
  const changed=await requestProfile(page,{command:'profile_rename',id,name:visible,expectedRevision:created.revision});
  await until(editorName,current=>current===visible);
  await navigate('Overview');

  const closed=name+' closed';
  const reopened=await requestProfile(page,{command:'profile_rename',id,name:closed,expectedRevision:changed.document.revision});
  await navigate('Profiles');
  const activationStarted=performance.now();
  await click('.profiles-workspace button',`Activate ${closed}`);
  activationRequested=true;
  const firstOwner=await until(currentOwner,owner=>owner?.profileId===id,5000);
  const activationMs=performance.now()-activationStarted;
  assert.ok(activationMs<5000,`Output-free activation took ${Math.round(activationMs)} ms`);
  await click('.profiles-workspace button',`Edit ${closed}`);
  await until(editorName,current=>current===closed);
  await until(()=>page.evaluate(`(()=>{
   const block=[...document.querySelectorAll('.resource-block .block-heading')]
    .find(button=>button.querySelector('strong')?.textContent==='Debug controller');
   if(!block?.checkVisibility())return false;
   block.click();return true;
  })()`));
  await until(()=>page.evaluate(`(()=>{
   const axis=document.querySelector('[aria-label="Debug controller settings"] select[aria-label="Axis 1 range"]');
   if(!axis||axis.disabled)return false;
   axis.value='unipolar';
   axis.dispatchEvent(new Event('change',{bubbles:true}));
   return true;
  })()`));
  const last=name+' saved again';
  await renameInEditor(last);
  await click('.profiles-workspace button','Save');
  const saved=await until(()=>profileRow(last));
  assert.equal(saved.id,id);
  assert.ok(saved.revision>reopened.document.revision);
  const applied=await until(currentOwner,owner=>owner?.profileId===id&&owner.profileRevision===saved.revision,30000);
  assert.notEqual(applied.generation,firstOwner.generation);
  const {document}=await requestProfile(page,{command:'profile_get',id});
  const debug=document.resources.find(resource=>resource.kind==='debug-controller');
  assert.equal(debug.ports.find(port=>port.id==='axis-1').contract.schema.min,0);
  assert.equal(calls.filter(command=>command==='profile_editor_save').length,2);
  assert.deepEqual(calls.filter(command=>['profile_list','profile_status','profile_automatic_status'].includes(command)),[]);
  console.log(JSON.stringify({transport,uiSaves:2,activationMs:Math.round(activationMs),visibleBackgroundRename:true,currentOnReopen:true,
   savedControllerRange:true,activeRevisionObserved:true,refreshRequests:0}));
 }catch(error){
  const diagnostics=await page.evaluate(`({
   runtime:window.__editorReopen?.runtime,
   messages:[...document.querySelectorAll('[role="alert"], .toast p, .runtime-notice')].map(node=>node.textContent.trim())
  })`).catch(()=>null);
  throw new Error(JSON.stringify({transport,id,calls,owner:await currentOwner().catch(()=>null),diagnostics}),{cause:error});
 }finally{
  try{
   if(activationRequested)await requestProfile(page,{command:'profile_stop'});
   if(id){
    const {document}=await requestProfile(page,{command:'profile_get',id});
    await requestProfile(page,{command:'profile_delete',id,expectedRevision:document.revision});
   }
  }finally{
   unlisten();
   await page.evaluate('window.__editorReopen?.stop()').catch(()=>{});
   await page.close();
  }
 }
});
