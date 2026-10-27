<script lang="ts">
  import CanvasViewControls from './CanvasViewControls.svelte';
  import { onDestroy, onMount, tick, untrack, type Snippet } from 'svelte';
  import Icon from './Icon.svelte';
  import {canvasOverviewLayoutScore} from './profile-overview-layout';
  import ProfileSignalLanes from './ProfileSignalLanes.svelte';
  import {signalOverview,signalBundleWidth} from './profile-signal-overview';
  import type { DeepReadonly, ProfileDocument, ProfileEndpoint, ProfileLocalAssignment, ProfilePort, ProfilePortGroup } from './profiles';
  import type { ObservedResourcePort, ProfileInventoryClient, ResourceDetails, ResourceSummary } from './profile-inventory';
  import type { GameId } from './games';
  import { PROFILE_BLOCK_HEIGHT, PROFILE_BLOCK_WIDTH } from './profile-editor';
  import { canvasControllerEffects, canvasCreateGroup, canvasDeleteGroup, canvasFit, canvasGameId, canvasGroupAt, canvasGroups, canvasFocusNode, canvasMoveGroup, canvasMoveIntoGroup, canvasNodes, canvasNodeSize, canvasNodeAnchor, PROFILE_TRANSFORM_HEADER, PROFILE_TRANSFORM_PORT_ROW, canvasPan, canvasPortCollapsed, canvasRenameGroup, canvasResizeGroup, canvasSearch, canvasStatus, canvasTogglePorts, canvasTransports, canvasZoom, portContractLabel, type CanvasGroup, type CanvasNode, type CanvasPoint, type CanvasViewport } from './profile-canvas';
  import { createDiagramRouter, diagramPendingPath, diagramRoutes, diagramSegmentLine } from './profile-diagram';
  import {canvasRouteNodes,canvasOverviewRouteNodes,canvasPreviewRoutes,bundleDiagramRoutes,buildDiagramWireTrunks,type DiagramWireBundle}from'./profile-canvas-render';
  import {profileReshapeWireSegment,profileSetWireRoute,profileInsertWireHandle,profileMoveWireHandle,profileRemoveWireHandle,profileWireHandleOnRoute}from'./profile-wire-layout';
  import {canvasStyling}from'./canvas-styling';
  import {canvasNavigation,canvasWheelNavigation}from'./canvas-navigation';
  import {inputAvailability}from'./profile-input-availability';
  import {processingSummary} from './profile-processing';
  import type{CompileDiagnostic}from'./profile-compilation';
  import { ProfilePortBrowser } from './profile-port-browser';
  import type { ProfilePaletteComponent, ProfilePaletteDrag } from './profile-palette';
  import {profileTabDocument,profileTabNodeIds} from './profile-vehicle-tabs';
  let { document, assignments, inventory, inventoryClient, activeVehicleTabId = '', selectedId = $bindable(''), companionAuthoring = false, simpleControllerInspector = false, readOnly = false, fitOnMount = true, change, openGameSettings, inspector, nodeControls, selectProviderPort, removeProviderPort, connectBlocks, connectionFrom, connectionTo, editConnection, selectedEdgeId = '', selectConnection, reconnectConnection, disconnectConnection, connectionSourceId = '', connectionTargetId = '', connectionSourcePort = '', connectionTargetPort = '', reconnectingBindingId = '', dropOnConnection, highlightedEdges = [], diagnostics = [], undo, redo, paletteDrag = null, canDropComponent, dropComponent, endPaletteDrag, discoveredPorts = {}, discoveryBusy = [], discoveryErrors = {}, connectionPanel, connectionOpen = false, closeConnectionPanel, transformSettings, insertTransform, transferTransform, routeEndpoint }: {
    document: DeepReadonly<ProfileDocument>; assignments: readonly DeepReadonly<ProfileLocalAssignment>[]; inventory: readonly DeepReadonly<ResourceSummary>[];
    inventoryClient: Pick<ProfileInventoryClient, 'inspect'>; activeVehicleTabId?:string; selectedId?: string; companionAuthoring?:boolean; simpleControllerInspector?:boolean; readOnly?: boolean; fitOnMount?: boolean;
    change: (document: ProfileDocument, label?:string) => void; openGameSettings?: (gameId: GameId, returnState?: unknown) => void;
    inspector?:Snippet<[CanvasNode]>;nodeControls?:Snippet<[CanvasNode]>; selectProviderPort?:(nodeId:string,port:DeepReadonly<ObservedResourcePort>)=>void; removeProviderPort?:(nodeId:string,portId:string)=>void;
    connectBlocks?:(fromNodeId:string,toNodeId:string,position?:CanvasPoint,fromPortId?:string,toPortId?:string)=>void;connectionFrom?:(nodeId:string,position?:CanvasPoint,portId?:string)=>void;connectionTo?:(nodeId:string,position?:CanvasPoint,portId?:string)=>void;highlightedEdges?:readonly string[];undo?:()=>void;redo?:()=>void;
    editConnection?:(bindingId:string)=>void;
    selectedEdgeId?:string;selectConnection?:(bindingId:string)=>void;connectionSourceId?:string;connectionTargetId?:string;connectionSourcePort?:string;connectionTargetPort?:string;reconnectingBindingId?:string;
    routeEndpoint?:Snippet<[string,'from'|'to',DeepReadonly<ProfileEndpoint>]>;
    reconnectConnection?:(bindingId:string,side:'from'|'to',nodeId:string,position?:CanvasPoint,portId?:string)=>void;disconnectConnection?:(bindingId:string)=>void;
    dropOnConnection?:(component:ProfilePaletteComponent,bindingId:string,position:CanvasPoint)=>void;
    diagnostics?:readonly DeepReadonly<CompileDiagnostic>[];
    paletteDrag?:ProfilePaletteDrag|null;canDropComponent?:(component:ProfilePaletteComponent)=>boolean;
    dropComponent?:(component:ProfilePaletteComponent,position:CanvasPoint)=>void;endPaletteDrag?:()=>void;
    discoveredPorts?:Record<string,readonly DeepReadonly<ProfilePort>[]>;discoveryBusy?:readonly string[];discoveryErrors?:Record<string,string>;
    connectionPanel?:Snippet;connectionOpen?:boolean;closeConnectionPanel?:()=>void;
    transformSettings?:Snippet<[CanvasNode]>;insertTransform?:(bindingId:string)=>void;transferTransform?:(sourceLane:string,nodeId:string,targetLane:string,before:string|null)=>boolean|void;
  } = $props();
  let stage: HTMLDivElement;
  let inspectorPanel=$state.raw<HTMLElement|undefined>();
  let mounted=$state(false);
  let selectionView:CanvasViewport|null=null,focusedSelectionId='';
  const inspectorContentId = $props.id();
  let search = $state('');
  let pendingOverviewDrop=$state<{component:ProfilePaletteComponent;position:CanvasPoint}|null>(null);
  const overview=true;
  let overviewBundleId=$state(''),showTransforms=$state(false),activeTransform=$state('');
  let searchInput: HTMLInputElement;
  let view = $state<CanvasViewport>({ ...untrack(() => document.editor.viewport) });
  let dragged = $state<{id:string;position:CanvasPoint}|null>(null);
  let wire=$state<{nodeId:string;portId?:string;direction:'input'|'output';pointer:number;start:CanvasPoint;end:CanvasPoint;moved:boolean;reconnect?:{bindingId:string;side:'from'|'to'}}|null>(null);
  let wireTargetId=$state(''),wireTargetPort=$state(''),hoveredEdgeId=$state(''),paletteConnectionId=$state('');
  let wireRejectedId=$state(''),wireRejectedPort=$state(''),wireRejectedReason=$state('');
  let segmentDrag=$state.raw<{bindingId:string;index:number;handleIndex?:number;points:CanvasPoint[];source:DeepReadonly<ProfileDocument>;pointer:number;start:CanvasPoint;moved:boolean}|null>(null);
  let segmentPreview=$state<ProfileDocument|null>(null),segmentBlocked=$state(false);
  let suppressWireClick=false;
  let connectionClickTimer:ReturnType<typeof setTimeout>|undefined;
  let bundlePicker=$state.raw<{ids:string[];x:number;y:number;width:number;maxHeight:number;insertion?:{component:ProfilePaletteComponent;position:CanvasPoint}}|null>(null);
  let palettePreview=$state<CanvasPoint|null>(null);
  let page = $state<DeepReadonly<ResourceDetails>|null>(null), portsBusy = $state(false), portError = $state('');
  let layoutError=$state(''),layoutBusy=$state(false),layoutEpoch=0;
  let groupPreview=$state<ProfileDocument|null>(null),groupBlocked=$state(false);
  const portCategories:readonly ProfilePortGroup[]=['outputs','inputs','actions'];
  type CanvasDrag = { id?: string; group?: {id:string;mode:'move'|'resize';frame:CanvasGroup;source:DeepReadonly<ProfileDocument>}; origin: CanvasPoint; start: CanvasPoint; viewport: CanvasViewport; pointer: number; moved: boolean };
  let drag = $state.raw<CanvasDrag|null>(null);
  let moveFrame=0,pendingMove:PointerEvent|null=null;
  const browser = new ProfilePortBrowser(untrack(() => inventoryClient));
  const router=createDiagramRouter();
  const committedDocument=$derived(profileTabDocument(document,activeVehicleTabId));
  const visibleDocument=$derived(segmentPreview||groupPreview?profileTabDocument((segmentPreview??groupPreview)!,activeVehicleTabId):committedDocument);
  const allNodes=$derived(canvasNodes(visibleDocument,assignments,inventory));
  const committedNodes=$derived(canvasNodes(committedDocument,assignments,inventory).filter(node=>!overview||!node.processing));
  const overviewBundles=$derived(signalOverview(committedDocument));
  const overviewById=$derived(new Map(overviewBundles.map(bundle=>[bundle.id,bundle])));
  const overviewBundle=$derived(overviewById.get(overviewBundleId));
  const routingBindings=$derived(overview?overviewBundles.map(bundle=>({...bundle.binding,to:{...bundle.binding.to,portId:bundle.id}})):committedDocument.bindings);
  const maximumRoutes=$derived(Math.max(1,...overviewBundles.map(bundle=>bundle.lanes.length)));
  const nodes = $derived(visibleDocument===committedDocument?committedNodes:allNodes.filter(node=>!overview||!node.processing));
  const nodesById=$derived(new Map(nodes.map(node=>[node.id,node])));
  const portGroups=$derived(new Map(nodes.map(node=>{const ports=node.resource?discoveredPorts[node.id]??node.resource.ports:null;const inputs=ports?ports.filter(port=>port.direction==='input'):node.processing?.inputs??node.virtual?.ports??node.desktop?.ports??[],outputs=ports?ports.filter(port=>port.direction==='output'):node.processing?.outputs??[];return[node.id,{inputs,outputs,names:new Map([...inputs,...outputs].map(port=>[`${port.direction}/${port.id}`,port.name]))}];})));
  let previousVehicleTab=untrack(()=>activeVehicleTabId);
  const controllerEffects=$derived(canvasControllerEffects(visibleDocument));
  const renderedNodes = $derived(nodes.map(node => node.id === dragged?.id ? { ...node, position: dragged.position } : node));
  const wireStyle=$derived($canvasStyling.arrowStyle);
  const cornerRadius=$derived($canvasStyling.cornerRadius);
  const committedRouteNodes=$derived(overview?canvasOverviewRouteNodes(canvasRouteNodes(committedNodes),routingBindings):canvasRouteNodes(committedNodes));
  const routeNodes=$derived(overview?canvasOverviewRouteNodes(canvasRouteNodes(renderedNodes),routingBindings):canvasRouteNodes(renderedNodes));
  const committedRoutes=$derived(router(committedRouteNodes,routingBindings,PROFILE_BLOCK_WIDTH,PROFILE_BLOCK_HEIGHT,{style:wireStyle,cornerRadius,wireRoutes:overview?undefined:committedDocument.editor.wireRoutes,clearance:overview?36:undefined}));
  const routes=$derived.by(()=>{
    if(segmentPreview&&segmentDrag){const preview=diagramRoutes(routeNodes,visibleDocument.bindings.filter(binding=>binding.id===segmentDrag!.bindingId),PROFILE_BLOCK_WIDTH,PROFILE_BLOCK_HEIGHT,{style:wireStyle,cornerRadius,wireRoutes:visibleDocument.editor.wireRoutes})[0];return preview?committedRoutes.map(route=>route.binding.id===preview.binding.id?preview:route):committedRoutes;}
    return dragged||groupPreview?canvasPreviewRoutes(committedRoutes,routeNodes,PROFILE_BLOCK_WIDTH,PROFILE_BLOCK_HEIGHT,wireStyle,cornerRadius):committedRoutes;
  });
  const bundledRoutes=$derived(bundleDiagramRoutes([...routes.filter(route=>route.binding.id!==selectedEdgeId),...routes.filter(route=>route.binding.id===selectedEdgeId)],selectedEdgeId));
  const wireNetwork=$derived(!overview?buildDiagramWireTrunks(bundledRoutes,routeNodes,PROFILE_BLOCK_WIDTH,PROFILE_BLOCK_HEIGHT,wireStyle,cornerRadius,!!dragged||!!groupPreview,selectedEdgeId):{bundles:bundledRoutes,trunks:[]});
  const renderedRoutes=$derived(wireNetwork.bundles);
  const sharedTrunks=$derived(wireNetwork.trunks);
  const clickableBundles=$derived([...renderedRoutes,...sharedTrunks]);
  const trunkBadges=$derived.by(()=>{
    const groups=new Map<string,typeof sharedTrunks[number]>();
    for(const trunk of sharedTrunks){const binding=trunk.route.binding,key=JSON.stringify([trunk.direction,trunk.direction==='output'?binding.from.nodeId:binding.to.nodeId,trunk.endpoint.x,trunk.endpoint.y]),previous=groups.get(key),length=(route:typeof trunk.route)=>route.points.slice(1).reduce((sum,point,index)=>sum+Math.hypot(point.x-route.points[index].x,point.y-route.points[index].y),0);if(!previous||trunk.members.length>previous.members.length||trunk.members.length===previous.members.length&&length(trunk.route)>length(previous.route))groups.set(key,trunk);}
    return [...groups.values()];
  });
  const bundleMembers=$derived(bundlePicker?routes.filter(route=>bundlePicker!.ids.includes(route.binding.id)):[]);
  const frames=$derived(overview?[]:canvasGroups(visibleDocument,nodes));
  const paletteSize=$derived.by(()=>{const size=canvasNodeSize({kind:paletteDrag?.component.kind==='transformation'?'processing':'resource'});return paletteDrag?.component.kind==='transformation'&&['gate','any','all','priority'].includes(paletteDrag.component.preset)?{...size,height:size.height+PROFILE_TRANSFORM_PORT_ROW}:size;});
  const dropGroup=$derived.by(()=>{if(dragged){const size=canvasNodeSize(nodes.find(node=>node.id===dragged!.id)??{});return canvasGroupAt(visibleDocument,{x:dragged.position.x+size.width/2,y:dragged.position.y+size.height/2});}return palettePreview?canvasGroupAt(visibleDocument,{x:palettePreview.x+paletteSize.width/2,y:palettePreview.y+paletteSize.height/2}):null;});
  const selected = $derived(nodes.find(node => node.id === selectedId));
  const focusSelectionId=$derived(selected?.id??'');
  const selectedEdge=$derived(document.bindings.find(binding=>binding.id===selectedEdgeId));
  const selectedDiagnostics=$derived(diagnostics.filter(diagnostic=>diagnostic.location.nodeId===selectedId||diagnostic.location.resourceId===selectedId||document.bindings.some(binding=>diagnostic.location.edgeIds.includes(binding.id)&&(binding.from.nodeId===selectedId||binding.to.nodeId===selectedId))));
  const matches = $derived(canvasSearch(nodes, search));
  const inputs = $derived(selected?.resource?.ports.filter(port=>port.direction==='input') ?? selected?.processing?.inputs ?? selected?.virtual?.ports ?? selected?.desktop?.ports ?? []);
  const outputs = $derived(selected?.resource?.ports.filter(port=>port.direction==='output') ?? selected?.processing?.outputs ?? []);
  const gameId = $derived(selected ? canvasGameId(selected) : null);
  const observedId = $derived(selected?.observation ? selected.assignment?.sourceId : undefined);
  const simpleInspector = $derived(simpleControllerInspector && selected?.resource?.kind === 'device' && selected.observation?.resourceType === 'host-controller');
  let portEpoch = 0;
  let laneScope=untrack(()=>`${document.id}/${activeVehicleTabId}`);
  $effect(()=>{const scope=`${document.id}/${activeVehicleTabId}`;if(scope!==laneScope){laneScope=scope;closeLanes();}});
  $effect(()=>{if(overview&&selectedId&&allNodes.some(node=>node.id===selectedId&&node.processing)){activeTransform=selectedId;showTransforms=true;selectedId='';}});
  $effect(()=>{if(activeVehicleTabId!==previousVehicleTab){previousVehicleTab=activeVehicleTabId;layoutEpoch++;layoutBusy=false;selectionView=null;focusedSelectionId='';clearPendingMove();clearConnectionClick();drag=null;dragged=null;wire=null;wireTargetId='';wireTargetPort='';wireRejectedId='';wireRejectedPort='';wireRejectedReason='';groupPreview=null;segmentDrag=null;segmentPreview=null;segmentBlocked=false;palettePreview=null;layoutError='';fitView();}});
  $effect(()=>{void focusSelectionId;void inspectorPanel;void drag;void wire;void segmentDrag;void connectionOpen;if(mounted)untrack(syncSelectionFocus);});
  $effect(()=>{if(!paletteDrag||readOnly||!dropComponent||canDropComponent?.(paletteDrag.component)===false)palettePreview=null;});
  $effect(()=>{if(bundlePicker&&!bundleMembers.length)bundlePicker=null;});
  $effect(()=>{void activeVehicleTabId;void document.id;bundlePicker=null;});
  $effect(() => {
    const resourceId = simpleInspector||companionAuthoring&&selected?.resource?.kind==='companion'?undefined:observedId;
    portEpoch++; browser.clear(); page = null; portError = ''; portsBusy = false;
    if (resourceId) void loadPorts('open',resourceId);
  });
  onMount(() => { if (fitOnMount) fitView();mounted=true; });
  onDestroy(() => { portEpoch++;layoutEpoch++; browser.clear();clearPendingMove();clearConnectionClick(); });
  function fitView() { const points=[...nodes,...frames.flatMap(frame=>[{position:{x:frame.x,y:frame.y}},{position:{x:frame.x+frame.width-PROFILE_BLOCK_WIDTH,y:frame.y+frame.height-PROFILE_BLOCK_HEIGHT}}])];view = canvasFit(points,stage?.clientWidth || 900,stage?.clientHeight || 600); }
  function fit() { bundlePicker=null;fitView(); }
  function closeLanes(){pendingOverviewDrop=null;overviewBundleId='';showTransforms=false;activeTransform='';}
  function openOverview(id:string){
    if(overviewBundleId===id){closeLanes();return;}
    pendingOverviewDrop=null;clearConnectionClick();selectionView=null;focusedSelectionId='';
    closeConnectionPanel?.();selectConnection?.('');selectedId='';showTransforms=false;activeTransform='';overviewBundleId=id;
  }
  function slideLanes(_node:HTMLElement){return {duration:window.matchMedia('(prefers-reduced-motion: reduce)').matches?0:180,css:(t:number)=>`transform:translateY(${(1-t)*100}%)`};}
  function trimArrowShaft(element:SVGPathElement,settings:{path:string;head:number}){
    function update({path,head}:typeof settings){
      // Actions may update before Svelte writes d during a drag or Undo.
      // Measure the new geometry, otherwise the old length leaves a gap mid-wire.
      element.setAttribute('d',path);
      const length=element.getTotalLength();element.style.strokeDasharray=`${Math.max(0,length-head*.55)} ${head}`;
    }
    update(settings);return{update};
  }
  function zoom(factor:number,at?:CanvasPoint) { bundlePicker=null;view=canvasZoom(view,factor,at??{x:(stage?.clientWidth||900)/2,y:(stage?.clientHeight||600)/2}); }
  async function layout() {
    if(readOnly||layoutBusy||drag||segmentDrag||wire)return;
    const sourceDocument=document,tab=activeVehicleTabId,epoch=++layoutEpoch;
    const layoutNodes=canvasRouteNodes(committedNodes),bindings=routingBindings;
    layoutBusy=true;layoutError='';
    try{
      const {layoutProfileWithElk}=await import('./profile-elk-runtime');
      const result=await layoutProfileWithElk(layoutNodes,bindings,PROFILE_BLOCK_WIDTH,PROFILE_BLOCK_HEIGHT,overview?candidate=>canvasOverviewLayoutScore(layoutNodes,bindings,candidate.positions,PROFILE_BLOCK_WIDTH,PROFILE_BLOCK_HEIGHT):undefined);
      if(epoch!==layoutEpoch||tab!==activeVehicleTabId||readOnly)return;
      if(document!==sourceDocument||drag||segmentDrag||wire){layoutError='The graph changed while arranging it. Run Auto-layout again.';return;}
      bundlePicker=null;selectionView=null;focusedSelectionId='';
      selectedId='';selectConnection?.('');closeConnectionPanel?.();
      editLayout(()=>scopedCanvasEdit(sourceDocument,source=>{
      // Freeze old frames before positions change, then reconcile every membership in the same edit.
      let next=nodes.length?canvasMoveIntoGroup(source,nodes[0].id,nodes[0].position):JSON.parse(JSON.stringify(source)) as ProfileDocument;
      const visibleIds=new Set(layoutNodes.map(node=>node.id)),positions=result.positions;
      const visibleBindings=new Set(bindings.map(binding=>binding.id));
      if(!overview)next.editor.wireRoutes=[...(next.editor.wireRoutes??[]).filter(route=>!visibleBindings.has(route.bindingId)),...result.wireRoutes];
      if(!next.editor.wireRoutes?.length)delete next.editor.wireRoutes;
      next.editor.positions=[...next.editor.positions.filter(position=>!visibleIds.has(position.nodeId)),...positions];
      for(const position of positions)next=canvasMoveIntoGroup(next,position.nodeId,position);
      const nextVisible=profileTabDocument(next,activeVehicleTabId),nextNodes=canvasNodes(nextVisible,assignments,inventory).filter(node=>!overview||!node.processing),nextFrames=overview?[]:canvasGroups(nextVisible,nextNodes);
      view=canvasFit([...nextNodes,...nextFrames.flatMap(frame=>[{position:{x:frame.x,y:frame.y}},{position:{x:frame.x+frame.width-PROFILE_BLOCK_WIDTH,y:frame.y+frame.height-PROFILE_BLOCK_HEIGHT}}])],stage?.clientWidth||900,stage?.clientHeight||600);
      return next;
      }),'Auto-layout');
    }catch(error){if(epoch===layoutEpoch&&tab===activeVehicleTabId)layoutError=error instanceof Error?error.message:String(error);}
    finally{if(epoch===layoutEpoch)layoutBusy=false;}
  }
  function choose(id:string) { closeLanes();bundlePicker=null;clearConnectionClick();closeConnectionPanel?.();selectConnection?.('');selectedId=id; }
  function chooseConnection(id:string) {bundlePicker=null;stage?.focus?.({preventScroll:true});if(selectConnection){selectedId='';if(restoreSelectionView())void tick().then(()=>{if(!selectedId)selectConnection?.(id);});else selectConnection(id);}else editConnection?.(id);}
  function openBundle(event:MouseEvent|KeyboardEvent,bundle:DiagramWireBundle,insertion?:{component:ProfilePaletteComponent;position:CanvasPoint}){
    event.preventDefault();event.stopPropagation();clearConnectionClick();closeConnectionPanel?.();selectConnection?.('');selectedId='';
    const members=bundle.id.startsWith('trunk:')?bundle.members:bundleDiagramRoutes(routes).find(group=>group.members.some(route=>route.binding.id===bundle.route.binding.id))?.members??bundle.members;
    const rect=stage.getBoundingClientRect(),target=(event.currentTarget as Element).getBoundingClientRect();
    const clientX='clientX'in event&&(event.clientX||event.clientY)?event.clientX:target.left+target.width/2,clientY='clientY'in event&&(event.clientX||event.clientY)?event.clientY:target.top+target.height/2;
    const width=Math.min(360,rect.width-16),top=Math.max(8,8-rect.top),bottom=Math.min(rect.height,window.innerHeight-rect.top)-8,maxHeight=Math.max(120,Math.min(360,bottom-top));
    const left=Math.max(8,8-rect.left),right=Math.max(left,Math.min(rect.width,window.innerWidth-rect.left)-width-8);
    bundlePicker={ids:members.map(route=>route.binding.id),x:Math.max(left,Math.min(right,clientX-rect.left+12)),y:Math.max(top,Math.min(bottom-maxHeight,clientY-rect.top+12)),width,maxHeight,insertion};
    void tick().then(()=>stage?.parentElement?.querySelector<HTMLButtonElement>('.canvas-wire-bundle-picker [data-bundle-binding]')?.focus({preventScroll:true}));
  }
  function chooseBundleMember(id:string){const insertion=bundlePicker?.insertion;bundlePicker=null;if(insertion)dropOnConnection?.(insertion.component,id,insertion.position);else chooseConnection(id);}
  function bundleKey(event:KeyboardEvent,bundle:DiagramWireBundle){if(['Enter',' ','Delete','Backspace'].includes(event.key))openBundle(event,bundle);}
  function positionBundleBadge(element:SVGGElement,settings:{path:string;zoom:number;bindingId:string;trunkId?:string}){
    function update({zoom,bindingId,trunkId}:typeof settings){const path=Array.from(element.parentElement?.querySelectorAll<SVGPathElement>(trunkId?'.signal-trunk':'.signal-edge')??[]).find(path=>trunkId?path.dataset.trunkId===trunkId:path.dataset.bindingId===bindingId);if(!path)return;const point=path.getPointAtLength(path.getTotalLength()/2);element.setAttribute('transform',`translate(${point.x},${point.y}) scale(${1/zoom})`);}
    update(settings);return{update};
  }
  function clearConnectionClick(){if(connectionClickTimer!==undefined){clearTimeout(connectionClickTimer);connectionClickTimer=undefined;}}
  function connectionClick(event:MouseEvent,id:string){clearConnectionClick();if(event.detail===0)chooseConnection(id);else if(event.detail===1)connectionClickTimer=setTimeout(()=>{connectionClickTimer=undefined;chooseConnection(id);},250);}
  function portName(nodeId:string,portId:string,direction:'input'|'output') {return portGroups.get(nodeId)?.names.get(`${direction}/${portId}`)??portId;}
  function activeHandle(nodeId:string,direction:'input'|'output',portId?:string) {
    const matches=(selected?:string)=>!portId||!selected||selected===portId;
    return direction==='output'?(connectionSourceId===nodeId&&matches(connectionSourcePort)||selectedEdge?.from.nodeId===nodeId&&matches(selectedEdge.from.portId)||wire?.direction==='output'&&wire.nodeId===nodeId&&matches(wire.portId)||wire?.direction==='input'&&wireTargetId===nodeId&&matches(wireTargetPort)):(connectionTargetId===nodeId&&matches(connectionTargetPort)||selectedEdge?.to.nodeId===nodeId&&matches(selectedEdge.to.portId)||wire?.direction==='input'&&wire.nodeId===nodeId&&matches(wire.portId)||wire?.direction==='output'&&wireTargetId===nodeId&&matches(wireTargetPort));
  }
  function blockPorts(node:CanvasNode,direction:'input'|'output'):readonly DeepReadonly<ProfilePort>[] {
    const groups=portGroups.get(node.id);if(groups)return direction==='input'?groups.inputs:groups.outputs;
    if(node.resource)return(discoveredPorts[node.id]??node.resource.ports).filter(port=>port.direction===direction);
    return(direction==='input'?node.processing?.inputs??node.virtual?.ports??node.desktop?.ports:node.processing?.outputs)??[];
  }
  function inputState(nodeId:string,portId?:string,replacingId?:string|null){const node=nodesById.get(nodeId),replacement=replacingId===null?undefined:replacingId??(wire?wire.reconnect?.bindingId:reconnectingBindingId||undefined);return inputAvailability(visibleDocument,nodeId,node?blockPorts(node,'input'):[],portId,replacement);}
  function rejectedHandle(nodeId:string,portId?:string){return wireRejectedId===nodeId&&(!portId||wireRejectedPort===portId);}
  function wireClick(event:MouseEvent,nodeId:string,direction:'input'|'output',portId?:string){
    event.stopPropagation();if(suppressWireClick&&event.detail!==0){suppressWireClick=false;return;}if(readOnly||direction==='input'&&inputState(nodeId,portId).full)return;
    bundlePicker=null;selectedId=nodeId;const position=event.detail===0?clientAnchor(nodeId,direction,portId):{x:event.clientX,y:event.clientY};if(direction==='output'){if(portId)connectionFrom?.(nodeId,position,portId);else connectionFrom?.(nodeId,position);}else if(portId)connectionTo?.(nodeId,position,portId);else connectionTo?.(nodeId,position);
  }
  function clientAnchor(nodeId:string,direction:'input'|'output',portId?:string):CanvasPoint|undefined {
    const node=renderedNodes.find(node=>node.id===nodeId);if(!node||!stage)return;
    const anchor=canvasNodeAnchor(node,direction,portId),rect=stage.getBoundingClientRect();return{x:rect.left+view.x+anchor.x*view.zoom,y:rect.top+view.y+anchor.y*view.zoom};
  }
  function clearPalettePreview(){palettePreview=null;paletteConnectionId='';}
  function acceptsPaletteDrop(event:DragEvent){
    if(readOnly||!paletteDrag||!dropComponent||canDropComponent?.(paletteDrag.component)===false)return false;
    const target=event.target as Element|null;
    if(target?.closest('.canvas-inspector,.canvas-connection-panel,.canvas-wire-bundle-picker'))return false;
    const rect=stage.getBoundingClientRect();
    return event.clientX>=rect.left&&event.clientX<=rect.right&&event.clientY>=rect.top&&event.clientY<=rect.bottom;
  }
  function palettePosition(event:DragEvent):CanvasPoint{
    const rect=stage.getBoundingClientRect(),clamp=(value:number)=>Math.min(1e6,Math.max(-1e6,value));
    return{x:clamp((event.clientX-rect.left-view.x)/view.zoom-paletteSize.width/2),y:clamp((event.clientY-rect.top-view.y)/view.zoom-paletteSize.height/2)};
  }
  function overPalette(event:DragEvent){
    if(!acceptsPaletteDrop(event)){clearPalettePreview();return;}
    event.preventDefault();if(event.dataTransfer)event.dataTransfer.dropEffect='copy';palettePreview=palettePosition(event);
    paletteConnectionId=dropOnConnection&&paletteDrag?.component.kind==='transformation'?(event.target as Element|null)?.closest<SVGElement>('[data-connection-id]')?.dataset.connectionId??'':'';
  }
  function leavePalette(event:DragEvent){if(event.relatedTarget&&stage.contains(event.relatedTarget as Node))return;clearPalettePreview();}
  function dropPalette(event:DragEvent){
    if(!acceptsPaletteDrop(event)){clearPalettePreview();return;}
    event.preventDefault();event.stopPropagation();
    const component=paletteDrag!.component,position=palettePosition(event),hit=(event.target as Element|null)?.closest<SVGElement>('[data-connection-id]'),bindingId=component.kind==='transformation'?hit?.dataset.connectionId:undefined,bundle=hit?.dataset.bundleId?clickableBundles.find(bundle=>bundle.id===hit.dataset.bundleId&&bundle.members.length>1):undefined;clearPalettePreview();
    try{if(overview&&bindingId&&overviewById.has(bindingId)&&component.kind==='transformation'){const projected=overviewById.get(bindingId)!;if(projected.lanes.length===1)dropOnConnection?.(component,projected.lanes[0].id,position);else{openOverview(bindingId);pendingOverviewDrop={component,position};}}else if(bundle&&component.kind==='transformation'&&dropOnConnection)openBundle(event,bundle,{component,position});else if(bindingId&&dropOnConnection)dropOnConnection(component,bindingId,position);else dropComponent?.(component,position);}finally{endPaletteDrag?.();}
  }
  function restoreSelectionView(){const previous=selectionView;selectionView=null;focusedSelectionId='';if(previous)view={...previous};return !!previous;}
  function syncSelectionFocus(){
    if(drag||wire||segmentDrag)return;
    const node=nodesById.get(selectedId);
    if(!node||connectionOpen){restoreSelectionView();return;}
    if(!stage||!inspectorPanel||node.id===focusedSelectionId)return;
    const width=stage.clientWidth||900,height=stage.clientHeight||600,availableWidth=Math.max(1,width-inspectorPanel.offsetWidth);
    selectionView??={...view};focusedSelectionId=node.id;view=canvasFocusNode(node,availableWidth,height);
  }
  function slideDetails(_node: HTMLElement) {
    return { duration: window.matchMedia('(prefers-reduced-motion: reduce)').matches ? 0 : 180,
      css: (t: number) => `transform:translateX(${(1-t)*100}%)` };
  }
  function editLayout(edit:()=>ProfileDocument,label:string){if(readOnly)return;layoutError='';try{change(edit(),label);}catch(error){layoutError=error instanceof Error?error.message:String(error);}}
  function scopedCanvasEdit(source:DeepReadonly<ProfileDocument>,edit:(source:ProfileDocument)=>ProfileDocument):ProfileDocument{
    if(!source.vehicleTabs?.length)return edit(JSON.parse(JSON.stringify(source)) as ProfileDocument);
    const ids=profileTabNodeIds(source,activeVehicleTabId),next=JSON.parse(JSON.stringify(source)) as ProfileDocument;
    next.editor.groups=next.editor.groups?.filter(group=>group.members.some(id=>ids.has(id))||!activeVehicleTabId&&!group.members.length).map(group=>({...group,members:group.members.filter(id=>ids.has(id))}));
    const visibleGroups=new Set(next.editor.groups?.map(group=>group.id)),result=edit(next),updatedGroups=result.editor.groups??[];
    result.editor.groups=[...(source.editor.groups??[]).filter(group=>!visibleGroups.has(group.id)).map(group=>JSON.parse(JSON.stringify(group))),...updatedGroups.map(group=>({...group,members:[...group.members,...(source.editor.groups?.find(old=>old.id===group.id)?.members.filter(id=>!ids.has(id))??[])]}))];
    if(!result.editor.groups.length)delete result.editor.groups;return result;
  }
  export function addGroup(name='Group'){if(readOnly||activeVehicleTabId)return;layoutError='';try{const next=canvasCreateGroup(document,name,{x:(32-view.x)/view.zoom,y:(32-view.y)/view.zoom});const id=next.editor.groups!.at(-1)!.id;change(next,'Create group');return id;}catch(error){layoutError=error instanceof Error?error.message:String(error);}}
  export function renameGroup(id:string,name:string){editLayout(()=>scopedCanvasEdit(document,source=>canvasRenameGroup(source,id,name)),'Rename group');}
  export function deleteGroup(id:string){editLayout(()=>scopedCanvasEdit(document,source=>canvasDeleteGroup(source,id)),'Delete group');}
  export function focusGroup(id:string){const frame=frames.find(group=>group.id===id);if(!frame)return;view=canvasFit([{position:{x:frame.x,y:frame.y}},{position:{x:frame.x+frame.width-PROFILE_BLOCK_WIDTH,y:frame.y+frame.height-PROFILE_BLOCK_HEIGHT}}],stage?.clientWidth||900,stage?.clientHeight||600);}
  function togglePorts(category:ProfilePortGroup){if(selected)editLayout(()=>canvasTogglePorts(document,selected.id,category),canvasPortCollapsed(document,selected.id,category)?'Expand port category':'Collapse port category');}
  function focusMatch() { const node=matches[0];if(node)choose(node.id); }
  function begin(event:PointerEvent,id?:string) {
    if(event.button!==0)return;
    const target = event.target as Element;
    if(target.closest('.signal-edge-hit,.signal-trunk-hit,.connection-bundle-badge,.connection-trunk-badge,.connection-endpoint,.connection-segment,.connection-reroute')||target.closest('button,input,select,a')&&!target.closest('.block-heading'))return;
    bundlePicker=null;clearConnectionClick();
    id ??= target.closest<HTMLElement>('[data-node-id]')?.dataset.nodeId;
    event.preventDefault();stage.setPointerCapture(event.pointerId);
    drag={id,origin:id?{...nodes.find(node=>node.id===id)!.position}:{x:0,y:0},start:{x:event.clientX,y:event.clientY},viewport:{...view},pointer:event.pointerId,moved:false};
  }
  function resetWireRoute(bindingId:string){editLayout(()=>profileSetWireRoute({document,assignments:[]},bindingId).document,'Reset wire bends');}
  function validateWireEdit(next:ProfileDocument,bindingId:string){const scoped=profileTabDocument(next,activeVehicleTabId),route=diagramRoutes(routeNodes,scoped.bindings.filter(binding=>binding.id===bindingId),PROFILE_BLOCK_WIDTH,PROFILE_BLOCK_HEIGHT,{style:wireStyle,cornerRadius,wireRoutes:scoped.editor.wireRoutes})[0];if(!route||route.customBlocked)throw new Error('Keep the wire clear of block borders. Release here to cancel this route change.');return next;}
  function canvasPointer(event:MouseEvent|PointerEvent):CanvasPoint{const rect=stage.getBoundingClientRect();return{x:(event.clientX-rect.left-view.x)/view.zoom,y:(event.clientY-rect.top-view.y)/view.zoom};}
  function addWireHandle(event:MouseEvent,bindingId:string){
    if(readOnly)return;clearConnectionClick();event.preventDefault();event.stopPropagation();closeConnectionPanel?.();selectedId='';stage?.focus?.({preventScroll:true});selectConnection?.(bindingId);
    const route=routes.find(route=>route.binding.id===bindingId);if(!route)return;
    editLayout(()=>{const next=profileInsertWireHandle({document,assignments:[]},bindingId,route.points,canvasPointer(event)).document;return validateWireEdit(next,bindingId);},'Add reroute point');
  }
  function routeHandles(bindingId:string,points:readonly CanvasPoint[]){return visibleDocument.editor.wireRoutes?.find(route=>route.bindingId===bindingId)?.handles?.map((point,index)=>({point,index})).filter(({point})=>profileWireHandleOnRoute(point,points))??[];}
  function beginHandle(event:PointerEvent,bindingId:string,index:number){
    if(readOnly||event.button!==0)return;const route=routes.find(route=>route.binding.id===bindingId),saved=visibleDocument.editor.wireRoutes?.find(route=>route.bindingId===bindingId);if(!route||!saved?.handles?.[index])return;
    event.preventDefault();event.stopPropagation();stage?.focus?.({preventScroll:true});layoutError='';segmentBlocked=false;segmentPreview=null;
    const source=profileSetWireRoute({document,assignments:[]},bindingId,route.points,saved.handles).document;
    segmentDrag={bindingId,index:-1,handleIndex:index,points:route.points.map(point=>({...point})),source,pointer:event.pointerId,start:{x:event.clientX,y:event.clientY},moved:false};stage.setPointerCapture(event.pointerId);
  }
  function handleKeyboard(event:KeyboardEvent,bindingId:string,index:number){
    if(readOnly||!['ArrowLeft','ArrowRight','ArrowUp','ArrowDown','Delete','Backspace'].includes(event.key))return;event.preventDefault();event.stopPropagation();const route=document.editor.wireRoutes?.find(route=>route.bindingId===bindingId),point=route?.handles?.[index];if(!point)return;
    if(event.key==='Delete'||event.key==='Backspace'){editLayout(()=>profileRemoveWireHandle({document,assignments:[]},bindingId,index).document,'Remove reroute point');return;}
    const step=event.shiftKey?40:10;editLayout(()=>validateWireEdit(profileMoveWireHandle({document,assignments:[]},bindingId,index,{x:point.x+(event.key==='ArrowLeft'?-step:event.key==='ArrowRight'?step:0),y:point.y+(event.key==='ArrowUp'?-step:event.key==='ArrowDown'?step:0)}).document,bindingId),'Move reroute point');
  }
  function movedSegment(source:DeepReadonly<ProfileDocument>,bindingId:string,points:CanvasPoint[],index:number,position:CanvasPoint){
    const next=profileReshapeWireSegment({document:source,assignments:[]},bindingId,points,index,position).document,scoped=profileTabDocument(next,activeVehicleTabId);
    const route=diagramRoutes(routeNodes,scoped.bindings.filter(binding=>binding.id===bindingId),PROFILE_BLOCK_WIDTH,PROFILE_BLOCK_HEIGHT,{style:'orthogonal',cornerRadius,wireRoutes:scoped.editor.wireRoutes})[0];
    if(!route||route.customBlocked)throw new Error('Keep the wire clear of block borders. Release here to cancel this bend change.');return next;
  }
  function beginSegment(event:PointerEvent,bindingId:string,index:number){
    if(readOnly||wireStyle!=='orthogonal'||event.button!==0)return;const route=routes.find(route=>route.binding.id===bindingId);if(!route||!route.points[index+1])return;
    event.preventDefault();event.stopPropagation();stage?.focus?.({preventScroll:true});layoutError='';segmentBlocked=false;segmentPreview=null;segmentDrag={bindingId,index,points:route.points.map(point=>({...point})),source:document,pointer:event.pointerId,start:{x:event.clientX,y:event.clientY},moved:false};stage.setPointerCapture(event.pointerId);
  }
  function moveSegment(event:PointerEvent){if(!segmentDrag||segmentDrag.pointer!==event.pointerId)return;const state=segmentDrag,delta={x:event.clientX-state.start.x,y:event.clientY-state.start.y};
    if(state.handleIndex!==undefined){if(!state.moved&&Math.hypot(delta.x,delta.y)<4)return;state.moved=true;const point=state.source.editor.wireRoutes?.find(route=>route.bindingId===state.bindingId)?.handles?.[state.handleIndex];if(!point)return;try{segmentPreview=validateWireEdit(profileMoveWireHandle({document:state.source,assignments:[]},state.bindingId,state.handleIndex,{x:point.x+delta.x/view.zoom,y:point.y+delta.y/view.zoom}).document,state.bindingId);segmentBlocked=false;layoutError='';}catch(error){segmentBlocked=true;layoutError=error instanceof Error?error.message:String(error);}return;}
    const a=state.points[state.index],b=state.points[state.index+1];if(!state.moved&&Math.abs(a.y===b.y?delta.y:delta.x)<4)return;state.moved=true;
    try{segmentPreview=movedSegment(state.source,state.bindingId,state.points,state.index,{x:(a.x+b.x)/2+delta.x/view.zoom,y:(a.y+b.y)/2+delta.y/view.zoom});segmentBlocked=false;layoutError='';}catch(error){segmentBlocked=true;layoutError=error instanceof Error?error.message:String(error);}
  }
  function cancelSegment(){clearPendingMove();const pointer=segmentDrag?.pointer;segmentDrag=null;segmentPreview=null;segmentBlocked=false;layoutError='';if(pointer!==undefined&&stage?.hasPointerCapture(pointer))stage.releasePointerCapture(pointer);}
  function segmentKeyboard(event:KeyboardEvent,bindingId:string,index:number){if(readOnly||wireStyle!=='orthogonal'||!['ArrowLeft','ArrowRight','ArrowUp','ArrowDown'].includes(event.key))return;const route=routes.find(route=>route.binding.id===bindingId),a=route?.points[index],b=route?.points[index+1];if(!route||!a||!b)return;event.preventDefault();event.stopPropagation();if((a.y===b.y)!==['ArrowUp','ArrowDown'].includes(event.key))return;const step=event.shiftKey?40:10;editLayout(()=>movedSegment(document,bindingId,route.points,index,{x:(a.x+b.x)/2+(event.key==='ArrowLeft'?-step:event.key==='ArrowRight'?step:0),y:(a.y+b.y)/2+(event.key==='ArrowUp'?-step:event.key==='ArrowDown'?step:0)}),'Move wire segment');}
  function beginGroup(event:PointerEvent,frame:CanvasGroup,mode:'move'|'resize'){
    event.stopPropagation();if(readOnly||event.button!==0)return;
    event.preventDefault();bundlePicker=null;stage.setPointerCapture(event.pointerId);layoutError='';groupBlocked=false;
    drag={group:{id:frame.id,mode,frame,source:document},origin:{x:frame.x,y:frame.y},start:{x:event.clientX,y:event.clientY},viewport:{...view},pointer:event.pointerId,moved:false};
  }
  function previewGroup(delta:CanvasPoint){
    if(!drag?.group)return;const{source,id,frame,mode}=drag.group;
    try{groupPreview=scopedCanvasEdit(source,visible=>mode==='move'?canvasMoveGroup(visible,id,{x:frame.x+delta.x/view.zoom,y:frame.y+delta.y/view.zoom}):canvasResizeGroup(visible,id,{width:frame.width+delta.x/view.zoom,height:frame.height+delta.y/view.zoom}));groupBlocked=false;layoutError='';}
    catch(error){groupBlocked=true;layoutError=error instanceof Error?error.message:String(error);}
  }
  function beginWire(event:PointerEvent,nodeId:string,direction:'input'|'output',portId?:string){
    if(readOnly||!connectBlocks||event.button!==0||direction==='input'&&inputState(nodeId,portId).full)return;event.stopPropagation();event.preventDefault();stage?.focus?.({preventScroll:true});suppressWireClick=false;wireRejectedId='';wireRejectedPort='';wireRejectedReason='';
    bundlePicker=null;clearConnectionClick();const node=nodes.find(node=>node.id===nodeId)!;
    wireTargetId='';wireTargetPort='';wire={nodeId,portId,direction,pointer:event.pointerId,start:{x:event.clientX,y:event.clientY},end:canvasNodeAnchor(node,direction,portId),moved:false};stage.setPointerCapture(event.pointerId);
  }
  function beginReconnect(event:PointerEvent,bindingId:string,side:'from'|'to'){
    if(readOnly||!reconnectConnection||event.button!==0)return;event.stopPropagation();event.preventDefault();stage?.focus?.({preventScroll:true});
    const binding=document.bindings.find(binding=>binding.id===bindingId);if(!binding)return;
    const nodeId=side==='from'?binding.to.nodeId:binding.from.nodeId,direction=side==='from'?'input':'output',node=nodes.find(node=>node.id===nodeId),moving=nodes.find(node=>node.id===binding[side].nodeId);if(!node||!moving)return;
    wireTargetId='';wireTargetPort='';wireRejectedId='';wireRejectedPort='';wireRejectedReason='';wire={nodeId,portId:side==='from'?binding.to.portId:binding.from.portId,direction,pointer:event.pointerId,start:{x:event.clientX,y:event.clientY},end:canvasNodeAnchor(moving,side==='from'?'output':'input',binding[side].portId),moved:false,reconnect:{bindingId,side}};stage.setPointerCapture(event.pointerId);
  }
  function reconnectKey(event:KeyboardEvent,bindingId:string,side:'from'|'to') {
    if(event.key!=='Enter'&&event.key!==' ')return;event.preventDefault();event.stopPropagation();const binding=document.bindings.find(binding=>binding.id===bindingId);if(binding)reconnectConnection?.(bindingId,side,binding[side].nodeId,clientAnchor(binding[side].nodeId,side==='from'?'output':'input',binding[side].portId));
  }
  function move(event:PointerEvent) {if(segmentDrag){moveSegment(event);return;}if(wire&&event.pointerId===wire.pointer){const rect=stage.getBoundingClientRect(),hit=window.document.elementFromPoint(event.clientX,event.clientY)?.closest<HTMLButtonElement>(wire.direction==='output'?'[data-connection-target]':'[data-connection-source]');const targetId=wire.direction==='output'?hit?.dataset.connectionTarget:hit?.dataset.connectionSource,blocked=wire.direction==='output'&&targetId?inputState(targetId,hit?.dataset.connectionPort):null;wireRejectedId=blocked?.full?targetId??'':'';wireRejectedPort=wireRejectedId?hit?.dataset.connectionPort??'':'';wireRejectedReason=blocked?.full?blocked.reason:'';wireTargetId=hit&&!hit.disabled&&!blocked?.full?targetId??'':'';wireTargetPort=wireTargetId?hit?.dataset.connectionPort??'':'';if(wireTargetId===wire.nodeId){wireTargetId='';wireTargetPort='';}wire={...wire,moved:wire.moved||Math.hypot(event.clientX-wire.start.x,event.clientY-wire.start.y)>=4,end:{x:(event.clientX-rect.left-view.x)/view.zoom,y:(event.clientY-rect.top-view.y)/view.zoom}};return;} if(!drag||event.pointerId!==drag.pointer)return;const delta={x:event.clientX-drag.start.x,y:event.clientY-drag.start.y};if(!drag.moved&&Math.hypot(delta.x,delta.y)<4)return;drag.moved=true;if(drag.group)previewGroup(delta);else if(drag.id&&!readOnly)dragged={id:drag.id,position:{x:drag.origin.x+delta.x/view.zoom,y:drag.origin.y+delta.y/view.zoom}};else if(!drag.id)view=canvasPan(drag.viewport,delta); }
  function clearPendingMove(){if(moveFrame)cancelAnimationFrame(moveFrame);moveFrame=0;pendingMove=null;}
  function scheduleMove(event:PointerEvent){if(!drag&&!wire&&!segmentDrag)return;pendingMove=event;if(!moveFrame)moveFrame=requestAnimationFrame(()=>{moveFrame=0;const latest=pendingMove;pendingMove=null;if(latest)move(latest);});}
  function flushMove(){const latest=pendingMove;clearPendingMove();if(latest)move(latest);}
  function finish(event:PointerEvent,cancel=false) {
    if(cancel)clearPendingMove();else flushMove();
    if(segmentDrag&&segmentDrag.pointer===event.pointerId){if(!cancel&&segmentDrag.moved&&segmentPreview&&!segmentBlocked)change(segmentPreview,segmentDrag.handleIndex===undefined?'Move wire segment':'Move reroute point');cancelSegment();return;}
    if(wire&&event.pointerId===wire.pointer){
      const pending=wire;let rejected=wireRejectedReason;wire=null;wireTargetId='';wireTargetPort='';wireRejectedId='';wireRejectedPort='';wireRejectedReason='';suppressWireClick=true;
      if(!cancel){
        const element=window.document.elementFromPoint(event.clientX,event.clientY);
        const handle=element?.closest<HTMLButtonElement>(pending.direction==='output'?'[data-connection-target]':'[data-connection-source]');
        const inputStatus=pending.direction==='output'&&handle?.dataset.connectionTarget?inputState(handle.dataset.connectionTarget,handle.dataset.connectionPort,pending.reconnect?.bindingId??null):null;if(inputStatus?.full)rejected=inputStatus.reason;
        const target=handle?.disabled||inputStatus?.full?undefined:pending.direction==='output'?handle?.dataset.connectionTarget:handle?.dataset.connectionSource;
        selectedId=pending.nodeId;
        const position={x:event.clientX,y:event.clientY};
        const targetPort=handle?.dataset.connectionPort;
        if(target&&target!==pending.nodeId){if(pending.reconnect){if(targetPort)reconnectConnection?.(pending.reconnect.bindingId,pending.reconnect.side,target,position,targetPort);else reconnectConnection?.(pending.reconnect.bindingId,pending.reconnect.side,target,position);}else if(pending.direction==='output'){if(pending.portId||targetPort)connectBlocks?.(pending.nodeId,target,position,pending.portId,targetPort);else connectBlocks?.(pending.nodeId,target,position);}else if(pending.portId||targetPort)connectBlocks?.(target,pending.nodeId,position,targetPort,pending.portId);else connectBlocks?.(target,pending.nodeId,position);}
        else if(rejected){closeConnectionPanel?.();selectConnection?.('');selectedId='';}
        else if(!pending.moved&&!pending.reconnect){if(pending.direction==='output'){if(pending.portId)connectionFrom?.(pending.nodeId,position,pending.portId);else connectionFrom?.(pending.nodeId,position);}else if(pending.portId)connectionTo?.(pending.nodeId,position,pending.portId);else connectionTo?.(pending.nodeId,position);}
        else if(element&&stage.contains(element)&&!element.closest('[data-node-id],.canvas-inspector,.canvas-connection-panel')){closeConnectionPanel?.();selectConnection?.('');selectedId='';}
      }
      if(stage.hasPointerCapture(event.pointerId))stage.releasePointerCapture(event.pointerId);return;
    }
    if(!drag||event.pointerId!==drag.pointer)return;if(!cancel){if(drag.group){if(drag.moved&&groupPreview&&!groupBlocked)change(groupPreview,drag.group.mode==='move'?'Move group':'Resize group');}else if(dragged&&!readOnly)editLayout(()=>scopedCanvasEdit(document,source=>canvasMoveIntoGroup(source,dragged!.id,dragged!.position)),'Move component');else if(!drag.moved){if(drag.id)choose(drag.id);else{closeLanes();closeConnectionPanel?.();selectConnection?.('');selectedId='';}}}else view={...drag.viewport};drag=null;dragged=null;groupPreview=null;groupBlocked=false;if(stage.hasPointerCapture(event.pointerId))stage.releasePointerCapture(event.pointerId);
  }
  function wheel(event:WheelEvent) {
    event.preventDefault();if(segmentDrag)return;bundlePicker=null;
    const navigation=canvasWheelNavigation($canvasNavigation.wheelAction,event,stage.clientHeight);
    if(navigation.zoom){if(!navigation.deltaY)return;const rect=stage.getBoundingClientRect();view=canvasZoom(view,navigation.multiplier,{x:event.clientX-rect.left,y:event.clientY-rect.top});}
    else view=canvasPan(view,{x:-navigation.deltaX,y:-navigation.deltaY});
  }
  function keyboard(event:KeyboardEvent) {
    if(event.key==='Escape'&&bundlePicker){event.preventDefault();bundlePicker=null;stage?.focus({preventScroll:true});return;}
    if(event.key==='Escape'){clearPendingMove();clearConnectionClick();}if(segmentDrag&&event.key==='Escape'){event.preventDefault();cancelSegment();return;}
    if(segmentDrag&&(event.ctrlKey||event.metaKey)&&['z','y'].includes(event.key.toLowerCase()))cancelSegment();
    if(event.key==='Escape'&&paletteDrag){clearPalettePreview();endPaletteDrag?.();return;}
    if(event.key==='Escape'&&wire){event.preventDefault();const pointer=wire.pointer;wire=null;wireTargetId='';wireTargetPort='';wireRejectedId='';wireRejectedPort='';wireRejectedReason='';suppressWireClick=true;if(stage.hasPointerCapture(pointer))stage.releasePointerCapture(pointer);return;}
    if(event.key==='Escape'&&drag){event.preventDefault();const pointer=drag.pointer;view={...drag.viewport};drag=null;dragged=null;groupPreview=null;groupBlocked=false;layoutError='';if(stage.hasPointerCapture(pointer))stage.releasePointerCapture(pointer);return;}
    if((event.target as Element).closest('input,textarea,select,[contenteditable="true"]'))return;
    if(event.key==='Escape'){event.preventDefault();closeLanes();closeConnectionPanel?.();selectConnection?.('');selectedId='';return;}
    if(!readOnly&&selectedEdgeId&&disconnectConnection&&['Delete','Backspace'].includes(event.key)){event.preventDefault();disconnectConnection(selectedEdgeId);return;}
    if(!readOnly&&(undo||redo)&&(event.ctrlKey||event.metaKey)&&['z','y'].includes(event.key.toLowerCase())){event.preventDefault();if(event.shiftKey||event.key.toLowerCase()==='y')redo?.();else undo?.();return;}
    if((event.target as Element).closest('button,a'))return;
    if(event.key==='+'||event.key==='='){event.preventDefault();zoom(1.2);}else if(event.key==='-'){event.preventDefault();zoom(1/1.2);}else if(event.key==='0'){event.preventDefault();fit();}else if(['ArrowLeft','ArrowRight','ArrowUp','ArrowDown'].includes(event.key)){event.preventDefault();view=canvasPan(view,{x:event.key==='ArrowLeft'?40:event.key==='ArrowRight'?-40:0,y:event.key==='ArrowUp'?40:event.key==='ArrowDown'?-40:0});}
  }
  function lostPointer(event:PointerEvent){clearPendingMove();if(segmentDrag?.pointer===event.pointerId)cancelSegment();if(wire?.pointer===event.pointerId){wire=null;wireTargetId='';wireTargetPort='';wireRejectedId='';wireRejectedPort='';wireRejectedReason='';suppressWireClick=true;}if(drag?.pointer===event.pointerId){view={...drag.viewport};drag=null;dragged=null;groupPreview=null;groupBlocked=false;}}
  function connectionKey(event:KeyboardEvent,id:string){if(event.key==='Enter'||event.key===' '){event.preventDefault();event.stopPropagation();chooseConnection(id);}else if(!readOnly&&['Delete','Backspace'].includes(event.key)&&disconnectConnection){event.preventDefault();event.stopPropagation();disconnectConnection(id);}}
  function moveBlock(event:KeyboardEvent,id:string) { if(!event.altKey||readOnly||!['ArrowLeft','ArrowRight','ArrowUp','ArrowDown'].includes(event.key))return;event.preventDefault();event.stopPropagation();const node=nodes.find(node=>node.id===id)!;const step=event.shiftKey?64:16;editLayout(()=>scopedCanvasEdit(document,source=>canvasMoveIntoGroup(source,id,{x:node.position.x+(event.key==='ArrowLeft'?-step:event.key==='ArrowRight'?step:0),y:node.position.y+(event.key==='ArrowUp'?-step:event.key==='ArrowDown'?step:0)})),'Move component'); }
  function groupKeyboard(event:KeyboardEvent,frame:CanvasGroup,mode:'move'|'resize'){
    if(readOnly||!event.altKey||!['ArrowLeft','ArrowRight','ArrowUp','ArrowDown'].includes(event.key))return;
    event.preventDefault();event.stopPropagation();const step=event.shiftKey?64:16,x=event.key==='ArrowLeft'?-step:event.key==='ArrowRight'?step:0,y=event.key==='ArrowUp'?-step:event.key==='ArrowDown'?step:0;
    editLayout(()=>scopedCanvasEdit(document,source=>mode==='move'?canvasMoveGroup(source,frame.id,{x:frame.x+x,y:frame.y+y}):canvasResizeGroup(source,frame.id,{width:frame.width+x,height:frame.height+y})),mode==='move'?'Move group':'Resize group');
  }
  async function loadPorts(operation:'open'|'next'|'previous',resourceId?:string) {
    const epoch=portEpoch;portsBusy=true;portError='';
    try{const result=operation==='open'?await browser.open(resourceId!):operation==='next'?await browser.next():await browser.previous();if(epoch===portEpoch&&result)page=result;}
    catch(error){if(epoch===portEpoch)portError=error instanceof Error?error.message:String(error);}
    finally{if(epoch===portEpoch)portsBusy=false;}
  }
