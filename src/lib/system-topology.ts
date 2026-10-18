import type { SystemComponent, SystemConnection } from './system-inventory';

export const TOPOLOGY_NODE_WIDTH = 238;
export const TOPOLOGY_NODE_HEIGHT = 116;

export interface TopologyNode {
  component: SystemComponent;
  context: boolean;
  x: number;
  y: number;
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
  return connection.current ? kind : `${kind} · Last known`;
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
  const rowHeight = TOPOLOGY_NODE_HEIGHT + 46, columnWidth = TOPOLOGY_NODE_WIDTH + 100;
  const rowsFor = (count: number) => count > 6 ? Math.min(6, Math.ceil(Math.sqrt(count * 1.2))) : count;
  const largestColumn = Math.max(1, ...[...columns.values()].map(column => rowsFor(column.length)));
  const height = largestColumn * rowHeight + 48;
  const nodes: TopologyNode[] = [];
  let layerX = 40;
  for (const [, column] of [...columns.entries()].sort(([a], [b]) => a - b)) {
    const rows = rowsFor(column.length), subcolumns = Math.ceil(column.length / rows);
    column.forEach((component, index) => nodes.push({
      component,
      context: !matches.has(component.id),
      x: layerX + Math.floor(index / rows) * columnWidth,
      y: 24 + (largestColumn - rows) * rowHeight / 2 + index % rows * rowHeight,
    }));
    layerX += subcolumns * columnWidth + 40;
  }
  const positions = new Map(nodes.map(node => [node.component.id, node]));
  const pairs = new Map<string, SystemConnection[]>();
  for (const connection of shownEdges) {
    const pair = `${connection.from}\0${connection.to}`;
    pairs.set(pair, [...(pairs.get(pair) ?? []), connection]);
  }
  const edges: TopologyEdge[] = [];
  for (const pair of pairs.values()) pair.forEach((connection, index) => {
    const from = positions.get(connection.from)!;
    const to = positions.get(connection.to)!;
    const offset = (index - (pair.length - 1) / 2) * 26;
    const leftToRight = to.x > from.x;
    const sameColumn = to.x === from.x;
    const startX = from.x + (leftToRight || sameColumn ? TOPOLOGY_NODE_WIDTH : 0);
    const endX = to.x + (leftToRight ? 0 : TOPOLOGY_NODE_WIDTH);
    const startY = from.y + TOPOLOGY_NODE_HEIGHT / 2 + offset;
    const endY = to.y + TOPOLOGY_NODE_HEIGHT / 2 + offset;
    const middleX = sameColumn ? startX + 55 + Math.abs(offset) : (startX + endX) / 2;
    // Long edges use the gaps between rows and columns instead of passing
    // through intervening cards in a wrapped layer.
    const long = Math.abs(to.x - from.x) > columnWidth + 40;
    const direction = leftToRight ? 1 : -1, exitX = startX + direction * 28, approachX = endX - direction * 28;
    const railY = to.y - 23 + offset;
    edges.push({ connection,
      path: long ? `M ${startX} ${startY} L ${exitX} ${startY} L ${exitX} ${railY} L ${approachX} ${railY} L ${approachX} ${endY} L ${endX} ${endY}` : `M ${startX} ${startY} C ${middleX} ${startY}, ${middleX} ${endY}, ${endX} ${endY}`,
      x: long ? (exitX + approachX) / 2 : sameColumn ? middleX : (startX + endX) / 2,
      y: long ? railY : (startY + endY) / 2,
      label: topologyConnectionLabel(connection),
    });
  });
  return { nodes, edges, width: Math.max(TOPOLOGY_NODE_WIDTH + 80, ...nodes.map(node => node.x + TOPOLOGY_NODE_WIDTH + 110)), height };
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
