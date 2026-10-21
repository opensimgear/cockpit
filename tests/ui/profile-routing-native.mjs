// Acceptance against the already-running native dev Cockpit, using a disposable
// copy of the real Feature Lab profile. No mock gateway or browser preview.
import test from 'node:test';
import assert from 'node:assert/strict';
import { randomUUID } from 'node:crypto';
import { mkdir, readFile, writeFile } from 'node:fs/promises';
import { resolve } from 'node:path';

const sourceId = process.env.OSG_ROUTING_PROFILE_ID ?? 'f592a7b4-6f39-4267-a8f9-b46fbe4ff91a';
const artifacts = resolve('.research/profile-routing-native', String(Date.now()));
const pause = ms => new Promise(resolve => setTimeout(resolve, ms));
const percentile = (values, p) => [...values].sort((a, b) => a - b)[Math.floor((values.length - 1) * p)];

// Runs inside the native WebView so SVG curves and arcs are measured by the
// same renderer as the cockpit. These are comparative readability metrics;
// geometric correctness is asserted separately at much finer sample spacing.
function qualityInBrowser(paths) {
  const namespace = 'http://www.w3.org/2000/svg', svg = document.createElementNS(namespace, 'svg');
  svg.style.cssText = 'position:fixed;width:0;height:0;overflow:hidden;pointer-events:none';
  svg.setAttribute('aria-hidden', 'true'); document.body.append(svg);
  const segments = [], cells = new Map(), tested = new Set(), intersections = new Map(), overlaps = new Map();
  let overlapLength = 0, totalLength = 0;
  try {
    for (const route of paths) {
      const path = document.createElementNS(namespace, 'path'); path.setAttribute('d', route.d ?? route.path); svg.append(path);
      const length = path.getTotalLength(); totalLength += length;
      // Common physical ports deliberately share their short attachment lead.
      // Exclude that lead when comparing overlap and crossing readability.
      const first = Math.min(24, length / 4), last = length - first, count = Math.max(1, Math.ceil((last - first) / 10));
      let previous = path.getPointAtLength(first);
      for (let index = 1; index <= count; index++) {
        const end = path.getPointAtLength(first + (last - first) * index / count), start = { x: previous.x, y: previous.y };
        previous = end;
        const segment = { id: route.id, a: start, b: { x: end.x, y: end.y }, index: segments.length };
        if (Math.hypot(end.x - start.x, end.y - start.y) < .01) continue;
        segments.push(segment);
        for (let x = Math.floor(Math.min(start.x, end.x) / 96); x <= Math.floor(Math.max(start.x, end.x) / 96); x++) {
          for (let y = Math.floor(Math.min(start.y, end.y) / 96); y <= Math.floor(Math.max(start.y, end.y) / 96); y++) {
            const key = `${x},${y}`; if (!cells.has(key)) cells.set(key, []); cells.get(key).push(segment);
          }
        }
      }
    }
    for (const cell of cells.values()) for (let i = 0; i < cell.length; i++) for (let j = i + 1; j < cell.length; j++) {
      const a = cell[i], b = cell[j]; if (a.id === b.id) continue;
      const key = a.index < b.index ? `${a.index}/${b.index}` : `${b.index}/${a.index}`;
      if (tested.has(key)) continue; tested.add(key);
      const rx = a.b.x - a.a.x, ry = a.b.y - a.a.y, sx = b.b.x - b.a.x, sy = b.b.y - b.a.y;
      const qx = b.a.x - a.a.x, qy = b.a.y - a.a.y, denominator = rx * sy - ry * sx;
      const rl = Math.hypot(rx, ry), sl = Math.hypot(sx, sy);
      const pair = [a.id, b.id].sort().join('/');
      if (Math.abs(denominator) / (rl * sl) < .005) {
        if (Math.abs(qx * ry - qy * rx) / rl > .35) continue;
        const from = (qx * rx + qy * ry) / rl, to = from + (sx * rx + sy * ry) / rl;
        const length = Math.max(0, Math.min(rl, Math.max(from, to)) - Math.max(0, Math.min(from, to)));
        overlapLength += length; if (length) overlaps.set(pair, (overlaps.get(pair) ?? 0) + length);
        continue;
      }
      const t = (qx * sy - qy * sx) / denominator, u = (qx * ry - qy * rx) / denominator;
      if (t <= .0001 || t >= .9999 || u <= .0001 || u >= .9999) continue;
      const point = { x: a.a.x + t * rx, y: a.a.y + t * ry };
      if (!intersections.has(pair)) intersections.set(pair, []);
      const prior = intersections.get(pair);
      if (!prior.some(other => Math.hypot(other.x - point.x, other.y - point.y) < 12)) prior.push(point);
    }
    return { crossingCount: [...intersections.values()].reduce((sum, points) => sum + points.length, 0),
      crossingPairs: intersections.size, overlapLength: Math.round(overlapLength), totalLength: Math.round(totalLength),
      sampleSpacing: 10, sharedLeadExcluded: 24, segments: segments.length,
      mostCrossedPairs: [...intersections].map(([pair, points]) => ({ pair, count: points.length })).sort((a, b) => b.count - a.count).slice(0, 15),
      mostOverlappedPairs: [...overlaps].map(([pair, length]) => ({ pair, length: Math.round(length) })).sort((a, b) => b.length - a.length).slice(0, 15) };
  } finally { svg.remove(); }
}