</script>

<svelte:window ondragend={clearPalettePreview}/>

<section class="profile-graph" aria-label="Profile resource graph">
  <div class="canvas-toolbar">
    <div class="canvas-search"><Icon name="search" size={14}/><input aria-label="Search profile blocks" placeholder="Find a block, type or transport…" bind:this={searchInput} bind:value={search} onkeydown={event=>{if(event.key==='Enter')focusMatch();}}/>{#if search}<button class="search-clear" type="button" aria-label="Clear block search" title="Clear block search" onclick={()=>{search='';searchInput.focus();}}><Icon name="close" size={14}/></button>{/if}</div>
    {#if selectedEdge&&document.editor.wireRoutes?.some(route=>route.bindingId===selectedEdgeId)}<button class="text-button" disabled={readOnly} onclick={()=>resetWireRoute(selectedEdgeId)}>Reset bends</button>{/if}
    <CanvasViewControls zoom={view.zoom} {layoutBusy} {readOnly} onfit={fit} onlayout={layout} onzoom={zoom}/>
  </div>
  {#if layoutBusy}<p class="canvas-help" role="status">Arranging blocks and connections…</p>{/if}
  <p class="canvas-help" id="profile-canvas-help">{#if overview}Drag between blocks to connect. Thicker arrows carry more signals; click an arrow to edit its transform chains. Drop a transform on an arrow to insert it. Click the selected arrow again, press Escape, or click the canvas to dismiss.{:else}Drop components, then drag between ports or input/output boxes. Full inputs cannot receive another connection. Choose arrow style and corner radius in Settings → Styling. Double-click a wire to add a draggable reroute point; select a point and press Delete to remove it. With horizontal/vertical arrows, select a wire to drag its straight segments. Select a block to configure it or a wire to manage its connection. Drag a wire endpoint to reconnect it; drop a transformation onto a wire to insert it. Escape or blank canvas cancels. Drag the background to pan. Mouse wheel {$canvasNavigation.wheelAction==='zoom'?'zooms':'scrolls'}; Ctrl/⌘ + wheel {$canvasNavigation.wheelAction==='zoom'?'scrolls':'zooms'}. Configure this in Settings → General. Ctrl/⌘ + Z undoes; Shift + Z redoes.{/if}</p>
  <div class="canvas-layout">
    <!-- svelte-ignore a11y_no_noninteractive_tabindex, a11y_no_noninteractive_element_interactions (The canvas is a keyboard pan/zoom application with instructions.) -->
    <div class="canvas-stage" data-view={overview?'overview':'detailed'} class:node-dragging={!!dragged||!!groupPreview} class:wire-blocked={!!wireRejectedReason||segmentBlocked} class:segment-dragging={!!segmentDrag} class:palette-dropping={!!palettePreview} role="application" aria-label="Profile canvas" aria-describedby="profile-canvas-help" tabindex="0" bind:this={stage} onpointerdown={event=>begin(event)} onpointermove={scheduleMove} onpointerup={event=>finish(event)} onpointercancel={event=>finish(event,true)} onlostpointercapture={lostPointer} onwheel={wheel} onkeydown={keyboard} ondragover={overPalette} ondragleave={leavePalette} ondrop={dropPalette}>
      {#if layoutError}<p class="canvas-layout-error port-error" role="alert">{layoutError}</p>{/if}
      <div class="canvas-world" class:route-focused={!!overviewBundleId||!!selectedEdgeId} style={`transform:translate(${view.x}px,${view.y}px) scale(${view.zoom})`}>
        {#if palettePreview&&paletteDrag}
          <div class="palette-drop-preview" role="img" aria-label="Component drop preview" data-component-kind={paletteDrag.component.kind} data-preview-x={palettePreview.x} data-preview-y={palettePreview.y} style={`left:${palettePreview.x}px;top:${palettePreview.y}px;width:${paletteSize.width}px;height:${paletteSize.height}px`}>
            <div class="preview-heading"><span class="block-category">{paletteDrag.category}</span><strong title={paletteDrag.name}>{paletteDrag.name}</strong></div>
            <span class="preview-hint"><Icon name="plus" size={18}/>{paletteConnectionId?'Release to insert into connection':'Release to add'}</span>
          </div>
        {/if}
        {#each frames as frame (frame.id)}
          <div class="editor-group-frame" class:drop-target={dropGroup===frame.id} class:blocked={groupBlocked&&drag?.group?.id===frame.id} data-group-id={frame.id} data-group-members={frame.members.join(',')} style={`left:${frame.x}px;top:${frame.y}px;width:${frame.width}px;height:${frame.height}px`}>
            <button class="editor-group-heading" title={`${frame.name} · Drag to move; double-click to focus`} aria-label={`Focus group ${frame.name}`} onpointerdown={event=>beginGroup(event,frame,'move')} onclick={event=>{if(event.detail===0)focusGroup(frame.id);}} ondblclick={()=>focusGroup(frame.id)} onkeydown={event=>groupKeyboard(event,frame,'move')}><Icon name="group" size={14}/><strong>{frame.name}</strong><span>{frame.members.length}</span></button>
            {#if !frame.members.length}<span class="editor-group-empty">Drop components here</span>{/if}
            {#if !readOnly}<button class="editor-group-resize" aria-label={`Resize group ${frame.name}`} title="Drag to resize; Alt + arrow keys to adjust" onpointerdown={event=>beginGroup(event,frame,'resize')} onkeydown={event=>groupKeyboard(event,frame,'resize')}></button>{/if}
          </div>
        {/each}
        <svg class="signal-edges" width="1" height="1" aria-label="Saved signal routes">
          <defs><marker id="profile-signal-arrow" markerUnits="userSpaceOnUse" markerWidth="10" markerHeight="10" viewBox="0 0 8 8" refX="8" refY="4" orient="auto"><path d="M0,0 L8,4 L0,8 Q1,4 0,0 Z" /></marker></defs>
          {#snippet renderWireBundle(bundle:DiagramWireBundle)}
            {@const route=bundle.route}
            {@const binding=route.binding}
            {@const grouped=bundle.members.length>1}
            {@const visibleLabel=!overview&&!grouped&&(selectedEdgeId===binding.id||hoveredEdgeId===binding.id)}
            {#if overview}
              {@const projected=overviewById.get(binding.id)!}
              {@const width=signalBundleWidth(projected.lanes.length,maximumRoutes)}
              {@const head=Math.max(10,width*2.4)}
              {@const markerId=`${inspectorContentId}-arrow-${overviewBundles.indexOf(projected)}`}
              <defs><marker id={markerId} markerUnits="userSpaceOnUse" markerWidth={head} markerHeight={head} viewBox="0 0 10 10" refX="10" refY="5" orient="auto"><path class:selected={overviewBundleId===binding.id} d="M0,0 L10,5 L0,10 Z"/></marker></defs>
              <path class="signal-edge overview-edge" data-binding-id={binding.id} data-route-count={projected.lanes.length} data-arrow-style={wireStyle} class:selected={overviewBundleId===binding.id} class:drop-target={paletteConnectionId===binding.id} style={`stroke-width:${width}px`} use:trimArrowShaft={{path:route.path,head}} marker-end={`url(#${markerId})`} d={route.path}><title>{nodesById.get(binding.from.nodeId)?.name} → {nodesById.get(binding.to.nodeId)?.name}</title></path>
              <path class="signal-edge-hit" data-connection-id={binding.id} d={route.path} role="button" tabindex="0" aria-label={`Inspect signals from ${nodesById.get(binding.from.nodeId)?.name} to ${nodesById.get(binding.to.nodeId)?.name}`} aria-pressed={overviewBundleId===binding.id} onpointerdown={event=>{event.preventDefault();event.stopPropagation();}} onclick={()=>openOverview(binding.id)} onkeydown={event=>{if(event.key==='Enter'||event.key===' '){event.preventDefault();event.stopPropagation();openOverview(binding.id);}}}/>
            {:else}
            <path class="signal-edge" class:bundled={grouped} class:orthogonal={wireStyle==='orthogonal'} data-binding-id={binding.id} data-binding-ids={JSON.stringify(bundle.members.map(route=>route.binding.id))} data-bundle-count={bundle.members.length} data-bundle-id={grouped?bundle.id:undefined} data-arrow-style={wireStyle} class:selected={selectedEdgeId===binding.id} class:drop-target={paletteConnectionId===binding.id} class:highlighted={bundle.members.some(route=>highlightedEdges.includes(route.binding.id))} class:problem={bundle.members.some(route=>route.customBlocked||diagnostics.some(diagnostic=>diagnostic.location.edgeIds.includes(route.binding.id)))} marker-end="url(#profile-signal-arrow)" d={route.path}><title>{grouped?`${bundle.members.length} connections: ${nodesById.get(binding.from.nodeId)?.name??binding.from.nodeId} → ${nodesById.get(binding.to.nodeId)?.name??binding.to.nodeId}`:`${portName(binding.from.nodeId,binding.from.portId,'output')} → ${portName(binding.to.nodeId,binding.to.portId,'input')}`}</title></path>
            {#if selectConnection||editConnection||dropOnConnection}
              <path class="signal-edge-hit" data-connection-id={binding.id} data-bundle-id={grouped?bundle.id:undefined} d={route.path} role="button" tabindex="0" aria-label={grouped?`Choose from ${bundle.members.length} connections between ${binding.from.nodeId} and ${binding.to.nodeId}`:`Select connection ${binding.id}: ${binding.from.portId} to ${binding.to.portId}`} aria-pressed={selectedEdgeId===binding.id} onpointerdown={event=>{event.preventDefault();event.stopPropagation();}} onpointerenter={()=>hoveredEdgeId=binding.id} onpointerleave={()=>hoveredEdgeId=''} onclick={event=>grouped?openBundle(event,bundle):connectionClick(event,binding.id)} ondblclick={event=>grouped?openBundle(event,bundle):addWireHandle(event,binding.id)} onkeydown={event=>grouped?bundleKey(event,bundle):connectionKey(event,binding.id)}><title>{grouped?'Choose an individual connection to inspect or edit':'Select to manage connection; double-click to add a reroute point'}</title></path>
            {/if}
            {#if wireStyle==='orthogonal'&&selectedEdgeId===binding.id&&!readOnly}
              {#each route.points.slice(0,-1) as a,index (index)}
                {@const b=route.points[index+1]}{@const horizontal=a.y===b.y}
                {@const line=diagramSegmentLine(route.points,index,route.cornerRadius??cornerRadius)}
                {#if line&&Math.hypot(line.end.x-line.start.x,line.end.y-line.start.y)>12}
                  <path class="connection-segment" class:horizontal data-connection-id={binding.id} data-segment-binding={binding.id} data-segment-index={index} data-segment-axis={horizontal?'horizontal':'vertical'} d={`M${line.start.x},${line.start.y} L${line.end.x},${line.end.y}`} role="button" tabindex="0" aria-label={`Move ${horizontal?'horizontal':'vertical'} segment ${index+1} of ${binding.id}`} onpointerdown={event=>beginSegment(event,binding.id,index)} onclick={event=>event.stopPropagation()} ondblclick={event=>addWireHandle(event,binding.id)} onkeydown={event=>segmentKeyboard(event,binding.id,index)}><title>Drag to move this segment; double-click to add a reroute point</title></path>
                  <circle class="segment-grip" cx={(line.start.x+line.end.x)/2} cy={(line.start.y+line.end.y)/2} r="3"/>
                {/if}
              {/each}
            {/if}
            {#if selectedEdgeId===binding.id&&!readOnly}
              {#each routeHandles(binding.id,route.points) as handle (handle.index)}
                <circle class="connection-reroute" data-reroute-binding={binding.id} data-reroute-index={handle.index} cx={handle.point.x} cy={handle.point.y} r="5" role="button" tabindex="0" aria-label={`Move reroute point ${handle.index+1} of ${binding.id}`} onpointerdown={event=>beginHandle(event,binding.id,handle.index)} onclick={event=>event.stopPropagation()} onkeydown={event=>handleKeyboard(event,binding.id,handle.index)}><title>Drag to reroute; arrow keys adjust; Delete removes this point</title></circle>
              {/each}
            {/if}
            {#if visibleLabel}
              {@const start=route.points[0]}{@const end=route.points.at(-1)!}
              <text class="connection-port-label" x={start.x+12} y={start.y-12} text-anchor="start">{portName(binding.from.nodeId,binding.from.portId,'output')}</text>
              <text class="connection-port-label" x={end.x-12} y={end.y+24} text-anchor="end">{portName(binding.to.nodeId,binding.to.portId,'input')}</text>
              {#if selectedEdgeId===binding.id&&reconnectConnection&&!readOnly}
                <circle class="connection-endpoint" data-reconnect-side="from" data-reconnect-binding={binding.id} cx={start.x} cy={start.y} r="7" role="button" tabindex="0" aria-label={`Reconnect source of ${binding.id}`} onpointerdown={event=>beginReconnect(event,binding.id,'from')} onkeydown={event=>reconnectKey(event,binding.id,'from')}><title>Drag to another output</title></circle>
                <circle class="connection-endpoint" data-reconnect-side="to" data-reconnect-binding={binding.id} cx={end.x} cy={end.y} r="7" role="button" tabindex="0" aria-label={`Reconnect destination of ${binding.id}`} onpointerdown={event=>beginReconnect(event,binding.id,'to')} onkeydown={event=>reconnectKey(event,binding.id,'to')}><title>Drag to another input</title></circle>
              {/if}
            {/if}
            {/if}
          {/snippet}
          {#each renderedRoutes.filter(bundle=>bundle.route.binding.id!==(overview?overviewBundleId:selectedEdgeId)) as bundle (bundle.route.binding.id)}{@render renderWireBundle(bundle)}{/each}
          {#each sharedTrunks as trunk (trunk.id)}
            <path class="signal-trunk" data-trunk-id={trunk.id} data-trunk-direction={trunk.direction} data-trunk-endpoint={JSON.stringify(trunk.endpoint)} data-binding-ids={JSON.stringify(trunk.members.map(route=>route.binding.id))} data-arrow-style={wireStyle} class:highlighted={trunk.members.some(route=>highlightedEdges.includes(route.binding.id))} class:drop-target={paletteConnectionId===trunk.route.binding.id} d={trunk.route.path}><title>{trunk.members.length} connections share this {trunk.direction==='output'?'output':'input'} trunk</title></path>
            <path class="signal-trunk-hit" data-trunk-id={trunk.id} data-bundle-id={trunk.id} data-connection-id={trunk.route.binding.id} d={trunk.route.path} role="button" tabindex="0" aria-label={`Choose from ${trunk.members.length} connections sharing this ${trunk.direction==='output'?'output':'input'}`} onpointerdown={event=>{event.preventDefault();event.stopPropagation();}} onclick={event=>openBundle(event,trunk)} ondblclick={event=>openBundle(event,trunk)} onkeydown={event=>bundleKey(event,trunk)}><title>Choose a signal on this shared trunk</title></path>
          {/each}
          {#each renderedRoutes.filter(bundle=>!overview&&bundle.members.length>1) as bundle (bundle.route.binding.id)}
            <g class="connection-bundle-badge" data-bundle-id={bundle.id} data-connection-id={bundle.route.binding.id} role="button" tabindex="0" aria-label={`Choose from ${bundle.members.length} connections`} use:positionBundleBadge={{path:bundle.route.path,zoom:view.zoom,bindingId:bundle.route.binding.id}} onpointerdown={event=>{event.preventDefault();event.stopPropagation();}} onclick={event=>openBundle(event,bundle)} onkeydown={event=>bundleKey(event,bundle)}>
              <title>{bundle.members.length} connections · Click to choose a signal</title><rect x="-17" y="-10" width="34" height="20" rx="10"/><text text-anchor="middle" dominant-baseline="central">×{bundle.members.length}</text>
            </g>
          {/each}
          {#each trunkBadges as trunk (trunk.id)}
            <g class="connection-trunk-badge" data-trunk-id={trunk.id} data-bundle-id={trunk.id} data-connection-id={trunk.route.binding.id} role="button" tabindex="0" aria-label={`Choose from ${trunk.members.length} connections on shared ${trunk.direction==='output'?'output':'input'}`} use:positionBundleBadge={{path:trunk.route.path,zoom:view.zoom,bindingId:trunk.route.binding.id,trunkId:trunk.id}} onpointerdown={event=>{event.preventDefault();event.stopPropagation();}} onclick={event=>openBundle(event,trunk)} onkeydown={event=>bundleKey(event,trunk)}>
              <title>{trunk.members.length} connections · Click to choose a signal</title><rect x="-17" y="-10" width="34" height="20" rx="10"/><text text-anchor="middle" dominant-baseline="central">×{trunk.members.length}</text>
            </g>
          {/each}
          {#each renderedRoutes.filter(bundle=>bundle.route.binding.id===(overview?overviewBundleId:selectedEdgeId)) as bundle (bundle.route.binding.id)}{@render renderWireBundle(bundle)}{/each}
          {#if wire}{@const pending=wire}{@const from=renderedNodes.find(node=>node.id===pending.nodeId)}{#if from}<path class="pending-wire" d={diagramPendingPath(canvasNodeAnchor(from,pending.direction,pending.portId),pending.end,pending.direction,wireStyle,cornerRadius)}><title>Choose the other block, then select compatible ports</title></path>{#if wireRejectedReason}<text class="wire-drop-error" x={pending.end.x+12} y={pending.end.y-12}>{wireRejectedReason}</text>{/if}{/if}{/if}
        </svg>
        {#each renderedNodes as node (node.id)}
          {@const blockInputs=blockPorts(node,'input')}
          {@const blockOutputs=blockPorts(node,'output')}
          {@const discovering=discoveryBusy.includes(node.id)}
          {@const size=canvasNodeSize(node)}
          <article class:route-related={overviewBundle?node.id===overviewBundle.binding.from.nodeId||node.id===overviewBundle.binding.to.nodeId:routes.some(route=>route.binding.id===selectedEdgeId&&(node.id===route.binding.from.nodeId||node.id===route.binding.to.nodeId))} data-node-id={node.id} class={['resource-block',node.kind,{selected:node.id===selectedId,dimmed:!!search&&!matches.some(match=>match.id===node.id)}]} style={`left:${node.position.x}px;top:${node.position.y}px;width:${size.width}px;height:${size.height}px`} aria-label={`${node.name} · ${node.category}`}>
            {#if node.processing}
              <button class="block-heading transform-heading" aria-pressed={node.id===selectedId} onclick={event=>{if(event.detail===0)choose(node.id);}} onkeydown={event=>moveBlock(event,node.id)}><span class="block-category">Transform</span><strong title={node.name}>{node.name}</strong></button>
              <div class="transform-summary" title={node.processing.draft?'Needs input · '+processingSummary(node.processing):processingSummary(node.processing)}><span class:pending={!!node.processing.draft}>{node.processing.draft?'Needs input · ':''}{processingSummary(node.processing)}</span>{#if nodeControls&&node.id===selectedId}{@render nodeControls(node)}{/if}</div>
              <div class="transform-ports" style={`top:${PROFILE_TRANSFORM_HEADER-1}px;--port-row:${PROFILE_TRANSFORM_PORT_ROW}px`}>
                <div class="transform-port-column">
                  {#each blockInputs as port (port.id)}<button class="transform-port" data-connection-target={node.id} data-connection-port={port.id} data-drop-disabled={inputState(node.id,port.id).full} class:connection-unavailable={inputState(node.id,port.id).full} class:connection-rejected={rejectedHandle(node.id,port.id)} class:connection-active={activeHandle(node.id,'input',port.id)} class:connection-hover={wire?.direction==='output'&&wireTargetId===node.id&&wireTargetPort===port.id} data-connection-active={activeHandle(node.id,'input',port.id)} aria-label={`${port.name} input for ${node.name}`} title={inputState(node.id,port.id).reason||`${port.name} · ${portContractLabel(port.contract)}`} disabled={readOnly||inputState(node.id,port.id).full} onpointerdown={event=>beginWire(event,node.id,'input',port.id)} onclick={event=>wireClick(event,node.id,'input',port.id)}><span class="port-dot" aria-hidden="true"></span><span class="transform-port-name">{port.name}</span></button>{/each}
                </div>
                <div class="transform-port-column">
                  {#each blockOutputs as port (port.id)}<button class="transform-port" data-connection-source={node.id} data-connection-port={port.id} class:connection-active={activeHandle(node.id,'output',port.id)} class:connection-hover={wire?.direction==='input'&&wireTargetId===node.id&&wireTargetPort===port.id} data-connection-active={activeHandle(node.id,'output',port.id)} aria-label={`${port.name} output for ${node.name}`} title={`${port.name} · ${portContractLabel(port.contract)}`} disabled={readOnly} onpointerdown={event=>beginWire(event,node.id,'output',port.id)} onclick={event=>wireClick(event,node.id,'output',port.id)}><span class="transform-port-name">{port.name}</span><span class="port-dot" aria-hidden="true"></span></button>{/each}
                </div>
              </div>
            {:else}
            {#if overview}{#each Object.entries(routeNodes.find(routeNode=>routeNode.id===node.id)?.inputAnchors??{}) as [id,anchor] (id)}<span class="overview-input" class:muted={!!overviewBundleId&&overviewBundleId!==id} style:top={`${anchor.y-node.position.y}px`} title="Route input"></span>{/each}{/if}
            <button class="block-heading" aria-pressed={node.id===selectedId} onclick={event=>{if(event.detail===0)choose(node.id);}} onkeydown={event=>moveBlock(event,node.id)}><span class="block-category">{node.category}</span><strong>{node.name}</strong>{#if !overview}<code>{node.id}</code>{/if}</button>
            <div class="block-body"><span class="block-state">{canvasStatus(node)}</span>{#if discovering}<p class="block-note" role="status">Discovering inputs and outputs…</p>{:else if discoveryErrors[node.id]}<p class="block-note block-discovery-error" title={discoveryErrors[node.id]} role="status">Port discovery failed · {discoveryErrors[node.id]}</p>{:else if node.resource?.kind==='game'}<p class="block-note">Waits for its matching verified game and active profile.</p>{:else if node.resource?.kind==='companion'&&!document.companionAssignments.some(item=>item.resourceId===node.id)}<p class="block-note">Choose a panel to discover its inputs and outputs.</p>{:else}<div class="transport-badges">{#each canvasTransports(node) as transport (transport)}<span>{transport}</span>{/each}{#if !overview&&node.kind==='desktop'}<span>{node.desktop?.actions.length} desktop actions</span>{:else if !overview&&node.kind==='virtual'}<span>{node.virtual?.layout.buttons.length} buttons · {node.virtual?.layout.axes.length} axes</span>{/if}</div>{/if}{#if nodeControls&&node.id===selectedId}{@render nodeControls(node)}{/if}</div>
            <div class="block-port-groups">
              <button data-connection-target={node.id} data-drop-disabled={inputState(node.id).full} class:connection-unavailable={inputState(node.id).full} class:connection-rejected={rejectedHandle(node.id)} class:connection-active={activeHandle(node.id,'input')} class:connection-hover={wire?.direction==='output'&&wireTargetId===node.id} data-connection-active={activeHandle(node.id,'input')} aria-label={`Inputs / actions for ${node.name} (${blockInputs.length})`} title={inputState(node.id).reason||'Drag to Outputs on another block, or click to choose an output'} disabled={readOnly||inputState(node.id).full||(!discovering&&!discoveryErrors[node.id]&&!blockInputs.length)} onpointerdown={event=>beginWire(event,node.id,'input')} onclick={event=>wireClick(event,node.id,'input')}>
                <span class="port-dot" aria-hidden="true"></span><span class="port-group-heading">Inputs / actions {#if !overview}<span>{discovering&&!blockInputs.length?'…':blockInputs.length}</span>{/if}</span>
              </button>
              <button data-connection-source={node.id} class:connection-active={activeHandle(node.id,'output')} class:connection-hover={wire?.direction==='input'&&wireTargetId===node.id} data-connection-active={activeHandle(node.id,'output')} aria-label={`Outputs for ${node.name} (${blockOutputs.length})`} title="Drag to Inputs / actions on another block, or click to choose an input" disabled={readOnly||(!discovering&&!discoveryErrors[node.id]&&!blockOutputs.length)} onpointerdown={event=>beginWire(event,node.id,'output')} onclick={event=>wireClick(event,node.id,'output')}>
                <span class="port-dot" aria-hidden="true"></span><span class="port-group-heading">Outputs {#if !overview}<span>{discovering&&!blockOutputs.length?'…':blockOutputs.length}</span>{/if}</span>
              </button>
            </div>
            {/if}
          </article>
        {/each}
      </div>
      {#if !nodes.length&&!frames.length&&!palettePreview}<div class="empty-canvas">Drag a component from the palette onto the canvas, or click one to add it.</div>{/if}
    </div>
    {#if bundlePicker&&bundleMembers.length}
      <div class="canvas-wire-bundle-picker" role="dialog" aria-label="Bundle connections" tabindex="-1" style={`left:${bundlePicker.x}px;top:${bundlePicker.y}px;width:${bundlePicker.width}px;max-height:${bundlePicker.maxHeight}px`} onkeydown={event=>{if(event.key==='Escape'){event.preventDefault();bundlePicker=null;stage?.focus({preventScroll:true});}}}>
        <div class="bundle-picker-heading"><strong>{bundleMembers.length} connections</strong><button type="button" aria-label="Close bundle connections" title="Close" onclick={()=>{bundlePicker=null;stage?.focus({preventScroll:true});}}><Icon name="close" size={16}/></button></div>
        <p>{bundlePicker.insertion?'Choose the signal to transform':'Choose a signal to inspect or edit'}</p>
        <div class="bundle-picker-members">{#each bundleMembers as member (member.binding.id)}
          <button type="button" data-bundle-binding={member.binding.id} aria-label={`Select connection ${member.binding.id}`} onclick={()=>chooseBundleMember(member.binding.id)}><span><strong>{portName(member.binding.from.nodeId,member.binding.from.portId,'output')}</strong><span aria-hidden="true">→</span><strong>{portName(member.binding.to.nodeId,member.binding.to.portId,'input')}</strong></span><small>{nodesById.get(member.binding.from.nodeId)?.name??member.binding.from.nodeId} → {nodesById.get(member.binding.to.nodeId)?.name??member.binding.to.nodeId}</small><small>{member.binding.id} · {member.binding.from.portId} → {member.binding.to.portId}</small></button>
        {/each}</div>
      </div>
    {/if}
    {#if connectionOpen&&connectionPanel}
      <aside class="canvas-connection-panel" aria-label="Exact connection editor" transition:slideDetails>{@render connectionPanel()}</aside>
    {:else if selected}
    <aside class="canvas-inspector" aria-label="Selected block inspector" data-expanded="true" bind:this={inspectorPanel} transition:slideDetails>
        {#if !simpleInspector}<button class="inspector-close" aria-label="Close component settings" onclick={()=>selectedId=''}><Icon name="close" size={16}/></button>{/if}
        <div class="inspector-content" class:controller-inspector={simpleInspector&&selected.resource?.kind==='device'} id={inspectorContentId} data-inspector-content>
        {#if !simpleInspector}
        <p class="inspector-category">{selected.category}</p><h3>{selected.name}</h3><p class="inspector-state">{canvasStatus(selected)}</p>
        {#if selected.resource}<p class="inspector-note">{selected.resource.required?'Required':'Optional'} resource. Its saved identity and routes remain when disconnected.</p>{/if}
        {#if gameId&&openGameSettings}<button class="text-button game-settings-link" onclick={()=>openGameSettings?.(gameId!,{profileId:document.id,resourceId:selected.id})}>Open game settings <Icon name="arrow" size={12}/></button>{/if}
        {#if selectedDiagnostics.length}<ul class="block-diagnostics" aria-label="Native preflight diagnostics">{#each selectedDiagnostics as diagnostic,index (index)}<li><strong>{diagnostic.code}</strong>: {diagnostic.message}</li>{/each}</ul>{/if}{#if selected.observation?.diagnostics.length}<ul class="block-diagnostics">{#each selected.observation.diagnostics as diagnostic,index (index)}<li>{diagnostic}</li>{/each}</ul>{/if}
        <details open><summary>Selected graph ports · {inputs.length+outputs.length}</summary><p class="inspector-note">Only these exact contracts are saved in the portable graph.</p>{#each portCategories as category (category)}{@const categoryPorts=category==='outputs'?outputs:inputs.filter(port=>(port.contract.delivery==='action')===(category==='actions'))}{@const collapsed=canvasPortCollapsed(document,selected.id,category)}<section class="selected-port-category"><button class="port-category-toggle" aria-label={`${category==='outputs'?'Source outputs':category==='inputs'?'Destination inputs':'Destination actions'} for ${selected.name}`} aria-expanded={!collapsed} disabled={readOnly} onclick={()=>togglePorts(category)}>{collapsed?'▸':'▾'} {category==='outputs'?'Source outputs':category==='inputs'?'Destination inputs':'Destination actions'} ({categoryPorts.length})</button>{#if !collapsed}{#each categoryPorts as port (port.id)}<div class="port-row"><strong>{port.name}</strong><code>{port.id}</code><p>{portContractLabel(port.contract)}</p>{#if selected.resource&&removeProviderPort}<button class="text-button" disabled={readOnly} onclick={()=>removeProviderPort?.(selected!.id,port.id)}>Remove port and its routes</button>{/if}</div>{:else}<p class="inspector-note">No {category} selected. Browse the provider catalogue below.</p>{/each}{/if}</section>{/each}</details>
        {#if selected.observation&&selected.assignment}
          {#if !companionAuthoring||selected.resource?.kind!=='companion'}<details open><summary>Provider catalogue · {selected.observation.portCount} advertised</summary><p class="inspector-note">Browse without changing routes or graph ports. Large catalogues stay paged.</p><button class="text-button" disabled={portsBusy} onclick={()=>loadPorts('open',selected!.assignment!.sourceId)}>Refresh ports</button>{#if portError}<p class="port-error" role="status">{portError}</p>{/if}{#if portsBusy}<p class="inspector-note" role="status">Loading port page…</p>{/if}{#if page}<p class="port-page-count">{page.total?`${page.offset+1}–${page.offset+page.ports.length}`:'0'} of {page.total} cached port contracts</p>{#each ['output','input'] as direction (direction)}<details open><summary>{direction==='output'?'Source outputs / telemetry':'Destination inputs / actions'} on this page ({page.ports.filter(port=>port.descriptor.direction===direction).length})</summary>{#each page.ports.filter(port=>port.descriptor.direction===direction) as port (`${port.descriptor.id}/${port.contractHash}`)}<div class="port-row"><strong>{port.descriptor.name}</strong><code>{port.descriptor.id}</code><p>{portContractLabel(port.descriptor.contract)}</p><small>{port.current?'Current capability':'Cached / unavailable capability'}</small>{#if selectProviderPort}<button class="secondary-button" disabled={readOnly||selected.assignment.ports.some(pin=>pin.observedPortId===port.descriptor.id&&pin.expectedContractHash===port.contractHash)} onclick={()=>selectProviderPort?.(selected!.id,port)}>{selected.assignment.ports.some(pin=>pin.observedPortId===port.descriptor.id&&pin.expectedContractHash===port.contractHash)?'Selected in graph':'Use this exact port'}</button>{/if}</div>{:else}<p class="inspector-note">None on this page.</p>{/each}</details>{/each}<div class="port-pagination"><button class="secondary-button" disabled={portsBusy||!browser.hasPrevious} onclick={()=>loadPorts('previous')}>Previous</button><button class="secondary-button" disabled={portsBusy||!browser.hasNext} onclick={()=>loadPorts('next')}>Next page</button></div>{/if}</details>
          {/if}<details><summary>Connection metadata ({selected.observation.connections.length})</summary><p class="inspector-note">Transport information describes the resource; it is never a canvas wire.</p>{#each selected.observation.connections as connection (`${connection.scope}/${connection.key}/${connection.route.id}`)}<div class="connection-row"><strong>{connection.route.transport.toUpperCase()}</strong><p>{connection.route.baselineReady?'Fresh baseline available':'Waiting for a fresh baseline'} · {connection.route.writable?'Write capability advertised':'No write capability advertised'}</p><dl>{#each Object.entries(connection.route.attributes) as [key,value] (key)}<div><dt>{key}</dt><dd>{value}</dd></div>{/each}</dl></div>{/each}<dl>{#each Object.entries(selected.observation.attributes) as [key,value] (key)}<div><dt>{key}</dt><dd>{value}</dd></div>{/each}</dl></details>
        {:else if selected.resource}<p class="inspector-note">{selected.assignment?'This saved resource is currently absent from inventory. Its block, exact ports and routes remain intact.':'Select a matching resource from inventory before using this copied block.'}</p>{/if}
        {/if}
        {#if inspector}{@render inspector(selected)}{/if}
        </div>
    </aside>
    {/if}
  {#if overview&&(overviewBundle||showTransforms)}
    <div class="canvas-lanes-panel" transition:slideLanes>
    {#key overviewBundle?.id??'transforms'}<ProfileSignalLanes {routeEndpoint} bundle={overviewBundle} nodes={allNodes} bind:activeTransform close={closeLanes} {transformSettings} transfer={transferTransform} insert={(id,before)=>{const target=before?visibleDocument.bindings.find(binding=>binding.to.nodeId===before&&overviewBundle?.lanes.find(lane=>lane.id===id)?.bindingIds.includes(binding.id))?.id??id:id;if(pendingOverviewDrop){dropOnConnection?.(pendingOverviewDrop.component,target,pendingOverviewDrop.position);pendingOverviewDrop=null;}else insertTransform?.(target);}} disconnect={disconnectConnection} {readOnly}/>{/key}
    </div>
  {/if}
  </div>
  {#if !overview}<div class="graph-result-summary"><span>{visibleDocument.resources.length} resource blocks</span><span>{visibleDocument.bindings.length} signal routes</span><span>{visibleDocument.processingNodes.length} transformations</span><span>{controllerEffects.virtual} requested virtual controllers</span><span>{visibleDocument.desktopOutputs?.length??0} desktop endpoint</span><span>{controllerEffects.hidden} hide-while-active requests</span></div>
  <section class="controller-effect-summary" aria-label="Draft controller effects"><strong>Draft controller effects</strong><p>Publication choices: <span>{controllerEffects.enabled} enable while active</span> · <span>{controllerEffects.disabled} disable while active</span> · <span>{controllerEffects.configured} keep configured</span>.</p><p>{controllerEffects.hidden} hide-while-active requests · {controllerEffects.virtual} requested virtual controllers.</p><small>Draft requests. Activation checks device support and ownership; Save applies changes when this profile is active.</small></section>{/if}
</section>

<style>
  .canvas-world.route-focused .signal-edge:not(.selected):not(.drop-target){opacity:.12}.canvas-world.route-focused .resource-block:not(.route-related),.canvas-world.route-focused .editor-group-frame{opacity:.2;filter:saturate(.2)}.overview-input{position:absolute;left:0;transform:translate(-50%,-50%);width:6px;height:6px;border-radius:50%;border:1px solid var(--accent);background:var(--surface);pointer-events:none;z-index:4}.overview-input.muted{opacity:.15}

  /* Zero-blur shadows outline the whole arrow, including its marker, at crossings. */
  .signal-edge{--arrow-outline:drop-shadow(1px 0 0 var(--bg)) drop-shadow(-1px 0 0 var(--bg)) drop-shadow(0 1px 0 var(--bg)) drop-shadow(0 -1px 0 var(--bg));filter:var(--arrow-outline);}
  .signal-edge.overview-edge.selected{stroke:var(--text);filter:var(--arrow-outline) drop-shadow(0 0 4px var(--accent));}.signal-edges marker path.selected{fill:var(--text);}
  .canvas-lanes-panel{position:absolute;inset:auto 0 0;z-index:6;max-height:65%;overflow:auto;overscroll-behavior:contain;background:var(--surface);border-top:1px solid var(--line);box-shadow:0 -10px 24px #0002;}
  .signal-trunk{pointer-events:none;fill:none;stroke:var(--accent);stroke-width:2.5;stroke-linecap:round;stroke-linejoin:round}.signal-trunk.highlighted{stroke:#e0bd7e;stroke-width:4}.signal-trunk.drop-target{stroke-width:4;filter:drop-shadow(0 0 4px color-mix(in srgb,var(--accent) 45%,transparent))}.signal-trunk-hit{pointer-events:stroke;fill:none;stroke:transparent;stroke-width:18;cursor:pointer;outline:none!important}.signal-trunk-hit:hover,.signal-trunk-hit:focus-visible{stroke:var(--accent);stroke-opacity:.22;outline:none!important}
  .connection-trunk-badge{pointer-events:all;cursor:pointer;outline:none}.connection-trunk-badge rect{fill:var(--surface);stroke:var(--accent);stroke-width:1}.connection-trunk-badge text{fill:var(--accent);font-size:11px;font-weight:600;pointer-events:none}.connection-trunk-badge:hover rect,.connection-trunk-badge:focus-visible rect{stroke-width:2;fill:var(--surface-raised)}
  .signal-edge.bundled{stroke-width:2.5}.connection-bundle-badge{pointer-events:all;cursor:pointer;outline:none}.connection-bundle-badge rect{fill:var(--surface);stroke:var(--accent);stroke-width:1}.connection-bundle-badge text{fill:var(--accent);font-size:11px;font-weight:600;pointer-events:none}.connection-bundle-badge:hover rect,.connection-bundle-badge:focus-visible rect{stroke-width:2;fill:var(--surface-raised)}
  .canvas-wire-bundle-picker{position:absolute;z-index:8;display:flex;flex-direction:column;gap:8px;box-sizing:border-box;padding:12px;border:1px solid var(--accent);border-radius:8px;background:var(--surface);box-shadow:0 8px 28px #0005;overflow:hidden}.bundle-picker-heading{display:flex;align-items:center;justify-content:space-between;gap:12px}.bundle-picker-heading>strong{font-size:12px}.bundle-picker-heading button{display:grid;place-items:center;width:26px;height:26px;padding:0;border:0;border-radius:4px;background:transparent;color:var(--soft);cursor:pointer}.bundle-picker-heading button:hover{background:var(--surface-raised)}.canvas-wire-bundle-picker>p{margin:0;font-size:11px;color:var(--muted);line-height:1.4}.bundle-picker-members{display:grid;gap:5px;overflow:auto;min-height:0}.bundle-picker-members>button{display:grid;gap:6px;width:100%;text-align:left;box-sizing:border-box;padding:10px;border:1px solid var(--line);border-radius:5px;background:var(--surface-raised);color:var(--text);cursor:pointer}.bundle-picker-members>button:hover,.bundle-picker-members>button:focus-visible{border-color:var(--accent);outline:1px solid var(--accent);outline-offset:-1px}.bundle-picker-members>button>span{display:grid;grid-template-columns:minmax(0,1fr) auto minmax(0,1fr);align-items:center;gap:8px;font-size:11px}.bundle-picker-members strong{font-size:11px;font-weight:500;overflow-wrap:anywhere}.bundle-picker-members small{font-size:9px;color:var(--muted);overflow-wrap:anywhere}
  .connection-reroute{fill:var(--surface);stroke:var(--accent);stroke-width:2;pointer-events:all;cursor:move;outline:none!important}.connection-reroute:hover,.connection-reroute:focus-visible{fill:var(--accent);stroke:var(--text);outline:none!important}
  .signal-edge.orthogonal{stroke-linejoin:miter}
  .signal-edges,.signal-edge,.signal-edge-hit,.connection-segment{outline:none!important}.signal-edge-hit:focus,.signal-edge-hit:focus-visible,.connection-segment:focus,.connection-segment:focus-visible{outline:none!important}.connection-segment{pointer-events:stroke;fill:none;stroke:transparent;stroke-width:14;cursor:ew-resize}.connection-segment.horizontal{cursor:ns-resize}.connection-segment:hover,.connection-segment:focus-visible{stroke:var(--accent);stroke-opacity:.2}.segment-grip{fill:var(--surface);stroke:var(--accent);stroke-width:1;pointer-events:none}.wire-drop-error{fill:#d3a078;font-size:11px;paint-order:stroke;stroke:var(--bg);stroke-width:4px;pointer-events:none}.canvas-stage.wire-blocked,.canvas-stage.wire-blocked :global(*){cursor:not-allowed!important}.canvas-stage.segment-dragging .connection-segment{stroke-opacity:.12}.transform-port.connection-unavailable,.block-port-groups button.connection-unavailable{cursor:not-allowed;opacity:.45}.transform-port.connection-unavailable .port-dot,.block-port-groups button.connection-unavailable .port-dot{background:var(--line);border-color:var(--muted)}.transform-port.connection-rejected,.block-port-groups button.connection-rejected{background:#d3a07822;box-shadow:inset 0 0 0 1px #d3a078;opacity:.75}
  .canvas-world .resource-block{overflow:visible}.block-heading{border-radius:8px 8px 0 0}.processing .transform-heading{height:52px;padding:9px 12px;gap:3px;border-bottom:0}.transform-heading strong{font-size:12px}.transform-heading .block-category{font-size:7px}.transform-summary{height:26px;flex-shrink:0;padding:0 12px;display:flex;align-items:center;gap:6px;font-size:10px;color:var(--muted);box-sizing:border-box}.transform-summary>span{min-width:0;flex:1;overflow:hidden;text-overflow:ellipsis;white-space:nowrap}.transform-summary .pending{color:#c6a0e5}.transform-ports{position:absolute;left:-1px;right:-1px;display:grid;grid-template-columns:minmax(0,1fr) minmax(0,1fr);padding:0}.transform-port-column{min-width:0}.transform-port{position:relative;display:flex;align-items:center;box-sizing:border-box;height:var(--port-row);width:100%;min-width:0;padding:0 13px;border:0;border-radius:4px;background:transparent;color:var(--soft);font-size:10px;text-align:left;cursor:crosshair}.transform-port[data-connection-source]{justify-content:flex-end;text-align:right}.transform-port-name{overflow:hidden;white-space:nowrap;text-overflow:ellipsis}.transform-port:hover:not(:disabled),.transform-port.connection-active{background:color-mix(in srgb,#b794d2 14%,transparent);color:var(--text)}.transform-port.connection-hover{background:color-mix(in srgb,#b794d2 26%,transparent)}.transform-port:focus-visible{outline:1px solid #b794d2;outline-offset:-2px}.transform-port:disabled{cursor:default;opacity:.55}.port-dot{position:absolute;top:50%;width:8px;height:8px;box-sizing:border-box;border:1px solid var(--accent);border-radius:50%;background:var(--surface);pointer-events:none;z-index:3}[data-connection-target]>.port-dot{left:-1px;transform:translate(-50%,-50%)}[data-connection-source]>.port-dot{right:-1px;transform:translate(50%,-50%)}.transform-port[data-connection-target]>.port-dot{left:0}.transform-port[data-connection-source]>.port-dot{right:0}.transform-port .port-dot{border-color:#c6a0e5}button.connection-active>.port-dot,button:hover:not(:disabled)>.port-dot{background:var(--accent)}.transform-port.connection-active>.port-dot,.transform-port:hover:not(:disabled)>.port-dot{background:#c6a0e5}.block-port-groups button:disabled:not(.connection-unavailable)>.port-dot{display:none}.block-port-groups button:first-child{border-radius:0 0 0 8px}.block-port-groups button:last-child{border-radius:0 0 8px 0}
  .palette-drop-preview{position:absolute;box-sizing:border-box;z-index:4;pointer-events:none;display:flex;flex-direction:column;border:2px dashed var(--accent);border-radius:9px;background:color-mix(in srgb,var(--surface) 90%,var(--accent));box-shadow:0 0 0 4px color-mix(in srgb,var(--accent) 12%,transparent),0 8px 24px #0003;opacity:.88;overflow:hidden;}.preview-heading{display:grid;gap:6px;padding:14px;border-bottom:1px dashed var(--accent);}.preview-heading strong{font-size:13px;white-space:nowrap;overflow:hidden;text-overflow:ellipsis;}.preview-hint{display:flex;align-items:center;justify-content:center;gap:8px;flex:1;color:var(--accent);font-size:12px;}.palette-dropping{outline:1px solid color-mix(in srgb,var(--accent) 45%,transparent);outline-offset:-1px;}
  .signal-edge-hit{pointer-events:stroke;fill:none;stroke:transparent;stroke-width:18;cursor:pointer}.signal-edge-hit:hover,.signal-edge-hit:focus-visible{stroke:var(--accent);stroke-opacity:.22;outline:none;stroke-width:18}
  .canvas-stage.node-dragging, .canvas-stage.node-dragging :global(*) { cursor: grabbing; }
  .controller-effect-summary{margin:0 16px 16px;padding:12px;border:1px solid var(--line);border-radius:6px;font-size:11px;line-height:1.7;}.controller-effect-summary strong{font-size:11px;color:var(--soft)}.controller-effect-summary p{margin:6px 0;color:var(--soft)}.controller-effect-summary small{font-size:10px;color:var(--muted)}
  .editor-group-frame{position:absolute;box-sizing:border-box;border:1px solid color-mix(in srgb,var(--accent) 45%,var(--line));border-radius:10px;background:color-mix(in srgb,var(--accent) 4%,transparent);pointer-events:none;}.editor-group-frame.drop-target{border:2px solid var(--accent);background:color-mix(in srgb,var(--accent) 10%,transparent);}.editor-group-frame.blocked{border-color:#d3a078;}.editor-group-heading{position:absolute;top:0;left:0;right:0;box-sizing:border-box;height:36px;display:flex;align-items:center;gap:8px;padding:0 12px;pointer-events:auto;border:0;border-bottom:1px solid color-mix(in srgb,var(--accent) 25%,var(--line));border-radius:9px 9px 0 0;background:color-mix(in srgb,var(--accent) 8%,var(--surface));color:var(--soft);font-size:12px;text-align:left;cursor:grab;}.editor-group-heading strong{font-size:12px;font-weight:600;overflow:hidden;text-overflow:ellipsis;white-space:nowrap;flex:1;min-width:0;}.editor-group-heading span{font-size:10px;font-weight:400;color:var(--muted);}.editor-group-heading:focus-visible,.editor-group-resize:focus-visible{outline:2px solid var(--accent);outline-offset:2px;}.editor-group-empty{position:absolute;inset:36px 12px 12px;display:grid;place-items:center;font-size:11px;color:var(--muted);}.editor-group-resize{position:absolute;bottom:0;right:0;width:24px;height:24px;pointer-events:auto;background:transparent;color:var(--accent);cursor:nwse-resize;border:0;}.editor-group-resize::after{content:'';position:absolute;width:10px;height:10px;right:5px;bottom:5px;border-right:2px solid currentColor;border-bottom:2px solid currentColor;opacity:.6;}.canvas-layout-error{position:absolute;bottom:12px;left:12px;max-width:calc(100% - 48px);box-sizing:border-box;z-index:4;pointer-events:none;margin:0;padding:8px 12px;border:1px solid var(--line);border-radius:6px;background:var(--surface);box-shadow:0 3px 12px #0003;}.port-category-toggle{display:block;width:100%;text-align:left;background:transparent;border:0;color:inherit;font-weight:600;padding:10px 0;cursor:pointer;}.port-category-toggle:disabled{cursor:default;}
  .signal-edge.problem{stroke:#d3a078;stroke-width:3}.signal-edge.highlighted{stroke:#e0bd7e;stroke-width:4}.pending-wire{fill:none;stroke:var(--accent);stroke-width:2;stroke-dasharray:5 4}.port-row button{font-size:9px;display:block;margin-top:9px}
  .profile-graph{border:1px solid var(--line);border-radius:9px;overflow:hidden;background:var(--surface);}.canvas-toolbar{display:flex;align-items:center;gap:10px;padding:14px;flex-wrap:wrap;border-bottom:1px solid var(--line);}.canvas-search{display:flex;align-items:center;gap:8px;flex:1;min-width:190px;max-width:350px;padding:9px;border:1px solid var(--line);border-radius:5px;background:var(--input-bg);color:var(--muted);}.canvas-search input{flex:1;background:transparent;border:0;color:var(--text);font-size:11px;min-width:0;width:100%;outline:0;}.canvas-search > :global(svg){flex-shrink:0;}.canvas-search:focus-within{outline:2px solid var(--accent);outline-offset:2px;}.search-clear{display:grid;place-items:center;flex-shrink:0;width:20px;height:20px;margin:-3px 0;padding:0;border:0;border-radius:3px;background:transparent;color:var(--muted);cursor:pointer;}.search-clear:hover{background:var(--surface-raised);color:var(--text);}.search-clear:focus-visible{outline:2px solid var(--accent);outline-offset:2px;}.canvas-help{font-size:9px;color:var(--muted);line-height:1.7;padding:10px 14px;margin:0;border-bottom:1px solid var(--line);}.canvas-layout{position:relative;overflow:hidden;}.canvas-stage{position:relative;min-height:620px;height:70vh;max-height:850px;overflow:hidden;touch-action:none;background-color:var(--bg);background-image:radial-gradient(var(--line) 1px,transparent 1px);background-size:20px 20px;}.canvas-stage:focus-visible{outline:2px solid var(--accent);outline-offset:-3px;}.canvas-world{position:absolute;left:0;top:0;transform-origin:0 0;}.signal-edges{position:absolute;overflow:visible;pointer-events:none;}.signal-edge:hover{stroke:var(--text);stroke-width:3}.signal-edge{pointer-events:stroke;stroke-linejoin:round;stroke-linecap:round;fill:none;stroke:var(--accent);stroke-width:2;}.signal-edges marker path{fill:var(--accent);}.resource-block{position:absolute;cursor:grab;border:1px solid var(--line);border-radius:9px;background:var(--surface);box-shadow:0 8px 24px #0002;overflow:hidden;display:flex;flex-direction:column;transition:opacity .12s;}.resource-block.selected{outline:2px solid var(--accent);outline-offset:2px;}.resource-block.processing{border-color:#b794d2;}.resource-block.virtual{border-color:#c6ab6d;}.resource-block.dimmed{opacity:.35;}.block-heading{display:grid;gap:6px;text-align:left;padding:14px;width:100%;background:var(--surface-raised);color:var(--text);border-bottom:1px solid var(--line);cursor:grab;}.block-heading:focus-visible{outline:2px solid var(--accent);outline-offset:-3px;}.block-category{font-size:8px;text-transform:uppercase;letter-spacing:1px;color:var(--accent);}.processing .block-category{color:#c6a0e5;}.virtual .block-category{color:#d3bd85;}.block-heading strong{font-size:13px;white-space:nowrap;overflow:hidden;text-overflow:ellipsis;}.block-heading code{font-size:8px;color:var(--muted);}.block-body{padding:12px 14px;flex:1;min-height:0;}.block-state{font-size:10px;color:var(--soft);}.transport-badges{display:flex;flex-wrap:wrap;gap:6px;margin-top:9px;}.transport-badges span{font-size:8px;line-height:1;border:1px solid var(--line);padding:4px 5px;border-radius:4px;color:var(--muted);}.block-note{font-size:9px;color:var(--muted);line-height:1.55;margin-top:8px;overflow:hidden;display:-webkit-box;-webkit-line-clamp:2;line-clamp:2;-webkit-box-orient:vertical;}.block-port-groups{display:grid;grid-template-columns:1fr 1fr;border-top:1px solid var(--line);}.block-port-groups button{display:flex;justify-content:space-between;gap:8px;background:transparent;color:var(--soft);font-size:9px;padding:11px;}.block-port-groups button+button{border-left:1px solid var(--line);}.block-port-groups span{color:var(--muted);}.canvas-inspector{position:absolute;inset:0 0 0 auto;width:75%;box-sizing:border-box;padding:24px;min-width:0;overflow:auto;border-left:1px solid var(--line);background:var(--surface);box-shadow:-12px 0 32px #0003;z-index:5;}.canvas-inspector h3{font-size:14px;overflow-wrap:anywhere;}.inspector-category{font-size:8px;letter-spacing:1px;text-transform:uppercase;color:var(--accent);margin-bottom:9px;}.inspector-state{font-size:10px;color:var(--soft);margin:10px 0;}.inspector-note{font-size:10px;color:var(--muted);line-height:1.75;margin:10px 0;}.canvas-inspector details{margin:18px 0 0;border-top:1px solid var(--line);padding-top:14px;}.canvas-inspector summary{font-size:10px;color:var(--soft);cursor:pointer;line-height:1.6;}.canvas-inspector details details{margin-top:10px;padding-top:10px;}.port-row{padding:11px 0;border-bottom:1px solid var(--faint-line);overflow-wrap:anywhere;}.port-row strong{display:block;font-size:10px;color:var(--soft);}.port-row code{display:block;font-size:8px;color:var(--muted);margin-top:5px;}.port-row p,.connection-row p{font-size:9px;color:var(--muted);line-height:1.7;margin-top:6px;}.port-row small{font-size:8px;color:var(--muted);}.port-error{font-size:10px;line-height:1.7;color:#d3a078;}.port-page-count{font-size:9px;color:var(--soft);margin-top:10px;}.port-pagination{display:flex;gap:8px;justify-content:space-between;margin-top:13px;}.port-pagination button{font-size:9px;}.connection-row{margin-top:14px;border-bottom:1px solid var(--line);padding-bottom:12px;}.connection-row strong{font-size:10px;color:var(--soft);}dl{font-size:9px;line-height:1.7;margin-top:12px;overflow-wrap:anywhere;}dl>div{margin-top:8px;}dt{color:var(--muted);}dd{margin:0;color:var(--soft);}.block-diagnostics{font-size:9px;color:#c7ad80;line-height:1.8;padding-left:16px;}.game-settings-link{font-size:10px;padding-left:0;}.empty-canvas{position:absolute;top:42%;width:100%;text-align:center;color:var(--muted);font-size:12px;}.graph-result-summary{display:flex;flex-wrap:wrap;gap:18px;padding:13px 16px;border-top:1px solid var(--line);font-size:9px;color:var(--muted);}@media(max-width:1000px){.canvas-stage{height:600px;min-height:400px;}}@media(max-width:600px){.canvas-toolbar{gap:8px;}.canvas-search{max-width:none;flex-basis:100%;}.canvas-stage{height:480px;}}
  .resource-block{box-sizing:border-box;}.block-heading{flex-shrink:0;padding:11px 12px;gap:4px;}.block-body{padding:9px 12px;overflow:hidden;}.block-note{margin:5px 0 0;}.block-discovery-error{color:#d3a078;}.transport-badges{margin-top:6px;flex-wrap:nowrap;overflow:hidden;}.transport-badges span{white-space:nowrap;}.block-port-groups{flex-shrink:0;height:40px;}.block-port-groups button{min-width:0;display:flex;flex-direction:column;align-items:stretch;justify-content:center;text-align:left;padding:10px;cursor:crosshair;}.block-port-groups button:hover:not(:disabled),.block-port-groups button:focus-visible{background:color-mix(in srgb,var(--accent) 8%,var(--surface));color:var(--text);}.block-port-groups button:focus-visible{outline:2px solid var(--accent);outline-offset:-3px;}.block-port-groups button:disabled{cursor:default;opacity:.55;}.block-port-groups .port-group-heading{display:flex;align-items:center;justify-content:space-between;gap:5px;color:var(--soft);font-size:9px;font-weight:600;}.block-port-groups .port-group-heading>span{font-variant-numeric:tabular-nums;color:var(--accent);}.canvas-connection-panel{position:absolute;inset:0 0 0 auto;width:min(420px,100%);max-width:100%;box-sizing:border-box;padding:22px;overflow:auto;border-left:1px solid var(--line);background:var(--surface);box-shadow:-12px 0 32px #0003;z-index:6;}
  .canvas-inspector{width:min(430px,46%);padding:0;overflow:hidden;}
  .canvas-inspector:global([data-expanded="true"]){width:min(430px,46%);}
  .inspector-close{position:absolute;top:9px;right:9px;z-index:2;display:grid;place-items:center;width:26px;height:26px;border:0;border-radius:5px;background:var(--surface-raised);color:var(--soft);cursor:pointer;}
  .inspector-close:hover{color:var(--text);background:var(--line);}
  .inspector-content{height:100%;box-sizing:border-box;padding:24px;overflow:auto;}
  .inspector-content.controller-inspector{display:flex;flex-direction:column;gap:16px;overflow:hidden}
  .controller-inspector :global(.authoring-inspector){flex-shrink:0}
  .signal-edges{z-index:2;}.signal-edge.selected,.signal-edge.drop-target{stroke:var(--accent);stroke-width:4;filter:var(--arrow-outline) drop-shadow(0 0 4px color-mix(in srgb,var(--accent) 45%,transparent));}
  .connection-endpoint{fill:var(--surface);stroke:var(--accent);stroke-width:2;pointer-events:all;cursor:crosshair;}.connection-endpoint:hover,.connection-endpoint:focus-visible{fill:var(--accent);stroke:var(--text);outline:none;}
  .connection-port-label{font-size:10px;fill:var(--text);paint-order:stroke;stroke:var(--bg);stroke-width:5px;stroke-linejoin:round;pointer-events:none;}
  .block-port-groups button{position:relative;transition:background .12s,box-shadow .12s;}.block-port-groups button.connection-active{background:color-mix(in srgb,var(--accent) 18%,var(--surface));box-shadow:inset 0 0 0 1px var(--accent);}.block-port-groups button.connection-hover{background:color-mix(in srgb,var(--accent) 32%,var(--surface));box-shadow:inset 0 0 0 2px var(--accent);}
  @media(max-width:700px){.canvas-inspector,.canvas-inspector:global([data-expanded="true"]){width:min(380px,80%);}}
  @media(prefers-reduced-motion:reduce){.canvas-inspector{transition:none;}}
</style>
