// Parallel-wire collapse against the running native dev Cockpit. The real
// Feature Lab is copied through native IPC, and the copy is removed afterward.
import test from 'node:test';
import assert from 'node:assert/strict';
import { randomUUID } from 'node:crypto';
import { mkdir, writeFile } from 'node:fs/promises';
import { resolve } from 'node:path';

const sourceId = process.env.OSG_ROUTING_PROFILE_ID ?? 'f592a7b4-6f39-4267-a8f9-b46fbe4ff91a';
const artifacts = resolve('.research/profile-signal-overview-native', String(Date.now()));
const pause = milliseconds => new Promise(resolve => setTimeout(resolve, milliseconds));

async function until(operation, description, timeout = 20000) {
  const end = Date.now() + timeout; let last;
  while (Date.now() < end) {
    try { const result = await operation(); if (result) return result; } catch (error) { last = error; }
    await pause(100);
  }
  throw new Error(`${description}${last ? `: ${last.message}` : ''}`);
}

async function connectNative() {
  let endpoint = process.env.OSG_NATIVE_PAGE_WS;
  if (!endpoint) {
    const response = await fetch(`${process.env.OSG_NATIVE_CDP_URL ?? 'http://127.0.0.1:9227'}/json/list`, { signal: AbortSignal.timeout(5000) });
    endpoint = (await response.json()).find(target => target.type === 'page' && !target.url.includes('profile-activation-window') && /127\.0\.0\.1:1420|localhost:1420|tauri\.localhost/.test(target.url))?.webSocketDebuggerUrl;
  }
  assert.ok(endpoint, 'The running native dev Cockpit must expose its WebView debugging page.');
  const socket = new WebSocket(endpoint), pending = new Map(), errors = []; let sequence = 0;
  await new Promise((resolve, reject) => {
    const timer = setTimeout(() => reject(new Error('Native page connection timed out')), 8000);
    socket.addEventListener('open', () => { clearTimeout(timer); resolve(); }, { once: true });
    socket.addEventListener('error', () => { clearTimeout(timer); reject(new Error('Native page connection failed')); }, { once: true });
  });
  socket.addEventListener('message', event => {
    const reply = JSON.parse(event.data);
    if (reply.method === 'Runtime.exceptionThrown') errors.push(reply.params.exceptionDetails);
    const operation = pending.get(reply.id); if (!operation) return;
    clearTimeout(operation.timer); pending.delete(reply.id);
    reply.error ? operation.reject(new Error(reply.error.message)) : operation.resolve(reply.result);
  });
  socket.addEventListener('close', () => { for (const operation of pending.values()) { clearTimeout(operation.timer); operation.reject(new Error('Native page closed')); } pending.clear(); });
  const send = (method, params = {}) => new Promise((resolve, reject) => {
    const id = ++sequence, timer = setTimeout(() => { pending.delete(id); reject(new Error(`${method} timed out`)); }, 20000);
    pending.set(id, { resolve, reject, timer }); socket.send(JSON.stringify({ id, method, params }));
  });
  return {
    send, errors,
    async evaluate(fn, ...args) {
      const reply = await send('Runtime.evaluate', { expression: `(${fn.toString()})(...${JSON.stringify(args)})`, awaitPromise: true, returnByValue: true });
      if (reply.exceptionDetails) throw new Error(reply.exceptionDetails.exception?.description ?? reply.exceptionDetails.text);
      return reply.result.value;
    },
    close() { socket.close(); },
  };
}

