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
  assert.equal(graph.edges.length, 3);
  assert.equal(graph.connectionCount,4);
  assert.equal(graph.nodes.find(node=>node.component.id==='panel').alternateConnections,1);
  assert.equal(graph.edges.find(route=>route.connection.to==='panel').connection.from,'gateway');
  assert.equal(graph.edges.some(item => item.connection.id === 'old-route'),false);
  assert.equal(topologyConnectionLabel(remembered), 'NETWORK · Last known');
  assert.equal(topologyConnectionLabel(edge('unknown', 'host', 'panel', 'unknown')), 'Unknown transport');
  assert.equal(topologyConnectionLabel(edge('software', 'host', 'panel', 'software')), 'Virtual');
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

test('larger rigs share one horizontal protocol bus with a normal-sized host', () => {
  const devices = Array.from({ length: 28 }, (_, index) => component('device-' + index, 'controller', [edge('edge-' + index, 'host', 'device-' + index)]));
  const inventory = [component('host', 'host'), ...devices], graph = systemTopology(inventory);
  assert.equal(graph.nodes.length, 29);
  assert.ok(graph.height < 400);
  const connected=graph.nodes.filter(node=>node.component.id!=='host'),host=graph.nodes.find(node=>node.component.id==='host');
  assert.equal(new Set(connected.map(node => node.y)).size, 1);
  assert.equal(new Set(connected.map(node => node.x)).size,28);
  assert.equal(graph.rows.length,1);assert.equal(graph.rows[0].count,28);
  assert.equal(host.height,TOPOLOGY_NODE_HEIGHT);
  assert.ok(host.x<connected[0].x);
  assert.equal(host.y+host.height/2,graph.rows[0].busY);
  for (let index = 0; index < graph.nodes.length; index++) for (const other of graph.nodes.slice(index + 1)) {
    const node = graph.nodes[index];
    assert.ok(node.x + TOPOLOGY_NODE_WIDTH <= other.x || other.x + TOPOLOGY_NODE_WIDTH <= node.x || node.y + TOPOLOGY_NODE_HEIGHT <= other.y || other.y + TOPOLOGY_NODE_HEIGHT <= node.y, `${node.component.id} overlaps ${other.component.id}`);
  }
  assert.deepEqual(plain(systemTopology([...inventory].reverse())), plain(graph));
  const disconnected=systemTopology(inventory.map(item => ({ ...item, presence: 'offline' })));
  assert.equal(disconnected.edges.length,0);assert.equal(disconnected.offline.count,28);
  assert.ok(disconnected.nodes.filter(n=>n.component.id!=='host').every(n=>n.x>disconnected.nodes.find(n=>n.component.id==='host').x+TOPOLOGY_NODE_WIDTH));
  const initial = topologyInitialView(graph.width, graph.height, 1280, 640), fitted = topologyFit(graph.width, graph.height, 1280, 640);
  assert.equal(initial.zoom, 0.8);
  assert.ok(fitted.zoom < initial.zoom);
  assert.ok(graph.edges.some(edge => edge.path.includes(' L ')), 'connections use orthogonal routing lanes');
  const small = topologyInitialView(500, 300, 1280, 640);
  assert.equal(small.zoom, 1); assert.ok(small.x > 0 && small.y > 0);
});

