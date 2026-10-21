// Real native Cockpit IPC and a disposable production browser companion.
// No mocked providers, fabricated input frames, direct configuration-file edits or native restart.
import test from 'node:test';
import assert from 'node:assert/strict';
import { createRequire } from 'node:module';
import { randomUUID } from 'node:crypto';
import { mkdir, writeFile } from 'node:fs/promises';
import { resolve } from 'node:path';

const require = createRequire(import.meta.url), modulePath = process.env.OSG_PLAYWRIGHT_MODULE ?? 'playwright';
const { chromium } = require(modulePath);
const { ws: RawSocket } = createRequire(require.resolve(modulePath))('playwright-core/lib/utilsBundle');
const artifacts = resolve('.research/profile-component-palette-native');
const sleep = ms => new Promise(resolve => setTimeout(resolve, ms));
const redact = value => String(value).replace(/[A-Za-z0-9_-]{43}/g, '[redacted]');

async function connectNative() {
  let endpoint = process.env.OSG_NATIVE_PAGE_WS;
  if (!endpoint) {
    const response = await fetch(`${process.env.OSG_NATIVE_CDP_URL ?? 'http://127.0.0.1:9222'}/json/list`, { signal: AbortSignal.timeout(5000) });
    endpoint = (await response.json()).find(target => target.type === 'page' && /127\.0\.0\.1:1420|localhost:1420|tauri\.localhost/.test(target.url))?.webSocketDebuggerUrl;
  }
  assert.ok(endpoint, 'The already-running native dev Cockpit must expose its page debugging target.');
  const socket = new RawSocket(endpoint, { handshakeTimeout: 8000 });
  await new Promise((resolve, reject) => { socket.once('open', resolve); socket.once('error', () => reject(new Error('Native page debugging connection failed.'))); });
  let sequence = 0; const pending = new Map();
  socket.on('error', () => {});
  socket.on('message', bytes => {
    const message = JSON.parse(bytes.toString()), operation = pending.get(message.id);
    if (!operation) return;
    clearTimeout(operation.timer); pending.delete(message.id);
    if (message.error) operation.reject(new Error(redact(message.error.message))); else operation.resolve(message.result);
  });
  socket.on('close', () => { for (const operation of pending.values()) { clearTimeout(operation.timer); operation.reject(new Error('Native page debugging connection closed.')); } pending.clear(); });
  const send = (method, params = {}) => new Promise((resolve, reject) => {
    const id = ++sequence, timer = setTimeout(() => { pending.delete(id); reject(new Error(`Native CDP ${method} timed out.`)); }, 12000);
    pending.set(id, { resolve, reject, timer }); socket.send(JSON.stringify({ id, method, params }));
  });
  return {
    send,
    async evaluate(expression) {
      const result = await send('Runtime.evaluate', { expression, returnByValue: true, awaitPromise: true });
      if (result.exceptionDetails) throw new Error(redact(result.exceptionDetails.exception?.description ?? result.exceptionDetails.text));
      return result.result.value;
    },
    close() { socket.close(); },
  };
}
async function until(operation, description, timeout = 15000) {
  const end = Date.now() + timeout; let last;
  while (Date.now() < end) { try { const value = await operation(); if (value) return value; } catch (error) { last = error; } await sleep(150); }
  throw new Error(`${description}${last ? ': ' + redact(last.message) : ''}`);
}

