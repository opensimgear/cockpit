import {canvasMoveIntoGroup, canvasNodes, type CanvasPoint} from './profile-canvas';
import type {DeepReadonly, ProfileDocument, ProfileLocalAssignment} from './profiles';
import type {ResourceSummary} from './profile-inventory';
import {profileTabDocument,profileTabNodeIds} from './profile-vehicle-tabs';
import {desktopDevices, desktopBlock, type DesktopBlockKind} from './profile-desktop-blocks';
import type {ProfileCompanionPanel} from './profile-companion-authoring';

export const PROFILE_PALETTE_MIME='application/x-opensimgear-profile-component';
export type ProfilePaletteComponent={kind:'resource';sourceId:string}|{kind:'integration';gameId:string;vehicleId:string;vehicleName:string}|{kind:'virtual'}|{kind:'debug-controller'}|{kind:'desktop';device?:DesktopBlockKind}|{kind:'companion-panel';sourceId:string;publicationHash:string};
export type ProfilePaletteDrag={component:ProfilePaletteComponent;name:string;category:string};

/** Only accept the bounded component identities carried by our own palette. */
export function profilePaletteComponentValid(value:unknown):value is ProfilePaletteComponent{
  if(!value||typeof value!=='object'||Array.isArray(value))return false;
  const item=value as Record<string,unknown>,keys=Object.keys(item);
  if(item.kind==='integration'){
    const bounded=(value:unknown,max:number,empty=false)=>typeof value==='string'&&(empty||!!value.trim())&&value.trim()===value&&new TextEncoder().encode(value).length<=max&&!/[\u0000-\u001f\u007f-\u009f]/.test(value);
    return keys.length===4&&typeof item.gameId==='string'&&/^[A-Za-z][A-Za-z0-9._-]{0,79}$/.test(item.gameId)&&bounded(item.vehicleId,256,true)&&bounded(item.vehicleName,320);
  }
  if(item.kind==='virtual'||item.kind==='debug-controller')return keys.length===1;
  if(item.kind==='desktop')return keys.length===1||keys.length===2&&keys.includes('device')&&(item.device==='keyboard'||item.device==='mouse');
  if(item.kind!=='resource'&&item.kind!=='companion-panel'||typeof item.sourceId!=='string'||!item.sourceId.length||new TextEncoder().encode(item.sourceId).length>256||/[\u0000-\u001f\u007f-\u009f]/.test(item.sourceId))return false;
  return item.kind==='resource'?keys.length===2:keys.length===3&&typeof item.publicationHash==='string'&&/^sha256:[0-9a-f]{64}$/.test(item.publicationHash);
}

export function profilePalettePanelReason(panel:DeepReadonly<ProfileCompanionPanel>,source:DeepReadonly<ResourceSummary>):string{
  if(source.resourceType!=='companion'||source.attributes.componentKind!=='paired-companion')return 'Choose a paired companion.';
  if(!panel.compatibility)return '';
  const match=panel.compatibility.find(item=>item.companionId===source.attributes.companionId);
  return match?.compatible?'':match?.reason??'This panel is not compatible with this companion.';
}

