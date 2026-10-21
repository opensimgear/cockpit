// Exercise the existing native dev Cockpit, its editor and persisted library.
import test from 'node:test';
import assert from 'node:assert/strict';
import {mkdir,writeFile} from 'node:fs/promises';
import {resolve} from 'node:path';
import {randomUUID} from 'node:crypto';

const artifacts=resolve('.research/profile-vehicles-native');
const delay=ms=>new Promise(resolve=>setTimeout(resolve,ms));
async function until(read,label,timeout=20000){const end=Date.now()+timeout;while(Date.now()<end){const value=await read();if(value)return value;await delay(120);}throw Error(label);}
async function connect(){
  const targets=await(await fetch(`${process.env.OSG_NATIVE_CDP_URL??'http://127.0.0.1:9222'}/json/list`)).json();
  const target=targets.find(target=>target.type==='page'&&/127\.0\.0\.1:1420|tauri\.localhost/.test(target.url)&&!target.url.includes('profile-activation-window'));assert.ok(target,'Start the native dev Cockpit with CDP enabled.');
  const socket=new WebSocket(target.webSocketDebuggerUrl),pending=new Map();let sequence=0;
  await new Promise((resolve,reject)=>{socket.addEventListener('open',resolve,{once:true});socket.addEventListener('error',reject,{once:true});});
  socket.addEventListener('message',event=>{const message=JSON.parse(event.data),entry=pending.get(message.id);if(!entry)return;clearTimeout(entry.timer);pending.delete(message.id);if(message.error)entry.reject(Error(message.error.message));else entry.resolve(message.result);});
  const send=(method,params={})=>new Promise((resolve,reject)=>{const id=++sequence,timer=setTimeout(()=>{pending.delete(id);reject(Error(`CDP ${method} timed out`));},20000);pending.set(id,{resolve,reject,timer});socket.send(JSON.stringify({id,method,params}));});
  const evaluate=async expression=>{const reply=await send('Runtime.evaluate',{expression,awaitPromise:true,returnByValue:true});if(reply.exceptionDetails)throw Error(reply.exceptionDetails.exception?.description??reply.exceptionDetails.text);return reply.result.value;};
  return{send,evaluate,close:()=>socket.close()};
}

