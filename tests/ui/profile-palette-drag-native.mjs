// Real HTML5 palette drags in the already-running native dev Cockpit.
// Chromium receives real mouse input and intercepts its resulting native drag;
// no DataTransfer construction, synthetic DragEvents or mocked native calls.
import test from 'node:test';
import assert from 'node:assert/strict';
import { createRequire } from 'node:module';
import { randomUUID } from 'node:crypto';
import { mkdir, writeFile } from 'node:fs/promises';
import { resolve } from 'node:path';

const require = createRequire(import.meta.url), modulePath = process.env.OSG_PLAYWRIGHT_MODULE ?? 'playwright';
const { ws: RawSocket } = createRequire(require.resolve(modulePath))('playwright-core/lib/utilsBundle');
const artifacts = resolve('.research/profile-palette-drag-native');
const sleep = milliseconds => new Promise(resolve => setTimeout(resolve, milliseconds));
const redact = value => String(value).replace(/[A-Za-z0-9_-]{43}/g, '[redacted]');
const near = (actual, expected, message) => assert.ok(Math.abs(actual - expected) < .05, `${message}: expected ${expected}, received ${actual}`);
// Native DragEvent client coordinates are integral CSS pixels in WebView2.
const pixelPoint = (x, y) => ({ x: Math.round(x), y: Math.round(y) });

async function until(operation, description, timeout = 15000) {
  const end = Date.now() + timeout; let last;
  while (Date.now() < end) { try { const value = await operation(); if (value) return value; } catch (error) { last = error; } await sleep(100); }
  throw new Error(`${description}${last ? ': ' + redact(last.message) : ''}`);
}

