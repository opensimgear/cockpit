import type {ProfileGraphDraft} from '../profile-graph';
import type {ProfileDiscoveredPort} from '../profile-discovery';
import type {ResourceDetails} from '../profile-inventory';
import type {DeepReadonly,ProfilePortContract} from '../profiles';

const canonical=(value:unknown):string=>JSON.stringify(value,(_key,item)=>item&&typeof item==='object'&&!Array.isArray(item)?Object.fromEntries(Object.entries(item).sort(([a],[b])=>a<b?-1:a>b?1:0)):item);
function contractKey(contract:DeepReadonly<ProfilePortContract>):string {
  const copy=JSON.parse(JSON.stringify(contract)) as ProfilePortContract;
  if(copy.delivery==='action'||copy.delivery==='invocation')for(const parameter of copy.parameters)parameter.name=parameter.id;
  return canonical(copy);
}

/** Browsing a native catalogue never creates an assignment or a declaration. */
export function integrationCataloguePorts(value:DeepReadonly<ProfileGraphDraft>,nodeId:string,sourceId:string,details:DeepReadonly<ResourceDetails>|null|undefined):ProfileDiscoveredPort[] {
  const resource=value.document.resources.find(resource=>resource.id===nodeId);
  if(resource?.kind!=='game'||!details||details.resource.id!==sourceId||details.resource.resourceType!=='game-integration')return [];
  const assignment=value.assignments.find(assignment=>assignment.resourceId===nodeId&&assignment.sourceId===sourceId);
  return details.ports.filter(observed=>observed.current).map(observed=>{
    const pin=assignment?.ports.find(pin=>pin.observedPortId===observed.descriptor.id&&pin.expectedContractHash===observed.contractHash);
    const saved=pin&&resource.ports.find(port=>port.id===pin.profilePortId&&port.direction===observed.descriptor.direction&&contractKey(port.contract)===contractKey(observed.descriptor.contract));
    const key=saved?saved.id:`@observed:${JSON.stringify([observed.descriptor.id,observed.contractHash])}`;
    return {...observed.descriptor,...(saved?{name:saved.name}:{}),id:key,key,profilePortId:saved?saved.id:null,sourceId,observed,current:true};
  });
}
