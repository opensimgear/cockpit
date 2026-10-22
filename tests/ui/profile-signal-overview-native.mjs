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
  const open=async()=>{await click('Profiles','nav[aria-label="Main navigation"]');await until(()=>native.evaluate(name=>{const discard=[...document.querySelectorAll('dialog[open] button')].find(button=>button.textContent.trim()==='Discard and continue');if(discard){discard.click();return false;}if(document.querySelector('.profile-title-button')?.textContent.trim()===name&&document.querySelector('.canvas-stage'))return true;const edit=[...document.querySelectorAll('button')].find(button=>button.getAttribute('aria-label')===`Edit ${name}`);if(edit)edit.click();else [...document.querySelectorAll('button')].find(button=>button.textContent.trim()==='← Back to Profiles')?.click();return false;},name),'Open DCS clone');await click('Fit all');};
  let created=false;
  try{
    // A shared running cockpit can retain exceptions from earlier activity.
    // Clear that history before collecting errors produced by this acceptance run.
    await native.send('Runtime.discardConsoleEntries');
    await native.send('Runtime.enable');
    const original=(await request({command:'profile_assignments_get',id:sourceId})).profiles;
    const document={...original.document,id:profileId,name,revision:1};
    await request({command:'profile_editor_save',document,assignments:original.assignments,expectedRevision:null});created=true;
    await open();
    const expected=await native.evaluate(async document=>{const {profileTabDocument}=await import('/src/lib/profile-vehicle-tabs.ts');const {signalOverview}=await import('/src/lib/profile-signal-overview.ts');const tab=profileTabDocument(document,'');return {nodes:tab.resources.length+tab.virtualOutputs.length+(tab.desktopOutputs?.length??0),bundles:signalOverview(tab),transforms:tab.processingNodes.length};},document);
    let state=await snapshot();assert.equal(state.view,'overview');assert.equal(state.nodes,expected.nodes);assert.equal(state.transforms,0);assert.equal(state.badges,0);assert.equal(state.edges.length,expected.bundles.length);assert.equal(state.saveDisabled,true);
    for(const edge of state.edges){const bundle=expected.bundles.find(bundle=>bundle.id===edge.id);assert.equal(edge.count,bundle.lanes.length);assert.doesNotMatch(edge.d,/NaN|Infinity/);}
    const layoutBaseline=await native.evaluate(async bundles=>{
      const {layoutProfileWithElk}=await import('/src/lib/profile-elk-runtime.ts');
      const {canvasOverviewLayoutScore}=await import('/src/lib/profile-overview-layout.ts');
      const nodes=[...document.querySelectorAll('.resource-block')].map(node=>({id:node.dataset.nodeId,position:{x:parseFloat(node.style.left),y:parseFloat(node.style.top)},width:parseFloat(node.style.width),height:parseFloat(node.style.height)}));
      const bindings=bundles.map(bundle=>({...bundle.binding,to:{...bundle.binding.to,portId:bundle.id}}));
      const baseline=await layoutProfileWithElk(nodes,bindings,288,188);
      return {nodes,bindings,score:canvasOverviewLayoutScore(nodes,bindings,baseline.positions,288,188)};
    },expected.bundles);
    await click('Auto-layout');await until(()=>native.evaluate(()=>!document.querySelector('[aria-label="Auto-layout"]').disabled),'Overview ELK layout');await pause(250);
    if(!(await snapshot()).saveDisabled)await click('Save profile');await until(async()=>(await snapshot()).saveDisabled,'Saved overview layout');
    const laidOut=(await request({command:'profile_assignments_get',id:profileId})).profiles.document;
    await writeFile(resolve(artifacts,'layout-baseline.json'),JSON.stringify(layoutBaseline));
    const layoutScore=await native.evaluate(async baseline=>{
      const {canvasOverviewLayoutScore}=await import('/src/lib/profile-overview-layout.ts');
      const positions=[...document.querySelectorAll('.resource-block')].map(node=>({nodeId:node.dataset.nodeId,x:parseFloat(node.style.left),y:parseFloat(node.style.top)}));
      return canvasOverviewLayoutScore(baseline.nodes,baseline.bindings,positions,288,188);
    },layoutBaseline);
    assert.ok(layoutScore[0]<=layoutBaseline.score[0],`Overview crossings must not increase: ${layoutBaseline.score} -> ${layoutScore}`);
    console.log('Rendered overview layout quality',layoutBaseline.score,'->',layoutScore);

    assert.deepEqual(laidOut.bindings,document.bindings);assert.deepEqual(laidOut.processingNodes,document.processingNodes);assert.ok(!(laidOut.editor.wireRoutes??[]).some(route=>route.bindingId.startsWith('overview:')));
    await native.evaluate(()=>document.querySelector('.canvas-stage').scrollIntoView({block:'center'}));await capture('overview');
    const bundle=expected.bundles.find(bundle=>bundle.lanes.some(lane=>lane.transforms.some(id=>document.processingNodes.find(node=>node.id===id)?.operation.kind==='deadzone')));
    assert.ok(bundle,'DCS includes a deadzone stage');
    const openBundle=async()=>{await native.evaluate(id=>{const path=[...window.document.querySelectorAll('.signal-edge-hit')].find(path=>path.dataset.connectionId===id);if(path.getAttribute('aria-pressed')!=='true')path.dispatchEvent(new MouseEvent('click',{bubbles:true}));},bundle.id);await until(async()=>(await snapshot()).lanes.length===bundle.lanes.length,'Click opens signal lanes');};
    const viewport=()=>native.evaluate(()=>{const stage=document.querySelector('.canvas-stage'),rect=stage.getBoundingClientRect();return{width:rect.width,height:rect.height,top:rect.top,left:rect.left,transform:document.querySelector('.canvas-world').style.transform};});
    const beforePanel=await viewport();
    const arrowPoint=await native.evaluate(id=>{const path=[...document.querySelectorAll('.signal-edge-hit')].find(path=>path.dataset.connectionId===id);for(const fraction of [.5,.25,.75,.1,.9]){const p=path.getPointAtLength(path.getTotalLength()*fraction),m=path.getScreenCTM(),point=new DOMPoint(p.x,p.y).matrixTransform(m);if(document.elementFromPoint(point.x,point.y)===path)return{x:point.x,y:point.y};}return null;},bundle.id);
    assert.ok(arrowPoint,'Bundled arrow has a visible mouse hit target');
    await native.send('Input.dispatchMouseEvent',{type:'mousePressed',...arrowPoint,button:'left',buttons:1,clickCount:1});
    await native.send('Input.dispatchMouseEvent',{type:'mouseReleased',...arrowPoint,button:'left',buttons:0,clickCount:1});
    await until(async()=>(await snapshot()).lanes.length===bundle.lanes.length,'Real mouse click opens lanes');assert.equal((await snapshot()).saveDisabled,true);
    await pause(220);assert.deepEqual(await viewport(),beforePanel,'Opening the bottom overlay preserves canvas size, zoom, pan and scroll');
    const panel=await native.evaluate(()=>{const root=document.querySelector('.signal-lanes'),wrapper=root.parentElement,header=root.querySelector('header'),selected=document.querySelector('.overview-edge.selected'),other=[...document.querySelectorAll('.overview-edge')].find(path=>!path.classList.contains('selected'));return{position:getComputedStyle(wrapper).position,radius:getComputedStyle(root).borderRadius,buttons:header.querySelectorAll('button').length,source:header.querySelector('.source-device')?.textContent,destination:header.querySelector('.destination-device')?.textContent,arrow:!!header.querySelector('.connection-direction svg'),selected:getComputedStyle(selected).stroke,other:getComputedStyle(other).stroke,removed:[...document.querySelectorAll('button,summary')].filter(el=>/Detailed graph|Ports & branches|Advanced connections|Find block|^Transforms$/.test(el.textContent)).map(el=>el.textContent),zoom:document.querySelector('[aria-label="Canvas zoom"]')?.textContent,disconnect:[...root.querySelectorAll('.disconnect-route')].every(button=>button.querySelector('svg')&&!button.textContent.trim())};});
    assert.equal(panel.position,'absolute');assert.equal(panel.radius,'0px');assert.equal(panel.buttons,0);assert.ok(panel.source&&panel.destination&&panel.arrow);assert.notEqual(panel.selected,panel.other);assert.deepEqual(panel.removed,[]);assert.match(panel.zoom,/%/);assert.equal(panel.disconnect,true);
    const compact=await native.evaluate(()=>{const panel=document.querySelector('.signal-lanes');return{headerPlus:panel.querySelectorAll('.lane-head .insert-transform').length,rows:[...panel.querySelectorAll('.signal-lane')].map(row=>{const chain=row.querySelector('.lane-chain'),plus=chain.querySelector('.insert-transform'),last=chain.querySelector('.transform-chip:last-of-type'),p=plus.getBoundingClientRect();return{height:row.getBoundingClientRect().height,sources:row.querySelectorAll('[data-endpoint-side="from"]').length,near:p.left-chain.querySelector('.chain-track').getBoundingClientRect().right>=0&&p.left-chain.querySelector('.chain-track').getBoundingClientRect().right<=8,lastBeforePlus:!last||Math.min(last.getBoundingClientRect().right,chain.querySelector('.chain-track').getBoundingClientRect().right)<=p.left,routers:chain.querySelectorAll('.chain-router').length,blocks:chain.querySelectorAll('.transform-chip').length};}),muted:[...document.querySelectorAll('.resource-block:not(.route-related)')].every(node=>parseFloat(getComputedStyle(node).opacity)<=.2),mutedRoutes:[...document.querySelectorAll('.overview-edge:not(.selected)')].every(node=>parseFloat(getComputedStyle(node).opacity)<=.12),inputs:[...document.querySelectorAll('.resource-block')].map(node=>[...node.querySelectorAll('.overview-input')].map(port=>parseFloat(port.style.top)))};});
    assert.equal(compact.headerPlus,0);assert.ok(compact.rows.every(row=>row.height<=Math.max(64,row.sources*31+10)&&row.near&&row.lastBeforePlus&&row.routers===Math.max(0,row.blocks-1)));assert.ok(compact.muted&&compact.mutedRoutes);for(const ports of compact.inputs)assert.equal(new Set(ports).size,ports.length);
    assert.equal(await native.evaluate(()=>document.querySelectorAll('.signal-lanes .lane-wires,.signal-lanes .lane-arrow,.signal-lanes marker').length),0,'Canvas arrows stay out of the route table');
    assert.ok(await native.evaluate(()=>[...document.querySelectorAll('.signal-lane .transform-chip')].every(node=>node.getBoundingClientRect().width<=112&&node.getBoundingClientRect().height<=30)),'Compact transform sizing is retained');
    const heads=await native.evaluate(()=>[...document.querySelectorAll('.overview-edge')].map(path=>{const markerId=path.getAttribute('marker-end').slice(5,-1),marker=document.getElementById(markerId);return{width:parseFloat(getComputedStyle(path).strokeWidth),head:Number(marker.getAttribute('markerWidth')),tip:Number(marker.getAttribute('refX')),dash:path.style.strokeDasharray};}));
    for(const head of heads){assert.ok(head.head>head.width*2);assert.equal(head.tip,10);assert.ok(head.dash);}
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
    await native.evaluate(()=>{const panel=document.querySelector('.canvas-lanes-panel');panel.scrollTop=100;});await pause(80);assert.equal(await native.evaluate(()=>Math.abs(document.querySelector('.signal-lanes header').getBoundingClientRect().top-document.querySelector('.canvas-lanes-panel').getBoundingClientRect().top)<3),true,'Source/destination header stays visible while editing');await capture('signal-lanes');
    await native.evaluate(()=>document.querySelector('.canvas-stage').dispatchEvent(new KeyboardEvent('keydown',{key:'Escape',bubbles:true})));await pause(220);assert.equal((await snapshot()).transforms,0);
    await openBundle();
    const chainBefore=await native.evaluate(id=>{const rows=[...document.querySelectorAll('.signal-lane')],source=rows.find(row=>row.querySelector(`[data-transform-id="${id}"]`)),target=rows.find(row=>row!==source&&row.querySelectorAll('.transform-chip').length>=2);return{source:source.dataset.laneId,target:target.dataset.laneId,sourceIds:[...source.querySelectorAll('.transform-chip')].map(node=>node.dataset.transformId),targetIds:[...target.querySelectorAll('.transform-chip')].map(node=>node.dataset.transformId)};},transformId);
    const dragStage=async(sourceLane,nodeId,targetLane,before)=>{
      await native.evaluate(({sourceLane,nodeId})=>{const row=[...document.querySelectorAll('.signal-lane')].find(row=>row.dataset.laneId===sourceLane),chip=[...row.querySelectorAll('.transform-chip')].find(chip=>chip.dataset.transformId===nodeId);window.__chainDrag=new DataTransfer();chip.dispatchEvent(new DragEvent('dragstart',{bubbles:true,cancelable:true,dataTransfer:window.__chainDrag}));},{sourceLane,nodeId});
      await until(()=>native.evaluate(()=>!!document.querySelector('.chain-end')),'Chain drag offers end drop target');
      await native.evaluate(({targetLane,before})=>{const row=[...document.querySelectorAll('.signal-lane')].find(row=>row.dataset.laneId===targetLane),target=before?[...row.querySelectorAll('.transform-chip')].find(chip=>chip.dataset.transformId===before):row.querySelector('.chain-end');target.dispatchEvent(new DragEvent('dragover',{bubbles:true,cancelable:true,dataTransfer:window.__chainDrag}));target.dispatchEvent(new DragEvent('drop',{bubbles:true,cancelable:true,dataTransfer:window.__chainDrag}));delete window.__chainDrag;},{targetLane,before});
    };
    const chainIds=lane=>native.evaluate(lane=>[...[...document.querySelectorAll('.signal-lane')].find(row=>row.dataset.laneId===lane).querySelectorAll('.transform-chip')].map(chip=>chip.dataset.transformId),lane);
    await dragStage(chainBefore.source,transformId,chainBefore.source,null);
    await until(async()=>JSON.stringify(await chainIds(chainBefore.source))===JSON.stringify([...chainBefore.sourceIds.filter(id=>id!==transformId),transformId]),'Same-row drag moves the stage');
    await click('Undo');await until(async()=>JSON.stringify(await chainIds(chainBefore.source))===JSON.stringify(chainBefore.sourceIds),'Undo restores chain order');
    await dragStage(chainBefore.source,transformId,chainBefore.target,chainBefore.targetIds.at(-1));
    await until(async()=>(await chainIds(chainBefore.target)).length===chainBefore.targetIds.length+1,'Other-row drag duplicates stage');
    assert.deepEqual(await chainIds(chainBefore.source),chainBefore.sourceIds);const copied=(await chainIds(chainBefore.target)).filter(id=>!chainBefore.targetIds.includes(id));assert.equal(copied.length,1);assert.notEqual(copied[0],transformId);
    await click('Undo');await until(async()=>JSON.stringify(await chainIds(chainBefore.target))===JSON.stringify(chainBefore.targetIds),'Undo removes only the copied stage');
    await openBundle();await native.evaluate(()=>document.querySelector('.signal-lane .insert-transform').click());await until(()=>native.evaluate(()=>!!document.querySelector('dialog[open]')),'Insert opens typed transform chooser');await click('Insert transformation','dialog[open]');await until(()=>native.evaluate(()=>!document.querySelector('dialog[open]')),'Insert completes');assert.equal((await snapshot()).transforms,0);await click('Undo');
    await openBundle();await native.evaluate(()=>[...document.querySelectorAll('.signal-lane button')].find(button=>button.classList.contains('disconnect-route')).click());await pause(100);await click('Undo');await openBundle();assert.equal((await snapshot()).lanes.length,bundle.lanes.length);
    await native.evaluate(()=>document.querySelector('.canvas-stage').dispatchEvent(new KeyboardEvent('keydown',{key:'Escape',bubbles:true})));await pause(220);
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
      const shaftErrors=await native.evaluate(()=>[...document.querySelectorAll('.overview-edge')].map(path=>{const marker=document.getElementById(path.getAttribute('marker-end').slice(5,-1));return Math.abs(parseFloat(path.style.strokeDasharray)-(path.getTotalLength()-Number(marker.getAttribute('markerWidth'))*.55));}));assert.ok(shaftErrors.every(error=>error<.01),'Arrow shafts use current geometry after dragging and Undo');
      const p95=latencies.sort((a,b)=>a-b)[Math.floor(latencies.length*.95)];assert.ok(p95<150,`${style} pointer latency ${p95}ms`);await capture(style);console.log(`${style} drag p95 ${p95.toFixed(1)}ms`);
    }
    assert.equal(native.errors.length,0,JSON.stringify(native.errors));
    console.log(JSON.stringify({artifacts,blocks:expected.nodes,bundles:expected.bundles.length,hiddenTransforms:expected.transforms}));
  }finally{
    if(created){const current=(await request({command:'profile_assignments_get',id:profileId})).profiles;await request({command:'profile_delete',id:profileId,expectedRevision:current.document.revision});}
    native.close();
  }
});


