import type { diagramRoutes } from './profile-diagram';

export type DiagramWireRoute = ReturnType<typeof diagramRoutes>[number];
export type DiagramWireBundle<T extends DiagramWireRoute = DiagramWireRoute> = {
  id: string;
  route: T;
  members: readonly T[];
};

function endpointKey(route: DiagramWireRoute): string | null {
  const first = route.points[0], last = route.points.at(-1);
  if (route.points.length < 2 || !first || !last || ![first.x, first.y, last.x, last.y].every(Number.isFinite)) return null;
  // Logical signals share footer anchors. Named transform ports have distinct
  // anchors and must remain separate, even when they belong to the same block.
  return JSON.stringify([route.binding.from.nodeId, route.binding.to.nodeId, first.x, first.y, last.x, last.y]);
}

function compareIds(a: { binding: { id: string } }, b: { binding: { id: string } }): number {
  return a.binding.id < b.binding.id ? -1 : a.binding.id > b.binding.id ? 1 : 0;
}

/** Collapse only the rendered wires; every original signal remains available.
 * A selected signal is drawn separately, last, with its own editable geometry.
 */
export function bundleDiagramRoutes<T extends DiagramWireRoute>(
  routes: readonly T[],
  selectedBindingId: string | null = null,
): DiagramWireBundle<T>[] {
  const singleton = (route: T): DiagramWireBundle<T> => ({ id: route.binding.id, route, members: [route] });

  const groups = new Map<string, T[]>(), ungrouped: T[] = [], selected: T[] = [];
  for (const route of routes) {
    if (route.binding.id === selectedBindingId) { selected.push(route); continue; }
    const key = endpointKey(route);
    if (key === null) { ungrouped.push(route); continue; }
    const group = groups.get(key);
    if (group) group.push(route);
    else groups.set(key, [route]);
  }
  const bundles = ungrouped.map(singleton);
  for (const [key, group] of groups) {
    group.sort(compareIds);
    bundles.push(group.length === 1 ? singleton(group[0]) : { id: `bundle:${key}`, route: group[0], members: group });
  }
  bundles.sort((a, b) => compareIds(a.route, b.route));
  return [...bundles, ...selected.sort(compareIds).map(singleton)];
}
