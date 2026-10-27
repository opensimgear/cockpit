// Shared source/target trunks against the running native dev Cockpit. The real
// Feature Lab is copied through native IPC, and the copy is removed afterward.
import test from 'node:test';
import assert from 'node:assert/strict';
import { randomUUID } from 'node:crypto';
import { mkdir, writeFile } from 'node:fs/promises';
import { resolve } from 'node:path';

const sourceId = process.env.OSG_ROUTING_PROFILE_ID ?? 'f592a7b4-6f39-4267-a8f9-b46fbe4ff91a';
const artifacts = resolve('.research/profile-wire-trunks-native', String(Date.now()));
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

function canvasSnapshot() {
  const stage = document.querySelector('.canvas-stage'), actions = [...document.querySelectorAll('.editor-save-actions button')];
  const edges = [...(stage?.querySelectorAll('.signal-edge') ?? [])].map(path => {
    const length = path.getTotalLength(), start = path.getPointAtLength(0), end = path.getPointAtLength(length);
    return { id: path.dataset.bindingId, ids: JSON.parse(path.dataset.bindingIds ?? 'null'), count: Number(path.dataset.bundleCount),
      style: path.dataset.arrowStyle, d: path.getAttribute('d'), start: { x: start.x, y: start.y }, end: { x: end.x, y: end.y }, selected: path.classList.contains('selected') };
  });
  return { nodes: stage?.querySelectorAll('.resource-block').length, edges,
    saveDisabled: actions.find(button => button.textContent.trim() === 'Save')?.disabled,
    undoDisabled: actions.find(button => button.getAttribute('aria-label') === 'Undo')?.disabled,
    redoDisabled: actions.find(button => button.getAttribute('aria-label') === 'Redo')?.disabled,
    picker: document.querySelector('.canvas-wire-bundle-picker') ? [...document.querySelectorAll('.canvas-wire-bundle-picker [data-bundle-binding]')].map(button => button.dataset.bundleBinding) : null,
    badges: [...document.querySelectorAll('.connection-bundle-badge')].map(badge => ({ id: badge.dataset.bundleId, role: badge.getAttribute('role'), text: badge.textContent.trim() })),
    trunks: [...document.querySelectorAll('.signal-trunk')].map(path => {
      const length = path.getTotalLength(), start = path.getPointAtLength(0), end = path.getPointAtLength(length);
      return { id: path.dataset.trunkId, ids: JSON.parse(path.dataset.bindingIds ?? 'null'), direction: path.dataset.trunkDirection, d: path.getAttribute('d'),
        start: { x: start.x, y: start.y }, end: { x: end.x, y: end.y }, length };
    }),
    trunkHits: [...document.querySelectorAll('.signal-trunk-hit')].map(hit => ({ id: hit.dataset.trunkId, role: hit.getAttribute('role') })),
    trunkBadges: [...document.querySelectorAll('.connection-trunk-badge')].map(badge => ({ id: badge.dataset.trunkId, role: badge.getAttribute('role'), text: badge.textContent.trim() })),
    endpoints: [...document.querySelectorAll('.connection-endpoint')].map(endpoint => endpoint.dataset.reconnectBinding),
  };
}

function assertRepresentation(snapshot, expected, description) {
  assert.equal(snapshot.nodes, 42, `${description}: every Base block remains visible`);
  const all = snapshot.edges.flatMap(edge => {
    assert.ok(Array.isArray(edge.ids) && edge.ids.length > 0, `${description}: every rendered line identifies its exact logical members`);
    assert.equal(edge.count, edge.ids.length, `${description}: badge count matches logical membership`);
    assert.doesNotMatch(edge.d, /NaN|Infinity/);
    return edge.ids;
  });
  assert.equal(new Set(all).size, all.length, `${description}: a signal is represented exactly once`);
  assert.deepEqual(all.sort(), [...expected].sort(), `${description}: no logical connections are lost`);
}

