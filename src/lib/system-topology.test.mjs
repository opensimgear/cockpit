import test from 'node:test';
import assert from 'node:assert/strict';
import { loadModules } from './test-module-loader.mjs';
const modules = await loadModules(['./system-topology']);
const { systemTopology, topologyFit, topologyInitialView, topologyZoom, topologyConnectionLabel, TOPOLOGY_NODE_WIDTH, TOPOLOGY_NODE_HEIGHT } = modules.get('./system-topology');
const plain = value => JSON.parse(JSON.stringify(value));
const edge = (id, from, to, transport = 'usb', current = true) => ({ id, from, to, transport, current });
const component = (id, type = 'controller', connections = [], presence = 'connected') => ({ id, name: id, type, role: type, presence, connections, hidden: null, enabled: null, owner: null, diagnostics: [], firstSeen: 1, lastSeen: 1 });

test('all known component categories and offline or unknown nodes are rendered without invented connections', () => {
  const inventory = ['host', 'gateway', 'osg', 'controller', 'virtual', 'integration', 'companion'].map((type, index) => component(type, type, [], index % 2 ? 'offline' : 'unknown'));
  const graph = systemTopology(inventory);
  assert.equal(graph.nodes.length, inventory.length);
  assert.equal(graph.edges.length, 0);
  for (const node of graph.nodes) assert.ok(Number.isFinite(node.x) && Number.isFinite(node.y));
  assert.equal(new Set(graph.nodes.map(node => `${node.x}:${node.y}`)).size, inventory.length);
});

test('filtered graphs retain exact upstream context without bringing unrelated siblings back', () => {
  const host = component('host', 'host');
  const gateway = component('gateway', 'gateway', [edge('host-gateway', 'host', 'gateway')]);
  const panel = component('panel', 'osg', [edge('gateway-panel', 'gateway', 'panel', 'can')]);
  const other = component('other', 'controller', [edge('host-other', 'host', 'other')]);
  const graph = systemTopology([panel], [host, gateway, panel, other]);
  assert.deepEqual(plain(graph.nodes.map(node => node.component.id).sort()), ['gateway', 'host', 'panel']);
  assert.deepEqual(plain(graph.nodes.filter(node => node.context).map(node => node.component.id).sort()), ['gateway', 'host']);
  assert.equal(graph.edges.length, 2);
});

test('layout and parallel transport labels are deterministic despite discovery order or repeated edge sightings', () => {
  const usb = edge('panel-usb', 'host', 'panel');
  const can = edge('panel-can', 'gateway', 'panel', 'can');
  const remembered = edge('old-route', 'host', 'panel', 'network', false);
  const inventory = [component('host', 'host', [usb]), component('gateway', 'gateway', [edge('gateway-usb', 'host', 'gateway')]), component('panel', 'osg', [can, usb, remembered]), component('software', 'integration', [edge('software-host', 'host', 'software', 'software')])];
  const graph = systemTopology(inventory);
  assert.deepEqual(plain(systemTopology([...inventory].reverse())), plain(graph));
  assert.equal(graph.nodes.filter(node => node.component.id === 'panel').length, 1);
  assert.equal(graph.edges.length, 5);
  assert.notEqual(graph.edges.find(item => item.connection.id === 'panel-usb').path, graph.edges.find(item => item.connection.id === 'old-route').path);
  assert.equal(topologyConnectionLabel(remembered), 'NETWORK · Last known');
  assert.equal(topologyConnectionLabel(edge('unknown', 'host', 'panel', 'unknown')), 'Unknown transport');
  assert.equal(topologyConnectionLabel(edge('software', 'host', 'panel', 'software')), 'Software');
});

test('cyclic sightings do not loop and fitting includes every node with finite anchored zoom', () => {
  const graph = systemTopology([component('host', 'host'), component('a', 'osg', [edge('a-b', 'a', 'b')]), component('b', 'osg', [edge('b-a', 'b', 'a')])]);
  assert.equal(graph.nodes.length, 3);
  const view = topologyFit(graph.width, graph.height, 1000, 600);
  for (const node of graph.nodes) {
    assert.ok(node.x * view.zoom + view.x >= 0);
    assert.ok((node.x + TOPOLOGY_NODE_WIDTH) * view.zoom + view.x <= 1000);
    assert.ok(node.y * view.zoom + view.y >= 0);
    assert.ok((node.y + TOPOLOGY_NODE_HEIGHT) * view.zoom + view.y <= 600);
  }
  const anchor = { x: 180, y: 230 }, next = topologyZoom(view, 1.5, anchor);
  assert.ok(Math.abs((anchor.x - view.x) / view.zoom - (anchor.x - next.x) / next.zoom) < 1e-8);
  assert.ok(Math.abs((anchor.y - view.y) / view.zoom - (anchor.y - next.y) / next.zoom) < 1e-8);
  assert.equal(topologyZoom(view, 1e9, anchor).zoom, 2);
  assert.equal(topologyZoom(view, 1e-9, anchor).zoom, 0.08);
});

