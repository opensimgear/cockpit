// Real mouse drags and native IPC against the already-running dev Cockpit.
// Discovery is read-only; creating a route atomically pins its chosen contract.
import test from 'node:test';
import assert from 'node:assert/strict';
import { randomUUID } from 'node:crypto';
import { mkdir, writeFile } from 'node:fs/promises';
import { resolve } from 'node:path';

const artifacts = resolve('.research/profile-wiring-native',String(Date.now()));
const sleep = milliseconds => new Promise(resolve => setTimeout(resolve, milliseconds));
const redact = value => String(value).replace(/[A-Za-z0-9_-]{43}/g, '[redacted]');
const point = (x, y) => ({ x: Math.round(x), y: Math.round(y) });

async function until(operation, description, timeout = 15000) {
  const end = Date.now() + timeout; let last;
  while (Date.now() < end) {
    try { const result = await operation(); if (result) return result; } catch (error) { last = error; }
    await sleep(100);
  }
  throw new Error(`${description}${last ? ': ' + redact(last.message) : ''}`);
}

async function connectNative() {
  let endpoint = process.env.OSG_NATIVE_PAGE_WS;
  if (!endpoint) {
    const response = await fetch(`${process.env.OSG_NATIVE_CDP_URL ?? 'http://127.0.0.1:9222'}/json/list`, { signal: AbortSignal.timeout(5000) });
    endpoint = (await response.json()).find(target => target.type === 'page' && /127\.0\.0\.1:1420|localhost:1420|tauri\.localhost/.test(target.url))?.webSocketDebuggerUrl;
  }
  assert.ok(endpoint, 'Start the actual native dev Cockpit with its page debugging target enabled.');
  const socket = new WebSocket(endpoint), pending = new Map(), listeners = new Map(); let sequence = 0;
  await new Promise((resolve, reject) => {
    const timer = setTimeout(() => reject(new Error('Native page connection timed out.')), 8000);
    socket.addEventListener('open', () => { clearTimeout(timer); resolve(); }, { once: true });
    socket.addEventListener('error', () => { clearTimeout(timer); reject(new Error('Native page connection failed.')); }, { once: true });
  });
  socket.addEventListener('message', event => {
    const message = JSON.parse(event.data);
    if (message.method) { for (const listener of listeners.get(message.method) ?? []) listener(message.params); return; }
    const operation = pending.get(message.id); if (!operation) return;
    clearTimeout(operation.timer); pending.delete(message.id);
    if (message.error) operation.reject(new Error(redact(message.error.message))); else operation.resolve(message.result);
  });
  socket.addEventListener('close', () => { for (const operation of pending.values()) { clearTimeout(operation.timer); operation.reject(new Error('Native page connection closed.')); } pending.clear(); });
  const send = (method, params = {}) => new Promise((resolve, reject) => {
    const id = ++sequence, timer = setTimeout(() => { pending.delete(id); reject(new Error(`Native CDP ${method} timed out.`)); }, 12000);
    pending.set(id, { resolve, reject, timer }); socket.send(JSON.stringify({ id, method, params }));
  });
  return {
    send,
    on(method, listener) { listeners.set(method, [...(listeners.get(method) ?? []), listener]); },
    async evaluate(expression) {
      const result = await send('Runtime.evaluate', { expression, returnByValue: true, awaitPromise: true });
      if (result.exceptionDetails) throw new Error(redact(result.exceptionDetails.exception?.description ?? result.exceptionDetails.exception?.value ?? result.exceptionDetails.text));
      return result.result.value;
    },
    close() { socket.close(); },
  };
}

