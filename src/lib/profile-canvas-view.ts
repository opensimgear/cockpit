import type {DeepReadonly,ProfileDocument} from './profiles';
import {profileTabDocument,profileTabNodeIds,profileVehicleDocument} from './profile-vehicle-tabs';
import {signalOverview} from './profile-signal-overview';
import {PROFILE_BLOCK_HEIGHT,PROFILE_BLOCK_WIDTH} from './profile-editor';

export type ProfileCanvasView={document:ProfileDocument;referenceNodeIds:Set<string>;referenceProcessingIds:Set<string>};

/** A vehicle owns its canvas edits while Base endpoints provide complete route context. */
export function profileCanvasView(document:DeepReadonly<ProfileDocument>,tabId=''):ProfileCanvasView {
  const referenceNodeIds=new Set<string>(),referenceProcessingIds=new Set<string>();
  if(!tabId)return {document:profileTabDocument(document),referenceNodeIds,referenceProcessingIds};
  const local=profileTabNodeIds(document,tabId),next=profileVehicleDocument(document,tabId),keep=new Set(local),bindings=new Set<string>();
  for(const binding of next.bindings)if(local.has(binding.from.nodeId)&&local.has(binding.to.nodeId))bindings.add(binding.id);
  for(const bundle of signalOverview(next))for(const lane of bundle.lanes){
    if(!local.has(lane.destination.nodeId)&&!lane.sources.some(source=>local.has(source.nodeId))&&!lane.transforms.some(id=>local.has(id)))continue;
    keep.add(lane.destination.nodeId);lane.sources.forEach(source=>keep.add(source.nodeId));lane.transforms.forEach(id=>keep.add(id));lane.bindingIds.forEach(id=>bindings.add(id));
  }
  const endpoints=[...next.resources,...next.virtualOutputs,...(next.desktopOutputs??[])];
  for(const node of endpoints)if(keep.has(node.id)&&!local.has(node.id))referenceNodeIds.add(node.id);
  for(const node of next.processingNodes)if(keep.has(node.id)&&!local.has(node.id))referenceProcessingIds.add(node.id);
  next.resources=next.resources.filter(node=>keep.has(node.id));
  next.processingNodes=next.processingNodes.filter(node=>keep.has(node.id));
  next.virtualOutputs=next.virtualOutputs.filter(node=>keep.has(node.id));
  if(next.desktopOutputs)next.desktopOutputs=next.desktopOutputs.filter(node=>keep.has(node.id));
  next.bindings=next.bindings.filter(binding=>bindings.has(binding.id));
  next.visibility=next.visibility.filter(row=>keep.has(row.resourceId));
  next.overrides=next.overrides.filter(row=>keep.has(row.resourceId));
  next.companionAssignments=next.companionAssignments.filter(row=>keep.has(row.resourceId));
  const positions=next.editor.positions.filter(position=>local.has(position.nodeId));
  const left=Math.min(48,...positions.map(position=>position.x))-PROFILE_BLOCK_WIDTH-96;
  const top=Math.min(48,...positions.map(position=>position.y));
  next.editor.positions=next.editor.positions.filter(position=>keep.has(position.nodeId)&&!referenceNodeIds.has(position.nodeId));
  [...referenceNodeIds].sort().forEach((nodeId,index)=>next.editor.positions.push({nodeId,x:left,y:top+index*(PROFILE_BLOCK_HEIGHT+48)}));
  if(next.editor.groups)next.editor.groups=next.editor.groups.filter(group=>group.members.some(id=>local.has(id))).map(group=>({...group,members:group.members.filter(id=>local.has(id))}));
  if(next.editor.collapsedPortGroups)next.editor.collapsedPortGroups=next.editor.collapsedPortGroups.filter(row=>keep.has(row.nodeId));
  if(next.editor.wireRoutes)next.editor.wireRoutes=next.editor.wireRoutes.filter(route=>next.bindings.some(binding=>binding.id===route.bindingId&&local.has(binding.from.nodeId)&&local.has(binding.to.nodeId)));
  return {document:next,referenceNodeIds,referenceProcessingIds};
}
