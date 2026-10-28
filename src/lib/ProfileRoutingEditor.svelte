<script lang="ts">
  import{onDestroy,tick,untrack}from'svelte';
  import {routeEndpointChoices,reconnectRouteEndpoint,type RouteEndpointEdit} from './profile-route-endpoints';
  import {signalOverview,destinationOverview,type SignalLane}from'./profile-signal-overview';
  import {chainTransformOptions,replaceChainTransform,instanceRowTransforms,addChainInput,removeChainInput}from'./profile-chain-edit';
  import ProfileCanvas from './ProfileCanvas.svelte';
  import ProfilePortCombobox from './ProfilePortCombobox.svelte';
  import Icon from './Icon.svelte';
  import ProfileVirtualEditor from './ProfileVirtualEditor.svelte';
  import ProfileDebugControllerEditor from './ProfileDebugControllerEditor.svelte';
  import {debugControllerResource} from './profile-debug-controller';
  import {graphSetDebugController} from './profile-debug-graph';
  import ProfileDesktopEditor from './ProfileDesktopEditor.svelte';
  import {desktopBlock,type DesktopBlockKind} from './profile-desktop-blocks';
  import type {ProfileDesktopOutput}from'./profile-desktop';
  import {graphSetDesktop}from'./profile-graph';
  import ProfileControllerInputs from './ProfileControllerInputs.svelte';
  import ProfileResourceSettings from './profile-inspector/ProfileResourceSettings.svelte';
  import ProfileIntegrationPorts from './profile-inspector/ProfileIntegrationPorts.svelte';
  import ProfileIntegrationAssignment from './profile-inspector/ProfileIntegrationAssignment.svelte';
  import {profileGameIntegrationSource} from './profile-game-context';
  import ProfileCompanionEditor from './ProfileCompanionEditor.svelte';
  import type{ProfileLearningClient}from'./profile-learning';
  import {canvasNodes,portContractLabel,integrationCataloguePorts,type CanvasNode,type CanvasPoint} from './profile-canvas';
  import {profilePaletteCanAdd,profilePlaceComponent,type ProfilePaletteComponent,type ProfilePaletteDrag} from './profile-palette';
  import {profileTabDocument,profileTabNodeIds} from './profile-vehicle-tabs';
  import {graphTransferProcessing,graphAssignResource,graphBypassProcessing,graphConnect,graphConnectionError,graphContractError,graphDisconnect,graphFreshId,graphInsertProcessing,graphInventorySignature,graphPort,graphPorts,graphPublicationCapability,graphRemoveNode,graphRemovePort,graphSelectProviderPort,graphSetProcessing,graphSetRuntimeOverride,graphSetVirtual,graphSetVisibility,type ProfileGraphDraft}from'./profile-graph';
  import {PROCESSING_PRESETS,processingActionPack,processingNodeError,processingTemplate,type ProcessingPreset}from'./profile-processing';
  import {ProfilePortBrowser}from'./profile-port-browser';
  import {profileDiscoverResource,profileDiscoverCompanion,ProfileDiscoveredPortCache,profilePhysicalPortPresentation,type ProfileDiscoveredPort,type ProfileDiscoveredEndpoint}from'./profile-discovery';
  import {PROFILE_BLOCK_HEIGHT,PROFILE_BLOCK_WIDTH}from'./profile-editor';
  import {canvasConnectionPlan,canvasPortReason,canvasInsertionPosition}from'./profile-canvas-construction';
  import {connectionDraftPlan,commitConnectionDraft,createConnectionTransform,connectionTransformInput,discardCommittedConnection}from'./profile-connection-draft';
  import {createProcessingDraft,isProcessingDraft,graphUpdateProcessing,processingDerivedNode,updateProcessingDraft}from'./profile-transform-drafts';
  import ProfileTransformSettings from './ProfileTransformSettings.svelte';
  import {gatewayRequest}from'./gateway';
  import {matchingGameIntegrationSources}from'./profile-game-source';
  import type{CompilationSummary,CompileDiagnostic,ProfileCompilerClient}from'./profile-compilation';
  import type{DeepReadonly,ProfileDocument,ProfileLocalAssignment,ProfilePortContract,ProfileProcessingNode,ProfileVirtualOutput,ProfileResource}from'./profiles';
  import type{ObservedResourcePort,ProfileInventoryClient,ResourceDetails,ResourceSummary}from'./profile-inventory';
  import type{Game,GameId}from'./games';
  import {ProfileCompanionAuthoringClient,type ProfileCompanionPage}from'./profile-companion-authoring';
  let{document,assignments,inventory,games=[],inventoryClient,compilerClient,learningClient,companionClient,editorContext='',activeVehicleTabId='',active=true,runtimeIdle=false,inventoryRevision=0,readOnly=false,selectedId=$bindable(''),change,openGameSettings,undo,redo,paletteDrag=null,canDropComponent,dropComponent,endPaletteDrag}:{
    document:DeepReadonly<ProfileDocument>;assignments:readonly DeepReadonly<ProfileLocalAssignment>[];inventory:readonly DeepReadonly<ResourceSummary>[];games?:readonly Game[];inventoryClient:Pick<ProfileInventoryClient,'inspect'>;compilerClient?:Pick<ProfileCompilerClient,'compile'|'inspect'>;inventoryRevision?:number;readOnly?:boolean;selectedId?:string;
    change:(draft:ProfileGraphDraft,label:string)=>void;openGameSettings?:(gameId:GameId,returnState?:unknown)=>void;undo?:()=>void;redo?:()=>void;
    companionClient?:Pick<ProfileCompanionAuthoringClient,'catalogue'|'inspect'>;learningClient?:Pick<ProfileLearningClient,'start'|'status'|'cancel'>;editorContext?:string;activeVehicleTabId?:string;active?:boolean;runtimeIdle?:boolean;
    paletteDrag?:ProfilePaletteDrag|null;canDropComponent?:(component:ProfilePaletteComponent)=>boolean;dropComponent?:(component:ProfilePaletteComponent,position:CanvasPoint)=>void;endPaletteDrag?:()=>void;
  }=$props();
  const copy=<T,>(value:T):T=>JSON.parse(JSON.stringify(value));
  let message=$state(''),routeOpen=$state(false),fromNode=$state(''),fromPort=$state(''),toNode=$state(''),toPort=$state(''),editingRoute=$state(''),highlighted=$state<string[]>([]),preset=$state<ProcessingPreset>('identity');
  let connectionRoute=$state(''),pendingConnection=$state(false);
  let queuedTransforms=$state<ProfileProcessingNode[]>([]),draftRowOpen=$state(false),selectedTransformLane=$state('');
  type IncompleteConnection={lane:SignalLane;index:number;from:ProfileDiscoveredEndpoint;to:ProfileDiscoveredEndpoint;transforms:ProfileProcessingNode[];error:string;inputs?:Record<string,ProfileDiscoveredEndpoint>;clearedSource?:boolean;clearedDestination?:boolean};
  let incompleteRows=$state<IncompleteConnection[]>([]),activeIncompleteRowId=$state(''),queuedInputs=$state<Record<string,ProfileDiscoveredEndpoint>>({});
  let clearedSource=$state(false),clearedDestination=$state(false);
  let laneError=$state<{laneId:string;side:'from'|'to';message:string}|null>(null);
  let routeErrors=$state<Record<string,string>>({});
  let sourceSearch=$state(''),targetSearch=$state('');
  let pickerAnchor=$state<CanvasPoint>({x:400,y:180});
  let selectedEdgeId=$state(''),autoCommitArmed=$state(false);
  let connectionTrigger:Element|null=null;
  let routingElement:HTMLElement;
  let canvas=$state.raw<{addGroup:(name?:string)=>string|undefined;focusGroup:(id:string)=>void;renameGroup:(id:string,name:string)=>void;deleteGroup:(id:string)=>void;openConnectionOverview:(from:string,to:string)=>void;closeConnectionOverview:()=>void;focusConnectionTransform:(id:string)=>void}>();
  const connectionUid=$props.id();
  const connectionTitleId=`${connectionUid}-connection`;
  let assignmentFor=$state(''),assignmentSource=$state(''),assignmentPage=$state.raw<DeepReadonly<ResourceDetails>|null>(null),assignmentBusy=$state(false),assignmentError=$state(''),selectedMappings=$state.raw<Record<string,DeepReadonly<ObservedResourcePort>>>({}),assignmentEpoch=0;
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
  const catalogueSources=$derived(new Map(nodes.map(node=>[node.id,node.resource?.kind==='game'?profileGameIntegrationSource(document,node.id,inventory):node.observation])));
  function catalogueSourceId(node:CanvasNode){return catalogueSources.get(node.id)?.id??node.assignment?.sourceId??'';}
  const discoveredChoices=$derived.by(()=>{
    const saved=discoveryChoices.collect(graph,nodes.map(node=>({nodeId:node.id,details:node.resource?.kind==='game'?null:discoveries[catalogueSourceId(node)]?.details})));
    return Object.fromEntries(nodes.map(node=>{const id=catalogueSourceId(node),details=discoveries[id]?.details;return[node.id,node.resource?.kind==='game'&&details&&'resource' in details?integrationCataloguePorts(graph,node.id,id,details):saved[node.id]??[]];}));
  });
  const discoveredPorts=$derived(Object.fromEntries(Object.entries(discoveredChoices).map(([id,ports])=>[id,ports.map(port=>({id:port.key,name:port.name,direction:port.direction,contract:port.contract}))])));
  const discoveryBusy=$derived(nodes.filter(node=>discoveries[catalogueSourceId(node)]?.busy).map(node=>node.id));
  const discoveryErrors=$derived(Object.fromEntries(nodes.flatMap(node=>{const error=discoveries[catalogueSourceId(node)]?.error||(node.resource?.kind==='game'&&!catalogueSources.get(node.id)?'Integration catalogue unavailable. Check the selected game and aircraft configuration.':'');return error?[[node.id,error]]:[];})));
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
  const connectionPlan=$derived.by(()=>fromChoice&&toChoice?connectionDraftPlan(graph,{nodeId:fromNode,port:fromChoice},{nodeId:toNode,port:toChoice},queuedTransforms,editingRoute||undefined,activeVehicleTabId,selectedTransformInputs()):null);
  const connectionError=$derived(connectionPlan?.error??(!fromNode?'Choose an output box on the canvas.':!toNode?'Choose an input box on the canvas.':!fromPort||!toPort?'Choose the ports to connect.':''));
  const connectionWarning=$derived(draftRowOpen&&message?message:fromChoice&&toChoice?connectionError:'');
  const transformOptions=$derived(sourceContract&&targetAction?connectionTransformOptions(queuedTransforms.at(-1)?.outputs[0]?.contract??sourceContract,targetAction):[]);
  function connectionTransformOptions(_input?:DeepReadonly<ProfilePortContract>,_target?:DeepReadonly<ProfilePortContract>){return PROCESSING_PRESETS.map(([value,label])=>({value,label}));}
  const assignmentGameId=$derived(selected?.resource?.kind==='game'?selected.observation?.attributes.gameId??document.vehicleTabs?.find(tab=>tab.nodeIds.includes(selected.id))?.gameId??document.eligibility.gameIds[0]??'':'');
  const assignmentGame=$derived(games.find(game=>game.id===assignmentGameId));
  const assignmentResources=$derived(selected?.resource?.kind==='game'?matchingGameIntegrationSources(assignmentGame,inventory):inventory.filter(resource=>selected?.resource?.kind!=='companion'&&selected?.resource?.kind===(resource.resourceType==='companion'?'companion':'device')));
  const mappingOptions=$derived.by(()=>{const ports=[...(assignmentPage?.ports??[]),...Object.values(selectedMappings)],seen=new Set<string>();return ports.filter(port=>{const key=optionKey(port);if(seen.has(key))return false;seen.add(key);return true;});});
  $effect(()=>{const id=selectedId;if(id!==assignmentFor){assignmentFor=id;assignmentSource='';assignmentPage=null;selectedMappings={};assignmentError='';assignmentBusy=false;assignmentEpoch++;assignmentBrowser.clear();}});
  let previousVehicleTab=untrack(()=>activeVehicleTabId);
  $effect(()=>{if(previousVehicleTab!==activeVehicleTabId){previousVehicleTab=activeVehicleTabId;clearedSource=false;clearedDestination=false;queuedInputs={};incompleteRows=[];activeIncompleteRowId='';queuedTransforms=[];draftRowOpen=false;laneError=null;routeErrors={};routeOpen=false;fromNode='';fromPort='';toNode='';toPort='';editingRoute='';connectionRoute='';pendingConnection=false;highlighted=[];message='';}});
  $effect(()=>{
    const sources=nodes.filter(node=>node.resource&&node.resource.kind!=='debug-controller').map(node=>({sourceId:catalogueSourceId(node),resource:catalogueSources.get(node.id),companion:node.resource?.kind==='companion',publication:document.companionAssignments.find(item=>item.resourceId===node.id)?.publicationHash})).filter(source=>!!source.sourceId),revision=inventoryRevision;
    untrack(()=>refreshDiscoveries(sources,revision));
  });
  $effect(()=>{if(!routeOpen||!autoCommitArmed||!active||readOnly)return;void sourcePorts;void targetPorts;void fromPort;void toPort;void queuedTransforms;void queuedInputs;void inputOptions;untrack(autoConnect);});
  $effect(()=>{const bindings=document.bindings,ids=new Set([...document.resources,...document.virtualOutputs,...(document.desktopOutputs??[])].map(node=>node.id));untrack(()=>{const removed=incompleteRows.filter(row=>{const target=incompletePort(row.to);return !ids.has(row.from.nodeId)||!ids.has(row.to.nodeId)||!!target?.profilePortId&&bindings.some(binding=>binding.to.nodeId===row.to.nodeId&&binding.to.portId===target.profilePortId);});if(!removed.length)return;incompleteRows=incompleteRows.filter(row=>!removed.includes(row));if(removed.some(row=>row.lane.id===activeIncompleteRowId)){activeIncompleteRowId='';draftRowOpen=false;queuedTransforms=[];queuedInputs={};}});});
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
  function retryDiscovery(nodeId:string){const node=nodes.find(node=>node.id===nodeId),id=node&&catalogueSourceId(node);if(id)void discover(id,discoveries[id]?.signature??'retry',document.companionAssignments.find(item=>item.resourceId===nodeId)?.publicationHash);}
  function apply(edit:()=>ProfileGraphDraft,label:string,laneId=selectedTransformLane||editingRoute):boolean{if(readOnly)return false;message='';try{change(edit(),label);if(laneId){const next={...routeErrors};for(const row of signalOverview(document).flatMap(bundle=>bundle.lanes).filter(row=>row.id===laneId||row.bindingIds.includes(laneId)))delete next[row.id];routeErrors=next;}return true;}catch(error){message=error instanceof Error?error.message:String(error);const rows=signalOverview(document).flatMap(bundle=>bundle.lanes),affected=rows.filter(row=>laneId?row.id===laneId||row.bindingIds.includes(laneId):row.bindingIds.some(id=>message.startsWith(`${id}:`)));routeErrors={...routeErrors,...Object.fromEntries(affected.map(row=>[row.id,message]))};return false;}}
  function routeIssue(laneId:string){const incomplete=incompleteRows.find(row=>row.lane.id===laneId);if(incomplete)return incomplete.error;if(routeErrors[laneId])return routeErrors[laneId];if(laneError?.laneId===laneId)return laneError.message;const lane=signalOverview(document).flatMap(bundle=>bundle.lanes).find(row=>row.id===laneId);for(const id of lane?.bindingIds??[]){const binding=document.bindings.find(row=>row.id===id);if(binding){const error=graphConnectionError(document,binding.from,binding.to,id);if(error)return error;}}return '';}
  function anchored(position?:CanvasPoint,panelHeight=420):CanvasPoint{const width=typeof window==='undefined'?1200:window.innerWidth,height=typeof window==='undefined'?800:window.innerHeight;return{x:Math.max(12,Math.min(width-372,position?.x??width/2)),y:Math.max(12,Math.min(height-panelHeight,position?.y??height/3))};}
  function focusRoute(position?:CanvasPoint){pickerAnchor=anchored(position);sourceSearch='';targetSearch='';selectedId='';void tick().then(()=>routingElement?.querySelector<HTMLElement>('.pending-connection input[role="combobox"]')?.focus());}
  function source(id:string,position?:CanvasPoint,portId?:string){if(!routeOpen||fromNode){closeRoute();toNode='';toPort='';}fromNode=id;fromPort=portId??'';routeOpen=true;autoCommitArmed=true;message='';focusRoute(position);autoConnect();if(fromNode&&toNode)canvas?.openConnectionOverview(fromNode,toNode);}
  function destination(id:string,position?:CanvasPoint,portId?:string){if(!routeOpen||toNode){closeRoute();fromNode='';fromPort='';}toNode=id;toPort=portId??'';routeOpen=true;autoCommitArmed=true;message='';focusRoute(position);autoConnect();if(fromNode&&toNode)canvas?.openConnectionOverview(fromNode,toNode);}
  function connectBlocks(from:string,to:string,position?:CanvasPoint,fromPortId?:string,toPortId?:string){closeRoute();fromNode=from;fromPort=fromPortId??'';toNode=to;toPort=toPortId??'';routeOpen=true;autoCommitArmed=true;if(fromPortId&&toPortId)draftRowOpen=true;focusRoute(position);autoConnect();if(fromNode&&toNode)canvas?.openConnectionOverview(fromNode,toNode);}
  function closeRoute(){rememberIncomplete();queuedInputs={};clearedSource=false;clearedDestination=false;activeIncompleteRowId='';const wasOpen=routeOpen;selectedTransformLane='';queuedTransforms=[];draftRowOpen=false;routeOpen=false;autoCommitArmed=false;fromNode='';fromPort='';toNode='';toPort='';sourceSearch='';targetSearch='';editingRoute='';connectionRoute='';pendingConnection=false;highlighted=[];message='';selectedId='';selectedEdgeId='';preset='identity';canvas?.closeConnectionOverview();if(wasOpen)void tick().then(()=>routingElement?.querySelector<HTMLElement>('[aria-label="Profile canvas"]')?.focus());}
  function chooseLanePort(edit:RouteEndpointEdit,key:string){if(readOnly)return;if(incompleteRows.some(row=>row.lane.id===edit.laneId)){activateIncomplete(edit.laneId);choosePort(edit.side,key);return;}if(!key){clearLaneEndpoint(edit);return;}laneError=null;const port=routeEndpointChoices(graph,edit,discoveredChoices[edit.endpoint.nodeId]??[]).find(port=>port.key===key);if(!port||port.profilePortId===edit.endpoint.portId)return;apply(()=>reconnectRouteEndpoint(graph,edit,port),'Change route endpoint',edit.laneId);}
  function clearLaneEndpoint(edit:RouteEndpointEdit){if(readOnly)return false;try{const local=instanceRowTransforms(graph,edit.laneId,activeVehicleTabId);return reviseLaneTransforms(local.draft,edit.laneId,nodes=>nodes,'Clear route assignment','',{},edit.side);}catch(issue){message=issue instanceof Error?issue.message:String(issue);return false;}}
  function routeKeyboard(event:KeyboardEvent){if((routeOpen||selectedEdgeId)&&event.key==='Escape'){event.preventDefault();event.stopPropagation();closeRoute();}}
  function selectedEndpoints(){const from=sourcePorts.find(port=>port.key===fromPort),to=targetPorts.find(port=>port.key===toPort);if(!from||!to)throw new Error('Select an output and an input.');return[{nodeId:fromNode,port:from},{nodeId:toNode,port:to}] as const;}
  function prepareConnection(){const[from,to]=selectedEndpoints();return canvasConnectionPlan(graph,from,to,editingRoute||undefined);}
  function reserveIncompleteRouteIds(next:ProfileGraphDraft,except=''){const reserved=incompleteRows.map(row=>row.lane.id).filter(id=>id!==except),existing=new Set(document.bindings.map(binding=>binding.id));for(const binding of next.document.bindings){if(!reserved.includes(binding.id)||existing.has(binding.id))continue;binding.id=graphFreshId({...next.document,bindings:[...next.document.bindings,...reserved.map(id=>({...binding,id}))]},'route');}}
  function preserveLaneId(next:ProfileGraphDraft,terminal:ProfileDocument['bindings'][number],id:string){const occupied=next.document.bindings.find(binding=>binding!==terminal&&binding.id===id);if(occupied)occupied.id=graphFreshId({...next.document,bindings:[...next.document.bindings,...incompleteRows.map(row=>({...terminal,id:row.lane.id}))]},'route');terminal.id=id;}
  function connect(replaceOccupied=false){const focus=queuedTransforms.at(-1)?.id,retainedRow=incompleteRows.find(row=>row.lane.id===activeIncompleteRowId);const accepted=apply(()=>{
    const[from,to]=selectedEndpoints(),next=commitConnectionDraft(graph,from,to,queuedTransforms,editingRoute||undefined,replaceOccupied,activeVehicleTabId,selectedTransformInputs());
    const reserved=incompleteRows.filter(row=>row.lane.id!==retainedRow?.lane.id).map(row=>({...next.document.bindings[0],id:row.lane.id}));for(const binding of next.document.bindings){if(!document.bindings.some(previous=>previous.id===binding.id)&&reserved.some(row=>row.id===binding.id))binding.id=graphFreshId({...next.document,bindings:[...next.document.bindings,...reserved]},'route');}
    reserveIncompleteRouteIds(next,retainedRow?.lane.id);
    if(retainedRow){const target=canvasConnectionPlan(next,from,to).to,terminal=next.document.bindings.find(binding=>binding.to.nodeId===target.nodeId&&binding.to.portId===target.portId&&!document.bindings.some(previous=>previous.id===binding.id));if(terminal){preserveLaneId(next,terminal,retainedRow.lane.id);const otherLanes=destinationOverview(signalOverview(next.document)).find(bundle=>bundle.binding.to.nodeId===toNode)?.lanes.filter(lane=>lane.id!==terminal.id)??[],before=otherLanes[retainedRow.index]?.id;next.document.bindings=next.document.bindings.filter(binding=>binding!==terminal);const index=before?next.document.bindings.findIndex(binding=>binding.id===before):-1;if(index>=0)next.document.bindings.splice(index,0,terminal);else next.document.bindings.push(terminal);}}
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
  function addConnectionTransform(kind:ProcessingPreset=transformOptions[0]?.value){if(readOnly||queuedTransforms.length>=32||!sourceContract||!targetAction||!kind)return;try{queuedTransforms=[...queuedTransforms,createConnectionTransform(graph,queuedTransforms.at(-1)?.outputs[0]?.contract??sourceContract,targetAction,kind,[...queuedTransforms,...incompleteRows.flatMap(row=>row.transforms)])];message='';autoConnect();}catch(error){message=error instanceof Error?error.message:String(error);}}
  function changeConnectionTransform(index:number,value:string){if(readOnly||!queuedTransforms[index])return;try{const previous=queuedTransforms[index];let node=createConnectionTransform(graph,index?queuedTransforms[index-1].outputs[0].contract:sourceContract??previous.inputs.find(port=>port.id==='value')?.contract??previous.inputs[0]?.contract??previous.outputs[0].contract,targetAction??previous.outputs[0].contract,value as ProcessingPreset,[...queuedTransforms,...incompleteRows.flatMap(row=>row.transforms)]);if(['any','all','priority'].includes(previous.operation.kind)&&['any','all','priority'].includes(node.operation.kind)){while(node.inputs.length<previous.inputs.length)node=addChainInput(node);node.inputs=node.inputs.map((port,at)=>({...port,id:previous.inputs[at]?.id??port.id}));if(node.operation.kind==='priority')node.operation.order=node.inputs.map(port=>port.id);}queuedTransforms=queuedTransforms.map((item,at)=>at===index?{...node,id:previous.id}:item);message='';autoConnect();}catch(error){message=error instanceof Error?error.message:String(error);}}
  function removeConnectionTransform(id:string){if(readOnly)return;queuedInputs=Object.fromEntries(Object.entries(queuedInputs).filter(([key])=>!key.startsWith(id+'/')));canvas?.focusConnectionTransform('');queuedTransforms=queuedTransforms.filter(node=>node.id!==id);message='';autoConnect();}
  function startConnectionDraft(fromId=fromNode,toId=toNode){if(readOnly)return;const fresh=!!activeIncompleteRowId;rememberIncomplete();clearedSource=false;clearedDestination=false;activeIncompleteRowId='';queuedInputs={};routeOpen=true;fromPort=!fresh&&fromId===fromNode?fromPort:'';toPort=!fresh&&toId===toNode?toPort:'';fromNode=fromId;toNode=toId;editingRoute='';autoCommitArmed=true;draftRowOpen=true;queuedTransforms=[];message='';autoConnect();void tick().then(()=>routingElement?.querySelector<HTMLElement>('.pending-connection input')?.focus());}
  function discardConnectionDraft(){queuedInputs={};clearedSource=false;clearedDestination=false;if(activeIncompleteRowId)incompleteRows=incompleteRows.filter(row=>row.lane.id!==activeIncompleteRowId);activeIncompleteRowId='';canvas?.focusConnectionTransform('');selectedTransformLane='';draftRowOpen=false;queuedTransforms=[];editingRoute='';fromPort='';toPort='';message='';}
  function selectOverviewTransform(laneId:string,id:string){if(incompleteRows.some(row=>row.lane.id===laneId))activateIncomplete(laneId);else if(activeIncompleteRowId){rememberIncomplete();activeIncompleteRowId='';draftRowOpen=false;queuedTransforms=[];queuedInputs={};}selectedTransformLane=laneId;return id;}
  function transformRow(id:string,laneId=selectedTransformLane){const rows=signalOverview(document).flatMap(bundle=>bundle.lanes).filter(lane=>lane.transforms.includes(id));return rows.find(lane=>lane.id===laneId)??rows[0];}
  function instanceContext(id:string,laneId=selectedTransformLane){const lane=transformRow(id,laneId);return lane?instanceRowTransforms(graph,lane.id,activeVehicleTabId):{draft:graph,nodeIds:{} as Record<string,string>,bindingIds:{} as Record<string,string>};}
  function incompletePort(endpoint:ProfileDiscoveredEndpoint){const ports=discoveredChoices[endpoint.nodeId]??[];return endpoint.port.observed?ports.find(port=>port.observed?.descriptor.id===endpoint.port.observed!.descriptor.id&&port.observed.contractHash===endpoint.port.observed!.contractHash):ports.find(port=>port.profilePortId===endpoint.port.profilePortId&&!!port.profilePortId);}
  function rememberIncomplete(){if(!activeIncompleteRowId)return;const previous=incompleteRows.find(row=>row.lane.id===activeIncompleteRowId);if(!previous)return;const from=fromChoice?{nodeId:fromNode,port:fromChoice}:previous.from,to=toChoice?{nodeId:toNode,port:toChoice}:previous.to;const plan=connectionDraftPlan(graph,from,to,queuedTransforms,undefined,activeVehicleTabId,selectedTransformInputs());incompleteRows=incompleteRows.map(row=>row.lane.id===activeIncompleteRowId?{...row,from,to,transforms:copy(queuedTransforms),inputs:copy(queuedInputs),lane:{...row.lane,transforms:queuedTransforms.map(node=>node.id),sources:[{nodeId:from.nodeId,portId:from.port.profilePortId??from.port.key},...Object.values(queuedInputs).map(endpoint=>({nodeId:endpoint.nodeId,portId:endpoint.port.profilePortId??endpoint.port.key}))]},error:clearedSource?'Choose an output source.':clearedDestination?'Choose a destination input.':plan.error,clearedSource,clearedDestination}:row);}
  function activateIncomplete(id:string){if(activeIncompleteRowId===id)return;rememberIncomplete();const row=incompleteRows.find(row=>row.lane.id===id);if(!row)return;activeIncompleteRowId=id;routeOpen=true;draftRowOpen=true;editingRoute='';fromNode=row.from.nodeId;toNode=row.to.nodeId;clearedSource=!!row.clearedSource;clearedDestination=!!row.clearedDestination;fromPort=clearedSource?'':incompletePort(row.from)?.key??'';toPort=clearedDestination?'':incompletePort(row.to)?.key??'';queuedTransforms=copy(row.transforms);queuedInputs=copy(row.inputs??{});autoCommitArmed=true;}

  const inputOptions=$derived(document.resources.flatMap(node=>(discoveredChoices[node.id]??[]).filter(port=>port.direction==='output').map(port=>({value:JSON.stringify([node.id,port.key]),label:portPresentation(port).label,description:node.name+' · '+portContractLabel(port.contract),searchText:node.name+' '+portPresentation(port).searchText}))));
  function transformInputSpecs(transforms:readonly DeepReadonly<ProfileProcessingNode>[],contract?:DeepReadonly<ProfilePortContract>){let tail=contract;return transforms.flatMap(node=>{const primary=tail?connectionTransformInput(node,tail):node.inputs.find(port=>port.id==='value')??node.inputs[0];tail=node.outputs[0]?.contract;const extras=node.inputs.filter(port=>port.id!==(primary?.id??node.inputs[0]?.id)),variadic=['any','all','priority'].includes(node.operation.kind);return extras.map((port,index)=>({nodeId:node.id,port,label:'Source for '+node.name+' '+port.name,key:node.id+'/'+port.id,addAfter:index===extras.length-1&&variadic,canAdd:node.inputs.length<8,canRemove:variadic&&node.inputs.length>2&&node.inputs.indexOf(port)>=2,nodeName:node.name}));});}
  const queuedInputSpecs=$derived(transformInputSpecs(queuedTransforms,sourceContract));
  function rowEndpoint(endpoint:DeepReadonly<ProfileDocument['bindings'][number]['from']>){const port=(discoveredChoices[endpoint.nodeId]??[]).find(port=>port.profilePortId===endpoint.portId);return port?{nodeId:endpoint.nodeId,port}:undefined;}
  function rowInputFields(lane:SignalLane){const retained=incompleteRows.find(row=>row.lane.id===lane.id),from=retained?.from??(lane.sources[0]?rowEndpoint(lane.sources[0]):undefined);if(!from)return[];const nodes=retained?.transforms??lane.transforms.flatMap(id=>{const node=document.processingNodes.find(node=>node.id===id);return node?[node]:[];});return[{key:'primary',primary:true,endpoint:from,nodeId:'',portId:'',portName:'',label:'Filter source signals',canRemove:false},...transformInputSpecs(nodes,from.port.contract).map(slot=>{const binding=document.bindings.find(binding=>binding.to.nodeId===slot.nodeId&&binding.to.portId===slot.port.id);return{key:slot.key,primary:false,endpoint:retained?retained.inputs?.[slot.key]:binding?rowEndpoint(binding.from):undefined,nodeId:slot.nodeId,portId:slot.port.id,portName:slot.port.name,label:slot.label,canRemove:slot.canRemove};})];}
  function rowInputGroups(lane:SignalLane){const retained=incompleteRows.find(row=>row.lane.id===lane.id);return(retained?.transforms??lane.transforms.flatMap(id=>{const node=document.processingNodes.find(node=>node.id===id);return node?[node]:[];})).filter(node=>['any','all','priority'].includes(node.operation.kind));}
  function inputSelection(endpoint?:ProfileDiscoveredEndpoint){if(!endpoint)return '';const port=incompletePort(endpoint)??endpoint.port;return JSON.stringify([endpoint.nodeId,port.key]);}
  function optionsForInput(endpoint?:ProfileDiscoveredEndpoint){const value=inputSelection(endpoint);if(!endpoint||!value||inputOptions.some(option=>option.value===value))return inputOptions;const port=incompletePort(endpoint)??endpoint.port;return[{value,label:portPresentation(port).label,description:portContractLabel(port.contract),searchText:portPresentation(port).searchText},...inputOptions];}

  function selectedTransformInputs(){return Object.fromEntries(Object.entries(queuedInputs).map(([key,endpoint])=>[key,{nodeId:endpoint.nodeId,port:incompletePort(endpoint)??(endpoint.port.observed?{...endpoint.port,profilePortId:null}:endpoint.port)}]));}
  function addTransformInput(laneId:string,nodeId:string){if(readOnly)return false;const retained=incompleteRows.find(row=>row.lane.id===laneId&&row.transforms.some(node=>node.id===nodeId));if(retained)activateIncomplete(laneId);const queued=queuedTransforms.find(node=>node.id===nodeId);if(queued){try{const next=addChainInput(queued);queuedTransforms=queuedTransforms.map(node=>node.id===nodeId?next:node);autoConnect();return true;}catch(issue){message=issue instanceof Error?issue.message:String(issue);return false;}}const row=transformRow(nodeId,laneId);if(!row)return false;const local=instanceContext(nodeId,row.id),id=local.nodeIds[nodeId]??nodeId;return reviseLaneTransforms(local.draft,row.id,nodes=>nodes.map(node=>node.id===id?addChainInput(node):node),'Add input',id);}
  function removeTransformInput(laneId:string,nodeId:string,portId:string){if(readOnly)return false;const retained=incompleteRows.find(row=>row.lane.id===laneId&&row.transforms.some(node=>node.id===nodeId));if(retained)activateIncomplete(laneId);const queued=queuedTransforms.find(node=>node.id===nodeId);if(queued){if(queued.inputs.length<=2)return false;try{const next=removeChainInput(queued,portId);queuedTransforms=queuedTransforms.map(node=>node.id===nodeId?next:node);const inputs={...queuedInputs};delete inputs[nodeId+'/'+portId];queuedInputs=inputs;autoConnect();return true;}catch(issue){message=issue instanceof Error?issue.message:String(issue);return false;}}const row=transformRow(nodeId,laneId),node=document.processingNodes.find(node=>node.id===nodeId);if(!row||!node||node.inputs.length<=2)return false;const local=instanceContext(nodeId,row.id),id=local.nodeIds[nodeId]??nodeId;return reviseLaneTransforms(local.draft,row.id,nodes=>nodes.map(node=>node.id===id?removeChainInput(node,portId):node),'Remove input',id);}
  function chooseLaneTransformInput(laneId:string,nodeId:string,portId:string,key:string){if(readOnly)return false;const retained=incompleteRows.find(row=>row.lane.id===laneId);if(retained){activateIncomplete(laneId);chooseTransformInput(nodeId,portId,key);return true;}let endpoint:ProfileDiscoveredEndpoint|null=null;if(key){let choice:string[];try{choice=JSON.parse(key);}catch{return false;}const port=(discoveredChoices[choice[0]]??[]).find(port=>port.key===choice[1]&&port.direction==='output');if(!port)return false;endpoint={nodeId:choice[0],port};}const row=transformRow(nodeId,laneId);if(!row)return false;const local=instanceContext(nodeId,row.id),id=local.nodeIds[nodeId]??nodeId;return reviseLaneTransforms(local.draft,row.id,nodes=>nodes,key?'Change input source':'Clear input source',id,{[id+'/'+portId]:endpoint});}
  function chooseTransformInput(nodeId:string,portId:string,key:string){if(readOnly)return;const next={...queuedInputs},id=nodeId+'/'+portId;if(!key)delete next[id];else{let selected:string[];try{selected=JSON.parse(key);}catch{return;}const port=(discoveredChoices[selected[0]]??[]).find(port=>port.key===selected[1]&&port.direction==='output');if(!port)return;next[id]={nodeId:selected[0],port};}queuedInputs=next;message='';autoConnect();}

  function removeOverviewTransform(id:string,laneId?:string){
    if(readOnly)return;const retained=incompleteRows.find(row=>row.lane.id===laneId&&row.transforms.some(node=>node.id===id));if(retained)activateIncomplete(retained.lane.id);if(queuedTransforms.some(node=>node.id===id)){removeConnectionTransform(id);return;}
    const row=transformRow(id,laneId);if(!row){apply(()=>graphRemoveNode(graph,id),'Remove transformation');return;}const context=instanceContext(id,laneId),mappedId=context.nodeIds[id]??id;if(context.draft.document.processingNodes.find(node=>node.id===mappedId)?.inputs.length!==1){reviseLaneTransforms(context.draft,row.id,nodes=>nodes.filter(node=>node.id!==mappedId),'Remove transformation','');return;}let pending:IncompleteConnection|undefined;
    const accepted=apply(()=>{const local=instanceContext(id,laneId),mapped=local.nodeIds[id]??id;
      try{return graphBypassProcessing(local.draft,mapped);}catch(issue){
        const lane=signalOverview(local.draft.document).flatMap(bundle=>bundle.lanes).find(lane=>lane.id===row?.id);
        if(!lane||lane.sources.length!==1||lane.transforms.some(id=>{const node=local.draft.document.processingNodes.find(node=>node.id===id)!;return node.inputs.length!==1||node.outputs.length!==1;}))throw issue;
        const from=lane.sources[0],sourcePort=(discoveredChoices[from.nodeId]??[]).find(port=>port.profilePortId===from.portId),targetPort=(discoveredChoices[lane.destination.nodeId]??[]).find(port=>port.profilePortId===lane.destination.portId);
        if(!sourcePort||!targetPort)throw issue;
        const transforms=lane.transforms.filter(id=>id!==mapped).map(id=>copy(local.draft.document.processingNodes.find(node=>node.id===id)!) as ProfileProcessingNode),bundle=destinationOverview(signalOverview(document)).find(bundle=>bundle.lanes.some(item=>item.id===lane.id));
        pending={lane:{...lane,transforms:transforms.map(node=>node.id)},index:bundle?.lanes.findIndex(item=>item.id===lane.id)??0,from:{nodeId:from.nodeId,port:sourcePort},to:{nodeId:lane.destination.nodeId,port:targetPort},transforms,error:issue instanceof Error?issue.message:String(issue)};
        return discardCommittedConnection(local.draft,lane.id);
      }
    },'Remove transformation',row?.id);
    if(accepted&&pending){rememberIncomplete();const next=pending;incompleteRows=[...incompleteRows.filter(row=>row.lane.id!==next.lane.id),next];activeIncompleteRowId='';activateIncomplete(next.lane.id);canvas?.openConnectionOverview(next.from.nodeId,next.to.nodeId);void tick().then(()=>{fromPort=incompletePort(next.from)?.key??'';toPort=incompletePort(next.to)?.key??'';autoConnect();});}
  }
  function existingTransformOptions(id:string){const lane=transformRow(id);return chainTransformOptions(lane?{...graph,document:{...document,bindings:document.bindings.filter(binding=>binding.from.nodeId!==id||lane.bindingIds.includes(binding.id))}}:graph,id);}
  function reviseLaneTransforms(local:DeepReadonly<ProfileGraphDraft>,laneId:string,revise:(nodes:ProfileProcessingNode[],input:DeepReadonly<ProfilePortContract>,target:DeepReadonly<ProfilePortContract>)=>ProfileProcessingNode[],label:string,focus:string,inputChanges:Record<string,ProfileDiscoveredEndpoint|null>={},clearSide?:'from'|'to'){
    let pending:IncompleteConnection|undefined;
    const accepted=apply(()=>{const lane=signalOverview(local.document).flatMap(bundle=>bundle.lanes).find(row=>row.id===laneId);if(!lane?.sources.length)throw Error('Choose a current signal row.');
      const source=lane.sources[0],from=(discoveredChoices[source.nodeId]??[]).find(port=>port.profilePortId===source.portId),to=(discoveredChoices[lane.destination.nodeId]??[]).find(port=>port.profilePortId===lane.destination.portId);if(!from||!to)throw Error('Refresh the ports before editing this row.');
      const transforms=revise(lane.transforms.map(id=>copy(local.document.processingNodes.find(node=>node.id===id)!) as ProfileProcessingNode),from.contract,to.contract),inputs:Record<string,ProfileDiscoveredEndpoint>={};let tail=from.contract;for(const node of transforms){const primary=connectionTransformInput(node,tail);for(const port of node.inputs.filter(port=>port.id!==primary?.id)){const binding=local.document.bindings.find(binding=>binding.to.nodeId===node.id&&binding.to.portId===port.id),chosen=binding?(discoveredChoices[binding.from.nodeId]??[]).find(port=>port.profilePortId===binding.from.portId):undefined;if(binding&&chosen)inputs[node.id+'/'+port.id]={nodeId:binding.from.nodeId,port:chosen};}tail=node.outputs[0]?.contract??tail;}for(const [key,value]of Object.entries(inputChanges)){if(value)inputs[key]=value;else delete inputs[key];}const cleared=discardCommittedConnection(local,lane.id),endpoint=(nodeId:string,port:ProfileDiscoveredPort):ProfileDiscoveredEndpoint=>({nodeId,port:port.observed?{...port,profilePortId:null}:port}),fromEndpoint=endpoint(source.nodeId,from),toEndpoint=endpoint(lane.destination.nodeId,to),plan=clearSide?{draft:cleared,error:clearSide==='from'?'Choose an output source.':'Choose a destination input.'}:connectionDraftPlan(cleared,fromEndpoint,toEndpoint,transforms,undefined,activeVehicleTabId,Object.fromEntries(Object.entries(inputs).map(([key,value])=>[key,endpoint(value.nodeId,value.port)]))),bundle=destinationOverview(signalOverview(document)).find(bundle=>bundle.lanes.some(row=>row.id===lane.id));
      if(plan.error){pending={lane:{...lane,transforms:transforms.map(node=>node.id),sources:[source,...Object.values(inputs).map(endpoint=>({nodeId:endpoint.nodeId,portId:endpoint.port.profilePortId??endpoint.port.key}))]},inputs,index:bundle?.lanes.findIndex(row=>row.id===lane.id)??0,from:{nodeId:source.nodeId,port:from},to:{nodeId:lane.destination.nodeId,port:to},transforms,error:plan.error,clearedSource:clearSide==='from',clearedDestination:clearSide==='to'};return cleared;}
      const next=plan.draft,target=canvasConnectionPlan(next,fromEndpoint,toEndpoint).to,terminal=next.document.bindings.find(binding=>binding.to.nodeId===target.nodeId&&binding.to.portId===target.portId);
      if(terminal){preserveLaneId(next,terminal,lane.id);const order=bundle?.lanes.findIndex(row=>row.id===lane.id)??0,others=destinationOverview(signalOverview(next.document)).find(item=>item.binding.to.nodeId===lane.destination.nodeId)?.lanes.filter(row=>row.id!==terminal.id)??[],before=others[order]?.id;next.document.bindings=next.document.bindings.filter(binding=>binding!==terminal);const index=before?next.document.bindings.findIndex(binding=>binding.id===before):-1;if(index>=0)next.document.bindings.splice(index,0,terminal);else next.document.bindings.push(terminal);}
      for(const node of transforms){const position=local.document.editor.positions.find(position=>position.nodeId===node.id);if(position&&!next.document.editor.positions.some(item=>item.nodeId===node.id))next.document.editor.positions.push({...position});}
      return next;
    },label,laneId);
    if(accepted&&pending){rememberIncomplete();const row=pending;incompleteRows=[...incompleteRows.filter(item=>item.lane.id!==row.lane.id),row];activeIncompleteRowId='';activateIncomplete(row.lane.id);canvas?.openConnectionOverview(row.from.nodeId,row.to.nodeId);void tick().then(()=>{if(!row.clearedSource)fromPort=incompletePort(row.from)?.key??'';if(!row.clearedDestination)toPort=incompletePort(row.to)?.key??'';autoConnect();if(focus)canvas?.focusConnectionTransform(focus);});}
    return accepted;
  }
  function changeOverviewTransform(id:string,preset:ProcessingPreset){if(readOnly)return false;const pendingIndex=queuedTransforms.findIndex(node=>node.id===id);if(pendingIndex>=0){changeConnectionTransform(pendingIndex,preset);return true;}const original=document.processingNodes.find(node=>node.id===id);if(original?.draft)return configureDraft(id,preset);const local=instanceContext(id),mapped=local.nodeIds[id]??id,lane=transformRow(id);let next:ProfileGraphDraft;
    try{next=replaceChainTransform(local.draft,mapped,preset);}catch{if(!lane)return apply(()=>graphSetProcessing(graph,createProcessingDraft(preset,id),activeVehicleTabId),'Change transformation type');return reviseLaneTransforms(local.draft,lane.id,(nodes,input,target)=>{const index=nodes.findIndex(node=>node.id===mapped),source=index>0?nodes[index-1].outputs[0].contract:input,destination=nodes[index+1]?.inputs.find(port=>port.id==='value')?.contract??nodes[index+1]?.inputs[0]?.contract??target,replacement=createConnectionTransform(local.draft,source,destination,preset,nodes);return nodes.map((node,at)=>at===index?{...replacement,id:mapped}:node);},'Change transformation type',mapped);}
    const accepted=apply(()=>next,'Change transformation type',lane?.id);if(accepted&&mapped!==id)void tick().then(()=>canvas?.focusConnectionTransform(mapped));return accepted;
  }
  function choosePort(side:'from'|'to',key:string){if(readOnly)return;if(side==='from'){fromPort=key;clearedSource=!key;}else{toPort=key;clearedDestination=!key;}autoCommitArmed=true;autoConnect();}
  function autoConnect(){if(!active||!routeOpen||!autoCommitArmed||readOnly||!fromNode||!toNode)return;
    if(!fromPort&&!clearedSource){const choices=sourcePorts.filter(port=>!isProcessingDraft(document,fromNode)&&(!toChoice||!canvasPortReason(document,{nodeId:fromNode,port},{nodeId:toNode,port:toChoice})));if(choices.length===1)fromPort=choices[0].key;}
    if(!toPort&&!clearedDestination){const choices=targetPorts.filter(port=>(!inputWriter(toNode,port))&&(!fromChoice||!canvasPortReason(document,{nodeId:fromNode,port:fromChoice},{nodeId:toNode,port})));if(choices.length===1)toPort=choices[0].key;}
    if(!commitReadyConnection())rememberIncomplete();
  }
  function commitReadyConnection(){if(!active||readOnly||!draftRowOpen||!autoCommitArmed||clearedSource||clearedDestination||!fromNode||!toNode||!fromPort||!toPort)return false;try{const[from,to]=selectedEndpoints(),plan=connectionDraftPlan(graph,from,to,queuedTransforms,editingRoute||undefined,activeVehicleTabId,selectedTransformInputs());if(plan.error)return false;return connect(!!plan.writer);}catch{return false;}}
  function editRoute(id:string){const binding=visibleDocument.bindings.find(binding=>binding.id===id);if(!binding)return;queuedTransforms=[];draftRowOpen=true;editingRoute=id;fromNode=binding.from.nodeId;fromPort=binding.from.portId;toNode=binding.to.nodeId;toPort=binding.to.portId;routeOpen=true;autoCommitArmed=false;highlighted=[id];focusRoute();const overview=signalOverview(visibleDocument).find(bundle=>bundle.lanes.some(lane=>lane.bindingIds.includes(id)));canvas?.openConnectionOverview(overview?.binding.from.nodeId??fromNode,overview?.binding.to.nodeId??toNode);}
  function selectConnection(id:string){closeRoute();if(!id)return;const bundle=signalOverview(visibleDocument).find(bundle=>bundle.lanes.some(lane=>lane.bindingIds.includes(id)));if(bundle)canvas?.openConnectionOverview(bundle.binding.from.nodeId,bundle.binding.to.nodeId);}
  function reconnectConnection(id:string,side:'from'|'to',nodeId:string,position?:CanvasPoint,portId?:string){editRoute(id);if(!routeOpen)return;if(side==='from'){fromNode=nodeId;fromPort=portId??'';}else{toNode=nodeId;toPort=portId??'';}autoCommitArmed=true;focusRoute(position);autoConnect();if(fromNode&&toNode)canvas?.openConnectionOverview(fromNode,toNode);}
  function disconnectConnection(id:string){if(readOnly)return;if(incompleteRows.some(row=>row.lane.id===id)){incompleteRows=incompleteRows.filter(row=>row.lane.id!==id);if(activeIncompleteRowId===id)discardConnectionDraft();return;}if(apply(()=>graphDisconnect(graph,id),'Disconnect ports'))closeRoute();}
  function insertOnEdge(bindingId:string,kind:ProcessingPreset,laneId=bindingId){if(activeIncompleteRowId){rememberIncomplete();activeIncompleteRowId='';draftRowOpen=false;queuedTransforms=[];queuedInputs={};}let id='',lane='';if(apply(()=>{const row=signalOverview(document).flatMap(bundle=>bundle.lanes).find(row=>row.id===laneId||row.bindingIds.includes(bindingId)),local=row?instanceRowTransforms(graph,row.id,activeVehicleTabId):{draft:graph,nodeIds:{},bindingIds:{} as Record<string,string>},insertion=processingInsertion(local.bindingIds[bindingId]??bindingId,kind,local.draft);id=insertion.node.id;lane=signalOverview(insertion.next.document).flatMap(bundle=>bundle.lanes).find(row=>row.transforms.includes(id))?.id??laneId;return insertion.next;},'Insert transformation',laneId)){discardConnectionDraft();selectedTransformLane=lane;void tick().then(()=>canvas?.focusConnectionTransform(id));}}
  function selectPort(nodeId:string,port:DeepReadonly<ObservedResourcePort>){if(readOnly)return;const choice=(discoveredChoices[nodeId]??[]).find(choice=>choice.observed?.descriptor.id===port.descriptor.id&&choice.observed.contractHash===port.contractHash);if(!choice){message='Refresh this resource before choosing its port.';return;}if(port.descriptor.direction==='output')source(nodeId,undefined,choice.key);else destination(nodeId,undefined,choice.key);}
  function placeComponent(next:ProfileGraphDraft,id:string,position?:CanvasPoint):ProfileGraphDraft{return{...next,document:profilePlaceComponent(document,next.document,id,position,activeVehicleTabId)};}
  export function addGroup(){return canvas?.addGroup();}
  export function focusGroup(id:string){canvas?.focusGroup(id);}
  export function renameGroup(id:string,name:string){canvas?.renameGroup(id,name);}
  export function deleteGroup(id:string){canvas?.deleteGroup(id);}
  export function addVirtual(position?:CanvasPoint){const id=graphFreshId(document,'virtual');apply(()=>{if(!profilePaletteCanAdd({kind:'virtual'},document,assignments,inventory,activeVehicleTabId))throw new Error('A profile supports at most eight active virtual controllers.');return placeComponent(graphSetVirtual(graph,{id,name:'Virtual controller',required:false,layout:{buttons:['button-1','button-2','button-3','button-4'],axes:[],hats:[]}},activeVehicleTabId),id,position);},'Add virtual controller');}
  export function addDebug(position?:CanvasPoint){if(!profilePaletteCanAdd({kind:'debug-controller'},document,assignments,inventory,activeVehicleTabId))return;const id=graphFreshId(document,'debug');apply(()=>placeComponent(graphSetDebugController(graph,debugControllerResource(id),activeVehicleTabId),id,position),'Add debug controller');}
  export function addDesktop(position?:CanvasPoint,kind:DesktopBlockKind='keyboard'){if(!profilePaletteCanAdd({kind:'desktop',device:kind},document,assignments,inventory,activeVehicleTabId))return;const id=graphFreshId(document,'desktop');apply(()=>placeComponent(graphSetDesktop(graph,desktopBlock(kind,id),activeVehicleTabId),id,position),`Add ${kind} block`);}
  function updateDesktop(output:ProfileDesktopOutput){return apply(()=>graphSetDesktop(graph,output),'Edit typed desktop actions');}
  function processingInsertion(bindingId:string|null,kind:ProcessingPreset,base:DeepReadonly<ProfileGraphDraft>=graph){
    const prepared=bindingId?null:prepareConnection(),working=prepared?.draft??base;
    const binding=bindingId?working.document.bindings.find(binding=>binding.id===bindingId):{id:'',from:prepared!.from,to:prepared!.to};
    if(!binding)throw new Error('Choose an existing connection.');
    const input=graphPort(working.document,binding.from)?.contract,target=graphPort(working.document,binding.to)?.contract;
    if(!input||!target)throw new Error('This connection no longer has both declared ports.');
    const node=processingTemplate(kind,graphFreshId({...working.document,processingNodes:[...working.document.processingNodes,...incompleteRows.flatMap(row=>row.transforms)]},'transform'),input,target);
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
  function openConnection(id:string,laneId?:string){if(readOnly)return;if(incompleteRows.some(row=>row.lane.id===(laneId??id))){activateIncomplete(laneId??id);void tick().then(()=>routingElement?.querySelector<HTMLButtonElement>('.incomplete-connection button[aria-label="Add transform"]')?.click());return;}insertOnEdge(id,'identity',laneId??id);}
  function openPendingConnection(){draftRowOpen=true;addConnectionTransform();}
  function closeConnection(){connectionRoute='';pendingConnection=false;void tick().then(()=>{if(connectionTrigger?.isConnected&&(connectionTrigger instanceof HTMLElement||connectionTrigger instanceof SVGElement))connectionTrigger.focus();else routingElement?.querySelector<HTMLElement>('[aria-label="Profile canvas"]')?.focus();});}
  function showConnection(node:HTMLDialogElement){node.showModal();return()=>node.close();}
  function insertTransformation(){draftRowOpen=true;addConnectionTransform(preset);}
  function actionPack(mode:'action-pack'|'set-on-change'){if(readOnly||targetAction?.delivery!=='action'||queuedTransforms.length>=32)return;const input=sourceContract,node=processingActionPack(graphFreshId({...document,processingNodes:[...document.processingNodes,...queuedTransforms,...incompleteRows.flatMap(row=>row.transforms)]},'transform'),targetAction,mode,input?.delivery==='event'?input:undefined);queuedTransforms=[...queuedTransforms,node];autoConnect();}
  function updateProcessing(node:ProfileProcessingNode){if(readOnly)return false;if(queuedTransforms.some(item=>item.id===node.id)){try{const next=processingDerivedNode(node),error=processingNodeError(next);if(error)throw Error(error);queuedTransforms=queuedTransforms.map(item=>item.id===node.id?next:item);message='';autoConnect();return true;}catch(error){message=error instanceof Error?error.message:String(error);return false;}}let mapped=node.id;const accepted=apply(()=>{const local=instanceContext(node.id);mapped=local.nodeIds[node.id]??node.id;return graphUpdateProcessing(local.draft,{...node,id:mapped});},'Edit transformation',transformRow(node.id)?.id);if(accepted&&mapped!==node.id)void tick().then(()=>canvas?.focusConnectionTransform(mapped));return accepted;}
  function configureDraft(id:string,kind:ProcessingPreset,parameters:Record<string,unknown>={}){return apply(()=>updateProcessingDraft(graph,id,kind,parameters),'Configure transformation');}
  function updateVirtual(output:Omit<ProfileVirtualOutput,'ports'>){apply(()=>graphSetVirtual(graph,output),'Edit virtual controller layout');}
  function updateDebug(resource:ProfileResource){apply(()=>graphSetDebugController(graph,resource),'Edit debug controller');}
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
  {#snippet draftConnection()}
    {#if !activeIncompleteRowId}<span class="connection-warning-slot">{#if connectionWarning}<span class="route-error-icon" role="img" title={connectionWarning} aria-label={`Connection error: ${connectionWarning}`}><Icon name="alert" size={16}/></span>{/if}</span>{/if}
    <div class="draft-endpoint">
      <ProfilePortCombobox compact allowEmpty label={activeIncompleteRowId?'Filter source signals':'Find an output'} placeholder="Choose an output" options={sourceOptions} value={fromPort} onselect={key=>choosePort('from',key)} disabled={readOnly}/>
      {#each queuedInputSpecs as slot (slot.key)}
        {@const assigned=queuedInputs[slot.key]}
        {@const port=assigned?incompletePort(assigned):undefined}
        <div class="source-input-row" data-input-key={slot.key}><ProfilePortCombobox compact allowEmpty label={slot.label} placeholder={'Choose '+slot.port.name+' input'} options={inputOptions} value={assigned&&port?JSON.stringify([assigned.nodeId,port.key]):''} onselect={key=>chooseTransformInput(slot.nodeId,slot.port.id,key)} showClearWhenEmpty onclear={slot.canRemove?()=>removeTransformInput(activeIncompleteRowId,slot.nodeId,slot.port.id):undefined} clearLabel={'Remove '+slot.port.name+' input'} disabled={readOnly}/></div>{#if slot.addAfter}<button class="add-input" aria-label={'Add input to '+slot.nodeName} disabled={readOnly||!slot.canAdd} onclick={()=>addTransformInput(activeIncompleteRowId,slot.nodeId)}><Icon name="plus" size={13}/>Add</button>{/if}
      {/each}
      {#if discoveryBusy.includes(fromNode)}<p role="status">Discovering outputs…</p>{/if}
      {#if discoveryErrors[fromNode]}<p role="alert">{discoveryErrors[fromNode]}</p><button class="text-button" onclick={()=>retryDiscovery(fromNode)}><Icon name="refresh" size={14}/>Retry</button>{/if}
    </div>
    <div class="draft-chain">
      <div class="draft-transforms">{#each queuedTransforms as node (node.id)}
        <div class="draft-transform" data-transform-id={node.id}>
          <button class="transform-chip" title="Click to edit · Middle-click to delete" onpointerdown={event=>{if(event.button===1){event.preventDefault();event.stopPropagation();}}} onauxclick={event=>{if(event.button===1){event.preventDefault();event.stopPropagation();removeConnectionTransform(node.id);}}} onclick={()=>canvas?.focusConnectionTransform(node.id)}><strong>{PROCESSING_PRESETS.find(([kind])=>kind===node.operation.kind)?.[1]??node.name}</strong></button>
        </div>
      {/each}</div>
      <ProfilePortCombobox compact iconTrigger label="Add transform" placeholder="Choose a transform" options={transformOptions} value="" onselect={kind=>addConnectionTransform(kind as ProcessingPreset)} disabled={readOnly||!sourceContract||!targetAction||queuedTransforms.length>=32}/>
      {#if targetAction?.delivery==='action'}<div class="draft-actions"><button class="secondary-button" disabled={readOnly} onclick={()=>actionPack('action-pack')}><Icon name="plus" size={14}/>Arguments</button>{#if targetAction.semantics==='setter'}<button class="secondary-button" disabled={readOnly} onclick={()=>actionPack('set-on-change')}><Icon name="plus" size={14}/>Set on change</button>{/if}</div>{/if}
    </div>
    <div class="draft-destination">
      <div class="draft-endpoint" class:needs-conversion={!!connectionWarning}>
      <ProfilePortCombobox compact allowEmpty label={activeIncompleteRowId?'Filter destination signals':'Find an input'} placeholder="Choose an input" options={targetOptions} value={toPort} onselect={key=>choosePort('to',key)} disabled={readOnly}/>
      {#if discoveryBusy.includes(toNode)}<p role="status">Discovering inputs…</p>{/if}
      {#if discoveryErrors[toNode]}<p role="alert">{discoveryErrors[toNode]}</p><button class="text-button" onclick={()=>retryDiscovery(toNode)}><Icon name="refresh" size={14}/>Retry</button>{/if}
      </div>
      <button class="delete-draft" type="button" aria-label={activeIncompleteRowId?'Delete connection row':'Delete new connection row'} title="Delete row" disabled={readOnly} onclick={discardConnectionDraft}><Icon name="trash" size={16}/></button>
    </div>
  {/snippet}
  <ProfileCanvas removeNode={removeNode} bind:this={canvas} {addTransformInput} {routeIssue} companionAuthoring simpleControllerInspector connectionDraft={draftRowOpen?draftConnection:undefined} draftTransforms={[...new Map([...queuedTransforms,...incompleteRows.flatMap(row=>row.transforms)].map(node=>[node.id,node])).values()]} incompleteRows={incompleteRows.map(row=>({lane:row.lane,index:row.index,sourceNodeId:row.from.nodeId,targetNodeId:row.to.nodeId}))} {activeIncompleteRowId} addOverviewConnection={startConnectionDraft} removeOverviewTransform={removeOverviewTransform} selectOverviewTransform={selectOverviewTransform} {document} {activeVehicleTabId} {assignments} {inventory} {inventoryClient} {readOnly} bind:selectedId {openGameSettings} {paletteDrag} {canDropComponent} {dropComponent} {endPaletteDrag} change={(next,label)=>change({document:next,assignments:copy(assignments) as ProfileLocalAssignment[]},label??'Edit canvas layout')} {connectBlocks} connectionFrom={source} connectionTo={destination} {selectedEdgeId} {selectConnection} {reconnectConnection} {disconnectConnection} connectionSourceId={routeOpen?fromNode:''} connectionTargetId={routeOpen?toNode:''} connectionSourcePort={routeOpen?fromPort:''} connectionTargetPort={routeOpen?toPort:''} reconnectingBindingId={routeOpen?editingRoute:''} selectProviderPort={selectPort} removeProviderPort={(nodeId,portId)=>apply(()=>graphRemovePort(graph,nodeId,portId),'Remove selected port and routes')} highlightedEdges={highlighted} diagnostics={[]} {undo} {redo} {discoveredPorts} {discoveryBusy} {discoveryErrors} connectionOpen={routeOpen} closeConnectionPanel={closeRoute} insertTransform={openConnection} transferTransform={(sourceLane,nodeId,targetLane,before)=>apply(()=>graphTransferProcessing(graph,sourceLane,nodeId,targetLane,before),sourceLane===targetLane?'Move transformation in chain':'Copy transformation to chain',targetLane)}>
    {#snippet sourceInputs(lane)}
      {#each rowInputFields(lane) as field (field.key)}
        {#if field.primary&&field.endpoint}{@render routeEndpoint!(lane.id,'from',{nodeId:field.endpoint.nodeId,portId:field.endpoint.port.profilePortId??field.endpoint.port.key})}
        {:else}<div class="source-input-row" data-input-key={field.key}><ProfilePortCombobox compact allowEmpty label={field.label} placeholder={'Choose '+field.portName+' input'} options={optionsForInput(field.endpoint)} value={inputSelection(field.endpoint)} onselect={key=>chooseLaneTransformInput(lane.id,field.nodeId,field.portId,key)} showClearWhenEmpty onclear={field.canRemove?()=>removeTransformInput(lane.id,field.nodeId,field.portId):undefined} clearLabel={'Remove '+field.portName+' input'} disabled={readOnly}/></div>{/if}
      {/each}
      {#each rowInputGroups(lane) as node (node.id)}<button class="add-input" aria-label={'Add input to '+node.name} disabled={readOnly||node.inputs.length>=8} onclick={()=>addTransformInput(lane.id,node.id)}><Icon name="plus" size={13}/>Add</button>{/each}
    {/snippet}
    {#snippet routeEndpoint(laneId,side,endpoint)}
      {@const incomplete=incompleteRows.find(row=>row.lane.id===laneId)}
      {@const edit={laneId,side,endpoint}}
      {@const choices=incomplete?(discoveredChoices[endpoint.nodeId]??[]).filter(port=>port.direction===(side==='from'?'output':'input')):routeEndpointChoices(graph,edit,discoveredChoices[endpoint.nodeId]??[])}
      <ProfilePortCombobox compact allowEmpty label={side==='from'?'Filter source signals':'Filter destination signals'} placeholder="Type to filter signals" options={choices.map(port=>({value:port.key,...portPresentation(port)}))} value={incomplete?(side==='from'?incomplete.clearedSource:incomplete.clearedDestination)?'':incompletePort(side==='from'?incomplete.from:incomplete.to)?.key??'':endpoint.portId} onselect={key=>chooseLanePort(edit,key)} disabled={readOnly}/>
    {/snippet}
    {#snippet transformSettings(node)}
      {@const pendingIndex=queuedTransforms.findIndex(item=>item.id===node.id)}
      {@const typeOptions=pendingIndex>=0?connectionTransformOptions():existingTransformOptions(node.id)}
      {#if node.processing}<ProfileTransformSettings node={node.processing} {typeOptions} remove={()=>removeOverviewTransform(node.id,selectedTransformLane)} changeType={kind=>pendingIndex>=0?changeConnectionTransform(pendingIndex,kind):changeOverviewTransform(node.id,kind)} change={updateProcessing} configure={(kind,parameters)=>configureDraft(node.id,kind,parameters)}/>{/if}
    {/snippet}
    {#snippet inspector(node)}
      {@const physicalController=node.resource?.kind==='device'&&node.observation?.resourceType==='host-controller'}
      <fieldset class="authoring-inspector" disabled={readOnly}>
        {#if node.resource?.kind==='debug-controller'}
          <ProfileDebugControllerEditor resource={node.resource} profileId={document.id} revision={document.revision} {active} disabled={readOnly} update={updateDebug}/>
        {:else}
        {#if node.resource}<ProfileResourceSettings {node} {document} {games} controller={physicalController} source={assignmentSource} resources={assignmentResources} page={assignmentPage} mappings={selectedMappings} options={mappingOptions} busy={assignmentBusy} error={assignmentError} hasPrevious={assignmentBrowser.hasPrevious} hasNext={assignmentBrowser.hasNext} policy={(key,value)=>resourcePolicy(node,key,value)} visibility={value=>apply(()=>graphSetVisibility(graph,node.id,value==='hidden-while-active'?'hidden-while-active':'visible'),'Edit visibility intent')} publication={value=>publicationPolicy(node,value)} chooseSource={value=>{assignmentSource=value;if(value)void browseAssignment('open');}} {chooseMapping} browse={browseAssignment} assign={assignResource}/>{/if}
        {#if node.virtual}<ProfileVirtualEditor output={node.virtual} profileId={document.id} revision={document.revision} disabled={readOnly} change={updateVirtual}/>{/if}
        {#if node.desktop}<ProfileDesktopEditor output={node.desktop} change={updateDesktop}/>{/if}
        {/if}
      </fieldset>
      {#if node.resource?.kind==='companion'}<ProfileCompanionEditor {graph} {inventory} resourceId={node.id} {readOnly} {active} client={companionDiscovery} {change}/>{/if}
      {#if physicalController}
        <ProfileControllerInputs resource={node.observation} {active}/>
      {/if}
      {#if node.resource?.kind==='game'}
        {@const catalogueSource=catalogueSources.get(node.id)}
        {@const details=discoveries[catalogueSourceId(node)]?.details}
        {@const assigned=!!catalogueSource&&node.assignment?.sourceId===catalogueSource.id&&node.resource.ports.every(port=>discoveredChoices[node.id]?.some(choice=>choice.current&&choice.profilePortId===port.id))}
        {#key `${node.id}/${catalogueSource?.id??''}`}
          {#if !assigned&&node.resource.ports.length&&catalogueSource}<ProfileIntegrationAssignment ports={node.resource.ports} options={details?.ports??[]} busy={discoveryBusy.includes(node.id)} {readOnly} assign={selections=>apply(()=>graphAssignResource(graph,node.id,catalogueSource,selections),'Match integration signals')}/>{/if}
          {#each ['input','output'] as direction (direction)}<ProfileIntegrationPorts direction={direction==='input'?'input':'output'} ports={discoveredChoices[node.id]??[]} busy={discoveryBusy.includes(node.id)} error={discoveryErrors[node.id]??''} readOnly={readOnly||!assigned} select={port=>port.direction==='input'?destination(node.id,undefined,port.key):source(node.id,undefined,port.key)} retry={()=>retryDiscovery(node.id)}/>{/each}
        {/key}
      {/if}

    {/snippet}
  </ProfileCanvas>


</section>
<style>
  .routing-workspace button{font-size:var(--ui-action-size)}
  p{font-size:var(--ui-text-size);color:var(--muted);line-height:var(--ui-line-height);margin:10px 0}

  .authoring-inspector{border:0;margin:0;padding:0;min-width:0}
  .draft-endpoint{min-width:0;display:grid;gap:3px;align-content:start}.draft-chain{min-width:0;display:flex;gap:6px;align-items:center;flex-wrap:wrap}.draft-actions{display:flex;gap:8px;flex-wrap:wrap}
  .connection-warning-slot{display:grid;place-items:center;width:18px;min-height:28px;color:#d3a078}.draft-destination{display:grid;grid-template-columns:minmax(0,1fr) auto;gap:12px;align-items:start;min-width:0}.needs-conversion :global(.port-combobox input){border-color:#d3a078}.delete-draft{display:grid;place-items:center;justify-self:end;width:26px;height:26px;padding:0;border:1px solid #bc3441;border-radius:4px;background:#9b2430;color:#fff;cursor:pointer}.delete-draft:hover:enabled{background:#b32b38;border-color:#e35d69}.delete-draft:focus-visible{outline:2px solid #ff8993;outline-offset:2px}.delete-draft:disabled{opacity:.4;cursor:default}
  .draft-transforms:empty{display:none}.draft-transforms{display:flex;align-items:center;gap:6px;flex-wrap:wrap}.draft-transform{position:relative;max-width:100%}
  .transform-chip{box-sizing:border-box;width:84px;max-width:100%;height:30px;padding:2px 6px;border:1px solid color-mix(in srgb,#9b80d8 40%,var(--line));border-radius:5px;background:color-mix(in srgb,#9b80d8 14%,var(--surface));color:var(--text);display:flex;flex-direction:column;align-items:center;justify-content:center;gap:2px;text-align:center;cursor:pointer}
  .transform-chip strong{width:100%;overflow:hidden;text-overflow:ellipsis;white-space:nowrap}.transform-chip strong{font-size:11px;line-height:12px}.transform-chip:hover{border-color:#bfa1ff}
.add-input{display:inline-flex;align-items:center;justify-self:start;gap:4px;min-height:24px;padding:2px 6px;border:1px solid var(--line);border-radius:4px;background:transparent;color:var(--accent);font:inherit;font-size:11px;cursor:pointer}.add-input:disabled{opacity:.4;cursor:default}
  .source-input-row{min-width:0}
</style>
