<script lang="ts">
  import{onDestroy,tick,untrack}from'svelte';
  import {routeEndpointChoices,reconnectRouteEndpoint,type RouteEndpointEdit} from './profile-route-endpoints';
  import {signalOverview}from'./profile-signal-overview';
  import {chainTransformOptions,replaceChainTransform,instanceRowTransforms}from'./profile-chain-edit';
  import ProfileCanvas from './ProfileCanvas.svelte';
  import ProfilePortCombobox from './ProfilePortCombobox.svelte';
  import Icon from './Icon.svelte';
  import ProfileProcessingEditor from './ProfileProcessingEditor.svelte';
  import ProfileVirtualEditor from './ProfileVirtualEditor.svelte';
  import ProfileDesktopEditor from './ProfileDesktopEditor.svelte';
  import type {ProfileDesktopOutput}from'./profile-desktop';
  import {graphSetDesktop}from'./profile-graph';
  import ProfileControllerInputs from './ProfileControllerInputs.svelte';
  import ProfileCompanionEditor from './ProfileCompanionEditor.svelte';
  import type{ProfileLearningClient}from'./profile-learning';
  import {canvasNodes,portContractLabel,type CanvasNode,type CanvasPoint} from './profile-canvas';
  import {profilePaletteCanAdd,profilePlaceComponent,type ProfilePaletteComponent,type ProfilePaletteDrag} from './profile-palette';
  import {profileTabDocument,profileTabNodeIds} from './profile-vehicle-tabs';
  import {PROFILE_BLOCK_HEIGHT,PROFILE_BLOCK_WIDTH}from'./profile-editor';
  import {graphTransferProcessing,graphAssignResource,graphBypassProcessing,graphConnect,graphConnectionError,graphContractError,graphDisconnect,graphFreshId,graphInsertProcessing,graphInventorySignature,graphPort,graphPorts,graphPublicationCapability,graphRemoveNode,graphRemovePort,graphSelectProviderPort,graphSetProcessing,graphSetRuntimeOverride,graphSetVirtual,graphSetVisibility,type ProfileGraphDraft}from'./profile-graph';
  import {PROCESSING_PRESETS,processingActionPack,processingNodeError,processingSummary,processingTemplate,type ProcessingPreset}from'./profile-processing';
  import {ProfilePortBrowser}from'./profile-port-browser';
  import {profileDiscoverResource,profileDiscoverCompanion,ProfileDiscoveredPortCache,profilePhysicalPortPresentation,type ProfileDiscoveredPort}from'./profile-discovery';
  import {canvasConnectionPlan,canvasPortReason,canvasInsertionPosition}from'./profile-canvas-construction';
  import {connectionDraftPlan,commitConnectionDraft,createConnectionTransform}from'./profile-connection-draft';
  import {graphDropProcessing,isProcessingDraft,graphUpdateProcessing,processingDerivedNode,updateProcessingDraft}from'./profile-transform-drafts';
  import ProfileTransformSettings from './ProfileTransformSettings.svelte';
  import {gatewayRequest}from'./gateway';
  import type{CompilationSummary,CompileDiagnostic,ProfileCompilerClient}from'./profile-compilation';
  import type{DeepReadonly,ProfileDocument,ProfileLocalAssignment,ProfilePortContract,ProfileProcessingNode,ProfileVirtualOutput}from'./profiles';
  import type{ObservedResourcePort,ProfileInventoryClient,ResourceDetails,ResourceSummary}from'./profile-inventory';
  import type{GameId}from'./games';
  import {ProfileCompanionAuthoringClient,type ProfileCompanionPage}from'./profile-companion-authoring';
  let{document,assignments,inventory,inventoryClient,compilerClient,learningClient,companionClient,editorContext='',activeVehicleTabId='',active=true,runtimeIdle=false,inventoryRevision=0,readOnly=false,selectedId=$bindable(''),change,openGameSettings,undo,redo,paletteDrag=null,canDropComponent,dropComponent,endPaletteDrag}:{
    document:DeepReadonly<ProfileDocument>;assignments:readonly DeepReadonly<ProfileLocalAssignment>[];inventory:readonly DeepReadonly<ResourceSummary>[];inventoryClient:Pick<ProfileInventoryClient,'inspect'>;compilerClient?:Pick<ProfileCompilerClient,'compile'|'inspect'>;inventoryRevision?:number;readOnly?:boolean;selectedId?:string;
    change:(draft:ProfileGraphDraft,label:string)=>void;openGameSettings?:(gameId:GameId,returnState?:unknown)=>void;undo?:()=>void;redo?:()=>void;
    companionClient?:Pick<ProfileCompanionAuthoringClient,'catalogue'|'inspect'>;learningClient?:Pick<ProfileLearningClient,'start'|'status'|'cancel'>;editorContext?:string;activeVehicleTabId?:string;active?:boolean;runtimeIdle?:boolean;
    paletteDrag?:ProfilePaletteDrag|null;canDropComponent?:(component:ProfilePaletteComponent)=>boolean;dropComponent?:(component:ProfilePaletteComponent,position:CanvasPoint)=>void;endPaletteDrag?:()=>void;
  }=$props();
  const copy=<T,>(value:T):T=>JSON.parse(JSON.stringify(value));
  let message=$state(''),routeOpen=$state(false),fromNode=$state(''),fromPort=$state(''),toNode=$state(''),toPort=$state(''),editingRoute=$state(''),highlighted=$state<string[]>([]),preset=$state<ProcessingPreset>('identity');
  let connectionRoute=$state(''),pendingConnection=$state(false);
  let queuedTransforms=$state<ProfileProcessingNode[]>([]),draftRowOpen=$state(false),selectedTransformLane=$state('');
  let laneError=$state<{laneId:string;side:'from'|'to';message:string}|null>(null);
  let sourceSearch=$state(''),targetSearch=$state('');
  let pickerAnchor=$state<CanvasPoint>({x:400,y:180}),wireAnchor=$state<CanvasPoint>({x:400,y:180});
  let selectedEdgeId=$state(''),autoCommitArmed=$state(false),wireTransforms=$state(false);
  let connectionTrigger:Element|null=null;
  let routingElement:HTMLElement;
  let canvas=$state.raw<{addGroup:(name?:string)=>string|undefined;focusGroup:(id:string)=>void;renameGroup:(id:string,name:string)=>void;deleteGroup:(id:string)=>void;openConnectionOverview:(from:string,to:string)=>void;closeConnectionOverview:()=>void;focusConnectionTransform:(id:string)=>void}>();
  const connectionUid=$props.id();
  const connectionTitleId=`${connectionUid}-connection`;
  let assignmentFor=$state(''),assignmentSource=$state(''),assignmentPage=$state.raw<DeepReadonly<ResourceDetails>|null>(null),assignmentBusy=$state(false),assignmentError=$state(''),selectedMappings=$state.raw<Record<string,DeepReadonly<ObservedResourcePort>>>({}),assignmentEpoch=0;
  let policyInspectorOpen=$state(untrack(()=>!assignments.some(assignment=>assignment.resourceId===selectedId)));
  const assignmentBrowser=new ProfilePortBrowser(untrack(()=>inventoryClient));
  const companionDiscovery=untrack(()=>companionClient)??new ProfileCompanionAuthoringClient(async request=>(await gatewayRequest(request)).profileCompanionAuthoring);
  let discoveries=$state.raw<Record<string,{signature:string;details:DeepReadonly<ResourceDetails|ProfileCompanionPage>|null;busy:boolean;error:string}>>({});
  let discoveryDisposed=false;
  const discoveryChoices=new ProfileDiscoveredPortCache();
  const visibleDocument=$derived(profileTabDocument(document,activeVehicleTabId)),visibleIds=$derived(profileTabNodeIds(document,activeVehicleTabId));
  const nodes=$derived(canvasNodes(visibleDocument,assignments,inventory)),selected=$derived(nodes.find(node=>node.id===selectedId));
  const graph=$derived({document,assignments});
  const connectionBinding=$derived(pendingConnection?{id:'',from:{nodeId:fromNode,portId:fromPort},to:{nodeId:toNode,portId:toPort}}:visibleDocument.bindings.find(binding=>binding.id===connectionRoute));
  const connectionChoices=$derived.by(()=>connectionBinding?PROCESSING_PRESETS.filter(([kind])=>{try{processingInsertion(connectionBinding.id||null,kind);return true;}catch{return false;}}):[]);
  const discoveredChoices=$derived(discoveryChoices.collect(graph,nodes.map(node=>({nodeId:node.id,details:discoveries[node.assignment?.sourceId??'']?.details}))));
  const discoveredPorts=$derived(Object.fromEntries(Object.entries(discoveredChoices).map(([id,ports])=>[id,ports.map(port=>({id:port.key,name:port.name,direction:port.direction,contract:port.contract}))])));
  const discoveryBusy=$derived(nodes.filter(node=>discoveries[node.assignment?.sourceId??'']?.busy).map(node=>node.id));
  const discoveryErrors=$derived(Object.fromEntries(nodes.flatMap(node=>discoveries[node.assignment?.sourceId??'']?.error?[[node.id,discoveries[node.assignment!.sourceId].error]]:[])));
  const sourcePorts=$derived((discoveredChoices[fromNode]??[]).filter(port=>port.direction==='output')),targetPorts=$derived((discoveredChoices[toNode]??[]).filter(port=>port.direction==='input'));
  const fromChoice=$derived(sourcePorts.find(port=>port.key===fromPort)),toChoice=$derived(targetPorts.find(port=>port.key===toPort));
  const inputWriters=$derived.by(()=>{const writers=new Map<string,Map<string,DeepReadonly<ProfileDocument['bindings'][number]>>>();for(const binding of document.bindings){if(binding.id===editingRoute)continue;let ports=writers.get(binding.to.nodeId);if(!ports){ports=new Map();writers.set(binding.to.nodeId,ports);}if(!ports.has(binding.to.portId))ports.set(binding.to.portId,binding);}return writers;});
  function inputWriter(nodeId:string,port:ProfileDiscoveredPort){return port.profilePortId?inputWriters.get(nodeId)?.get(port.profilePortId):undefined;}
  function portPresentation(port:ProfileDiscoveredPort){
    const physical=profilePhysicalPortPresentation(port),identity=port.observed?.descriptor.id??port.key,label=physical?.label??port.name;
    return{label,description:physical?.description??`${identity} · ${portContractLabel(port.contract)}`,searchText:`${label} ${port.name} ${identity} ${port.key}`};
  }
  function portOption(port:ProfileDiscoveredPort,side:'from'|'to'){
    const from=side==='from'?port:fromChoice,to=side==='to'?port:toChoice??(targetPorts.length===1?targetPorts[0]:undefined);
    const reason=from&&to?canvasPortReason(document,{nodeId:fromNode,port:from},{nodeId:toNode,port:to}):'';
    const writer=side==='to'?inputWriter(toNode,port):undefined;
    const presentation=portPresentation(port);
    return{value:port.key,...presentation,description:`${presentation.description}${port.companionPage?' · selected panel':port.current?'':' · saved / unavailable'}`,reason:reason|| (writer?'Already connected. Select to replace its connection.':''),rank:reason?2:writer?1:0};
  }
  const sourceOptions=$derived(sourcePorts.map(port=>portOption(port,'from')).sort((a,b)=>a.rank-b.rank)),targetOptions=$derived(targetPorts.map(port=>portOption(port,'to')).sort((a,b)=>a.rank-b.rank));
  const sourceContract=$derived(sourcePorts.find(port=>port.key===fromPort)?.contract);
  const targetAction=$derived(targetPorts.find(port=>port.key===toPort)?.contract);
  const connectionPlan=$derived.by(()=>fromChoice&&toChoice?connectionDraftPlan(graph,{nodeId:fromNode,port:fromChoice},{nodeId:toNode,port:toChoice},queuedTransforms,editingRoute||undefined,activeVehicleTabId):null);
  const connectionError=$derived(connectionPlan?.error??(!fromNode?'Choose an output box on the canvas.':!toNode?'Choose an input box on the canvas.':!fromPort||!toPort?'Choose the ports to connect.':''));
  const connectionWarning=$derived(fromChoice&&toChoice?connectionError:'');
  const transformOptions=$derived(sourceContract&&targetAction?connectionTransformOptions(queuedTransforms.at(-1)?.outputs[0]?.contract??sourceContract,targetAction):[]);
  function connectionTransformOptions(input:DeepReadonly<ProfilePortContract>,target:DeepReadonly<ProfilePortContract>){return PROCESSING_PRESETS.flatMap(([value,label])=>{try{const node=createConnectionTransform(graph,input,target,value,queuedTransforms);return[{value,label,rank:graphContractError(node.outputs[0].contract,target)?1:0}];}catch{return[];}}).sort((a,b)=>a.rank-b.rank);}
  const selectedEdge=$derived(visibleDocument.bindings.find(binding=>binding.id===selectedEdgeId));
  const insertOptions=$derived(PROCESSING_PRESETS.filter(([kind])=>{if(!selectedEdge)return false;try{processingInsertion(selectedEdge.id,kind);return true;}catch{return false;}}).map(([value,label])=>({value,label})));
  const actionPorts=$derived(nodes.flatMap(node=>graphPorts(document,node.id).filter(port=>port.contract.delivery==='action').map(port=>({label:`${node.name} / ${port.name}`,contract:port.contract}))));
  const assignmentResources=$derived(inventory.filter(resource=>selected?.resource?.kind!=='companion'&&selected?.resource?.kind===(resource.resourceType==='game-integration'?'game':resource.resourceType==='companion'?'companion':'device')));
  const mappingOptions=$derived.by(()=>{const ports=[...(assignmentPage?.ports??[]),...Object.values(selectedMappings)],seen=new Set<string>();return ports.filter(port=>{const key=optionKey(port);if(seen.has(key))return false;seen.add(key);return true;});});
  $effect(()=>{const id=selectedId;if(id!==assignmentFor){assignmentFor=id;policyInspectorOpen=!assignments.some(assignment=>assignment.resourceId===id);assignmentSource='';assignmentPage=null;selectedMappings={};assignmentError='';assignmentBusy=false;assignmentEpoch++;assignmentBrowser.clear();}});
  let previousVehicleTab=untrack(()=>activeVehicleTabId);
  $effect(()=>{if(previousVehicleTab!==activeVehicleTabId){previousVehicleTab=activeVehicleTabId;queuedTransforms=[];draftRowOpen=false;laneError=null;routeOpen=false;fromNode='';fromPort='';toNode='';toPort='';editingRoute='';connectionRoute='';pendingConnection=false;highlighted=[];message='';}});
  $effect(()=>{
    const sources=assignments.filter(assignment=>visibleIds.has(assignment.resourceId)).map(assignment=>({sourceId:assignment.sourceId,resource:inventory.find(resource=>resource.id===assignment.sourceId),companion:document.resources.some(resource=>resource.id===assignment.resourceId&&resource.kind==='companion'),publication:document.companionAssignments.find(item=>item.resourceId===assignment.resourceId)?.publicationHash})),revision=inventoryRevision;
    untrack(()=>refreshDiscoveries(sources,revision));
  });
  $effect(()=>{if(!routeOpen||!autoCommitArmed||!active||readOnly)return;void sourcePorts;void targetPorts;void fromPort;void toPort;void queuedTransforms;untrack(autoConnect);});
  onDestroy(()=>{discoveryDisposed=true;assignmentEpoch++;assignmentBrowser.clear();});
  function refreshDiscoveries(sources:readonly {sourceId:string;resource?:DeepReadonly<ResourceSummary>;companion:boolean;publication?:string}[],revision:number){
    const wanted=new Set(sources.filter(source=>!source.companion||source.publication).map(source=>source.sourceId));
    if(Object.keys(discoveries).some(id=>!wanted.has(id)))discoveries=Object.fromEntries(Object.entries(discoveries).filter(([id])=>wanted.has(id)));
    for(const source of sources){if(!source.resource||source.companion&&!source.publication)continue;const signature=`${revision}/${graphInventorySignature([source.resource])}/${source.publication??''}`;if(discoveries[source.sourceId]?.signature!==signature)void discover(source.sourceId,signature,source.companion?source.publication:undefined);}
  }
  async function discover(sourceId:string,signature:string,publication?:string){
    discoveries={...discoveries,[sourceId]:{signature,details:null,busy:true,error:''}};
    try{const details=publication?await profileDiscoverCompanion(companionDiscovery,sourceId,publication):await profileDiscoverResource(inventoryClient,sourceId);if(!discoveryDisposed&&discoveries[sourceId]?.signature===signature)discoveries={...discoveries,[sourceId]:{signature,details,busy:false,error:''}};}
    catch(error){if(!discoveryDisposed&&discoveries[sourceId]?.signature===signature)discoveries={...discoveries,[sourceId]:{signature,details:null,busy:false,error:error instanceof Error?error.message:String(error)}};}
  }
  function retryDiscovery(nodeId:string){const assignment=assignments.find(item=>item.resourceId===nodeId);if(assignment)void discover(assignment.sourceId,discoveries[assignment.sourceId]?.signature??'retry',document.companionAssignments.find(item=>item.resourceId===nodeId)?.publicationHash);}
  function apply(edit:()=>ProfileGraphDraft,label:string):boolean{if(readOnly)return false;message='';try{change(edit(),label);return true;}catch(error){message=error instanceof Error?error.message:String(error);return false;}}
  function anchored(position?:CanvasPoint,panelHeight=420):CanvasPoint{const width=typeof window==='undefined'?1200:window.innerWidth,height=typeof window==='undefined'?800:window.innerHeight;return{x:Math.max(12,Math.min(width-372,position?.x??width/2)),y:Math.max(12,Math.min(height-panelHeight,position?.y??height/3))};}
  function focusRoute(position?:CanvasPoint){pickerAnchor=anchored(position);sourceSearch='';targetSearch='';selectedId='';void tick().then(()=>routingElement?.querySelector<HTMLElement>('.pending-connection input[role="combobox"]')?.focus());}
  function source(id:string,position?:CanvasPoint,portId?:string){if(!routeOpen||fromNode){closeRoute();toNode='';toPort='';}fromNode=id;fromPort=portId??'';routeOpen=true;autoCommitArmed=true;message='';focusRoute(position);autoConnect();if(fromNode&&toNode)canvas?.openConnectionOverview(fromNode,toNode);}
  function destination(id:string,position?:CanvasPoint,portId?:string){if(!routeOpen||toNode){closeRoute();fromNode='';fromPort='';}toNode=id;toPort=portId??'';routeOpen=true;autoCommitArmed=true;message='';focusRoute(position);autoConnect();if(fromNode&&toNode)canvas?.openConnectionOverview(fromNode,toNode);}
  function connectBlocks(from:string,to:string,position?:CanvasPoint,fromPortId?:string,toPortId?:string){closeRoute();fromNode=from;fromPort=fromPortId??'';toNode=to;toPort=toPortId??'';routeOpen=true;autoCommitArmed=true;if(fromPortId&&toPortId)draftRowOpen=true;focusRoute(position);autoConnect();if(fromNode&&toNode)canvas?.openConnectionOverview(fromNode,toNode);}
  function closeRoute(){const wasOpen=routeOpen;selectedTransformLane='';queuedTransforms=[];draftRowOpen=false;routeOpen=false;autoCommitArmed=false;fromNode='';fromPort='';toNode='';toPort='';sourceSearch='';targetSearch='';editingRoute='';connectionRoute='';pendingConnection=false;highlighted=[];message='';selectedId='';selectedEdgeId='';wireTransforms=false;preset='identity';canvas?.closeConnectionOverview();if(wasOpen)void tick().then(()=>routingElement?.querySelector<HTMLElement>('[aria-label="Profile canvas"]')?.focus());}
  function chooseLanePort(edit:RouteEndpointEdit,key:string){if(readOnly)return;laneError=null;const port=routeEndpointChoices(graph,edit,discoveredChoices[edit.endpoint.nodeId]??[]).find(port=>port.key===key);if(!port||port.profilePortId===edit.endpoint.portId)return;const lane=signalOverview(visibleDocument).flatMap(bundle=>bundle.lanes).find(lane=>lane.id===edit.laneId),binding=document.bindings.find(binding=>lane?.bindingIds.includes(binding.id)&&(edit.side==='from'?binding.from.nodeId===edit.endpoint.nodeId&&binding.from.portId===edit.endpoint.portId:binding.to.nodeId===edit.endpoint.nodeId&&binding.to.portId===edit.endpoint.portId));if(!binding)return;editRoute(binding.id);if(edit.side==='from'){fromNode=edit.endpoint.nodeId;fromPort=key;}else{toNode=edit.endpoint.nodeId;toPort=key;}autoCommitArmed=true;autoConnect();}
  function routeKeyboard(event:KeyboardEvent){if((routeOpen||selectedEdgeId)&&event.key==='Escape'){event.preventDefault();event.stopPropagation();closeRoute();}}
  function selectedEndpoints(){const from=sourcePorts.find(port=>port.key===fromPort),to=targetPorts.find(port=>port.key===toPort);if(!from||!to)throw new Error('Select an output and an input.');return[{nodeId:fromNode,port:from},{nodeId:toNode,port:to}] as const;}
  function prepareConnection(){const[from,to]=selectedEndpoints();return canvasConnectionPlan(graph,from,to,editingRoute||undefined);}
  function connect(replaceOccupied=false){const focus=queuedTransforms.at(-1)?.id;const accepted=apply(()=>{
    const[from,to]=selectedEndpoints(),next=commitConnectionDraft(graph,from,to,queuedTransforms,editingRoute||undefined,replaceOccupied,activeVehicleTabId);
    if(queuedTransforms.length){
      const retained=new Set([...next.document.resources,...next.document.processingNodes,...next.document.virtualOutputs,...(next.document.desktopOutputs??[])].map(node=>node.id)),existing=canvasNodes(document,assignments,inventory).filter(node=>retained.has(node.id)),placed=existing.map(node=>({position:node.position})),source=existing.find(node=>node.id===fromNode),target=existing.find(node=>node.id===toNode);
      next.document.editor.positions=existing.map(node=>({nodeId:node.id,...node.position}));
      for(const node of next.document.processingNodes.filter(node=>!document.processingNodes.some(before=>before.id===node.id))){
        const point=canvasInsertionPosition(placed,{x:((source?.position.x??0)+(target?.position.x??600))/2,y:((source?.position.y??0)+(target?.position.y??0))/2},PROFILE_BLOCK_WIDTH,PROFILE_BLOCK_HEIGHT);
        next.document.editor.positions.push({nodeId:node.id,...point});placed.push({position:point});
      }
    }
    return next;
  },queuedTransforms.length?'Connect through transformations':editingRoute?'Reconnect ports':replaceOccupied?'Replace connection':'Connect ports');if(accepted){discardConnectionDraft();if(focus)void tick().then(()=>canvas?.focusConnectionTransform(focus));}return accepted;}
  function addConnectionTransform(kind:ProcessingPreset=transformOptions[0]?.value){if(readOnly||queuedTransforms.length>=32||!sourceContract||!targetAction||!kind)return;try{queuedTransforms=[...queuedTransforms,createConnectionTransform(graph,queuedTransforms.at(-1)?.outputs[0]?.contract??sourceContract,targetAction,kind,queuedTransforms)];message='';autoConnect();}catch(error){message=error instanceof Error?error.message:String(error);}}
  function changeConnectionTransform(index:number,value:string){if(readOnly||!sourceContract||!targetAction)return;try{const previous=queuedTransforms[index],node=createConnectionTransform(graph,index?queuedTransforms[index-1].outputs[0].contract:sourceContract,targetAction,value as ProcessingPreset,queuedTransforms);queuedTransforms=queuedTransforms.map((item,at)=>at===index?{...node,id:previous.id}:item);message='';autoConnect();}catch(error){message=error instanceof Error?error.message:String(error);}}
  function removeConnectionTransform(id:string){if(readOnly)return;canvas?.focusConnectionTransform('');queuedTransforms=queuedTransforms.filter(node=>node.id!==id);message='';autoConnect();}
  function startConnectionDraft(fromId=fromNode,toId=toNode){if(readOnly)return;routeOpen=true;fromPort=fromId===fromNode?fromPort:'';toPort=toId===toNode?toPort:'';fromNode=fromId;toNode=toId;editingRoute='';autoCommitArmed=true;draftRowOpen=true;queuedTransforms=[];message='';autoConnect();void tick().then(()=>routingElement?.querySelector<HTMLElement>('.pending-connection input')?.focus());}
  function discardConnectionDraft(){canvas?.focusConnectionTransform('');selectedTransformLane='';draftRowOpen=false;queuedTransforms=[];editingRoute='';fromPort='';toPort='';message='';}
  function selectOverviewTransform(laneId:string,id:string){selectedTransformLane=laneId;return id;}
  function transformRow(id:string,laneId=selectedTransformLane){const rows=signalOverview(document).flatMap(bundle=>bundle.lanes).filter(lane=>lane.transforms.includes(id));return rows.find(lane=>lane.id===laneId)??rows[0];}
  function instanceContext(id:string,laneId=selectedTransformLane){const lane=transformRow(id,laneId);return lane?instanceRowTransforms(graph,lane.id,activeVehicleTabId):{draft:graph,nodeIds:{} as Record<string,string>,bindingIds:{} as Record<string,string>};}
  function removeOverviewTransform(id:string,laneId?:string){if(queuedTransforms.some(node=>node.id===id)){removeConnectionTransform(id);return;}apply(()=>{const local=instanceContext(id,laneId);return graphBypassProcessing(local.draft,local.nodeIds[id]??id);},'Remove transformation');}
  function existingTransformOptions(id:string){const lane=transformRow(id);return chainTransformOptions(lane?{...graph,document:{...document,bindings:document.bindings.filter(binding=>binding.from.nodeId!==id||lane.bindingIds.includes(binding.id))}}:graph,id);}
  function changeOverviewTransform(id:string,preset:ProcessingPreset){let mapped=id;const accepted=apply(()=>{const local=instanceContext(id);mapped=local.nodeIds[id]??id;return replaceChainTransform(local.draft,mapped,preset);},'Change transformation type');if(accepted&&mapped!==id)void tick().then(()=>canvas?.focusConnectionTransform(mapped));return accepted;}
  function choosePort(side:'from'|'to',key:string){if(readOnly)return;if(side==='from')fromPort=key;else toPort=key;autoCommitArmed=true;autoConnect();}
  function autoConnect(){if(!active||!routeOpen||!autoCommitArmed||readOnly||!fromNode||!toNode)return;
    if(!fromPort){const choices=sourcePorts.filter(port=>!isProcessingDraft(document,fromNode)&&(!toChoice||!canvasPortReason(document,{nodeId:fromNode,port},{nodeId:toNode,port:toChoice})));if(choices.length===1)fromPort=choices[0].key;}
    if(!toPort){const choices=targetPorts.filter(port=>!inputWriter(toNode,port)&&(!fromChoice||!canvasPortReason(document,{nodeId:fromNode,port:fromChoice},{nodeId:toNode,port})));if(choices.length===1)toPort=choices[0].key;}
    commitReadyConnection();
  }
  function commitReadyConnection(){if(!active||readOnly||!draftRowOpen||!autoCommitArmed||!fromNode||!toNode||!fromPort||!toPort)return false;try{const[from,to]=selectedEndpoints(),plan=connectionDraftPlan(graph,from,to,queuedTransforms,editingRoute||undefined,activeVehicleTabId);if(plan.error)return false;return connect(!!plan.writer);}catch{return false;}}
  function editRoute(id:string){const binding=visibleDocument.bindings.find(binding=>binding.id===id);if(!binding)return;queuedTransforms=[];draftRowOpen=true;editingRoute=id;fromNode=binding.from.nodeId;fromPort=binding.from.portId;toNode=binding.to.nodeId;toPort=binding.to.portId;routeOpen=true;autoCommitArmed=false;highlighted=[id];focusRoute(wireAnchor);const overview=signalOverview(visibleDocument).find(bundle=>bundle.lanes.some(lane=>lane.bindingIds.includes(id)));canvas?.openConnectionOverview(overview?.binding.from.nodeId??fromNode,overview?.binding.to.nodeId??toNode);}
  function selectConnection(id:string){closeRoute();if(!id)return;selectedEdgeId=id;highlighted=[id];const path=Array.from(routingElement?.querySelectorAll<SVGPathElement>('.signal-edge')??[]).find(item=>item.dataset.bindingId===id);if(path){const point=path.getPointAtLength(path.getTotalLength()/2),matrix=path.getScreenCTM(),mid=matrix?{x:matrix.a*point.x+matrix.c*point.y+matrix.e,y:matrix.b*point.x+matrix.d*point.y+matrix.f}:anchored();const binding=visibleDocument.bindings.find(item=>item.id===id),handles=Array.from(routingElement.querySelectorAll<HTMLElement>('[data-connection-source],[data-connection-target]')).filter(handle=>handle.dataset.connectionSource===binding?.from.nodeId||handle.dataset.connectionTarget===binding?.to.nodeId).map(handle=>handle.getBoundingClientRect());const candidates=[{x:mid.x-175,y:mid.y-250},{x:mid.x-175,y:mid.y+36},{x:mid.x+36,y:mid.y-120},{x:mid.x-386,y:mid.y-120}].map(position=>anchored(position,240)),overlap=(position:CanvasPoint)=>handles.reduce((score,rect)=>score+Math.max(0,Math.min(position.x+350,rect.right+12)-Math.max(position.x,rect.left-12))*Math.max(0,Math.min(position.y+220,rect.bottom+12)-Math.max(position.y,rect.top-12)),0);wireAnchor=candidates.sort((a,b)=>overlap(a)-overlap(b))[0];}else wireAnchor=anchored(undefined,240);}
  function reconnectConnection(id:string,side:'from'|'to',nodeId:string,position?:CanvasPoint,portId?:string){editRoute(id);if(!routeOpen)return;if(side==='from'){fromNode=nodeId;fromPort=portId??'';}else{toNode=nodeId;toPort=portId??'';}autoCommitArmed=true;focusRoute(position);autoConnect();if(fromNode&&toNode)canvas?.openConnectionOverview(fromNode,toNode);}
  function reconnectFromCanvas(side:'from'|'to'){if(!selectedEdge)return;const id=selectedEdge.id;editRoute(id);if(side==='from'){fromNode='';fromPort='';}else{toNode='';toPort='';}autoCommitArmed=true;}
  function disconnectConnection(id:string){if(apply(()=>graphDisconnect(graph,id),'Disconnect ports'))closeRoute();}
  function insertOnEdge(bindingId:string,kind:ProcessingPreset,position?:CanvasPoint,keepOverview=false,laneId=bindingId){let id='',lane='';if(apply(()=>{const row=signalOverview(document).flatMap(bundle=>bundle.lanes).find(row=>row.id===laneId||row.bindingIds.includes(bindingId)),local=row?instanceRowTransforms(graph,row.id,activeVehicleTabId):{draft:graph,nodeIds:{},bindingIds:{} as Record<string,string>},insertion=processingInsertion(local.bindingIds[bindingId]??bindingId,kind,local.draft);id=insertion.node.id;lane=signalOverview(insertion.next.document).flatMap(bundle=>bundle.lanes).find(row=>row.transforms.includes(id))?.id??laneId;return position?placeComponent(insertion.next,id,canvasInsertionPosition(nodes,position,PROFILE_BLOCK_WIDTH,PROFILE_BLOCK_HEIGHT)):insertion.next;},'Insert transformation')){if(keepOverview){discardConnectionDraft();selectedTransformLane=lane;void tick().then(()=>canvas?.focusConnectionTransform(id));}else{closeRoute();selectedId=id;}}}
  function dropOnConnection(component:ProfilePaletteComponent,bindingId:string,position:CanvasPoint){if(component.kind==='transformation')insertOnEdge(bindingId,component.preset,position);}
  function selectPort(nodeId:string,port:DeepReadonly<ObservedResourcePort>){if(readOnly)return;const choice=(discoveredChoices[nodeId]??[]).find(choice=>choice.observed?.descriptor.id===port.descriptor.id&&choice.observed.contractHash===port.contractHash);if(!choice){message='Refresh this resource before choosing its port.';return;}if(port.descriptor.direction==='output')source(nodeId,undefined,choice.key);else destination(nodeId,undefined,choice.key);}
  function placeComponent(next:ProfileGraphDraft,id:string,position?:CanvasPoint):ProfileGraphDraft{return{...next,document:profilePlaceComponent(document,next.document,id,position,activeVehicleTabId)};}
  export function addGroup(){return canvas?.addGroup();}
  export function addTransformation(kind:ProcessingPreset,position?:CanvasPoint){let id='';if(apply(()=>{const next=graphDropProcessing(graph,kind,kind==='scale'?{factor:10,offset:0}:{},activeVehicleTabId);id=next.document.processingNodes.at(-1)!.id;return placeComponent(next,id,position);},'Add transformation')){closeRoute();selectedId=id;}}
  export function focusGroup(id:string){canvas?.focusGroup(id);}
  export function renameGroup(id:string,name:string){canvas?.renameGroup(id,name);}
  export function deleteGroup(id:string){canvas?.deleteGroup(id);}
  export function addVirtual(position?:CanvasPoint){const id=graphFreshId(document,'virtual');apply(()=>{if(!profilePaletteCanAdd({kind:'virtual'},document,assignments,inventory,activeVehicleTabId))throw new Error('A profile supports at most eight active virtual controllers.');return placeComponent(graphSetVirtual(graph,{id,name:'Virtual controller',required:false,layout:{buttons:['button-1','button-2','button-3','button-4'],axes:[],hats:[]}},activeVehicleTabId),id,position);},'Add virtual controller');}
  export function addDesktop(position?:CanvasPoint){if(!profilePaletteCanAdd({kind:'desktop'},document,assignments,inventory,activeVehicleTabId))return;const id=graphFreshId(document,'desktop');apply(()=>placeComponent(graphSetDesktop(graph,{id,name:'Keyboard and mouse',required:false,scope:{kind:'selected-game'},ports:[],actions:[{portId:'action-1',action:{kind:'key-hold',key:44}}]},activeVehicleTabId),id,position),'Add keyboard and mouse endpoint');}
  function updateDesktop(output:ProfileDesktopOutput){apply(()=>graphSetDesktop(graph,output),'Edit typed desktop actions');}
  function processingInsertion(bindingId:string|null,kind:ProcessingPreset,base:DeepReadonly<ProfileGraphDraft>=graph){
    const prepared=bindingId?null:prepareConnection(),working=prepared?.draft??base;
    const binding=bindingId?working.document.bindings.find(binding=>binding.id===bindingId):{id:'',from:prepared!.from,to:prepared!.to};
    if(!binding)throw new Error('Choose an existing connection.');
    const input=graphPort(working.document,binding.from)?.contract,target=graphPort(working.document,binding.to)?.contract;
    if(!input||!target)throw new Error('This connection no longer has both declared ports.');
    const node=processingTemplate(kind,graphFreshId(working.document,'transform'),input,target);
    const from=node.inputs.find(port=>port.id==='value'&&!graphContractError(input,port.contract))??node.inputs.find(port=>!graphContractError(input,port.contract)),to=node.outputs.find(port=>!graphContractError(port.contract,target));
    if(!from||!to)throw new Error('This transformation cannot preserve the connection’s input and output contracts.');
    const next=binding.id?graphInsertProcessing(working,binding.id,node,from.id,to.id):graphConnect(graphConnect(graphSetProcessing(working,node,activeVehicleTabId),binding.from,{nodeId:node.id,portId:from.id}),{nodeId:node.id,portId:to.id},binding.to);
    const source=nodes.find(item=>item.id===binding.from.nodeId),destination=nodes.find(item=>item.id===binding.to.nodeId);
    if(source&&destination){
      let position={x:(source.position.x+destination.position.x)/2,y:(source.position.y+destination.position.y)/2};
      if(nodes.some(item=>position.x<item.position.x+PROFILE_BLOCK_WIDTH+48&&position.x+PROFILE_BLOCK_WIDTH+48>item.position.x&&position.y<item.position.y+PROFILE_BLOCK_HEIGHT+48&&position.y+PROFILE_BLOCK_HEIGHT+48>item.position.y))position.y=Math.max(...nodes.map(item=>item.position.y+PROFILE_BLOCK_HEIGHT))+80;
      // Freeze implicit positions before adding a positioned block; otherwise their
      // automatic fallback moves beneath it and can hide one of the new routes.
      next.document=profilePlaceComponent(document,next.document,node.id,position,activeVehicleTabId);
    }
    return{node,next};
  }
  function openConnection(id:string,laneId?:string){if(!readOnly)insertOnEdge(id,'identity',undefined,true,laneId??id);}
  function openPendingConnection(){draftRowOpen=true;addConnectionTransform();}
  function closeConnection(){connectionRoute='';pendingConnection=false;void tick().then(()=>{if(connectionTrigger?.isConnected&&(connectionTrigger instanceof HTMLElement||connectionTrigger instanceof SVGElement))connectionTrigger.focus();else routingElement?.querySelector<HTMLElement>('[aria-label="Profile canvas"]')?.focus();});}
  function showConnection(node:HTMLDialogElement){node.showModal();return()=>node.close();}
  function insertTransformation(){draftRowOpen=true;addConnectionTransform(preset);}
  function actionPack(mode:'action-pack'|'set-on-change'){if(readOnly||targetAction?.delivery!=='action'||queuedTransforms.length>=32)return;const input=sourceContract,node=processingActionPack(graphFreshId({...document,processingNodes:[...document.processingNodes,...queuedTransforms]},'transform'),targetAction,mode,input?.delivery==='event'?input:undefined);queuedTransforms=[...queuedTransforms,node];autoConnect();}
  function updateProcessing(node:ProfileProcessingNode){if(readOnly)return false;if(queuedTransforms.some(item=>item.id===node.id)){try{const next=processingDerivedNode(node),error=processingNodeError(next);if(error)throw Error(error);queuedTransforms=queuedTransforms.map(item=>item.id===node.id?next:item);message='';autoConnect();return true;}catch(error){message=error instanceof Error?error.message:String(error);return false;}}let mapped=node.id;const accepted=apply(()=>{const local=instanceContext(node.id);mapped=local.nodeIds[node.id]??node.id;return graphUpdateProcessing(local.draft,{...node,id:mapped});},'Edit transformation');if(accepted&&mapped!==node.id)void tick().then(()=>canvas?.focusConnectionTransform(mapped));return accepted;}
  function configureDraft(id:string,kind:ProcessingPreset,parameters:Record<string,unknown>={}){return apply(()=>updateProcessingDraft(graph,id,kind,parameters),'Configure transformation');}
  function quickFactor(node:DeepReadonly<ProfileProcessingNode>,input:HTMLInputElement){if(node.operation.kind!=='scale')return;if(!updateProcessing({...copy(node) as ProfileProcessingNode,operation:{...node.operation,factor:Number(input.value)}}))input.value=String(node.operation.factor);}
  function updateVirtual(output:Omit<ProfileVirtualOutput,'ports'>){apply(()=>graphSetVirtual(graph,output),'Edit virtual controller layout');}
  function removeNode(id:string){apply(()=>{const next=graphRemoveNode(graph,id);if(document.resources.some(resource=>resource.id===id&&resource.kind==='game')&&!next.document.resources.some(resource=>resource.kind==='game'))next.document.eligibility.gameIds=[];return next;},'Remove block and its routes');selectedId='';}
  function optionKey(port:DeepReadonly<ObservedResourcePort>):string{return JSON.stringify([port.descriptor.id,port.contractHash]);}
  function chooseMapping(portId:string,key:string){const observed=mappingOptions.find(port=>optionKey(port)===key);const next={...selectedMappings};if(observed)next[portId]=observed;else delete next[portId];selectedMappings=next;}
  async function browseAssignment(operation:'open'|'next'|'previous'){
    if(selected?.resource?.kind==='companion')return;
    const epoch=++assignmentEpoch,resourceId=assignmentSource;assignmentBusy=true;assignmentError='';if(operation==='open'){assignmentPage=null;selectedMappings={};}
    try{const page=operation==='open'?await assignmentBrowser.open(resourceId):operation==='next'?await assignmentBrowser.next():await assignmentBrowser.previous();if(epoch===assignmentEpoch&&page)assignmentPage=page;}
    catch(error){if(epoch===assignmentEpoch)assignmentError=error instanceof Error?error.message:String(error);}finally{if(epoch===assignmentEpoch)assignmentBusy=false;}
  }
  function assignResource(){const source=inventory.find(resource=>resource.id===assignmentSource),resource=selected?.resource;if(!source||!resource||resource.kind==='companion')return;apply(()=>graphAssignResource(graph,resource.id,source,resource.ports.flatMap(port=>selectedMappings[port.id]?[{profilePortId:port.id,observed:selectedMappings[port.id]}]:[])),'Assign exact resource and ports');}
  function resourcePolicy(node:CanvasNode,key:'name'|'required',value:string|boolean){apply(()=>{const next=copy(graph) as ProfileGraphDraft,resource=next.document.resources.find(resource=>resource.id===node.id)!;if(key==='name'&&typeof value==='string')resource.name=value;if(key==='required'&&typeof value==='boolean')resource.required=value;return next;},'Edit resource policy');}
  function publicationPolicy(node:CanvasNode,value:string){apply(()=>{if(!['configured','enabled','disabled'].includes(value))throw new Error('Choose an explicit controller publication policy.');const capability=graphPublicationCapability(node.observation);if(value!=='configured'&&capability.status==='unsupported')throw new Error(capability.message);return graphSetRuntimeOverride(graph,node.id,value==='configured'?null:value==='enabled');},'Edit controller publication policy');}
