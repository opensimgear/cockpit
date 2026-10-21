// Real mouse drags and native IPC against the already-running dev Cockpit.
// Discovery is read-only; creating a route atomically pins its chosen contract.
import test from 'node:test';
import assert from 'node:assert/strict';
import { randomUUID } from 'node:crypto';
import { mkdir, writeFile } from 'node:fs/promises';
import { resolve } from 'node:path';

const artifacts = resolve('.research/profile-wiring-native');
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

test('native dropped blocks discover every port and wire in either drag direction through the side panel', { timeout: 150000 }, async () => {
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
  const snapshot = () => native.evaluate(`(()=>{const stage=document.querySelector('.canvas-stage'),rect=stage?.getBoundingClientRect(),panel=document.querySelector('.canvas-connection-panel'),pr=panel?.getBoundingClientRect();return{stage:rect?{left:rect.left,top:rect.top,width:rect.width,height:rect.height}:null,nodes:[...document.querySelectorAll('.canvas-stage .resource-block')].map(node=>({id:node.dataset.nodeId,inputs:Number(node.querySelector('[data-connection-target] .port-group-heading > span')?.textContent),outputs:Number(node.querySelector('[data-connection-source] .port-group-heading > span')?.textContent)})),edges:document.querySelectorAll('.canvas-stage .signal-edge').length,panel:panel?{source:panel.querySelector('[aria-label="Source block"]').value,target:panel.querySelector('[aria-label="Destination block"]').value,output:panel.querySelector('input[aria-label="Find an output"]')?.dataset.value,input:panel.querySelector('input[aria-label="Find an input"]')?.dataset.value,visible:pr.width>0&&pr.height>0&&pr.top<innerHeight&&pr.bottom>0,right:pr.right,width:pr.width,insideCanvas:!!panel.closest('.canvas-layout')}:null};})()`);
  const stageVisible = async () => { await native.evaluate(`document.querySelector('.canvas-stage').scrollIntoView({block:'start'});window.scrollBy(0,-150)`); await sleep(150); return snapshot(); };
  const mouse = (type, location, extra = {}) => native.send('Input.dispatchMouseEvent', { type, ...location, ...extra });
  const center = selector => until(() => native.evaluate(`(()=>{const element=document.querySelector(${JSON.stringify(selector)});if(!element||element.disabled)return null;const r=element.getBoundingClientRect(),x=Math.round(r.left+r.width/2),y=Math.round(r.top+r.height/2);if(!r.width||!r.height||r.left<0||r.top<0||r.right>innerWidth||r.bottom>innerHeight||!element.contains(document.elementFromPoint(x,y)))return null;return{x,y};})()`), `Visible mouse target unavailable: ${selector}`);
  const background = () => until(() => native.evaluate(`(()=>{const stage=document.querySelector('.canvas-stage'),r=stage.getBoundingClientRect();for(const fy of [.15,.3,.8,.6])for(const fx of [.1,.5,.85]){const x=Math.round(r.left+r.width*fx),y=Math.round(r.top+r.height*fy),hit=document.elementFromPoint(x,y);if(x>0&&y>0&&x<innerWidth&&y<innerHeight&&hit&&stage.contains(hit)&&!hit.closest('[data-node-id],.signal-edge-hit,.canvas-inspector,.canvas-connection-panel,button,input,select'))return{x,y};}return null;})()`), 'Visible blank canvas is unavailable');
  const dragWire = async (source, target) => {
    await stageVisible(); const from = await center(source), to = await center(target);
    await mouse('mouseMoved', from); await mouse('mousePressed', from, { button: 'left', buttons: 1, clickCount: 1 });
    for (const fraction of [.2, .5, .8, 1]) await mouse('mouseMoved', point(from.x + (to.x - from.x) * fraction, from.y + (to.y - from.y) * fraction), { button: 'left', buttons: 1 });
    await mouse('mouseReleased', to, { button: 'left', buttons: 0, clickCount: 1 });
    return until(async () => { const state = await snapshot(); return state.panel?.visible && state.panel.right <= state.stage.left + state.stage.width + 2 ? state : null; }, 'A real connection drag did not open the visible side panel');
  };
  const portChoices = async label => {
    await until(() => native.evaluate(`(()=>{const input=document.querySelector(${JSON.stringify(`.canvas-connection-panel input[aria-label="${label}"]`)});if(!input||input.disabled)return false;input.focus();input.click();return true;})()`), `Port search unavailable: ${label}`);
    return until(() => native.evaluate(`(()=>{const input=document.querySelector(${JSON.stringify(`.canvas-connection-panel input[aria-label="${label}"]`)}),list=document.getElementById(input?.getAttribute('aria-controls'));return list?[...list.querySelectorAll('[role="option"]')].map(option=>({value:option.dataset.value,text:option.textContent})):null;})()`), `Port dropdown unavailable: ${label}`);
  };
  const search = async (label, value) => {
    await portChoices(label);
    await native.evaluate(`(()=>{const input=document.querySelector(${JSON.stringify(`.canvas-connection-panel input[aria-label="${label}"]`)});input.value=${JSON.stringify(value)};input.dispatchEvent(new Event('input',{bubbles:true}));})()`);
  };
  const choosePort = async (label, value) => {
    await search(label, value);
    const position = await until(() => native.evaluate(`(()=>{const input=document.querySelector(${JSON.stringify(`.canvas-connection-panel input[aria-label="${label}"]`)}),list=document.getElementById(input?.getAttribute('aria-controls'));const option=[...(list?.querySelectorAll('[role="option"]')??[])].find(option=>option.dataset.value===${JSON.stringify(value)});if(!option||option.disabled)return null;option.scrollIntoView({block:'nearest'});const r=option.getBoundingClientRect(),x=Math.round(r.left+r.width/2),y=Math.round(r.top+r.height/2);return r.width&&r.height&&option.contains(document.elementFromPoint(x,y))?{x,y}:null;})()`), `Visible port result unavailable: ${label} ${value}`);
    await mouse('mouseMoved', position); await mouse('mousePressed', position, { button: 'left', buttons: 1, clickCount: 1 }); await mouse('mouseReleased', position, { button: 'left', buttons: 0, clickCount: 1 });
    await until(() => native.evaluate(`document.querySelector(${JSON.stringify(`.canvas-connection-panel input[aria-label="${label}"]`)})?.dataset.value===${JSON.stringify(value)}`), `Chosen port was not retained: ${label}`);
  };
  const capture = async filename => { const result = await native.send('Page.captureScreenshot', { format: 'png' }); await writeFile(resolve(artifacts, filename), Buffer.from(result.data, 'base64')); };
  native.on('Input.dragIntercepted', event => { intercepted = event; });
  try {
    await native.send('Page.reload', { ignoreCache: true });
    await until(() => native.evaluate('typeof window.__TAURI_INTERNALS__?.invoke === "function"'), 'Native cockpit IPC did not finish starting');
    assert.equal(await native.evaluate('document.visibilityState'), 'visible');
    await native.send('Input.cancelDragging'); await mouse('mouseReleased', { x: 10, y: 10 }, { button: 'left', buttons: 0 });
    beforeProfiles = await until(async () => (await request({ command: 'profile_list' })).profiles.profiles, 'Native gateway did not become ready'); beforeAutomatic = await automatic();
    const inventory = (await request({ command: 'profile_resources_list', offset: 0, limit: 128 })).profileResources;
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
    await clickIfPresent('← Back to Profiles'); await clickIfPresent('Discard and continue');
    await click('Add profile'); draftOpen = true;
    await click('Edit profile name');
    await native.evaluate(`(()=>{const input=document.querySelector('.profile-name-input');input.value=${JSON.stringify(name)};input.dispatchEvent(new Event('input',{bubbles:true}));input.dispatchEvent(new KeyboardEvent('keydown',{key:'Enter',bubbles:true,cancelable:true}));})()`);
    await save(); draftOpen = false;
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
    assert.equal(state.panel.source, resourceId); assert.equal(state.panel.target, virtual.id); assert.equal(state.panel.insideCanvas, true);
    assert.ok(state.panel.right <= state.stage.left + state.stage.width + 2); assert.ok(state.panel.width > 250);
    assert.equal((await portChoices('Find an output')).length, Math.min(outputs.length, 200)); assert.equal((await portChoices('Find an input')).length, Math.min(virtual.ports.length, 200));
    assert.equal(await native.evaluate(`document.querySelectorAll('.canvas-connection-panel select[aria-label="Exact source output"],.canvas-connection-panel select[aria-label="Exact destination input / action"]').length`), 0, 'Port search and selection share one combobox per direction.');
    assert.equal(await native.evaluate(`!!document.querySelector('.canvas-connection-panel button')&&[...document.querySelectorAll('.canvas-connection-panel button')].some(button=>button.textContent.trim()==='Use this exact port')`), false);
    await capture('output-to-input-side-panel.png');
    const cancelPoint = await background();
    await mouse('mouseMoved', cancelPoint); await mouse('mousePressed', cancelPoint, { button: 'left', buttons: 1, clickCount: 1 }); await mouse('mouseReleased', cancelPoint, { button: 'left', buttons: 0, clickCount: 1 });
    await until(async () => !(await snapshot()).panel, 'Clicking blank canvas did not cancel the connection panel');
    assert.equal(await native.evaluate(`document.querySelectorAll('.pending-wire,.resource-block.selected').length`), 0);
    assert.equal((await snapshot()).edges, 0); assert.equal(await saveDisabled(), true);
    assert.deepEqual((await saved()).document, baseline.document); assert.deepEqual((await saved()).assignments, baseline.assignments);
    checks.push('Output-to-input mouse drag opens an on-canvas side panel; clicking blank canvas cancels it without changing the graph or undo history');

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
    await choosePort('Find an output', observedKey);
    await search('Find an input', virtual.ports[0].id);
    await until(async () => (await portChoices('Find an input')).some(option => option.value === virtual.ports[0].id), 'Searching the exact input did not retain its selectable result');
    await choosePort('Find an input', virtual.ports[0].id);
    await search('Find an output', `no-matching-port-${randomUUID()}`);
    await until(() => native.evaluate(`document.querySelector('.canvas-connection-panel .port-count')?.textContent.includes('No matching ports.')&&document.querySelector('input[aria-label="Find an output"]').dataset.value===''&&[...document.querySelectorAll('.canvas-connection-panel button')].some(button=>button.textContent.trim()==='Create exact route'&&button.disabled)`), 'Editing the search retained a stale selection or hid the empty result state');
    await native.send('Input.dispatchKeyEvent', { type: 'keyDown', key: 'Escape', code: 'Escape', windowsVirtualKeyCode: 27 }); await native.send('Input.dispatchKeyEvent', { type: 'keyUp', key: 'Escape', code: 'Escape', windowsVirtualKeyCode: 27 });
    assert.ok((await snapshot()).panel, 'Escape should close the search dropdown before closing the connection panel');
    assert.equal(await native.evaluate(`document.querySelector('input[aria-label="Find an output"]').getAttribute('aria-expanded')`), 'false');
    await choosePort('Find an output', observedKey);
    await click('Create exact route', '.canvas-connection-panel');
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
      await until(() => native.evaluate(`(()=>{const button=document.querySelector('.component-palette [data-resource-id="${companion.id}"]');if(!button||button.disabled)return false;button.click();return true;})()`), 'The existing paired companion could not be added from the palette');
      let companionState = await save(); const companionNodeId = companionState.assignments.find(assignment => assignment.sourceId === companion.id).resourceId;
      assert.deepEqual(companionState.document.resources.find(node => node.id === companionNodeId).ports, []);
      await native.evaluate(`document.querySelector('.resource-block[data-node-id="${companionNodeId}"] .block-heading').click()`);
      await until(() => native.evaluate(`(()=>{const root=document.querySelector('[aria-label="Profile companion panel selection"]'),select=[...(root?.querySelectorAll('label')??[])].find(label=>label.textContent.trim().startsWith('Panel'))?.querySelector('select');if(!select||select.disabled||![...select.options].some(option=>option.value===${JSON.stringify(panel.publicationHash)}))return false;select.value=${JSON.stringify(panel.publicationHash)};select.dispatchEvent(new Event('change',{bubbles:true}));return true;})()`), 'The companion panel picker did not offer the exact compatible retained publication');
      await click('Use panel in profile', '[aria-label="Profile companion panel selection"]');
      const expectedOutputs = ports.filter(port => port.descriptor.direction === 'output').length, expectedInputs = ports.filter(port => port.descriptor.direction === 'input').length;
      await until(async () => (await snapshot()).nodes.some(node => node.id === companionNodeId && node.outputs === expectedOutputs && node.inputs === expectedInputs), 'Selecting the companion panel did not automatically discover its exact retained ports');
      await mouse('mouseMoved', { x: 10, y: 10 }); await click('Auto-layout'); await click('Fit all'); companionState = await save();
      assert.deepEqual(companionState.document.resources.find(node => node.id === companionNodeId).ports, []);
      assert.equal(companionState.document.companionAssignments.find(assignment => assignment.resourceId === companionNodeId).publicationHash, panel.publicationHash);
      await dragWire(`[data-connection-source="${companionNodeId}"]`, `[data-connection-target="${virtual.id}"]`);
      const companionOutputs = await portChoices('Find an output');
      assert.equal(companionOutputs.length, Math.min(expectedOutputs, 200));
      if (expectedOutputs <= 200) for (const port of ports.filter(port => port.descriptor.direction === 'output')) assert.ok(companionOutputs.some(option => option.value === `@observed:${JSON.stringify([port.descriptor.id, port.contractHash])}`));
      await choosePort('Find an output', `@observed:${JSON.stringify([output.descriptor.id, output.contractHash])}`); await choosePort('Find an input', virtual.ports[1].id);
      await capture('companion-panel-wiring.png'); await click('Create exact route', '.canvas-connection-panel');
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
      checks.push('Selecting an existing retained companion panel automatically discovers its own ports; direct wiring pins one selected control and survives one Undo/Redo, Save and reopen');
    }
    await writeFile(resolve(artifacts, 'evidence.json'), JSON.stringify({ actualNativeCockpit: true, realPaletteDrag: true, realConnectionMouseDrags: true, profileId, resource: { id: resource.id, name: resource.name, total: observed.length, outputs: outputs.length, inputs: inputs.length }, chosenPort: boolean.descriptor.id, companion: companionEvidence, checks }, null, 2) + '\n');
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
        await clickIfPresent('Close connection editor', '.canvas-connection-panel'); await clickIfPresent('← Back to Profiles'); await clickIfPresent('Discard and continue');
        const mode = await automatic(); if (mode.manualProfileId !== beforeAutomatic.manualProfileId) await request({ command: 'profile_select', id: beforeAutomatic.manualProfileId, selectionSource: 'manual' });
        const current = await saved(); await request({ command: 'profile_delete', id: profileId, expectedRevision: current.document.revision }); await clickIfPresent('Refresh profiles');
      } else if (draftOpen) await clickIfPresent('Cancel creation');
      if (beforeProfiles) assert.deepEqual((await request({ command: 'profile_list' })).profiles.profiles, beforeProfiles, 'Acceptance must preserve every original saved profile');
      if (beforeAutomatic) { const after = await automatic(); assert.equal(after.mode, beforeAutomatic.mode); assert.equal(after.manualProfileId, beforeAutomatic.manualProfileId); assert.deepEqual(after.rules, beforeAutomatic.rules); }
    } finally { native.close(); }
  }
});
