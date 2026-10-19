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
}

export interface TopologyView { x: number; y: number; zoom: number }

const typeOrder: Record<SystemComponent['type'], number> = {
  host: 0, gateway: 1, osg: 2, controller: 3, virtual: 4, integration: 5, companion: 6,
};

function compare(a: SystemComponent, b: SystemComponent): number {
  return typeOrder[a.type] - typeOrder[b.type] || a.name.localeCompare(b.name) || a.id.localeCompare(b.id);
}

export function topologyConnectionLabel(connection: SystemConnection): string {
  const kind = connection.transport === 'software' ? 'Software' : connection.transport === 'unknown' ? 'Unknown transport' : connection.transport.toUpperCase();
  return connection.current ? kind : connection.observed === false ? `${kind} · Configured (unconfirmed)` : `${kind} · Last known`;
}

/** The graph uses observed edges only. Filters retain the host and upstream context. */
export function systemTopology(components: readonly SystemComponent[], context: readonly SystemComponent[] = components) {
  const inventory = new Map([...context, ...components].map(component => [component.id, component]));
  const matches = new Set(components.map(component => component.id));
  const visible = new Set(matches);
  if (inventory.has('host')) visible.add('host');
  const connections = [...new Map([...inventory.values()].flatMap(component => component.connections).map(connection => [connection.id, connection])).values()]
    .sort((a, b) => a.id.localeCompare(b.id));
  // Follow upstream edges only, so filtering does not bring every sibling back.
  for (let changed = true; changed;) {
    changed = false;
    for (const connection of connections) {
      if (visible.has(connection.to) && !visible.has(connection.from) && inventory.has(connection.from)) {
        visible.add(connection.from);
        changed = true;
      }
    }
  }
  const shown = [...visible].map(id => inventory.get(id)).filter((component): component is SystemComponent => !!component).sort(compare);
  const shownEdges = connections.filter(connection => visible.has(connection.from) && visible.has(connection.to));
  const depth = new Map<string, number>();
  if (visible.has('host')) depth.set('host', 0);
  const queue = visible.has('host') ? ['host'] : [];
  for (let index = 0; index < queue.length; index++) {
    const parent = queue[index];
    for (const connection of shownEdges) {
      if (connection.from !== parent || depth.has(connection.to)) continue;
      depth.set(connection.to, (depth.get(parent) ?? 0) + 1);
      queue.push(connection.to);
    }
  }
  // Disconnected and unknown devices are still first-class nodes, with no invented edge.
  for (const component of shown) if (!depth.has(component.id)) depth.set(component.id, 1);
  const columns = new Map<number, SystemComponent[]>();
  for (const component of shown) {
    const level = depth.get(component.id)!;
    columns.set(level, [...(columns.get(level) ?? []), component]);
  }
  const levels = [...columns.keys()].sort((a, b) => a - b);
  const rank = new Map<string, number>();
  for (const level of levels) {
    const parentRank = (id: string) => Math.min(Infinity, ...shownEdges.filter(edge => edge.to === id && (depth.get(edge.from) ?? level) < level).map(edge => rank.get(edge.from) ?? Infinity));
    columns.get(level)!.sort((a, b) => parentRank(a.id) - parentRank(b.id) || compare(a, b));
    columns.get(level)!.forEach((component, index) => rank.set(component.id, index));
  }
  const incident = (id: string, outgoing: boolean) => shownEdges.filter(edge => outgoing ? edge.from === id : edge.to === id);
  const nodeHeight = (id: string) => Math.max(TOPOLOGY_NODE_HEIGHT, 40 + Math.max(incident(id, true).length, incident(id, false).length) * 26);
  const crossEdges = shownEdges.filter(edge => depth.get(edge.to)! !== depth.get(edge.from)! + 1);
  const top = 48 + crossEdges.length * 32;
  const height = top + Math.max(1, ...levels.map(level => columns.get(level)!.reduce((sum, component) => sum + nodeHeight(component.id) + 64, 0))) + 48;
  const nodes: TopologyNode[] = [];
  const rootReturns = shownEdges.filter(edge => depth.get(edge.to) === levels[0]).length;
  let layerX = rootReturns ? 300 + rootReturns * 22 : 40;
  for (const level of levels) {
    let y = top;
    for (const component of columns.get(level)!) {
      const h = nodeHeight(component.id);
      nodes.push({ component, context: !matches.has(component.id), x: layerX, y, height: h });
      y += h + 64;
    }
    const count = shownEdges.filter(edge => depth.get(edge.from) === level || depth.get(edge.to)! - 1 === level).length;
    layerX += TOPOLOGY_NODE_WIDTH + 300 + count * 22;
  }
  const positions = new Map(nodes.map(node => [node.component.id, node]));
  const ports = (node: TopologyNode, connection: SystemConnection, outgoing: boolean) => {
    const list = incident(node.component.id, outgoing).sort((a, b) => {
      const otherA = positions.get(outgoing ? a.to : a.from)!;
      const otherB = positions.get(outgoing ? b.to : b.from)!;
      return otherA.y - otherB.y || a.id.localeCompare(b.id);
    });
    return node.y + node.height / 2 + (list.findIndex(edge => edge.id === connection.id) - (list.length - 1) / 2) * 26;
  };
  let returnLane = 0;
  const laneCounts = new Map<number, number>();
  const lane = (level: number) => {
    const index = laneCounts.get(level) ?? 0;
    laneCounts.set(level, index + 1);
    const node = nodes.find(node => depth.get(node.component.id) === level)!;
    return node.x + TOPOLOGY_NODE_WIDTH + 40 + index * 22;
  };
  // Route lower destinations first so a fan-out nests without crossings.
  const routed = [...shownEdges].sort((a, b) => positions.get(b.to)!.y - positions.get(a.to)!.y || a.id.localeCompare(b.id));
  const edges: TopologyEdge[] = routed.map(connection => {
    const from = positions.get(connection.from)!, to = positions.get(connection.to)!;
    const fromLevel = depth.get(connection.from)!, toLevel = depth.get(connection.to)!;
    const startX = from.x + TOPOLOGY_NODE_WIDTH;
    const startY = ports(from, connection, true), endY = ports(to, connection, false);
    const exitX = lane(fromLevel);
    let path: string;
    if (toLevel === fromLevel + 1) {
      path = `M ${startX} ${startY} L ${exitX} ${startY} L ${exitX} ${endY} L ${to.x} ${endY}`;
    } else {
      // Exclusive overhead rails keep cycles and skipped levels outside cards.
      const railY = 32 + crossEdges.findIndex(edge => edge.id === connection.id) * 32;
      const previousLevel = levels[levels.indexOf(toLevel) - 1];
      const approachX = previousLevel === undefined ? 16 + returnLane++ * 22 : lane(previousLevel);
      path = `M ${startX} ${startY} L ${exitX} ${startY} L ${exitX} ${railY} L ${approachX} ${railY} L ${approachX} ${endY} L ${to.x} ${endY}`;
    }
    return { connection, path, x: to.x - 140, y: endY, label: topologyConnectionLabel(connection) };
  });
  return { nodes, edges, width: layerX + 40, height };
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
