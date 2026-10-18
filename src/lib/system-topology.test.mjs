import test from 'node:test';
import assert from 'node:assert/strict';
import { loadModules } from './test-module-loader.mjs';
const modules = await loadModules(['./system-topology']);
const { systemTopology, topologyFit, topologyZoom, topologyConnectionLabel, TOPOLOGY_NODE_WIDTH, TOPOLOGY_NODE_HEIGHT } = modules.get('./system-topology');
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
