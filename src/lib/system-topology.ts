import type { SystemComponent, SystemConnection } from './system-inventory';

export const TOPOLOGY_NODE_WIDTH = 238;
export const TOPOLOGY_NODE_HEIGHT = 116;

export interface TopologyNode {
  component: SystemComponent;
  context: boolean;
  x: number;
  y: number;
  height: number;
}

export interface TopologyEdge {
  connection: SystemConnection;
  path: string;
  x: number;
  y: number;
  label: string;
  start: { x: number; y: number };
  end: { x: number; y: number };
}

export interface TopologyView { x: number; y: number; zoom: number }

const typeOrder: Record<SystemComponent['type'], number> = {
  host: 0, gateway: 1, osg: 2, controller: 3, virtual: 4, integration: 5, companion: 6,
};
const transportOrder: Record<SystemConnection['transport'], number> = { usb: 0, can: 1, network: 2, bluetooth: 3, software: 4, unknown: 5 };

function compare(a: SystemComponent, b: SystemComponent): number {
  return typeOrder[a.type] - typeOrder[b.type] || a.name.localeCompare(b.name) || a.id.localeCompare(b.id);
}

export function topologyConnectionLabel(connection: SystemConnection): string {
  const kind = connection.transport === 'software' ? 'Software' : connection.transport === 'unknown' ? 'Unknown transport' : connection.transport.toUpperCase();
  return connection.current ? kind : connection.observed === false ? `${kind} · Configured (unconfirmed)` : `${kind} · Last known`;
}

