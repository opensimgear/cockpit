<script lang="ts">
  import{onDestroy,tick,untrack}from'svelte';
  import ProfileCanvas from './ProfileCanvas.svelte';
  import ProfilePortCombobox from './ProfilePortCombobox.svelte';
  import Icon from './Icon.svelte';
  import ProfileProcessingEditor from './ProfileProcessingEditor.svelte';
  import ProfileVirtualEditor from './ProfileVirtualEditor.svelte';
  import ProfileDesktopEditor from './ProfileDesktopEditor.svelte';
  import type {ProfileDesktopOutput}from'./profile-desktop';
  import {graphSetDesktop}from'./profile-graph';
  import ProfileInputLearning from './ProfileInputLearning.svelte';
  import ProfileCompanionEditor from './ProfileCompanionEditor.svelte';
  import {ProfileLearningController}from'./profile-learning-controller';
  import type{ProfileLearningClient}from'./profile-learning';
  import {canvasNodes,portContractLabel,type CanvasNode,type CanvasPoint} from './profile-canvas';
  import {profilePaletteCanAdd,profilePlaceComponent,type ProfilePaletteComponent,type ProfilePaletteDrag} from './profile-palette';
  import {profileTabDocument,profileTabNodeIds} from './profile-vehicle-tabs';
  import {PROFILE_BLOCK_HEIGHT,PROFILE_BLOCK_WIDTH}from'./profile-editor';
  import {graphAssignResource,graphBypassProcessing,graphConnect,graphConnectionError,graphContractError,graphDisconnect,graphFreshId,graphInsertProcessing,graphInventorySignature,graphPort,graphPorts,graphPublicationCapability,graphRemoveNode,graphRemovePort,graphReorderProcessing,graphSelectProviderPort,graphSetProcessing,graphSetRuntimeOverride,graphSetVirtual,graphSetVisibility,type ProfileGraphDraft}from'./profile-graph';
  import {PROCESSING_PRESETS,processingActionPack,processingNodeError,processingTemplate,type ProcessingPreset}from'./profile-processing';
  import {ProfilePortBrowser}from'./profile-port-browser';
  import {profileDiscoverResource,profileDiscoverCompanion,profileDiscoveredPorts,profilePrepareDiscoveredPorts,profileConnectDiscoveredPorts}from'./profile-discovery';
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
  let message=$state(''),routeOpen=$state(false),fromNode=$state(''),fromPort=$state(''),toNode=$state(''),toPort=$state(''),editingRoute=$state(''),highlighted=$state<string[]>([]),preset=$state<ProcessingPreset>('identity'),chainOrder=$state<string[]>([]);
  let connectionRoute=$state(''),pendingConnection=$state(false);
  let sourceSearch=$state(''),targetSearch=$state('');
  let connectionTrigger:Element|null=null;
  let routingElement:HTMLElement;
  let canvas=$state.raw<{addGroup:(name?:string)=>string|undefined;focusGroup:(id:string)=>void;renameGroup:(id:string,name:string)=>void;deleteGroup:(id:string)=>void}>();
  const connectionUid=$props.id();
  const connectionTitleId=`${connectionUid}-connection`;
  let assignmentFor=$state(''),assignmentSource=$state(''),assignmentPage=$state.raw<DeepReadonly<ResourceDetails>|null>(null),assignmentBusy=$state(false),assignmentError=$state(''),selectedMappings=$state.raw<Record<string,DeepReadonly<ObservedResourcePort>>>({}),assignmentEpoch=0;
  let policyInspectorOpen=$state(untrack(()=>!assignments.some(assignment=>assignment.resourceId===selectedId)));
  const assignmentBrowser=new ProfilePortBrowser(untrack(()=>inventoryClient));
  const companionDiscovery=untrack(()=>companionClient)??new ProfileCompanionAuthoringClient(async request=>(await gatewayRequest(request)).profileCompanionAuthoring);
  let discoveries=$state.raw<Record<string,{signature:string;details:DeepReadonly<ResourceDetails|ProfileCompanionPage>|null;busy:boolean;error:string}>>({});
  let discoveryDisposed=false;
  const learning=untrack(()=>learningClient)?new ProfileLearningController(untrack(()=>learningClient)!):null;
  const visibleDocument=$derived(profileTabDocument(document,activeVehicleTabId)),visibleIds=$derived(profileTabNodeIds(document,activeVehicleTabId));
  const nodes=$derived(canvasNodes(visibleDocument,assignments,inventory)),selected=$derived(nodes.find(node=>node.id===selectedId));
  const graph=$derived({document,assignments});
  const connectionBinding=$derived(pendingConnection?{id:'',from:{nodeId:fromNode,portId:fromPort},to:{nodeId:toNode,portId:toPort}}:visibleDocument.bindings.find(binding=>binding.id===connectionRoute));
  const connectionChoices=$derived.by(()=>connectionBinding?PROCESSING_PRESETS.filter(([kind])=>{try{processingInsertion(connectionBinding.id||null,kind);return true;}catch{return false;}}):[]);
  const discoveredChoices=$derived(Object.fromEntries(nodes.map(node=>[node.id,profileDiscoveredPorts(graph,node.id,discoveries[node.assignment?.sourceId??'']?.details)])));
  const discoveredPorts=$derived(Object.fromEntries(Object.entries(discoveredChoices).map(([id,ports])=>[id,ports.map(port=>({id:port.key,name:port.name,direction:port.direction,contract:port.contract}))])));
  const discoveryBusy=$derived(nodes.filter(node=>discoveries[node.assignment?.sourceId??'']?.busy).map(node=>node.id));
  const discoveryErrors=$derived(Object.fromEntries(nodes.flatMap(node=>discoveries[node.assignment?.sourceId??'']?.error?[[node.id,discoveries[node.assignment!.sourceId].error]]:[])));
  const sourcePorts=$derived((discoveredChoices[fromNode]??[]).filter(port=>port.direction==='output')),targetPorts=$derived((discoveredChoices[toNode]??[]).filter(port=>port.direction==='input'));
  const portOption=(port:(typeof sourcePorts)[number])=>({value:port.key,label:port.name,description:`${port.observed?.descriptor.id??port.key}${port.companionPage?' (selected panel)':port.current?'':' (saved / unavailable)'}`,searchText:`${port.name} ${port.observed?.descriptor.id??port.key} ${port.key}`});
  const sourceOptions=$derived(sourcePorts.map(portOption)),targetOptions=$derived(targetPorts.map(portOption));
  const sourceContract=$derived(sourcePorts.find(port=>port.key===fromPort)?.contract);
  const targetAction=$derived(targetPorts.find(port=>port.key===toPort)?.contract);
  const connectionError=$derived.by(()=>{if(!fromNode||!fromPort||!toNode||!toPort)return'Select an output and an input.';try{const prepared=prepareConnection();return graphConnectionError(prepared.draft.document,prepared.from,prepared.to,editingRoute||undefined);}catch(error){return error instanceof Error?error.message:String(error);}});
  const actionPorts=$derived(nodes.flatMap(node=>graphPorts(document,node.id).filter(port=>port.contract.delivery==='action').map(port=>({label:`${node.name} / ${port.name}`,contract:port.contract}))));
  const assignmentResources=$derived(inventory.filter(resource=>selected?.resource?.kind!=='companion'&&selected?.resource?.kind===(resource.resourceType==='game-integration'?'game':resource.resourceType==='companion'?'companion':'device')));
  const mappingOptions=$derived.by(()=>{const ports=[...(assignmentPage?.ports??[]),...Object.values(selectedMappings)],seen=new Set<string>();return ports.filter(port=>{const key=optionKey(port);if(seen.has(key))return false;seen.add(key);return true;});});
  $effect(()=>{const id=selectedId;if(id!==assignmentFor){assignmentFor=id;policyInspectorOpen=!assignments.some(assignment=>assignment.resourceId===id);assignmentSource='';assignmentPage=null;selectedMappings={};assignmentError='';assignmentBusy=false;assignmentEpoch++;assignmentBrowser.clear();}});
  let previousVehicleTab=untrack(()=>activeVehicleTabId);
  $effect(()=>{if(previousVehicleTab!==activeVehicleTabId){previousVehicleTab=activeVehicleTabId;routeOpen=false;fromNode='';fromPort='';toNode='';toPort='';editingRoute='';connectionRoute='';pendingConnection=false;chainOrder=[];highlighted=[];message='';}});
  $effect(()=>{
    const sources=assignments.filter(assignment=>visibleIds.has(assignment.resourceId)).map(assignment=>({sourceId:assignment.sourceId,resource:inventory.find(resource=>resource.id===assignment.sourceId),companion:document.resources.some(resource=>resource.id===assignment.resourceId&&resource.kind==='companion'),publication:document.companionAssignments.find(item=>item.resourceId===assignment.resourceId)?.publicationHash})),revision=inventoryRevision;
    untrack(()=>refreshDiscoveries(sources,revision));
  });
  onDestroy(()=>{discoveryDisposed=true;assignmentEpoch++;assignmentBrowser.clear();learning?.dispose();});
  function refreshDiscoveries(sources:readonly {sourceId:string;resource?:DeepReadonly<ResourceSummary>;companion:boolean;publication?:string}[],revision:number){
    const wanted=new Set(sources.filter(source=>!source.companion||source.publication).map(source=>source.sourceId));
    discoveries=Object.fromEntries(Object.entries(discoveries).filter(([id])=>wanted.has(id)));
    for(const source of sources){if(!source.resource||source.companion&&!source.publication)continue;const signature=`${revision}/${graphInventorySignature([source.resource])}/${source.publication??''}`;if(discoveries[source.sourceId]?.signature!==signature)void discover(source.sourceId,signature,source.companion?source.publication:undefined);}
  }
  async function discover(sourceId:string,signature:string,publication?:string){
    discoveries={...discoveries,[sourceId]:{signature,details:null,busy:true,error:''}};
    try{const details=publication?await profileDiscoverCompanion(companionDiscovery,sourceId,publication):await profileDiscoverResource(inventoryClient,sourceId);if(!discoveryDisposed&&discoveries[sourceId]?.signature===signature)discoveries={...discoveries,[sourceId]:{signature,details,busy:false,error:''}};}
    catch(error){if(!discoveryDisposed&&discoveries[sourceId]?.signature===signature)discoveries={...discoveries,[sourceId]:{signature,details:null,busy:false,error:error instanceof Error?error.message:String(error)}};}
  }
  function retryDiscovery(nodeId:string){const assignment=assignments.find(item=>item.resourceId===nodeId);if(assignment)void discover(assignment.sourceId,discoveries[assignment.sourceId]?.signature??'retry',document.companionAssignments.find(item=>item.resourceId===nodeId)?.publicationHash);}
  function apply(edit:()=>ProfileGraphDraft,label:string):boolean{if(readOnly)return false;message='';try{change(edit(),label);return true;}catch(error){message=error instanceof Error?error.message:String(error);return false;}}
  function focusRoute(){sourceSearch='';targetSearch='';void tick().then(()=>routingElement?.querySelector<HTMLElement>('.canvas-connection-panel select')?.focus());}
  function source(id:string){fromNode=id;fromPort='';toNode='';toPort='';editingRoute='';routeOpen=true;message='';focusRoute();}
  function destination(id:string){fromNode='';fromPort='';toNode=id;toPort='';editingRoute='';routeOpen=true;message='';focusRoute();}
  function connectBlocks(from:string,to:string){fromNode=from;toNode=to;fromPort='';toPort='';editingRoute='';routeOpen=true;message='';focusRoute();}
  function closeRoute(){if(!routeOpen)return;routeOpen=false;selectedId='';void tick().then(()=>routingElement?.querySelector<HTMLElement>('[aria-label="Profile canvas"]')?.focus());}
  function routeKeyboard(event:KeyboardEvent){if(routeOpen&&event.key==='Escape'&&!connectionBinding){event.preventDefault();event.stopPropagation();closeRoute();}}
  function selectedEndpoints(){const from=sourcePorts.find(port=>port.key===fromPort),to=targetPorts.find(port=>port.key===toPort);if(!from||!to)throw new Error('Select an output and an input.');return[{nodeId:fromNode,port:from},{nodeId:toNode,port:to}] as const;}
  function prepareConnection(){const[from,to]=selectedEndpoints();return profilePrepareDiscoveredPorts(graph,from,to);}
  function connect(){if(apply(()=>{const[from,to]=selectedEndpoints();return profileConnectDiscoveredPorts(graph,from,to,editingRoute||undefined);},editingRoute?'Edit exact route':'Connect exact ports'))closeRoute();}
  function editRoute(id:string){const binding=visibleDocument.bindings.find(binding=>binding.id===id);if(!binding)return;editingRoute=id;fromNode=binding.from.nodeId;fromPort=binding.from.portId;toNode=binding.to.nodeId;toPort=binding.to.portId;routeOpen=true;highlighted=[id];focusRoute();}
  function selectPort(nodeId:string,port:DeepReadonly<ObservedResourcePort>){const assignment=assignments.find(assignment=>assignment.resourceId===nodeId);if(!assignment)return;apply(()=>graphSelectProviderPort(graph,nodeId,assignment.sourceId,port),'Select exact provider port');}
  function placeComponent(next:ProfileGraphDraft,id:string,position?:CanvasPoint):ProfileGraphDraft{return{...next,document:profilePlaceComponent(document,next.document,id,position,activeVehicleTabId)};}
  export function addGroup(){return canvas?.addGroup();}
  export function focusGroup(id:string){canvas?.focusGroup(id);}
  export function renameGroup(id:string,name:string){canvas?.renameGroup(id,name);}
  export function deleteGroup(id:string){canvas?.deleteGroup(id);}
  export function addVirtual(position?:CanvasPoint){const id=graphFreshId(document,'virtual');apply(()=>{if(!profilePaletteCanAdd({kind:'virtual'},document,assignments,inventory,activeVehicleTabId))throw new Error('A profile supports at most eight active virtual controllers.');return placeComponent(graphSetVirtual(graph,{id,name:'Virtual controller',required:false,layout:{buttons:['button-1','button-2','button-3','button-4'],axes:[],hats:[]}},activeVehicleTabId),id,position);},'Add virtual controller');}
  export function addDesktop(position?:CanvasPoint){if(!profilePaletteCanAdd({kind:'desktop'},document,assignments,inventory,activeVehicleTabId))return;const id=graphFreshId(document,'desktop');apply(()=>placeComponent(graphSetDesktop(graph,{id,name:'Keyboard and mouse',required:false,scope:{kind:'selected-game'},ports:[],actions:[{portId:'action-1',action:{kind:'key-hold',key:44}}]},activeVehicleTabId),id,position),'Add keyboard and mouse endpoint');}
  function updateDesktop(output:ProfileDesktopOutput){apply(()=>graphSetDesktop(graph,output),'Edit typed desktop actions');}
  function processingInsertion(bindingId:string|null,kind:ProcessingPreset){
    const prepared=bindingId?null:prepareConnection(),working=prepared?.draft??graph;
    const binding=bindingId?document.bindings.find(binding=>binding.id===bindingId):{id:'',from:prepared!.from,to:prepared!.to};
    if(!binding)throw new Error('Choose an existing connection.');
    const input=graphPort(working.document,binding.from)?.contract,target=graphPort(working.document,binding.to)?.contract;
    if(!input||!target)throw new Error('This connection no longer has both declared ports.');
    const node=processingTemplate(kind,graphFreshId(document,'transform'),input,target);
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
  function openConnection(id:string){if(!visibleDocument.bindings.some(binding=>binding.id===id))return;connectionTrigger=typeof window==='undefined'?null:window.document.activeElement;pendingConnection=false;connectionRoute=id;preset='identity';message='';}
  function openPendingConnection(){if(!sourceContract||!targetAction||editingRoute)return;connectionTrigger=typeof window==='undefined'?null:window.document.activeElement;pendingConnection=true;connectionRoute='';message='';preset=connectionChoices[0]?.[0]??'identity';}
  function closeConnection(){connectionRoute='';pendingConnection=false;void tick().then(()=>{if(connectionTrigger?.isConnected&&(connectionTrigger instanceof HTMLElement||connectionTrigger instanceof SVGElement))connectionTrigger.focus();else routingElement?.querySelector<HTMLElement>('[aria-label="Profile canvas"]')?.focus();});}
  function showConnection(node:HTMLDialogElement){node.showModal();return()=>node.close();}
  function insertTransformation(){if(!connectionBinding)return;let selected='';if(apply(()=>{const insertion=processingInsertion(connectionBinding.id||null,preset);selected=insertion.node.id;return insertion.next;},pendingConnection?'Connect through transformation':'Insert transformation')){selectedId=selected;highlighted=[];routeOpen=false;editingRoute='';closeConnection();}}
  function actionPack(mode:'action-pack'|'set-on-change'){if(targetAction?.delivery!=='action')return;apply(()=>{const input=sourcePorts.find(port=>port.key===fromPort)?.contract,node=processingActionPack(graphFreshId(document,'transform'),targetAction,mode,input?.delivery==='event'?input:undefined),next=graphSetProcessing(graph,node,activeVehicleTabId);selectedId=node.id;return next;},'Add explicit action arguments');}
  function updateProcessing(node:ProfileProcessingNode){apply(()=>{const error=processingNodeError(node);if(error)throw new Error(error);return graphSetProcessing(graph,node);},'Edit visible transformation');}
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
  {#if connectionBinding}<dialog class="connection-dialog" aria-labelledby={connectionTitleId} oncancel={event=>{event.preventDefault();closeConnection();}} {@attach showConnection}>
    <div class="route-heading"><h3 id={connectionTitleId}>Connection transformation</h3><button class="text-button" aria-label="Close connection transformations" onclick={closeConnection}>Close</button></div>
    <p>{nodes.find(node=>node.id===connectionBinding.from.nodeId)?.name??connectionBinding.from.nodeId} · {connectionBinding.from.portId} → {nodes.find(node=>node.id===connectionBinding.to.nodeId)?.name??connectionBinding.to.nodeId} · {connectionBinding.to.portId}</p>
    <p>Insert a visible transformation between these exact ports. Both connections keep their declared types.</p>
    {#if connectionChoices.length}<label>Transformation<select aria-label="Connection transformation" bind:value={preset} disabled={readOnly}>{#each connectionChoices as [kind,label]}<option value={kind}>{label}</option>{/each}</select></label><button class="primary-button" disabled={readOnly||!connectionChoices.some(([kind])=>kind===preset)} onclick={insertTransformation}>Insert transformation</button>
    {:else}<p>No transformation can be inserted while preserving these contracts. Action arguments are configured upstream of their invocation connection.</p>{/if}
    {#if message}<p class="routing-error" role="alert">{message}</p>{/if}
    <button class="secondary-button" disabled={readOnly} onclick={()=>{const id=connectionBinding!.id;closeConnection();if(id)editRoute(id);}}>Edit exact ports</button>
  </dialog>{/if}
  <ProfileCanvas bind:this={canvas} companionAuthoring {document} {activeVehicleTabId} {assignments} {inventory} {inventoryClient} {readOnly} bind:selectedId {openGameSettings} {paletteDrag} {canDropComponent} {dropComponent} {endPaletteDrag} change={(next,label)=>change({document:next,assignments:copy(assignments) as ProfileLocalAssignment[]},label??'Edit canvas layout')} {connectBlocks} connectionFrom={source} connectionTo={destination} editConnection={openConnection} selectProviderPort={selectPort} removeProviderPort={(nodeId,portId)=>apply(()=>graphRemovePort(graph,nodeId,portId),'Remove selected port and routes')} highlightedEdges={highlighted} diagnostics={[]} {undo} {redo} {discoveredPorts} {discoveryBusy} {discoveryErrors} connectionOpen={routeOpen} closeConnectionPanel={closeRoute}>
    {#snippet connectionPanel()}
      <section class="route-composer" aria-label="Choose connection ports">
        <div class="route-heading"><h3>{editingRoute?'Edit connection':'Connect blocks'}</h3><button class="text-button" aria-label="Close connection editor" onclick={closeRoute}><Icon name="close" size={18}/></button></div>
        <p>Select an output and an input to connect these blocks.</p>
        <div class="connection-fields">
          <label>Source block<select aria-label="Source block" value={fromNode} disabled={readOnly} onchange={event=>{fromNode=event.currentTarget.value;fromPort='';sourceSearch='';}}><option value="">Choose source</option>{#each nodes.filter(node=>node.kind!=='virtual'&&node.kind!=='desktop') as node (node.id)}<option value={node.id}>{node.name}</option>{/each}</select></label>
          <ProfilePortCombobox label="Find an output" placeholder="Search outputs by name or ID" options={sourceOptions} bind:value={fromPort} bind:query={sourceSearch} disabled={readOnly||!fromNode}/>
          {#if fromNode}<p class="port-choice-count">{sourcePorts.length} outputs discovered</p>{/if}
          {#if discoveryBusy.includes(fromNode)}<p role="status">Discovering outputs…</p>{/if}
          {#if discoveryErrors[fromNode]}<p class="routing-error" role="status">{discoveryErrors[fromNode]}</p><button class="text-button" onclick={()=>retryDiscovery(fromNode)}>Retry output discovery</button>{:else if fromNode&&!discoveryBusy.includes(fromNode)&&!sourcePorts.length}<p>No outputs available on this block.</p>{/if}
          <label>Destination block<select aria-label="Destination block" value={toNode} disabled={readOnly} onchange={event=>{toNode=event.currentTarget.value;toPort='';targetSearch='';}}><option value="">Choose destination</option>{#each nodes as node (node.id)}<option value={node.id}>{node.name}</option>{/each}</select></label>
          <ProfilePortCombobox label="Find an input" placeholder="Search inputs by name or ID" options={targetOptions} bind:value={toPort} bind:query={targetSearch} disabled={readOnly||!toNode}/>
          {#if toNode}<p class="port-choice-count">{targetPorts.length} inputs / actions discovered</p>{/if}
          {#if discoveryBusy.includes(toNode)}<p role="status">Discovering inputs…</p>{/if}
          {#if discoveryErrors[toNode]}<p class="routing-error" role="status">{discoveryErrors[toNode]}</p><button class="text-button" onclick={()=>retryDiscovery(toNode)}>Retry input discovery</button>{:else if toNode&&!discoveryBusy.includes(toNode)&&!targetPorts.length}<p>No inputs available on this block.</p>{/if}
        </div>
        {#if sourceContract}<p>{portContractLabel(sourceContract)}</p>{/if}{#if targetAction}<p>{portContractLabel(targetAction)}</p>{/if}
        <p class:connection-problem={!!connectionError} role="status">{connectionError||'Ready to connect.'}</p>
        {#if message}<p class="routing-error" role="alert">{message}</p>{/if}
        <div class="connection-actions"><button class="secondary-button" onclick={closeRoute}>Cancel</button><button class="primary-button" disabled={readOnly||!!connectionError} onclick={connect}>{editingRoute?'Update exact route':'Create exact route'}</button></div>
        {#if !editingRoute&&sourceContract&&targetAction}<button class="secondary-button" disabled={readOnly} onclick={openPendingConnection}>Connect with transformation</button>{/if}
        {#if targetAction?.delivery==='action'}<details><summary>Action arguments</summary><div class="argument-actions"><button class="secondary-button" disabled={readOnly} onclick={()=>actionPack('action-pack')}>Add event-triggered argument pack</button>{#if targetAction.semantics==='setter'}<button class="secondary-button" disabled={readOnly} onclick={()=>actionPack('set-on-change')}>Add setter on argument change</button>{/if}<p>Map each named argument into the new node, then connect its invocation output to this action.</p></div></details>{/if}
      </section>
    {/snippet}
    {#snippet inspector(node)}
      <fieldset class="authoring-inspector" disabled={readOnly}>
        {#if node.resource}<details bind:open={policyInspectorOpen}><summary>Resource assignment and policy</summary><label>Block label<input value={node.resource.name} maxlength="80" onchange={event=>resourcePolicy(node,'name',event.currentTarget.value)}/></label><label class="check"><input type="checkbox" checked={node.resource.required} onchange={event=>resourcePolicy(node,'required',event.currentTarget.checked)}/> Required resource</label>
          {#if node.resource.kind==='device'}<label>Game visibility intent<select value={document.visibility.find(intent=>intent.resourceId===node.id)?.mode??'visible'} onchange={event=>apply(()=>graphSetVisibility(graph,node.id,event.currentTarget.value==='hidden-while-active'?'hidden-while-active':'visible'),'Edit visibility intent')}><option value="visible">Visible</option><option value="hidden-while-active">Hidden while profile is active</option></select></label>{/if}
          {#if node.resource.kind==='device'}{@const publication=graphPublicationCapability(node.observation)}<label>Controller publication policy<select value={document.overrides.some(policy=>policy.resourceId===node.id)?document.overrides.find(policy=>policy.resourceId===node.id)!.enabled?'enabled':'disabled':'configured'} onchange={event=>publicationPolicy(node,event.currentTarget.value)}><option value="configured">Keep configured</option><option value="enabled" disabled={publication.status==='unsupported'}>Enable while active</option><option value="disabled" disabled={publication.status==='unsupported'}>Disable while active</option></select></label><p class="publication-status" role="status">{publication.message}{#if publication.configuredEnabled!==null} Configured controller publication: {publication.configuredEnabled?'enabled':'disabled'}.{/if}</p><p class="publication-warning">Enabling or disabling publication can change USB controller collections, require a USB reconnect and require game rebinding. This policy does not save device flash. Controller publication and game hiding are separate.</p>{/if}
          {#if node.resource.kind!=='companion'}<p>{node.assignment?'Assigned locally; replacing it requires an exact match for every selected port.':'Unassigned. Choose a matching inventory resource; copied graphs are never resolved by name.'}</p><label>Inventory resource<select value={assignmentSource} onchange={event=>{assignmentSource=event.currentTarget.value;if(assignmentSource)void browseAssignment('open');}}><option value="">Choose resource explicitly</option>{#each assignmentResources as resource (resource.id)}<option value={resource.id}>{resource.name} · {resource.availability}</option>{/each}</select></label>
          {#if assignmentBusy}<p role="status">Loading provider port page…</p>{/if}{#if assignmentError}<p class="routing-error" role="status">{assignmentError}</p>{/if}
          {#if assignmentSource}{#each node.resource.ports as port (port.id)}<label>Logical {port.id} → provider port<select value={selectedMappings[port.id]?optionKey(selectedMappings[port.id]):''} onchange={event=>chooseMapping(port.id,event.currentTarget.value)}><option value="">Choose exact matching port</option>{#each mappingOptions.filter(observed=>observed.descriptor.direction===port.direction) as observed (optionKey(observed))}<option value={optionKey(observed)}>{observed.descriptor.name} · {observed.descriptor.id} {observed.current?'':'(cached)'}</option>{/each}</select><small>{portContractLabel(port.contract)}</small></label>{/each}{#if assignmentPage}<p>{assignmentPage.offset+1}–{assignmentPage.offset+assignmentPage.ports.length} of {assignmentPage.total} cached contracts</p><div class="small-actions"><button class="text-button" disabled={assignmentBusy||!assignmentBrowser.hasPrevious} onclick={()=>browseAssignment('previous')}>Previous ports</button><button class="text-button" disabled={assignmentBusy||!assignmentBrowser.hasNext} onclick={()=>browseAssignment('next')}>Next ports</button></div>{/if}<button class="secondary-button" disabled={node.resource.ports.some(port=>!selectedMappings[port.id])||assignmentBusy} onclick={assignResource}>Assign resource and exact ports</button>{/if}
          {/if}
        </details>{/if}
        {#if node.processing}<ProfileProcessingEditor node={node.processing} change={updateProcessing} actions={actionPorts}/>{#if node.processing.inputs.length===1&&node.processing.outputs.length===1}<button class="text-button" onclick={()=>apply(()=>graphBypassProcessing(graph,node.id),'Remove transformation and join compatible route')}>Remove transformation and join compatible route</button>{/if}{/if}
        {#if node.virtual}<ProfileVirtualEditor output={node.virtual} change={updateVirtual}/>{/if}
        {#if node.desktop}<ProfileDesktopEditor output={node.desktop} change={updateDesktop}/>{/if}
        <p class="remove-help">Removing this block also removes {document.bindings.filter(binding=>binding.from.nodeId===node.id||binding.to.nodeId===node.id).length} exact routes.</p><button class="text-button" onclick={()=>removeNode(node.id)}>Remove block and its routes</button>
      </fieldset>
      {#if node.resource?.kind==='companion'}<ProfileCompanionEditor {graph} {inventory} resourceId={node.id} {readOnly} {active} client={companionDiscovery} {change}/>{/if}
      {#if learning&&node.resource&&node.assignment&&node.observation?.resourceType==='host-controller'}<ProfileInputLearning controller={learning} resource={node.observation} context={`${editorContext}/${node.id}/${node.assignment.sourceId}`} {active} disabled={readOnly} {runtimeIdle} choose={port=>selectPort(node.id,port)}/>{/if}
    {/snippet}
  </ProfileCanvas>
  <section class="bindings-panel panel" aria-label="Exact bindings table"><div class="route-heading"><h3>Exact signal bindings · {visibleDocument.bindings.length}</h3><button class="text-button" onclick={()=>highlighted=[]}>Clear route highlight</button></div><p>Each row is one directed edge in this graph. Choose a row to inspect or retarget exact ports; insert a visible transformation using the selected preset.</p><div class="bindings-scroll"><table><thead><tr><th>Route</th><th>Source output</th><th>Destination input / action</th><th>Graph editing</th></tr></thead><tbody>{#each visibleDocument.bindings as binding (binding.id)}<tr class:highlighted={highlighted.includes(binding.id)}><td><button class="text-button" onclick={()=>editRoute(binding.id)}>{binding.id}</button></td><td><button class="endpoint-button" onclick={()=>selectedId=binding.from.nodeId}>{nodes.find(node=>node.id===binding.from.nodeId)?.name??binding.from.nodeId}<code>{binding.from.portId}</code></button></td><td><button class="endpoint-button" onclick={()=>selectedId=binding.to.nodeId}>{nodes.find(node=>node.id===binding.to.nodeId)?.name??binding.to.nodeId}<code>{binding.to.portId}</code></button></td><td><div class="small-actions"><button class="text-button" disabled={readOnly} onclick={()=>editRoute(binding.id)}>Edit ports</button><button class="text-button" onclick={()=>openConnection(binding.id)}>Transform connection</button><button class="text-button" disabled={readOnly} onclick={()=>apply(()=>graphDisconnect(graph,binding.id),'Remove exact route')}>Remove</button></div></td></tr>{:else}<tr><td colspan="4">No routes yet. Drag between block inputs and outputs to choose a connection.</td></tr>{/each}</tbody></table></div>
    {#if visibleDocument.processingNodes.length>=2}<details><summary>Reorder a linear transformation chain</summary><p>Select a connected chain in the desired order. Each selected block must have one input and one output, without branches. Incompatible orders are rejected.</p><div class="chain-options">{#each visibleDocument.processingNodes.filter(node=>node.inputs.length===1&&node.outputs.length===1) as node (node.id)}<button class="secondary-button" class:chosen={chainOrder.includes(node.id)} disabled={readOnly} onclick={()=>chainOrder=chainOrder.includes(node.id)?chainOrder.filter(id=>id!==node.id):[...chainOrder,node.id]}>{chainOrder.includes(node.id)?`${chainOrder.indexOf(node.id)+1}. `:''}{node.name}</button>{/each}</div><button class="secondary-button" disabled={readOnly||chainOrder.length<2} onclick={()=>apply(()=>graphReorderProcessing(graph,chainOrder),'Reorder visible transformation chain')}>Use selected chain order</button></details>{/if}
  </section>
</section>
<style>
  .connection-dialog{position:fixed;inset:0;margin:auto;width:min(520px,calc(100vw - 40px));max-height:80dvh;overflow:auto;box-sizing:border-box;padding:24px;border:1px solid var(--line);border-radius:10px;background:var(--surface);color:var(--text);box-shadow:0 20px 80px #0005}.connection-dialog::backdrop{background:#0009}.connection-dialog .route-heading h3{margin:0}.connection-dialog>button{margin:12px 8px 0 0}.connection-dialog label{margin:16px 0}.connection-dialog select{font:inherit;width:100%;padding:8px;border:1px solid var(--line);border-radius:5px;background:var(--input-bg);color:var(--text)}
  .routing-workspace button{font-size:10px}.routing-workspace select,.routing-workspace input:not([type="checkbox"]){background:var(--input-bg);color:var(--text);border:1px solid var(--line);border-radius:5px;padding:8px;font-size:10px;min-width:0;width:100%;box-sizing:border-box}.route-composer{padding:0;margin:0}.bindings-panel{padding:18px;margin:16px 0}.connection-actions{display:flex;gap:8px;justify-content:flex-end;margin:16px 0}.port-choice-count{margin:0;font-size:9px}.route-heading{display:flex;align-items:center;justify-content:space-between;gap:12px}h3{font-size:13px;color:var(--soft)}p{font-size:10px;color:var(--muted);line-height:1.8;margin:10px 0}.connection-fields{display:grid;grid-template-columns:minmax(0,1fr);gap:12px;margin:15px 0}label{font-size:9px;color:var(--muted);display:grid;gap:7px}.connection-problem,.routing-error{color:#d3a078}.routing-error{padding:12px;border:1px solid #ad875440;border-radius:6px;font-size:10px;line-height:1.8}.argument-actions{margin-top:15px;border-top:1px solid var(--line);padding-top:14px;display:flex;gap:9px;flex-wrap:wrap}.argument-actions p{flex-basis:100%}.authoring-inspector{border:0;margin:0;padding:0;min-width:0}.authoring-inspector label{margin:12px 0}.authoring-inspector details{margin:16px 0;border-top:1px solid var(--line);padding-top:14px}.authoring-inspector summary{font-size:10px;color:var(--soft);cursor:pointer}.authoring-inspector small{font-size:9px;line-height:1.8}.check{display:flex;align-items:center;gap:8px}.small-actions{display:flex;gap:12px;align-items:center;flex-wrap:wrap}.bindings-scroll{overflow:auto;max-height:460px}table{width:100%;border-collapse:collapse;font-size:10px;text-align:left}th{font-size:9px;font-weight:400;color:var(--muted);padding:12px;border-bottom:1px solid var(--line)}td{padding:12px;border-bottom:1px solid var(--faint-line);color:var(--soft);vertical-align:top}tr.highlighted{background:var(--accent-bg)}.endpoint-button{display:grid;gap:7px;background:none;color:var(--soft);padding:0;text-align:left;max-width:220px;overflow-wrap:anywhere}.endpoint-button code{font-size:9px;color:var(--muted)}.bindings-panel details{margin-top:16px}.bindings-panel summary{font-size:10px;color:var(--soft);cursor:pointer}.chain-options{display:flex;gap:8px;flex-wrap:wrap;margin:12px 0}.chain-options .chosen{border-color:var(--accent);color:var(--accent)}@media(max-width:700px){.connection-fields{grid-template-columns:1fr}.route-heading{align-items:start;flex-direction:column}}
</style>
