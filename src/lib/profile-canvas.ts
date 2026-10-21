import { profileDocumentError, type DeepReadonly, type ProfileDocument, type ProfileEditorGroup, type ProfileEditorGroupBounds, type ProfileLocalAssignment, type ProfilePortContract, type ProfilePortGroup, type ProfileScalarSchema } from './profiles';
import type { ResourceSummary } from './profile-inventory';
import { isGameId, type GameId } from './games';
import { PROFILE_BLOCK_HEIGHT, PROFILE_BLOCK_WIDTH, profileAutoLayout, profileCategory } from './profile-editor';
import type { ProfileDesktopOutput } from './profile-desktop';
export type CanvasPoint = {x:number;y:number};
export type CanvasViewport = CanvasPoint & {zoom:number};
export type CanvasNode = {id:string;name:string;kind:'resource'|'processing'|'virtual'|'desktop';category:string;position:CanvasPoint;desktop?:DeepReadonly<ProfileDesktopOutput>;resource?:DeepReadonly<ProfileDocument['resources'][number]>;processing?:DeepReadonly<ProfileDocument['processingNodes'][number]>;virtual?:DeepReadonly<ProfileDocument['virtualOutputs'][number]>;assignment?:DeepReadonly<ProfileLocalAssignment>;observation?:DeepReadonly<ResourceSummary>};
export const PROFILE_TRANSFORM_WIDTH=224,PROFILE_TRANSFORM_HEADER=78,PROFILE_TRANSFORM_PORT_ROW=28,PROFILE_TRANSFORM_BOTTOM=8;
type CanvasNodeGeometry=Partial<Pick<CanvasNode,'kind'|'processing'>>;
/** Named transform ports determine the card height; resource cards retain their footer. */
export function canvasNodeSize(node:CanvasNodeGeometry):{width:number;height:number}{
  return node.kind==='processing'?{width:PROFILE_TRANSFORM_WIDTH,height:PROFILE_TRANSFORM_HEADER+PROFILE_TRANSFORM_BOTTOM+Math.max(1,node.processing?.inputs.length??0,node.processing?.outputs.length??0)*PROFILE_TRANSFORM_PORT_ROW}:{width:PROFILE_BLOCK_WIDTH,height:PROFILE_BLOCK_HEIGHT};
}
/** Attachments sit exactly on the block border, centered on their named port row. */
export function canvasNodeAnchor(node:CanvasNodeGeometry&{position:CanvasPoint},direction:'input'|'output',portId?:string):CanvasPoint{
  const {width,height}=canvasNodeSize(node);
  const ports=direction==='input'?node.processing?.inputs:node.processing?.outputs;
  const row=Math.max(0,ports?.findIndex(port=>port.id===portId)??0);
  return{x:node.position.x+(direction==='output'?width:0),y:node.position.y+(node.kind==='processing'?PROFILE_TRANSFORM_HEADER+(row+.5)*PROFILE_TRANSFORM_PORT_ROW:height-20)};
}
/** Authored choices only: inventory or an applied run cannot change these counts. */
export function canvasControllerEffects(document:DeepReadonly<ProfileDocument>):{enabled:number;disabled:number;configured:number;hidden:number;virtual:number}{
  const devices=document.resources.filter(resource=>resource.kind==='device');
  const choices=devices.map(resource=>document.overrides.find(intent=>intent.resourceId===resource.id)?.enabled);
  return{enabled:choices.filter(value=>value===true).length,disabled:choices.filter(value=>value===false).length,configured:choices.filter(value=>value===undefined).length,hidden:document.visibility.filter(intent=>intent.mode==='hidden-while-active').length,virtual:document.virtualOutputs.length};
}
const clamp=(value:number,min:number,max:number)=>Math.min(max,Math.max(min,value));
const finite=(value:number)=>Number.isFinite(value);
export function canvasNodes(document:DeepReadonly<ProfileDocument>,assignments:readonly DeepReadonly<ProfileLocalAssignment>[],inventory:readonly DeepReadonly<ResourceSummary>[]):CanvasNode[]{
  const positions=new Map(document.editor.positions.map(position=>[position.nodeId,position]));
  const nodes:Omit<CanvasNode,'position'>[]=[...document.resources.map(resource=>{const assignment=assignments.find(value=>value.resourceId===resource.id);const observation=inventory.find(value=>value.id===assignment?.sourceId);return{id:resource.id,name:resource.name,kind:'resource' as const,category:observation?profileCategory(observation):resource.kind==='game'?'Game integration':resource.kind==='companion'?'Companion':'Physical resource',resource,assignment,observation};}),...document.processingNodes.map(processing=>({id:processing.id,name:processing.name,kind:'processing' as const,category:'Transform',processing})),...document.virtualOutputs.map(virtual=>({id:virtual.id,name:virtual.name,kind:'virtual' as const,category:'Virtual controller',virtual})),...(document.desktopOutputs??[]).map(desktop=>({id:desktop.id,name:desktop.name,kind:'desktop' as const,category:'Keyboard and mouse',desktop}))];
  const missing=profileAutoLayout(nodes.filter(node=>!positions.has(node.id)).map(node=>node.id));
  const bottom=document.editor.positions.reduce((max,p)=>Math.max(max,p.y+canvasNodeSize(nodes.find(node=>node.id===p.nodeId)??{}).height+48),0);
  return nodes.map(node=>({...node,position:positions.get(node.id)??{x:missing.find(p=>p.nodeId===node.id)!.x,y:missing.find(p=>p.nodeId===node.id)!.y+bottom}}));
}
export function canvasZoom(view:CanvasViewport,factor:number,at:CanvasPoint):CanvasViewport{
  if(![view.x,view.y,view.zoom,factor,at.x,at.y].every(finite)||view.zoom<=0||factor<=0)throw new Error('Use a finite positive canvas zoom.');
  const zoom=clamp(view.zoom*factor,0.1,4);return{x:clamp(at.x-(at.x-view.x)*zoom/view.zoom,-1e6,1e6),y:clamp(at.y-(at.y-view.y)*zoom/view.zoom,-1e6,1e6),zoom};
}
export function canvasPan(view:CanvasViewport,delta:CanvasPoint):CanvasViewport{
  if(![view.x,view.y,view.zoom,delta.x,delta.y].every(finite))throw new Error('Use finite canvas coordinates.');
  return{...view,x:clamp(view.x+delta.x,-1e6,1e6),y:clamp(view.y+delta.y,-1e6,1e6)};
}
export function canvasFit(nodes:readonly (CanvasNodeGeometry&Pick<CanvasNode,'position'>)[],width:number,height:number):CanvasViewport{
  if(![width,height].every(finite)||width<=0||height<=0)throw new Error('The canvas needs a positive viewport size.');
  if(!nodes.length)return{x:0,y:0,zoom:1};
  const left=Math.min(...nodes.map(node=>node.position.x)),top=Math.min(...nodes.map(node=>node.position.y));
  const right=Math.max(...nodes.map(node=>node.position.x+canvasNodeSize(node).width)),bottom=Math.max(...nodes.map(node=>node.position.y+canvasNodeSize(node).height));
  const zoom=clamp(Math.min((width-64)/(right-left),(height-64)/(bottom-top)),0.1,1.5);
  return{x:clamp((width-(right-left)*zoom)/2-left*zoom,-1e6,1e6),y:clamp((height-(bottom-top)*zoom)/2-top*zoom,-1e6,1e6),zoom};
}
/** Center a selected card in the region left of its inspector, with room around it. */
export function canvasFocusNode(node:CanvasNodeGeometry&{position:CanvasPoint},availableWidth:number,height:number):CanvasViewport{
  if(![availableWidth,height,node.position.x,node.position.y].every(finite)||availableWidth<=0||height<=0)throw new Error('Focus needs finite block coordinates and a positive viewport size.');
  const size=canvasNodeSize(node),zoom=clamp(Math.min(availableWidth*.5/size.width,height*.5/size.height),0.1,4);
  const x=availableWidth/2-(node.position.x+size.width/2)*zoom,y=height/2-(node.position.y+size.height/2)*zoom;
  if(![x,y].every(finite))throw new Error('Keep the focused block within finite canvas coordinates.');
  return{x,y,zoom};
}
export function canvasMove(document:DeepReadonly<ProfileDocument>,nodeId:string,position:CanvasPoint):ProfileDocument{
  if(![position.x,position.y].every(finite)||![...document.resources,...document.processingNodes,...document.virtualOutputs,...(document.desktopOutputs??[])].some(node=>node.id===nodeId))throw new Error('Move an existing block to finite coordinates.');
  const next=JSON.parse(JSON.stringify(document)) as ProfileDocument;next.editor.positions=next.editor.positions.filter(p=>p.nodeId!==nodeId);next.editor.positions.push({nodeId,x:clamp(position.x,-1e6,1e6),y:clamp(position.y,-1e6,1e6)});return next;
}
export type CanvasGroup = DeepReadonly<ProfileEditorGroup> & {x:number;y:number;width:number;height:number};
const GROUP_WIDTH=440,GROUP_HEIGHT=360,GROUP_GAP=24;
export const PROFILE_GROUP_MIN_WIDTH=328,PROFILE_GROUP_MIN_HEIGHT=300;
const boundsOf=(group:Pick<CanvasGroup,'x'|'y'|'width'|'height'>):ProfileEditorGroupBounds=>({x:group.x,y:group.y,width:group.width,height:group.height});
const overlaps=(a:ProfileEditorGroupBounds,b:ProfileEditorGroupBounds)=>a.x<b.x+b.width&&a.x+a.width>b.x&&a.y<b.y+b.height&&a.y+a.height>b.y;
/** Old profiles derive their frames without changing the saved document. */
export function canvasGroups(document:DeepReadonly<ProfileDocument>,nodes:readonly CanvasNode[]):CanvasGroup[]{
  return(document.editor.groups??[]).map((group,index)=>{if(group.bounds)return{...group,...group.bounds};const members=nodes.filter(node=>group.members.includes(node.id));if(!members.length)return{...group,x:index*(GROUP_WIDTH+GROUP_GAP),y:0,width:GROUP_WIDTH,height:GROUP_HEIGHT};const x=Math.min(...members.map(node=>node.position.x))-20,y=Math.min(...members.map(node=>node.position.y))-44;return{...group,x,y,width:Math.max(PROFILE_GROUP_MIN_WIDTH,Math.max(...members.map(node=>node.position.x+canvasNodeSize(node).width))-x+20),height:Math.max(PROFILE_GROUP_MIN_HEIGHT,Math.max(...members.map(node=>node.position.y+canvasNodeSize(node).height))-y+20)};});
}
function checkedLayout(document:ProfileDocument):ProfileDocument{const error=profileDocumentError(document);if(error)throw new Error(error);return document;}
function freeBounds(bounds:ProfileEditorGroupBounds,occupied:readonly ProfileEditorGroupBounds[]):ProfileEditorGroupBounds{
  let next={...bounds,x:clamp(bounds.x,-1e6,1e6),y:clamp(bounds.y,-1e6,1e6)};
  for(let attempt=0;attempt<2048;attempt++){
    const conflict=occupied.find(other=>overlaps(next,other));if(!conflict)return next;
    const x=conflict.x+conflict.width+GROUP_GAP;
    if(x<=1e6)next.x=x;else{next.x=-1e6;next.y=Math.max(...occupied.map(other=>other.y+other.height))+GROUP_GAP;if(next.y>1e6)break;}
  }
  throw new Error('There is no space for another group at these coordinates.');
}
/** Materialize legacy layout only as part of an authored action, retaining relative member positions. */
function groupLayout(document:DeepReadonly<ProfileDocument>):ProfileDocument{
  const next=JSON.parse(JSON.stringify(document)) as ProfileDocument;
  const nodes=canvasNodes(document,[],[]),frames=canvasGroups(document,nodes),occupied=frames.filter(group=>group.bounds).map(boundsOf);
  // Auto-placed blocks must stay still when another block acquires a saved position.
  for(const node of nodes)if(!next.editor.positions.some(row=>row.nodeId===node.id))next.editor.positions.push({nodeId:node.id,...node.position});
  for(const group of next.editor.groups??[]){
    if(group.bounds)continue;const frame=frames.find(candidate=>candidate.id===group.id)!;
    const bounds=freeBounds(boundsOf(frame),occupied);occupied.push(bounds);group.bounds=bounds;
    if(bounds.x!==frame.x||bounds.y!==frame.y)for(const member of group.members){const node=canvasNodes(next,[],[]).find(candidate=>candidate.id===member);if(node){next.editor.positions=next.editor.positions.filter(row=>row.nodeId!==member);next.editor.positions.push({nodeId:member,x:clamp(node.position.x+bounds.x-frame.x,-1e6,1e6),y:clamp(node.position.y+bounds.y-frame.y,-1e6,1e6)});}}
  }
  return next;
}
function nextGroupId(groups:readonly DeepReadonly<ProfileEditorGroup>[]):string{for(let n=1;n<=4096;n++)if(!groups.some(group=>group.id===`group-${n}`))return`group-${n}`;throw new Error('The canvas cannot add another group.');}
export function canvasCreateGroup(document:DeepReadonly<ProfileDocument>,name:string,at:CanvasPoint={x:0,y:0}):ProfileDocument{
  if(![at.x,at.y].every(finite))throw new Error('Use finite group coordinates.');
  const next=groupLayout(document),groups=next.editor.groups??[],bounds=freeBounds({x:at.x,y:at.y,width:GROUP_WIDTH,height:GROUP_HEIGHT},groups.map(group=>group.bounds!));
  next.editor.groups=[...groups,{id:nextGroupId(groups),name:name.trim(),members:[],bounds}];return checkedLayout(next);
}
export function canvasRenameGroup(document:DeepReadonly<ProfileDocument>,id:string,name:string):ProfileDocument{
  const next=JSON.parse(JSON.stringify(document)) as ProfileDocument,group=next.editor.groups?.find(group=>group.id===id);if(!group)throw new Error('Choose an existing editor group.');group.name=name.trim();return checkedLayout(next);
}
/** Hit testing is based on the card center and excludes the group's title bar. */
export function canvasGroupAt(document:DeepReadonly<ProfileDocument>,point:CanvasPoint):string|null{
  if(![point.x,point.y].every(finite))throw new Error('Use finite group coordinates.');
  return canvasGroups(document,canvasNodes(document,[],[])).find(group=>point.x>=group.x&&point.x<=group.x+group.width&&point.y>=group.y+36&&point.y<=group.y+group.height)?.id??null;
}
/** A drop is one history action: move the block and update its single group membership. */
export function canvasMoveIntoGroup(document:DeepReadonly<ProfileDocument>,nodeId:string,position:CanvasPoint):ProfileDocument{
  const next=canvasMove(groupLayout(document),nodeId,position),saved=next.editor.positions.find(row=>row.nodeId===nodeId)!,size=canvasNodeSize(canvasNodes(next,[],[]).find(node=>node.id===nodeId)!),id=canvasGroupAt(next,{x:saved.x+size.width/2,y:saved.y+size.height/2});
  for(const group of next.editor.groups??[])group.members=group.members.filter(member=>member!==nodeId);
  if(id)next.editor.groups!.find(group=>group.id===id)!.members.push(nodeId);return checkedLayout(next);
}
export function canvasSetGroupBounds(document:DeepReadonly<ProfileDocument>,id:string,bounds:ProfileEditorGroupBounds):ProfileDocument{
  const next=groupLayout(document),group=next.editor.groups?.find(group=>group.id===id);if(!group)throw new Error('Choose an existing editor group.');
  if(next.editor.groups!.some(other=>other.id!==id&&overlaps(bounds,other.bounds!)))throw new Error('Groups cannot overlap.');
  const nodes=canvasNodes(next,[],[]).filter(node=>group.members.includes(node.id));
  if(nodes.some(node=>{const size=canvasNodeSize(node),x=node.position.x+size.width/2,y=node.position.y+size.height/2;return x<bounds.x||x>bounds.x+bounds.width||y<bounds.y+36||y>bounds.y+bounds.height;}))throw new Error('Keep the group large enough for its components.');
  group.bounds={...bounds};return checkedLayout(next);
}
export function canvasResizeGroup(document:DeepReadonly<ProfileDocument>,id:string,size:{width:number;height:number}):ProfileDocument{
  const frame=canvasGroups(document,canvasNodes(document,[],[])).find(group=>group.id===id);if(!frame)throw new Error('Choose an existing editor group.');return canvasSetGroupBounds(document,id,{x:frame.x,y:frame.y,...size});
}
export function canvasMoveGroup(document:DeepReadonly<ProfileDocument>,id:string,position:CanvasPoint):ProfileDocument{
  if(![position.x,position.y].every(finite))throw new Error('Use finite group coordinates.');
  let next=groupLayout(document);const group=next.editor.groups?.find(group=>group.id===id);if(!group)throw new Error('Choose an existing editor group.');const bounds=group.bounds!,x=clamp(position.x,-1e6,1e6),y=clamp(position.y,-1e6,1e6),dx=x-bounds.x,dy=y-bounds.y;
  for(const node of canvasNodes(next,[],[]).filter(node=>group.members.includes(node.id)))next=canvasMove(next,node.id,{x:node.position.x+dx,y:node.position.y+dy});
  return canvasSetGroupBounds(next,id,{...bounds,x,y});
}
export function canvasSaveGroup(document:DeepReadonly<ProfileDocument>,name:string,members:readonly string[],id?:string):ProfileDocument{
  const next=JSON.parse(JSON.stringify(document)) as ProfileDocument,groups=next.editor.groups??[];
  if(id&&!groups.some(group=>group.id===id))throw new Error('Choose an existing editor group.');
  if(!id)id=nextGroupId(groups);const previous=groups.find(group=>group.id===id);
  next.editor.groups=[...groups.filter(group=>group.id!==id),{...previous,id,name:name.trim(),members:[...members]}];return checkedLayout(next);
}
export function canvasDeleteGroup(document:DeepReadonly<ProfileDocument>,id:string):ProfileDocument{
  const next=JSON.parse(JSON.stringify(document)) as ProfileDocument;if(!next.editor.groups?.some(group=>group.id===id))throw new Error('Choose an existing editor group.');
  next.editor.groups=next.editor.groups.filter(group=>group.id!==id);if(!next.editor.groups.length)delete next.editor.groups;return checkedLayout(next);
}
export function canvasPortCollapsed(document:DeepReadonly<ProfileDocument>,nodeId:string,group:ProfilePortGroup):boolean{return document.editor.collapsedPortGroups?.some(pair=>pair.nodeId===nodeId&&pair.group===group)??false;}
export function canvasTogglePorts(document:DeepReadonly<ProfileDocument>,nodeId:string,group:ProfilePortGroup):ProfileDocument{
  const next=JSON.parse(JSON.stringify(document)) as ProfileDocument,collapsed=canvasPortCollapsed(document,nodeId,group);
  next.editor.collapsedPortGroups=(next.editor.collapsedPortGroups??[]).filter(pair=>pair.nodeId!==nodeId||pair.group!==group);
  if(!collapsed)next.editor.collapsedPortGroups.push({nodeId,group});if(!next.editor.collapsedPortGroups.length)delete next.editor.collapsedPortGroups;return checkedLayout(next);
}
export function canvasStatus(node:CanvasNode):string{return node.kind==='processing'?'Explicit transformation':node.kind==='virtual'||node.kind==='desktop'?'Profile output':!node.assignment?'Unassigned':node.observation?.availability==='online'?'Online':node.observation?.availability==='unavailable'?'Unavailable / dormant':'Offline · saved block retained';}
export function canvasTransports(node:CanvasNode):string[]{return[...new Set(node.observation?.connections.map(connection=>connection.route.transport.toUpperCase())??[])];}
export function canvasSearch(nodes:readonly CanvasNode[],query:string):CanvasNode[]{const value=query.trim().toLowerCase();return nodes.filter(node=>`${node.name} ${node.category} ${canvasStatus(node)} ${canvasTransports(node).join(' ')}`.toLowerCase().includes(value));}
export function scalarLabel(schema:DeepReadonly<ProfileScalarSchema>):string{
  if(schema.type==='integer'||schema.type==='number')return`${schema.type} ${schema.min}…${schema.max}${schema.unit?` ${schema.unit}`:''}`;
  if(schema.type==='text')return`text · ${schema.encoding} · ${schema.maxBytes} bytes`;
  if(schema.type==='enum')return`enum · ${schema.values.join(' / ')}`;
  return schema.type;
}
export function portContractLabel(contract:DeepReadonly<ProfilePortContract>):string{
  if(contract.delivery==='state')return`state · ${scalarLabel(contract.schema)} · fresh ${contract.freshnessMs} ms · fallback ${contract.fallback===null?'unknown':String(contract.fallback)}`;
  if(contract.delivery==='event')return`event · ${scalarLabel(contract.schema)}`;
  return`${contract.delivery} · ${contract.semantics} · ${contract.parameters.length?contract.parameters.map(parameter=>`${parameter.name}: ${scalarLabel(parameter.schema)}`).join(', '):'no arguments'}`;
}
export function canvasGameId(node:CanvasNode):GameId|null{const id=node.observation?.attributes.gameId;return isGameId(id)?id:null;}
