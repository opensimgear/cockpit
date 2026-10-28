import type { DeepReadonly, ProfileDocument } from './profiles';

export type ProfileIntegrationScopeDocument = Pick<DeepReadonly<ProfileDocument>, 'bindings'> &
  Partial<Pick<DeepReadonly<ProfileDocument>, 'vehicleTabs' | 'processingNodes'>>;

/** Base runs with every vehicle; distinct vehicle integrations never run together. */
export function profileVehicleScopesOverlap(left: string, right: string): boolean {
  return !left || !right || left === right;
}

/** Infer a route's execution scope from its endpoints and processing inputs.
 * Shared hardware does not acquire the scope of its other connected routes. */
export function profileRouteVehicleScope(document: ProfileIntegrationScopeDocument, fromNodeId: string, toNodeId: string): string {
  const scopes = new Set<string>(), visited = new Set<string>();
  const processing = new Set(document.processingNodes?.map(node => node.id) ?? []);
  const visit = (nodeId: string): void => {
    if (visited.has(nodeId)) return;
    visited.add(nodeId);
    for (const tab of document.vehicleTabs ?? []) if (tab.nodeIds.includes(nodeId)) scopes.add(tab.id);
    if (processing.has(nodeId)) {
      for (const binding of document.bindings) if (binding.to.nodeId === nodeId) visit(binding.from.nodeId);
    }
  };
  visit(fromNodeId); visit(toNodeId);
  if (scopes.size > 1) throw Error('Connect blocks within one vehicle integration or Common. Different vehicles are never active together.');
  return scopes.values().next().value ?? '';
}