test('native vehicle tabs isolate their canvases, save, reopen, delete and undo',{timeout:150000},async()=>{
  await mkdir(artifacts,{recursive:true});const native=await connect(),name=`Vehicle tabs ${randomUUID()}`,checks=[];let profileId;
  const request=async value=>{const reply=await native.evaluate(`window.__TAURI_INTERNALS__.invoke('gateway_request',{request:${JSON.stringify({...value,clientId:'profile-vehicles-native'})}})`);assert.equal(reply.ok,true,`${value.command}: ${reply.error}`);return reply.state;};
  const click=(label,scope='.profiles-workspace')=>until(()=>native.evaluate(`(()=>{const root=document.querySelector(${JSON.stringify(scope)}),button=[...(root?.querySelectorAll('button')??[])].find(button=>(button.getAttribute('aria-label')??button.textContent.trim())===${JSON.stringify(label)});if(!button||button.disabled)return false;button.click();return true;})()`),`Button unavailable: ${label}`);
  const fill=(label,value)=>native.evaluate(`(()=>{const input=document.querySelector('input[aria-label='+JSON.stringify(${JSON.stringify(label)})+']');if(!input)throw Error('Missing input');input.value=${JSON.stringify(value)};input.dispatchEvent(new Event('input',{bubbles:true}));})()`);
  const select=(label,value)=>native.evaluate(`(()=>{const input=document.querySelector('select[aria-label='+JSON.stringify(${JSON.stringify(label)})+']');if(!input)throw Error('Missing select');input.value=${JSON.stringify(value)};input.dispatchEvent(new Event('change',{bubbles:true}));})()`);
  const saved=async()=> (await request({command:'profile_assignments_get',id:profileId})).profiles;
  const save=async()=>{await click('Save profile');await until(()=>native.evaluate(`(()=>{const button=document.querySelector('.editor-save-actions .primary-button');return button?.disabled&&button.textContent.trim()==='Save profile';})()`),'Save did not complete');if(!profileId)profileId=await until(async()=>(await request({command:'profile_list'})).profiles.profiles.find(profile=>profile.name===name)?.id,'Profile was not persisted');return saved();};
  const nodes=()=>native.evaluate(`[...document.querySelectorAll('.canvas-stage [data-node-id]')].map(node=>node.dataset.nodeId)`);
  const tabs=()=>native.evaluate(`[...document.querySelectorAll('[aria-label="Profile vehicle tabs"] button[data-tab-index]')].map(tab=>({name:tab.getAttribute('aria-label')??tab.textContent.trim(),selected:tab.getAttribute('aria-pressed')}))`);
  async function activateVehicle(name){await click(name,'[aria-label="Profile vehicle tabs"]');await until(async()=>(await tabs()).some(tab=>tab.name===name&&tab.selected==='true'),`Vehicle canvas unavailable: ${name}`);}
  async function assertVehicleContent({draft=false}={}){
    assert.deepEqual(await native.evaluate(`(()=>{const bar=document.querySelector('[aria-label="Profile vehicle tabs"]'),content=document.getElementById('profile-vehicle-canvas'),settings=content?.firstElementChild,row=settings?.firstElementChild,select=content?.querySelector('select[aria-label="Tab vehicle"]'),graph=content?.querySelector('.profile-graph'),corners=element=>{const style=getComputedStyle(element);return[style.borderTopLeftRadius,style.borderTopRightRadius];};return{barSelectors:bar.querySelectorAll('select').length,contentSelectors:content.querySelectorAll('select[aria-label="Tab vehicle"]').length,firstRow:settings?.classList.contains('vehicle-settings')&&row?.classList.contains('vehicle-context')&&!!select&&row.contains(select),contentCorners:corners(content),graphCorners:graph?corners(graph):null};})()`),{barSelectors:0,contentSelectors:1,firstRow:true,contentCorners:['0px','0px'],graphCorners:draft?null:['0px','0px']});
  }
  async function addTab(id,name){await click('Add vehicle tab');await assertVehicleContent({draft:true});assert.equal(await native.evaluate(`document.activeElement===document.querySelector('#profile-vehicle-canvas select[aria-label="Tab vehicle"]')`),true,'Adding a vehicle focuses its content-row selector');await select('Tab vehicle','__custom');await fill('Exact vehicle ID',id);await fill('Vehicle name',name);await click('Use vehicle');await until(async()=>(await tabs()).some(tab=>tab.name===name&&tab.selected==='true'),'Vehicle tab was not selected');}
  async function back(){await click('← Back to Profiles');await until(()=>native.evaluate(`!!document.querySelector('[aria-label="Saved profiles"]')`),'Profile list unavailable');}
  async function compiledPreview(){
    const {document,assignments}=await saved(),index=await native.evaluate(`Number(document.querySelector('[aria-label="Profile vehicle tabs"] button[aria-pressed="true"]')?.dataset.tabIndex??0)`);
    const scoped=new Set(document.vehicleTabs.flatMap(tab=>tab.nodeIds)),selected=new Set(document.vehicleTabs[index-1]?.nodeIds??[]),included=id=>!scoped.has(id)||selected.has(id);
    for(const field of ['resources','processingNodes','virtualOutputs','desktopOutputs'])if(document[field])document[field]=document[field].filter(node=>included(node.id));
    document.bindings=document.bindings.filter(edge=>included(edge.from.nodeId)&&included(edge.to.nodeId));
    for(const field of ['visibility','overrides','companionAssignments'])document[field]=document[field].filter(row=>included(row.resourceId));
    document.editor.positions=document.editor.positions.filter(row=>included(row.nodeId));
    if(document.editor.collapsedPortGroups)document.editor.collapsedPortGroups=document.editor.collapsedPortGroups.filter(row=>included(row.nodeId));
    delete document.vehicleTabs;
    const resources=new Set(document.resources.map(resource=>resource.id)),preview=(await request({command:'profile_compile',document,assignments:assignments.filter(row=>resources.has(row.resourceId))})).profileCompilation;
    assert.ok(preview?.compilationId,'Native compiler did not return a real preview');
    assert.ok(!preview.diagnostics.some(diagnostic=>diagnostic.code==='invalidCapture'),JSON.stringify(preview.diagnostics));return preview;
  }
  async function conflictsHidden(){
    const preview=await compiledPreview();assert.ok(!preview.diagnostics.some(diagnostic=>diagnostic.code==='inputUnbound'));await delay(700); // Allow the editor's 350ms debounce to settle too.
    assert.equal(await native.evaluate(`!!document.querySelector('[aria-label="Vehicle configuration conflicts"]')`),false,'A clean canvas must not render the conflict section');
  }
  try{
    await native.evaluate(`(()=>{const nav=document.querySelector('nav[aria-label="Main navigation"]');[...nav.querySelectorAll('button')].find(button=>button.textContent.trim()==='Profiles').click();})()`);
    await delay(350);
    for(const label of ['Discard and continue','← Back to profile','← Back to Profiles','Discard and continue'])await native.evaluate(`(()=>{const button=[...document.querySelectorAll('button')].find(button=>(button.getAttribute('aria-label')??button.textContent.trim())===${JSON.stringify(label)});if(button&&!button.disabled)button.click();})()`);
    await click('Add profile');await click('Edit profile name');await fill('Profile name',name);await native.evaluate(`document.querySelector('[aria-label="Profile name"]').dispatchEvent(new KeyboardEvent('keydown',{key:'Enter',bubbles:true}))`);
    await select('Game context','dcs');await until(async()=>(await tabs()).map(tab=>tab.name).join()==='Base','Base tab missing');
    await click('Add virtual controller','.component-palette');await until(async()=>(await nodes()).length===2,'Base integration/controller missing');
    await addTab('FA-18C_hornet','F/A-18C');assert.equal((await nodes()).length,1);
    await click('Add virtual controller','.component-palette');await until(async()=>(await nodes()).length===2,'Vehicle controller missing');const hornetNodes=await nodes();
    await addTab('F-16C_50','F-16C');assert.equal((await nodes()).length,1);await click('Add virtual controller','.component-palette');await until(async()=>(await nodes()).length===2,'Second vehicle controller missing');const viperNodes=await nodes();
    assert.notDeepEqual(hornetNodes,viperNodes);let first=await save();assert.equal(first.document.vehicleTabs.length,2);assert.equal(first.document.virtualOutputs.length,3);assert.equal(first.document.resources.filter(resource=>resource.kind==='game').length,3);
    assert.deepEqual(first.document.vehicleTabs.map(tab=>tab.nodeIds),[hornetNodes,viperNodes]);checks.push('Base and two vehicle canvases own distinct nodes and persist in the native library.');
    assert.deepEqual(await native.evaluate(`(()=>{const group=document.querySelector('[aria-label="Profile vehicle tabs"]'),history=document.querySelector('.editor-save-actions [aria-label="Edit history"]');return{role:group.getAttribute('role'),selectors:group.querySelectorAll('select').length,vehicleButtons:group.querySelectorAll('button[data-vehicle-tab-id]').length,headerUndo:history?.querySelectorAll('[aria-label="Undo"]').length,headerRedo:history?.querySelectorAll('[aria-label="Redo"]').length,localHistory:document.querySelectorAll('.routing-workspace [aria-label="Edit history"]').length,nested:group.querySelectorAll('button select,button button').length,removesInside:[...group.querySelectorAll('button[data-vehicle-tab-id]')].every(button=>button.parentElement.classList.contains('vehicle-tab')&&!!button.parentElement.querySelector('.remove-vehicle-tab'))};})()`),{role:'group',selectors:0,vehicleButtons:2,headerUndo:1,headerRedo:1,localHistory:0,nested:0,removesInside:true});
    await activateVehicle('F/A-18C');assert.deepEqual(await nodes(),hornetNodes);
    await assertVehicleContent();
    const hornetTabId=first.document.vehicleTabs.find(tab=>tab.vehicleId==='FA-18C_hornet').id;
    assert.deepEqual(await native.evaluate(`(()=>{const select=document.querySelector('select[data-vehicle-tab-id='+JSON.stringify(${JSON.stringify(hornetTabId)})+']');return{own:[...select.options].some(option=>option.value==='FA-18C_hornet'),other:[...select.options].some(option=>option.value==='F-16C_50')};})()`),{own:true,other:false});
    // Real native End must reach the content selector's last option, without a
    // tab-navigation handler consuming it. The custom choice changes this tab.
    assert.equal(await native.evaluate(`(()=>{const select=document.querySelector('#profile-vehicle-canvas select[aria-label="Tab vehicle"]');select.focus();return document.activeElement===select;})()`),true);
    await native.send('Input.dispatchKeyEvent',{type:'keyDown',key:'End',code:'End',windowsVirtualKeyCode:35,nativeVirtualKeyCode:35});
    await native.send('Input.dispatchKeyEvent',{type:'keyUp',key:'End',code:'End',windowsVirtualKeyCode:35,nativeVirtualKeyCode:35});
    await until(()=>native.evaluate(`!!document.querySelector('.vehicle-custom [aria-label="Exact vehicle ID"]')`),'Native select End key was intercepted instead of opening the custom vehicle choice');
    await fill('Exact vehicle ID','A-10C_2');await fill('Vehicle name','A-10C test');await click('Use vehicle');
    await until(()=>native.evaluate(`document.querySelector('select[data-vehicle-tab-id='+JSON.stringify(${JSON.stringify(hornetTabId)})+']')?.value==='A-10C_2'&&!document.querySelector('.profiles-workspace').textContent.includes('Loading this aircraft’s controls and telemetry')`),'Changing the content vehicle selection did not settle');
    const changed=await save(),changedTab=changed.document.vehicleTabs.find(tab=>tab.id===hornetTabId);
    assert.equal(changedTab.vehicleId,'A-10C_2');assert.equal(changedTab.vehicleName,'A-10C test');assert.deepEqual(changedTab.nodeIds,hornetNodes);assert.deepEqual(await nodes(),hornetNodes);
    await select('Tab vehicle','__custom');await fill('Exact vehicle ID','FA-18C_hornet');await fill('Vehicle name','F/A-18C');await click('Use vehicle');
    await until(()=>native.evaluate(`document.querySelector('select[data-vehicle-tab-id='+JSON.stringify(${JSON.stringify(hornetTabId)})+']')?.value==='FA-18C_hornet'&&!document.querySelector('.profiles-workspace').textContent.includes('Loading this aircraft’s controls and telemetry')`),'Restoring the content vehicle choice did not settle');
    first=await save();assert.deepEqual(first.document.vehicleTabs.map(tab=>tab.nodeIds),[hornetNodes,viperNodes]);
    checks.push('Plain tab buttons activate canvases with their own sibling remove controls; the first content row owns the native vehicle selector, excludes duplicate vehicles, accepts native End, and saves exact retargeted vehicle IDs without moving canvas nodes. Content and canvas card top corners are square.');
    await click('Base','[aria-label="Profile vehicle tabs"]');assert.equal((await nodes()).length,2);
    await back();await click(`Edit ${name}`);await until(async()=>(await tabs()).length===3,'Reopened tabs missing');await activateVehicle('F/A-18C');assert.deepEqual(await nodes(),hornetNodes);checks.push('Reopening restores tab names and exact scoped canvases.');
    const cleanPreview=await compiledPreview();assert.ok(!cleanPreview.diagnostics.some(diagnostic=>diagnostic.code==='inputUnbound'));
    await conflictsHidden();checks.push('Native compiler preflight and the actual clean vehicle canvas leave no conflict section.');
    const screenshot=await native.send('Page.captureScreenshot',{format:'png',captureBeyondViewport:false});await writeFile(resolve(artifacts,'vehicle-tab.png'),Buffer.from(screenshot.data,'base64'));
    // A real unbound processing input needs no online controller, game or activation.
    // It belongs only to Hornet; Base and Viper must still remain clean.
    await back();const conflicted=JSON.parse(JSON.stringify(first.document)),contract={delivery:'state',schema:{type:'boolean'},freshnessMs:1000,fallback:false},conflictId='unbound-input';
    conflicted.processingNodes.push({id:conflictId,name:'Unbound processing input',operation:{kind:'identity'},inputs:[{id:'in',name:'Input',direction:'input',contract}],outputs:[{id:'out',name:'Output',direction:'output',contract}]});
    conflicted.vehicleTabs.find(tab=>tab.nodeIds.includes(hornetNodes[0])).nodeIds.push(conflictId);
    const conflictSaved=(await request({command:'profile_editor_save',document:conflicted,assignments:first.assignments,expectedRevision:conflicted.revision})).profiles.document;
    await click(`Edit ${name}`);await until(async()=>(await tabs()).length===3,'Conflict profile did not reopen');await activateVehicle('F/A-18C');
    const conflictPreview=await compiledPreview();assert.ok(conflictPreview.diagnostics.some(diagnostic=>diagnostic.code==='inputUnbound'));
    await until(()=>native.evaluate(`(()=>{const section=document.querySelector('[aria-label="Vehicle configuration conflicts"]');return section?.textContent.includes('Base + F/A-18C conflicts')&&section.textContent.includes('Connect every declared processing input');})()`),'The genuine native unbound-input conflict was not shown in profile details');
    const conflictScreenshot=await native.send('Page.captureScreenshot',{format:'png',captureBeyondViewport:false});await writeFile(resolve(artifacts,'vehicle-conflict.png'),Buffer.from(conflictScreenshot.data,'base64'));
    await click('Base','[aria-label="Profile vehicle tabs"]');await conflictsHidden();
    await activateVehicle('F-16C');await conflictsHidden();
    await back();const resolved=JSON.parse(JSON.stringify(first.document));resolved.revision=conflictSaved.revision;
    await request({command:'profile_editor_save',document:resolved,assignments:first.assignments,expectedRevision:resolved.revision});
    await click(`Edit ${name}`);await until(async()=>(await tabs()).length===3,'Resolved profile did not reopen');await activateVehicle('F/A-18C');
    const resolvedPreview=await compiledPreview();assert.ok(!resolvedPreview.diagnostics.some(diagnostic=>diagnostic.code==='inputUnbound'));await conflictsHidden();
    checks.push('A genuine native unbound processing input is visible only for its vehicle; Base and the other vehicle stay clear, and resolving the graph hides the section again.');
    await click('Remove F/A-18C tab');await until(async()=>(await tabs()).length===2,'Tab removal failed');
    assert.equal(await native.evaluate(`document.activeElement===document.querySelector('[aria-label="Profile vehicle tabs"] button[data-tab-index="0"]')`),true,'Removing the active vehicle must focus Base');
    assert.equal((await tabs()).find(tab=>tab.name==='Base').selected,'true');
    const removed=await save();assert.equal(removed.document.virtualOutputs.some(node=>hornetNodes.includes(node.id)),false);assert.equal(removed.document.resources.some(node=>hornetNodes.includes(node.id)),false);assert.equal(removed.assignments.some(row=>hornetNodes.includes(row.resourceId)),false);assert.equal(removed.document.vehicleTabs.length,1);assert.equal(removed.document.virtualOutputs.length,2);
    await click('Undo','.editor-save-actions [aria-label="Edit history"]');await until(async()=>(await tabs()).length===3,'Header Undo did not restore deleted tab');
    await click('Redo','.editor-save-actions [aria-label="Edit history"]');await until(async()=>(await tabs()).length===2,'Header Redo did not remove the vehicle again');
    await click('Undo','.editor-save-actions [aria-label="Edit history"]');await until(async()=>(await tabs()).length===3,'Header Undo did not restore the vehicle again');
    const restored=await save();assert.deepEqual(restored.document.vehicleTabs.map(tab=>tab.nodeIds),[hornetNodes,viperNodes]);checks.push('Removing the active tab focuses Base and deletes its graph; global header Undo/Redo restores and removes it atomically.');
    await select('Game context','');await until(async()=>(await tabs()).length===0,'Removing game context retained tabs');await click('Undo');await until(async()=>(await tabs()).length===3,'Undo did not restore game/vehicle context');
    await writeFile(resolve(artifacts,'evidence.json'),JSON.stringify({actualNativeCockpit:true,profileId,checks,conflictPreview,document:restored.document},null,2)+'\n');
  }catch(error){await writeFile(resolve(artifacts,'failure.txt'),String(error));throw error;}
  finally{
    try{profileId??=(await request({command:'profile_list'})).profiles.profiles.find(profile=>profile.name===name)?.id;
      if(profileId){const document=(await saved()).document;await request({command:'profile_delete',id:profileId,expectedRevision:document.revision});}
    }finally{native.close();}
  }
});
