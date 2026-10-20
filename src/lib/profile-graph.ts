import { profileAssignmentsError, profileDocumentError, type DeepReadonly, type ProfileBinding, type ProfileControllerLayout, type ProfileDocument, type ProfileEndpoint, type ProfileLocalAssignment, type ProfilePort, type ProfilePortContract, type ProfileProcessingNode, type ProfileScalarSchema, type ProfileVirtualOutput } from './profiles';
import { resourcePortError, type ObservedResourcePort, type ResourceSummary } from './profile-inventory';
import { desktopActionLabel, desktopActionPort, type ProfileDesktopOutput } from './profile-desktop';

export type ProfileGraphDraft = { document: ProfileDocument; assignments: ProfileLocalAssignment[] };
export type ProfileGraphSelection = { profilePortId: string; observed: DeepReadonly<ObservedResourcePort> };
const clone = <T>(value: DeepReadonly<T>): T => JSON.parse(JSON.stringify(value));
const sameEndpoint = (a: DeepReadonly<ProfileEndpoint>, b: DeepReadonly<ProfileEndpoint>) => a.nodeId === b.nodeId && a.portId === b.portId;
const nodes = (document: DeepReadonly<ProfileDocument>) => [...document.resources, ...document.processingNodes, ...document.virtualOutputs,...(document.desktopOutputs??[])];
const label = (value: string) => [...value.trim()].slice(0,80).join('') || 'Selected port';
const hash = (value: string) => /^sha256:[0-9a-f]{64}$/.test(value);

export function graphFreshId(document: DeepReadonly<ProfileDocument>, prefix: 'route'|'transform'|'virtual'|'desktop'|'port'): string {
  const used = new Set([...nodes(document).map(node=>node.id), ...document.bindings.map(binding=>binding.id), ...nodes(document).flatMap(node=>'ports' in node ? node.ports.map(port=>port.id) : [...node.inputs,...node.outputs].map(port=>port.id))]);
  for(let n=1;n<=4096;n++)if(!used.has(`${prefix}-${n}`))return `${prefix}-${n}`;
  throw new Error('No unused logical graph identity is available.');
}
export function graphPorts(document: DeepReadonly<ProfileDocument>, nodeId: string): readonly DeepReadonly<ProfilePort>[] {
  const node=nodes(document).find(node=>node.id===nodeId);
  return !node ? [] : 'ports' in node ? node.ports : [...node.inputs,...node.outputs];
}
export function graphPort(document: DeepReadonly<ProfileDocument>, endpoint: DeepReadonly<ProfileEndpoint>): DeepReadonly<ProfilePort>|undefined {
  return graphPorts(document,endpoint.nodeId).find(port=>port.id===endpoint.portId);
}
export function graphScalarFits(from: DeepReadonly<ProfileScalarSchema>, to: DeepReadonly<ProfileScalarSchema>): boolean {
  if(from.type!==to.type)return false;
  if((from.type==='integer'||from.type==='number')&&(to.type==='integer'||to.type==='number'))return from.unit===to.unit&&from.min>=to.min&&from.max<=to.max;
  if(from.type==='text'&&to.type==='text')return from.maxBytes<=to.maxBytes&&(from.encoding===to.encoding||from.encoding==='ascii');
  if(from.type==='enum'&&to.type==='enum')return from.values.every(value=>to.values.includes(value));
  return true;
}
/** This mirrors native wire compatibility. Conversion never happens on an edge. */
export function graphContractError(from: DeepReadonly<ProfilePortContract>, to: DeepReadonly<ProfilePortContract>): string {
  if((from.delivery==='state'&&to.delivery==='state')||(from.delivery==='event'&&to.delivery==='event'))return graphScalarFits(from.schema,to.schema)?'':'Types, units or declared ranges differ. Add an explicit conversion node.';
  if(from.delivery==='invocation'&&to.delivery==='action')return from.semantics===to.semantics&&from.parameters.length===to.parameters.length&&from.parameters.every((parameter,index)=>parameter.id===to.parameters[index].id&&graphScalarFits(parameter.schema,to.parameters[index].schema))?'':'Ordered action arguments or invocation semantics differ. Configure an explicit argument-pack node.';
  return to.delivery==='action'?'An action needs one complete invocation. Add Action pack (event trigger) or Set on change (setter).':'Signal delivery differs. Add an explicit event/state conversion node.';
}
export function graphConnectionError(document: DeepReadonly<ProfileDocument>, from: DeepReadonly<ProfileEndpoint>, to: DeepReadonly<ProfileEndpoint>, replacingId?: string): string {
  const source=graphPort(document,from),target=graphPort(document,to);
  if(!source||!target)return 'Choose an existing concrete source output and destination port.';
  if(source.direction!=='output'||target.direction!=='input')return 'Connect a source output to a destination input or action.';
  const contractError=graphContractError(source.contract,target.contract);if(contractError)return contractError;
  const bindings=document.bindings.filter(binding=>binding.id!==replacingId);
  if(bindings.some(binding=>sameEndpoint(binding.to,to)))return 'This input already has a writer. Use an explicit OR or priority node for fan-in.';
  if(bindings.length>=512)return 'A profile supports at most 512 explicit signal routes. Remove a route first.';
  const processing=new Set(document.processingNodes.map(node=>node.id)),edges=new Map<string,string[]>();
  for(const binding of [...bindings,{from,to}])if(processing.has(binding.from.nodeId)&&processing.has(binding.to.nodeId))edges.set(binding.from.nodeId,[...(edges.get(binding.from.nodeId)??[]),binding.to.nodeId]);
  const visiting=new Set<string>(),done=new Set<string>();
  const cycle=(id:string):boolean=>{if(visiting.has(id))return true;if(done.has(id))return false;visiting.add(id);for(const next of edges.get(id)??[])if(cycle(next))return true;visiting.delete(id);done.add(id);return false;};
  if([...processing].some(cycle))return 'This connection creates a processing cycle. External command and telemetry directions remain separate.';
  const resources=new Set(document.resources.map(resource=>resource.id));
  if(new Set([...bindings.map(binding=>binding.from.nodeId),from.nodeId].filter(id=>resources.has(id))).size>16)return 'The native engine supports at most 16 used source resources. Split this setup into profiles.';
  return '';
}