test('native palette authors an unassigned production companion and inserts connection transformations before first activation', { timeout: 180000 }, async () => {
  await mkdir(artifacts, { recursive: true });
  const native = await connectNative(), token = randomUUID(), name = `Palette ${token}`, peerName = `Palette peer ${token}`, errors = [], checks = [], peerFrames = [], requestFailures = [], started = Date.now();
  let browser, context, peerPage, companionId, profileId, draftOpen = false, activationAttempted = false, before, basicResource, peerResource;
  const request = async value => {
    const result = await native.evaluate(`window.__TAURI_INTERNALS__.invoke('gateway_request',{request:${JSON.stringify({ ...value, clientId: 'profile-palette-native' })}})`);
    assert.equal(result.ok, true, `${value.command}: ${redact(result.error ?? 'Native request failed')}`); return result.state;
  };
  const companions = async () => (await request({ command: 'companion_status' })).companions;
  const profileStatus = async () => (await request({ command: 'profile_status' })).profiles;
  const automatic = async () => (await request({ command: 'profile_automatic_status' })).profiles.automatic;
  const saved = async () => (await request({ command: 'profile_assignments_get', id: profileId })).profiles;
  const resources = () => until(async () => {
    const result = []; let first;
    do { const page = (await request({ command: 'profile_resources_list', offset: result.length, limit: 128 })).profileResources;
      if (!first) first = page; else if (page.revision !== first.revision || page.total !== first.total) return null;
      result.push(...page.resources);
    } while (result.length < first.total);
    return result;
  }, 'Native inventory did not settle for a complete read');
  const clickIfPresent = (label, scope = '.profiles-workspace') => native.evaluate(`(()=>{const root=document.querySelector(${JSON.stringify(scope)});const button=[...(root?.querySelectorAll('button')??[])].find(button=>(button.getAttribute('aria-label')??button.textContent.replace(/\\s+/g,' ').trim())===${JSON.stringify(label)});if(!button||button.disabled)return false;button.scrollIntoView({block:'center'});button.click();return true;})()`);
  const click = (label, scope = '.profiles-workspace') => until(() => clickIfPresent(label, scope), `UI action unavailable: ${label}`);
  const input = (selector, value, event = 'input') => native.evaluate(`(()=>{const element=document.querySelector(${JSON.stringify(selector)});if(!element)return false;element.value=${JSON.stringify(value)};element.dispatchEvent(new Event(${JSON.stringify(event)},{bubbles:true}));return true;})()`);
  const selectLabel = (label, value, scope) => until(() => native.evaluate(`(()=>{const root=document.querySelector(${JSON.stringify(scope)});const select=[...(root?.querySelectorAll('label')??[])].find(element=>element.textContent.trim().startsWith(${JSON.stringify(label)}))?.querySelector('select');if(!select||select.disabled||![...select.options].some(option=>option.value===${JSON.stringify(value)}))return false;select.value=${JSON.stringify(value)};select.dispatchEvent(new Event('change',{bubbles:true}));return true;})()`), `UI select unavailable: ${label}`);
  const choosePort = async (label, value) => {
    await until(() => native.evaluate(`(()=>{const input=[...document.querySelectorAll('.canvas-connection-panel input[role="combobox"]')].find(input=>input.getAttribute('aria-label')===${JSON.stringify(label)});if(!input||input.disabled)return false;input.focus();input.click();input.value=${JSON.stringify(value)};input.dispatchEvent(new Event('input',{bubbles:true}));return true;})()`), `Port search unavailable: ${label}`);
    await until(() => native.evaluate(`(()=>{const input=[...document.querySelectorAll('.canvas-connection-panel input[role="combobox"]')].find(input=>input.getAttribute('aria-label')===${JSON.stringify(label)}),list=document.getElementById(input?.getAttribute('aria-controls'));const option=[...(list?.querySelectorAll('[role="option"]')??[])].find(option=>option.dataset.value===${JSON.stringify(value)});if(!option||option.disabled)return false;option.click();return true;})()`), `Port result unavailable: ${label} ${value}`);
  };
  const uiGraph = () => native.evaluate(`(()=>{const root=document.querySelector('.profiles-workspace');return{nodes:[...root.querySelectorAll('.resource-block')].map(node=>({id:node.dataset.nodeId,kind:[...node.classList].find(kind=>['resource','processing','virtual','desktop','device','companion','game'].includes(kind))})),processing:root.querySelectorAll('.resource-block.processing').length,edges:root.querySelectorAll('.signal-edge').length};})()`);
  const selectNode = id => until(() => native.evaluate(`(()=>{const node=[...document.querySelectorAll('.profiles-workspace .resource-block')].find(node=>node.dataset.nodeId===${JSON.stringify(id)});if(!node)return false;node.querySelector('.block-heading').click();return true;})()`), `Graph block unavailable: ${id}`);
  const selectedNode = () => native.evaluate(`document.querySelector('.profiles-workspace .resource-block.selected')?.dataset.nodeId??null`);
  const capture = async file => { const value = await native.send('Page.captureScreenshot', { format: 'png' }); await writeFile(resolve(artifacts, file), Buffer.from(value.data, 'base64')); };
  const mouse = (type, location, extra = {}) => native.send('Input.dispatchMouseEvent', { type, ...location, ...extra });
  const collapseInspector = async () => { await mouse('mouseMoved',{x:10,y:10});await until(()=>native.evaluate(`!document.querySelector('.canvas-inspector[data-expanded="true"]')`),'Moving away did not collapse the settings rail'); };
  const center = selector => until(() => native.evaluate(`(()=>{const element=document.querySelector(${JSON.stringify(selector)});if(!element||element.disabled)return null;const r=element.getBoundingClientRect(),x=Math.round(r.left+r.width/2),y=Math.round(r.top+r.height/2);if(!r.width||!r.height||r.left<0||r.top<0||r.right>innerWidth||r.bottom>innerHeight||!element.contains(document.elementFromPoint(x,y)))return null;return{x,y};})()`), `Visible mouse target unavailable: ${selector}`);
  const dragWire = async (source, target) => {
    await native.evaluate(`document.querySelector('.canvas-stage').scrollIntoView({block:'start'});window.scrollBy(0,-150)`);await sleep(150);
    const from=await center(source),to=await center(target);
    await mouse('mouseMoved',from);await mouse('mousePressed',from,{button:'left',buttons:1,clickCount:1});
    try{for(const fraction of [.2,.5,.8,1])await mouse('mouseMoved',{x:Math.round(from.x+(to.x-from.x)*fraction),y:Math.round(from.y+(to.y-from.y)*fraction)},{button:'left',buttons:1});}
    finally{await mouse('mouseReleased',to,{button:'left',buttons:0,clickCount:1});}
    await until(()=>native.evaluate(`(()=>{const panel=document.querySelector('.canvas-connection-panel'),r=panel?.getBoundingClientRect();return !!r&&r.width>0&&r.height>0&&r.top<innerHeight&&r.bottom>0;})()`),'A real connection drag did not open the exact port picker');
  };
  const save = async () => {
    await click('Save profile');
    if (!profileId) profileId = await until(async () => (await request({ command: 'profile_list' })).profiles.profiles.find(profile => profile.name === name)?.id, 'New temporary profile was not saved');
    await until(async () => {
      const state = await saved();
      const disabled = await native.evaluate(`(()=>{const button=[...document.querySelectorAll('.profiles-workspace button')].find(button=>button.textContent.trim()==='Save profile');return button?.disabled;})()`);
      return disabled ? state : null;
    }, 'Profile save did not settle');
    return saved();
  };
  const unassigned = async () => !(await companions()).host.instances.some(instance => instance.companionId === companionId);
  const assignmentShape = state => state.host.instances.map(instance => ({ id: instance.instanceId, companionId: instance.companionId, publicationHash: instance.publicationHash })).sort((a,b) => a.id.localeCompare(b.id));
  const peerDiagnostics = async () => peerPage ? peerPage.evaluate(() => ({ visibility: document.visibilityState, focused: document.hasFocus(), status: document.querySelector('.toolbar span')?.textContent ?? document.querySelector('.empty h1')?.textContent, alerts: [...document.querySelectorAll('[role="alert"]')].map(node => node.textContent) })).catch(() => null) : null;
  const traceFrame = (direction, payload) => {
    if (peerFrames.length >= 512) return;
    try { const value = JSON.parse(String(payload)), safe = { elapsedMs: Date.now() - started, direction };
      for (const key of ['type', 'instanceId', 'publicationHash', 'sequence', 'ok', 'error']) if (['string', 'boolean'].includes(typeof value[key])) safe[key] = typeof value[key] === 'string' ? (key === 'error' ? redact(value[key]) : value[key]).slice(0,1024) : value[key];
      peerFrames.push(safe);
    } catch { peerFrames.push({ elapsedMs: Date.now() - started, direction, unparsable: true }); }
  };

  try {
    assert.equal(await native.evaluate('typeof window.__TAURI_INTERNALS__?.invoke'), 'function');
    assert.equal(await native.evaluate('document.visibilityState'), 'visible');
    const initialStatus = await profileStatus(), initialMode = await automatic(), initialCompanions = await companions(), initialProfiles = (await request({ command: 'profile_list' })).profiles.profiles;
    assert.equal(initialStatus.profileSession.owner, null, 'Start this native acceptance with no active profile.');
    assert.equal(initialStatus.profileSession.state, 'stopped', 'Start this native acceptance after any prior recovery is complete.');
    assert.equal(initialMode.mode, 'manual', 'This test preserves the existing Manual selection policy.');
    assert.equal(initialCompanions.listener.enabled, true, 'The existing companion listener must already be enabled.');
    before = { clients: initialCompanions.host.clients.map(client => client.companionId).sort(), instances: assignmentShape(initialCompanions), profiles: initialProfiles, automatic: initialMode };
    const offered = (await request({ command: 'companion_pair_offer', companion: { kind: 'browser' } })).companions;
    browser = await chromium.launch({ headless: true, ...(process.env.OSG_BROWSER_CHANNEL ? { channel: process.env.OSG_BROWSER_CHANNEL } : process.platform === 'win32' ? { channel: 'msedge' } : {}) });
    context = await browser.newContext({ ignoreHTTPSErrors: true, viewport: { width: 1280, height: 900 } });
    peerPage = await context.newPage(); peerPage.on('pageerror', error => errors.push(redact(error.message)));
    peerPage.on('requestfailed', request => requestFailures.push({ method: request.method(), resourceType: request.resourceType(), error: redact(request.failure()?.errorText ?? 'Request failed') }));
    peerPage.on('websocket', socket => { socket.on('framesent', event => traceFrame('sent', event.payload)); socket.on('framereceived', event => traceFrame('received', event.payload)); socket.on('socketerror', error => peerFrames.push({ elapsedMs: Date.now() - started, direction: 'socketerror', error: redact(error) })); });
    try { await peerPage.goto(offered.pairing.browserUrl, { timeout: 15000 }); } catch { throw Error('Temporary production companion pairing page did not load.'); }
    companionId = await until(() => peerPage.evaluate(() => { const credential = localStorage.getItem('osg-browser-credential-v2'); return credential ? JSON.parse(credential).companionId : null; }), 'Temporary production browser did not pair');
    assert.ok(!before.clients.includes(companionId));
    await request({ command: 'companion_update', companion: { companionId, name: peerName } });
    assert.equal(await unassigned(), true, 'Pairing must not create any panel assignment.');
    peerResource = await until(async () => (await resources()).find(resource => resource.resourceType === 'companion' && resource.name === peerName && resource.attributes.componentKind === 'paired-companion' && resource.attributes.companionId === companionId), 'Unassigned paired companion did not appear in native component inventory');
    assert.equal(peerResource.name, peerName);
    checks.push('Fresh production browser pairs without an assigned panel and appears as a paired-companion component');

    await click('Profiles', 'nav[aria-label="Main navigation"]');
    await until(() => native.evaluate(`!![...document.querySelectorAll('.profiles-workspace h1')].find(heading=>heading.textContent==='Profiles')`), 'Open Profiles list before starting acceptance');
    await click('Add profile'); draftOpen = true;
    await until(() => native.evaluate(`!!document.querySelector('[aria-label="Component palette"]')`), 'New profile did not open the component palette');
    await click('Edit profile name');
    assert.equal(await input('.profile-name-input', name), true);
    await native.evaluate(`document.querySelector('.profile-name-input').dispatchEvent(new KeyboardEvent('keydown',{key:'Enter',bubbles:true,cancelable:true}))`);
    await save(); draftOpen = false;
    assert.ok(!before.profiles.some(profile => profile.id === profileId));
    assert.equal((await saved()).document.resources.length, 0);
    assert.equal(await native.evaluate(`!!document.querySelector('.inventory-picker')||!!document.querySelector('[aria-label="Visible transformation"]')||!!document.querySelector('[aria-label="Desktop output presets"]')`), false);

    await input('.component-palette [aria-label="Search components"]', peerName);
    await until(() => native.evaluate(`document.querySelectorAll('.component-palette button[data-resource-id]').length===1`), 'Palette search did not isolate the temporary companion');
    await until(() => native.evaluate(`document.querySelector('.component-palette button[data-resource-id]')?.dataset.resourceId===${JSON.stringify(peerResource.id)}`), 'Palette did not show the exact paired resource identity');
    await input('.component-palette [aria-label="Search components"]', '');
    assert.equal(await native.evaluate(`!![...document.querySelectorAll('.component-palette button[data-resource-id]')].find(button=>button.dataset.resourceId===${JSON.stringify(peerResource.id)}&&button.disabled)`), false);
    const legacyCompanionIds = (await resources()).filter(resource => resource.resourceType === 'companion' && resource.attributes.componentKind !== 'paired-companion').map(resource => resource.id);
    assert.equal(await native.evaluate(`Array.from(document.querySelectorAll('.component-palette button[data-resource-id]')).some(button=>${JSON.stringify(legacyCompanionIds)}.includes(button.dataset.resourceId))`), false, 'Legacy assigned-panel instances must not appear as separately addable companion components');
    await click('Add virtual controller', '.component-palette');
    assert.equal(await selectedNode(), null, 'Adding a component must not select it or open its inspector.');
    let state = await save(); const virtualId = state.document.virtualOutputs[0].id;
    assert.equal(state.document.processingNodes.length, 0); assert.equal(state.document.bindings.length, 0);
    await selectNode(virtualId);
    await click('Add keyboard and mouse', '.component-palette');
    assert.equal(await selectedNode(), virtualId, 'Adding another output must preserve the selected block.');
    state = await save(); const desktopId = state.document.desktopOutputs[0].id;
    assert.equal(state.document.processingNodes.length, 0); assert.equal(state.document.bindings.length, 0); assert.equal(state.document.desktopOutputs[0].actions.length, 1);
    assert.equal(await native.evaluate(`document.querySelector('.component-palette [aria-label="Add keyboard and mouse"]')?.disabled`), true);
    await selectNode(desktopId);
    basicResource = (await resources()).find(resource => resource.resourceType === 'host-controller') ?? (await resources()).find(resource => resource.resourceType === 'osg-module');
    if (basicResource) {
      await until(() => native.evaluate(`(()=>{const button=[...document.querySelectorAll('.component-palette [data-resource-id]')].find(button=>button.dataset.resourceId===${JSON.stringify(basicResource.id)});if(!button||button.disabled)return false;button.click();return true;})()`), 'Basic hardware component was not available in the palette');
      assert.equal(await selectedNode(), desktopId, 'Adding hardware must preserve the selected output.');
      state = await save(); const local = state.assignments.find(assignment => assignment.sourceId === basicResource.id); assert.ok(local); assert.deepEqual(state.document.resources.find(resource => resource.id === local.resourceId).ports, []);
    }
    checks.push('Palette search and output/resource buttons add only the requested component, without routes or implicit transformations');

    await until(() => native.evaluate(`(()=>{const button=[...document.querySelectorAll('.component-palette [data-resource-id]')].find(button=>button.dataset.resourceId===${JSON.stringify(peerResource.id)});if(!button||button.disabled)return false;button.click();return true;})()`), 'Temporary companion component could not be added');
    assert.equal(await selectedNode(), desktopId, 'Adding a companion must preserve the selected output.');
    state = await save(); const local = state.assignments.find(assignment => assignment.sourceId === peerResource.id), companionNodeId = local.resourceId;
    assert.deepEqual(state.document.resources.find(resource => resource.id === companionNodeId).ports, []); assert.deepEqual(state.document.companionAssignments, []); assert.equal(await unassigned(), true);
    await selectNode(companionNodeId);
    await until(() => native.evaluate(`document.querySelector('[aria-label="Profile companion panel selection"]')?.textContent.includes('No panel is selected for this block.')`), 'Selecting the companion did not open its unconfigured panel inspector');
    const catalogue = (await request({ command: 'profile_companion_catalogue' })).profileCompanionAuthoring;
    let selectedPanel, selectedPort;
    for (const panel of catalogue.panels) {
      const page = (await request({ command: 'profile_companion_inspect', resourceId: peerResource.id, publicationHash: panel.publicationHash, offset: 0, limit: 64 })).profileCompanionAuthoring;
      const port = page.ports.find(port => port.descriptor.direction === 'output' && port.descriptor.contract.delivery === 'state' && port.descriptor.contract.schema.type === 'boolean');
      if (port) { selectedPanel = panel; selectedPort = port; break; }
    }
    assert.ok(selectedPanel && selectedPort, 'An existing published panel with a real boolean control is required.');
    await selectLabel('Published panel', selectedPanel.publicationHash, '[aria-label="Profile companion panel selection"]');
    await click('Use panel in profile', '[aria-label="Profile companion panel selection"]');
    await click('Inspect exact panel ports', '[aria-label="Profile companion panel selection"]');
    await selectLabel('Port group', 'output', '[aria-label="Profile companion panel selection"]');
    await until(() => native.evaluate(`(()=>{const row=[...document.querySelectorAll('.companion-authoring .panel-ports li')].find(row=>row.querySelector('code')?.textContent===${JSON.stringify(selectedPort.descriptor.id)});const button=[...(row?.querySelectorAll('button')??[])].find(button=>button.textContent.trim()==='Add control output');if(!button||button.disabled)return false;button.click();return true;})()`), 'The selected real companion output could not be added');
    state = await save(); const sourcePortId = state.assignments.find(assignment => assignment.resourceId === companionNodeId).ports.find(port => port.observedPortId === selectedPort.descriptor.id).profilePortId;
    assert.equal(state.document.companionAssignments[0].publicationHash, selectedPanel.publicationHash); assert.equal(await unassigned(), true);
    checks.push('Palette additions preserve selection; explicitly selecting the unassigned companion opens panel configuration without assigning it natively');

    await collapseInspector();await click('Auto-layout');await click('Fit all');
    await dragWire(`[data-connection-source="${companionNodeId}"]`,`[data-connection-target="${virtualId}"]`);
    assert.deepEqual(await native.evaluate(`(()=>{const panel=document.querySelector('.canvas-connection-panel');return{source:panel.querySelector('[aria-label="Source block"]').value,target:panel.querySelector('[aria-label="Destination block"]').value};})()`),{source:companionNodeId,target:virtualId});
    for (const [label, value] of [['Find an output', sourcePortId], ['Find an input', state.document.virtualOutputs[0].ports[0].id]]) await choosePort(label, value);
    await click('Create exact route', '.canvas-connection-panel');
    await until(()=>native.evaluate(`!document.querySelector('.canvas-connection-panel')`),'Creating the exact route did not close the port picker');
    await collapseInspector();
    await click('Fit all');
    await until(async () => (await uiGraph()).edges === 1, 'The explicit companion-to-virtual route was not drawn');
    const hit = await until(() => native.evaluate(`(()=>{const path=document.querySelector('.signal-edge-hit'),stage=document.querySelector('.canvas-stage');if(!path||!stage)return null;const bounds=stage.getBoundingClientRect(),length=path.getTotalLength(),matrix=path.getScreenCTM();for(const fraction of [.5,.35,.65,.2,.8]){const point=path.getPointAtLength(length*fraction),screen=new DOMPoint(point.x,point.y).matrixTransform(matrix);if(screen.x>bounds.left&&screen.x<bounds.right&&screen.y>bounds.top&&screen.y<bounds.bottom&&document.elementFromPoint(screen.x,screen.y)?.closest('.signal-edge-hit')===path)return{x:screen.x,y:screen.y};}return null;})()`), 'The actual SVG connection did not expose a visible hit target');
    await native.send('Input.dispatchMouseEvent', { type: 'mouseMoved', x: hit.x, y: hit.y });
    await native.send('Input.dispatchMouseEvent', { type: 'mousePressed', x: hit.x, y: hit.y, button: 'left', clickCount: 1 });
    await native.send('Input.dispatchMouseEvent', { type: 'mouseReleased', x: hit.x, y: hit.y, button: 'left', clickCount: 1 });
    await until(() => native.evaluate(`!!document.querySelector('.connection-dialog[open]')`), 'Clicking the real SVG connection did not open transformations');
    await selectLabel('Transformation', 'identity', '.connection-dialog');
    await click('Insert transformation', '.connection-dialog');
    await until(async () => { const graph = await uiGraph(); return graph.processing === 1 && graph.edges === 2; }, 'Transformation insertion was not atomic').catch(async error => { console.error('Actual inserted graph', await uiGraph()); throw error; });
    await click('Undo'); await until(async () => { const graph = await uiGraph(); return graph.processing === 0 && graph.edges === 1; }, 'One Undo did not restore the original connection');
    await click('Redo'); await until(async () => { const graph = await uiGraph(); return graph.processing === 1 && graph.edges === 2; }, 'One Redo did not restore the inserted transformation');
    state = await save(); assert.equal(state.document.processingNodes[0].operation.kind, 'identity'); assert.equal(state.document.bindings.length, 2);
    await collapseInspector();
    await click('Fit all'); state = await save(); await native.evaluate(`document.querySelector('.profile-canvas-workspace').scrollIntoView({block:'start'})`); await capture('connection-transformation.png');
    checks.push('Actual SVG hit testing opens transformations; Identity replaces one edge with two in a single Undo/Redo edit');

    // Exercise activation without acquiring a controller output or sending a key.
    // Keep the real companion-to-identity route so native baseline acquisition remains necessary.
    for (const nodeId of [virtualId, desktopId, ...(basicResource ? [state.assignments.find(assignment => assignment.sourceId === basicResource.id).resourceId] : [])]) { await selectNode(nodeId); await click('Remove block and its routes', '.canvas-inspector'); }
    state = await save(); assert.equal(state.document.resources.length, 1); assert.equal(state.document.virtualOutputs.length, 0); assert.equal(state.document.desktopOutputs?.length ?? 0, 0); assert.equal(state.document.processingNodes.length, 1); assert.equal(state.document.bindings.length, 1);
    const savedGraph = state.document; await click('← Back to Profiles'); await click(`Edit ${name}`); state = await saved(); assert.deepEqual(state.document, savedGraph);
    assert.equal(await unassigned(), true);
    assert.equal((await peerDiagnostics()).visibility, 'visible', 'The real companion page must be visible while supplying its activation baseline');
    activationAttempted = true; await click('Activate profile', '[aria-label="Runtime profile status"]');
    const activation = await until(async () => { const session = (await profileStatus()).profileSession; return session.owner?.profileId === profileId ? { active: true } : session.state === 'recovery-needed' || session.state === 'faulted' ? { failure: session.fault ?? session.state } : null; }, 'The temporary profile did not activate');
    assert.equal(activation.active, true, activation.failure);
    await until(async () => (await companions()).host.instances.some(instance => instance.companionId === companionId && instance.publicationHash === selectedPanel.publicationHash && instance.connected === true), 'Activation did not provision the first actual production companion panel and baseline', 30000);
    await click('Deactivate current profile', '[aria-label="Runtime profile status"]');
    await until(async () => (await profileStatus()).profileSession.owner === null && await unassigned(), 'Deactivate did not restore the temporary peer to no panel assignment', 30000);
    checks.push('Saved/reopened companion-only profile activates its first native panel assignment and Deactivate restores the original unassigned state');
    assert.deepEqual(errors, []);
    await writeFile(resolve(artifacts, 'evidence.json'), JSON.stringify({ actualNativeCockpit: true, rawCdp: true, productionBrowser: true, profileId, companionId, pairedResourceId: peerResource.id, publicationHash: selectedPanel.publicationHash, checks, errors, requestFailures, peer: await peerDiagnostics(), peerFrames }, null, 2) + '\n');
  } catch (error) { console.error('Native palette failure:', redact(error.message)); await capture('failure.png').catch(() => {}); await writeFile(resolve(artifacts, 'failure.json'), JSON.stringify({ profileId, companionId, checks, errors, requestFailures, peer: await peerDiagnostics(), peerFrames, native: await profileStatus().catch(() => null), failure: redact(error.message) }, null, 2) + '\n'); throw error; }
  finally {
    try {
      if (before) {
        const activeOwner = (await profileStatus()).profileSession.owner;
        if (activeOwner?.profileId === profileId) { await request({ command: 'profile_stop' }); await until(async () => (await profileStatus()).profileSession.owner === null, 'Temporary profile did not deactivate during cleanup'); }
        assert.ok(!activeOwner || activeOwner.profileId === profileId, 'Never stop a profile started outside this acceptance.');
        const session = (await profileStatus()).profileSession;
        if (activationAttempted && session.owner === null && session.state === 'recovery-needed') await request({ command: 'profile_recover' });
      }
      if (profileId) {
        assert.ok(!before.profiles.some(profile => profile.id === profileId));
        await native.evaluate(`document.querySelector('.connection-dialog[open] button[aria-label="Close connection transformations"]')?.click()`);
        await clickIfPresent('← Back to Profiles');
        await clickIfPresent('Discard and continue');
        await request({ command: 'profile_select', id: before.automatic.manualProfileId, selectionSource: 'manual' });
        const current = await saved(); await request({ command: 'profile_delete', id: profileId, expectedRevision: current.document.revision });
        await clickIfPresent('Refresh profiles');
      } else if (draftOpen) await click('Cancel creation');
      if (!companionId && peerPage) companionId = await peerPage.evaluate(() => { const credential = localStorage.getItem('osg-browser-credential-v2'); return credential ? JSON.parse(credential).companionId : null; }).catch(() => null);
      if (context) await context.close();
      if (browser) await browser.close();
      if (companionId) {
        assert.ok(before && !before.clients.includes(companionId));
        if ((await companions()).host.clients.some(client => client.companionId === companionId)) await request({ command: 'companion_revoke', companion: { companionId } });
      }
      if (before) {
        const finalCompanions = await companions(), finalMode = await automatic();
        assert.deepEqual(finalCompanions.host.clients.map(client => client.companionId).sort(), before.clients);
        assert.deepEqual(assignmentShape(finalCompanions), before.instances);
        assert.deepEqual((await request({ command: 'profile_list' })).profiles.profiles, before.profiles);
        assert.equal(finalMode.mode, before.automatic.mode); assert.equal(finalMode.manualProfileId, before.automatic.manualProfileId); assert.deepEqual(finalMode.rules, before.automatic.rules);
      }
    } finally { native.close(); }
  }
});
