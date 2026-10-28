import {profileRouteVehicleScope, profileVehicleScopesOverlap} from './profile-integration-scope';
import {graphConnect, graphConnectionError, graphContractError, graphDisconnect, graphPort, type ProfileGraphDraft} from './profile-graph';
import {profilePrepareDiscoveredPorts, type ProfileDiscoveredEndpoint, type ProfileDiscoveredPort} from './profile-discovery';
import {isProcessingDraft, resolveProcessingDraft, resolveProcessingNode} from './profile-transform-drafts';
import type {DeepReadonly, ProfileBinding, ProfileDocument} from './profiles';

/** Keep an inserted block near the drop without enclosing another attachment. */
export function canvasInsertionPosition(nodes:readonly {position:{x:number;y:number}}[],point:{x:number;y:number},width:number,height:number):{x:number;y:number}{
  const margin=32,free=(p:{x:number;y:number})=>nodes.every(node=>p.x+width+margin<=node.position.x||p.x>=node.position.x+width+margin||p.y+height+margin<=node.position.y||p.y>=node.position.y+height+margin);
  if(free(point))return point;
  const candidates=nodes.flatMap(node=>[{x:point.x,y:node.position.y+height+margin},{x:point.x,y:node.position.y-height-margin},{x:node.position.x+width+margin,y:point.y},{x:node.position.x-width-margin,y:point.y}]).filter(p=>Math.abs(p.x)<=1e6&&Math.abs(p.y)<=1e6&&free(p));
  const distance=(p:{x:number;y:number})=>(p.x-point.x)**2+(p.y-point.y)**2;
  return candidates.sort((a,b)=>distance(a)-distance(b))[0]??{x:point.x,y:Math.max(point.y,...nodes.map(node=>node.position.y+height))+margin};
}

export function canvasPortReason(document:DeepReadonly<ProfileDocument>,from:ProfileDiscoveredEndpoint,to:ProfileDiscoveredEndpoint):string {
  if(from.port.direction!=='output'||to.port.direction!=='input')return 'Connect a source output to a destination input.';
  if(isProcessingDraft(document,from.nodeId))return 'Connect this transformation’s input first to establish its output type.';
  const pending=document.processingNodes.find(node=>node.id===to.nodeId);
  if(pending?.draft){try{
    const portId=to.port.profilePortId??to.port.key,resolved=resolveProcessingNode(pending,from.port.contract,portId),target=resolved.inputs.find(port=>port.id===portId);
    return target?graphContractError(from.port.contract,target.contract):'This transformation port changed. Choose its port again.';
  }catch(error){return error instanceof Error?error.message:String(error);}}
  return graphContractError(from.port.contract,to.port.contract);
}

function currentProcessingPort(document:DeepReadonly<ProfileDocument>,endpoint:ProfileDiscoveredEndpoint,resolvedNodeId?:string):ProfileDiscoveredEndpoint {
  // Only the freshly resolved target replaces its editing placeholder. Every
  // other captured choice must still match its exact saved contract.
  if(endpoint.nodeId!==resolvedNodeId)return endpoint;
  const port=graphPort(document,{nodeId:endpoint.nodeId,portId:endpoint.port.profilePortId??endpoint.port.key});
  if(!port)throw Error('This transformation port changed. Choose its port again.');
  return {nodeId:endpoint.nodeId,port:{...endpoint.port,...port,key:port.id,profilePortId:port.id,sourceId:null,observed:null,current:true}};
}

export function canvasConnectionPlan(value:DeepReadonly<ProfileGraphDraft>,from:ProfileDiscoveredEndpoint,to:ProfileDiscoveredEndpoint,replacingId?:string) {
  if(replacingId&&!value.document.bindings.some(binding=>binding.id===replacingId))throw Error('This connection was removed. Choose an existing wire to edit.');
  if(isProcessingDraft(value.document,from.nodeId))throw Error('Connect this transformation’s input first to establish its output type.');
  const pending=isProcessingDraft(value.document,to.nodeId),resolved=pending?resolveProcessingDraft(value,to.nodeId,from.port.contract,to.port.profilePortId??to.port.key):value;
  const prepared=profilePrepareDiscoveredPorts(resolved,from,currentProcessingPort(resolved.document,to,pending?to.nodeId:undefined));
  const scope=profileRouteVehicleScope(prepared.draft.document,prepared.from.nodeId,prepared.to.nodeId);
  const writer=prepared.draft.document.bindings.find(binding=>binding.id!==replacingId&&binding.to.nodeId===prepared.to.nodeId&&binding.to.portId===prepared.to.portId&&profileVehicleScopesOverlap(scope,profileRouteVehicleScope(prepared.draft.document,binding.from.nodeId,binding.to.nodeId)));
  const check={...prepared.draft.document,bindings:prepared.draft.document.bindings.filter(binding=>binding.id!==replacingId&&binding.id!==writer?.id)};
  const error=graphConnectionError(check,prepared.from,prepared.to);
  return {...prepared,writer,error};
}

/** Resolution, exact port pins, replacement and the final wire are one undoable edit. */
export function canvasConnect(value:DeepReadonly<ProfileGraphDraft>,from:ProfileDiscoveredEndpoint,to:ProfileDiscoveredEndpoint,replacingId?:string,replaceOccupied=false):ProfileGraphDraft {
  const plan=canvasConnectionPlan(value,from,to,replacingId);
  if(plan.error)throw Error(plan.error);
  if(plan.writer&&!replaceOccupied)throw Error('This input is already connected. Choose Replace connection to change its writer.');
  let next=plan.draft;
  if(plan.writer&&replacingId)next=graphDisconnect(next,plan.writer.id);
  return graphConnect(next,plan.from,plan.to,replacingId??plan.writer?.id);
}

export function canvasPortWriter(document:DeepReadonly<ProfileDocument>,nodeId:string,port:DeepReadonly<ProfileDiscoveredPort>,replacingId?:string,routeScope?:string):DeepReadonly<ProfileBinding>|undefined {
  return port.profilePortId?document.bindings.find(binding=>binding.id!==replacingId&&binding.to.nodeId===nodeId&&binding.to.portId===port.profilePortId&&(routeScope===undefined||profileVehicleScopesOverlap(routeScope,profileRouteVehicleScope(document,binding.from.nodeId,binding.to.nodeId)))):undefined;
}
