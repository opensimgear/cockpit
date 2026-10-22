import type { ELK, ElkExtendedEdge, ElkNode, ElkPoint, ElkPort } from 'elkjs/lib/elk-api';
import type { DiagramNode, DiagramPoint } from './profile-diagram';
import type { DeepReadonly, ProfileBinding } from './profiles';

type Binding = DeepReadonly<Pick<ProfileBinding, 'id' | 'from' | 'to'>>;
export type ProfileElkLayout = {
  positions: { nodeId: string; x: number; y: number }[];
  wireRoutes: { bindingId: string; points: DiagramPoint[]; automatic: true }[];
};
type NodeGeometry = { node: DiagramNode; width: number; height: number; ports: ElkPort[] };
type Attachment = { nodeId: string; portId: string; offset: DiagramPoint };
type PreparedGraph = { graph: ElkNode; nodes: Map<string, NodeGeometry>; edges: Map<string, { from: Attachment; to: Attachment }> };

const EPSILON = 1e-6;
const bounded = (value: unknown): value is number => typeof value === 'number' && Number.isFinite(value) && Math.abs(value) <= 1_000_000;
const same = (a: DiagramPoint, b: DiagramPoint) => Math.abs(a.x - b.x) <= EPSILON && Math.abs(a.y - b.y) <= EPSILON;
function invalid(reason: string): never { throw new Error(`ELK layout could not be applied: ${reason}`); }
function point(value: ElkPoint | undefined, label: string): DiagramPoint {
  if (!value || !bounded(value.x) || !bounded(value.y)) invalid(`${label} has invalid coordinates.`);
  return { x: value.x, y: value.y };
}

/** Physical attachment points are shared by signals that use the same card
 * footer, while named transform ports retain their own fixed border offsets. */
export function profileElkGraph(nodes: readonly DiagramNode[], bindings: readonly Binding[], width: number, height: number): PreparedGraph {
  if (!bounded(width) || !bounded(height) || width <= 0 || height <= 0) invalid('block dimensions must be positive.');
  const knownNodes = new Map<string, NodeGeometry>();
  for (const node of nodes) {
    if (!node.id || knownNodes.has(node.id)) invalid('block identifiers must be unique.');
    point(node.position, `Block ${node.id}`);
    const nodeWidth = node.width ?? width, nodeHeight = node.height ?? height;
    if (!bounded(nodeWidth) || !bounded(nodeHeight) || nodeWidth <= 0 || nodeHeight <= 0) invalid(`Block ${node.id} has invalid dimensions.`);
    knownNodes.set(node.id, { node, width: nodeWidth, height: nodeHeight, ports: [] });
  }
  const reservedIds = new Set([...knownNodes.keys(), ...bindings.map(binding => binding.id)]);
  const internalId = (base: string) => {
    let id = base;
    while (reservedIds.has(id)) id += '_';
    reservedIds.add(id);
    return id;
  };
  const attachments = new Map<string, Attachment>();
  function attachment(endpoint: Binding['from'], direction: 'input' | 'output'): Attachment {
    const geometry = knownNodes.get(endpoint.nodeId);
    if (!geometry) invalid(`Connection references missing block ${endpoint.nodeId}.`);
    const { node, width: nodeWidth, height: nodeHeight } = geometry;
    const named = (direction === 'input' ? node.inputAnchors : node.outputAnchors)?.[endpoint.portId];
    const absolute = point(named ?? { x: node.position.x + (direction === 'output' ? nodeWidth : 0), y: node.position.y + nodeHeight - 20 }, `Port ${endpoint.nodeId}/${endpoint.portId}`);
    const offset = { x: absolute.x - node.position.x, y: absolute.y - node.position.y };
    if (Math.abs(offset.x - (direction === 'output' ? nodeWidth : 0)) > EPSILON || offset.y < -EPSILON || offset.y > nodeHeight + EPSILON) invalid(`Port ${endpoint.nodeId}/${endpoint.portId} must be on the ${direction} border.`);
    offset.x = direction === 'output' ? nodeWidth : 0;
    const key = JSON.stringify([node.id, direction, offset.x, offset.y]);
    const existing = attachments.get(key);
    if (existing) return existing;
    // Generated IDs cannot collide with user block IDs or punctuation in a
    // signal name. Only these IDs are handed to the engine as endpoints.
    const portId = internalId(`__cockpit_elk_port_${attachments.size}`);
    const result = { nodeId: node.id, portId, offset };
    geometry.ports.push({ id: portId, width: 0, height: 0, ...offset, layoutOptions: { 'elk.port.side': direction === 'output' ? 'EAST' : 'WEST' } });
    attachments.set(key, result);
    return result;
  }
  const knownEdges = new Map<string, { from: Attachment; to: Attachment }>();
  const edges: ElkExtendedEdge[] = [];
  for (const binding of bindings) {
    if (!binding.id || knownEdges.has(binding.id)) invalid('connection identifiers must be unique.');
    const from = attachment(binding.from, 'output'), to = attachment(binding.to, 'input');
    knownEdges.set(binding.id, { from, to });
    edges.push({ id: binding.id, sources: [from.portId], targets: [to.portId] });
  }
  return {
    nodes: knownNodes, edges: knownEdges,
    graph: {
      id: internalId('__cockpit_elk_root'),
      layoutOptions: {
        'elk.algorithm': 'layered', 'elk.direction': 'RIGHT', 'elk.edgeRouting': 'ORTHOGONAL',
        'elk.padding': '[top=48,left=48,bottom=48,right=48]',
        'elk.spacing.nodeNode': '64', 'elk.spacing.edgeNode': '24', 'elk.spacing.edgeEdge': '12',
        'elk.layered.spacing.nodeNodeBetweenLayers': '176',
        'elk.layered.spacing.edgeNodeBetweenLayers': '32', 'elk.layered.spacing.edgeEdgeBetweenLayers': '12',
        'elk.layered.mergeEdges': 'false', 'elk.layered.unnecessaryBendpoints': 'false',
        'elk.layered.thoroughness': '12', 'elk.randomSeed': '1',
        'elk.separateConnectedComponents': 'true', 'elk.spacing.componentComponent': '96'
      },
      children: [...knownNodes.values()].map(({ node, width: nodeWidth, height: nodeHeight, ports }) => ({
        id: node.id, width: nodeWidth, height: nodeHeight, ports: ports.map(port => ({ ...port, layoutOptions: { ...port.layoutOptions } })),
        layoutOptions: { 'elk.portConstraints': 'FIXED_POS' }
      })),
      edges
    }
  };
}