// Inspect individual rendered paths as well as crossings between different
// wires. A terminal fan can otherwise improve global scores while introducing
// a tiny loop or reversing over its own final lead.
function fanInInBrowser(bindingIds) {
  const routes = [], spacing = .5, clearance = 20;
  const bounds = [...document.querySelectorAll('.canvas-stage .resource-block')].map(node => ({
    id: node.dataset.nodeId, left: parseFloat(node.style.left), top: parseFloat(node.style.top),
    right: parseFloat(node.style.left) + parseFloat(getComputedStyle(node).width),
    bottom: parseFloat(node.style.top) + parseFloat(getComputedStyle(node).height),
  }));
  for (const id of bindingIds) {
    const path = [...document.querySelectorAll('.canvas-stage .signal-edge')].find(path => path.dataset.bindingId === id);
    if (!path) throw new Error(`Missing fan-in route ${id}`);
    const length = path.getTotalLength(), end = path.getPointAtLength(length), count = Math.max(1, Math.ceil(length / spacing));
    // Sample the complete route. A long excursion must not disappear merely
    // because its final lead is short or it leaves and re-enters the close-up.
    const points = Array.from({ length: count + 1 }, (_, index) => { const distance = length * index / count, point = path.getPointAtLength(distance); return { x: point.x, y: point.y, distance }; });
    const segments = points.slice(1).map((b, index) => ({ a: points[index], b, index }));
    const cells = new Map(), tested = new Set(), intersections = [], retraces = [];
    const region = { left: end.x - 360, right: end.x + 20, top: end.y - 260, bottom: end.y + 260 };
    const nearPort = point => point.x >= region.left && point.x <= region.right && point.y >= region.top && point.y <= region.bottom;
    const nearPortRanges = [];
    for (const segment of segments) {
      if (!nearPort(segment.a) && !nearPort(segment.b)) continue;
      const range = nearPortRanges.at(-1);
      if (range && Math.abs(range.end - segment.a.distance) < 1e-6) range.end = segment.b.distance;
      else nearPortRanges.push({ start: segment.a.distance, end: segment.b.distance });
      for (let x = Math.floor(Math.min(segment.a.x, segment.b.x) / 16); x <= Math.floor(Math.max(segment.a.x, segment.b.x) / 16); x++) {
        for (let y = Math.floor(Math.min(segment.a.y, segment.b.y) / 16); y <= Math.floor(Math.max(segment.a.y, segment.b.y) / 16); y++) {
          const key = `${x},${y}`; if (!cells.has(key)) cells.set(key, []); cells.get(key).push(segment);
        }
      }
    }
    for (const cell of cells.values()) for (let i = 0; i < cell.length; i++) for (let j = i + 1; j < cell.length; j++) {
      const a = cell[i], b = cell[j];
      if (Math.abs(a.index - b.index) <= 2) continue;
      const key = [a.index, b.index].sort((a, b) => a - b).join('/'); if (tested.has(key)) continue; tested.add(key);
      const rx = a.b.x - a.a.x, ry = a.b.y - a.a.y, sx = b.b.x - b.a.x, sy = b.b.y - b.a.y;
      const rl = Math.hypot(rx, ry), sl = Math.hypot(sx, sy); if (rl < 1e-6 || sl < 1e-6) continue;
      const qx = b.a.x - a.a.x, qy = b.a.y - a.a.y, denominator = rx * sy - ry * sx;
      if (Math.abs(denominator) / (rl * sl) < 1e-5) {
        if (Math.abs(qx * ry - qy * rx) / rl > .02) continue;
        const from = (qx * rx + qy * ry) / rl, to = from + (sx * rx + sy * ry) / rl;
        const overlap = Math.max(0, Math.min(rl, Math.max(from, to)) - Math.max(0, Math.min(from, to)));
        if (overlap > .02 && retraces.length < 12) retraces.push({ x: a.a.x, y: a.a.y, overlap, opposite: rx * sx + ry * sy < 0 });
        continue;
      }
      const t = (qx * sy - qy * sx) / denominator, u = (qx * ry - qy * rx) / denominator;
      if (t < -1e-5 || t > 1 + 1e-5 || u < -1e-5 || u > 1 + 1e-5) continue;
      const point = { x: a.a.x + t * rx, y: a.a.y + t * ry };
      if (!intersections.some(prior => Math.hypot(prior.x - point.x, prior.y - point.y) < .5)) intersections.push(point);
    }
    // This terminal rectangle is empty by construction: its left edge is 20px
    // beyond every nearby block's right edge. Feature Lab's physical gap is
    // 208px, leaving a 188px clear strip. Once a wire first enters it, neither
    // an obstacle nor its endpoint requires it to double back or leave again.
    const obstacles = bounds.filter(box => box.left < end.x - 1e-6 && box.bottom + clearance > region.top && box.top - clearance < region.bottom);
    const nearestRight = Math.max(end.x - 360, ...obstacles.map(box => Math.min(end.x, box.right)));
    const strip = { left: nearestRight + clearance, right: end.x + .01, top: region.top, bottom: region.bottom,
      physicalGap: end.x - nearestRight, clearance,
      limitingBlocks: obstacles.filter(box => Math.abs(box.right - nearestRight) < 1e-6).map(box => box.id) };
    if (strip.right - strip.left < 10) throw new Error(`${id}: no obstacle-free terminal strip is available for inspection`);
    const inStrip = point => point.x >= strip.left && point.x <= strip.right && point.y >= strip.top && point.y <= strip.bottom;
    const firstEntry = points.findIndex(inStrip);
    if (firstEntry < 0) throw new Error(`${id}: the rendered route never reaches its terminal strip`);
    const terminal = points.slice(firstEntry);
    const terminalWindow = length - terminal[0].distance;
    let reverseXDistance = 0, awayYDistance = 0, excursions = 0;
    for (let index = 1; index < terminal.length; index++) {
      reverseXDistance += Math.max(0, terminal[index - 1].x - terminal[index].x);
      awayYDistance += Math.max(0, Math.abs(terminal[index].y - end.y) - Math.abs(terminal[index - 1].y - end.y));
      if (!inStrip(terminal[index])) excursions++;
    }
    routes.push({ id, length, samples: points.length, inspectionWindow: length, completeRouteSampled: true,
      region, nearPortRanges, nearPortLength: nearPortRanges.reduce((sum, range) => sum + range.end - range.start, 0),
      terminalStrip: strip, firstEntryDistance: terminal[0].distance, terminalSamples: terminal.length, terminalWindow, excursions, intersections, retraces,
      reverseXDistance, awayYDistance, endpoint: { x: end.x, y: end.y } });
  }
  return { spacing, routes };
}

