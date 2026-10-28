import {graphContractError,graphPort,graphIsolateSignalLane,type ProfileGraphDraft} from './profile-graph';
import {profileDiscoveredPorts,type ProfileDiscoveredPort} from './profile-discovery';
import {canvasConnect,canvasPortWriter} from './profile-canvas-construction';
import {profileRouteVehicleScope} from './profile-integration-scope';
import {signalOverview,type SignalLane} from './profile-signal-overview';
import type {DeepReadonly,ProfileEndpoint} from './profiles';

export type RouteEndpointEdit={laneId:string;side:'from'|'to';endpoint:DeepReadonly<ProfileEndpoint>};
const same=(a:DeepReadonly<ProfileEndpoint>,b:DeepReadonly<ProfileEndpoint>)=>a.nodeId===b.nodeId&&a.portId===b.portId;
function laneFor(value:DeepReadonly<ProfileGraphDraft>,edit:RouteEndpointEdit):SignalLane {
  const lane=signalOverview(value.document).flatMap(bundle=>bundle.lanes).find(lane=>lane.id===edit.laneId);
  if(!lane||(edit.side==='to'?!same(lane.destination,edit.endpoint):!lane.sources.some(source=>same(source,edit.endpoint))))throw Error('This signal endpoint changed. Open its selector again.');
  return lane;
}
/** Offer compatible ports on the row's device without stealing another input. */
export function routeEndpointChoices(value:DeepReadonly<ProfileGraphDraft>,edit:RouteEndpointEdit,ports:readonly ProfileDiscoveredPort[]):ProfileDiscoveredPort[] {
  let lane:SignalLane;try{lane=laneFor(value,edit);}catch{return [];}
  const boundaries=value.document.bindings.filter(binding=>edit.side==='to'?binding.id===lane.id:lane.bindingIds.includes(binding.id)&&same(binding.from,edit.endpoint));
  let scope:string;try{const terminal=value.document.bindings.find(binding=>binding.id===lane.id);scope=terminal?profileRouteVehicleScope(value.document,terminal.from.nodeId,terminal.to.nodeId):'';}catch{return [];}
  return ports.filter(port=>port.direction===(edit.side==='from'?'output':'input')&&boundaries.every(binding=>{
    const opposite=graphPort(value.document,edit.side==='from'?binding.to:binding.from);
    return opposite&&!graphContractError(edit.side==='from'?port.contract:opposite.contract,edit.side==='from'?opposite.contract:port.contract);
  })&&(edit.side==='from'||!canvasPortWriter(value.document,edit.endpoint.nodeId,port,lane.id,scope)));
}
/** Exact discovered port pins and source branch isolation commit in one edit. */
export function reconnectRouteEndpoint(value:DeepReadonly<ProfileGraphDraft>,edit:RouteEndpointEdit,port:ProfileDiscoveredPort):ProfileGraphDraft {
  laneFor(value,edit);
  if(!routeEndpointChoices(value,edit,[port]).length)throw Error('Choose a compatible, available port.');
  let next=edit.side==='from'?graphIsolateSignalLane(value,edit.laneId):value;
  const lane=laneFor(next,edit),bindings=next.document.bindings.filter(binding=>edit.side==='to'?binding.id===lane.id:lane.bindingIds.includes(binding.id)&&same(binding.from,edit.endpoint));
  for(const binding of bindings){
    const endpoint=edit.side==='from'?binding.to:binding.from;
    const opposite=profileDiscoveredPorts(next,endpoint.nodeId).find(port=>port.profilePortId===endpoint.portId);
    if(!opposite)throw Error('The connected port is no longer available.');
    const chosen={nodeId:edit.endpoint.nodeId,port},existing={nodeId:endpoint.nodeId,port:opposite};
    next=canvasConnect(next,edit.side==='from'?chosen:existing,edit.side==='from'?existing:chosen,binding.id);
  }
  return next as ProfileGraphDraft;
}
