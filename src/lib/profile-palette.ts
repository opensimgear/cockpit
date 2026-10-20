import {canvasMoveIntoGroup, canvasNodes, type CanvasPoint} from './profile-canvas';
import type {DeepReadonly, ProfileDocument, ProfileLocalAssignment} from './profiles';
import type {ResourceSummary} from './profile-inventory';

export const PROFILE_PALETTE_MIME='application/x-opensimgear-profile-component';
export type ProfilePaletteComponent={kind:'resource';sourceId:string}|{kind:'virtual'}|{kind:'desktop'};
export type ProfilePaletteDrag={component:ProfilePaletteComponent;name:string;category:string};

/** Recheck the current graph and inventory when a drag ends, not just when it starts. */
export function profilePaletteCanAdd(component:ProfilePaletteComponent,document:DeepReadonly<ProfileDocument>,assignments:readonly DeepReadonly<ProfileLocalAssignment>[],inventory:readonly DeepReadonly<ResourceSummary>[]):boolean{
  if(component.kind==='virtual')return document.virtualOutputs.length<8;
  if(component.kind==='desktop')return !document.desktopOutputs?.length;
  const source=inventory.find(resource=>resource.id===component.sourceId);
  if(!source||source.resourceType==='game-integration'||document.resources.length>=64||source.resourceType==='companion'&&source.attributes.componentKind!=='paired-companion')return false;
  return !assignments.some(assignment=>assignment.sourceId===source.id||!!source.attributes.companionId&&inventory.find(resource=>resource.id===assignment.sourceId)?.attributes.companionId===source.attributes.companionId);
}

/** Preserve every existing block while adding the new block and its drop position in one edit. */
export function profilePlaceComponent(before:DeepReadonly<ProfileDocument>,after:ProfileDocument,nodeId:string,position?:CanvasPoint):ProfileDocument{
  if(!position)return after;
  if(![position.x,position.y].every(value=>Number.isFinite(value)&&Math.abs(value)<=1e6))throw new Error('Drop a component at bounded canvas coordinates.');
  if(![...after.resources,...after.processingNodes,...after.virtualOutputs,...(after.desktopOutputs??[])].some(node=>node.id===nodeId))throw new Error('Place an existing profile component.');
  after.editor.positions=[...canvasNodes(before,[],[]).filter(node=>node.id!==nodeId).map(node=>({nodeId:node.id,...node.position})),{nodeId,...position}];
  return canvasMoveIntoGroup(after,nodeId,position);
}