// A compact layout can deliberately share port leads. Each signal still needs
// a centerline outside its neighbors' 18px pointer-hit strokes somewhere else.
function selectableWiresInBrowser() {
  const cellSize = 32, radius = 9.5, cells = new Map(), routes = [];
  const key = point => `${Math.floor(point.x / cellSize)},${Math.floor(point.y / cellSize)}`;
  for (const path of document.querySelectorAll('.canvas-stage .signal-edge')) {
    const id = path.dataset.bindingId, length = path.getTotalLength(), count = Math.max(1, Math.ceil(length / 4));
    const points = Array.from({ length: count + 1 }, (_, index) => { const point = path.getPointAtLength(length * index / count); return { x: point.x, y: point.y }; });
    routes.push({ id, path, length });
    for (let index = 1; index < points.length; index++) {
      const a = points[index - 1], b = points[index], segment = { id, a, b };
      for (let x = Math.floor((Math.min(a.x, b.x) - radius) / cellSize); x <= Math.floor((Math.max(a.x, b.x) + radius) / cellSize); x++) {
        for (let y = Math.floor((Math.min(a.y, b.y) - radius) / cellSize); y <= Math.floor((Math.max(a.y, b.y) + radius) / cellSize); y++) {
          const cell = `${x},${y}`; if (!cells.has(cell)) cells.set(cell, []); cells.get(cell).push(segment);
        }
      }
    }
  }
  const distance = (point, a, b) => { const dx = b.x - a.x, dy = b.y - a.y, t = Math.max(0, Math.min(1, ((point.x - a.x) * dx + (point.y - a.y) * dy) / (dx * dx + dy * dy || 1))); return Math.hypot(point.x - a.x - t * dx, point.y - a.y - t * dy); };
  const regions = [], unselectable = [];
  for (const route of routes) {
    let region;
    for (let at = 28; at < route.length - 28; at += 4) {
      const point = route.path.getPointAtLength(at), peers = (cells.get(key(point)) ?? []).filter(segment => segment.id !== route.id);
      if (peers.every(segment => distance(point, segment.a, segment.b) > radius)) { region = { id: route.id, x: point.x, y: point.y }; break; }
    }
    if (region) regions.push(region); else unselectable.push(route.id);
  }
  return { hitRadius: radius, sampleSpacing: 4, regions, unselectable };
}

async function until(operation, message, timeout = 20000) {
  const end = Date.now() + timeout; let last;
  while (Date.now() < end) {
    try { const value = await operation(); if (value) return value; } catch (error) { last = error; }
    await pause(100);
  }
  throw new Error(`${message}${last ? `: ${last.message}` : ''}`);
}