test('larger rigs keep one column per level and separate routing lanes at readable initial zoom', () => {
  const devices = Array.from({ length: 28 }, (_, index) => component('device-' + index, 'controller', [edge('edge-' + index, 'host', 'device-' + index)]));
  const inventory = [component('host', 'host'), ...devices], graph = systemTopology(inventory);
  assert.equal(graph.nodes.length, 29);
  assert.ok(graph.height >= 28 * (TOPOLOGY_NODE_HEIGHT + 64));
  assert.equal(new Set(graph.nodes.filter(node => node.component.id !== 'host').map(node => node.x)).size, 1);
  for (let index = 0; index < graph.nodes.length; index++) for (const other of graph.nodes.slice(index + 1)) {
    const node = graph.nodes[index];
    assert.ok(node.x + TOPOLOGY_NODE_WIDTH <= other.x || other.x + TOPOLOGY_NODE_WIDTH <= node.x || node.y + TOPOLOGY_NODE_HEIGHT <= other.y || other.y + TOPOLOGY_NODE_HEIGHT <= node.y, `${node.component.id} overlaps ${other.component.id}`);
  }
  assert.deepEqual(plain(systemTopology([...inventory].reverse())), plain(graph));
  assert.deepEqual(plain(systemTopology(inventory.map(item => ({ ...item, presence: 'offline', name: item.name }))).nodes.map(node => [node.component.id, node.x, node.y])), plain(graph.nodes.map(node => [node.component.id, node.x, node.y])));
  const initial = topologyInitialView(graph.width, graph.height, 1280, 640), fitted = topologyFit(graph.width, graph.height, 1280, 640);
  assert.equal(initial.zoom, 0.8);
  assert.ok(fitted.zoom < initial.zoom);
  assert.ok(graph.edges.some(edge => edge.path.includes(' L ')), 'connections use orthogonal routing lanes');
  const small = topologyInitialView(500, 300, 1280, 640);
  assert.equal(small.zoom, 1); assert.ok(small.x > 0 && small.y > 0);
});

// Verify actual segment geometry, including cycles and skipped-level edges.
test('routes avoid cards and keep vertical lanes and labels separate', () => {
  const links = [edge('h-a', 'host', 'a'), edge('h-b', 'host', 'b'), edge('a-c', 'a', 'c', 'can'), edge('b-c', 'b', 'c'), edge('c-a', 'c', 'a'), edge('a-b', 'a', 'b'), edge('h-c', 'host', 'c'), edge('parallel', 'host', 'a', 'software'), edge('return-a', 'a', 'host'), edge('return-b', 'b', 'host')];
  const graph = systemTopology([component('host', 'host', links), ...['a', 'b', 'c'].map(id => component(id))]);
  const verticals = [];
  for (const route of graph.edges) {
    const values = route.path.match(/-?\d+(?:\.\d+)?/g).map(Number);
    const points = [];
    for (let i = 0; i < values.length; i += 2) points.push({ x: values[i], y: values[i + 1] });
    for (let i = 1; i < points.length; i++) {
      const a = points[i - 1], b = points[i];
      assert.ok(a.x === b.x || a.y === b.y);
      if (a.x === b.x && a.y !== b.y) verticals.push({ x: a.x, top: Math.min(a.y, b.y), bottom: Math.max(a.y, b.y) });
      for (const node of graph.nodes) {
        const intersects = a.x === b.x
          ? a.x > node.x && a.x < node.x + TOPOLOGY_NODE_WIDTH && Math.max(a.y, b.y) > node.y && Math.min(a.y, b.y) < node.y + node.height
          : a.y > node.y && a.y < node.y + node.height && Math.max(a.x, b.x) > node.x && Math.min(a.x, b.x) < node.x + TOPOLOGY_NODE_WIDTH;
        assert.ok(!intersects, `${route.connection.id} passes through ${node.component.id}`);
      }
    }
  }
  for (let i = 0; i < verticals.length; i++) for (const b of verticals.slice(i + 1)) {
    const a = verticals[i];
    assert.ok(a.x !== b.x || a.bottom <= b.top || b.bottom <= a.top, 'vertical routes share a lane');
  }
  for (let i = 0; i < graph.edges.length; i++) for (const b of graph.edges.slice(i + 1)) {
    const a = graph.edges[i];
    assert.ok(Math.abs(a.y - b.y) >= 20 || Math.abs(a.x - b.x) > 280, 'connection labels overlap');
  }
});