/** Recheck the current graph and inventory when a drag ends, not just when it starts. */
export function profilePaletteCanAdd(component:ProfilePaletteComponent,document:DeepReadonly<ProfileDocument>,assignments:readonly DeepReadonly<ProfileLocalAssignment>[],inventory:readonly DeepReadonly<ResourceSummary>[],activeVehicleTabId=''):boolean{
  if(!profilePaletteComponentValid(component))return false;
  if(component.kind==='integration'){
    if(document.resources.length>=64||document.eligibility.gameIds.length!==1||document.eligibility.gameIds[0]!==component.gameId)return false;
    if(component.vehicleId){
      const scope=document.vehicleTabs?.find(tab=>tab.gameId===component.gameId&&tab.vehicleId===component.vehicleId);
      return scope?!document.resources.some(resource=>resource.kind==='game'&&scope.nodeIds.includes(resource.id)):(document.vehicleTabs?.length??0)<32;
    }
    const vehicleNodes=new Set(document.vehicleTabs?.flatMap(tab=>[...tab.nodeIds])??[]);
    return !document.resources.some(resource=>resource.kind==='game'&&!vehicleNodes.has(resource.id));
  }
  if(component.kind==='debug-controller')return document.resources.length<64;
  const scoped=profileTabDocument(document,activeVehicleTabId),base=profileTabDocument(document);
  if(component.kind==='virtual')return document.virtualOutputs.length<16&&(activeVehicleTabId?base.virtualOutputs.length+scoped.virtualOutputs.length<8:base.virtualOutputs.length<8&&(document.vehicleTabs??[]).every(tab=>base.virtualOutputs.length+profileTabDocument(document,tab.id).virtualOutputs.length<8));
  if(component.kind==='desktop'){
    const kind=component.device??'keyboard',conflicts=(outputs:DeepReadonly<ProfileDocument['desktopOutputs']>)=>outputs?.some(output=>desktopDevices(output).includes(kind));
    const budget=document.processingNodes.length+(document.desktopOutputs??[]).reduce((count,node)=>count+node.actions.length,0)+desktopBlock(kind,'preview').actions.length;
    return budget<=256&&(activeVehicleTabId?!conflicts(base.desktopOutputs)&&!conflicts(scoped.desktopOutputs):!conflicts(document.desktopOutputs));
  }
  const source=inventory.find(resource=>resource.id===component.sourceId);
  if(!source||source.resourceType==='game-integration'||document.resources.length>=64||source.resourceType==='companion'&&source.attributes.componentKind!=='paired-companion')return false;
  if(component.kind==='companion-panel'&&(source.resourceType!=='companion'||document.companionAssignments.length>=8||document.companionAssignments.some(panel=>assignments.some(assignment=>assignment.resourceId===panel.resourceId&&(assignment.sourceId===source.id||!!source.attributes.companionId&&inventory.find(other=>other.id===assignment.sourceId)?.attributes.companionId===source.attributes.companionId)))))return false;
  return !assignments.some(assignment=>assignment.sourceId===source.id||!!source.attributes.companionId&&inventory.find(resource=>resource.id===assignment.sourceId)?.attributes.companionId===source.attributes.companionId);
}

/** Preserve every existing block while adding the new block and its drop position in one edit. */
export function profilePlaceComponent(before:DeepReadonly<ProfileDocument>,after:ProfileDocument,nodeId:string,position?:CanvasPoint,activeVehicleTabId=''):ProfileDocument{
  if(!position)return after;
  if(![position.x,position.y].every(value=>Number.isFinite(value)&&Math.abs(value)<=1e6))throw new Error('Drop a component at bounded canvas coordinates.');
  if(![...after.resources,...after.processingNodes,...after.virtualOutputs,...(after.desktopOutputs??[])].some(node=>node.id===nodeId))throw new Error('Place an existing profile component.');
  after.editor.positions=[...canvasNodes(before,[],[]).filter(node=>node.id!==nodeId).map(node=>({nodeId:node.id,...node.position})),{nodeId,...position}];
  if(!after.vehicleTabs?.length||!activeVehicleTabId)return canvasMoveIntoGroup(after,nodeId,position);
  const scoped=profileTabNodeIds(after,activeVehicleTabId),groups=after.editor.groups??[];
  const included=new Set(groups.filter(group=>group.members.some(id=>scoped.has(id))||!activeVehicleTabId&&!group.members.length).map(group=>group.id));
  const working=JSON.parse(JSON.stringify(after)) as ProfileDocument;
  working.editor.groups=groups.filter(group=>included.has(group.id)).map(group=>({...group,members:group.members.filter(id=>scoped.has(id))}));
  const next=canvasMoveIntoGroup(working,nodeId,position);
  next.editor.groups=[...groups.filter(group=>!included.has(group.id)).map(group=>JSON.parse(JSON.stringify(group))),...(next.editor.groups??[]).map(group=>({...group,members:[...group.members,...(groups.find(old=>old.id===group.id)?.members.filter(id=>!scoped.has(id))??[])]}))];
  if(!next.editor.groups.length)delete next.editor.groups;return next;
}