/** Validate before cloning so NaN/Infinity cannot be silently converted to null. */
export function graphDraft(value: DeepReadonly<ProfileGraphDraft>): ProfileGraphDraft {
  const error=profileDocumentError({...value.document,name:'Profile draft'})||profileAssignmentsError(value.assignments);
  if(error)throw new Error(error);
  for(const assignment of value.assignments){
    const resource=value.document.resources.find(resource=>resource.id===assignment.resourceId);
    if(!resource||resource.ports.length!==assignment.ports.length||resource.ports.some(port=>!assignment.ports.some(pin=>pin.profilePortId===port.id)))throw new Error('A local assignment must pin every selected logical port of its resource exactly once.');
  }
  return clone<ProfileGraphDraft>(value);
}
function checked(value: ProfileGraphDraft): ProfileGraphDraft {
  const next=graphDraft(value);
  for(const binding of next.document.bindings){const error=graphConnectionError(next.document,binding.from,binding.to,binding.id);if(error)throw new Error(`${binding.id}: ${error}`);}
  return next;
}
export function graphConnect(value: DeepReadonly<ProfileGraphDraft>, from: ProfileEndpoint, to: ProfileEndpoint, replacingId?: string): ProfileGraphDraft {
  const next=graphDraft(value);
  if(replacingId&&!next.document.bindings.some(binding=>binding.id===replacingId))throw new Error('Choose an existing route to edit.');
  const error=graphConnectionError(next.document,from,to,replacingId);if(error)throw new Error(error);
  const binding:ProfileBinding={id:replacingId??graphFreshId(next.document,'route'),from:clone(from),to:clone(to)};
  next.document.bindings=next.document.bindings.filter(row=>row.id!==binding.id);next.document.bindings.push(binding);return checked(next);
}
export function graphDisconnect(value: DeepReadonly<ProfileGraphDraft>, bindingId: string): ProfileGraphDraft {
  const next=graphDraft(value);if(!next.document.bindings.some(binding=>binding.id===bindingId))throw new Error('Choose an existing route to remove.');
  next.document.bindings=next.document.bindings.filter(binding=>binding.id!==bindingId);return checked(next);
}
/** Scope belongs to the saved endpoint; native identities and enablement remain local. */
export function graphSetDesktop(value:DeepReadonly<ProfileGraphDraft>,output:ProfileDesktopOutput):ProfileGraphDraft {
  const next=graphDraft(value),existing=next.document.desktopOutputs??[];
  if(nodes(next.document).some(node=>node.id===output.id)&&!existing.some(node=>node.id===output.id))throw new Error('Use a unique desktop endpoint identity.');
  if(existing.some(node=>node.id!==output.id))throw new Error('A profile supports one logical keyboard and mouse endpoint.');
  const updated=clone<ProfileDesktopOutput>(output);
  updated.ports=updated.actions.map(definition=>{
    const previous=existing.find(node=>node.id===output.id),oldAction=previous?.actions.find(action=>action.portId===definition.portId),oldName=previous?.ports.find(port=>port.id===definition.portId)?.name;
    const name=!oldAction||oldName===desktopActionLabel(oldAction.action)?desktopActionLabel(definition.action):updated.ports.find(port=>port.id===definition.portId)?.name??desktopActionLabel(definition.action);
    return desktopActionPort(definition.portId,name,definition.action);
  });
  // A changed action contract invalidates connected edges; the user must retarget explicitly.
  for(const binding of next.document.bindings.filter(binding=>binding.to.nodeId===updated.id)){
    const previous=existing.find(node=>node.id===updated.id)?.ports.find(port=>port.id===binding.to.portId),replacement=updated.ports.find(port=>port.id===binding.to.portId);
    if(!replacement||previous&&canonical(previous.contract)!==canonical(replacement.contract))throw new Error('Disconnect routes before removing an action or changing its input delivery.');
  }
  next.document.desktopOutputs=[updated];return checked(next);
}
function portableContract(contract: DeepReadonly<ProfilePortContract>): ProfilePortContract {
  const next=clone(contract);if(next.delivery==='action'||next.delivery==='invocation')for(const parameter of next.parameters)parameter.name=parameter.id;return next;
}
const canonical=(value:unknown):string=>JSON.stringify(value,(_key,item)=>item&&typeof item==='object'&&!Array.isArray(item)?Object.fromEntries(Object.entries(item).sort(([a],[b])=>a<b?-1:a>b?1:0)):item);
/** Conservative preview invalidation; this metadata is never execution authority. */
export function graphInventorySignature(inventory:readonly DeepReadonly<ResourceSummary>[]):string{return canonical(inventory.map(resource=>({id:resource.id,resourceType:resource.resourceType,availability:resource.availability,generation:resource.generation,capabilityRevision:resource.capabilityRevision,portCount:resource.portCount,cachedPortCount:resource.cachedPortCount,attributes:resource.attributes,diagnostics:resource.diagnostics,connections:[...resource.connections].sort((a,b)=>canonical([a.scope,a.key,a.route.id]).localeCompare(canonical([b.scope,b.key,b.route.id])))})).sort((a,b)=>a.id<b.id?-1:a.id>b.id?1:0));}
function portSelection(existing: DeepReadonly<ProfilePort>, observed: DeepReadonly<ObservedResourcePort>): void {
  if(resourcePortError(observed.descriptor)||!hash(observed.contractHash))throw new Error('Choose an advertised port with its exact native contract hash.');
  if(existing.direction!==observed.descriptor.direction||canonical(portableContract(existing.contract))!==canonical(portableContract(observed.descriptor.contract)))throw new Error('The selected provider contract differs from the saved logical port. Select a matching port or explicitly replace its graph declaration.');
}
export function graphAssignResource(value: DeepReadonly<ProfileGraphDraft>, resourceId: string, source: DeepReadonly<ResourceSummary>, selections: readonly ProfileGraphSelection[]): ProfileGraphDraft {
  const next=graphDraft(value),resource=next.document.resources.find(resource=>resource.id===resourceId);if(!resource)throw new Error('Choose an existing resource block.');
  const kind=source.resourceType==='game-integration'?'game':source.resourceType==='companion'?'companion':'device';
  if(kind!==resource.kind)throw new Error('Choose an inventory resource of the same category.');
  if(selections.length!==resource.ports.length||new Set(selections.map(selection=>selection.profilePortId)).size!==selections.length)throw new Error('Map every saved logical port explicitly before assigning this resource.');
  for(const port of resource.ports){const selection=selections.find(selection=>selection.profilePortId===port.id);if(!selection)throw new Error('Map every saved logical port explicitly before assigning this resource.');portSelection(port,selection.observed);}
  next.assignments=next.assignments.filter(assignment=>assignment.resourceId!==resourceId);
  next.assignments.push({resourceId,sourceId:source.id,ports:selections.map(selection=>({profilePortId:selection.profilePortId,observedPortId:selection.observed.descriptor.id,expectedContractHash:selection.observed.contractHash}))});return checked(next);
}
/** Native IDs stay in local pins even when longer than a legal portable port ID. */
export function graphSelectProviderPort(value: DeepReadonly<ProfileGraphDraft>, resourceId: string, sourceId: string, observed: DeepReadonly<ObservedResourcePort>, logicalPortId?: string): ProfileGraphDraft {
  const next=graphDraft(value),resource=next.document.resources.find(resource=>resource.id===resourceId),assignment=next.assignments.find(assignment=>assignment.resourceId===resourceId);
  if(!resource||!assignment||assignment.sourceId!==sourceId)throw new Error('Explicitly assign this block to the selected inventory resource first.');
  if(resourcePortError(observed.descriptor)||!hash(observed.contractHash))throw new Error('Choose an advertised port with its exact native contract hash.');
  const previous=assignment.ports.find(pin=>pin.observedPortId===observed.descriptor.id);
  if(previous){if(previous.expectedContractHash!==observed.contractHash)throw new Error('This port contract changed. Explicitly remove or replace its saved declaration and routes.');return next;}
  const id=logicalPortId??graphFreshId(next.document,'port');
  if(resource.ports.some(port=>port.id===id))throw new Error('Choose an unused logical port identity.');
  resource.ports.push({id,name:label(observed.descriptor.name),direction:observed.descriptor.direction,contract:clone<ProfilePortContract>(observed.descriptor.contract)});
  assignment.ports.push({profilePortId:id,observedPortId:observed.descriptor.id,expectedContractHash:observed.contractHash});return checked(next);
}
export function graphRemovePort(value: DeepReadonly<ProfileGraphDraft>, resourceId: string, portId: string): ProfileGraphDraft {
  const next=graphDraft(value),resource=next.document.resources.find(resource=>resource.id===resourceId);
  if(!resource?.ports.some(port=>port.id===portId))throw new Error('Choose an existing resource port.');
  resource.ports=resource.ports.filter(port=>port.id!==portId);
  for(const assignment of next.assignments)if(assignment.resourceId===resourceId)assignment.ports=assignment.ports.filter(pin=>pin.profilePortId!==portId);
  next.document.bindings=next.document.bindings.filter(binding=>!sameEndpoint(binding.from,{nodeId:resourceId,portId})&&!sameEndpoint(binding.to,{nodeId:resourceId,portId}));return checked(next);
}
export function graphRemoveNode(value: DeepReadonly<ProfileGraphDraft>, nodeId: string): ProfileGraphDraft {
  const next=graphDraft(value);if(!nodes(next.document).some(node=>node.id===nodeId))throw new Error('Choose an existing block.');
  next.document.resources=next.document.resources.filter(node=>node.id!==nodeId);next.document.processingNodes=next.document.processingNodes.filter(node=>node.id!==nodeId);next.document.virtualOutputs=next.document.virtualOutputs.filter(node=>node.id!==nodeId);
  if(next.document.desktopOutputs){next.document.desktopOutputs=next.document.desktopOutputs.filter(node=>node.id!==nodeId);if(!next.document.desktopOutputs.length)delete next.document.desktopOutputs;}
  next.document.bindings=next.document.bindings.filter(binding=>binding.from.nodeId!==nodeId&&binding.to.nodeId!==nodeId);next.document.editor.positions=next.document.editor.positions.filter(position=>position.nodeId!==nodeId);
  if(next.document.editor.groups)next.document.editor.groups=next.document.editor.groups.map(group=>({...group,members:group.members.filter(member=>member!==nodeId)}));
  if(next.document.editor.collapsedPortGroups){next.document.editor.collapsedPortGroups=next.document.editor.collapsedPortGroups.filter(pair=>pair.nodeId!==nodeId);if(!next.document.editor.collapsedPortGroups.length)delete next.document.editor.collapsedPortGroups;}
  next.document.visibility=next.document.visibility.filter(intent=>intent.resourceId!==nodeId);next.document.overrides=next.document.overrides.filter(intent=>intent.resourceId!==nodeId);next.document.companionAssignments=next.document.companionAssignments.filter(intent=>intent.resourceId!==nodeId);next.assignments=next.assignments.filter(assignment=>assignment.resourceId!==nodeId);return checked(next);
}
export function graphSetProcessing(value: DeepReadonly<ProfileGraphDraft>, node: ProfileProcessingNode): ProfileGraphDraft {
  const next=graphDraft(value);if([...next.document.resources,...next.document.virtualOutputs,...(next.document.desktopOutputs??[])].some(row=>row.id===node.id))throw new Error('Use a unique processing block identity.');
  if(node.inputs.length>128||node.outputs.length>16||node.inputs.some(port=>port.direction!=='input')||node.outputs.some(port=>port.direction!=='output')||!node.outputs.length||new Set([...node.inputs,...node.outputs].map(port=>port.id)).size!==node.inputs.length+node.outputs.length)throw new Error('Processing blocks need unique directed ports and 1–16 declared outputs.');
  next.document.processingNodes=next.document.processingNodes.filter(row=>row.id!==node.id);next.document.processingNodes.push(clone(node));return checked(next);
}
export function graphInsertProcessing(value: DeepReadonly<ProfileGraphDraft>, bindingId: string, node: ProfileProcessingNode, inputPortId: string, outputPortId: string): ProfileGraphDraft {
  const binding=value.document.bindings.find(binding=>binding.id===bindingId);if(!binding)throw new Error('Choose an existing route for this visible transformation.');
  if(nodes(value.document).some(existing=>existing.id===node.id))throw new Error('Insert a new transformation identity.');
  let next=graphSetProcessing(graphDisconnect(value,bindingId),node);
  next=graphConnect(next,clone(binding.from),{nodeId:node.id,portId:inputPortId});
  return graphConnect(next,{nodeId:node.id,portId:outputPortId},clone(binding.to));
}
export function graphBypassProcessing(value: DeepReadonly<ProfileGraphDraft>, nodeId: string): ProfileGraphDraft {
  const node=value.document.processingNodes.find(node=>node.id===nodeId),incoming=value.document.bindings.filter(binding=>binding.to.nodeId===nodeId),outgoing=value.document.bindings.filter(binding=>binding.from.nodeId===nodeId);
  if(!node||node.inputs.length!==1||node.outputs.length!==1||incoming.length!==1||!outgoing.length)throw new Error('Joining a removed transformation requires one connected input and one output. Otherwise remove its routes explicitly.');
  const next=graphRemoveNode(value,nodeId);
  for(const binding of outgoing)next.document.bindings.push({id:binding.id,from:clone(incoming[0].from),to:clone(binding.to)});
  return checked(next);
}
export function graphReorderProcessing(value: DeepReadonly<ProfileGraphDraft>, orderedNodeIds: readonly string[]): ProfileGraphDraft {
  const selected=new Set(orderedNodeIds),document=value.document;
  if(selected.size!==orderedNodeIds.length||selected.size<2||orderedNodeIds.some(id=>{const node=document.processingNodes.find(node=>node.id===id);return !node||node.inputs.length!==1||node.outputs.length!==1;}))throw new Error('Choose at least two unique single-input/output transformations in one linear route.');
  const routes=document.bindings.filter(binding=>selected.has(binding.from.nodeId)||selected.has(binding.to.nodeId)),enter=routes.filter(binding=>!selected.has(binding.from.nodeId)),exit=routes.filter(binding=>!selected.has(binding.to.nodeId));
  if(routes.length!==selected.size+1||enter.length!==1||exit.length!==1||orderedNodeIds.some(id=>routes.filter(binding=>binding.from.nodeId===id).length!==1||routes.filter(binding=>binding.to.nodeId===id).length!==1))throw new Error('Reorder one linear route without fan-in or fan-out. Edit branched connections individually.');
  const next=graphDraft(value),removed=new Set(routes.map(binding=>binding.id));next.document.bindings=next.document.bindings.filter(binding=>!removed.has(binding.id));
  const remaining=routes.filter(binding=>binding!==enter[0]&&binding!==exit[0]).sort((a,b)=>a.id<b.id?-1:a.id>b.id?1:0),ids=[enter[0].id,...remaining.map(binding=>binding.id),exit[0].id];
  let from=clone<ProfileEndpoint>(enter[0].from);
  for(let i=0;i<orderedNodeIds.length;i++){const node=next.document.processingNodes.find(node=>node.id===orderedNodeIds[i])!;next.document.bindings.push({id:ids[i],from,to:{nodeId:node.id,portId:node.inputs[0].id}});from={nodeId:node.id,portId:node.outputs[0].id};}
  next.document.bindings.push({id:ids.at(-1)!,from,to:clone(exit[0].to)});return checked(next);
}
export function graphVirtualPorts(layout: DeepReadonly<ProfileControllerLayout>): ProfilePort[] {
  if(layout.buttons.length>128||layout.axes.length>16||layout.hats.length>4||layout.buttons.length+layout.axes.length+layout.hats.length>128||layout.buttons.length+layout.axes.length+layout.hats.length===0)throw new Error('Choose 1–128 total virtual controls, at most 16 axes and four hats.');
  const state=(id:string,name:string,schema:ProfileScalarSchema,fallback:boolean|number):ProfilePort=>({id,name,direction:'input',contract:{delivery:'state',schema,freshnessMs:1000,fallback}});
  return [...layout.buttons.map((id,index)=>state(id,`Button ${index+1}`,{type:'boolean'},false)),...layout.axes.map((axis,index)=>state(axis.portId,`Axis ${index+1}`,{type:'integer',min:axis.range==='bipolar'?-32768:0,max:32767,unit:null},axis.fallback)),...layout.hats.map((id,index)=>state(id,`Hat ${index+1}`,{type:'integer',min:-1,max:7,unit:null},-1))];
}
export function graphSetVirtual(value: DeepReadonly<ProfileGraphDraft>, output: Omit<ProfileVirtualOutput,'ports'>): ProfileGraphDraft {
  const next=graphDraft(value);if([...next.document.resources,...next.document.processingNodes,...(next.document.desktopOutputs??[])].some(node=>node.id===output.id))throw new Error('Use a unique virtual controller identity.');
  if(!next.document.virtualOutputs.some(node=>node.id===output.id)&&next.document.virtualOutputs.length>=8)throw new Error('The native engine supports at most eight virtual controllers.');
  next.document.virtualOutputs=next.document.virtualOutputs.filter(node=>node.id!==output.id);next.document.virtualOutputs.push({...clone(output),ports:graphVirtualPorts(output.layout)});return checked(next);
}
export function graphSetVisibility(value: DeepReadonly<ProfileGraphDraft>, resourceId: string, mode: 'visible'|'hidden-while-active'): ProfileGraphDraft {
  const next=graphDraft(value);if(!next.document.resources.some(resource=>resource.id===resourceId&&resource.kind==='device'))throw new Error('Visibility intent belongs to an existing physical device resource.');
  next.document.visibility=next.document.visibility.filter(intent=>intent.resourceId!==resourceId);next.document.visibility.push({resourceId,mode});return checked(next);
}
/** Save publication intent only; native admission owns capability and device effects. */
export function graphSetRuntimeOverride(value: DeepReadonly<ProfileGraphDraft>, resourceId: string, enabled: boolean|null): ProfileGraphDraft {
  const next=graphDraft(value);if(!next.document.resources.some(resource=>resource.id===resourceId&&resource.kind==='device'))throw new Error('Controller publication intent belongs to an existing physical device resource.');
  if(enabled!==null&&typeof enabled!=='boolean')throw new Error('Keep configured, enable or disable controller publication explicitly.');
  next.document.overrides=next.document.overrides.filter(intent=>intent.resourceId!==resourceId);if(enabled!==null)next.document.overrides.push({resourceId,enabled});return checked(next);
}
export type PublicationPolicyCapability = {status:'advertised'|'unsupported'|'unavailable';configuredEnabled:boolean|null;message:string};
/** Advisory native metadata only. This cannot authorize or claim publication. */
export function graphPublicationCapability(resource?:DeepReadonly<ResourceSummary>):PublicationPolicyCapability{
  const unavailable={status:'unavailable' as const,configuredEnabled:null,message:'Controller publication capability is unavailable. The saved policy is retained; Apply must verify current native support and ownership.'};
  const unsupported={status:'unsupported' as const,configuredEnabled:null,message:'This current resource does not advertise a supported nonpersistent controller publication capability. Keep configured or reconcile to a supported resource.'};
  if(!resource)return unavailable;if(resource.resourceType!=='osg-module')return unsupported;if(resource.availability!=='online')return unavailable;
  const source=resource.attributes.controllerPublicationOverride;if(!source||new TextEncoder().encode(source).length>4096||!resource.connections.some(connection=>connection.route.transport==='can'))return unsupported;
  const object=(value:unknown):value is Record<string,unknown>=>!!value&&typeof value==='object'&&!Array.isArray(value);
  const exact=(value:unknown,keys:string[]):value is Record<string,unknown>=>object(value)&&Object.keys(value).length===keys.length&&keys.every(key=>Object.hasOwn(value,key));
  const integer=(value:unknown,min:number,max:number):value is number=>typeof value==='number'&&Number.isSafeInteger(value)&&value>=min&&value<=max;
  try{
    const proof:unknown=JSON.parse(source);if(!exact(proof,['schema','epoch','revision','module'])||proof.schema!==1||!integer(proof.epoch,1,0xffffffff)||!integer(proof.revision,1,0xffffffff)||!exact(proof.module,['id','boot','configurationGeneration','node','sourceNode','profile','publicationSlot','configuredEnabled','publicationReportId','configuredDescriptorIndex']))return unsupported;
    const module=proof.module;if(typeof module.id!=='string'||!/^[0-9a-f]{32}$/.test(module.id)||/^(0{32}|f{32})$/.test(module.id)||!integer(module.boot,1,0xffffffff)||!integer(module.configurationGeneration,1,0xffffffff)||!integer(module.node,1,32)||!integer(module.sourceNode,0,0xffffffff)||!integer(module.publicationSlot,0,31)||typeof module.configuredEnabled!=='boolean'||!integer(module.publicationReportId,1,32)||!integer(module.configuredDescriptorIndex,-1,31)||module.configuredEnabled!==(module.configuredDescriptorIndex>=0))return unsupported;
    const profile=module.profile;if(!exact(profile,['revision','buttons','axes','hats','neutral','axisUsage'])||!integer(profile.revision,1,65535)||!integer(profile.buttons,0,128)||!integer(profile.axes,0,8)||!integer(profile.hats,0,4)||!Array.isArray(profile.neutral)||profile.neutral.length!==profile.axes||!Array.from(profile.neutral).every(value=>integer(value,0,65535))||!Array.isArray(profile.axisUsage)||profile.axisUsage.length!==profile.axes||!Array.from(profile.axisUsage).every(value=>integer(value,0x30,0x38))||new Set(profile.axisUsage).size!==profile.axisUsage.length)return unsupported;
    return{status:'advertised',configuredEnabled:module.configuredEnabled,message:'Gateway CAN capability advertised. Apply rechecks the actual route, availability, publication set and ownership; this is not a Ready result.'};
  }catch{return unsupported;}
}