test('native route list edits source and destination through filtered dropdowns and retains undo',{timeout:90000},async()=>{
  const native=await connectNative(),id=randomUUID(),name=`Endpoint editing ${id}`;
  const request=value=>native.evaluate(async value=>{const reply=await window.__TAURI_INTERNALS__.invoke('gateway_request',{request:{...value,clientId:'endpoint-edit-test'}});if(!reply.ok)throw Error(reply.error);return reply.state;},value);
  const click=(selector)=>native.evaluate(selector=>{const node=document.querySelector(selector);if(!node||node.disabled)throw Error(`Unavailable: ${selector}`);if(node instanceof HTMLInputElement)node.focus();if(typeof node.click==='function')node.click();else node.dispatchEvent(new MouseEvent('click',{bubbles:true}));},selector);
  const button=label=>native.evaluate(label=>{const node=[...document.querySelectorAll('button')].find(node=>(node.getAttribute('aria-label')??node.textContent.trim())===label);if(!node||node.disabled)throw Error(`Unavailable: ${label}`);node.click();},label);
  const input=async text=>{await native.evaluate(text=>{const input=document.activeElement;input.value=text;input.dispatchEvent(new Event('input',{bubbles:true}));},text);await pause(30);};
  const row='.signal-lane[data-lane-id="editable"]';
  let created=false;
  try{
    const document=await native.evaluate(async({id,name})=>{
      const {blankProfile}=await import('/src/lib/profile-editor.ts');const document=blankProfile(id);document.name=name;
      const contract={delivery:'state',schema:{type:'boolean'},freshnessMs:1000,fallback:null};
      const port=(id,direction)=>({id,name:id,direction,contract});
      document.resources=[{id:'source',name:'Endpoint source',kind:'device',required:false,ports:[port('Signal Alpha','output'),port('Signal Bravo','output'),{...port('Incompatible number','output'),contract:{...contract,schema:{type:'number',min:0,max:1,unit:null}}}]},{id:'target',name:'Endpoint target',kind:'device',required:false,ports:[port('Input Alpha','input'),port('Input Bravo','input'),port('Occupied input','input')]}];
      document.bindings=[{id:'editable',from:{nodeId:'source',portId:'Signal Alpha'},to:{nodeId:'target',portId:'Input Alpha'}},{id:'occupied',from:{nodeId:'source',portId:'Signal Bravo'},to:{nodeId:'target',portId:'Occupied input'}}];
      document.editor.positions=[{nodeId:'source',x:48,y:48},{nodeId:'target',x:550,y:48}];return document;
    },{id,name});
    await request({command:'profile_editor_save',document,assignments:[],expectedRevision:null});created=true;
    await native.evaluate(()=>[...document.querySelectorAll('nav button')].find(b=>b.textContent.trim()==='Profiles').click());
    await until(()=>native.evaluate(name=>{const discard=[...document.querySelectorAll('dialog[open] button')].find(b=>b.textContent.trim()==='Discard and continue');if(discard){discard.click();return false;}if(document.querySelector('.profile-title-button')?.textContent.trim()===name&&document.querySelector('.canvas-stage'))return true;const edit=[...document.querySelectorAll('button')].find(b=>b.getAttribute('aria-label')===`Edit ${name}`);if(edit)edit.click();else [...document.querySelectorAll('button')].find(b=>b.textContent.includes('Back to Profiles'))?.click();return false;},name),'Open endpoint fixture');
    await button('Fit all');await click('.signal-edge-hit');await until(()=>native.evaluate(()=>!!document.querySelector('.signal-lane[data-lane-id="editable"]')),'Signal row opens');
    await click(`${row} [data-endpoint-side="from"] input`);
    await until(()=>native.evaluate(()=>document.activeElement?.getAttribute('aria-expanded')==='true'),'Source dropdown opens with filter');
    await input('missing signal');assert.equal(await native.evaluate(()=>document.querySelectorAll('.signal-lanes [role="option"]').length),0);
    await input('Bravo');assert.ok(await native.evaluate(()=>{const option=document.querySelector('.signal-lanes [role="option"]'),rect=option.getBoundingClientRect();return option.contains(document.elementFromPoint(rect.left+rect.width/2,rect.top+rect.height/2));}),'Inline dropdown options must be visible and clickable above the panel');const shot=await native.send('Page.captureScreenshot',{format:'png'});await writeFile(resolve(artifacts,'endpoint-dropdown.png'),Buffer.from(shot.data,'base64'));assert.equal(await native.evaluate(()=>document.querySelectorAll('.signal-lanes [role="option"]').length),1);
    await click('.signal-lanes [role="option"]');await until(()=>native.evaluate(()=>!document.querySelector('.signal-lanes .port-dropdown')),'Source selection commits');
    assert.match(await native.evaluate(row=>document.querySelector(`${row} [data-endpoint-side="from"] input`).value,row),/Bravo/);
    await button('Undo');assert.match(await native.evaluate(row=>document.querySelector(`${row} [data-endpoint-side="from"] input`).value,row),/Alpha/);
    await button('Redo');assert.match(await native.evaluate(row=>document.querySelector(`${row} [data-endpoint-side="from"] input`).value,row),/Bravo/);
    await click(`${row} [data-endpoint-side="to"] input`);await until(()=>native.evaluate(()=>document.activeElement?.getAttribute('aria-expanded')==='true'),'Destination dropdown opens');
    assert.ok(!(await native.evaluate(()=>[...document.querySelectorAll('.signal-lanes [role="option"]')].map(node=>node.textContent))).some(text=>text.includes('Occupied input')));
    await input('Input Bravo');await click('.signal-lanes [role="option"]');await until(()=>native.evaluate(()=>!document.querySelector('.signal-lanes .port-dropdown')),'Destination selection commits');
    await button('Save profile');await until(()=>native.evaluate(()=>[...document.querySelectorAll('.editor-save-actions button')].find(b=>b.textContent.trim()==='Save profile')?.disabled),'Endpoint edits saved');
    const saved=(await request({command:'profile_assignments_get',id})).profiles.document;
    assert.deepEqual(saved.bindings.find(b=>b.id==='editable'),{id:'editable',from:{nodeId:'source',portId:'Signal Bravo'},to:{nodeId:'target',portId:'Input Bravo'}});
    assert.deepEqual(saved.bindings.find(b=>b.id==='occupied'),document.bindings.find(b=>b.id==='occupied'));
    await click(`${row} [data-endpoint-side="from"] input`);await native.evaluate(()=>document.activeElement.dispatchEvent(new KeyboardEvent('keydown',{key:'Escape',bubbles:true})));assert.equal(await native.evaluate(()=>document.activeElement.getAttribute('aria-expanded')),'false');assert.equal(await native.evaluate(()=>!!document.querySelector('.route-endpoint-picker')),false);
  }finally{try{if(created){const current=(await request({command:'profile_assignments_get',id})).profiles;await request({command:'profile_delete',id,expectedRevision:current.document.revision});}}finally{native.close();}}
});