test('native canvas constructs compact transforms with named border ports and smooth shared resource anchors', { timeout: 240000 }, async () => {
  await mkdir(artifacts, { recursive: true });
  const native = await connectNative(), name = `Wiring ${randomUUID()}`, checks = [];
  let profileId, beforeProfiles, beforeAutomatic, intercepted, dragging = false, draftOpen = false, companionEvidence;
  const request = async value => {
    const result = await native.evaluate(`window.__TAURI_INTERNALS__.invoke('gateway_request',{request:${JSON.stringify({ ...value, clientId: 'profile-wiring-native' })}})`);
    assert.equal(result.ok, true, `${value.command}: ${redact(result.error ?? 'Native request failed')}`); return result.state;
  };
  const automatic = async () => (await request({ command: 'profile_automatic_status' })).profiles.automatic;
  const saved = async () => (await request({ command: 'profile_assignments_get', id: profileId })).profiles;
  const clickIfPresent = (label, scope = '.profiles-workspace') => native.evaluate(`(()=>{const root=document.querySelector(${JSON.stringify(scope)});const button=[...(root?.querySelectorAll('button')??[])].find(button=>(button.getAttribute('aria-label')??button.textContent.replace(/\\s+/g,' ').trim())===${JSON.stringify(label)});if(!button||button.disabled)return false;button.scrollIntoView({block:'nearest'});button.click();return true;})()`);
  const click = (label, scope) => until(() => clickIfPresent(label, scope), `UI action unavailable: ${label}`);
  const saveDisabled = () => native.evaluate(`(()=>{const button=[...document.querySelectorAll('.profiles-workspace button')].find(button=>button.textContent.trim()==='Save profile');return button?.disabled;})()`);
  const save = async () => {
    await click('Save profile');
    if (!profileId) profileId = await until(async () => (await request({ command: 'profile_list' })).profiles.profiles.find(profile => profile.name === name)?.id, 'Temporary profile was not saved');
    await until(saveDisabled, 'Profile save did not settle'); return saved();
  };
  const snapshot = () => native.evaluate(`(()=>{const stage=document.querySelector('.canvas-stage'),rect=stage?.getBoundingClientRect(),panel=document.querySelector('.canvas-port-picker'),pr=panel?.getBoundingClientRect();return{stage:rect?{left:rect.left,top:rect.top,width:rect.width,height:rect.height}:null,nodes:[...document.querySelectorAll('.canvas-stage .resource-block')].map(node=>({id:node.dataset.nodeId,inputs:node.classList.contains('processing')?node.querySelectorAll('[data-connection-target][data-connection-port]').length:Number(node.querySelector('[data-connection-target] .port-group-heading > span')?.textContent),outputs:node.classList.contains('processing')?node.querySelectorAll('[data-connection-source][data-connection-port]').length:Number(node.querySelector('[data-connection-source] .port-group-heading > span')?.textContent)})),edges:document.querySelectorAll('.canvas-stage .signal-edge').length,panel:panel?{source:panel.dataset.fromNode,target:panel.dataset.toNode,output:panel.querySelector('input[aria-label="Find an output"]')?.dataset.value,input:panel.querySelector('input[aria-label="Find an input"]')?.dataset.value,visible:pr.width>0&&pr.height>0&&pr.top<innerHeight&&pr.bottom>0,right:pr.right,width:pr.width,insideCanvas:!!panel.closest('.canvas-layout')}:null};})()`);
  const stageVisible = async () => { await native.evaluate(`document.querySelector('.canvas-stage').scrollIntoView({block:'start'});window.scrollBy(0,-150)`); await sleep(150); return snapshot(); };
  const mouse = (type, location, extra = {}) => native.send('Input.dispatchMouseEvent', { type, ...location, ...extra });
  const center = selector => until(() => native.evaluate(`(()=>{const element=document.querySelector(${JSON.stringify(selector)});if(!element||element.disabled)return null;const r=element.getBoundingClientRect(),x=Math.round(r.left+r.width/2),y=Math.round(r.top+r.height/2);if(!r.width||!r.height||r.left<0||r.top<0||r.right>innerWidth||r.bottom>innerHeight||!element.contains(document.elementFromPoint(x,y)))return null;return{x,y};})()`), `Visible mouse target unavailable: ${selector}`);
  const background = () => until(() => native.evaluate(`(()=>{const stage=document.querySelector('.canvas-stage'),r=stage.getBoundingClientRect();for(const fy of [.15,.3,.8,.6])for(const fx of [.1,.5,.85]){const x=Math.round(r.left+r.width*fx),y=Math.round(r.top+r.height*fy),hit=document.elementFromPoint(x,y);if(x>0&&y>0&&x<innerWidth&&y<innerHeight&&hit&&stage.contains(hit)&&!hit.closest('[data-node-id],.signal-edge-hit,.canvas-inspector,.canvas-port-picker,button,input,select'))return{x,y};}return null;})()`), 'Visible blank canvas is unavailable');
  const dragWire = async (source, target, allowDirectCommit = false) => {
    const beforeEdges = (await snapshot()).edges;
    await clickIfPresent('Close component settings','.canvas-inspector');await stageVisible(); const from = await center(source), to = await center(target);
    await mouse('mouseMoved', from); await mouse('mousePressed', from, { button: 'left', buttons: 1, clickCount: 1 });
    for (const fraction of [.2, .5, .8, 1]) await mouse('mouseMoved', point(from.x + (to.x - from.x) * fraction, from.y + (to.y - from.y) * fraction), { button: 'left', buttons: 1 });
    await mouse('mouseReleased', to, { button: 'left', buttons: 0, clickCount: 1 });
    return until(async () => { const state = await snapshot(); return state.panel?.visible || allowDirectCommit && state.edges > beforeEdges ? state : null; }, 'A real connection drag did not open the contextual port picker or commit its exact ports');
  };
  const portChoices = async label => {
    await until(() => native.evaluate(`(()=>{const input=document.querySelector(${JSON.stringify(`.canvas-port-picker input[aria-label="${label}"]`)});if(!input||input.disabled)return false;input.focus();input.click();return true;})()`), `Port search unavailable: ${label}`);
    return until(() => native.evaluate(`(()=>{const input=document.querySelector(${JSON.stringify(`.canvas-port-picker input[aria-label="${label}"]`)}),list=document.getElementById(input?.getAttribute('aria-controls'));return list?[...list.querySelectorAll('[role="option"]')].map(option=>({value:option.dataset.value,text:option.textContent})):null;})()`), `Port dropdown unavailable: ${label}`);
  };
  const search = async (label, value) => {
    await portChoices(label);
    await native.evaluate(`(()=>{const input=document.querySelector(${JSON.stringify(`.canvas-port-picker input[aria-label="${label}"]`)});input.value=${JSON.stringify(value)};input.dispatchEvent(new Event('input',{bubbles:true}));})()`);
  };
  const choosePort = async (label, value) => {
    await search(label, value);
    const resultPosition = () => until(() => native.evaluate(`(()=>{const input=document.querySelector(${JSON.stringify(`.canvas-port-picker input[aria-label="${label}"]`)}),list=document.getElementById(input?.getAttribute('aria-controls'));const option=[...(list?.querySelectorAll('[role="option"]')??[])].find(option=>option.dataset.value===${JSON.stringify(value)});if(!option||option.disabled)return null;option.scrollIntoView({block:'nearest'});const r=option.getBoundingClientRect(),x=Math.round(r.left+r.width/2),y=Math.round(r.top+r.height/2);return r.width&&r.height&&option.contains(document.elementFromPoint(x,y))?{x,y}:null;})()`), `Visible port result unavailable: ${label} ${value}`);
    await mouse('mouseMoved', await resultPosition());await sleep(80);const position=await resultPosition();await mouse('mousePressed', position, { button: 'left', buttons: 1, clickCount: 1 }); await mouse('mouseReleased', position, { button: 'left', buttons: 0, clickCount: 1 });
    await until(() => native.evaluate(`!document.querySelector('.canvas-port-picker')||document.querySelector(${JSON.stringify(`.canvas-port-picker input[aria-label="${label}"]`)})?.dataset.value===${JSON.stringify(value)}`), `Chosen port was not retained: ${label}`);
  };
  const capture = async filename => { const result = await native.send('Page.captureScreenshot', { format: 'png' }); await writeFile(resolve(artifacts, filename), Buffer.from(result.data, 'base64')); };
  const transformGeometry = async transform => {
    const geometry = await native.evaluate(`(()=>{const id=${JSON.stringify(transform.id)},node=[...document.querySelectorAll('.resource-block')].find(node=>node.dataset.nodeId===id),resource=document.querySelector('.resource-block:not(.processing)'),rect=node.getBoundingClientRect(),reference=resource.getBoundingClientRect(),scale=rect.width/parseFloat(getComputedStyle(node).width);const ports=[...node.querySelectorAll('[data-connection-port]')].map(handle=>{const r=handle.getBoundingClientRect(),direction=handle.hasAttribute('data-connection-source')?'output':'input',dot=handle.querySelector('.port-dot,.connection-dot');let dotCenter;if(dot){const d=dot.getBoundingClientRect();dotCenter={x:d.left+d.width/2,y:d.top+d.height/2};}else{const style=getComputedStyle(handle,'::after'),hs=getComputedStyle(handle),matrix=new DOMMatrixReadOnly(style.transform==='none'?undefined:style.transform),width=parseFloat(style.width)+(style.boxSizing==='border-box'?0:parseFloat(style.borderLeftWidth)+parseFloat(style.borderRightWidth)),height=parseFloat(style.height)+(style.boxSizing==='border-box'?0:parseFloat(style.borderTopWidth)+parseFloat(style.borderBottomWidth)),left=style.left==='auto'?r.right-(parseFloat(hs.borderRightWidth)+parseFloat(style.right)+width)*scale:r.left+(parseFloat(hs.borderLeftWidth)+parseFloat(style.left))*scale;dotCenter={x:left+(matrix.e+width/2)*scale,y:r.top+(parseFloat(hs.borderTopWidth)+parseFloat(style.top)+matrix.f+height/2)*scale};}return{id:handle.dataset.connectionPort,direction,name:handle.textContent.trim(),x:direction==='output'?rect.right:rect.left,y:r.top+r.height/2,dotCenter};});const edges=[...document.querySelectorAll('.signal-edge')].map(path=>{const binding=${JSON.stringify(transform.bindings ?? [])}.find(binding=>binding.id===path.dataset.bindingId);if(!binding)return null;const matrix=path.getScreenCTM(),a=path.getPointAtLength(0),b=path.getPointAtLength(path.getTotalLength()),start=new DOMPoint(a.x,a.y).matrixTransform(matrix),end=new DOMPoint(b.x,b.y).matrixTransform(matrix);return{binding,start:{x:start.x,y:start.y},end:{x:end.x,y:end.y}};}).filter(Boolean);return{category:node.querySelector('.block-category')?.textContent.trim(),width:rect.width,height:rect.height,referenceWidth:reference.width,referenceHeight:reference.height,countBoxes:node.querySelectorAll('.block-port-groups').length,ports,edges};})()`);
    assert.equal(geometry.category, 'Transform');
    assert.ok(geometry.width < geometry.referenceWidth && geometry.height < geometry.referenceHeight, 'Transforms occupy less canvas space than resource blocks.');
    assert.equal(geometry.countBoxes, 0, 'Transforms expose individual named ports without generic input/output count boxes.');
    for (const [direction, ports] of [['input', transform.inputs], ['output', transform.outputs]]) {
      assert.equal(geometry.ports.filter(port => port.direction === direction).length, ports.length);
      for (const port of ports) assert.ok(geometry.ports.some(handle => handle.id === port.id && handle.direction === direction && handle.name.includes(port.name)), `Named ${direction} port ${port.id} was not rendered.`);
    }
    for (const port of geometry.ports) assert.ok(Math.hypot(port.dotCenter.x - port.x, port.dotCenter.y - port.y) < .65, `The ${port.direction} port dot ${port.id} must be centered on the block border.`);
    for (const edge of geometry.edges) {
      const endpoint = edge.binding.from.nodeId === transform.id ? edge.binding.from : edge.binding.to.nodeId === transform.id ? edge.binding.to : null;
      if (!endpoint) continue;
      const output = edge.binding.from.nodeId === transform.id, handle = geometry.ports.find(port => port.id === endpoint.portId && port.direction === (output ? 'output' : 'input')), at = output ? edge.start : edge.end;
      assert.ok(handle, `A named handle is required for exact edge ${edge.binding.id}.`);
      assert.ok(Math.hypot(at.x - handle.x, at.y - handle.y) < .65, `Wire ${edge.binding.id} must meet the exact port center on the block border.`);
    }
    return geometry;
  };
  const edgePoint = bindingId => until(() => native.evaluate(`(()=>{const path=[...document.querySelectorAll('.signal-edge-hit')].find(path=>path.dataset.connectionId===${JSON.stringify(bindingId)}),stage=document.querySelector('.canvas-stage');if(!path||!stage)return null;const bounds=stage.getBoundingClientRect(),length=path.getTotalLength(),matrix=path.getScreenCTM();for(const fraction of [.5,.35,.65,.2,.8,.1,.9]){const point=path.getPointAtLength(length*fraction),screen=new DOMPoint(point.x,point.y).matrixTransform(matrix);if(screen.x>bounds.left&&screen.x<bounds.right&&screen.y>bounds.top&&screen.y<bounds.bottom&&screen.x<innerWidth&&screen.y<innerHeight&&document.elementFromPoint(screen.x,screen.y)?.closest('.signal-edge-hit')===path)return{x:Math.round(screen.x),y:Math.round(screen.y)};}return null;})()`),`Visible selectable wire unavailable: ${bindingId}`);
  const selectEdge = async bindingId => {
    await clickIfPresent('Close component settings','.canvas-inspector');await stageVisible();const at=await edgePoint(bindingId);
    await mouse('mouseMoved',at);await mouse('mousePressed',at,{button:'left',buttons:1,clickCount:1});await mouse('mouseReleased',at,{button:'left',buttons:0,clickCount:1});
    await until(()=>native.evaluate(`document.querySelectorAll('.connection-endpoint[data-reconnect-binding="${bindingId}"]').length===2`),'Selecting a wire did not reveal its endpoint handles');
  };
  const dropPaletteComponent = async (selector,bindingId) => {
    await clickIfPresent('Close component settings','.canvas-inspector');
    await native.evaluate(`(()=>{const button=document.querySelector(${JSON.stringify(selector)}),details=button.closest('details');if(!details.open)details.querySelector('summary').click();})()`);await sleep(220);await native.evaluate(`document.querySelector(${JSON.stringify(selector)}).scrollIntoView({block:'nearest'})`);await sleep(80);intercepted=null;
    const from=await center(selector);await native.send('Input.setInterceptDrags',{enabled:true});
    await mouse('mouseMoved',from);await mouse('mousePressed',from,{button:'left',buttons:1,clickCount:1});dragging=true;
    for(const delta of [3,9,20,40]){await mouse('mouseMoved',point(from.x+delta,from.y+3),{button:'left',buttons:1});await sleep(35);if(intercepted)break;}
    const data=await until(()=>intercepted?.data,'Actual transformation palette drag did not start',5000);await native.send('Input.setInterceptDrags',{enabled:false});await stageVisible();const drop=bindingId?await edgePoint(bindingId):await background();
    await native.send('Input.dispatchDragEvent',{type:'dragEnter',...drop,data});await native.send('Input.dispatchDragEvent',{type:'dragOver',...drop,data});
    await until(()=>native.evaluate(`document.querySelector('[aria-label="Component drop preview"]')?.textContent.includes(${JSON.stringify(bindingId?'Release to insert into connection':'Release to add')})`),'Actual transformation drop did not offer the expected insertion/add operation');
    await native.send('Input.dispatchDragEvent',{type:'drop',...drop,data});dragging=false;await mouse('mouseReleased',drop,{button:'left',buttons:0});
  };
  const dropTransformation=(preset,bindingId)=>dropPaletteComponent(`.component-palette [data-processing-preset="${preset}"]`,bindingId);
  native.on('Input.dragIntercepted', event => { intercepted = event; });
  try {
    await native.send('Page.reload', { ignoreCache: true });
    await until(() => native.evaluate('typeof window.__TAURI_INTERNALS__?.invoke === "function"'), 'Native cockpit IPC did not finish starting');
    assert.equal(await native.evaluate('document.visibilityState'), 'visible');
    await native.send('Input.cancelDragging'); await mouse('mouseReleased', { x: 10, y: 10 }, { button: 'left', buttons: 0 });
    beforeProfiles = await until(async () => (await request({ command: 'profile_list' })).profiles.profiles, 'Native gateway did not become ready'); beforeAutomatic = await automatic();
    const inventory = await until(async()=>{const value=(await request({ command: 'profile_resources_list', offset: 0, limit: 128 })).profileResources;return value.resources.some(resource=>resource.resourceType==='host-controller'&&resource.availability==='online'&&resource.portCount>0)?value:null;},'Native wiring acceptance requires one real online controller.');
    const resource = inventory.resources.filter(resource => resource.resourceType === 'host-controller' && resource.availability === 'online' && resource.portCount > 0).sort((a, b) => b.portCount - a.portCount)[0];
    assert.ok(resource, 'Native wiring acceptance requires one real online controller.');
    const observed = []; let metadata;
    do {
      const page = (await request({ command: 'profile_resources_inspect', resourceId: resource.id, offset: observed.length, limit: 32 })).profileResources;
      if (!metadata) metadata = page; else { assert.equal(page.revision, metadata.revision); assert.equal(page.total, metadata.total); }
      observed.push(...page.ports);
    } while (observed.length < metadata.total);
    const outputs = observed.filter(port => port.current && port.descriptor.direction === 'output'), inputs = observed.filter(port => port.current && port.descriptor.direction === 'input');
    const boolean = outputs.find(port => port.descriptor.contract.delivery === 'state' && port.descriptor.contract.schema.type === 'boolean');
    assert.ok(boolean, 'A real boolean controller output is required for the virtual button route.');

    await click('Profiles', 'nav[aria-label="Main navigation"]');
    await until(async()=>{if(await clickIfPresent('← Back to Profiles'))return false;if(await clickIfPresent('Discard and continue'))return false;return native.evaluate(`[...document.querySelectorAll('.profiles-workspace button')].some(button=>button.textContent.trim()==='Add profile')`);},'The profile list did not finish opening');
    await click('Add profile'); draftOpen = true;
    await click('Edit profile name');
    await native.evaluate(`(()=>{const input=document.querySelector('.profile-name-input');input.value=${JSON.stringify(name)};input.dispatchEvent(new Event('input',{bubbles:true}));input.dispatchEvent(new KeyboardEvent('keydown',{key:'Enter',bubbles:true,cancelable:true}));})()`);
    await save(); draftOpen = false;
    await native.evaluate(`(()=>{const d=document.querySelector('.component-palette [aria-label="Add virtual controller"]').closest('details');if(!d.open)d.querySelector('summary').click();})()`);
    await click('Add virtual controller', '.component-palette');
    assert.equal(await native.evaluate(`Array.from(document.querySelectorAll('.routing-toolbar button')).some(button=>button.textContent.trim()==='Connect exact ports')`), false, 'Connection creation starts from the canvas drag handles.');

    // Start the actual palette HTML5 drag with mouse input, then use its own data.
    intercepted = null;
    await native.evaluate(`(()=>{const details=document.querySelector('.component-palette [data-resource-id="${resource.id}"]').closest('details');if(!details.open)details.querySelector('summary').click();})()`);
    await sleep(250);
    await native.evaluate(`document.querySelector('.component-palette [data-resource-id="${resource.id}"]').scrollIntoView({block:'nearest'})`);
    const paletteSource = await center(`.component-palette [data-resource-id="${resource.id}"]`);
    await native.send('Input.setInterceptDrags', { enabled: true });
    await mouse('mouseMoved', paletteSource); await mouse('mousePressed', paletteSource, { button: 'left', buttons: 1, clickCount: 1 }); dragging = true;
    for (const delta of [3, 9, 20, 40]) { await mouse('mouseMoved', point(paletteSource.x + delta, paletteSource.y + 3), { button: 'left', buttons: 1 }); await sleep(35); if (intercepted) break; }
    const data = await until(() => intercepted?.data, 'Real palette movement did not start a native drag', 5000);
    await native.send('Input.setInterceptDrags', { enabled: false });
    const stage = (await stageVisible()).stage, drop = point(stage.left + stage.width * .25, stage.top + 250);
    await native.send('Input.dispatchDragEvent', { type: 'dragEnter', ...drop, data });
    await native.send('Input.dispatchDragEvent', { type: 'dragOver', ...drop, data });
    await until(() => native.evaluate(`!!document.querySelector('[aria-label="Component drop preview"]')`), 'Real resource drag did not show its preview');
    await native.send('Input.dispatchDragEvent', { type: 'drop', ...drop, data }); dragging = false;
    await until(async () => { const state = await snapshot(); return state.nodes.length === 2 && state.nodes.some(node => node.outputs === outputs.length && node.inputs === inputs.length) ? state : null; }, 'Dropped controller did not automatically discover all its inputs and outputs');
    assert.ok(outputs.length > 32, 'This acceptance should exercise the actual multipage controller catalogue.');
    await click('Auto-layout'); await click('Fit all');
    const baseline = await save(), resourceId = baseline.assignments.find(assignment => assignment.sourceId === resource.id).resourceId, virtual = baseline.document.virtualOutputs[0];
    assert.deepEqual(baseline.document.resources.find(node => node.id === resourceId).ports, []);
    assert.deepEqual(baseline.assignments.find(assignment => assignment.resourceId === resourceId).ports, []);
    await stageVisible(); await capture('discovered-controller.png');
    checks.push(`A real dropped controller automatically presents all ${outputs.length} outputs and ${inputs.length} inputs across ${Math.ceil(observed.length / 32)} catalogue pages`);

    let state = await dragWire(`[data-connection-source="${resourceId}"]`, `[data-connection-target="${virtual.id}"]`);
    assert.equal(state.panel.source, resourceId); assert.equal(state.panel.target, virtual.id); assert.ok(state.panel.width<500);
    assert.equal(await native.evaluate(`document.querySelector('[data-connection-source="${resourceId}"]').dataset.connectionActive==='true'&&document.querySelector('[data-connection-target="${virtual.id}"]').dataset.connectionActive==='true'`),true);
    assert.ok(state.panel.width > 220);
    assert.ok((await portChoices('Find an output')).length>0); assert.ok((await portChoices('Find an input')).length>0);
    assert.equal(await native.evaluate(`document.querySelectorAll('.canvas-port-picker select[aria-label="Exact source output"],.canvas-port-picker select[aria-label="Exact destination input / action"]').length`), 0, 'Port search and selection share one combobox per direction.');
    assert.equal(await native.evaluate(`!!document.querySelector('.canvas-port-picker button')&&[...document.querySelectorAll('.canvas-port-picker button')].some(button=>button.textContent.trim()==='Use this exact port')`), false);
    await capture('output-to-input-side-panel.png');
    const cancelPoint = await background();
    await mouse('mouseMoved', cancelPoint); await mouse('mousePressed', cancelPoint, { button: 'left', buttons: 1, clickCount: 1 }); await mouse('mouseReleased', cancelPoint, { button: 'left', buttons: 0, clickCount: 1 });
    await until(async () => !(await snapshot()).panel, 'Clicking blank canvas did not cancel the connection panel');
    assert.equal(await native.evaluate(`document.querySelectorAll('.pending-wire,.resource-block.selected').length`), 0);
    assert.equal((await snapshot()).edges, 0); assert.equal(await saveDisabled(), true);
    assert.deepEqual((await saved()).document, baseline.document); assert.deepEqual((await saved()).assignments, baseline.assignments);
    checks.push('Output-to-input mouse drag opens a contextual picker and highlights both endpoint boxes; clicking blank canvas cancels without changing the graph or undo history');

    await stageVisible();
    const wireStart = await center(`[data-connection-source="${resourceId}"]`), wireEnd = await background();
    await mouse('mouseMoved', wireStart); await mouse('mousePressed', wireStart, { button: 'left', buttons: 1, clickCount: 1 });
    for (const fraction of [.2,.5,.8,1]) await mouse('mouseMoved', point(wireStart.x+(wireEnd.x-wireStart.x)*fraction,wireStart.y+(wireEnd.y-wireStart.y)*fraction), { button: 'left', buttons: 1 });
    await mouse('mouseReleased', wireEnd, { button: 'left', buttons: 0, clickCount: 1 });
    assert.equal((await snapshot()).panel, null); assert.equal(await saveDisabled(), true);
    assert.equal(await native.evaluate(`document.querySelectorAll('.pending-wire,.resource-block.selected').length`), 0);
    checks.push('Releasing an unfinished wire on blank canvas cancels the operation and leaves no pending selection or graph edit');

    state = await dragWire(`[data-connection-target="${virtual.id}"]`, `[data-connection-source="${resourceId}"]`);
    assert.equal(state.panel.source, resourceId); assert.equal(state.panel.target, virtual.id);
    const observedKey = `@observed:${JSON.stringify([boolean.descriptor.id, boolean.contractHash])}`;
    await search('Find an output', observedKey);
    await until(async () => (await portChoices('Find an output')).length === 1, 'Searching the exact output did not isolate its port');
    await capture('port-search-results.png');
    await search('Find an output', `no-matching-port-${randomUUID()}`);
    await until(() => native.evaluate(`document.querySelector('.canvas-port-picker .port-count')?.textContent.includes('No matching ports.')&&document.querySelector('input[aria-label="Find an output"]').dataset.value===''`), 'Typing a new query did not clear the stale selection or expose empty results');
    await native.send('Input.dispatchKeyEvent', { type: 'keyDown', key: 'Escape', code: 'Escape', windowsVirtualKeyCode: 27 }); await native.send('Input.dispatchKeyEvent', { type: 'keyUp', key: 'Escape', code: 'Escape', windowsVirtualKeyCode: 27 });
    assert.ok((await snapshot()).panel, 'Escape closes the dropdown before the contextual picker');
    await choosePort('Find an output', observedKey);
    await choosePort('Find an input', virtual.ports[0].id);
    await until(async () => { const state = await snapshot(); return state.edges === 1 && !state.panel; }, 'Creating the chosen route did not close the panel and draw one connection');
    const connected = await save(), pin = connected.assignments.find(assignment => assignment.resourceId === resourceId).ports;
    assert.equal(pin.length, 1); assert.equal(pin[0].observedPortId, boolean.descriptor.id); assert.equal(pin[0].expectedContractHash, boolean.contractHash);
    assert.equal(connected.document.resources.find(node => node.id === resourceId).ports.length, 1);
    assert.deepEqual(connected.document.bindings[0].from, { nodeId: resourceId, portId: pin[0].profilePortId });
    assert.deepEqual(connected.document.bindings[0].to, { nodeId: virtual.id, portId: virtual.ports[0].id });
    checks.push('Input-to-output mouse drag preserves signal direction and the chosen compatible route pins only its selected real provider contract');

    await click('Undo'); await until(async () => (await snapshot()).edges === 0, 'One Undo did not remove the route');
    const undone = await save();
    assert.deepEqual(undone.document.resources.find(node => node.id === resourceId).ports, []); assert.deepEqual(undone.assignments.find(assignment => assignment.resourceId === resourceId).ports, []);
    await click('Redo'); await until(async () => (await snapshot()).edges === 1, 'One Redo did not restore the complete route');
    const redone = await save(); assert.deepEqual(redone.document.bindings, connected.document.bindings); assert.deepEqual(redone.assignments, connected.assignments);
    await click('← Back to Profiles'); await click(`Edit ${name}`);
    await until(async () => { const state = await snapshot(); return state.edges === 1 && state.nodes.some(node => node.id === resourceId && node.outputs === outputs.length); }, 'Reopening did not restore the route and rediscover the full component catalogue');
    const reopened = await saved(); assert.deepEqual(reopened.document, redone.document); assert.deepEqual(reopened.assignments, redone.assignments);
    await stageVisible(); await capture('saved-wiring.png');
    checks.push('One Undo removes both route and newly selected provider port; Redo, Save and reopen preserve the exact route and full discovered catalogue');

    // A companion discovers its profile's chosen retained panel, independently
    // of the panel currently displayed by the actual device. No activation occurs.
    const paired = inventory.resources.filter(resource => resource.resourceType === 'companion' && resource.attributes.componentKind === 'paired-companion').sort((a, b) => Number(b.availability === 'online') - Number(a.availability === 'online'));
    if (paired.length) {
      const catalogue = (await request({ command: 'profile_companion_catalogue' })).profileCompanionAuthoring;
      let fixture;
      for (const companion of paired) {
        for (const panel of catalogue.panels.filter(panel => !panel.compatibility || panel.compatibility.some(item => item.companionId === companion.attributes.companionId && item.compatible))) {
          const ports = []; let first;
          do {
            const page = (await request({ command: 'profile_companion_inspect', resourceId: companion.id, publicationHash: panel.publicationHash, offset: ports.length, limit: 32 })).profileCompanionAuthoring;
            if (!first) first = page; else { assert.equal(page.identityFingerprint, first.identityFingerprint); assert.equal(page.capabilityFingerprint, first.capabilityFingerprint); }
            ports.push(...page.ports);
          } while (ports.length < first.total);
          const output = ports.find(port => port.descriptor.direction === 'output' && port.descriptor.contract.delivery === 'state' && port.descriptor.contract.schema.type === 'boolean');
          if (output) { fixture = { companion, panel, ports, output }; break; }
        }
        if (fixture) break;
      }
      assert.ok(fixture, 'An existing compatible retained companion panel with a boolean control is required.');
      const { companion, panel, ports, output } = fixture;
      await native.evaluate(`(()=>{const details=document.querySelector('.component-palette [aria-label="Companion panel components"]');if(!details.open)details.querySelector('summary').click();})()`);
      const panelSelector=`.component-palette [data-panel-source-id="${companion.id}"][data-panel-publication="${panel.publicationHash}"]`;
      await until(()=>native.evaluate(`!!document.querySelector(${JSON.stringify(panelSelector)})&&!document.querySelector(${JSON.stringify(panelSelector)}).disabled`),'The retained companion panel was not offered in the palette');await dropPaletteComponent(panelSelector);
      let companionState = await save(); const companionNodeId = companionState.assignments.find(assignment => assignment.sourceId === companion.id).resourceId;
      assert.deepEqual(companionState.document.resources.find(node => node.id === companionNodeId).ports, []);
      assert.equal(companionState.document.companionAssignments.find(assignment=>assignment.resourceId===companionNodeId).publicationHash,panel.publicationHash);
      const expectedOutputs = ports.filter(port => port.descriptor.direction === 'output').length, expectedInputs = ports.filter(port => port.descriptor.direction === 'input').length;
      await until(async () => (await snapshot()).nodes.some(node => node.id === companionNodeId && node.outputs === expectedOutputs && node.inputs === expectedInputs), 'Selecting the companion panel did not automatically discover its exact retained ports');
      await mouse('mouseMoved', { x: 10, y: 10 }); await click('Auto-layout'); await click('Fit all'); companionState = await save();
      assert.deepEqual(companionState.document.resources.find(node => node.id === companionNodeId).ports, []);
      assert.equal(companionState.document.companionAssignments.find(assignment => assignment.resourceId === companionNodeId).publicationHash, panel.publicationHash);
      await dragWire(`[data-connection-source="${companionNodeId}"]`, `[data-connection-target="${virtual.id}"]`);
      const companionOutputs = await portChoices('Find an output');
      assert.ok(companionOutputs.length>0);
      // The searchable list is virtualized; exact native IDs remain searchable.
      await choosePort('Find an output', `@observed:${JSON.stringify([output.descriptor.id, output.contractHash])}`); await choosePort('Find an input', virtual.ports[1].id);
      await capture('companion-panel-wiring.png');
      await until(async () => (await snapshot()).edges === 2, 'The discovered companion control did not create a second route');
      const companionConnected = await save(), companionPins = companionConnected.assignments.find(assignment => assignment.resourceId === companionNodeId).ports;
      assert.equal(companionPins.length, 1); assert.equal(companionPins[0].observedPortId, output.descriptor.id); assert.equal(companionPins[0].expectedContractHash, output.contractHash);
      await click('Undo'); await until(async () => (await snapshot()).edges === 1, 'One Undo did not remove the companion route');
      const companionUndone = await save(); assert.deepEqual(companionUndone.document.resources.find(node => node.id === companionNodeId).ports, []); assert.deepEqual(companionUndone.assignments.find(assignment => assignment.resourceId === companionNodeId).ports, []);
      await click('Redo'); await until(async () => (await snapshot()).edges === 2, 'One Redo did not restore the companion route'); const companionRedone = await save();
      await click('← Back to Profiles'); await click(`Edit ${name}`);
      await until(async () => (await snapshot()).edges === 2 && (await snapshot()).nodes.some(node => node.id === companionNodeId && node.outputs === expectedOutputs && node.inputs === expectedInputs), 'Reopening did not rediscover the selected companion panel and preserve both routes');
      assert.deepEqual((await saved()).document, companionRedone.document);
      companionEvidence = { sourceId: companion.id, name: companion.name, panelId: panel.panelId, publicationHash: panel.publicationHash, total: ports.length, outputs: expectedOutputs, inputs: expectedInputs, chosenPort: output.descriptor.id };
      checks.push('Dragging an actual retained companion panel atomically captures its companion/publication and discovers its ports; wiring pins one selected control and survives Undo/Redo, Save and reopen');
    }
    // Parallel wires share the one output/input attachment but keep distinct
    // smooth paths. Selection and endpoint dragging operate on exact bindings.
    const beforeParallel=await saved(),edgeCount=beforeParallel.document.bindings.length;
    await dragWire(`[data-connection-source="${resourceId}"]`,`[data-connection-target="${virtual.id}"]`);
    await choosePort('Find an output',pin[0].profilePortId);await choosePort('Find an input',virtual.ports[3].id);
    await until(async()=>(await snapshot()).edges===edgeCount+1&&!(await snapshot()).panel,'A second parallel wire did not auto-commit');
    const parallel=await save(),parallelEdge=parallel.document.bindings.find(binding=>binding.to.nodeId===virtual.id&&binding.to.portId===virtual.ports[3].id);
    const geometry=await native.evaluate(`(()=>{const bindings=${JSON.stringify(parallel.document.bindings.filter(binding=>binding.from.nodeId===resourceId&&binding.to.nodeId===virtual.id))};return bindings.map(binding=>{const path=[...document.querySelectorAll('.signal-edge')].find(path=>path.dataset.bindingId===binding.id),matrix=path.getScreenCTM(),a=path.getPointAtLength(0),b=path.getPointAtLength(path.getTotalLength()),start=new DOMPoint(a.x,a.y).matrixTransform(matrix),end=new DOMPoint(b.x,b.y).matrixTransform(matrix),fromHandle=document.querySelector('[data-connection-source="'+binding.from.nodeId+'"]'),toHandle=document.querySelector('[data-connection-target="'+binding.to.nodeId+'"]'),from=fromHandle.closest('.resource-block').getBoundingClientRect(),to=toHandle.closest('.resource-block').getBoundingClientRect(),fromDot=fromHandle.querySelector('.port-dot').getBoundingClientRect(),toDot=toHandle.querySelector('.port-dot').getBoundingClientRect();return{id:binding.id,path:path.getAttribute('d'),start:{x:start.x,y:start.y},end:{x:end.x,y:end.y},from:{x:from.right,y:fromDot.top+fromDot.height/2},to:{x:to.left,y:toDot.top+toDot.height/2},fromDot:{x:fromDot.left+fromDot.width/2,y:fromDot.top+fromDot.height/2},toDot:{x:toDot.left+toDot.width/2,y:toDot.top+toDot.height/2}};});})()`);
    assert.equal(geometry.length,2);for(const route of geometry){assert.ok(Math.hypot(route.start.x-route.from.x,route.start.y-route.from.y)<.65,'Resource wire starts at its border-centered dot');assert.ok(Math.hypot(route.end.x-route.to.x,route.end.y-route.to.y)<.65,'Resource wire ends at its border-centered dot');assert.ok(Math.hypot(route.fromDot.x-route.from.x,route.fromDot.y-route.from.y)<.65,'Output dot centers on the resource border');assert.ok(Math.hypot(route.toDot.x-route.to.x,route.toDot.y-route.to.y)<.65,'Input dot centers on the resource border');assert.ok(!/NaN|Infinity/.test(route.path));}
    assert.deepEqual(geometry[0].start,geometry[1].start);assert.deepEqual(geometry[0].end,geometry[1].end);assert.notEqual(geometry[0].path,geometry[1].path);assert.ok(geometry.some(route=>/[CQ]/.test(route.path)));
    await selectEdge(parallelEdge.id);await until(()=>native.evaluate(`document.querySelectorAll('.connection-port-label').length===2`),'Wire selection did not show its endpoint names');await capture('wire-actions-and-shared-anchors.png');
    await click('Disconnect','[aria-label="Connection actions"]');await until(async()=>(await snapshot()).edges===edgeCount,'Disconnect did not remove the selected route');await click('Undo');await until(async()=>(await snapshot()).edges===edgeCount+1,'Undo did not restore the disconnected route');
    await selectEdge(parallelEdge.id);await dragWire(`.connection-endpoint[data-reconnect-binding="${parallelEdge.id}"][data-reconnect-side="to"]`,`[data-connection-target="${virtual.id}"]`);
    await choosePort('Find an input',virtual.ports[2].id);await until(async()=>!(await snapshot()).panel,'Endpoint reconnection did not auto-commit');
    const reconnected=await save();assert.equal(reconnected.document.bindings.length,edgeCount+1);assert.deepEqual(reconnected.document.bindings.find(binding=>binding.id===parallelEdge.id).to,{nodeId:virtual.id,portId:virtual.ports[2].id});
    checks.push('Parallel wires have smooth distinct paths and share exact footer centers; selecting a wire shows named ports and highlights boxes; Disconnect/Undo and endpoint dragging preserve the exact binding');
    const insertionTarget=reconnected.document.bindings.find(binding=>binding.from.nodeId===resourceId),beforeInsertion=reconnected.document.bindings;
    await dropTransformation('not',insertionTarget.id);await until(()=>native.evaluate(`document.querySelectorAll('.resource-block.processing').length===1&&document.querySelectorAll('.signal-edge').length===${edgeCount+2}`),'Dropping Boolean NOT onto the SVG wire did not split the route');
    const inserted=await save(),transform=inserted.document.processingNodes[0];assert.equal(transform.operation.kind,'not');assert.ok(!transform.draft);assert.equal(inserted.document.bindings.filter(binding=>binding.to.nodeId===transform.id).length,1);assert.equal(inserted.document.bindings.filter(binding=>binding.from.nodeId===transform.id).length,1);const insertedGeometry=await transformGeometry({...transform,bindings:inserted.document.bindings});await capture('dropped-wire-transformation.png');
    assert.equal(await native.evaluate(`(()=>{const boxes=[...document.querySelectorAll('.resource-block')].map(node=>({id:node.dataset.nodeId,rect:node.getBoundingClientRect()}));return boxes.some(a=>boxes.some(b=>a.id!==b.id&&Math.min(a.rect.right,b.rect.right)-Math.max(a.rect.left,b.rect.left)>1&&Math.min(a.rect.bottom,b.rect.bottom)-Math.max(a.rect.top,b.rect.top)>1));})()`),false,'Wire insertion must keep the new transformation clear of existing blocks');
    await clickIfPresent('Close component settings','.canvas-inspector');await click('Undo');await until(()=>native.evaluate(`document.querySelectorAll('.resource-block.processing').length===0`),'One Undo did not remove the inserted transformation');const insertionUndone=await save();assert.deepEqual(insertionUndone.document.bindings,beforeInsertion);
    await click('Redo');await until(()=>native.evaluate(`document.querySelectorAll('.resource-block.processing').length===1`),'One Redo did not restore the transformation');await save();
    await click('Fit all');await save();const exactOutput=transform.outputs[0];
    state=await dragWire(`[data-connection-source="${transform.id}"][data-connection-port="${exactOutput.id}"]`,`[data-connection-target="${virtual.id}"]`,true);
    assert.equal(state.panel,null,'A named output and sole free compatible input connect without a picker.');
    const namedOutputConnected=await save(),namedOutputBinding=namedOutputConnected.document.bindings.find(binding=>binding.from.nodeId===transform.id&&binding.to.portId===virtual.ports[3].id);assert.ok(namedOutputBinding);assert.equal(namedOutputBinding.from.portId,exactOutput.id,'Dragging a named transform output preserves its exact port.');
    await capture('compact-transform-named-port-drag.png');
    await click('Undo');await until(async()=>(await snapshot()).edges===inserted.document.bindings.length,'One Undo did not remove the directly connected named port route');await save();
    checks.push('A wire drop inserts a compact Transform with exact named ports; its wires meet port centers on the outer border, dragging its named output preselects that port, and Undo/Redo remains atomic');
    const numeric=outputs.find(port=>port.descriptor.contract.delivery==='state'&&['number','integer'].includes(port.descriptor.contract.schema.type));assert.ok(numeric,'The real controller should expose a numeric axis for Multiply inference');
    await dropTransformation('scale');await until(()=>native.evaluate(`document.querySelectorAll('.resource-block.processing').length===2&&!!document.querySelector('.canvas-inspector [aria-label="Multiply by"]')`),'Dropping Multiply did not expose its on-canvas settings');
    await native.evaluate(`(()=>{const input=document.querySelector('.canvas-inspector [aria-label="Multiply by"]');input.value='10';input.dispatchEvent(new Event('change',{bubbles:true}));})()`);
    const draftTransform=await save(),multiply=draftTransform.document.processingNodes.find(node=>node.operation.kind==='scale');assert.ok(multiply.draft);assert.equal(multiply.operation.factor,10);
    const draftCompilation=(await request({command:'profile_compile',document:draftTransform.document,assignments:draftTransform.assignments})).profileCompilation;assert.ok(draftCompilation.diagnostics.some(diagnostic=>diagnostic.code==='unresolvedTransformation'&&diagnostic.location.nodeId===multiply.id));assert.notEqual(draftCompilation.readiness,'ready');
    await click('← Back to Profiles');await click(`Edit ${name}`);await until(()=>native.evaluate(`document.querySelectorAll('.resource-block.processing').length===2`),'Reopen did not retain the independent Multiply draft');const draftReopened=await saved();assert.deepEqual(draftReopened.document.processingNodes.find(node=>node.id===multiply.id),multiply);
    await clickIfPresent('Close component settings','.canvas-inspector');await click('Auto-layout');await click('Fit all');
    await dragWire(`[data-connection-source="${resourceId}"]`,`[data-connection-target="${multiply.id}"][data-connection-port="${multiply.inputs[0].id}"]`);assert.equal((await snapshot()).panel.input,multiply.inputs[0].id,'Dragging onto a named transform input preselects its exact port.');await choosePort('Find an output',`@observed:${JSON.stringify([numeric.descriptor.id,numeric.contractHash])}`);
    await until(async()=>!(await snapshot()).panel&&(await snapshot()).edges===edgeCount+3,'Selecting the numeric source did not infer the sole transformation input and auto-connect');
    const resolved=await save(),typedMultiply=resolved.document.processingNodes.find(node=>node.id===multiply.id);assert.ok(!typedMultiply.draft);assert.equal(typedMultiply.operation.factor,10);assert.deepEqual(typedMultiply.inputs[0].contract,numeric.descriptor.contract);const multiplyGeometry=await transformGeometry({...typedMultiply,bindings:resolved.document.bindings});await capture('multiply-with-inferred-input.png');
    checks.push('A standalone Multiply node stores factor10 before wiring; selecting a real numeric output infers its sole input type/unit/range and connects it in one action');
    await dropTransformation('gate');await until(()=>native.evaluate(`document.querySelectorAll('.resource-block.processing').length===3`),'Dropping Gate did not add its independent named inputs');
    const gateDraft=await save(),gate=gateDraft.document.processingNodes.find(node=>node.operation.kind==='gate');assert.ok(gate?.draft);assert.equal(gate.inputs.length,2);
    await clickIfPresent('Close component settings','.canvas-inspector');await click('Auto-layout');await click('Fit all');
    await dragWire(`[data-connection-source="${resourceId}"]`,`[data-connection-target="${gate.id}"][data-connection-port="value"]`);assert.equal((await snapshot()).panel.input,'value');await choosePort('Find an output',pin[0].profilePortId);await until(async()=>!(await snapshot()).panel,'The exact Gate value input did not resolve and connect');
    await dragWire(`[data-connection-source="${resourceId}"]`,`[data-connection-target="${gate.id}"][data-connection-port="gate"]`);assert.equal((await snapshot()).panel.input,'gate');await choosePort('Find an output',pin[0].profilePortId);await until(async()=>!(await snapshot()).panel,'The exact Gate control input did not connect');
    const gated=await save(),typedGate=gated.document.processingNodes.find(node=>node.id===gate.id),gateGeometry=await transformGeometry({...typedGate,bindings:gated.document.bindings});assert.ok(!typedGate.draft);assert.equal(gated.document.bindings.filter(binding=>binding.to.nodeId===gate.id).length,2);assert.equal(new Set(gateGeometry.edges.filter(edge=>edge.binding.to.nodeId===gate.id).map(edge=>edge.end.y)).size,2,'Different named inputs need separate border anchor centers.');await capture('gate-multiple-named-inputs.png');
    checks.push('A two-input Gate exposes separate named rows; actual drags choose value and gate exactly and each saved arrow meets its own border-centered port');
    console.log('Canvas wiring evidence: '+resolve(artifacts,'evidence.json'));
    await writeFile(resolve(artifacts, 'evidence.json'), JSON.stringify({ actualNativeCockpit: true, realPaletteDrag: true, realConnectionMouseDrags: true, profileId, resource: { id: resource.id, name: resource.name, total: observed.length, outputs: outputs.length, inputs: inputs.length }, chosenPort: boolean.descriptor.id, companion: companionEvidence,geometry,insertedGeometry,multiplyGeometry,gateGeometry,draftCompilation,multiply:typedMultiply,checks }, null, 2) + '\n');
  } catch (error) {
    await capture('failure.png').catch(() => {});
    await writeFile(resolve(artifacts, 'failure.json'), JSON.stringify({ profileId, checks, error: redact(error.message), state: await snapshot().catch(() => null) }, null, 2) + '\n'); throw error;
  } finally {
    try {
      if (dragging) await native.send('Input.dispatchDragEvent', { type: 'dragCancel', x: 10, y: 10, data: intercepted?.data ?? { items: [], dragOperationsMask: 65535 } }).catch(() => {});
      await native.send('Input.cancelDragging').catch(() => {}); await mouse('mouseReleased', { x: 10, y: 10 }, { button: 'left', buttons: 0 }).catch(() => {});
      await native.send('Input.setInterceptDrags', { enabled: false }).catch(() => {});
      if (profileId) {
        assert.ok(beforeProfiles && !beforeProfiles.some(profile => profile.id === profileId));
        await clickIfPresent('Close connection editor', '.canvas-port-picker'); await clickIfPresent('← Back to Profiles'); await clickIfPresent('Discard and continue');
        const mode = await automatic(); if (mode.manualProfileId !== beforeAutomatic.manualProfileId) await request({ command: 'profile_select', id: beforeAutomatic.manualProfileId, selectionSource: 'manual' });
        const current = await saved(); await request({ command: 'profile_delete', id: profileId, expectedRevision: current.document.revision }); await clickIfPresent('Refresh profiles');
      } else if (draftOpen) await clickIfPresent('Cancel creation');
      if (beforeProfiles) assert.deepEqual((await request({ command: 'profile_list' })).profiles.profiles, beforeProfiles, 'Acceptance must preserve every original saved profile');
      if (beforeAutomatic) { const after = await automatic(); assert.equal(after.mode, beforeAutomatic.mode); assert.equal(after.manualProfileId, beforeAutomatic.manualProfileId); assert.deepEqual(after.rules, beforeAutomatic.rules); }
    } finally { native.close(); }
  }
});
