<script lang="ts">
  import { onDestroy, onMount, untrack, type Snippet } from 'svelte';
  import Icon from './Icon.svelte';
  import type { DeepReadonly, ProfileDocument, ProfileLocalAssignment, ProfilePort, ProfilePortGroup } from './profiles';
  import type { ObservedResourcePort, ProfileInventoryClient, ResourceDetails, ResourceSummary } from './profile-inventory';
  import type { GameId } from './games';
  import { PROFILE_BLOCK_HEIGHT, PROFILE_BLOCK_WIDTH, profileAutoLayout } from './profile-editor';
  import { canvasControllerEffects, canvasCreateGroup, canvasDeleteGroup, canvasFit, canvasGameId, canvasGroupAt, canvasGroups, canvasMoveGroup, canvasMoveIntoGroup, canvasNodes, canvasPan, canvasPortCollapsed, canvasRenameGroup, canvasResizeGroup, canvasSearch, canvasStatus, canvasTogglePorts, canvasTransports, canvasZoom, portContractLabel, type CanvasGroup, type CanvasNode, type CanvasPoint, type CanvasViewport } from './profile-canvas';
  import { diagramRoutes } from './profile-diagram';
  import {processingSummary} from './profile-processing';
  import type{CompileDiagnostic}from'./profile-compilation';
  import { ProfilePortBrowser } from './profile-port-browser';
  import type { ProfilePaletteComponent, ProfilePaletteDrag } from './profile-palette';
  let { document, assignments, inventory, inventoryClient, selectedId = $bindable(''), companionAuthoring = false, readOnly = false, fitOnMount = false, change, openGameSettings, inspector, selectProviderPort, removeProviderPort, connectBlocks, connectionFrom, connectionTo, editConnection, highlightedEdges = [], diagnostics = [], undo, redo, paletteDrag = null, canDropComponent, dropComponent, endPaletteDrag, discoveredPorts = {}, discoveryBusy = [], discoveryErrors = {}, connectionPanel, connectionOpen = false, closeConnectionPanel }: {
    document: DeepReadonly<ProfileDocument>; assignments: readonly DeepReadonly<ProfileLocalAssignment>[]; inventory: readonly DeepReadonly<ResourceSummary>[];
    inventoryClient: Pick<ProfileInventoryClient, 'inspect'>; selectedId?: string; companionAuthoring?:boolean; readOnly?: boolean; fitOnMount?: boolean;
    change: (document: ProfileDocument, label?:string) => void; openGameSettings?: (gameId: GameId, returnState?: unknown) => void;
    inspector?:Snippet<[CanvasNode]>; selectProviderPort?:(nodeId:string,port:DeepReadonly<ObservedResourcePort>)=>void; removeProviderPort?:(nodeId:string,portId:string)=>void;
    connectBlocks?:(fromNodeId:string,toNodeId:string)=>void;connectionFrom?:(nodeId:string)=>void;connectionTo?:(nodeId:string)=>void;highlightedEdges?:readonly string[];undo?:()=>void;redo?:()=>void;
    editConnection?:(bindingId:string)=>void;
    diagnostics?:readonly DeepReadonly<CompileDiagnostic>[];
    paletteDrag?:ProfilePaletteDrag|null;canDropComponent?:(component:ProfilePaletteComponent)=>boolean;
    dropComponent?:(component:ProfilePaletteComponent,position:CanvasPoint)=>void;endPaletteDrag?:()=>void;
    discoveredPorts?:Record<string,readonly DeepReadonly<ProfilePort>[]>;discoveryBusy?:readonly string[];discoveryErrors?:Record<string,string>;
    connectionPanel?:Snippet;connectionOpen?:boolean;closeConnectionPanel?:()=>void;
  } = $props();
  let stage: HTMLDivElement;
  let search = $state('');
  let view = $state<CanvasViewport>({ ...untrack(() => document.editor.viewport) });
  let dragged = $state<{id:string;position:CanvasPoint}|null>(null);
  let wire=$state<{nodeId:string;direction:'input'|'output';pointer:number;start:CanvasPoint;end:CanvasPoint;moved:boolean}|null>(null);
  let suppressWireClick=false;
  let palettePreview=$state<CanvasPoint|null>(null);
  let page = $state<DeepReadonly<ResourceDetails>|null>(null), portsBusy = $state(false), portError = $state('');
  let layoutError=$state('');
  let groupPreview=$state<ProfileDocument|null>(null),groupBlocked=$state(false);
  const portCategories:readonly ProfilePortGroup[]=['outputs','inputs','actions'];
  type CanvasDrag = { id?: string; group?: {id:string;mode:'move'|'resize';frame:CanvasGroup;source:DeepReadonly<ProfileDocument>}; origin: CanvasPoint; start: CanvasPoint; viewport: CanvasViewport; pointer: number; moved: boolean };
  let drag = $state.raw<CanvasDrag|null>(null);
  const browser = new ProfilePortBrowser(untrack(() => inventoryClient));
  const nodes = $derived(canvasNodes(groupPreview??document, assignments, inventory));
  let knownNodeIds=new Set(untrack(()=>nodes.map(node=>node.id)));
  let savedViewportSignature='';
  const controllerEffects=$derived(canvasControllerEffects(document));
  const renderedNodes = $derived(nodes.map(node => node.id === dragged?.id ? { ...node, position: dragged.position } : node));
  const routes=$derived(diagramRoutes(renderedNodes,document.bindings,PROFILE_BLOCK_WIDTH,PROFILE_BLOCK_HEIGHT));
  const frames=$derived(canvasGroups(groupPreview??document,nodes));
  const dropGroup=$derived(dragged?canvasGroupAt(document,{x:dragged.position.x+PROFILE_BLOCK_WIDTH/2,y:dragged.position.y+PROFILE_BLOCK_HEIGHT/2}):palettePreview?canvasGroupAt(document,{x:palettePreview.x+PROFILE_BLOCK_WIDTH/2,y:palettePreview.y+PROFILE_BLOCK_HEIGHT/2}):null);
  const selected = $derived(nodes.find(node => node.id === selectedId));
  const selectedDiagnostics=$derived(diagnostics.filter(diagnostic=>diagnostic.location.nodeId===selectedId||diagnostic.location.resourceId===selectedId||document.bindings.some(binding=>diagnostic.location.edgeIds.includes(binding.id)&&(binding.from.nodeId===selectedId||binding.to.nodeId===selectedId))));
  const matches = $derived(canvasSearch(nodes, search));
  const inputs = $derived(selected?.resource?.ports.filter(port=>port.direction==='input') ?? selected?.processing?.inputs ?? selected?.virtual?.ports ?? selected?.desktop?.ports ?? []);
  const outputs = $derived(selected?.resource?.ports.filter(port=>port.direction==='output') ?? selected?.processing?.outputs ?? []);
  const gameId = $derived(selected ? canvasGameId(selected) : null);
  const observedId = $derived(selected?.observation ? selected.assignment?.sourceId : undefined);
  let portEpoch = 0;
  $effect(() => { const signature=JSON.stringify(document.editor.viewport);if(signature!==savedViewportSignature){savedViewportSignature=signature;view = { ...document.editor.viewport };} });
  $effect(()=>{const added=nodes.find(node=>node.id===selectedId&&!knownNodeIds.has(node.id));knownNodeIds=new Set(nodes.map(node=>node.id));if(added)focusNewNode(added);});
  $effect(()=>{if(!paletteDrag||readOnly||!dropComponent||canDropComponent?.(paletteDrag.component)===false)palettePreview=null;});
  $effect(() => {
    const resourceId = companionAuthoring&&selected?.resource?.kind==='companion'?undefined:observedId;
    portEpoch++; browser.clear(); page = null; portError = ''; portsBusy = false;
    if (resourceId) void loadPorts('open',resourceId);
  });
  onMount(() => { if (fitOnMount) fit(); });
  onDestroy(() => { portEpoch++; browser.clear(); });
  function persist(nextView = view) { if (!readOnly) { const next = JSON.parse(JSON.stringify(document)) as ProfileDocument; next.editor.viewport = { ...nextView }; change(next); } }
  function fit() { const points=[...nodes,...frames.flatMap(frame=>[{position:{x:frame.x,y:frame.y}},{position:{x:frame.x+frame.width-PROFILE_BLOCK_WIDTH,y:frame.y+frame.height-PROFILE_BLOCK_HEIGHT}}])];view = canvasFit(points,stage?.clientWidth || 900,stage?.clientHeight || 600); persist(); }
  function zoom(factor:number,at?:CanvasPoint) { view=canvasZoom(view,factor,at??{x:(stage?.clientWidth||900)/2,y:(stage?.clientHeight||600)/2});persist(); }
  function layout() {
    editLayout(()=>{
      // Freeze old frames before positions change, then reconcile every membership in the same edit.
      let next=nodes.length?canvasMoveIntoGroup(document,nodes[0].id,nodes[0].position):JSON.parse(JSON.stringify(document)) as ProfileDocument;
      next.editor.positions=profileAutoLayout(nodes.map(node=>node.id),document.bindings);
      for(const position of [...next.editor.positions])next=canvasMoveIntoGroup(next,position.nodeId,position);
      const nextNodes=canvasNodes(next,assignments,inventory),nextFrames=canvasGroups(next,nextNodes);
      next.editor.viewport=canvasFit([...nextNodes,...nextFrames.flatMap(frame=>[{position:{x:frame.x,y:frame.y}},{position:{x:frame.x+frame.width-PROFILE_BLOCK_WIDTH,y:frame.y+frame.height-PROFILE_BLOCK_HEIGHT}}])],stage?.clientWidth||900,stage?.clientHeight||600);
      return next;
    },'Auto-layout');
  }
  function choose(id:string) { closeConnectionPanel?.();selectedId=id; }
  function blockPorts(node:CanvasNode,direction:'input'|'output'):readonly DeepReadonly<ProfilePort>[] {
    if(node.resource)return(discoveredPorts[node.id]??node.resource.ports).filter(port=>port.direction===direction);
    if(node.processing)return direction==='output'?node.processing.outputs:node.processing.inputs;
    return(direction==='input'?node.virtual?.ports??node.desktop?.ports:[])??[];
  }
  function wireClick(event:MouseEvent,nodeId:string,direction:'input'|'output'){
    event.stopPropagation();if(suppressWireClick&&event.detail!==0){suppressWireClick=false;return;}if(readOnly)return;
    selectedId=nodeId;if(direction==='output')connectionFrom?.(nodeId);else connectionTo?.(nodeId);
  }
  function clearPalettePreview(){palettePreview=null;}
  function acceptsPaletteDrop(event:DragEvent){
    if(readOnly||!paletteDrag||!dropComponent||canDropComponent?.(paletteDrag.component)===false)return false;
    const target=event.target as Element|null;
    if(target?.closest('.canvas-inspector,.canvas-connection-panel'))return false;
    const rect=stage.getBoundingClientRect();
    return event.clientX>=rect.left&&event.clientX<=rect.right&&event.clientY>=rect.top&&event.clientY<=rect.bottom;
  }
  function palettePosition(event:DragEvent):CanvasPoint{
    const rect=stage.getBoundingClientRect(),clamp=(value:number)=>Math.min(1e6,Math.max(-1e6,value));
    return{x:clamp((event.clientX-rect.left-view.x)/view.zoom-PROFILE_BLOCK_WIDTH/2),y:clamp((event.clientY-rect.top-view.y)/view.zoom-PROFILE_BLOCK_HEIGHT/2)};
  }
  function overPalette(event:DragEvent){
    if(!acceptsPaletteDrop(event)){clearPalettePreview();return;}
    event.preventDefault();if(event.dataTransfer)event.dataTransfer.dropEffect='copy';palettePreview=palettePosition(event);
  }
  function leavePalette(event:DragEvent){if(event.relatedTarget&&stage.contains(event.relatedTarget as Node))return;clearPalettePreview();}
  function dropPalette(event:DragEvent){
    if(!acceptsPaletteDrop(event)){clearPalettePreview();return;}
    event.preventDefault();event.stopPropagation();
    const component=paletteDrag!.component,position=palettePosition(event);clearPalettePreview();
    try{dropComponent?.(component,position);}finally{endPaletteDrag?.();}
  }
  function focusNewNode(node:CanvasNode){view={...view,x:(stage?.clientWidth||900)/2-(node.position.x+PROFILE_BLOCK_WIDTH/2)*view.zoom,y:(stage?.clientHeight||600)/2-(node.position.y+PROFILE_BLOCK_HEIGHT/2)*view.zoom};}
  function slideDetails(_node: HTMLElement) {
    return { duration: window.matchMedia('(prefers-reduced-motion: reduce)').matches ? 0 : 180,
      css: (t: number) => `transform:translateX(${(1-t)*100}%)` };
  }
  function editLayout(edit:()=>ProfileDocument,label:string){if(readOnly)return;layoutError='';try{change(edit(),label);}catch(error){layoutError=error instanceof Error?error.message:String(error);}}
  export function addGroup(name='Group'){if(readOnly)return;layoutError='';try{const next=canvasCreateGroup(document,name,{x:(32-view.x)/view.zoom,y:(32-view.y)/view.zoom});const id=next.editor.groups!.at(-1)!.id;change(next,'Create group');return id;}catch(error){layoutError=error instanceof Error?error.message:String(error);}}
  export function renameGroup(id:string,name:string){editLayout(()=>canvasRenameGroup(document,id,name),'Rename group');}
  export function deleteGroup(id:string){editLayout(()=>canvasDeleteGroup(document,id),'Delete group');}
  export function focusGroup(id:string){const frame=frames.find(group=>group.id===id);if(!frame)return;view=canvasFit([{position:{x:frame.x,y:frame.y}},{position:{x:frame.x+frame.width-PROFILE_BLOCK_WIDTH,y:frame.y+frame.height-PROFILE_BLOCK_HEIGHT}}],stage?.clientWidth||900,stage?.clientHeight||600);persist();}
  function togglePorts(category:ProfilePortGroup){if(selected)editLayout(()=>canvasTogglePorts(document,selected.id,category),canvasPortCollapsed(document,selected.id,category)?'Expand port category':'Collapse port category');}
  function focusMatch() { const node=matches[0];if(!node)return;choose(node.id);view={...view,x:(stage?.clientWidth||900)/2-(node.position.x+PROFILE_BLOCK_WIDTH/2)*view.zoom,y:(stage?.clientHeight||600)/2-(node.position.y+PROFILE_BLOCK_HEIGHT/2)*view.zoom};persist(); }
  function begin(event:PointerEvent,id?:string) {
    if(event.button!==0)return;
    const target = event.target as Element;
    if(target.closest('.signal-edge-hit')||target.closest('button,input,select,a')&&!target.closest('.block-heading'))return;
    id ??= target.closest<HTMLElement>('[data-node-id]')?.dataset.nodeId;
    event.preventDefault();stage.setPointerCapture(event.pointerId);
    drag={id,origin:id?{...nodes.find(node=>node.id===id)!.position}:{x:0,y:0},start:{x:event.clientX,y:event.clientY},viewport:{...view},pointer:event.pointerId,moved:false};
  }
  function beginGroup(event:PointerEvent,frame:CanvasGroup,mode:'move'|'resize'){
    event.stopPropagation();if(readOnly||event.button!==0)return;
    event.preventDefault();stage.setPointerCapture(event.pointerId);layoutError='';groupBlocked=false;
    drag={group:{id:frame.id,mode,frame,source:document},origin:{x:frame.x,y:frame.y},start:{x:event.clientX,y:event.clientY},viewport:{...view},pointer:event.pointerId,moved:false};
  }
  function previewGroup(delta:CanvasPoint){
    if(!drag?.group)return;const{source,id,frame,mode}=drag.group;
    try{groupPreview=mode==='move'?canvasMoveGroup(source,id,{x:frame.x+delta.x/view.zoom,y:frame.y+delta.y/view.zoom}):canvasResizeGroup(source,id,{width:frame.width+delta.x/view.zoom,height:frame.height+delta.y/view.zoom});groupBlocked=false;layoutError='';}
    catch(error){groupBlocked=true;layoutError=error instanceof Error?error.message:String(error);}
  }
  function beginWire(event:PointerEvent,nodeId:string,direction:'input'|'output'){
    if(readOnly||!connectBlocks||event.button!==0)return;event.stopPropagation();event.preventDefault();suppressWireClick=false;
    const node=nodes.find(node=>node.id===nodeId)!;
    wire={nodeId,direction,pointer:event.pointerId,start:{x:event.clientX,y:event.clientY},end:{x:node.position.x+(direction==='output'?PROFILE_BLOCK_WIDTH:0),y:node.position.y+PROFILE_BLOCK_HEIGHT/2},moved:false};stage.setPointerCapture(event.pointerId);
  }
  function move(event:PointerEvent) {if(wire&&event.pointerId===wire.pointer){const rect=stage.getBoundingClientRect();wire={...wire,moved:wire.moved||Math.hypot(event.clientX-wire.start.x,event.clientY-wire.start.y)>=4,end:{x:(event.clientX-rect.left-view.x)/view.zoom,y:(event.clientY-rect.top-view.y)/view.zoom}};return;} if(!drag||event.pointerId!==drag.pointer)return;const delta={x:event.clientX-drag.start.x,y:event.clientY-drag.start.y};if(!drag.moved&&Math.hypot(delta.x,delta.y)<4)return;drag.moved=true;if(drag.group)previewGroup(delta);else if(drag.id&&!readOnly)dragged={id:drag.id,position:{x:drag.origin.x+delta.x/view.zoom,y:drag.origin.y+delta.y/view.zoom}};else if(!drag.id)view=canvasPan(drag.viewport,delta); }
  function finish(event:PointerEvent,cancel=false) {
    if(wire&&event.pointerId===wire.pointer){
      const pending=wire;wire=null;suppressWireClick=true;
      if(!cancel){
        const element=window.document.elementFromPoint(event.clientX,event.clientY);
        const handle=element?.closest<HTMLButtonElement>(pending.direction==='output'?'[data-connection-target]':'[data-connection-source]');
        const target=handle?.disabled?undefined:pending.direction==='output'?handle?.dataset.connectionTarget:handle?.dataset.connectionSource;
        selectedId=pending.nodeId;
        if(target&&target!==pending.nodeId){if(pending.direction==='output')connectBlocks?.(pending.nodeId,target);else connectBlocks?.(target,pending.nodeId);}
        else if(!pending.moved||(element&&stage.contains(element)&&!element.closest('[data-node-id],.canvas-inspector,.canvas-connection-panel'))){if(pending.direction==='output')connectionFrom?.(pending.nodeId);else connectionTo?.(pending.nodeId);}
      }
      if(stage.hasPointerCapture(event.pointerId))stage.releasePointerCapture(event.pointerId);return;
    }
    if(!drag||event.pointerId!==drag.pointer)return;if(!cancel){if(drag.group){if(drag.moved&&groupPreview&&!groupBlocked)change(groupPreview,drag.group.mode==='move'?'Move group':'Resize group');}else if(dragged&&!readOnly)editLayout(()=>canvasMoveIntoGroup(document,dragged!.id,dragged!.position),'Move component');else if(!drag.moved){if(drag.id)choose(drag.id);else selectedId='';}else if(!drag.id)persist();}else view={...drag.viewport};drag=null;dragged=null;groupPreview=null;groupBlocked=false;if(stage.hasPointerCapture(event.pointerId))stage.releasePointerCapture(event.pointerId);
  }
  function wheel(event:WheelEvent) { event.preventDefault();if(event.ctrlKey||event.metaKey){const rect=stage.getBoundingClientRect();zoom(event.deltaY<0?1.1:1/1.1,{x:event.clientX-rect.left,y:event.clientY-rect.top});}else{view=canvasPan(view,{x:-event.deltaX,y:-event.deltaY});persist();} }
  function keyboard(event:KeyboardEvent) {
    if(event.key==='Escape'&&paletteDrag){clearPalettePreview();endPaletteDrag?.();return;}
    if(event.key==='Escape'&&wire){event.preventDefault();const pointer=wire.pointer;wire=null;suppressWireClick=true;if(stage.hasPointerCapture(pointer))stage.releasePointerCapture(pointer);return;}
    if(event.key==='Escape'&&drag){event.preventDefault();const pointer=drag.pointer;view={...drag.viewport};drag=null;dragged=null;groupPreview=null;groupBlocked=false;layoutError='';if(stage.hasPointerCapture(pointer))stage.releasePointerCapture(pointer);return;}
    if((event.target as Element).closest('input,textarea,select,[contenteditable="true"]'))return;
    if(!readOnly&&(undo||redo)&&(event.ctrlKey||event.metaKey)&&['z','y'].includes(event.key.toLowerCase())){event.preventDefault();if(event.shiftKey||event.key.toLowerCase()==='y')redo?.();else undo?.();return;}
    if((event.target as Element).closest('button,a'))return;
    if(event.key==='+'||event.key==='='){event.preventDefault();zoom(1.2);}else if(event.key==='-'){event.preventDefault();zoom(1/1.2);}else if(event.key==='0'){event.preventDefault();fit();}else if(['ArrowLeft','ArrowRight','ArrowUp','ArrowDown'].includes(event.key)){event.preventDefault();view=canvasPan(view,{x:event.key==='ArrowLeft'?40:event.key==='ArrowRight'?-40:0,y:event.key==='ArrowUp'?40:event.key==='ArrowDown'?-40:0});persist();}
  }
  function lostPointer(event:PointerEvent){if(wire?.pointer===event.pointerId){wire=null;suppressWireClick=true;}if(drag?.pointer===event.pointerId){view={...drag.viewport};drag=null;dragged=null;groupPreview=null;groupBlocked=false;}}
  function connectionKey(event:KeyboardEvent,id:string){if(event.key==='Enter'||event.key===' '){event.preventDefault();event.stopPropagation();editConnection?.(id);}}
  function moveBlock(event:KeyboardEvent,id:string) { if(!event.altKey||readOnly||!['ArrowLeft','ArrowRight','ArrowUp','ArrowDown'].includes(event.key))return;event.preventDefault();event.stopPropagation();const node=nodes.find(node=>node.id===id)!;const step=event.shiftKey?64:16;editLayout(()=>canvasMoveIntoGroup(document,id,{x:node.position.x+(event.key==='ArrowLeft'?-step:event.key==='ArrowRight'?step:0),y:node.position.y+(event.key==='ArrowUp'?-step:event.key==='ArrowDown'?step:0)}),'Move component'); }
  function groupKeyboard(event:KeyboardEvent,frame:CanvasGroup,mode:'move'|'resize'){
    if(readOnly||!event.altKey||!['ArrowLeft','ArrowRight','ArrowUp','ArrowDown'].includes(event.key))return;
    event.preventDefault();event.stopPropagation();const step=event.shiftKey?64:16,x=event.key==='ArrowLeft'?-step:event.key==='ArrowRight'?step:0,y=event.key==='ArrowUp'?-step:event.key==='ArrowDown'?step:0;
    editLayout(()=>mode==='move'?canvasMoveGroup(document,frame.id,{x:frame.x+x,y:frame.y+y}):canvasResizeGroup(document,frame.id,{width:frame.width+x,height:frame.height+y}),mode==='move'?'Move group':'Resize group');
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
  <div class="canvas-toolbar"><label class="canvas-search"><Icon name="search" size={14}/><input aria-label="Search profile blocks" placeholder="Find a block, type or transport…" bind:value={search} onkeydown={event=>{if(event.key==='Enter')focusMatch();}}/></label><button class="text-button" onclick={focusMatch} disabled={!matches.length}>{search?`${matches.length} matches`:`${nodes.length} blocks`}</button><button class="secondary-button" onclick={fit}>Fit all</button><button class="secondary-button" disabled={readOnly} onclick={layout}>Auto-layout</button><div class="zoom-controls"><button aria-label="Zoom out" onclick={()=>zoom(1/1.2)}>−</button><output aria-label="Canvas zoom">{Math.round(view.zoom*100)}%</output><button aria-label="Zoom in" onclick={()=>zoom(1.2)}>+</button></div></div>
  <p class="canvas-help" id="profile-canvas-help">Drag the background to pan. Ctrl/⌘ + wheel zooms. Drop components into a group to add them; drag them outside to remove them. Drag a group heading to move it, or its corner to resize. Select a block to inspect ports; Alt + arrow keys move its focused heading.{#if connectBlocks}{' '}Drag Inputs / actions to Outputs, or Outputs to Inputs / actions, then choose the exact ports in the side panel. You can also click either port group. Ctrl/⌘ + Z undoes; Shift + Z redoes.{/if}{' '}Every solid line is an explicit signal route.{#if editConnection}{' '}Select a connection line to insert a transformation.{/if}</p>
  <div class="canvas-layout">
    <!-- svelte-ignore a11y_no_noninteractive_tabindex, a11y_no_noninteractive_element_interactions (The canvas is a keyboard pan/zoom application with instructions.) -->
    <div class="canvas-stage" class:node-dragging={!!dragged||!!groupPreview} class:palette-dropping={!!palettePreview} role="application" aria-label="Profile canvas" aria-describedby="profile-canvas-help" tabindex="0" bind:this={stage} onpointerdown={event=>begin(event)} onpointermove={move} onpointerup={event=>finish(event)} onpointercancel={event=>finish(event,true)} onlostpointercapture={lostPointer} onwheel={wheel} onkeydown={keyboard} ondragover={overPalette} ondragleave={leavePalette} ondrop={dropPalette}>
      {#if layoutError}<p class="canvas-layout-error port-error" role="alert">{layoutError}</p>{/if}
      <div class="canvas-world" style={`transform:translate(${view.x}px,${view.y}px) scale(${view.zoom})`}>
        {#if palettePreview&&paletteDrag}
          <div class="palette-drop-preview" role="img" aria-label="Component drop preview" data-component-kind={paletteDrag.component.kind} data-preview-x={palettePreview.x} data-preview-y={palettePreview.y} style={`left:${palettePreview.x}px;top:${palettePreview.y}px;width:${PROFILE_BLOCK_WIDTH}px;height:${PROFILE_BLOCK_HEIGHT}px`}>
            <div class="preview-heading"><span class="block-category">{paletteDrag.category}</span><strong title={paletteDrag.name}>{paletteDrag.name}</strong></div>
            <span class="preview-hint"><Icon name="plus" size={18}/>Release to add</span>
          </div>
        {/if}
        {#each frames as frame (frame.id)}
          <div class="editor-group-frame" class:drop-target={dropGroup===frame.id} class:blocked={groupBlocked&&drag?.group?.id===frame.id} data-group-id={frame.id} data-group-members={frame.members.join(',')} style={`left:${frame.x}px;top:${frame.y}px;width:${frame.width}px;height:${frame.height}px`}>
            <button class="editor-group-heading" title={`${frame.name} · Drag to move; double-click to focus`} aria-label={`Focus group ${frame.name}`} onpointerdown={event=>beginGroup(event,frame,'move')} onclick={event=>{if(event.detail===0)focusGroup(frame.id);}} ondblclick={()=>focusGroup(frame.id)} onkeydown={event=>groupKeyboard(event,frame,'move')}><Icon name="group" size={14}/><strong>{frame.name}</strong><span>{frame.members.length}</span></button>
            {#if !frame.members.length}<span class="editor-group-empty">Drop components here</span>{/if}
            {#if !readOnly}<button class="editor-group-resize" aria-label={`Resize group ${frame.name}`} title="Drag to resize; Alt + arrow keys to adjust" onpointerdown={event=>beginGroup(event,frame,'resize')} onkeydown={event=>groupKeyboard(event,frame,'resize')}></button>{/if}
          </div>
        {/each}
        <svg class="signal-edges" width="1" height="1" aria-label="Saved signal routes"><defs><marker id="profile-signal-arrow" markerWidth="8" markerHeight="8" refX="7" refY="4" orient="auto"><path d="M0,0 L8,4 L0,8 Z" /></marker></defs>{#each routes as route (route.binding.id)}{@const binding=route.binding}<path class="signal-edge" data-binding-id={binding.id} class:highlighted={highlightedEdges.includes(binding.id)} class:problem={diagnostics.some(diagnostic=>diagnostic.location.edgeIds.includes(binding.id))} marker-end="url(#profile-signal-arrow)" d={route.path}><title>{binding.id}: {binding.from.portId} → {binding.to.portId}</title></path>{#if editConnection}<path class="signal-edge-hit" data-connection-id={binding.id} d={route.path} role="button" tabindex="0" aria-label={`Edit connection ${binding.id}: ${binding.from.portId} to ${binding.to.portId}`} onpointerdown={event=>event.stopPropagation()} onclick={()=>editConnection?.(binding.id)} onkeydown={event=>connectionKey(event,binding.id)}><title>Click to insert a transformation</title></path>{/if}{/each}{#if wire}{@const pending=wire}{@const from=renderedNodes.find(node=>node.id===pending.nodeId)}{#if from}<path class="pending-wire" d={`M${from.position.x+(pending.direction==='output'?PROFILE_BLOCK_WIDTH:0)},${from.position.y+PROFILE_BLOCK_HEIGHT/2} L${pending.end.x},${pending.end.y}`}><title>Choose the other block, then exact ports</title></path>{/if}{/if}</svg>
        {#each renderedNodes as node (node.id)}
          {@const blockInputs=blockPorts(node,'input')}
          {@const blockOutputs=blockPorts(node,'output')}
          {@const discovering=discoveryBusy.includes(node.id)}
          <article data-node-id={node.id} class={['resource-block',node.kind,{selected:node.id===selectedId,dimmed:!!search&&!matches.some(match=>match.id===node.id)}]} style={`left:${node.position.x}px;top:${node.position.y}px;width:${PROFILE_BLOCK_WIDTH}px;height:${PROFILE_BLOCK_HEIGHT}px`} aria-label={`${node.name} · ${node.category}`}>
            <button class="block-heading" aria-pressed={node.id===selectedId} onclick={event=>{if(event.detail===0)choose(node.id);}} onkeydown={event=>moveBlock(event,node.id)}><span class="block-category">{node.category}</span><strong>{node.name}</strong><code>{node.id}</code></button>
            <div class="block-body"><span class="block-state">{canvasStatus(node)}</span>{#if discovering}<p class="block-note" role="status">Discovering inputs and outputs…</p>{:else if discoveryErrors[node.id]}<p class="block-note block-discovery-error" title={discoveryErrors[node.id]} role="status">Port discovery failed · {discoveryErrors[node.id]}</p>{:else if node.resource?.kind==='game'}<p class="block-note">Waits for its matching verified game and active profile.</p>{:else if node.resource?.kind==='companion'&&!document.companionAssignments.some(item=>item.resourceId===node.id)}<p class="block-note">Choose a panel to discover its inputs and outputs.</p>{:else if node.processing}<p class="block-note" title={processingSummary(node.processing)}>{processingSummary(node.processing)}</p>{:else}<div class="transport-badges">{#each canvasTransports(node) as transport (transport)}<span>{transport}</span>{/each}{#if node.kind==='desktop'}<span>{node.desktop?.actions.length} desktop actions</span>{:else if node.kind==='virtual'}<span>{node.virtual?.layout.buttons.length} buttons · {node.virtual?.layout.axes.length} axes</span>{/if}</div>{/if}</div>
            <div class="block-port-groups">
              <button data-connection-target={node.id} aria-label={`Inputs / actions for ${node.name} (${blockInputs.length})`} title="Drag to Outputs on another block, or click to choose an output" disabled={readOnly||(!discovering&&!discoveryErrors[node.id]&&!blockInputs.length)} onpointerdown={event=>beginWire(event,node.id,'input')} onclick={event=>wireClick(event,node.id,'input')}>
                <span class="port-group-heading">Inputs / actions <span>{discovering&&!blockInputs.length?'…':blockInputs.length}</span></span>
                <span class="port-name-preview">{blockInputs.slice(0,2).map(port=>port.name).join(' · ')|| (discovering?'Discovering…':'No inputs')}</span>
                {#if blockInputs.length>2}<small>+{blockInputs.length-2} more</small>{/if}
              </button>
              <button data-connection-source={node.id} aria-label={`Outputs for ${node.name} (${blockOutputs.length})`} title="Drag to Inputs / actions on another block, or click to choose an input" disabled={readOnly||(!discovering&&!discoveryErrors[node.id]&&!blockOutputs.length)} onpointerdown={event=>beginWire(event,node.id,'output')} onclick={event=>wireClick(event,node.id,'output')}>
                <span class="port-group-heading">Outputs <span>{discovering&&!blockOutputs.length?'…':blockOutputs.length}</span></span>
                <span class="port-name-preview">{blockOutputs.slice(0,2).map(port=>port.name).join(' · ')|| (discovering?'Discovering…':'No outputs')}</span>
                {#if blockOutputs.length>2}<small>+{blockOutputs.length-2} more</small>{/if}
              </button>
            </div>
          </article>
        {/each}
      </div>
      {#if !nodes.length&&!frames.length&&!palettePreview}<div class="empty-canvas">Drag a component from the palette onto the canvas, or click one to add it.</div>{/if}
    </div>
    {#if connectionOpen&&connectionPanel}
      <aside class="canvas-connection-panel" aria-label="Exact connection editor" transition:slideDetails>{@render connectionPanel()}</aside>
    {:else if selected}
    <aside class="canvas-inspector" aria-label="Selected block inspector" transition:slideDetails>
        <button class="inspector-close text-button" aria-label="Close node details" onclick={()=>selectedId=''}><Icon name="close" size={18}/></button>
        <p class="inspector-category">{selected.category}</p><h3>{selected.name}</h3><p class="inspector-state">{canvasStatus(selected)}</p>
        {#if selected.resource}<p class="inspector-note">{selected.resource.required?'Required':'Optional'} resource. Its saved identity and routes remain when disconnected.</p>{/if}
        {#if gameId&&openGameSettings}<button class="text-button game-settings-link" onclick={()=>openGameSettings?.(gameId!,{profileId:document.id,resourceId:selected.id})}>Open game settings <Icon name="arrow" size={12}/></button>{/if}
        {#if selectedDiagnostics.length}<ul class="block-diagnostics" aria-label="Native preflight diagnostics">{#each selectedDiagnostics as diagnostic,index (index)}<li><strong>{diagnostic.code}</strong>: {diagnostic.message}</li>{/each}</ul>{/if}{#if selected.observation?.diagnostics.length}<ul class="block-diagnostics">{#each selected.observation.diagnostics as diagnostic,index (index)}<li>{diagnostic}</li>{/each}</ul>{/if}
        <details open><summary>Selected graph ports · {inputs.length+outputs.length}</summary><p class="inspector-note">Only these exact contracts are saved in the portable graph.</p>{#each portCategories as category (category)}{@const categoryPorts=category==='outputs'?outputs:inputs.filter(port=>(port.contract.delivery==='action')===(category==='actions'))}{@const collapsed=canvasPortCollapsed(document,selected.id,category)}<section class="selected-port-category"><button class="port-category-toggle" aria-label={`${category==='outputs'?'Source outputs':category==='inputs'?'Destination inputs':'Destination actions'} for ${selected.name}`} aria-expanded={!collapsed} disabled={readOnly} onclick={()=>togglePorts(category)}>{collapsed?'▸':'▾'} {category==='outputs'?'Source outputs':category==='inputs'?'Destination inputs':'Destination actions'} ({categoryPorts.length})</button>{#if !collapsed}{#each categoryPorts as port (port.id)}<div class="port-row"><strong>{port.name}</strong><code>{port.id}</code><p>{portContractLabel(port.contract)}</p>{#if selected.resource&&removeProviderPort}<button class="text-button" disabled={readOnly} onclick={()=>removeProviderPort?.(selected!.id,port.id)}>Remove port and its routes</button>{/if}</div>{:else}<p class="inspector-note">No {category} selected. Browse the provider catalogue below.</p>{/each}{/if}</section>{/each}</details>
        {#if selected.observation&&selected.assignment}
          {#if !companionAuthoring||selected.resource?.kind!=='companion'}<details open><summary>Provider catalogue · {selected.observation.portCount} advertised</summary><p class="inspector-note">Browse without changing routes or graph ports. Large catalogues stay paged.</p><button class="text-button" disabled={portsBusy} onclick={()=>loadPorts('open',selected!.assignment!.sourceId)}>Refresh ports</button>{#if portError}<p class="port-error" role="status">{portError}</p>{/if}{#if portsBusy}<p class="inspector-note" role="status">Loading port page…</p>{/if}{#if page}<p class="port-page-count">{page.total?`${page.offset+1}–${page.offset+page.ports.length}`:'0'} of {page.total} cached port contracts</p>{#each ['output','input'] as direction (direction)}<details open><summary>{direction==='output'?'Source outputs / telemetry':'Destination inputs / actions'} on this page ({page.ports.filter(port=>port.descriptor.direction===direction).length})</summary>{#each page.ports.filter(port=>port.descriptor.direction===direction) as port (`${port.descriptor.id}/${port.contractHash}`)}<div class="port-row"><strong>{port.descriptor.name}</strong><code>{port.descriptor.id}</code><p>{portContractLabel(port.descriptor.contract)}</p><small>{port.current?'Current capability':'Cached / unavailable capability'}</small>{#if selectProviderPort}<button class="secondary-button" disabled={readOnly||selected.assignment.ports.some(pin=>pin.observedPortId===port.descriptor.id&&pin.expectedContractHash===port.contractHash)} onclick={()=>selectProviderPort?.(selected!.id,port)}>{selected.assignment.ports.some(pin=>pin.observedPortId===port.descriptor.id&&pin.expectedContractHash===port.contractHash)?'Selected in graph':'Use this exact port'}</button>{/if}</div>{:else}<p class="inspector-note">None on this page.</p>{/each}</details>{/each}<div class="port-pagination"><button class="secondary-button" disabled={portsBusy||!browser.hasPrevious} onclick={()=>loadPorts('previous')}>Previous</button><button class="secondary-button" disabled={portsBusy||!browser.hasNext} onclick={()=>loadPorts('next')}>Next page</button></div>{/if}</details>
          {/if}<details><summary>Connection metadata ({selected.observation.connections.length})</summary><p class="inspector-note">Transport information describes the resource; it is never a canvas wire.</p>{#each selected.observation.connections as connection (`${connection.scope}/${connection.key}/${connection.route.id}`)}<div class="connection-row"><strong>{connection.route.transport.toUpperCase()}</strong><p>{connection.route.baselineReady?'Fresh baseline available':'Waiting for a fresh baseline'} · {connection.route.writable?'Write capability advertised':'No write capability advertised'}</p><dl>{#each Object.entries(connection.route.attributes) as [key,value] (key)}<div><dt>{key}</dt><dd>{value}</dd></div>{/each}</dl></div>{/each}<dl>{#each Object.entries(selected.observation.attributes) as [key,value] (key)}<div><dt>{key}</dt><dd>{value}</dd></div>{/each}</dl></details>
        {:else if selected.resource}<p class="inspector-note">{selected.assignment?'This saved resource is currently absent from inventory. Its block, exact ports and routes remain intact.':'Select a matching resource from inventory before using this copied block.'}</p>{/if}
        {#if inspector}{@render inspector(selected)}{/if}
    </aside>
    {/if}
  </div>
  <div class="graph-result-summary"><span>{document.resources.length} resource blocks</span><span>{document.bindings.length} signal routes</span><span>{document.processingNodes.length} transformations</span><span>{controllerEffects.virtual} requested virtual controllers</span><span>{document.desktopOutputs?.length??0} desktop endpoint</span><span>{controllerEffects.hidden} hide-while-active requests</span></div>
  <section class="controller-effect-summary" aria-label="Draft controller effects"><strong>Draft controller effects</strong><p>Publication choices: <span>{controllerEffects.enabled} enable while active</span> · <span>{controllerEffects.disabled} disable while active</span> · <span>{controllerEffects.configured} keep configured</span>.</p><p>{controllerEffects.hidden} hide-while-active requests · {controllerEffects.virtual} requested virtual controllers.</p><small>Draft requests. Activation checks device support and ownership; Save applies changes when this profile is active.</small></section>
</section>

<style>
  .palette-drop-preview{position:absolute;box-sizing:border-box;z-index:4;pointer-events:none;display:flex;flex-direction:column;border:2px dashed var(--accent);border-radius:9px;background:color-mix(in srgb,var(--surface) 90%,var(--accent));box-shadow:0 0 0 4px color-mix(in srgb,var(--accent) 12%,transparent),0 8px 24px #0003;opacity:.88;overflow:hidden;}.preview-heading{display:grid;gap:6px;padding:14px;border-bottom:1px dashed var(--accent);}.preview-heading strong{font-size:13px;white-space:nowrap;overflow:hidden;text-overflow:ellipsis;}.preview-hint{display:flex;align-items:center;justify-content:center;gap:8px;flex:1;color:var(--accent);font-size:12px;}.palette-dropping{outline:1px solid color-mix(in srgb,var(--accent) 45%,transparent);outline-offset:-1px;}
  .signal-edge-hit{pointer-events:stroke;fill:none;stroke:transparent;stroke-width:18;cursor:pointer}.signal-edge-hit:hover,.signal-edge-hit:focus-visible{stroke:var(--accent);stroke-opacity:.22;outline:none;stroke-width:18}
  .canvas-stage.node-dragging, .canvas-stage.node-dragging :global(*) { cursor: grabbing; }
  .controller-effect-summary{margin:0 16px 16px;padding:12px;border:1px solid var(--line);border-radius:6px;font-size:11px;line-height:1.7;}.controller-effect-summary strong{font-size:11px;color:var(--soft)}.controller-effect-summary p{margin:6px 0;color:var(--soft)}.controller-effect-summary small{font-size:10px;color:var(--muted)}
  .editor-group-frame{position:absolute;box-sizing:border-box;border:1px solid color-mix(in srgb,var(--accent) 45%,var(--line));border-radius:10px;background:color-mix(in srgb,var(--accent) 4%,transparent);pointer-events:none;}.editor-group-frame.drop-target{border:2px solid var(--accent);background:color-mix(in srgb,var(--accent) 10%,transparent);}.editor-group-frame.blocked{border-color:#d3a078;}.editor-group-heading{position:absolute;top:0;left:0;right:0;box-sizing:border-box;height:36px;display:flex;align-items:center;gap:8px;padding:0 12px;pointer-events:auto;border:0;border-bottom:1px solid color-mix(in srgb,var(--accent) 25%,var(--line));border-radius:9px 9px 0 0;background:color-mix(in srgb,var(--accent) 8%,var(--surface));color:var(--soft);font-size:12px;text-align:left;cursor:grab;}.editor-group-heading strong{font-size:12px;font-weight:600;overflow:hidden;text-overflow:ellipsis;white-space:nowrap;flex:1;min-width:0;}.editor-group-heading span{font-size:10px;font-weight:400;color:var(--muted);}.editor-group-heading:focus-visible,.editor-group-resize:focus-visible{outline:2px solid var(--accent);outline-offset:2px;}.editor-group-empty{position:absolute;inset:36px 12px 12px;display:grid;place-items:center;font-size:11px;color:var(--muted);}.editor-group-resize{position:absolute;bottom:0;right:0;width:24px;height:24px;pointer-events:auto;background:transparent;color:var(--accent);cursor:nwse-resize;border:0;}.editor-group-resize::after{content:'';position:absolute;width:10px;height:10px;right:5px;bottom:5px;border-right:2px solid currentColor;border-bottom:2px solid currentColor;opacity:.6;}.canvas-layout-error{position:absolute;bottom:12px;left:12px;max-width:calc(100% - 48px);box-sizing:border-box;z-index:4;pointer-events:none;margin:0;padding:8px 12px;border:1px solid var(--line);border-radius:6px;background:var(--surface);box-shadow:0 3px 12px #0003;}.port-category-toggle{display:block;width:100%;text-align:left;background:transparent;border:0;color:inherit;font-weight:600;padding:10px 0;cursor:pointer;}.port-category-toggle:disabled{cursor:default;}
  .signal-edge.problem{stroke:#d3a078;stroke-width:3}.signal-edge.highlighted{stroke:#e0bd7e;stroke-width:4}.pending-wire{fill:none;stroke:var(--accent);stroke-width:2;stroke-dasharray:5 4}.port-row button{font-size:9px;display:block;margin-top:9px}
  .profile-graph{border:1px solid var(--line);border-radius:9px;overflow:hidden;background:var(--surface);}.canvas-toolbar{display:flex;align-items:center;gap:10px;padding:14px;flex-wrap:wrap;border-bottom:1px solid var(--line);}.canvas-search{display:flex;align-items:center;gap:8px;flex:1;min-width:190px;max-width:350px;color:var(--muted);}.canvas-search input{background:transparent;border:0;color:var(--text);font-size:11px;min-width:0;width:100%;outline:0;}.canvas-search:focus-within{outline:1px solid var(--accent);outline-offset:6px;border-radius:2px;}.canvas-toolbar button{font-size:10px;}.zoom-controls{display:flex;align-items:center;border:1px solid var(--line);border-radius:6px;margin-left:auto;}.zoom-controls button{width:30px;height:30px;background:transparent;color:var(--soft);font-size:16px;}.zoom-controls output{min-width:44px;text-align:center;font-size:10px;color:var(--muted);}.canvas-help{font-size:9px;color:var(--muted);line-height:1.7;padding:10px 14px;margin:0;border-bottom:1px solid var(--line);}.canvas-layout{position:relative;overflow:hidden;}.canvas-stage{position:relative;min-height:620px;height:70vh;max-height:850px;overflow:hidden;touch-action:none;background-color:var(--bg);background-image:radial-gradient(var(--line) 1px,transparent 1px);background-size:20px 20px;}.canvas-stage:focus-visible{outline:2px solid var(--accent);outline-offset:-3px;}.canvas-world{position:absolute;left:0;top:0;transform-origin:0 0;}.signal-edges{position:absolute;overflow:visible;pointer-events:none;}.signal-edge:hover{stroke:var(--text);stroke-width:3}.signal-edge{pointer-events:stroke;stroke-linejoin:round;stroke-linecap:round;fill:none;stroke:var(--accent);stroke-width:2;}.signal-edges marker path{fill:var(--accent);}.resource-block{position:absolute;cursor:grab;border:1px solid var(--line);border-radius:9px;background:var(--surface);box-shadow:0 8px 24px #0002;overflow:hidden;display:flex;flex-direction:column;transition:opacity .12s;}.resource-block.selected{outline:2px solid var(--accent);outline-offset:2px;}.resource-block.processing{border-color:#b794d2;}.resource-block.virtual{border-color:#c6ab6d;}.resource-block.dimmed{opacity:.35;}.block-heading{display:grid;gap:6px;text-align:left;padding:14px;width:100%;background:var(--surface-raised);color:var(--text);border-bottom:1px solid var(--line);cursor:grab;}.block-heading:focus-visible{outline:2px solid var(--accent);outline-offset:-3px;}.block-category{font-size:8px;text-transform:uppercase;letter-spacing:1px;color:var(--accent);}.processing .block-category{color:#c6a0e5;}.virtual .block-category{color:#d3bd85;}.block-heading strong{font-size:13px;white-space:nowrap;overflow:hidden;text-overflow:ellipsis;}.block-heading code{font-size:8px;color:var(--muted);}.block-body{padding:12px 14px;flex:1;min-height:0;}.block-state{font-size:10px;color:var(--soft);}.transport-badges{display:flex;flex-wrap:wrap;gap:6px;margin-top:9px;}.transport-badges span{font-size:8px;line-height:1;border:1px solid var(--line);padding:4px 5px;border-radius:4px;color:var(--muted);}.block-note{font-size:9px;color:var(--muted);line-height:1.55;margin-top:8px;overflow:hidden;display:-webkit-box;-webkit-line-clamp:2;line-clamp:2;-webkit-box-orient:vertical;}.block-port-groups{display:grid;grid-template-columns:1fr 1fr;border-top:1px solid var(--line);}.block-port-groups button{display:flex;justify-content:space-between;gap:8px;background:transparent;color:var(--soft);font-size:9px;padding:11px;}.block-port-groups button+button{border-left:1px solid var(--line);}.block-port-groups span{color:var(--muted);}.canvas-inspector{position:absolute;inset:0 0 0 auto;width:75%;box-sizing:border-box;padding:24px;min-width:0;overflow:auto;border-left:1px solid var(--line);background:var(--surface);box-shadow:-12px 0 32px #0003;z-index:5;}.inspector-close{float:right;margin:-8px -8px 8px 12px;padding:8px;display:grid;place-items:center;}.canvas-inspector h3{font-size:14px;overflow-wrap:anywhere;}.inspector-category{font-size:8px;letter-spacing:1px;text-transform:uppercase;color:var(--accent);margin-bottom:9px;}.inspector-state{font-size:10px;color:var(--soft);margin:10px 0;}.inspector-note{font-size:10px;color:var(--muted);line-height:1.75;margin:10px 0;}.canvas-inspector details{margin:18px 0 0;border-top:1px solid var(--line);padding-top:14px;}.canvas-inspector summary{font-size:10px;color:var(--soft);cursor:pointer;line-height:1.6;}.canvas-inspector details details{margin-top:10px;padding-top:10px;}.port-row{padding:11px 0;border-bottom:1px solid var(--faint-line);overflow-wrap:anywhere;}.port-row strong{display:block;font-size:10px;color:var(--soft);}.port-row code{display:block;font-size:8px;color:var(--muted);margin-top:5px;}.port-row p,.connection-row p{font-size:9px;color:var(--muted);line-height:1.7;margin-top:6px;}.port-row small{font-size:8px;color:var(--muted);}.port-error{font-size:10px;line-height:1.7;color:#d3a078;}.port-page-count{font-size:9px;color:var(--soft);margin-top:10px;}.port-pagination{display:flex;gap:8px;justify-content:space-between;margin-top:13px;}.port-pagination button{font-size:9px;}.connection-row{margin-top:14px;border-bottom:1px solid var(--line);padding-bottom:12px;}.connection-row strong{font-size:10px;color:var(--soft);}dl{font-size:9px;line-height:1.7;margin-top:12px;overflow-wrap:anywhere;}dl>div{margin-top:8px;}dt{color:var(--muted);}dd{margin:0;color:var(--soft);}.block-diagnostics{font-size:9px;color:#c7ad80;line-height:1.8;padding-left:16px;}.game-settings-link{font-size:10px;padding-left:0;}.empty-canvas{position:absolute;top:42%;width:100%;text-align:center;color:var(--muted);font-size:12px;}.graph-result-summary{display:flex;flex-wrap:wrap;gap:18px;padding:13px 16px;border-top:1px solid var(--line);font-size:9px;color:var(--muted);}@media(max-width:1000px){.canvas-stage{height:600px;min-height:400px;}}@media(max-width:600px){.canvas-toolbar{gap:8px;}.canvas-search{max-width:none;flex-basis:100%;}.zoom-controls{margin-left:0;}.canvas-stage{height:480px;}}
  .resource-block{box-sizing:border-box;}.block-heading{flex-shrink:0;padding:11px 12px;gap:4px;}.block-body{padding:9px 12px;overflow:hidden;}.block-note{margin:5px 0 0;}.block-discovery-error{color:#d3a078;}.transport-badges{margin-top:6px;flex-wrap:nowrap;overflow:hidden;}.transport-badges span{white-space:nowrap;}.block-port-groups{flex-shrink:0;height:84px;}.block-port-groups button{min-width:0;display:flex;flex-direction:column;align-items:stretch;justify-content:flex-start;gap:6px;text-align:left;padding:10px;cursor:crosshair;}.block-port-groups button:hover:not(:disabled),.block-port-groups button:focus-visible{background:color-mix(in srgb,var(--accent) 8%,var(--surface));color:var(--text);}.block-port-groups button:focus-visible{outline:2px solid var(--accent);outline-offset:-3px;}.block-port-groups button:disabled{cursor:default;opacity:.55;}.block-port-groups .port-group-heading{display:flex;align-items:center;justify-content:space-between;gap:5px;color:var(--soft);font-size:9px;font-weight:600;}.block-port-groups .port-group-heading>span{font-variant-numeric:tabular-nums;color:var(--accent);}.port-name-preview{font-size:9px;line-height:1.5;overflow:hidden;display:-webkit-box;-webkit-line-clamp:2;line-clamp:2;-webkit-box-orient:vertical;}.block-port-groups small{font-size:8px;color:var(--muted);}.canvas-connection-panel{position:absolute;inset:0 0 0 auto;width:min(420px,100%);max-width:100%;box-sizing:border-box;padding:22px;overflow:auto;border-left:1px solid var(--line);background:var(--surface);box-shadow:-12px 0 32px #0003;z-index:6;}
</style>