/** Only live links are drawn; disconnected devices form a separate unwired group. */
export function systemTopology(components: readonly SystemComponent[], context: readonly SystemComponent[] = components) {
  const inventory = new Map([...context, ...components].map(c => [c.id, c]));
  const matches = new Set(components.map(c => c.id)), visible = new Set(matches);
  if (inventory.has('host'))
    visible.add('host');
  const connections = [...new Map([...inventory.values()].flatMap(c => c.connections).map(c => [c.id, c])).values()]
    .filter(c => c.current && inventory.get(c.from)?.presence === 'connected' && inventory.get(c.to)?.presence === 'connected').sort((a, b) => a.id.localeCompare(b.id));
  for (let changed = true; changed;) {
    changed = false;
    for (const c of connections)
      if (visible.has(c.to) && !visible.has(c.from)) {
        visible.add(c.from);
        changed = true;
      }
  }
  const shown = [...visible].map(id => inventory.get(id)).filter((c): c is SystemComponent => !!c).sort(compare);
  const shownEdges = connections.filter(c => visible.has(c.from) && visible.has(c.to));
  const connectionRank = (component: SystemComponent) => Math.min(6, ...(component.presence === 'connected' ? shownEdges.filter(edge => edge.to === component.id) : component.connections).map(edge => transportOrder[edge.transport]));
  const byConnection = (a: SystemComponent, b: SystemComponent) => connectionRank(a) - connectionRank(b) || compare(a, b);
  const offline = shown.filter(c => c.id !== 'host' && c.presence !== 'connected').sort(byConnection);
  const active = shown.filter(c => !offline.includes(c));
  const depth = new Map<string, number>(), queue: string[] = [];
  if (visible.has('host')) {
    depth.set('host', 0);
    queue.push('host');
  }
  for (let i = 0; i < queue.length; i++)
    for (const c of shownEdges) {
      if (c.from !== queue[i] || depth.has(c.to))
        continue;
      depth.set(c.to, depth.get(c.from)! + 1);
      queue.push(c.to);
    }
  for (const c of active)
    if (!depth.has(c.id))
      depth.set(c.id, 1);
  const groups = [...new Set(active.map(c => depth.get(c.id)!))].sort((a, b) => a - b).map(level => ({ level, components: active.filter(c => depth.get(c.id) === level).sort(byConnection) }));
  const gap = 120, rowGap = 24, inset = 40;
  const activeWidth = Math.max(1, groups.length) * (TOPOLOGY_NODE_WIDTH + gap) - gap;
  const groupHeight = (count: number) => count * TOPOLOGY_NODE_HEIGHT + Math.max(0, count - 1) * rowGap;
  const height = Math.max(200, ...groups.map(group => groupHeight(group.components.length)), offline.length * 120 + 44) + inset * 2;
  const nodes: TopologyNode[] = [];
  groups.forEach((group, column) => {
    const top = (height - groupHeight(group.components.length)) / 2;
    group.components.forEach((component, row) => nodes.push({ component, context: !matches.has(component.id), x: inset + column * (TOPOLOGY_NODE_WIDTH + gap), y: top + row * (TOPOLOGY_NODE_HEIGHT + rowGap), height: TOPOLOGY_NODE_HEIGHT }));
  });
  const offlineX = inset + activeWidth + 96;
  const offlineTop = (height - (offline.length * 120 + 44)) / 2;
  offline.forEach((component, index) => nodes.push({ component, context: !matches.has(component.id), x: offlineX, y: offlineTop + 52 + index * 120, height: 100 }));
  const positions = new Map(nodes.map(n => [n.component.id, n]));
  const protocols = (id: string, outgoing: boolean) => [...new Set(shownEdges.filter(e => outgoing ? e.from === id : e.to === id).map(e => e.transport))].sort((a,b)=>transportOrder[a]-transportOrder[b]);
  const portY = (node: TopologyNode, c: SystemConnection, outgoing: boolean) => {
    const list = protocols(node.component.id, outgoing);
    return node.y + node.height / 2 + (list.indexOf(c.transport) - (list.length - 1) / 2) * 24;
  };
  const edges: TopologyEdge[] = shownEdges.map(connection => {
    const from = positions.get(connection.from)!, to = positions.get(connection.to)!, fromLevel = depth.get(connection.from)!, toLevel = depth.get(connection.to)!;
    const start = { x: from.x + TOPOLOGY_NODE_WIDTH, y: portY(from, connection, true) };
    const end = { x: to.x, y: portY(to, connection, false) };
    const outgoing = protocols(from.component.id, true);
    const offset = (outgoing.length - 1 - outgoing.indexOf(connection.transport)) * 10;
    const exitX = start.x + 40 + offset, approachX = end.x - 24;
    const nextLevel = groups[groups.findIndex(group => group.level === fromLevel) + 1]?.level;
    let path: string;
    if (toLevel === nextLevel) {
      path = `M ${start.x} ${start.y} L ${exitX} ${start.y} L ${exitX} ${end.y} L ${end.x} ${end.y}`;
    }
    else {
      const railY = 16 + offset;
      path = `M ${start.x} ${start.y} L ${exitX} ${start.y} L ${exitX} ${railY} L ${approachX} ${railY} L ${approachX} ${end.y} L ${end.x} ${end.y}`;
    }
    return { connection, path, x: end.x, y: end.y, start, end, label: topologyConnectionLabel(connection) };
  });
  return { nodes, edges, offline: offline.length ? { x: offlineX - 16, y: offlineTop, width: TOPOLOGY_NODE_WIDTH + 32, height: offline.length * 120 + 60, count: offline.length } : null,
    width: (offline.length ? offlineX + TOPOLOGY_NODE_WIDTH : inset + activeWidth) + inset, height };
}

export function topologyFit(width: number, height: number, viewportWidth: number, viewportHeight: number): TopologyView {
  const zoom = Math.max(0.08, Math.min(1, (Math.max(1, viewportWidth) - 48) / width, (Math.max(1, viewportHeight) - 48) / height));
  return { x: (viewportWidth - width * zoom) / 2, y: (viewportHeight - height * zoom) / 2, zoom };
}

/** Start at a readable scale on larger rigs. Fit remains an explicit all-node
 * overview; keyboard focus and pan make every component reachable at this scale. */
export function topologyInitialView(width: number, height: number, viewportWidth: number, viewportHeight: number): TopologyView {
  const fitted = topologyFit(width, height, viewportWidth, viewportHeight);
  const zoom = Math.min(1, (Math.max(1, viewportWidth) - 48) / width, Math.max(0.65, fitted.zoom));
  return { x: width * zoom <= viewportWidth ? (viewportWidth - width * zoom) / 2 : 24, y: height * zoom <= viewportHeight ? (viewportHeight - height * zoom) / 2 : 24, zoom };
}

export function topologyZoom(view: TopologyView, multiplier: number, point: { x: number; y: number }): TopologyView {
  const zoom = Math.max(0.08, Math.min(2, view.zoom * multiplier));
  return { x: point.x - (point.x - view.x) * zoom / view.zoom, y: point.y - (point.y - view.y) * zoom / view.zoom, zoom };
}
