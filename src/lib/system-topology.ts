import type { SystemComponent, SystemConnection } from './system-inventory';

export const TOPOLOGY_NODE_WIDTH = 238;
export const TOPOLOGY_NODE_HEIGHT = 116;

export interface TopologyNode {
  component: SystemComponent;
  context: boolean;
  x: number;
  y: number;
  height: number;
  alternateConnections: number;
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

export interface TopologyBusRow {
  id: string; parentId: string | null; transport: SystemConnection['transport']; label: string;
  x: number; y: number; busY: number; width: number; count: number; path: string; anchor: string;
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
  const kind = connection.transport === 'software' ? 'Virtual' : connection.transport === 'unknown' ? 'Unknown transport' : connection.transport.toUpperCase();
  return connection.current ? kind : connection.observed === false ? `${kind} · Configured (unconfirmed)` : `${kind} · Last known`;
}

/** Each device has one visual bus attachment. Additional observed routes remain in its details. */
export function systemTopology(components: readonly SystemComponent[], context: readonly SystemComponent[] = components) {
  const inventory = new Map([...context, ...components].map(component => [component.id, component]));
  const matches = new Set(components.map(component => component.id)), visible = new Set(matches);
  if (inventory.has('host')) visible.add('host');
  const live = [...new Map([...inventory.values()].flatMap(component => component.connections).map(connection => [connection.id, connection])).values()]
    .filter(connection => connection.current && inventory.get(connection.from)?.presence === 'connected' && inventory.get(connection.to)?.presence === 'connected')
    .sort((a, b) => a.id.localeCompare(b.id));
  for (let changed = true; changed;) {
    changed = false;
    for (const connection of live) if (visible.has(connection.to) && !visible.has(connection.from)) { visible.add(connection.from); changed = true; }
  }
  const shown = [...visible].map(id => inventory.get(id)).filter((component): component is SystemComponent => !!component).sort(compare);
  const connections = live.filter(connection => visible.has(connection.from) && visible.has(connection.to));
  const offline = shown.filter(component => component.id !== 'host' && component.presence !== 'connected');
  const active = shown.filter(component => component.id !== 'host' && component.presence === 'connected');
  const attachments = new Map<string, SystemConnection>();
  const sourceRank = (id: string) => inventory.get(id)?.type === 'gateway' ? 0 : id === 'host' ? 1 : 2;
  // Gateway routes take precedence over a second direct USB observation. Never duplicate a physical card.
  for (const component of active) {
    const candidates = connections.filter(connection => connection.to === component.id && connection.from !== component.id)
      .sort((a, b) => sourceRank(a.from) - sourceRank(b.from) || transportOrder[a.transport] - transportOrder[b.transport] || a.id.localeCompare(b.id));
    for (const candidate of candidates) {
      const ancestors = new Set([component.id]);
      let parent: string | undefined = candidate.from;
      while (parent && !ancestors.has(parent)) { ancestors.add(parent); parent = attachments.get(parent)?.from; }
      if (parent) continue;
      attachments.set(component.id, candidate); break;
    }
  }
  type Group = { id: string; parentId: string | null; transport: SystemConnection['transport']; components: SystemComponent[] };
  const groups = new Map<string, Group>();
  for (const component of active) {
    const connection = attachments.get(component.id), transport = connection?.transport ?? 'unknown';    const parentId = connection ? inventory.get(connection.from)?.type === 'gateway' ? connection.from : inventory.has('host') ? 'host' : connection.from : null;
    const id = `${parentId ?? 'unlinked'}:${transport}`;
    if (!groups.has(id)) groups.set(id, { id, parentId, transport, components: [] });
    groups.get(id)!.components.push(component);
  }
  const nodes: TopologyNode[] = [], rows: TopologyBusRow[] = [], placed = new Set<string>();
  const positions = new Map<string, TopologyNode>();  const rowForComponent = new Map<string, TopologyBusRow>();
  let top = 44;
  const protocolName = (transport: SystemConnection['transport']) => transport === 'software' ? 'Virtual' : transport === 'unknown' ? 'Unknown transport' : transport.toUpperCase();
  const childGroups = (parentId: string | null) => [...groups.values()].filter(group => group.parentId === parentId)
    .sort((a, b) => transportOrder[a.transport] - transportOrder[b.transport] || a.id.localeCompare(b.id));
  function place(group: Group) {
    if (placed.has(group.id)) return;
    placed.add(group.id);
    const parent = group.parentId ? positions.get(group.parentId) : undefined;
    const x = parent ? parent.x + TOPOLOGY_NODE_WIDTH / 2 : 318;
    const y = top, busY = y + 42;
    const members = [...group.components].sort(compare);
    const row: TopologyBusRow = { id: group.id, parentId: group.parentId, transport: group.transport,
      label: group.parentId && group.parentId !== 'host' ? `${protocolName(group.transport)} · ${inventory.get(group.parentId)!.name}` : group.parentId ? protocolName(group.transport) : 'No current connection',
      x, y, busY, width: 48 + members.length * (TOPOLOGY_NODE_WIDTH + 24) - 24, count: members.length, path: '', anchor: '' };
    members.forEach((component, index) => {
      const node = { component, context: !matches.has(component.id), x: x + 48 + index * (TOPOLOGY_NODE_WIDTH + 24), y: y + 78, height: TOPOLOGY_NODE_HEIGHT,
        alternateConnections: Math.max(0, connections.filter(connection => connection.to === component.id).length - (attachments.has(component.id) ? 1 : 0)) };
      nodes.push(node); positions.set(component.id, node); rowForComponent.set(component.id, row);
    });
    if (group.parentId) {
      const last = positions.get(members[members.length - 1].id)!;
      row.path = `M ${x} ${busY} L ${last.x + TOPOLOGY_NODE_WIDTH / 2} ${busY}`;
      if (parent) row.anchor = `M ${x} ${parent.y + parent.height} L ${x} ${busY}`;
    }
    rows.push(row); top += TOPOLOGY_NODE_HEIGHT + 128;
    for (const component of members) for (const children of childGroups(component.id)) place(children);
  }
  for (const group of [...childGroups('host'), ...childGroups(null)]) place(group);
  // Incomplete or cyclic provider relationships remain inspectable, without inventing an attachment.
  for (const group of groups.values()) place(group);
  const host = shown.find(component => component.id === 'host');
  const rootRows = rows.filter(row => row.parentId === 'host');
  let trunk = '';
  if (host) {
    const center = rootRows.length ? (rootRows[0].busY + rootRows[rootRows.length - 1].busY) / 2 : 150;
    nodes.unshift({ component: host, context: !matches.has(host.id), x: 40, y: center - TOPOLOGY_NODE_HEIGHT / 2, height: TOPOLOGY_NODE_HEIGHT, alternateConnections: 0 });
    if (rootRows.length) trunk = `M ${40 + TOPOLOGY_NODE_WIDTH} ${center} L 318 ${center} M 318 ${rootRows[0].busY} L 318 ${rootRows[rootRows.length - 1].busY}`;
  }
  const activeRight = Math.max(40 + TOPOLOGY_NODE_WIDTH, ...nodes.map(node => node.x + TOPOLOGY_NODE_WIDTH));
  const offlineX = activeRight + 96;
  offline.sort((a, b) => Math.min(6, ...a.connections.map(connection => transportOrder[connection.transport])) - Math.min(6, ...b.connections.map(connection => transportOrder[connection.transport])) || compare(a, b));
  offline.forEach((component, index) => nodes.push({ component, context: !matches.has(component.id), x: offlineX, y: 96 + index * 120, height: 100, alternateConnections: 0 }));
  const edges: TopologyEdge[] = [...attachments.values()].map(connection => {
    const node = positions.get(connection.to)!, row = rowForComponent.get(connection.to)!;
    const start = { x: node.x + TOPOLOGY_NODE_WIDTH / 2, y: row.busY }, end = { x: start.x, y: node.y };
    return { connection, start, end, x: end.x, y: end.y, path: `M ${start.x} ${start.y} L ${end.x} ${end.y}`, label: topologyConnectionLabel(connection) };
  });
  return { nodes, edges, rows, trunk, connectionCount: connections.length,
    offline: offline.length ? { x: offlineX - 16, y: 44, width: TOPOLOGY_NODE_WIDTH + 32, height: offline.length * 120 + 60, count: offline.length } : null,
    width: (offline.length ? offlineX + TOPOLOGY_NODE_WIDTH : activeRight) + 40, height: Math.max(top + 24, offline.length * 120 + 120, 300) };
}

export function topologyFit(width: number, height: number, viewportWidth: number, viewportHeight: number): TopologyView {
  const zoom = Math.max(0.08, Math.min(1, (Math.max(1, viewportWidth) - 48) / width, (Math.max(1, viewportHeight) - 48) / height));
  return { x: (viewportWidth - width * zoom) / 2, y: (viewportHeight - height * zoom) / 2, zoom };
}

/** Start at a readable scale on larger rigs. Fit remains an explicit all-node
 * overview; keyboard focus and pan make every component reachable at this scale. */
export function topologyInitialView(width: number, height: number, viewportWidth: number, viewportHeight: number): TopologyView {
  const fitted = topologyFit(width, height, viewportWidth, viewportHeight);
  const zoom = Math.max(0.8, fitted.zoom);
  return { x: width * zoom <= viewportWidth ? (viewportWidth - width * zoom) / 2 : 24, y: height * zoom <= viewportHeight ? (viewportHeight - height * zoom) / 2 : 24, zoom };
}

export function topologyZoom(view: TopologyView, multiplier: number, point: { x: number; y: number }): TopologyView {
  const zoom = Math.max(0.08, Math.min(2, view.zoom * multiplier));
  return { x: point.x - (point.x - view.x) * zoom / view.zoom, y: point.y - (point.y - view.y) * zoom / view.zoom, zoom };
}