// Verify actual segment geometry, including cycles and skipped-level edges.
test('shared connection buses avoid cards and expose exact protocol endpoints', () => {
  const links = [edge('h-a', 'host', 'a'), edge('h-b', 'host', 'b'), edge('a-c', 'a', 'c', 'can'), edge('b-c', 'b', 'c'), edge('c-a', 'c', 'a'), edge('a-b', 'a', 'b'), edge('h-c', 'host', 'c'), edge('parallel', 'host', 'a', 'software'), edge('return-a', 'a', 'host'), edge('return-b', 'b', 'host')];
  const graph = systemTopology([component('host', 'host', links), ...['a', 'b', 'c'].map(id => component(id))]);
  const routes=[...graph.edges,...graph.rows.flatMap(row=>[{id:row.id,path:row.path},{id:row.id+' anchor',path:row.anchor}]),{id:'host',path:graph.trunk}].filter(route=>route.path);
  for (const route of routes) {
    const values = route.path.match(/-?\d+(?:\.\d+)?/g).map(Number);
    const points = [];
    for (let i = 0; i < values.length; i += 2) points.push({ x: values[i], y: values[i + 1] });
    for (let i = 1; i < points.length; i++) {
      const a = points[i - 1], b = points[i];
      assert.ok(a.x === b.x || a.y === b.y);
      for (const node of graph.nodes) {
        const intersects = a.x === b.x
          ? a.x > node.x && a.x < node.x + TOPOLOGY_NODE_WIDTH && Math.max(a.y, b.y) > node.y && Math.min(a.y, b.y) < node.y + node.height
          : a.y > node.y && a.y < node.y + node.height && Math.max(a.x, b.x) > node.x && Math.min(a.x, b.x) < node.x + TOPOLOGY_NODE_WIDTH;
        assert.ok(!intersects, `${route.connection?.id ?? route.id} passes through ${node.component.id}`);
      }
    }
  }
  for (const route of graph.edges) {
    const to=graph.nodes.find(n=>n.component.id===route.connection.to);
    assert.equal(route.start.x,to.x+TOPOLOGY_NODE_WIDTH/2);
    assert.equal(route.end.x,route.start.x);assert.equal(route.end.y,to.y);
    assert.ok(route.start.y<route.end.y);
  }
});

test('offline, unavailable and unknown devices sit on the right with no current or remembered lines',()=>{
 const items=[component('host','host'),component('live','controller',[edge('live','host','live')]),...['offline','unavailable','unknown'].map(state=>component(state,'controller',[edge(state,'host',state,'usb',false)],state))];
 const graph=systemTopology(items),active=graph.nodes.filter(n=>n.component.presence==='connected');
 assert.equal(graph.edges.length,1);assert.equal(graph.offline.count,3);
 for(const n of graph.nodes.filter(n=>n.component.presence!=='connected'))assert.ok(n.x>Math.max(...active.map(n=>n.x+TOPOLOGY_NODE_WIDTH)));
});

test('protocol bus rows follow connection type independently of names and discovery order',()=>{
 const protocols=['unknown','software','bluetooth','network','can','usb'];
 const devices=protocols.map((transport,index)=>component('device-'+index,'controller',[edge(transport,'host','device-'+index,transport)]));
 const items=[component('host','host'),...devices],graph=systemTopology(items);
 const column=graph.nodes.filter(node=>node.component.id!=='host').sort((a,b)=>a.y-b.y);
 assert.deepEqual(plain(column.map(node=>node.component.connections[0].transport)),[...protocols].reverse());
 const starts=graph.edges.slice().sort((a,b)=>a.start.y-b.start.y);
 assert.deepEqual(plain(starts.map(route=>route.connection.transport)),[...protocols].reverse());
 assert.deepEqual(plain(systemTopology([...items].reverse())),plain(graph));
});

test('each gateway gets its own attached-device row immediately beneath its parent bus',()=>{
 const gateway=component('gateway','gateway',[edge('gateway','host','gateway')]);
 const child=component('panel','osg',[edge('usb','host','panel'),edge('can','gateway','panel','can')]);
 const sibling=component('pedals','controller',[edge('pedals','host','pedals')]);
 const graph=systemTopology([component('host','host'),gateway,child,sibling]);
 assert.deepEqual(plain(graph.rows.map(row=>[row.parentId,row.transport,row.count])),[['host','usb',2],['gateway','can',1]]);
 const parent=graph.nodes.find(node=>node.component.id==='gateway'),node=graph.nodes.find(node=>node.component.id==='panel');
 assert.ok(node.y>parent.y+parent.height);assert.ok(graph.rows[1].anchor);
 assert.equal(graph.nodes.filter(node=>node.component.id==='panel').length,1);
 assert.equal(node.alternateConnections,1);
});