/** ELK can differ by machine epsilon when adding relative port coordinates.
 * Canonicalize each straight run to its exact physical endpoint when present. */
function orthogonalPoints(raw: readonly DiagramPoint[], start: DiagramPoint, end: DiagramPoint, id: string): DiagramPoint[] {
  const points = raw.map(value => ({ ...value }));
  if (points.length < 2 || points.length > 128 || !same(points[0], start) || !same(points.at(-1)!, end)) invalid(`Connection ${id} does not meet its physical ports.`);
  points[0] = { ...start }; points[points.length - 1] = { ...end };
  for (const axis of ['x', 'y'] as const) {
    let begin = 0;
    while (begin < points.length) {
      let finish = begin;
      while (finish + 1 < points.length && Math.abs(points[finish + 1][axis] - points[finish][axis]) <= EPSILON) finish++;
      const coordinate = finish === points.length - 1 ? points[finish][axis] : points[begin][axis];
      for (let index = begin; index <= finish; index++) points[index][axis] = coordinate;
      begin = finish + 1;
    }
  }
  if (!same(points[0], start) || !same(points.at(-1)!, end)) invalid(`Connection ${id} has inconsistent endpoints.`);
  const corners: DiagramPoint[] = [];
  for (const value of points) {
    if (corners.length && same(corners.at(-1)!, value)) continue;
    corners.push(value);
    while (corners.length >= 3) {
      const [a, b, c] = corners.slice(-3);
      if ((a.x === b.x && b.x === c.x && (b.y - a.y) * (c.y - b.y) > 0) || (a.y === b.y && b.y === c.y && (b.x - a.x) * (c.x - b.x) > 0)) corners.splice(corners.length - 2, 1);
      else break;
    }
  }
  if (corners.length < 2 || corners.some((value, index) => index > 0 && value.x !== corners[index - 1].x && value.y !== corners[index - 1].y)) invalid(`Connection ${id} is not orthogonal.`);
  const first = corners[0], second = corners[1], last = corners.at(-1)!, previous = corners.at(-2)!;
  if (first.y !== second.y || second.x <= first.x || previous.y !== last.y || previous.x >= last.x) invalid(`Connection ${id} does not leave and enter its ports horizontally.`);
  return corners;
}

