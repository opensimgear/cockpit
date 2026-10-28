import { graphConnectionError, graphDraft, type ProfileGraphDraft } from './profile-graph';
import { debugControllerError } from './profile-debug-controller';
import { profileStampVehicleNodes } from './profile-vehicle-tabs';
import { profileDocumentError, type DeepReadonly, type ProfileResource } from './profiles';

/** Layout and route retirement form one undoable edit, with no local assignment. */
export function graphSetDebugController(value: DeepReadonly<ProfileGraphDraft>, resource: ProfileResource, activeVehicleTabId = ''): ProfileGraphDraft {
  const error = debugControllerError(resource);
  if (error) throw Error(error);
  const next = graphDraft(value);
  const previous = next.document.resources.find(node => node.id === resource.id);
  if (previous && previous.kind !== 'debug-controller' || [...next.document.processingNodes, ...next.document.virtualOutputs, ...(next.document.desktopOutputs ?? [])].some(node => node.id === resource.id)) throw Error('Use a unique debug controller identity.');
  if (!previous && next.document.resources.length >= 64) throw Error('A profile supports at most 64 resource blocks.');
  if (next.assignments.some(assignment => assignment.resourceId === resource.id)) throw Error('Debug controllers do not use local hardware assignments.');
  const ports = new Set(resource.ports.map(port => port.id));
  next.document.bindings = next.document.bindings.filter(binding =>
    (binding.from.nodeId !== resource.id || ports.has(binding.from.portId)) &&
    (binding.to.nodeId !== resource.id || ports.has(binding.to.portId)));
  next.document.resources = next.document.resources.filter(node => node.id !== resource.id);
  next.document.resources.push(structuredClone(resource));
  if (next.document.editor.wireRoutes) {
    const routes = new Set(next.document.bindings.map(binding => binding.id));
    next.document.editor.wireRoutes = next.document.editor.wireRoutes.filter(route => routes.has(route.bindingId));
  }
  profileStampVehicleNodes(value.document, next.document, activeVehicleTabId);
  const invalid = profileDocumentError(next.document);
  if (invalid) throw Error(invalid);
  for (const binding of next.document.bindings) {
    const error = graphConnectionError(next.document, binding.from, binding.to, binding.id);
    if (error) throw Error(`${binding.id}: ${error}`);
  }
  return next;
}
