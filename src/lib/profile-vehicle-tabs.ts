import type { DeepReadonly, ProfileDocument, ProfileLocalAssignment } from './profiles';
import type { GameId } from './games';

type Document = DeepReadonly<ProfileDocument>;
const clone = <T>(value: DeepReadonly<T>): T => JSON.parse(JSON.stringify(value));
export function profileNodeIds(document: Document): string[] {
  return [...document.resources, ...document.processingNodes, ...document.virtualOutputs, ...(document.desktopOutputs ?? [])].map(node => node.id);
}
/** Base owns every node that has not been assigned to a vehicle tab. */
export function profileTabNodeIds(document: Document, tabId = ''): Set<string> {
  if (tabId) return new Set(document.vehicleTabs?.find(tab => tab.id === tabId)?.nodeIds ?? []);
  const scoped = new Set(document.vehicleTabs?.flatMap(tab => [...tab.nodeIds]) ?? []);
  return new Set(profileNodeIds(document).filter(id => !scoped.has(id)));
}
/** A view for the palette and canvas; never used as a saved document. */
export function profileTabDocument(document: Document, tabId = ''): ProfileDocument {
  return profileDocumentNodes(document,profileTabNodeIds(document,tabId),!tabId);
}
/** One possible execution: Base and the selected vehicle, without unrelated tabs. */
export function profileVehicleDocument(document:Document,tabId=''):ProfileDocument {
  if(tabId&&!document.vehicleTabs?.some(tab=>tab.id===tabId))throw Error('Choose an existing vehicle tab to check conflicts.');
  return profileDocumentNodes(document,new Set([...profileTabNodeIds(document),...(tabId?profileTabNodeIds(document,tabId):[])]),true);
}
function profileDocumentNodes(document:Document,ids:Set<string>,keepEmptyGroups:boolean):ProfileDocument{
  const next = clone<ProfileDocument>(document);
  next.resources = next.resources.filter(node => ids.has(node.id));
  next.processingNodes = next.processingNodes.filter(node => ids.has(node.id));
  next.virtualOutputs = next.virtualOutputs.filter(node => ids.has(node.id));
  if (next.desktopOutputs) next.desktopOutputs = next.desktopOutputs.filter(node => ids.has(node.id));
  next.bindings = next.bindings.filter(binding => ids.has(binding.from.nodeId) && ids.has(binding.to.nodeId));
  next.visibility = next.visibility.filter(row => ids.has(row.resourceId));
  next.overrides = next.overrides.filter(row => ids.has(row.resourceId));
  next.companionAssignments = next.companionAssignments.filter(row => ids.has(row.resourceId));
  next.editor.positions = next.editor.positions.filter(row => ids.has(row.nodeId));
  if (next.editor.groups) next.editor.groups = next.editor.groups.filter(group => group.members.some(id => ids.has(id)) || keepEmptyGroups && !group.members.length).map(group => ({ ...group, members: group.members.filter(id => ids.has(id)) }));
  if (next.editor.collapsedPortGroups) next.editor.collapsedPortGroups = next.editor.collapsedPortGroups.filter(row => ids.has(row.nodeId));
  delete next.vehicleTabs;
  return next;
}
/** New components acquire the current tab in the same undoable graph edit. */
export function profileStampVehicleNodes(before: Document, after: ProfileDocument, tabId = ''): ProfileDocument {
  const previous = new Set(profileNodeIds(before)), current = new Set(profileNodeIds(after));
  for (const tab of after.vehicleTabs ?? []) {
    tab.nodeIds = tab.nodeIds.filter(id => current.has(id));
    if (tab.id === tabId) tab.nodeIds = [...new Set([...tab.nodeIds, ...[...current].filter(id => !previous.has(id))])];
  }
  return after;
}
export function profileAddVehicleTab(document: Document, gameId: GameId, vehicle: { id: string; name: string }): { document: ProfileDocument; tabId: string } {
  const next = clone<ProfileDocument>(document);
  if (next.eligibility.gameIds[0] !== gameId) throw Error('Select this game context before adding a vehicle tab.');
  if (!vehicle.id.trim() || !vehicle.name.trim()) throw Error('Choose a vehicle for this tab.');
  if (next.vehicleTabs?.some(tab => tab.gameId === gameId && tab.vehicleId === vehicle.id)) throw Error('This vehicle already has a tab.');
  if ((next.vehicleTabs?.length ?? 0) >= 32) throw Error('A profile supports at most 32 vehicle tabs.');
  let number = 1; while (next.vehicleTabs?.some(tab => tab.id === `vehicle-${number}`)) number++;
  const tabId = `vehicle-${number}`;
  next.vehicleTabs = [...(next.vehicleTabs ?? []), { id: tabId, gameId, vehicleId: vehicle.id, vehicleName: vehicle.name, nodeIds: [] }];
  return { document: next, tabId };
}
export function profileChangeVehicleTab(document: Document, tabId: string, vehicle: { id: string; name: string }): ProfileDocument {
  const next = clone<ProfileDocument>(document), tab = next.vehicleTabs?.find(tab => tab.id === tabId);
  if (!tab) throw Error('Choose an existing vehicle tab.');
  if (next.vehicleTabs?.some(other => other.id !== tabId && other.gameId === tab.gameId && other.vehicleId === vehicle.id)) throw Error('This vehicle already has a tab.');
  if (!vehicle.id.trim() || !vehicle.name.trim()) throw Error('Choose a vehicle for this tab.');
  tab.vehicleId = vehicle.id; tab.vehicleName = vehicle.name;
  return next;
}
/** Remove the complete layer, routes, local assignments and editor references atomically. */
export function profileRemoveVehicleTab(document: Document, assignments: readonly DeepReadonly<ProfileLocalAssignment>[], tabId: string): { document: ProfileDocument; assignments: ProfileLocalAssignment[] } {
  if (!document.vehicleTabs?.some(tab => tab.id === tabId)) throw Error('Choose an existing vehicle tab.');
  const next = clone<ProfileDocument>(document), removed = profileTabNodeIds(document, tabId), keep = new Set(profileNodeIds(document).filter(id => !removed.has(id)));
  next.vehicleTabs = next.vehicleTabs?.filter(tab => tab.id !== tabId);
  if (!next.vehicleTabs?.length) delete next.vehicleTabs;
  next.resources = next.resources.filter(node => keep.has(node.id));
  next.processingNodes = next.processingNodes.filter(node => keep.has(node.id));
  next.virtualOutputs = next.virtualOutputs.filter(node => keep.has(node.id));
  if (next.desktopOutputs) next.desktopOutputs = next.desktopOutputs.filter(node => keep.has(node.id));
  next.bindings = next.bindings.filter(binding => keep.has(binding.from.nodeId) && keep.has(binding.to.nodeId));
  next.visibility = next.visibility.filter(row => keep.has(row.resourceId));
  next.overrides = next.overrides.filter(row => keep.has(row.resourceId));
  next.companionAssignments = next.companionAssignments.filter(row => keep.has(row.resourceId));
  next.editor.positions = next.editor.positions.filter(row => keep.has(row.nodeId));
  if (next.editor.groups) next.editor.groups = next.editor.groups.filter(group => !group.members.length || group.members.some(id => keep.has(id))).map(group => ({ ...group, members: group.members.filter(id => keep.has(id)) }));
  if (next.editor.collapsedPortGroups) next.editor.collapsedPortGroups = next.editor.collapsedPortGroups.filter(row => keep.has(row.nodeId));
  return { document: next, assignments: clone<ProfileLocalAssignment[]>(assignments.filter(row => keep.has(row.resourceId))) };
}