test('native DCS overview hides transforms and counts, edits real signal lanes, and retains both styles and responsive dragging',{timeout:240000},async()=>{
  await mkdir(artifacts,{recursive:true});
  const native=await connectNative(),profileId=randomUUID(),name=`Signal overview ${profileId}`;
  const request=value=>native.evaluate(async value=>{const reply=await window.__TAURI_INTERNALS__.invoke('gateway_request',{request:{...value,clientId:'signal-overview-test'}});if(!reply.ok)throw Error(reply.error);return reply.state;},value);
  const click=(label,scope='body')=>until(()=>native.evaluate((label,scope)=>{const button=[...document.querySelector(scope).querySelectorAll('button')].find(button=>(button.getAttribute('aria-label')??button.textContent.trim())===label);if(!button||button.disabled)return false;button.click();return true;},label,scope),`Click ${label}`);
  const snapshot=()=>native.evaluate(()=>({view:document.querySelector('.canvas-stage')?.dataset.view,nodes:document.querySelectorAll('.resource-block').length,transforms:document.querySelectorAll('.resource-block.processing').length,badges:document.querySelectorAll('.connection-bundle-badge,.connection-trunk-badge').length,edges:[...document.querySelectorAll('.overview-edge')].map(path=>({id:path.dataset.bindingId,count:Number(path.dataset.routeCount),width:Number.parseFloat(getComputedStyle(path).strokeWidth),style:path.dataset.arrowStyle,d:path.getAttribute('d')})),lanes:[...document.querySelectorAll('.signal-lane')].map(lane=>lane.dataset.laneId),saveDisabled:[...document.querySelectorAll('.editor-save-actions button')].find(button=>button.textContent.trim()==='Save profile')?.disabled}));
  const capture=async label=>{const shot=await native.send('Page.captureScreenshot',{format:'png'});await writeFile(resolve(artifacts,label+'.png'),Buffer.from(shot.data,'base64'));};
  const open=async()=>{await click('Profiles','nav[aria-label="Main navigation"]');await until(()=>native.evaluate(name=>{if(document.querySelector('.profile-title-button')?.textContent.trim()===name&&document.querySelector('.canvas-stage'))return true;const edit=[...document.querySelectorAll('button')].find(button=>button.getAttribute('aria-label')===`Edit ${name}`);if(edit)edit.click();else [...document.querySelectorAll('button')].find(button=>button.textContent.trim()==='← Back to Profiles')?.click();return false;},name),'Open DCS clone');await click('Fit all');};
  let created=false;
  try{
    await native.send('Runtime.enable');
    const original=(await request({command:'profile_assignments_get',id:sourceId})).profiles;
    const document={...original.document,id:profileId,name,revision:1};
    await request({command:'profile_editor_save',document,assignments:original.assignments,expectedRevision:null});created=true;
    await open();
    const expected=await native.evaluate(async document=>{const {profileTabDocument}=await import('/src/lib/profile-vehicle-tabs.ts');const {signalOverview}=await import('/src/lib/profile-signal-overview.ts');const tab=profileTabDocument(document,'');return {nodes:tab.resources.length+tab.virtualOutputs.length+(tab.desktopOutputs?.length??0),bundles:signalOverview(tab),transforms:tab.processingNodes.length};},document);
    let state=await snapshot();assert.equal(state.view,'overview');assert.equal(state.nodes,expected.nodes);assert.equal(state.transforms,0);assert.equal(state.badges,0);assert.equal(state.edges.length,expected.bundles.length);assert.equal(state.saveDisabled,true);
    for(const edge of state.edges){const bundle=expected.bundles.find(bundle=>bundle.id===edge.id);assert.equal(edge.count,bundle.lanes.length);assert.doesNotMatch(edge.d,/NaN|Infinity/);}
    await click('Auto-layout');await until(()=>native.evaluate(()=>!document.querySelector('[aria-label="Auto-layout"]').disabled),'Overview ELK layout');await pause(250);
    await click('Save profile');await until(async()=>(await snapshot()).saveDisabled,'Saved overview layout');
    const laidOut=(await request({command:'profile_assignments_get',id:profileId})).profiles.document;
    assert.deepEqual(laidOut.bindings,document.bindings);assert.deepEqual(laidOut.processingNodes,document.processingNodes);assert.ok(!(laidOut.editor.wireRoutes??[]).some(route=>route.bindingId.startsWith('overview:')));
    await native.evaluate(()=>document.querySelector('.canvas-stage').scrollIntoView({block:'center'}));await capture('overview');
    const bundle=expected.bundles.find(bundle=>bundle.lanes.some(lane=>lane.transforms.some(id=>document.processingNodes.find(node=>node.id===id)?.operation.kind==='deadzone')));
    assert.ok(bundle,'DCS includes a deadzone stage');
    const openBundle=async()=>{await native.evaluate(id=>{const path=[...window.document.querySelectorAll('.signal-edge-hit')].find(path=>path.dataset.connectionId===id);path.dispatchEvent(new MouseEvent('click',{bubbles:true}));},bundle.id);await until(async()=>(await snapshot()).lanes.length===bundle.lanes.length,'Click opens signal lanes');};
    const arrowPoint=await native.evaluate(id=>{const path=[...document.querySelectorAll('.signal-edge-hit')].find(path=>path.dataset.connectionId===id);for(const fraction of [.5,.25,.75,.1,.9]){const p=path.getPointAtLength(path.getTotalLength()*fraction),m=path.getScreenCTM(),point=new DOMPoint(p.x,p.y).matrixTransform(m);if(document.elementFromPoint(point.x,point.y)===path)return{x:point.x,y:point.y};}return null;},bundle.id);
    assert.ok(arrowPoint,'Bundled arrow has a visible mouse hit target');
    await native.send('Input.dispatchMouseEvent',{type:'mousePressed',...arrowPoint,button:'left',buttons:1,clickCount:1});
    await native.send('Input.dispatchMouseEvent',{type:'mouseReleased',...arrowPoint,button:'left',buttons:0,clickCount:1});
    await until(async()=>(await snapshot()).lanes.length===bundle.lanes.length,'Real mouse click opens lanes');assert.equal((await snapshot()).saveDisabled,true);
    const transformId=bundle.lanes.flatMap(lane=>lane.transforms).find(id=>document.processingNodes.find(node=>node.id===id)?.operation.kind==='deadzone');
    await native.evaluate(id=>window.document.querySelector(`[data-transform-id="${id}"]`).click(),transformId);
    const factor=document.processingNodes.find(node=>node.id===transformId).operation.radius;
    await until(()=>native.evaluate(value=>{const input=document.querySelector('.lane-settings input[aria-label="Radius"]');if(!input)return false;input.value=String(value);input.dispatchEvent(new Event('change',{bubbles:true}));return true;},factor+1),'Edit deadzone inline');
    await until(async()=>!(await snapshot()).saveDisabled,'Editing dirties graph');assert.ok((await snapshot()).lanes.length,'Inspector stays open after edit');
    await click('Save profile');await until(async()=>(await snapshot()).saveDisabled,'Save deadzone');
    const edited=(await request({command:'profile_assignments_get',id:profileId})).profiles.document;
    assert.equal(edited.processingNodes.find(node=>node.id===transformId).operation.radius,factor+1);assert.deepEqual(edited.bindings,laidOut.bindings);
    await click('Undo');await until(()=>native.evaluate(()=>Number(document.querySelector('.lane-settings input[aria-label="Radius"]')?.value)===0||!!document.querySelector('.lane-settings')), 'Undo retains lane editor');
    assert.equal(await native.evaluate(()=>Number(document.querySelector('.lane-settings input[aria-label="Radius"]').value)),factor);
    await click('Redo');assert.equal(await native.evaluate(()=>Number(document.querySelector('.lane-settings input[aria-label="Radius"]').value)),factor+1);
    await native.evaluate(()=>document.querySelector('.signal-lanes').scrollIntoView({block:'center'}));await capture('signal-lanes');
    await click('Close signal lanes');await click('Toggle detailed graph');assert.equal((await snapshot()).transforms,expected.transforms);await click('Toggle detailed graph');assert.equal((await snapshot()).transforms,0);
    await openBundle();await native.evaluate(()=>document.querySelector('.signal-lane .insert-transform').click());await until(()=>native.evaluate(()=>!!document.querySelector('dialog[open]')),'Insert opens typed transform chooser');await click('Insert transformation','dialog[open]');await until(()=>native.evaluate(()=>!document.querySelector('dialog[open]')),'Insert completes');assert.equal((await snapshot()).transforms,0);await click('Undo');
    await openBundle();await native.evaluate(()=>[...document.querySelectorAll('.signal-lane button')].find(button=>button.textContent==='Disconnect').click());await pause(100);await click('Undo');await openBundle();assert.equal((await snapshot()).lanes.length,bundle.lanes.length);
    await click('Close signal lanes');
    for(const style of ['smooth','orthogonal']){
      await native.evaluate(async style=>{const {canvasStyling}=await import('/src/lib/canvas-styling.ts');canvasStyling.setArrowStyle(style);},style);await until(async()=>(await snapshot()).edges.every(edge=>edge.style===style),`Render ${style}`);
      await click('Fit all');await native.evaluate(()=>document.querySelector('.canvas-stage').scrollIntoView({block:'center'}));
      const position=()=>native.evaluate(()=>{const node=document.querySelector('.resource-block');return [node.style.left,node.style.top];});
      const beforePosition=await position();
      const point=await native.evaluate(()=>{const rect=document.querySelector('.resource-block .block-heading').getBoundingClientRect();return{x:rect.left+rect.width/2,y:rect.top+rect.height/2};});
      await native.send('Input.dispatchMouseEvent',{type:'mousePressed',...point,button:'left',buttons:1,clickCount:1});
      const latencies=[];
      for(let i=1;i<=20;i++){const start=performance.now();await native.send('Input.dispatchMouseEvent',{type:'mouseMoved',x:point.x+i*3,y:point.y+i,button:'left',buttons:1});latencies.push(performance.now()-start);}
      assert.equal(await native.evaluate(()=>document.querySelector('.canvas-stage').classList.contains('node-dragging')),true,'Pointer gesture actually moves a block');
      await native.send('Input.dispatchMouseEvent',{type:'mouseReleased',x:point.x+60,y:point.y+20,button:'left',buttons:0,clickCount:1});await pause(100);assert.notDeepEqual(await position(),beforePosition);await click('Undo');assert.deepEqual(await position(),beforePosition,'Undo restores the dragged block');
      const p95=latencies.sort((a,b)=>a-b)[Math.floor(latencies.length*.95)];assert.ok(p95<150,`${style} pointer latency ${p95}ms`);await capture(style);console.log(`${style} drag p95 ${p95.toFixed(1)}ms`);
    }
    assert.equal(native.errors.length,0,JSON.stringify(native.errors));
    console.log(JSON.stringify({artifacts,blocks:expected.nodes,bundles:expected.bundles.length,hiddenTransforms:expected.transforms}));
  }finally{
    if(created){const current=(await request({command:'profile_assignments_get',id:profileId})).profiles;await request({command:'profile_delete',id:profileId,expectedRevision:current.document.revision});}
    native.close();
  }
});