/** Validate a complete engine response before any profile/history mutation. */
export function profileElkResult(prepared: PreparedGraph, result: ElkNode): ProfileElkLayout {
  if (!result || !Array.isArray(result.children) || result.children.length !== prepared.nodes.size || !Array.isArray(result.edges) || result.edges.length !== prepared.edges.size) invalid('the engine returned an incomplete graph.');
  const placed = new Map<string, { x: number; y: number; width: number; height: number }>();
  for (const child of result.children) {
    const expected = prepared.nodes.get(child.id);
    if (!expected || placed.has(child.id)) invalid('the engine returned unknown or repeated blocks.');
    const position = point(child as ElkPoint, `Block ${child.id}`);
    if (child.width !== expected.width || child.height !== expected.height) invalid(`Block ${child.id} changed size.`);
    if (!Array.isArray(child.ports) || child.ports.length !== expected.ports.length) invalid(`Block ${child.id} lost its physical ports.`);
    const ports = new Map(child.ports.map(port => [port.id, port]));
    if (ports.size !== expected.ports.length || expected.ports.some(port => !ports.has(port.id) || !same(point(ports.get(port.id) as ElkPoint, `Port ${port.id}`), port as ElkPoint))) invalid(`Block ${child.id} moved its physical ports.`);
    placed.set(child.id, { ...position, width: expected.width, height: expected.height });
  }
  const boxes = [...placed.entries()];
  for (let index = 0; index < boxes.length; index++) for (let other = 0; other < index; other++) {
    const [id, a] = boxes[index], [otherId, b] = boxes[other];
    if (Math.min(a.x + a.width, b.x + b.width) - Math.max(a.x, b.x) > EPSILON && Math.min(a.y + a.height, b.y + b.height) - Math.max(a.y, b.y) > EPSILON) invalid(`Blocks ${id} and ${otherId} overlap.`);
  }
  const routes = new Map<string, ProfileElkLayout['wireRoutes'][number]>();
  for (const edge of result.edges) {
    const expected = prepared.edges.get(edge.id);
    if (!expected || routes.has(edge.id)) invalid('the engine returned unknown or repeated connections.');
    if (edge.sources?.length !== 1 || edge.sources[0] !== expected.from.portId || edge.targets?.length !== 1 || edge.targets[0] !== expected.to.portId || edge.sections?.length !== 1) invalid(`Connection ${edge.id} has unsupported or missing sections.`);
    const section = edge.sections[0], from = placed.get(expected.from.nodeId)!, to = placed.get(expected.to.nodeId)!;
    const start = { x: from.x + expected.from.offset.x, y: from.y + expected.from.offset.y }, end = { x: to.x + expected.to.offset.x, y: to.y + expected.to.offset.y };
    const points = orthogonalPoints([section.startPoint, ...(section.bendPoints ?? []), section.endPoint].map(value => point(value, `Connection ${edge.id}`)), start, end, edge.id);
    for (let index = 1; index < points.length; index++) for (const [id, box] of boxes) {
      const a = points[index - 1], b = points[index];
      const crosses = a.x === b.x
        ? a.x > box.x + EPSILON && a.x < box.x + box.width - EPSILON && Math.max(a.y, b.y) > box.y + EPSILON && Math.min(a.y, b.y) < box.y + box.height - EPSILON
        : a.y > box.y + EPSILON && a.y < box.y + box.height - EPSILON && Math.max(a.x, b.x) > box.x + EPSILON && Math.min(a.x, b.x) < box.x + box.width - EPSILON;
      if (crosses) invalid(`Connection ${edge.id} crosses block ${id}.`);
    }
    routes.set(edge.id, { bindingId: edge.id, points, automatic: true });
  }
  return {
    positions: [...prepared.nodes.keys()].map(nodeId => ({ nodeId, x: placed.get(nodeId)!.x, y: placed.get(nodeId)!.y })),
    wireRoutes: [...prepared.edges.keys()].map(id => routes.get(id)!)
  };
}

export type ProfileLayoutScore = (layout: ProfileElkLayout) => readonly number[];

export async function layoutElkDiagram(nodes: readonly DiagramNode[], bindings: readonly Binding[], width: number, height: number, engine: Pick<ELK, 'layout'>, score?: ProfileLayoutScore): Promise<ProfileElkLayout> {
  const prepared = profileElkGraph(nodes, bindings, width, height);
  if (!nodes.length) return { positions: [], wireRoutes: [] };
  const first = profileElkResult(prepared, await engine.layout(prepared.graph));
  if (!score || !bindings.length) return first;
  let best = first, bestScore = score(first);
  // Keep the original candidate as a floor. Deterministic alternatives vary
  // both node ordering and sink layering; short branches need not sit beside
  // the final outputs when that forces other routes across them.
  for (const strategy of ['NETWORK_SIMPLEX', 'LONGEST_PATH_SOURCE']) {
    for (const seed of [1, 7, 19, 31]) {
      if (strategy === 'NETWORK_SIMPLEX' && seed === 1) continue;
      const graph = {...prepared.graph, layoutOptions: {...prepared.graph.layoutOptions,
        'elk.layered.layering.strategy': strategy, 'elk.randomSeed': String(seed)}};
      const candidate = profileElkResult(prepared, await engine.layout(graph)), candidateScore = score(candidate);
      const difference = candidateScore.findIndex((value, index) => value !== bestScore[index]);
      if (difference >= 0 && candidateScore[difference] < bestScore[difference]) {
        best = candidate; bestScore = candidateScore;
      }
    }
  }
  return best;
}