// Measure the actual WebView SVG curves, rather than only the router's guide
// points. Shared cubic controls must agree along every member's full route.
function trunkGeometryInBrowser(bindings) {
  const spacing = 2, tolerance = .8, errors = [], sourceFans = [], targetFans = [];
  const routes = new Map(), boxes = [...document.querySelectorAll('.canvas-stage .resource-block')].map(node => ({
    id: node.dataset.nodeId, left: parseFloat(node.style.left), top: parseFloat(node.style.top),
    right: parseFloat(node.style.left) + parseFloat(getComputedStyle(node).width),
    bottom: parseFloat(node.style.top) + parseFloat(getComputedStyle(node).height),
  }));
  const sample = path => {
    const length = path.getTotalLength(), count = Math.max(1, Math.ceil(length / spacing));
    return Array.from({ length: count + 1 }, (_, index) => { const point = path.getPointAtLength(length * index / count); return { x: point.x, y: point.y }; });
  };
  const distance = (point, a, b) => {
    const dx = b.x - a.x, dy = b.y - a.y, t = Math.max(0, Math.min(1, ((point.x - a.x) * dx + (point.y - a.y) * dy) / (dx * dx + dy * dy || 1)));
    return Math.hypot(point.x - a.x - t * dx, point.y - a.y - t * dy);
  };
  const equal = (a, b) => Math.hypot(a.x - b.x, a.y - b.y) < tolerance;
  const cellsFor = (a, b, size, margin = 0) => {
    const cells = [];
    for (let x = Math.floor((Math.min(a.x, b.x) - margin) / size); x <= Math.floor((Math.max(a.x, b.x) + margin) / size); x++) {
      for (let y = Math.floor((Math.min(a.y, b.y) - margin) / size); y <= Math.floor((Math.max(a.y, b.y) + margin) / size); y++) cells.push(`${x},${y}`);
    }
    return cells;
  };
  for (const path of document.querySelectorAll('.canvas-stage .signal-edge')) {
    const points = sample(path), cells = new Map();
    for (let index = 1; index < points.length; index++) {
      const segment = { a: points[index - 1], b: points[index] };
      for (const key of cellsFor(segment.a, segment.b, 16, tolerance)) {
        if (!cells.has(key)) cells.set(key, []); cells.get(key).push(segment);
      }
    }
    for (const id of JSON.parse(path.dataset.bindingIds)) routes.set(id, { path, points, cells, binding: bindings.find(binding => binding.id === id) });
  }
  const trunks = [];
  for (const path of document.querySelectorAll('.canvas-stage .signal-trunk')) {
    const id = path.dataset.trunkId, ids = JSON.parse(path.dataset.bindingIds ?? 'null');
    if (!Array.isArray(ids) || ids.length < 2 || new Set(ids).size !== ids.length) { errors.push({ id, reason: 'invalid membership', ids }); continue; }
    const members = ids.map(id => routes.get(id));
    if (members.some(member => !member?.binding)) { errors.push({ id, reason: 'unknown member' }); continue; }
    const points = sample(path), first = points[0], last = points.at(-1), length = path.getTotalLength();
    const commonSource = members.every(member => member.binding.from.nodeId === members[0].binding.from.nodeId && equal(member.points[0], members[0].points[0]));
    const commonTarget = members.every(member => member.binding.to.nodeId === members[0].binding.to.nodeId && equal(member.points.at(-1), members[0].points.at(-1)));
    const source = commonSource, target = commonTarget;
    if (!source && !target) errors.push({ id, reason: 'trunk members do not share one common physical port', ids });
    if (path.dataset.trunkDirection === 'output' && !source || path.dataset.trunkDirection === 'input' && !target) errors.push({ id, reason: 'trunk direction disagrees with common member endpoint', ids });
    if (!Number.isFinite(length) || length < 2) errors.push({ id, reason: 'empty or invalid trunk' });
    if (source && equal(first, members[0].points[0]) && points[1].x < first.x - .01) errors.push({ id, reason: 'trunk leaves output backwards' });
    if (target && equal(last, members[0].points.at(-1)) && last.x < points.at(-2).x - .01) errors.push({ id, reason: 'trunk enters input backwards' });
    const sourceNodes = [...new Set(members.map(member => member.binding.from.nodeId))], targetNodes = [...new Set(members.map(member => member.binding.to.nodeId))];
    if (source && targetNodes.length > 1) sourceFans.push({ id, nodeId: sourceNodes[0], targets: targetNodes, members: ids });
    if (target && sourceNodes.length > 1) targetFans.push({ id, nodeId: targetNodes[0], sources: sourceNodes, members: ids });
    for (const point of points) {
      const obstacle = boxes.find(box => point.x > box.left + .5 && point.x < box.right - .5 && point.y > box.top + .5 && point.y < box.bottom - .5);
      if (obstacle) { errors.push({ id, reason: 'trunk crosses block interior', obstacle: obstacle.id, point }); break; }
      for (let index = 0; index < members.length; index++) {
        const segments = members[index].cells.get(`${Math.floor(point.x / 16)},${Math.floor(point.y / 16)}`) ?? [];
        if (!segments.some(segment => distance(point, segment.a, segment.b) <= tolerance)) {
          errors.push({ id, reason: 'trunk is not part of member rendered curve', member: ids[index], point }); break;
        }
      }
      if (errors.length > 30) break;
    }
    const segments = points.slice(1).map((b, index) => ({ a: points[index], b, index })), cells = new Map(), tested = new Set();
    for (const segment of segments) for (const key of cellsFor(segment.a, segment.b, 16)) {
      if (!cells.has(key)) cells.set(key, []); cells.get(key).push(segment);
    }
    let selfIntersection = null;
    for (const cell of cells.values()) {
      if (selfIntersection) break;
      for (let i = 0; i < cell.length && !selfIntersection; i++) for (let j = i + 1; j < cell.length; j++) {
        const a = cell[i], b = cell[j]; if (Math.abs(a.index - b.index) <= 2) continue;
        const key = [a.index, b.index].sort((x, y) => x - y).join('/'); if (tested.has(key)) continue; tested.add(key);
        const rx = a.b.x - a.a.x, ry = a.b.y - a.a.y, sx = b.b.x - b.a.x, sy = b.b.y - b.a.y;
        const rl = Math.hypot(rx, ry), sl = Math.hypot(sx, sy); if (rl < 1e-6 || sl < 1e-6) continue;
        const qx = b.a.x - a.a.x, qy = b.a.y - a.a.y, denominator = rx * sy - ry * sx;
        if (Math.abs(denominator) / (rl * sl) < 1e-5) {
          if (Math.abs(qx * ry - qy * rx) / rl > .02) continue;
          const from = (qx * rx + qy * ry) / rl, to = from + (sx * rx + sy * ry) / rl;
          const overlap = Math.max(0, Math.min(rl, Math.max(from, to)) - Math.max(0, Math.min(from, to)));
          if (overlap > .1) selfIntersection = { reason: 'retraced trunk', overlap };
        } else {
          const t = (qx * sy - qy * sx) / denominator, u = (qx * ry - qy * rx) / denominator;
          if (t >= -1e-5 && t <= 1 + 1e-5 && u >= -1e-5 && u <= 1 + 1e-5) selfIntersection = { reason: 'self-crossing trunk', point: { x: a.a.x + t * rx, y: a.a.y + t * ry } };
        }
        if (selfIntersection) break;
      }
    }
    if (selfIntersection) errors.push({ id, ...selfIntersection });
    trunks.push({ id, ids, source, target, sourceNodes, targetNodes, length, samples: points.length });
  }
  return { spacing, tolerance, trunks, sourceFans, targetFans, errors };
}