</script>

<svelte:window onkeydown={routeKeyboard}/>

<section class="routing-workspace" aria-label="Explicit profile routing editor" bind:this={routingElement}>
  {#if message}<p class="routing-error" role="alert">{message}</p>{/if}
  {#snippet draftConnection()}
    <div class="draft-endpoint">
      <ProfilePortCombobox compact label="Find an output" placeholder="Choose an output" options={sourceOptions} value={fromPort} onselect={key=>choosePort('from',key)} disabled={readOnly}/>
      {#if discoveryBusy.includes(fromNode)}<p role="status">Discovering outputs…</p>{/if}
      {#if discoveryErrors[fromNode]}<p role="alert">{discoveryErrors[fromNode]}</p><button class="text-button" onclick={()=>retryDiscovery(fromNode)}><Icon name="refresh" size={14}/>Retry</button>{/if}
    </div>
    <div class="draft-chain">
      <div class="draft-transforms">{#each queuedTransforms as node (node.id)}
        <div class="draft-transform" data-transform-id={node.id}>
          <button class="transform-chip" onclick={()=>canvas?.focusConnectionTransform(node.id)}><strong>{PROCESSING_PRESETS.find(([kind])=>kind===node.operation.kind)?.[1]??node.name}</strong><small>{processingSummary(node)}</small></button>
          <button class="remove-transform" aria-label={`Remove ${node.name}`} disabled={readOnly} onclick={()=>removeConnectionTransform(node.id)}><Icon name="trash" size={12}/></button>
        </div>
      {/each}</div>
      <ProfilePortCombobox compact label="Add transform" placeholder="Choose a transform" options={transformOptions} value="" onselect={kind=>addConnectionTransform(kind as ProcessingPreset)} disabled={readOnly||!sourceContract||!targetAction||queuedTransforms.length>=32}/>
      {#if targetAction?.delivery==='action'}<div class="draft-actions"><button class="secondary-button" disabled={readOnly} onclick={()=>actionPack('action-pack')}><Icon name="plus" size={14}/>Arguments</button>{#if targetAction.semantics==='setter'}<button class="secondary-button" disabled={readOnly} onclick={()=>actionPack('set-on-change')}><Icon name="plus" size={14}/>Set on change</button>{/if}</div>{/if}
    </div>
    <div class="draft-endpoint">
      <ProfilePortCombobox compact label="Find an input" placeholder="Choose an input" options={targetOptions} value={toPort} onselect={key=>choosePort('to',key)} disabled={readOnly}/>
      {#if discoveryBusy.includes(toNode)}<p role="status">Discovering inputs…</p>{/if}
      {#if discoveryErrors[toNode]}<p role="alert">{discoveryErrors[toNode]}</p><button class="text-button" onclick={()=>retryDiscovery(toNode)}><Icon name="refresh" size={14}/>Retry</button>{/if}
      {#if connectionWarning}<p class="connection-warning" role="status">{connectionWarning}</p>{/if}
    </div>
  {/snippet}
  <ProfileCanvas bind:this={canvas} companionAuthoring simpleControllerInspector connectionDraft={draftRowOpen?draftConnection:undefined} draftTransforms={queuedTransforms} addOverviewConnection={startConnectionDraft} removeOverviewTransform={removeOverviewTransform} selectOverviewTransform={selectOverviewTransform} {document} {activeVehicleTabId} {assignments} {inventory} {inventoryClient} {readOnly} bind:selectedId {openGameSettings} {paletteDrag} {canDropComponent} {dropComponent} {endPaletteDrag} change={(next,label)=>change({document:next,assignments:copy(assignments) as ProfileLocalAssignment[]},label??'Edit canvas layout')} {connectBlocks} connectionFrom={source} connectionTo={destination} {selectedEdgeId} {selectConnection} {reconnectConnection} {disconnectConnection} {dropOnConnection} connectionSourceId={routeOpen?fromNode:''} connectionTargetId={routeOpen?toNode:''} connectionSourcePort={routeOpen?fromPort:''} connectionTargetPort={routeOpen?toPort:''} reconnectingBindingId={routeOpen?editingRoute:''} selectProviderPort={selectPort} removeProviderPort={(nodeId,portId)=>apply(()=>graphRemovePort(graph,nodeId,portId),'Remove selected port and routes')} highlightedEdges={highlighted} diagnostics={[]} {undo} {redo} {discoveredPorts} {discoveryBusy} {discoveryErrors} connectionOpen={routeOpen} closeConnectionPanel={closeRoute} insertTransform={openConnection} transferTransform={(sourceLane,nodeId,targetLane,before)=>apply(()=>graphTransferProcessing(graph,sourceLane,nodeId,targetLane,before),sourceLane===targetLane?'Move transformation in chain':'Copy transformation to chain')}>
    {#snippet routeEndpoint(laneId,side,endpoint)}
      {@const edit={laneId,side,endpoint}}
      {@const choices=routeEndpointChoices(graph,edit,discoveredChoices[endpoint.nodeId]??[])}
      <ProfilePortCombobox compact label={side==='from'?'Filter source signals':'Filter destination signals'} placeholder="Type to filter signals" options={choices.map(port=>({value:port.key,...portPresentation(port)}))} value={endpoint.portId} onselect={key=>chooseLanePort(edit,key)} disabled={readOnly}/>
      {#if laneError?.laneId===laneId&&laneError.side===side}<p class="routing-error" role="alert">{laneError.message}</p>{/if}
    {/snippet}
    {#snippet transformSettings(node)}
      {@const pendingIndex=queuedTransforms.findIndex(item=>item.id===node.id)}
      {@const typeOptions=pendingIndex>=0&&sourceContract&&targetAction?connectionTransformOptions(pendingIndex?queuedTransforms[pendingIndex-1].outputs[0].contract:sourceContract,targetAction):existingTransformOptions(node.id)}
      {#if typeOptions.length}<label>Type<select aria-label="Transformation type" value={node.processing?.operation.kind} disabled={readOnly} onchange={event=>pendingIndex>=0?changeConnectionTransform(pendingIndex,event.currentTarget.value):changeOverviewTransform(node.id,event.currentTarget.value as ProcessingPreset)}>{#each typeOptions as option}<option value={option.value}>{option.label}</option>{/each}</select></label>{/if}
      {#if node.processing}<ProfileTransformSettings node={node.processing} change={updateProcessing} configure={(kind,parameters)=>configureDraft(node.id,kind,parameters)} actions={actionPorts}/>{/if}
    {/snippet}
    {#snippet nodeControls(node)}
      {#if node.processing?.operation.kind==='scale'}<label class="quick-factor">× <input aria-label="Quick multiply factor" type="number" step="any" value={node.processing.operation.factor} disabled={readOnly} onchange={event=>quickFactor(node.processing!,event.currentTarget)}/></label>{/if}
    {/snippet}
    {#snippet inspector(node)}
      {@const physicalController=node.resource?.kind==='device'&&node.observation?.resourceType==='host-controller'}
      <fieldset class="authoring-inspector" disabled={readOnly}>
        {#if physicalController&&node.resource}
          <label>Label<input value={node.resource.name} maxlength="80" onchange={event=>resourcePolicy(node,'name',event.currentTarget.value)}/></label>
          <label class="check"><input type="checkbox" checked={node.resource.required} onchange={event=>resourcePolicy(node,'required',event.currentTarget.checked)}/> Required</label>
          <label>Visibility<select aria-label="Visibility" value={document.visibility.find(intent=>intent.resourceId===node.id)?.mode??'visible'} onchange={event=>apply(()=>graphSetVisibility(graph,node.id,event.currentTarget.value==='hidden-while-active'?'hidden-while-active':'visible'),'Edit visibility intent')}><option value="visible">Visible</option><option value="hidden-while-active">Hidden while profile is active</option></select></label>
        {:else}
        {#if node.resource}<details bind:open={policyInspectorOpen}><summary>Resource assignment and policy</summary><label>Block label<input value={node.resource.name} maxlength="80" onchange={event=>resourcePolicy(node,'name',event.currentTarget.value)}/></label><label class="check"><input type="checkbox" checked={node.resource.required} onchange={event=>resourcePolicy(node,'required',event.currentTarget.checked)}/> Required resource</label>
          {#if node.resource.kind==='device'}<label>Game visibility intent<select value={document.visibility.find(intent=>intent.resourceId===node.id)?.mode??'visible'} onchange={event=>apply(()=>graphSetVisibility(graph,node.id,event.currentTarget.value==='hidden-while-active'?'hidden-while-active':'visible'),'Edit visibility intent')}><option value="visible">Visible</option><option value="hidden-while-active">Hidden while profile is active</option></select></label>{/if}
          {#if node.resource.kind==='device'}{@const publication=graphPublicationCapability(node.observation)}<label>Controller publication policy<select value={document.overrides.some(policy=>policy.resourceId===node.id)?document.overrides.find(policy=>policy.resourceId===node.id)!.enabled?'enabled':'disabled':'configured'} onchange={event=>publicationPolicy(node,event.currentTarget.value)}><option value="configured">Keep configured</option><option value="enabled" disabled={publication.status==='unsupported'}>Enable while active</option><option value="disabled" disabled={publication.status==='unsupported'}>Disable while active</option></select></label><p class="publication-status" role="status">{publication.message}{#if publication.configuredEnabled!==null} Configured controller publication: {publication.configuredEnabled?'enabled':'disabled'}.{/if}</p><p class="publication-warning">Enabling or disabling publication can change USB controller collections, require a USB reconnect and require game rebinding. This policy does not save device flash. Controller publication and game hiding are separate.</p>{/if}
          {#if node.resource.kind!=='companion'}<p>{node.assignment?'Assigned locally; replacing it requires an exact match for every selected port.':'Unassigned. Choose a matching inventory resource; copied graphs are never resolved by name.'}</p><label>Inventory resource<select value={assignmentSource} onchange={event=>{assignmentSource=event.currentTarget.value;if(assignmentSource)void browseAssignment('open');}}><option value="">Choose resource explicitly</option>{#each assignmentResources as resource (resource.id)}<option value={resource.id}>{resource.name} · {resource.availability}</option>{/each}</select></label>
          {#if assignmentBusy}<p role="status">Loading provider port page…</p>{/if}{#if assignmentError}<p class="routing-error" role="status">{assignmentError}</p>{/if}
          {#if assignmentSource}{#each node.resource.ports as port (port.id)}<label>Logical {port.id} → provider port<select value={selectedMappings[port.id]?optionKey(selectedMappings[port.id]):''} onchange={event=>chooseMapping(port.id,event.currentTarget.value)}><option value="">Choose exact matching port</option>{#each mappingOptions.filter(observed=>observed.descriptor.direction===port.direction) as observed (optionKey(observed))}<option value={optionKey(observed)}>{observed.descriptor.name} · {observed.descriptor.id} {observed.current?'':'(cached)'}</option>{/each}</select><small>{portContractLabel(port.contract)}</small></label>{/each}{#if assignmentPage}<p>{assignmentPage.offset+1}–{assignmentPage.offset+assignmentPage.ports.length} of {assignmentPage.total} cached contracts</p><div class="small-actions"><button class="text-button" disabled={assignmentBusy||!assignmentBrowser.hasPrevious} onclick={()=>browseAssignment('previous')}>Previous ports</button><button class="text-button" disabled={assignmentBusy||!assignmentBrowser.hasNext} onclick={()=>browseAssignment('next')}>Next ports</button></div>{/if}<button class="secondary-button" disabled={node.resource.ports.some(port=>!selectedMappings[port.id])||assignmentBusy} onclick={assignResource}>Assign resource and exact ports</button>{/if}
          {/if}
        </details>{/if}
        {#if node.processing}<ProfileTransformSettings node={node.processing} change={updateProcessing} configure={(kind,parameters)=>configureDraft(node.id,kind,parameters)} actions={actionPorts}/>{#if node.processing.inputs.length===1&&node.processing.outputs.length===1}<button class="text-button" onclick={()=>apply(()=>graphBypassProcessing(graph,node.id),'Remove transformation and join compatible route')}>Remove transformation and join compatible route</button>{/if}{/if}
        {#if node.virtual}<ProfileVirtualEditor output={node.virtual} change={updateVirtual}/>{/if}
        {#if node.desktop}<ProfileDesktopEditor output={node.desktop} change={updateDesktop}/>{/if}
        <p class="remove-help">Removing this block also removes {document.bindings.filter(binding=>binding.from.nodeId===node.id||binding.to.nodeId===node.id).length} exact routes.</p><button class="text-button" onclick={()=>removeNode(node.id)}>Remove block and its routes</button>
        {/if}
      </fieldset>
      {#if node.resource?.kind==='companion'}<ProfileCompanionEditor {graph} {inventory} resourceId={node.id} {readOnly} {active} client={companionDiscovery} {change}/>{/if}
      {#if physicalController}
        <ProfileControllerInputs resource={node.observation} {active}/>
        <footer class="controller-actions"><button class="secondary-button destructive-button" disabled={readOnly} onclick={()=>removeNode(node.id)}><Icon name="trash" size={14}/>Remove</button></footer>
      {/if}
    {/snippet}
  </ProfileCanvas>
  {#if selectedEdge}
    <section class="wire-actions" aria-label="Connection actions" style={`left:${wireAnchor.x}px;top:${wireAnchor.y}px`}>
      <div class="route-heading"><h3>Connection</h3><button class="text-button" aria-label="Close connection actions" onclick={closeRoute}><Icon name="close" size={16}/></button></div>
      <p>{nodes.find(node=>node.id===selectedEdge!.from.nodeId)?.name} · {selectedEdge.from.portId} → {nodes.find(node=>node.id===selectedEdge!.to.nodeId)?.name} · {selectedEdge.to.portId}</p>
      <div class="small-actions"><button class="text-button" disabled={readOnly} onclick={()=>editRoute(selectedEdge!.id)}>Change ports</button><button class="text-button" disabled={readOnly} onclick={()=>reconnectFromCanvas('from')}>Reconnect source</button><button class="text-button" disabled={readOnly} onclick={()=>reconnectFromCanvas('to')}>Reconnect input</button><button class="text-button" disabled={readOnly} onclick={()=>wireTransforms=!wireTransforms}>Insert transformation</button><button class="text-button" disabled={readOnly} onclick={()=>disconnectConnection(selectedEdge!.id)}>Disconnect</button></div>
      {#if wireTransforms}<ProfilePortCombobox label="Insert transformation" placeholder="Search transformations" options={insertOptions} disabled={readOnly} onselect={kind=>insertOnEdge(selectedEdge!.id,kind as ProcessingPreset)}/>{#if !insertOptions.length}<p>No compatible transformation preserves both contracts.</p>{/if}{/if}
    </section>
  {/if}

</section>
<style>
  .wire-actions{position:fixed;z-index:35;width:350px;max-width:calc(100vw - 24px);box-sizing:border-box;border:1px solid var(--line);border-radius:8px;background:var(--surface);padding:16px;box-shadow:0 12px 36px #0006}
  .wire-actions h3{margin:0}.wire-actions .small-actions{gap:10px}.wire-actions p{overflow-wrap:anywhere}
  .quick-factor{display:flex;align-items:center;gap:6px;margin:0;color:var(--accent);font-size:var(--ui-text-size)}.quick-factor input{width:70px!important;padding:3px 5px!important}
  .routing-workspace button{font-size:var(--ui-action-size)}
  .routing-workspace select,.routing-workspace input:not([type="checkbox"]){background:var(--input-bg);color:var(--text);border:1px solid var(--line);border-radius:5px;padding:8px;font-size:var(--ui-text-size);min-width:0;width:100%;box-sizing:border-box;min-height:var(--ui-control-height)}
  .route-heading{display:flex;align-items:center;justify-content:space-between;gap:12px}h3{font-size:var(--ui-heading-size);color:var(--soft)}
  p{font-size:var(--ui-text-size);color:var(--muted);line-height:var(--ui-line-height);margin:10px 0}label{font-size:var(--ui-text-size);color:var(--muted);display:grid;gap:7px}
  .routing-error{color:#d3a078;padding:12px;border:1px solid #ad875440;border-radius:6px;font-size:var(--ui-text-size);line-height:var(--ui-line-height)}
  .authoring-inspector{border:0;margin:0;padding:0;min-width:0}.authoring-inspector label{margin:12px 0}.authoring-inspector details{margin:16px 0;border-top:1px solid var(--line);padding-top:14px}.authoring-inspector summary{font-size:var(--ui-text-size);color:var(--soft);cursor:pointer}.authoring-inspector small{font-size:var(--ui-text-size);line-height:var(--ui-line-height)}
  .check{display:flex;align-items:center;gap:8px}.small-actions{display:flex;gap:12px;align-items:center;flex-wrap:wrap}
  .controller-actions{display:flex;justify-content:flex-end;flex-shrink:0;padding-top:12px;border-top:1px solid var(--line)}.controller-actions .destructive-button{background:#b32b38;border-color:#cd4552;color:#fff}.controller-actions .destructive-button:hover:enabled{background:#98232e;border-color:#e35d69}
  .draft-endpoint,.draft-chain{min-width:0;display:grid;gap:8px;align-content:start}.draft-actions{display:flex;gap:8px;flex-wrap:wrap}.connection-warning{margin:0;font-size:11px;color:#d3a078}
  .draft-transforms{display:flex;align-items:center;gap:6px;flex-wrap:wrap}.draft-transform{position:relative;max-width:100%}
  .transform-chip{box-sizing:border-box;width:112px;max-width:100%;min-height:38px;padding:4px 6px;border:1px solid color-mix(in srgb,#9b80d8 40%,var(--line));border-radius:5px;background:color-mix(in srgb,#9b80d8 14%,var(--surface));color:var(--text);display:flex;flex-direction:column;gap:2px;text-align:left;cursor:pointer}
  .transform-chip strong,.transform-chip small{width:100%;overflow:hidden;text-overflow:ellipsis;white-space:nowrap}.transform-chip strong{font-size:11px;line-height:12px}.transform-chip small{font-size:9px;line-height:11px;color:var(--soft)}.transform-chip:hover{border-color:#bfa1ff}
  .remove-transform{position:absolute;right:2px;top:2px;display:grid;place-items:center;width:22px;height:22px;padding:0;border:1px solid #cd4552;border-radius:4px;background:#b32b38;color:#fff;opacity:0;pointer-events:none}
  .draft-transform:hover .remove-transform,.draft-transform:focus-within .remove-transform{opacity:1;pointer-events:auto}.remove-transform:disabled{opacity:.4;cursor:default}@media(hover:none){.remove-transform{opacity:1;pointer-events:auto}}
</style>