async function connectNative() {
  let endpoint = process.env.OSG_NATIVE_PAGE_WS;
  if (!endpoint) {
    const response = await fetch(`${process.env.OSG_NATIVE_CDP_URL ?? 'http://127.0.0.1:9222'}/json/list`, { signal: AbortSignal.timeout(5000) });
    endpoint = (await response.json()).find(target => target.type === 'page' && !target.url.includes('profile-activation-window') && /127\.0\.0\.1:1420|localhost:1420|tauri\.localhost/.test(target.url))?.webSocketDebuggerUrl;
  }
  assert.ok(endpoint, 'The running native dev Cockpit must expose its WebView CDP page.');
  const socket = new WebSocket(endpoint), pending = new Map(), errors = []; let sequence = 0;
  await new Promise((resolve, reject) => {
    const timer = setTimeout(() => reject(new Error('Native page connection timed out')), 8000);
    socket.addEventListener('open', () => { clearTimeout(timer); resolve(); }, { once: true });
    socket.addEventListener('error', () => { clearTimeout(timer); reject(new Error('Native page connection failed')); }, { once: true });
  });
  socket.addEventListener('message', event => {
    const reply = JSON.parse(event.data);
    if(reply.method === 'Runtime.exceptionThrown') errors.push(reply.params.exceptionDetails);
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

test('Feature Lab routes both styles around blocks and keeps real native dragging responsive', { timeout: 240000 }, async () => {
  await mkdir(artifacts, { recursive: true });
  const native = await connectNative(), profileId = randomUUID(), name = `Routing acceptance ${profileId}`, checks = [];
  let created = false, originalStyling, pointerDown = false;
  const request = value => native.evaluate(async value => {
    const reply = await window.__TAURI_INTERNALS__.invoke('gateway_request', { request: { ...value, clientId: 'profile-routing-native' } });
    if (!reply.ok) throw new Error(`${value.command}: ${reply.error}`); return reply.state;
  }, value);
  const click = (label, scope = '.profiles-workspace') => until(() => native.evaluate((label, scope) => {
    const button = [...(document.querySelector(scope)?.querySelectorAll('button') ?? [])].find(button => (button.getAttribute('aria-label') ?? button.textContent.trim()) === label);
    if (!button || button.disabled) return false; button.click(); return true;
  }, label, scope), `Missing UI action ${label}`);
  const state = () => native.evaluate(() => {
    const buttons = [...document.querySelectorAll('.editor-save-actions button')], stage = document.querySelector('.canvas-stage');
    const world = document.querySelector('.canvas-world'), matrix = world ? new DOMMatrix(getComputedStyle(world).transform) : null;
    return { nodes: stage?.querySelectorAll('.resource-block').length, edges: stage?.querySelectorAll('.signal-edge').length,
      saveDisabled: buttons.find(button => button.textContent.trim() === 'Save profile')?.disabled,
      undoDisabled: buttons.find(button => button.getAttribute('aria-label') === 'Undo')?.disabled,
      view: matrix ? { x: matrix.e, y: matrix.f, zoom: matrix.a } : null };
  });
  const open = async () => {
    await native.evaluate(() => [...document.querySelectorAll('nav[aria-label="Main navigation"] button')].find(button => button.textContent.trim() === 'Profiles')?.click());
    await until(() => native.evaluate(name => {
      const stage = document.querySelector('.canvas-stage');
      if (document.querySelector('.profile-title-button')?.textContent.trim() === name && stage?.getBoundingClientRect().width > 0) return true;
      const edit = [...document.querySelectorAll('.profiles-workspace button')].find(button => button.getAttribute('aria-label') === `Edit ${name}`);
      if (edit) edit.click(); else {
        [...document.querySelectorAll('nav[aria-label="Main navigation"] button')].find(button => button.textContent.trim() === 'Profiles')?.click();
        [...document.querySelectorAll('.profiles-workspace button')].find(button => button.textContent.trim() === '← Back to Profiles')?.click();
      }
      return false;
    }, name), 'Open Feature Lab test copy');
    await until(async () => (await state()).edges === 73, 'All 73 Feature Lab routes must render');
    await native.evaluate(() => { document.querySelector('.canvas-stage').scrollIntoView({ block: 'center' }); });
    await pause(150);
  };
  const setStyle = async style => {
    assert.equal((await state()).saveDisabled, true, 'Style changes start from a clean graph');
    await click('Settings', '.sidebar-bottom');
    await click('Styling', '[role="tablist"][aria-label="Settings sections"]');
    await native.evaluate(style => {
      const select = document.querySelector('select[aria-label="Arrow style"]');
      if (!select) throw new Error('Arrow style control is missing');
      select.value = style; select.dispatchEvent(new Event('change', { bubbles: true }));
      const radius = document.querySelector('input[aria-label="Arrow corner radius"]');
      if (radius) { radius.value = '24'; radius.dispatchEvent(new Event('input', { bubbles: true })); }
    }, style);
    await open();
    await until(() => native.evaluate(style => [...document.querySelectorAll('.signal-edge')].every(path => path.dataset.arrowStyle === style), style), `${style} routes did not render`);
    assert.equal((await state()).saveDisabled, true, 'Global styling does not dirty profiles');
  };
  const capture = async filename => {
    const shot = await native.send('Page.captureScreenshot', { format: 'png' });
    await writeFile(resolve(artifacts, filename), Buffer.from(shot.data, 'base64'));
  };
  const pathGeometry = () => native.evaluate(() => {
    const world = document.querySelector('.canvas-world'), origin = world.getBoundingClientRect(), zoom = new DOMMatrix(getComputedStyle(world).transform).a;
    const nodes = [...document.querySelectorAll('.canvas-stage .resource-block')].map(node => {
      const x = parseFloat(node.style.left), y = parseFloat(node.style.top), width = parseFloat(getComputedStyle(node).width), height = parseFloat(getComputedStyle(node).height);
      const anchors = [...node.querySelectorAll('[data-connection-source],[data-connection-target]')].map(handle => {
        const rect = handle.getBoundingClientRect(), direction = handle.hasAttribute('data-connection-source') ? 'output' : 'input';
        return { direction, portId: handle.dataset.connectionPort, x: direction === 'output' ? x + width : x, y: (rect.top + rect.height / 2 - origin.top) / zoom };
      });
      return { id: node.dataset.nodeId, x, y, width, height, anchors };
    });
    let sampledPoints = 0, totalLength = 0; const collisions = [], paths = [];
    for (const path of document.querySelectorAll('.canvas-stage .signal-edge')) {
      const id = path.dataset.bindingId, length = path.getTotalLength(), count = Math.ceil(length / 2);
      totalLength += length;
      const start = path.getPointAtLength(0), end = path.getPointAtLength(length);
      const bad = new Set();
      for (let index = 0; index <= count; index++) {
        const point = path.getPointAtLength(length * index / count); sampledPoints++;
        for (const node of nodes) if (!bad.has(node.id) && point.x > node.x + .5 && point.x < node.x + node.width - .5 && point.y > node.y + .5 && point.y < node.y + node.height - .5) {
          bad.add(node.id); collisions.push({ bindingId: id, nodeId: node.id, x: point.x, y: point.y });
        }
      }
      paths.push({ id, d: path.getAttribute('d'), start: { x: start.x, y: start.y }, end: { x: end.x, y: end.y }, length });
    }
    return { nodes, paths, sampledPoints, collisions, totalLength };
  });
  const mouse = (type, point, extra = {}) => native.send('Input.dispatchMouseEvent', { type, x: point.x, y: point.y, ...extra });
  const endDrag = async point => {
    await mouse('mouseReleased', point, { button: 'left', buttons: 0, clickCount: 1 }); pointerDown = false;
  };
  const inspectFanIn = async (style, bindings) => {
    const ids = bindings.filter(binding => binding.to.nodeId === 'virtual-aux').map(binding => binding.id);
    assert.ok(ids.length > 8, 'Feature Lab must exercise a dense virtual-aux fan-in');
    await click('Fit all');
    const wheelAction = await native.evaluate(async () => {
      const { canvasNavigation } = await import('/src/lib/canvas-navigation.ts'); let current;
      const unsubscribe = canvasNavigation.subscribe(value => { current = value; }); unsubscribe(); return current.wheelAction;
    });
    const position = () => native.evaluate(() => {
      const stage = document.querySelector('.canvas-stage'), bounds = stage.getBoundingClientRect();
      const node = [...stage.querySelectorAll('.resource-block')].find(node => node.dataset.nodeId === 'virtual-aux'), handle = node.querySelector('[data-connection-target] .port-dot'), dot = handle.getBoundingClientRect();
      const zoom = new DOMMatrix(getComputedStyle(document.querySelector('.canvas-world')).transform).a;
      return { x: dot.left + dot.width / 2, y: dot.top + dot.height / 2, zoom,
        desired: { x: bounds.left + bounds.width * .7, y: bounds.top + bounds.height * .5 },
        stage: { left: bounds.left, top: bounds.top, right: bounds.right, bottom: bounds.bottom }, viewport: { width: innerWidth, height: innerHeight } };
    });
    let at = await position();
    await mouse('mouseWheel', at.desired, { deltaX: at.x - at.desired.x, deltaY: at.y - at.desired.y, modifiers: wheelAction === 'zoom' ? 2 : 0 });
    await native.evaluate(() => new Promise(resolve => requestAnimationFrame(() => resolve(true))));
    for (let index = 0; index < 40 && (at = await position()).zoom < 1.2; index++) {
      await mouse('mouseWheel', { x: at.x, y: at.y }, { deltaX: 0, deltaY: -40, modifiers: wheelAction === 'zoom' ? 0 : 2 });
      await native.evaluate(() => new Promise(resolve => requestAnimationFrame(() => resolve(true))));
    }
    at = await position(); assert.ok(at.zoom >= 1.2, 'Fan-in inspection uses at least 120% physical canvas zoom');
    assert.ok(at.x > at.stage.left && at.x < Math.min(at.stage.right, at.viewport.width) && at.y > at.stage.top && at.y < Math.min(at.stage.bottom, at.viewport.height), 'The real input port must be visible for close-up inspection');
    const x = Math.max(at.stage.left, at.x - 540), y = Math.max(at.stage.top, at.y - 240);
    const clip = { x, y, width: Math.min(720, at.stage.right - x, at.viewport.width - x), height: Math.min(480, at.stage.bottom - y, at.viewport.height - y), scale: 1 };
    const shot = await native.send('Page.captureScreenshot', { format: 'png', clip });
    await writeFile(resolve(artifacts, `${style}-virtual-aux-fanin.png`), Buffer.from(shot.data, 'base64'));
    const result = await native.evaluate(fanInInBrowser, ids);
    await writeFile(resolve(artifacts, `${style}-virtual-aux-fanin.json`), JSON.stringify({ position: at, clip, ...result }, null, 2));
    assert.equal((await state()).edges, 73, 'Close-up keeps all routes visible');
    assert.equal((await state()).saveDisabled, true, 'Close-up pan and zoom never edit the profile');
    for (const route of result.routes) {
      assert.equal(route.excursions, 0, `${style} ${route.id} must stay in the obstacle-free input corridor after entering it`);
      assert.deepEqual(route.intersections, [], `${style} ${route.id} must not cross itself near the virtual-aux input`);
      assert.deepEqual(route.retraces, [], `${style} ${route.id} must not retrace its own path near the virtual-aux input`);
      assert.ok(route.reverseXDistance < .5, `${style} ${route.id} must approach the input monotonically: ${route.reverseXDistance.toFixed(2)}px horizontal reversal`);
      assert.ok(route.awayYDistance < .5, `${style} ${route.id} must merge towards the input without a vertical hairpin: ${route.awayYDistance.toFixed(2)}px away from the port`);
    }
    await click('Fit all');
    return { zoom: at.zoom, routeCount: ids.length, screenshot: `${style}-virtual-aux-fanin.png`, ...result };
  };
  const drag = async style => {
    await click('Fit all');
    const before = await pathGeometry(), previous = new Map(before.paths.map(path => [path.id, path.d]));
    // Choose an unobstructed destination using actual world geometry. Requiring
    // a visible topmost header keeps this a real pointer test at any window size.
    const move = await until(() => native.evaluate(() => {
      const stage = document.querySelector('.canvas-stage'), bounds = stage.getBoundingClientRect(), zoom = new DOMMatrix(getComputedStyle(document.querySelector('.canvas-world')).transform).a;
      const nodes = [...stage.querySelectorAll('.resource-block')];
      for (const node of nodes.filter(node => node.classList.contains('processing'))) {
        const heading = node.querySelector('.block-heading'), rect = heading.getBoundingClientRect(), x = rect.left + rect.width / 2, y = rect.top + rect.height / 2;
        if (x < bounds.left + 10 || x > bounds.right - 10 || y < Math.max(0, bounds.top) + 10 || y > Math.min(innerHeight, bounds.bottom) - 10 || !heading.contains(document.elementFromPoint(x, y))) continue;
        const nx = parseFloat(node.style.left), ny = parseFloat(node.style.top), nw = parseFloat(getComputedStyle(node).width), nh = parseFloat(getComputedStyle(node).height);
        for (const [screenX, screenY] of [[12, 0], [-12, 0], [0, 12], [0, -12], [12, 12], [-12, -12]]) {
          if (x + screenX < bounds.left + 4 || x + screenX > bounds.right - 4 || y + screenY < Math.max(0, bounds.top) + 4 || y + screenY > Math.min(innerHeight, bounds.bottom) - 4) continue;
          const dx = screenX / zoom, dy = screenY / zoom;
          const overlaps = nodes.some(other => { if (other === node) return false; const ox = parseFloat(other.style.left), oy = parseFloat(other.style.top), ow = parseFloat(getComputedStyle(other).width), oh = parseFloat(getComputedStyle(other).height); return nx + dx < ox + ow + 22 && nx + dx + nw > ox - 22 && ny + dy < oy + oh + 22 && ny + dy + nh > oy - 22; });
          if (!overlaps) return { id: node.dataset.nodeId, x, y, screenX, screenY, original: { x: nx, y: ny } };
        }
      }
      return null;
    }), 'A visible Feature Lab transform needs an unobstructed drag destination');
    const incident = await native.evaluate(async ({ id, profileId }) => {
      const reply = await window.__TAURI_INTERNALS__.invoke('gateway_request', { request: { command: 'profile_assignments_get', id: profileId, clientId: 'profile-routing-native' } });
      return reply.state.profiles.document.bindings.filter(binding => binding.from.nodeId === id || binding.to.nodeId === id).map(binding => binding.id);
    }, { id: move.id, profileId });
    await native.evaluate(() => {
      window.__routingAcceptance = { frames: [], longTasks: [], running: true };
      const data = window.__routingAcceptance; let last = performance.now();
      const frame = now => { data.frames.push(now - last); last = now; if (data.running) requestAnimationFrame(frame); }; requestAnimationFrame(frame);
      data.observer = new PerformanceObserver(entries => data.longTasks.push(...entries.getEntries().map(entry => ({ start: entry.startTime, duration: entry.duration }))));
      data.observer.observe({ type: 'longtask', buffered: false });
    });
    await mouse('mouseMoved', move); await mouse('mousePressed', move, { button: 'left', buttons: 1, clickCount: 1 }); pointerDown = true;
    const times = [], previewChanged = new Set();
    for (let index = 1; index <= 12; index++) {
      const point = { x: move.x + move.screenX * index / 12, y: move.y + move.screenY * index / 12 }, start = performance.now();
      await mouse('mouseMoved', point, { button: 'left', buttons: 1 });
      await native.evaluate(() => new Promise(resolve => requestAnimationFrame(() => resolve(true))));
      times.push(performance.now() - start);
      const preview = await native.evaluate(() => [...document.querySelectorAll('.signal-edge')].map(path => ({ id: path.dataset.bindingId, d: path.getAttribute('d') })));
      assert.equal(preview.length, 73, 'Dragging keeps every connection visible');
      for (const path of preview) if (path.d !== previous.get(path.id)) previewChanged.add(path.id);
      await pause(8);
    }
    for (const id of previewChanged) assert.ok(incident.includes(id), `Unrelated route ${id} must remain stable during a block drag`);
    const during = await native.evaluate(() => { const data = window.__routingAcceptance; data.running = false; data.observer.disconnect(); return { frames: data.frames, longTasks: data.longTasks }; });
    const dropStart = performance.now(); await endDrag({ x: move.x + move.screenX, y: move.y + move.screenY });
    await until(async () => !(await state()).saveDisabled, 'Dropping a moved block must create an undoable edit');
    await native.evaluate(() => new Promise(resolve => requestAnimationFrame(() => resolve(true))));
    const dropMs = performance.now() - dropStart, after = await pathGeometry();
    assert.equal(after.paths.length, 73, 'Routing after a valid drop keeps all connections');
    assert.deepEqual(after.collisions, [], `${style} paths after drop must avoid all blocks`);
    assert.ok(percentile(times, .95) < 80, `Responsive drag p95 must be below 80ms, received ${percentile(times, .95).toFixed(1)}ms`);
    await click('Undo', '.editor-save-actions');
    await until(async () => (await state()).saveDisabled, 'Undo must restore the clean graph');
    const restored = await pathGeometry();
    assert.deepEqual(restored.paths.map(path => ({ id: path.id, d: path.d })), before.paths.map(path => ({ id: path.id, d: path.d })), 'Undo restores stable routing');
    return { style, nodeId: move.id, samples: times, p50: percentile(times, .5), p95: percentile(times, .95), dropMs, previewChanged: [...previewChanged], ...during };
  };
  const reroute = async style => {
    await click('Fit all');
    const target = await until(() => native.evaluate(() => {
      const stage = document.querySelector('.canvas-stage').getBoundingClientRect();
      for (const path of document.querySelectorAll('.signal-edge-hit')) {
        const length = path.getTotalLength(), matrix = path.getScreenCTM();
        for (const fraction of [.5, .35, .65, .2, .8]) {
          const at = path.getPointAtLength(length * fraction), point = new DOMPoint(at.x, at.y).matrixTransform(matrix);
          if (point.x < stage.left + 30 || point.x > stage.right - 30 || point.y < Math.max(0, stage.top) + 60 || point.y > Math.min(innerHeight, stage.bottom) - 30) continue;
          if (document.elementFromPoint(point.x, point.y)?.closest('.signal-edge-hit') === path) return { id: path.dataset.connectionId, x: point.x, y: point.y };
        }
      }
      return null;
    }), 'A visible wire is required for a real double-click reroute edit');
    const handles = () => native.evaluate(id => [...document.querySelectorAll('.connection-reroute')].filter(handle => handle.dataset.rerouteBinding === id).map(handle => ({
      x: Number(handle.getAttribute('cx')), y: Number(handle.getAttribute('cy')), index: Number(handle.dataset.rerouteIndex),
    })), target.id);
    const handlePoint = () => until(() => native.evaluate(id => {
      const handle = [...document.querySelectorAll('.connection-reroute')].find(handle => handle.dataset.rerouteBinding === id), rect = handle?.getBoundingClientRect();
      if (!rect) return null; const x = rect.left + rect.width / 2, y = rect.top + rect.height / 2;
      return handle.contains(document.elementFromPoint(x, y)) ? { x, y } : null;
    }, target.id), 'The reroute handle must be visible and draggable');
    const key = async (key, code, value) => {
      await native.send('Input.dispatchKeyEvent', { type: 'keyDown', key, code, windowsVirtualKeyCode: value });
      await native.send('Input.dispatchKeyEvent', { type: 'keyUp', key, code, windowsVirtualKeyCode: value });
    };
    await mouse('mouseMoved', target);
    for (const count of [1, 2]) {
      await mouse('mousePressed', target, { button: 'left', buttons: 1, clickCount: count });
      await mouse('mouseReleased', target, { button: 'left', buttons: 0, clickCount: count });
      if (count === 1) await pause(60);
    }
    await until(async () => (await handles()).length === 1, `${style} double-click must add one reroute point`);
    assert.equal((await state()).saveDisabled, false);
    const inserted = (await handles())[0];
    await click('Undo', '.editor-save-actions'); await until(async () => !(await handles()).length, 'Undo removes the reroute point');
    assert.equal((await state()).saveDisabled, true, 'Undoing insertion restores the clean graph');
    await click('Redo', '.editor-save-actions'); await until(async () => (await handles()).length === 1, 'Redo restores the reroute point');
    let moved = false, movement;
    for (const [dx, dy] of [[6, 0], [-6, 0], [0, 6], [0, -6], [6, 6], [-6, -6]]) {
      const from = await handlePoint(), to = { x: from.x + dx, y: from.y + dy };
      await mouse('mouseMoved', from); await mouse('mousePressed', from, { button: 'left', buttons: 1, clickCount: 1 }); pointerDown = true;
      await mouse('mouseMoved', to, { button: 'left', buttons: 1 });
      await native.evaluate(() => new Promise(resolve => requestAnimationFrame(() => resolve(true))));
      if (await native.evaluate(() => document.querySelector('.canvas-stage').classList.contains('wire-blocked'))) {
        await key('Escape', 'Escape', 27); await endDrag(to); continue;
      }
      await endDrag(to);
      const point = (await handles())[0];
      if (point && Math.hypot(point.x - inserted.x, point.y - inserted.y) > 1) { movement = point; moved = true; break; }
    }
    assert.ok(moved, `${style} reroute point must move through a real mouse drag`);
    assert.deepEqual((await pathGeometry()).collisions, [], 'Moving a handle keeps rendered curves clear of blocks');
    await click('Undo', '.editor-save-actions');
    await until(async () => { const point = (await handles())[0]; return point && Math.hypot(point.x - inserted.x, point.y - inserted.y) < .01; }, 'Undo restores the previous reroute point');
    await click('Redo', '.editor-save-actions');
    await until(async () => { const point = (await handles())[0]; return point && Math.hypot(point.x - movement.x, point.y - movement.y) < .01; }, 'Redo restores the moved reroute point');
    await click('Save profile'); await until(async () => (await state()).saveDisabled, 'Save reroute point');
    const saved = (await request({ command: 'profile_assignments_get', id: profileId })).profiles;
    const savedRoute = saved.document.editor.wireRoutes?.find(route => route.bindingId === target.id);
    assert.equal(savedRoute?.handles?.length, 1, 'The native backend persists reroute metadata');
    assert.ok(Math.hypot(savedRoute.handles[0].x - movement.x, savedRoute.handles[0].y - movement.y) < .01);
    await click('← Back to Profiles'); await open();
    await native.evaluate(id => {
      const path = [...document.querySelectorAll('.signal-edge-hit')].find(path => path.dataset.connectionId === id);
      path.focus(); path.dispatchEvent(new KeyboardEvent('keydown', { key: 'Enter', bubbles: true }));
    }, target.id);
    await until(async () => (await handles()).length === 1, 'Saved reroute point must reappear after reopening');
    const reopened = (await handles())[0]; assert.ok(Math.hypot(reopened.x - movement.x, reopened.y - movement.y) < .01);
    await native.evaluate(id => [...document.querySelectorAll('.connection-reroute')].find(handle => handle.dataset.rerouteBinding === id).focus(), target.id);
    await key('Delete', 'Delete', 46); await until(async () => !(await handles()).length, 'Delete removes only the reroute handle');
    assert.equal((await state()).edges, 73, 'Deleting a reroute handle retains its connection');
    await click('Undo', '.editor-save-actions'); await until(async () => (await handles()).length === 1, 'Undo restores a removed handle');
    await click('Reset bends'); await until(async () => !(await handles()).length, 'Reset bends removes authored points');
    await click('Save profile'); await until(async () => (await state()).saveDisabled, 'Save reset reroute geometry');
    const reset = (await request({ command: 'profile_assignments_get', id: profileId })).profiles;
    assert.equal(reset.document.editor.wireRoutes?.some(route => route.bindingId === target.id) ?? false, false, 'Reset clears the complete authored route');
    await click('← Back to Profiles'); await open();
    return { bindingId: target.id, inserted, moved: movement, persisted: savedRoute.handles[0], reopened, deleteKeepsConnection: true, resetSaved: true };
  };
  try {
    await native.send('Runtime.enable');
    await native.send('Page.reload', { ignoreCache: true });
    await until(() => native.evaluate(() => typeof window.__TAURI_INTERNALS__?.invoke === 'function'), 'Native gateway unavailable');
    await native.send('Page.bringToFront');
    originalStyling = await native.evaluate(() => localStorage.getItem('osg-canvas-styling'));
    const original = (await request({ command: 'profile_assignments_get', id: sourceId })).profiles;
    const baseBindings = await native.evaluate(async document => {
      const { profileTabDocument } = await import('/src/lib/profile-vehicle-tabs.ts');
      return profileTabDocument(document, '').bindings;
    }, original.document);
    assert.equal(baseBindings.length, 73, 'Acceptance uses the real 73-route Feature Lab Base tab');
    const document = { ...original.document, id: profileId, name, revision: 1 };
    // A saved demo may already have been arranged with ELK. Reproduce the
    // recorded starting geometry only in this disposable acceptance copy.
    if (process.env.OSG_ROUTING_START_GEOMETRY_PATH) {
      const starting = JSON.parse((await readFile(process.env.OSG_ROUTING_START_GEOMETRY_PATH, 'utf8')).replace(/^\uFEFF/, ''));
      const endpoints = bindings => bindings.map(({id,from,to}) => ({id,from,to})).sort((a,b) => a.id.localeCompare(b.id));
      assert.deepEqual(endpoints(starting.bindings), endpoints(baseBindings), 'Recorded starting geometry must match every actual Base connection');
      const ids = new Set(starting.nodes.map(node => node.id)), bindingIds = new Set(baseBindings.map(binding => binding.id));
      assert.equal(ids.size, 42);
      document.editor = { ...document.editor,
        positions: [...document.editor.positions.filter(position => !ids.has(position.nodeId)), ...starting.nodes.map(node => ({nodeId:node.id,...node.position}))],
        wireRoutes: document.editor.wireRoutes?.filter(route => !bindingIds.has(route.bindingId)) };
      if (!document.editor.wireRoutes?.length) delete document.editor.wireRoutes;
    }
    await request({ command: 'profile_editor_save', document, assignments: original.assignments, expectedRevision: null }); created = true;
    await native.send('Page.reload', { ignoreCache: true }); await open();
    let elkLayout;
    if (process.env.OSG_ROUTING_AUTO_LAYOUT === 'elk') {
      const before = await pathGeometry(), started = performance.now();
      await click('Auto-layout');
      await until(async () => (await state()).saveDisabled === false && await native.evaluate(() => document.querySelector('button[aria-label="Auto-layout"]')?.getAttribute('aria-busy') === 'false'), 'ELK did not finish its layout');
      const arranged = await pathGeometry(), layoutMs = performance.now() - started;
      assert.equal(arranged.paths.length, 73); assert.equal(arranged.nodes.length, 42);
      assert.deepEqual(arranged.collisions, [], 'ELK rendering must avoid all blocks');
      const coordinates = geometry => geometry.nodes.map(node => ({ id: node.id, x: node.x, y: node.y }));
      assert.notDeepEqual(coordinates(arranged), coordinates(before), 'ELK changes the actual block arrangement');
      await click('Undo', '.editor-save-actions'); await until(async () => (await state()).saveDisabled, 'One Undo must restore the original layout');
      assert.deepEqual(coordinates(await pathGeometry()), coordinates(before), 'Undo restores every block in one step');
      await click('Redo', '.editor-save-actions'); await until(async () => !(await state()).saveDisabled, 'Redo must restore the ELK layout');
      assert.deepEqual(coordinates(await pathGeometry()), coordinates(arranged), 'Redo restores the complete ELK arrangement');
      await click('Save profile'); await until(async () => (await state()).saveDisabled, 'Save ELK layout');
      const saved = (await request({ command: 'profile_assignments_get', id: profileId })).profiles.document;
      const generated = saved.editor.wireRoutes?.filter(route => route.automatic) ?? [];
      assert.equal(generated.length, 73, 'Native persistence retains every generated ELK corridor');
      const visibleIds = new Set(arranged.nodes.map(node => node.id));
      assert.deepEqual(saved.editor.positions.filter(position => !visibleIds.has(position.nodeId)), original.document.editor.positions.filter(position => !visibleIds.has(position.nodeId)), 'ELK leaves hidden vehicle tabs unchanged');
      assert.deepEqual(saved.bindings, original.document.bindings, 'Layout never changes the signal graph');
      await click('← Back to Profiles'); await open();
      assert.deepEqual(coordinates(await pathGeometry()), coordinates(arranged), 'Native reopen retains the ELK arrangement');
      elkLayout = { layoutMs, nodes: arranged.nodes.length, routes: generated.length, atomicUndoRedo: true, nativeSaveReopen: true, hiddenTabsPreserved: true };
      console.log(`ELK: 42 blocks / 73 routes; arrange ${layoutMs.toFixed(1)}ms; native save/reopen and atomic Undo/Redo pass`);
    }
    const baseline = process.env.OSG_ROUTING_BASELINE_PATH
      ? JSON.parse((await readFile(process.env.OSG_ROUTING_BASELINE_PATH, 'utf8')).replace(/^\uFEFF/, '')) : null;
    const baselineRoutes = baseline?.result?.value?.styles ?? baseline?.styles;
    for (const style of ['smooth', 'orthogonal']) {
      await setStyle(style);
      const initial = await state(); assert.equal(initial.nodes, 42); assert.equal(initial.edges, 73);
      assert.equal(initial.saveDisabled, true); assert.equal(initial.undoDisabled, true);
      await click('Fit all'); await click('Zoom in'); await click('Zoom out'); await click('Fit all');
      assert.equal((await state()).saveDisabled, true, 'Fit and zoom remain view-only');
      assert.equal((await state()).undoDisabled, true, 'Canvas navigation does not enter graph undo history');
      const geometry = await pathGeometry();
      const segments = geometry.paths.map(path => ({ id: path.id,
        cubics: (path.d.match(/C/g) ?? []).length,
        corners: (path.d.match(/A/g) ?? []).length }));
      const segmentCounts = { cubicPieces: segments.reduce((sum, route) => sum + route.cubics, 0),
        singleCubicRoutes: segments.filter(route => route.cubics === 1).length,
        roundedCorners: segments.reduce((sum, route) => sum + route.corners, 0) };
      await writeFile(resolve(artifacts, `${style}-segments.json`), JSON.stringify(segments, null, 2));
      await writeFile(resolve(artifacts, `${style}-geometry.json`), JSON.stringify(geometry, null, 2));
      assert.equal(geometry.paths.length, 73); assert.deepEqual(geometry.collisions, [], `${style} rendered SVG paths must avoid blocks`);
      for (const path of geometry.paths) {
        assert.doesNotMatch(path.d, /NaN|Infinity/);
        if (style === 'smooth') assert.doesNotMatch(path.d, /[LQAHV]/, 'Smooth arrows use cubic Beziers throughout');
        const binding = baseBindings.find(binding => binding.id === path.id);
        for (const [endpoint, at, direction] of [[binding.from, path.start, 'output'], [binding.to, path.end, 'input']]) {
          const node = geometry.nodes.find(node => node.id === endpoint.nodeId);
          const anchor = node.anchors.find(anchor => anchor.direction === direction && (!anchor.portId || anchor.portId === endpoint.portId));
          assert.ok(anchor, `${path.id} requires a visible ${direction} port`);
          const distance = Math.hypot(at.x - anchor.x, at.y - anchor.y);
          assert.ok(distance < .7, `${path.id} must meet its physical ${direction} port center: ${JSON.stringify({ nodeId: node.id, at, anchor, distance })}`);
        }
      }
      await capture(`${style}.png`);
      const quality = await native.evaluate(qualityInBrowser, geometry.paths);
      const selection = await native.evaluate(selectableWiresInBrowser);
      await writeFile(resolve(artifacts, `${style}-selection.json`), JSON.stringify(selection, null, 2));
      if (elkLayout) assert.deepEqual(selection.unselectable, [], `${style} every ELK wire needs an individually selectable region`);
      const beforeQuality = baselineRoutes?.[style] ? await native.evaluate(qualityInBrowser, baselineRoutes[style]) : null;
      await writeFile(resolve(artifacts, `${style}-quality.json`), JSON.stringify({ before: beforeQuality, after: quality }, null, 2));
      const fanIn = await inspectFanIn(style, baseBindings);
      // ELK changes node placement and may bundle common port leads. Keep that
      // overlap measurement explicit, while gating actual selectability above.
      if (beforeQuality) for (const metric of (elkLayout ? ['crossingCount', 'crossingPairs', 'totalLength'] : ['crossingCount', 'crossingPairs', 'overlapLength'])) {
        assert.ok(quality[metric] <= beforeQuality[metric], `${style} rendered ${metric} must not regress: ${quality[metric]} vs baseline ${beforeQuality[metric]}`);
      }
      const interaction = await drag(style), handles = await reroute(style);
      checks.push({ style, paths: geometry.paths.length, sampledPoints: geometry.sampledPoints, totalLength: geometry.totalLength, segmentCounts, quality, beforeQuality, selection, fanIn, interaction, handles });
      console.log(`${style}: 73 clear routes; drag p95 ${interaction.p95.toFixed(1)}ms; drop ${interaction.dropMs.toFixed(1)}ms`);
      console.log(`${style} readability: ${JSON.stringify({ before: beforeQuality, after: quality })}`);
    }
    await writeFile(resolve(artifacts, 'evidence.json'), JSON.stringify({ actualNativeCockpit: true, sourceProfileName: original.document.name, elkLayout, checks }, null, 2));
    console.log(`Native routing evidence: ${artifacts}`);
  } catch(error) {
    await capture('failure.png').catch(()=>{});
    const body=await native.evaluate(()=>document.body.innerText.slice(0,12000)).catch(()=>null);
    await writeFile(resolve(artifacts,'failure.json'),JSON.stringify({error:error.message,body,errors:native.errors},null,2));
    throw error;
  } finally {
    if (pointerDown) await endDrag({ x: 10, y: 10 }).catch(() => {});
    await native.evaluate(() => { const data = window.__routingAcceptance; if (data) { data.running = false; data.observer?.disconnect(); } }).catch(() => {});
    if (created) {
      await native.evaluate(() => [...document.querySelectorAll('.profiles-workspace button')].find(button => button.textContent.trim() === '← Back to Profiles')?.click()).catch(() => {});
      await pause(100);
      await native.evaluate(() => [...document.querySelectorAll('dialog button')].find(button => button.textContent.trim() === 'Discard and continue')?.click()).catch(() => {});
      const current = (await request({ command: 'profile_assignments_get', id: profileId })).profiles;
      await request({ command: 'profile_delete', id: profileId, expectedRevision: current.document.revision });
    }
    if (originalStyling !== undefined) await native.evaluate(value => { value === null ? localStorage.removeItem('osg-canvas-styling') : localStorage.setItem('osg-canvas-styling', value); }, originalStyling).catch(() => {});
    await native.send('Page.reload', { ignoreCache: true }).catch(() => {}); native.close();
  }
});