async function connectNative() {
  let endpoint = process.env.OSG_NATIVE_PAGE_WS;
  if (!endpoint) {
    const response = await fetch(`${process.env.OSG_NATIVE_CDP_URL ?? 'http://127.0.0.1:9222'}/json/list`, { signal: AbortSignal.timeout(5000) });
    endpoint = (await response.json()).find(target => target.type === 'page' && /127\.0\.0\.1:1420|localhost:1420|tauri\.localhost/.test(target.url))?.webSocketDebuggerUrl;
  }
  assert.ok(endpoint, 'The already-running native dev Cockpit must expose its page debugging target.');
  const socket = new RawSocket(endpoint, { handshakeTimeout: 8000 });
  await new Promise((resolve, reject) => { socket.once('open', resolve); socket.once('error', () => reject(new Error('Native page debugging connection failed.'))); });
  let sequence = 0; const pending = new Map(), listeners = new Map();
  socket.on('error', () => {});
  socket.on('message', bytes => {
    const message = JSON.parse(bytes.toString());
    if (message.method) { for (const listener of listeners.get(message.method) ?? []) listener(message.params); return; }
    const operation = pending.get(message.id); if (!operation) return;
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
    on(method, callback) { listeners.set(method, [...(listeners.get(method) ?? []), callback]); },
    async evaluate(expression) {
      const result = await send('Runtime.evaluate', { expression, returnByValue: true, awaitPromise: true });
      if (result.exceptionDetails) throw new Error(redact(result.exceptionDetails.exception?.description ?? result.exceptionDetails.text));
      return result.result.value;
    },
    close() { socket.close(); },
  };
}

test('native HTML5 palette drags preview and persist exact canvas placement without selecting additions', { timeout: 120000 }, async () => {
  await mkdir(artifacts, { recursive: true });
  const native = await connectNative(), name = `Palette drag ${randomUUID()}`, checks = [], evidence = [];
  let profileId, beforeProfiles, beforeAutomatic, draftOpen = false, intercepted, dragging = false;
  const request = async value => {
    const result = await native.evaluate(`window.__TAURI_INTERNALS__.invoke('gateway_request',{request:${JSON.stringify({ ...value, clientId: 'profile-palette-drag-native' })}})`);
    assert.equal(result.ok, true, `${value.command}: ${redact(result.error ?? 'Native request failed')}`); return result.state;
  };
  const saved = async () => (await request({ command: 'profile_assignments_get', id: profileId })).profiles;
  const automatic = async () => (await request({ command: 'profile_automatic_status' })).profiles.automatic;
  const clickIfPresent = (label, scope = '.profiles-workspace') => native.evaluate(`(()=>{const root=document.querySelector(${JSON.stringify(scope)});const button=[...(root?.querySelectorAll('button')??[])].find(button=>(button.getAttribute('aria-label')??button.textContent.replace(/\\s+/g,' ').trim())===${JSON.stringify(label)});if(!button||button.disabled)return false;button.scrollIntoView({block:'nearest'});button.click();return true;})()`);
  const click = (label, scope) => until(() => clickIfPresent(label, scope), `UI action unavailable: ${label}`);
  const snapshot = () => native.evaluate(`(()=>{const stage=document.querySelector('.canvas-stage'),world=document.querySelector('.canvas-world'),preview=document.querySelector('[aria-label="Component drop preview"]');const rect=stage.getBoundingClientRect(),matrix=new DOMMatrix(getComputedStyle(world).transform);return{stage:{left:rect.left,top:rect.top,width:rect.width,height:rect.height},view:{x:matrix.e,y:matrix.f,zoom:matrix.a},nodes:[...document.querySelectorAll('.canvas-stage .resource-block[data-node-id]')].map(node=>({id:node.dataset.nodeId,x:parseFloat(node.style.left),y:parseFloat(node.style.top),width:parseFloat(node.style.width),height:parseFloat(node.style.height),selected:node.classList.contains('selected')})),inspector:document.querySelector('.canvas-inspector h3')?.textContent??null,preview:preview?{x:Number(preview.dataset.previewX),y:Number(preview.dataset.previewY),kind:preview.dataset.componentKind,width:parseFloat(preview.style.width),height:parseFloat(preview.style.height),text:preview.textContent}:null};})()`);
  const save = async () => {
    await click('Save');
    if (!profileId) profileId = await until(async () => (await request({ command: 'profile_list' })).profiles.profiles.find(profile => profile.name === name)?.id, 'Temporary profile did not save');
    await until(() => native.evaluate(`(()=>{const button=[...document.querySelectorAll('.profiles-workspace button')].find(button=>button.textContent.trim()==='Save');return button?.disabled;})()`), 'Profile save did not settle');
    return saved();
  };
  const capture = async filename => { const result = await native.send('Page.captureScreenshot', { format: 'png' }); await writeFile(resolve(artifacts, filename), Buffer.from(result.data, 'base64')); };
  const mouse = (type, point, extra = {}) => native.send('Input.dispatchMouseEvent', { type, x: point.x, y: point.y, ...extra });
  const stageVisible = async () => {
    await native.evaluate(`document.querySelector('.canvas-stage').scrollIntoView({block:'start'});window.scrollBy(0,-160)`);
    await sleep(150); return snapshot();
  };
  const startDrag = async selector => {
    intercepted = null;
    const source = await until(() => native.evaluate(`(()=>{const button=document.querySelector(${JSON.stringify(selector)});if(!button||button.disabled||!button.draggable)return null;button.scrollIntoView({block:'nearest'});const r=button.getBoundingClientRect();return{x:r.left+Math.min(r.width/2,60),y:r.top+r.height/2};})()`), 'The requested palette component is not draggable');
    await native.send('Input.setInterceptDrags', { enabled: true });
    await mouse('mouseMoved', source); await mouse('mousePressed', source, { button: 'left', buttons: 1, clickCount: 1 });
    dragging = true;
    for (const delta of [3, 9, 20, 40]) { await mouse('mouseMoved', { x: source.x + delta, y: source.y + 3 }, { button: 'left', buttons: 1 }); await sleep(35); if (intercepted) break; }
    const data = await until(() => intercepted?.data, 'Real palette mouse movement did not initiate an intercepted HTML5 drag', 5000);
    // Match Chromium's own Playwright DragManager: interception is only needed
    // until the actual drag starts, then native target drag events take over.
    await native.send('Input.setInterceptDrags', { enabled: false });
    assert.ok(data.items.some(item => /opensimgear|profile|component/.test(item.mimeType)), 'The actual palette drag must advertise its component data.');
    return data;
  };
  const over = async (data, point) => {
    await native.send('Input.dispatchDragEvent', { type: 'dragEnter', x: point.x, y: point.y, data });
    await native.send('Input.dispatchDragEvent', { type: 'dragOver', x: point.x, y: point.y, data });
    return until(async () => { const state = await snapshot(); return state.preview ? state : null; }, 'Canvas placement preview did not appear during the real native drag');
  };
  const finish = async (data, point, type = 'drop') => {
    await native.send('Input.dispatchDragEvent', { type, x: point.x, y: point.y, data });
    // drop/dragCancel already complete the intercepted mouse gesture. An extra
    // mouseReleased here would be an unrelated click on the just-created block.
    dragging = false;
    await until(async () => !(await snapshot()).preview, 'Canvas placement preview remained after the drag ended');
  };
  const positionMatches = (state, point) => {
    const preview = state.preview;
    near(preview.x, (point.x - state.stage.left - state.view.x) / state.view.zoom - preview.width / 2, 'Preview world X honors the current pan and zoom');
    near(preview.y, (point.y - state.stage.top - state.view.y) / state.view.zoom - preview.height / 2, 'Preview world Y honors the current pan and zoom');
    return preview;
  };
  native.on('Input.dragIntercepted', event => { intercepted = event; });
  try {
    assert.equal(await native.evaluate('typeof window.__TAURI_INTERNALS__?.invoke'), 'function');
    assert.equal(await native.evaluate('document.visibilityState'), 'visible');
    // Recover real browser input if a prior interrupted acceptance left a button down.
    await native.send('Input.cancelDragging');
    await mouse('mouseReleased', { x: 10, y: 10 }, { button: 'left', buttons: 0 });
    beforeProfiles = (await request({ command: 'profile_list' })).profiles.profiles; beforeAutomatic = await automatic();
    await native.send('Input.setInterceptDrags', { enabled: false });
    await click('Profiles', 'nav[aria-label="Main navigation"]');
    await click('Add'); draftOpen = true;
    await until(() => native.evaluate(`!!document.querySelector('[aria-label="Component palette"]')`), 'Profile editor did not open its palette');
    await click('Edit profile name');
    await native.evaluate(`(()=>{const input=document.querySelector('.profile-name-input');input.value=${JSON.stringify(name)};input.dispatchEvent(new Event('input',{bubbles:true}));input.dispatchEvent(new KeyboardEvent('keydown',{key:'Enter',bubbles:true,cancelable:true}));})()`);
    await save(); draftOpen = false;
    await click('Zoom out'); await click('Zoom out');
    let baseline = await stageVisible();
    const panPoint = pixelPoint(baseline.stage.left + baseline.stage.width / 2, baseline.stage.top + 80), pannedPoint = { x: panPoint.x + 73, y: panPoint.y - 47 };
    await mouse('mouseMoved', panPoint); await mouse('mousePressed', panPoint, { button: 'left', buttons: 1, clickCount: 1 });
    await mouse('mouseMoved', pannedPoint, { button: 'left', buttons: 1 });
    await mouse('mouseReleased', pannedPoint, { button: 'left', buttons: 0, clickCount: 1 });
    await until(async () => { const state = await snapshot(); return state.view.x !== baseline.view.x && state.view.y !== baseline.view.y; }, 'Real canvas background drag did not pan the viewport');
    await save(); baseline = await stageVisible();
    assert.ok(baseline.view.zoom < .9 && (baseline.view.x !== 0 || baseline.view.y !== 0));
    assert.deepEqual(baseline.nodes, []); assert.equal(baseline.inspector, null);

    let data = await startDrag('.component-palette [aria-label="Add virtual controller"]');
    let current = await snapshot(), point = pixelPoint(current.stage.left + current.stage.width * .38, current.stage.top + 240);
    let previewState = await over(data, point), preview = positionMatches(previewState, point);
    assert.deepEqual(previewState.nodes, []); assert.equal(previewState.inspector, null);
    assert.equal(await native.evaluate(`!!document.querySelector('.canvas-stage .empty-canvas')`), false, 'The empty-canvas hint must not overlap the placement preview');
    await capture('virtual-placement-preview.png');
    await finish(data, point);
    current = await until(async () => { const state = await snapshot(); return state.nodes.length === 1 ? state : null; }, 'Dropping the virtual component did not add exactly one block');
    const first = current.nodes[0];
    near(first.x, preview.x, 'Dropped block X equals its preview'); near(first.y, preview.y, 'Dropped block Y equals its preview');
    assert.equal(first.selected, false); assert.equal(current.inspector, null); assert.deepEqual(current.view, previewState.view);
    await click('Undo'); await until(async () => (await snapshot()).nodes.length === 0, 'One Undo did not remove the entire dropped component');
    await click('Redo'); current = await until(async () => { const state = await snapshot(); return state.nodes.length === 1 ? state : null; }, 'One Redo did not restore the dropped component');
    assert.equal(current.nodes[0].id, first.id); near(current.nodes[0].x, preview.x, 'Redo restores drop X'); near(current.nodes[0].y, preview.y, 'Redo restores drop Y');
    assert.equal(current.nodes[0].selected, false); assert.equal(current.inspector, null);
    let state = await save();
    const savedFirst = state.document.editor.positions.find(position => position.nodeId === first.id);
    near(savedFirst.x, preview.x, 'Saved position X equals preview'); near(savedFirst.y, preview.y, 'Saved position Y equals preview');
    checks.push('Real HTML5 virtual-controller drag previews the centered block under pan and zoom, drops exactly there without selection, and is one Undo/Redo edit');
    evidence.push({ component: 'virtual', viewport: previewState.view, pointer: point, preview, block: first });

    // A cancelled drag with a visible preview and an outside drop must be inert.
    data = await startDrag('.component-palette [aria-label="Add virtual controller"]');
    current = await snapshot(); point = pixelPoint(current.stage.left + current.stage.width * .6, current.stage.top + 270);
    await over(data, point); await finish(data, point, 'dragCancel');
    assert.equal((await snapshot()).nodes.length, 1);
    data = await startDrag('.component-palette [aria-label="Add virtual controller"]');
    current = await snapshot(); point = pixelPoint(current.stage.left + current.stage.width * .6, current.stage.top + 270);
    await over(data, point);
    const outside = pixelPoint(Math.max(10, current.stage.left - 60), Math.max(10, current.stage.top - 90));
    await native.send('Input.dispatchDragEvent', { type: 'dragEnter', x: outside.x, y: outside.y, data });
    await native.send('Input.dispatchDragEvent', { type: 'dragOver', x: outside.x, y: outside.y, data });
    await finish(data, outside);
    assert.equal((await snapshot()).nodes.length, 1);
    assert.equal(await native.evaluate(`(()=>{const button=[...document.querySelectorAll('.profiles-workspace button')].find(button=>button.textContent.trim()==='Save');return button.disabled;})()`), true, 'Cancellation/outside drop must not make the saved profile dirty');
    checks.push('Cancelling a visible preview and dropping outside the canvas leave the graph and saved state unchanged');

    // Keep an existing inspector open while dropping a real inventory component.
    current = await stageVisible();
    const heading = await native.evaluate(`(()=>{const button=document.querySelector('.resource-block[data-node-id="${first.id}"] .block-heading');const r=button.getBoundingClientRect();return{x:r.left+r.width/2,y:r.top+20};})()`);
    await mouse('mouseMoved', heading); await mouse('mousePressed', heading, { button: 'left', buttons: 1, clickCount: 1 }); await mouse('mouseReleased', heading, { button: 'left', buttons: 0, clickCount: 1 });
    const selected = await until(async () => { const state = await snapshot(); return state.nodes.find(node => node.id === first.id)?.selected && state.inspector ? state : null; }, 'Selecting an existing block did not open its inspector');
    const resourceId = await native.evaluate(`(()=>{const groups=[...document.querySelectorAll('.component-palette section')].filter(section=>['Controllers','OSG devices','Companions'].includes(section.getAttribute('aria-label')));return groups.flatMap(section=>[...section.querySelectorAll('button[data-resource-id]')]).find(button=>!button.disabled)?.dataset.resourceId??null;})()`);
    assert.ok(resourceId, 'Native acceptance requires one existing physical controller, module or paired companion in the real palette.');
    data = await startDrag(`.component-palette button[data-resource-id="${resourceId}"]`);
    current = await snapshot(); point = pixelPoint(current.stage.left + Math.min(160, current.stage.width * .125), current.stage.top + 300);
    previewState = await over(data, point); preview = positionMatches(previewState, point);
    assert.equal(previewState.inspector, selected.inspector); assert.equal(previewState.nodes.find(node => node.id === first.id).selected, true);
    await capture('resource-placement-preview-with-selection.png');
    await finish(data, point);
    current = await until(async () => { const state = await snapshot(); return state.nodes.length === 2 ? state : null; }, 'Dropping the actual inventory component did not add one block');
    const resource = current.nodes.find(node => node.id !== first.id);
    near(resource.x, preview.x, 'Resource drop X equals preview'); near(resource.y, preview.y, 'Resource drop Y equals preview');
    assert.equal(resource.selected, false); assert.equal(current.nodes.find(node => node.id === first.id).selected, true); assert.equal(current.inspector, selected.inspector); assert.deepEqual(current.view, previewState.view);
    state = await save();
    assert.equal(state.assignments.find(assignment => assignment.resourceId === resource.id)?.sourceId, resourceId);
    assert.deepEqual(state.document.resources.find(node => node.id === resource.id).ports, []);
    const savedResource = state.document.editor.positions.find(position => position.nodeId === resource.id);
    near(savedResource.x, preview.x, 'Saved resource X equals preview'); near(savedResource.y, preview.y, 'Saved resource Y equals preview');
    checks.push('Dragging a real native inventory component keeps the existing selected block, inspector and viewport, and saves only that resource at its preview position');
    evidence.push({ component: 'native-resource', sourceId: resourceId, viewport: previewState.view, pointer: point, preview, block: resource });

    const expectedPositions = state.document.editor.positions;
    await click('← Back to Profiles'); await click(`Edit ${name}`);
    current = await until(async () => { const state = await snapshot(); return state.nodes.length === 2 ? state : null; }, 'Reopening saved profile did not restore both dropped blocks');
    for (const position of expectedPositions) { const node = current.nodes.find(node => node.id === position.nodeId); assert.ok(node); near(node.x, position.x, 'Reopened block X'); near(node.y, position.y, 'Reopened block Y'); }
    assert.equal(current.nodes.some(node => node.selected), false); assert.equal(current.inspector, null);
    await stageVisible(); await capture('saved-drop-positions.png');
    checks.push('Saving and reopening preserves both exact dropped world positions without an automatic selection');
    await writeFile(resolve(artifacts, 'evidence.json'), JSON.stringify({ actualNativeCockpit: true, realHtml5Drag: true, profileId, checks, placements: evidence }, null, 2) + '\n');
  } catch (error) {
    await capture('failure.png').catch(() => {});
    await writeFile(resolve(artifacts, 'failure.json'), JSON.stringify({ profileId, checks, failure: redact(error.message), state: await snapshot().catch(() => null) }, null, 2) + '\n');
    throw error;
  } finally {
    try {
      if (dragging) await native.send('Input.dispatchDragEvent', { type: 'dragCancel', x: 10, y: 10, data: intercepted?.data ?? { items: [], dragOperationsMask: 65535 } }).catch(() => {});
      await native.send('Input.cancelDragging').catch(() => {});
      await mouse('mouseReleased', { x: 10, y: 10 }, { button: 'left', buttons: 0 }).catch(() => {});
      await native.send('Input.setInterceptDrags', { enabled: false }).catch(() => {});
      if (profileId) {
        assert.ok(beforeProfiles && !beforeProfiles.some(profile => profile.id === profileId));
        await clickIfPresent('← Back to Profiles'); await clickIfPresent('Discard and continue');
        const mode = await automatic();
        if (mode.manualProfileId !== beforeAutomatic.manualProfileId) await request({ command: 'profile_select', id: beforeAutomatic.manualProfileId, selectionSource: 'manual' });
        const current = await saved(); await request({ command: 'profile_delete', id: profileId, expectedRevision: current.document.revision });
        await clickIfPresent('Refresh profiles');
      } else if (draftOpen) await clickIfPresent('Cancel creation');
      if (beforeProfiles) assert.deepEqual((await request({ command: 'profile_list' })).profiles.profiles, beforeProfiles, 'Acceptance must preserve every original saved profile');
      if (beforeAutomatic) { const finalMode = await automatic(); assert.equal(finalMode.mode, beforeAutomatic.mode); assert.equal(finalMode.manualProfileId, beforeAutomatic.manualProfileId); assert.deepEqual(finalMode.rules, beforeAutomatic.rules); }
    } finally { native.close(); }
  }
});