test('native Feature Lab merges common port trunks while preserving branches, individual editing and responsive dragging', { timeout: 300000 }, async () => {
  await mkdir(artifacts, { recursive: true });
  const native = await connectNative(), profileId = randomUUID(), name = `Wire trunks ${profileId}`, checks = [];
  let created = false, originalStyling, pointerDown = false, baseBindings;
  const request = value => native.evaluate(async value => {
    const reply = await window.__TAURI_INTERNALS__.invoke('gateway_request', { request: { ...value, clientId: 'profile-wire-trunks-native' } });
    if (!reply.ok) throw new Error(`${value.command}: ${reply.error}`); return reply.state;
  }, value);
  const canvasStyling = () => native.evaluate(async () => (await window.__TAURI_INTERNALS__.invoke('persistence_ui_read')).entries['canvas-styling']);
  const snapshot = () => native.evaluate(canvasSnapshot);
  const click = (label, scope = '.profiles-workspace') => until(() => native.evaluate((label, scope) => {
    const button = [...(document.querySelector(scope)?.querySelectorAll('button') ?? [])].find(button => (button.getAttribute('aria-label') ?? button.textContent.trim()) === label);
    if (!button || button.disabled) return false; button.click(); return true;
  }, label, scope), `UI action unavailable: ${label}`);
  const mouse = (type, at, extra = {}) => native.send('Input.dispatchMouseEvent', { type, x: at.x, y: at.y, ...extra });
  const pointerClick = async at => {
    await mouse('mouseMoved', at); await mouse('mousePressed', at, { button: 'left', buttons: 1, clickCount: 1 });
    await mouse('mouseReleased', at, { button: 'left', buttons: 0, clickCount: 1 });
  };
  const key = async (key, code, value) => {
    await native.send('Input.dispatchKeyEvent', { type: 'keyDown', key, code, windowsVirtualKeyCode: value });
    await native.send('Input.dispatchKeyEvent', { type: 'keyUp', key, code, windowsVirtualKeyCode: value });
  };
  const capture = async filename => {
    const shot = await native.send('Page.captureScreenshot', { format: 'png' });
    await writeFile(resolve(artifacts, filename), Buffer.from(shot.data, 'base64'));
  };
  const open = async () => {
    await until(() => native.evaluate(name => {
      const stage = document.querySelector('.canvas-stage');
      if (document.querySelector('.profile-title-button')?.textContent.trim() === name && stage?.getBoundingClientRect().width > 0) return true;
      const workspace = document.querySelector('.profiles-workspace');
      if (!workspace) {
        [...document.querySelectorAll('nav[aria-label="Main navigation"] button')].find(button => button.textContent.trim() === 'Profiles')?.click();
        return false;
      }
      const edit = [...workspace.querySelectorAll('button')].find(button => button.getAttribute('aria-label') === `Edit ${name}`);
      if (edit) edit.click(); else [...workspace.querySelectorAll('button')].find(button => button.textContent.trim() === '← Back to Profiles')?.click();
      return false;
    }, name), 'Open the disposable Feature Lab copy');
    await native.evaluate(() => { if(document.querySelector('.canvas-stage')?.dataset.view==='overview')document.querySelector('[aria-label="Toggle detailed graph"]').click(); });
    await until(async () => (await snapshot()).edges.reduce((sum, edge) => sum + edge.count, 0) === 73, 'Every Base signal must be represented');
    await native.evaluate(() => document.querySelector('.canvas-stage').scrollIntoView({ block: 'center' }));
    await click('Fit all'); await pause(150);
  };
  const assertClean = async description => {
    const state = await snapshot();
    assert.equal(state.saveDisabled, true, `${description} does not dirty the graph`);
    assert.equal(state.undoDisabled, true, `${description} does not enter Undo history`);
    assert.equal(state.redoDisabled, true, `${description} does not enter Redo history`);
  };
  const preferences = async style => {
    await assertClean('Changing view preferences');
    await click('Settings', '.sidebar-bottom'); await click('Styling', '[role="tablist"][aria-label="Settings sections"]');
    await until(() => native.evaluate(style => {
      const select = document.querySelector('select[aria-label="Arrow style"]');
      if (!select) return false;
      select.value = style; select.dispatchEvent(new Event('change', { bubbles: true }));
      return true;
    }, style), 'Styling includes the arrow-style preference');
    const persisted = await until(async () => { const row = await canvasStyling(); return row?.value?.arrowStyle === style ? row : null; }, 'Arrow style was not persisted through native UI preferences');
    assert.deepEqual(Object.keys(persisted.value).sort(), ['arrowStyle', 'cornerRadius'], 'Canvas styling persists only the supported preferences');
    await open();
    await until(async () => {
      const state = await snapshot();
      return state.edges.length > 0 && state.edges.every(edge => edge.style === style) && state.trunks.length > 0;
    }, `${style} collapsed routes and shared trunks must render`);
    await assertClean('Changing arrow style');
  };
  const badge = () => until(() => native.evaluate(() => {
    const stage = document.querySelector('.canvas-stage').getBoundingClientRect();
    for (const badge of document.querySelectorAll('.connection-trunk-badge')) {
      const rect = badge.getBoundingClientRect(), x = rect.left + rect.width / 2, y = rect.top + rect.height / 2;
      if (x < stage.left + 2 || x > stage.right - 2 || y < Math.max(0, stage.top) + 2 || y > Math.min(innerHeight, stage.bottom) - 2 || !badge.contains(document.elementFromPoint(x, y))) continue;
      return { id: badge.dataset.trunkId, x, y };
    }
    return null;
  }), 'A shared trunk must expose a visible clickable count badge');
  const openBundle = async () => {
    const at = await badge(), trunk = (await snapshot()).trunks.find(trunk => trunk.id === at.id); await pointerClick(at);
    const members = await until(async () => (await snapshot()).picker, 'Clicking a wire count opens its exact-signal picker');
    assert.ok(members.length >= 2, 'The picker lists every connection in a shared route');
    assert.equal(new Set(members).size, members.length, 'The signal picker has no duplicate entries');
    assert.deepEqual([...members].sort(), [...trunk.ids].sort(), 'The trunk picker lists precisely that trunk\'s logical members');
    return { id: at.id, members };
  };
  const openTrunkHit = async () => {
    const at = await until(() => native.evaluate(() => {
      const stage = document.querySelector('.canvas-stage').getBoundingClientRect();
      for (const path of document.querySelectorAll('.signal-trunk-hit')) {
        const length = path.getTotalLength(), matrix = path.getScreenCTM();
        for (const ratio of [.5, .35, .65, .2, .8]) {
          const sample = path.getPointAtLength(length * ratio), point = new DOMPoint(sample.x, sample.y).matrixTransform(matrix);
          if (point.x < Math.max(0, stage.left) + 4 || point.x > Math.min(innerWidth, stage.right) - 4 || point.y < Math.max(0, stage.top) + 4 || point.y > Math.min(innerHeight, stage.bottom) - 4) continue;
          if (document.elementFromPoint(point.x, point.y)?.closest('.signal-trunk-hit') === path) return { id: path.dataset.trunkId, x: point.x, y: point.y };
        }
      }
      return null;
    }), 'A shared trunk has a visible clickable wire segment');
    const trunk = (await snapshot()).trunks.find(trunk => trunk.id === at.id); await pointerClick(at);
    const members = await until(async () => (await snapshot()).picker, 'Clicking a shared trunk opens its signal picker');
    assert.deepEqual([...members].sort(), [...trunk.ids].sort(), 'The shared line opens precisely its member connections');
    return { id: at.id, members };
  };
  const blankClick = async () => {
    const at = await until(() => native.evaluate(() => {
      const stage = document.querySelector('.canvas-stage'), rect = stage.getBoundingClientRect();
      for (const fy of [.12, .28, .82, .65]) for (const fx of [.08, .35, .7, .9]) {
        const x = rect.left + rect.width * fx, y = rect.top + rect.height * fy, hit = document.elementFromPoint(x, y);
        if (x > 0 && y > 0 && x < innerWidth && y < innerHeight && hit && stage.contains(hit) && !hit.closest('[data-node-id],.signal-edge,.signal-edge-hit,.signal-trunk,.signal-trunk-hit,.connection-bundle-badge,.connection-trunk-badge,.canvas-wire-bundle-picker,.wire-actions,button,input,select')) return { x, y };
      }
      return null;
    }), 'A visible blank canvas point is available');
    await pointerClick(at); await until(async () => !(await snapshot()).picker, 'Clicking blank canvas closes the bundle picker');
  };
  const selectMember = async id => {
    await until(() => native.evaluate(id => {
      const button = [...document.querySelectorAll('.canvas-wire-bundle-picker [data-bundle-binding]')].find(button => button.dataset.bundleBinding === id);
      if (!button || button.disabled) return false; button.click(); return true;
    }, id), 'A bundle member is independently selectable');
    const selected = await until(async () => {
      const state = await snapshot(), selected = state.edges.find(edge => edge.id === id && edge.selected && edge.count === 1);
      return !state.picker && selected && state.endpoints.filter(binding => binding === id).length === 2 ? state : null;
    }, 'Selecting a member opens its individual wire and reconnect endpoints');
    assertRepresentation(selected, baseBindings.map(binding => binding.id), 'Member selection');
    await assertClean('Inspecting a bundled connection');
  };
  const assertAnchors = async () => {
    const geometry = await native.evaluate(bindings => {
      const world = document.querySelector('.canvas-world'), origin = world.getBoundingClientRect(), zoom = new DOMMatrix(getComputedStyle(world).transform).a;
      const anchors = [...document.querySelectorAll('.canvas-stage .resource-block')].flatMap(node => [...node.querySelectorAll('[data-connection-source],[data-connection-target]')].map(handle => {
        const rect = handle.getBoundingClientRect(), output = handle.hasAttribute('data-connection-source');
        return { nodeId: node.dataset.nodeId, portId: handle.dataset.connectionPort, direction: output ? 'output' : 'input',
          x: parseFloat(node.style.left) + (output ? parseFloat(getComputedStyle(node).width) : 0), y: (rect.top + rect.height / 2 - origin.top) / zoom };
      }));
      const errors = [];
      for (const path of document.querySelectorAll('.canvas-stage .signal-edge')) for (const id of JSON.parse(path.dataset.bindingIds)) {
        const binding = bindings.find(binding => binding.id === id);
        for (const [endpoint, point, direction] of [[binding.from, path.getPointAtLength(0), 'output'], [binding.to, path.getPointAtLength(path.getTotalLength()), 'input']]) {
          const anchor = anchors.find(anchor => anchor.nodeId === endpoint.nodeId && anchor.direction === direction && (!anchor.portId || anchor.portId === endpoint.portId));
          if (!anchor || Math.hypot(point.x - anchor.x, point.y - anchor.y) > .8) errors.push({ id, direction, point: { x: point.x, y: point.y }, anchor });
        }
      }
      return { errors, visualRoutes: document.querySelectorAll('.canvas-stage .signal-edge').length };
    }, baseBindings);
    assert.deepEqual(geometry.errors, [], 'Every shared route joins the actual physical ports of all its members');
    return geometry;
  };
  const drag = async style => {
    await click('Fit all');
    const before = await snapshot(), nodeIds = [...new Set(before.trunks.flatMap(edge => edge.ids.flatMap(id => {
      const binding = baseBindings.find(binding => binding.id === id); return [binding.from.nodeId, binding.to.nodeId];
    })))];
    const move = await until(() => native.evaluate(nodeIds => {
      const stage = document.querySelector('.canvas-stage').getBoundingClientRect(), zoom = new DOMMatrix(getComputedStyle(document.querySelector('.canvas-world')).transform).a;
      const nodes = [...document.querySelectorAll('.canvas-stage .resource-block')];
      for (const node of nodes.filter(node => nodeIds.includes(node.dataset.nodeId))) {
        const heading = node.querySelector('.block-heading'), rect = heading.getBoundingClientRect(), x = rect.left + rect.width / 2, y = rect.top + rect.height / 2;
        if (x < stage.left + 16 || x > stage.right - 16 || y < Math.max(0, stage.top) + 16 || y > Math.min(innerHeight, stage.bottom) - 16 || !heading.contains(document.elementFromPoint(x, y))) continue;
        const nx = parseFloat(node.style.left), ny = parseFloat(node.style.top), nw = parseFloat(getComputedStyle(node).width), nh = parseFloat(getComputedStyle(node).height);
        for (const [dx, dy] of [[10, 0], [-10, 0], [0, 10], [0, -10], [10, 10], [-10, -10]]) {
          const overlaps = nodes.some(other => { if (other === node) return false; const ox = parseFloat(other.style.left), oy = parseFloat(other.style.top), ow = parseFloat(getComputedStyle(other).width), oh = parseFloat(getComputedStyle(other).height); return nx + dx / zoom < ox + ow + 22 && nx + dx / zoom + nw > ox - 22 && ny + dy / zoom < oy + oh + 22 && ny + dy / zoom + nh > oy - 22; });
          if (!overlaps) return { nodeId: node.dataset.nodeId, x, y, dx, dy };
        }
      }
      return null;
    }, nodeIds), 'A trunk-connected block has a visible unobstructed drag destination');
    const beforePaths = new Map(before.trunks.map(edge => [JSON.stringify(edge.ids), edge.d])), times = []; let changed = false;
    await mouse('mouseMoved', move); await mouse('mousePressed', move, { button: 'left', buttons: 1, clickCount: 1 }); pointerDown = true;
    for (let index = 1; index <= 6; index++) {
      const at = { x: move.x + move.dx * index / 6, y: move.y + move.dy * index / 6 }, started = performance.now();
      await mouse('mouseMoved', at, { button: 'left', buttons: 1 });
      await native.evaluate(() => new Promise(resolve => requestAnimationFrame(() => resolve(true))));
      times.push(performance.now() - started);
      const current = await snapshot(); assertRepresentation(current, baseBindings.map(binding => binding.id), 'Drag preview');
      changed ||= current.trunks.some(edge => beforePaths.get(JSON.stringify(edge.ids)) !== edge.d);
      await assertAnchors();
    }
    await mouse('mouseReleased', { x: move.x + move.dx, y: move.y + move.dy }, { button: 'left', buttons: 0, clickCount: 1 }); pointerDown = false;
    await until(async () => (await snapshot()).saveDisabled === false, 'Moving a block creates a graph edit');
    assert.ok(changed, 'The shared trunk updates while its block is being dragged');
    assert.ok(Math.max(...times) < 250, `Native trunk dragging remains responsive: ${times.join(', ')}`);
    await assertAnchors();
    const movedGeometry = await native.evaluate(trunkGeometryInBrowser, baseBindings);
    assert.deepEqual(movedGeometry.errors, [], 'Moved trunk curves remain simple, clear and shared by their members');
    await click('Undo', '.editor-save-actions');
    await until(async () => (await snapshot()).saveDisabled, 'Undo restores the saved block layout');
    const restored = await snapshot();
    assert.deepEqual(restored.edges.map(edge => ({ ids: edge.ids, d: edge.d })), before.edges.map(edge => ({ ids: edge.ids, d: edge.d })), 'Undo restores bundled routing');
    assert.deepEqual(restored.trunks, before.trunks, 'Undo restores the shared trunk geometry and membership');
    return { style, nodeId: move.nodeId, dragSamplesMs: times, anchorsVerifiedDuringDrag: true };
  };
  try {
    await native.send('Runtime.enable');
    await until(() => native.evaluate(() => document.readyState === 'complete' && typeof window.__TAURI_INTERNALS__?.invoke === 'function'), 'The native cockpit must finish startup');
    originalStyling = await canvasStyling();
    const original = (await request({ command: 'profile_assignments_get', id: sourceId })).profiles;
    baseBindings = await native.evaluate(async document => {
      const { profileTabDocument } = await import('/src/lib/profile-vehicle-tabs.ts'); return profileTabDocument(document, '').bindings;
    }, original.document);
    assert.equal(baseBindings.length, 73, 'The test uses the actual DCS Feature Lab Base graph');
    assert.equal(original.document.bindings.length, 87, 'Vehicle-specific connections remain part of the saved graph');
    const document = { ...original.document, id: profileId, name, revision: 1 };
    await request({ command: 'profile_editor_save', document, assignments: original.assignments, expectedRevision: null }); created = true;
    const savedBefore = (await request({ command: 'profile_assignments_get', id: profileId })).profiles.document;
    await native.send('Page.reload', { ignoreCache: true }); await open(); await native.send('Page.bringToFront');
    for (const style of ['smooth', 'orthogonal']) {
      await preferences(style);
      const collapsed = await snapshot(); assertRepresentation(collapsed, baseBindings.map(binding => binding.id), `${style} collapsed view`);
      assert.ok(collapsed.edges.length < 73); assert.ok(collapsed.edges.some(edge => edge.count >= 2));
      assert.ok(collapsed.badges.length > 0); assert.ok(collapsed.badges.every(badge => badge.role === 'button'), 'Wire counts are accessible controls');
      assert.ok(collapsed.trunks.length > 0, 'Common source or target points create real shared trunks');
      assert.ok(collapsed.trunkHits.length > 0 && collapsed.trunkHits.every(hit => hit.role === 'button'), 'Trunk lines are accessible interactive controls');
      assert.ok(collapsed.trunkBadges.length > 0 && collapsed.trunkBadges.every(badge => badge.role === 'button'), 'Trunk count badges are accessible controls');
      const geometry = await native.evaluate(trunkGeometryInBrowser, baseBindings);
      assert.deepEqual(geometry.errors, [], `${style} trunk curves are simple, clear and coincide with every member path`);
      assert.ok(geometry.sourceFans.length > 0, 'At least one output trunk branches towards different destination blocks');
      assert.ok(geometry.targetFans.some(fan => fan.nodeId === 'virtual-aux'), 'The dense virtual-aux input receives a common trunk from different source blocks');
      await assertAnchors(); await capture(`${style}-collapsed.png`);
      const clickedTrunk = await openTrunkHit();
      await key('Escape', 'Escape', 27); await until(async () => !(await snapshot()).picker, 'Escape closes the clicked-trunk picker');
      await assertClean('Inspecting a trunk line');
      const escapeBundle = await openBundle();
      await capture(`${style}-bundle-picker.png`);
      await key('Escape', 'Escape', 27); await until(async () => !(await snapshot()).picker, 'Escape closes the shared-wire picker');
      await assertClean('Closing the picker with Escape');
      const blankBundle = await openBundle(); await blankClick(); await assertClean('Closing the picker by clicking canvas');
      const selectedBundle = await openBundle();
      const selectedId = selectedBundle.members[0]; await selectMember(selectedId); await capture(`${style}-member-selected.png`);
      await click('Close connection actions', '.wire-actions');
      await until(async () => !(await snapshot()).endpoints.length, 'Closing connection actions restores the collapsed view');
      const afterSelection = await snapshot();
      assertRepresentation(afterSelection, baseBindings.map(binding => binding.id), `${style} after independent signal selection`);
      assert.ok(afterSelection.edges.some(edge => edge.count > 1), 'Closing signal actions restores the always-collapsed view');
      assert.ok(afterSelection.trunks.length > 0, 'Shared trunks remain available after individual signal inspection');
      const savedAfter = (await request({ command: 'profile_assignments_get', id: profileId })).profiles.document;
      assert.deepEqual(savedAfter, savedBefore, 'Arrow style changes and inspection leave the saved graph untouched');
      const interaction = await drag(style);
      // Undo leaves a Redo step for the actual block edit. Reopen the saved
      // document so the next style starts with a genuinely empty history.
      await click('← Back to Profiles'); await open();
      checks.push({ style, logicalConnections: 73, visualRoutes: collapsed.edges.length, bundles: collapsed.badges.length,
        sharedTrunks: collapsed.trunks.length, geometry, clickedTrunk,
        escapeMembers: escapeBundle.members, blankMembers: blankBundle.members, selectedId, interaction });
      console.log(`${style}: ${collapsed.edges.length} branches and ${collapsed.trunks.length} shared trunks represent 73 signals; individual selection and live drag pass`);
    }
    await preferences('orthogonal');
    const deletionBundle = await openBundle(), deletedId = deletionBundle.members[0]; await selectMember(deletedId);
    await click('Disconnect', '.wire-actions');
    const afterDelete = await until(async () => { const state = await snapshot(); return state.edges.reduce((sum, edge) => sum + edge.count, 0) === 72 ? state : null; }, 'Disconnect removes the selected logical signal only');
    assertRepresentation(afterDelete, baseBindings.filter(binding => binding.id !== deletedId).map(binding => binding.id), 'Individual bundled-signal deletion');
    assert.equal(afterDelete.saveDisabled, false); await click('Undo', '.editor-save-actions');
    await until(async () => (await snapshot()).saveDisabled, 'Undo restores the individually disconnected signal');
    assertRepresentation(await snapshot(), baseBindings.map(binding => binding.id), 'Undo bundled-signal deletion');
    const savedAfter = (await request({ command: 'profile_assignments_get', id: profileId })).profiles.document;
    assert.deepEqual(savedAfter, savedBefore, 'Individual delete/Undo never accidentally saves changes or removes another vehicle tab');
    const originalAfter = (await request({ command: 'profile_assignments_get', id: sourceId })).profiles.document;
    assert.deepEqual(originalAfter, original.document, 'The original DCS demo remains untouched');
    assert.deepEqual(native.errors, [], 'The actual native cockpit raises no UI exceptions during trunk interaction');
    await writeFile(resolve(artifacts, 'evidence.json'), JSON.stringify({ actualNativeCockpit: true, sourceProfileName: original.document.name, checks,
      individualDelete: { deletedId, undoRestored: true }, sourceUnchanged: true, savedSignalCount: savedAfter.bindings.length, errors: native.errors }, null, 2));
    console.log(`Native trunk evidence: ${artifacts}`);
  } catch (error) {
    await capture('failure.png').catch(() => {});
    const state = await snapshot().catch(() => null), body = await native.evaluate(() => document.body.innerText.slice(0, 12000)).catch(() => null);
    await writeFile(resolve(artifacts, 'failure.json'), JSON.stringify({ error: error.message, state, body, errors: native.errors }, null, 2));
    throw error;
  } finally {
    if (pointerDown) await mouse('mouseReleased', { x: 10, y: 10 }, { button: 'left', buttons: 0, clickCount: 1 }).catch(() => {});
    if (created) {
      await native.evaluate(() => [...document.querySelectorAll('.profiles-workspace button')].find(button => button.textContent.trim() === '← Back to Profiles')?.click()).catch(() => {});
      await pause(100);
      await native.evaluate(() => [...document.querySelectorAll('dialog button')].find(button => button.textContent.trim() === 'Discard and continue')?.click()).catch(() => {});
      const current = (await request({ command: 'profile_assignments_get', id: profileId })).profiles;
      await request({ command: 'profile_delete', id: profileId, expectedRevision: current.document.revision });
    }
    if (originalStyling) {
      const current = await canvasStyling();
      if (JSON.stringify(current.value) !== JSON.stringify(originalStyling.value)) await request({ command: 'ui_preferences_save', key: 'canvas-styling', expectedRevision: current.revision, value: originalStyling.value });
    }
    await native.send('Page.reload', { ignoreCache: true }).catch(() => {}); native.close();
  }
});
