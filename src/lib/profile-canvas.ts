import type { DeepReadonly, ProfileDocument, ProfileLocalAssignment, ProfilePortContract, ProfileScalarSchema } from './profiles';
import type { ResourceSummary } from './profile-inventory';
import type { GameId } from './games';
import { PROFILE_BLOCK_HEIGHT, PROFILE_BLOCK_WIDTH, profileAutoLayout, profileCategory } from './profile-editor';
export type CanvasPoint = {x:number;y:number};
export type CanvasViewport = CanvasPoint & {zoom:number};
export type CanvasNode = {id:string;name:string;kind:'resource'|'processing'|'virtual';category:string;position:CanvasPoint;resource?:DeepReadonly<ProfileDocument['resources'][number]>;processing?:DeepReadonly<ProfileDocument['processingNodes'][number]>;virtual?:DeepReadonly<ProfileDocument['virtualOutputs'][number]>;assignment?:DeepReadonly<ProfileLocalAssignment>;observation?:DeepReadonly<ResourceSummary>};
const clamp=(value:number,min:number,max:number)=>Math.min(max,Math.max(min,value));
const finite=(value:number)=>Number.isFinite(value);
export function canvasNodes(document:DeepReadonly<ProfileDocument>,assignments:readonly DeepReadonly<ProfileLocalAssignment>[],inventory:readonly DeepReadonly<ResourceSummary>[]):CanvasNode[]{
  const positions=new Map(document.editor.positions.map(position=>[position.nodeId,position]));
  const nodes:Omit<CanvasNode,'position'>[]=[...document.resources.map(resource=>{const assignment=assignments.find(value=>value.resourceId===resource.id);const observation=inventory.find(value=>value.id===assignment?.sourceId);return{id:resource.id,name:resource.name,kind:'resource' as const,category:observation?profileCategory(observation):resource.kind==='game'?'Game integration':resource.kind==='companion'?'Companion':'Physical resource',resource,assignment,observation};}),...document.processingNodes.map(processing=>({id:processing.id,name:processing.name,kind:'processing' as const,category:'Processing',processing})),...document.virtualOutputs.map(virtual=>({id:virtual.id,name:virtual.name,kind:'virtual' as const,category:'Virtual controller',virtual}))];
  const missing=profileAutoLayout(nodes.filter(node=>!positions.has(node.id)).map(node=>node.id));
  const bottom=document.editor.positions.reduce((max,p)=>Math.max(max,p.y+PROFILE_BLOCK_HEIGHT+48),0);
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
export function canvasFit(nodes:readonly Pick<CanvasNode,'position'>[],width:number,height:number):CanvasViewport{
  if(![width,height].every(finite)||width<=0||height<=0)throw new Error('The canvas needs a positive viewport size.');
  if(!nodes.length)return{x:0,y:0,zoom:1};
  const left=Math.min(...nodes.map(node=>node.position.x)),top=Math.min(...nodes.map(node=>node.position.y));
  const right=Math.max(...nodes.map(node=>node.position.x+PROFILE_BLOCK_WIDTH)),bottom=Math.max(...nodes.map(node=>node.position.y+PROFILE_BLOCK_HEIGHT));
  const zoom=clamp(Math.min((width-64)/(right-left),(height-64)/(bottom-top)),0.1,1.5);
  return{x:clamp((width-(right-left)*zoom)/2-left*zoom,-1e6,1e6),y:clamp((height-(bottom-top)*zoom)/2-top*zoom,-1e6,1e6),zoom};
}
export function canvasMove(document:DeepReadonly<ProfileDocument>,nodeId:string,position:CanvasPoint):ProfileDocument{
  if(![position.x,position.y].every(finite)||![...document.resources,...document.processingNodes,...document.virtualOutputs].some(node=>node.id===nodeId))throw new Error('Move an existing block to finite coordinates.');
  const next=JSON.parse(JSON.stringify(document)) as ProfileDocument;next.editor.positions=next.editor.positions.filter(p=>p.nodeId!==nodeId);next.editor.positions.push({nodeId,x:clamp(position.x,-1e6,1e6),y:clamp(position.y,-1e6,1e6)});return next;
}
export function canvasStatus(node:CanvasNode):string{return node.kind==='processing'?'Explicit transformation':node.kind==='virtual'?'Profile output':!node.assignment?'Unassigned':node.observation?.availability==='online'?'Online':node.observation?.availability==='unavailable'?'Unavailable / dormant':'Offline · saved block retained';}
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
export function canvasGameId(node:CanvasNode):GameId|null{const id=node.observation?.attributes.gameId;return id==='dcs'||id==='assetto-corsa-rally'||id==='x-plane-11'||id==='x-plane-12'?id:null;}
