<script lang="ts">
  import type {Snippet} from 'svelte';
  import type {ProfileEndpoint,DeepReadonly} from './profiles';
  import Icon from './Icon.svelte';
  import type {CanvasNode} from './profile-canvas';
  import type {SignalBundle,SignalLane} from './profile-signal-overview';
  import {processingSummary,PROCESSING_PRESETS} from './profile-processing';
  import {slide} from 'svelte/transition';
  let {bundle,nodes,activeTransform=$bindable(''),close,transformSettings,insert,transfer,disconnect,routeEndpoint,sourceInputs,draftConnection,activeIncompleteRowId='',addConnection,addTransformInput,removeTransform,selectTransform,routeIssue,readOnly=false}:
    {bundle?:SignalBundle;nodes:CanvasNode[];activeTransform?:string;close:()=>void;
      transformSettings?:Snippet<[CanvasNode]>;insert?:(id:string,before?:string)=>void;transfer?:(sourceLane:string,nodeId:string,targetLane:string,before:string|null)=>boolean|void;disconnect?:(id:string)=>void;routeEndpoint?:Snippet<[string,'from'|'to',DeepReadonly<ProfileEndpoint>]>;sourceInputs?:Snippet<[SignalLane]>;draftConnection?:Snippet;activeIncompleteRowId?:string;addConnection?:()=>void;addTransformInput?:(laneId:string,nodeId:string)=>void;removeTransform?:(id:string,laneId?:string)=>void;selectTransform?:(laneId:string,id:string)=>string|undefined;routeIssue?:(laneId:string)=>string;readOnly?:boolean}=$props();
  const dragType='application/x-osg-chain-transform';
  let dragged=$state<{laneId:string;nodeId:string}|null>(null),dropTarget=$state(''),dragError=$state(''),dragErrorLane=$state(''),selectedLaneId=$state('');
  function movable(node:CanvasNode){return !!transfer&&!readOnly&&node.processing?.inputs.length===1&&node.processing.outputs.length===1;}
  function startDrag(event:DragEvent,laneId:string,node:CanvasNode){
    if(!movable(node)||!event.dataTransfer){event.preventDefault();return;}
    dragged={laneId,nodeId:node.id};event.dataTransfer.setData(dragType,JSON.stringify(dragged));event.dataTransfer.effectAllowed='copyMove';dragError='';event.stopPropagation();
  }
  function over(event:DragEvent,laneId:string,before:string|null){
    if(readOnly||!transfer||!event.dataTransfer?.types.includes(dragType))return;
    event.preventDefault();event.stopPropagation();dropTarget=`${laneId}:${before??''}`;
    event.dataTransfer.dropEffect=dragged?.laneId===laneId?'move':'copy';
  }
  function drop(event:DragEvent,laneId:string,before:string|null){
    if(readOnly||!transfer||!event.dataTransfer?.types.includes(dragType))return;
    event.preventDefault();event.stopPropagation();
    dragErrorLane=laneId;
    try{const value=JSON.parse(event.dataTransfer.getData(dragType));if(typeof value.laneId!=='string'||typeof value.nodeId!=='string')throw Error('Choose a transformation from this chain editor.');
      if(transfer(value.laneId,value.nodeId,laneId,before)===false)dragError='This position has incompatible ports or branched inputs. Choose another position.';else{dragError='';activeTransform='';}
    }catch(error){dragError=error instanceof Error?error.message:String(error);}finally{dragged=null;dropTarget='';}
  }
  function moveKey(event:KeyboardEvent,lane:SignalLane,id:string){
    if(!event.altKey||!['ArrowLeft','ArrowRight'].includes(event.key)||readOnly||!transfer)return;
    event.preventDefault();event.stopPropagation();const index=lane.transforms.indexOf(id),to=event.key==='ArrowLeft'?index-1:index+2;
    if(to<0||index===lane.transforms.length-1&&event.key==='ArrowRight')return;
    dragErrorLane=lane.id;try{dragError=transfer(lane.id,id,lane.id,lane.transforms[to]??null)===false?'This position has incompatible ports or branched inputs. Choose another position.':'';}catch(error){dragError=error instanceof Error?error.message:String(error);}
  }
  const byId=$derived(new Map(nodes.map(node=>[node.id,node])));
  const selected=$derived(byId.get(activeTransform));
  const lanes=$derived(bundle?.lanes??[]);
  const selectedLane=$derived(lanes.find(lane=>lane.id===selectedLaneId&&lane.transforms.includes(activeTransform))??lanes.find(lane=>lane.transforms.includes(activeTransform)));
  const transforms=$derived(nodes.filter(node=>node.processing));
  function port(id:string,portId:string){const node=byId.get(id);return [...(node?.resource?.ports??[]),...(node?.virtual?.ports??[]),...(node?.desktop?.ports??[])].find(port=>port.id===portId)?.name??portId;}
  function edit(id:string,laneId?:string){
    if(activeTransform===id&&(!laneId||selectedLane?.id===laneId)){activeTransform='';selectedLaneId='';return;}
    selectedLaneId=laneId??'';
    activeTransform=laneId&&selectTransform?selectTransform(laneId,id)??id:id;
  }
  function label(node:CanvasNode){const operation=node.processing!.operation;const kind=operation.kind==='builtin'?operation.rule.kind:operation.kind;return PROCESSING_PRESETS.find(([preset])=>preset===kind)?.[1]??kind.replaceAll('-',' ');}
</script>

{#snippet addButton(lane:SignalLane,before?:string)}{#if insert}<button class="insert-transform" title="Add transformation" aria-label={`Add transformation to ${port(lane.destination.nodeId,lane.destination.portId)}`} disabled={readOnly} onclick={()=>insert?.(lane.id,before)}><Icon name="plus" size={16}/></button>{/if}{/snippet}
{#snippet removeButton(node:CanvasNode,laneId?:string)}{#if removeTransform}<button type="button" class="remove-transform" aria-label={`Delete ${node.name}`} title={`Delete ${node.name}`} disabled={readOnly} onclick={event=>{event.stopPropagation();removeTransform?.(node.id,laneId);}}><Icon name="trash" size={13}/></button>{/if}{/snippet}
{#snippet settingsPanel(node:CanvasNode)}<div class="lane-settings" data-transform-settings={node.id} transition:slide={{duration:140}}><fieldset disabled={readOnly}>{@render transformSettings!(node)}</fieldset></div>{/snippet}

<!-- svelte-ignore a11y_no_noninteractive_element_interactions (Escape dismisses this region from its child controls.) -->
<section class="signal-lanes" aria-label="Signal lanes" onkeydown={event=>{if(event.key==='Escape'){event.preventDefault();event.stopPropagation();close();}}}>
  <header>{#if bundle}<h3 class="source-device">{byId.get(bundle.binding.from.nodeId)?.name}</h3><span class="connection-direction"><Icon name="arrow" size={24}/></span><h3 class="destination-device">{byId.get(bundle.binding.to.nodeId)?.name}</h3>{:else}<h3>Transform settings</h3>{/if}</header>
  <div class="overview-content">
    <div class="lane-scroll">
      {#if bundle}
      <div class="lane-head"><span>Status</span><span>Source</span><span>Transform chain</span><span>Destination</span><span class="actions-heading">Actions</span></div>
      {#each lanes as lane (lane.id)}
        {@const issue=routeIssue?.(lane.id)||(dragErrorLane===lane.id?dragError:'')}
        <div class="signal-lane" class:incomplete-connection={lane.id===activeIncompleteRowId} data-lane-id={lane.id} aria-label={lane.id===activeIncompleteRowId?'Incomplete connection':undefined}>
          {#if lane.id===activeIncompleteRowId&&draftConnection}
          <span class="connection-warning-slot">{#if issue}<span class="route-error-icon" role="img" title={issue} aria-label={`Connection error: ${issue}`}><Icon name="alert" size={16}/></span>{/if}</span>{@render draftConnection()}
          {:else}
          <span class="connection-warning-slot">{#if issue}<span class="route-error-icon" role="img" title={issue} aria-label={`Connection error: ${issue}`}><Icon name="alert" size={16}/></span>{:else}<span class="route-valid-icon" role="img" title="Valid connection" aria-label="Valid connection"><Icon name="check" size={16}/></span>{/if}</span>
          <div class="lane-endpoint">{#if sourceInputs}{@render sourceInputs(lane)}{:else}{#each lane.sources as source (`${source.nodeId}/${source.portId}`)}<div data-endpoint-side="from">{#if routeEndpoint}{@render routeEndpoint(lane.id,'from',source)}{:else}<strong>{port(source.nodeId,source.portId)}</strong>{/if}</div>{/each}
            {#if addTransformInput}{#each lane.transforms as id (id)}{@const node=byId.get(id)}{@const kind=node?.processing?.operation.kind}
              {#if node?.processing&&(kind==='any'||kind==='all'||kind==='priority')}
                <button class="add-transform-input" type="button" aria-label={`Add input to ${node.name}`} disabled={readOnly||node.processing.inputs.length>=8} onclick={()=>addTransformInput?.(lane.id,node.id)}><Icon name="plus" size={14}/>Add</button>
              {/if}
            {/each}{/if}
          {/if}</div>
          <div class="lane-chain" role="group" aria-label={`Transform chain for ${port(lane.destination.nodeId,lane.destination.portId)}`}>
            <div class="chain-track">
            {#each lane.transforms as id,index (id)}{@const node=byId.get(id)}
              {#if node?.processing}
                {#if index>0}<button class="chain-router" class:drop-target={dropTarget===`${lane.id}:${id}`} aria-label={`Drop transformation before ${node.name}`} title="Insert here, or drop to move within this row or copy from another row" disabled={readOnly} onclick={()=>insert?.(lane.id,id)} ondragover={event=>over(event,lane.id,id)} ondrop={event=>drop(event,lane.id,id)}><Icon name="arrow" size={16}/></button>{/if}
                <span class="transform-chip-wrap"><button class="transform-chip" class:active={activeTransform===id&&selectedLane?.id===lane.id} class:dragging={dragged?.nodeId===id&&dragged.laneId===lane.id} class:drop-target={dropTarget===`${lane.id}:${id}`} draggable={movable(node)} aria-pressed={activeTransform===id&&selectedLane?.id===lane.id} data-transform-id={id} title={`${node.name} · ${processingSummary(node.processing)}${movable(node)?' · Drag to move; drop in another row to copy. Alt + arrows moves within this chain.':''}`} onclick={()=>edit(id,lane.id)} onkeydown={event=>moveKey(event,lane,id)} ondragstart={event=>startDrag(event,lane.id,node)} ondragend={()=>{dragged=null;dropTarget='';}} ondragover={event=>over(event,lane.id,id)} ondrop={event=>drop(event,lane.id,id)}><strong>{label(node)}</strong><small>{processingSummary(node.processing)}</small></button>{@render removeButton(node,lane.id)}</span>
              {/if}
            {:else}<span class="direct">Direct</span>{/each}
            {#if dragged}<button class="chain-router chain-end" class:drop-target={dropTarget===`${lane.id}:`} aria-label="Drop transformation at end of chain" title="Drop at end of chain" disabled={readOnly} ondragover={event=>over(event,lane.id,null)} ondrop={event=>drop(event,lane.id,null)}><Icon name="arrow" size={16}/></button>{/if}
          </div>{@render addButton(lane)}</div>
          <div class="lane-endpoint"><div data-endpoint-side="to">{#if routeEndpoint}{@render routeEndpoint(lane.id,'to',lane.destination)}{:else}<strong>{port(lane.destination.nodeId,lane.destination.portId)}</strong>{/if}</div></div><div class="lane-actions">{#if disconnect}<button class="disconnect-route danger" aria-label={`Disconnect ${port(lane.destination.nodeId,lane.destination.portId)}`} title="Disconnect route" disabled={readOnly} onclick={()=>disconnect?.(lane.id)}><Icon name="trash" size={16}/></button>{/if}</div>
          {/if}
        </div>
        {#if selected?.processing&&transformSettings&&selectedLane?.id===lane.id&&lane.transforms.includes(selected.id)}{@render settingsPanel(selected)}{/if}
      {/each}
      {#if draftConnection&&!lanes.some(lane=>lane.id===activeIncompleteRowId)}<div class="signal-lane pending-connection" aria-label="New connection draft">{@render draftConnection()}</div>{#if selected?.processing&&transforms.some(node=>node.id===selected.id)&&transformSettings}{@render settingsPanel(selected)}{/if}{/if}
      {#if addConnection}<div class="connection-footer">{#if !lanes.length&&!draftConnection}<p class="empty">No connections yet.</p>{/if}<button class="add-connection" type="button" disabled={readOnly} onclick={addConnection}><Icon name="plus" size={15}/>Add</button></div>{:else if !lanes.length&&!draftConnection}<p class="empty">No connections yet.</p>{/if}
      {:else}<div class="transform-library">{#each transforms as node (node.id)}<span class="transform-chip-wrap"><button class="transform-chip" class:active={activeTransform===node.id} aria-pressed={activeTransform===node.id} data-transform-id={node.id} onclick={()=>edit(node.id)}><strong>{node.name}</strong><small>{node.processing?.draft?'Needs input · ':''}{processingSummary(node.processing!)}</small></button>{@render removeButton(node)}</span>{:else}<p>No transforms. Drop one from the component palette to start a chain.</p>{/each}</div>{#if selected?.processing&&transformSettings}{@render settingsPanel(selected)}{/if}{/if}
    </div>
  </div>
</section>

<style>
  .signal-lanes{background:var(--surface)}
  .lane-endpoint [data-endpoint-side="to"]{display:flex;align-items:center;gap:6px}.lane-endpoint [data-endpoint-side="to"] :global(.port-combobox){flex:1;min-width:0}.connection-warning-slot{display:grid;place-items:center;justify-self:center;width:18px;min-height:28px;color:#d3a078}
  .signal-lanes header{position:sticky;top:0;z-index:2;background:var(--surface);display:grid;grid-template-columns:minmax(0,1fr) 40px minmax(0,1fr);align-items:center;gap:16px;padding:10px 16px;border-bottom:1px solid var(--line)}
  .connection-direction{display:grid;place-items:center;color:var(--accent)}.destination-device{text-align:right}h3{margin:3px 0;font-size:15px}
  .overview-content{display:grid;grid-template-columns:minmax(0,1fr)}.lane-scroll{max-height:340px;overflow:auto;min-width:0}.lane-head,.signal-lane{display:grid;grid-template-columns:44px minmax(110px,.8fr) minmax(280px,2.5fr) minmax(130px,.9fr) 48px;gap:16px;padding:5px 16px;box-sizing:border-box}
  .lane-head{align-items:center;font-size:var(--ui-text-size);font-weight:700;color:var(--muted);border-bottom:1px solid var(--line)}.actions-heading{text-align:right}
  .signal-lane{align-items:center;min-height:44px;border-bottom:1px solid var(--line)}.lane-endpoint{display:flex;flex-direction:column;gap:3px;min-width:0}
  .lane-chain,.transform-library{display:flex;gap:6px;align-items:center;flex-wrap:wrap;min-width:0}.transform-chip-wrap{position:relative;display:inline-flex;flex:0 0 auto;min-width:0;max-width:100%}
  .transform-chip{box-sizing:border-box;background:color-mix(in srgb,#9b80d8 14%,var(--surface));border:1px solid color-mix(in srgb,#9b80d8 40%,var(--line));color:var(--text);border-radius:5px;padding:6px 8px;display:flex;flex-direction:column;gap:2px;max-width:220px;min-width:0;text-align:left;cursor:pointer}
  .transform-chip strong{font-size:12px;line-height:14px}.transform-chip small{font-size:10px;line-height:12px;color:var(--soft);overflow-wrap:anywhere}
  .lane-chain .transform-chip{flex:0 0 112px;width:112px;max-width:100%;height:30px;padding:2px 25px 2px 6px}.lane-chain .transform-chip strong{font-size:11px;line-height:12px}.lane-chain .transform-chip small{font-size:9px;line-height:11px}.lane-chain .transform-chip strong,.lane-chain .transform-chip small{display:block;width:100%;overflow:hidden;text-overflow:ellipsis;white-space:nowrap}
  .transform-chip.active,.transform-chip:hover{border-color:#bfa1ff;background:color-mix(in srgb,#9b80d8 24%,var(--surface))}
  .insert-transform{display:grid;place-items:center;flex:0 0 24px;width:24px;height:24px;padding:0;background:transparent;border:1px solid var(--line);color:var(--accent);border-radius:4px;cursor:pointer}.insert-transform:disabled{opacity:.4;cursor:default}
  .lane-chain{flex-wrap:nowrap}.chain-track{display:flex;align-items:center;gap:6px;flex:0 1 auto;min-width:0;overflow-x:auto;scrollbar-width:thin;padding-bottom:2px}.chain-router{display:grid;place-items:center;flex:0 0 20px;width:20px;height:28px;padding:0;border:0;background:transparent;color:var(--muted)}.chain-router:enabled{cursor:copy}.chain-router:disabled{opacity:.5}.transform-chip[draggable=true]{cursor:grab}.transform-chip.dragging{opacity:.35}.drop-target{outline:2px solid var(--accent);outline-offset:-2px}

  .direct{color:var(--muted);font-size:12px;font-style:italic}.lane-actions{display:flex;justify-self:end;justify-content:flex-end}
  .disconnect-route{display:grid;place-items:center;width:26px;height:26px;padding:0;border:1px solid #bc3441;border-radius:4px;background:#9b2430;color:#fff;cursor:pointer}.disconnect-route:hover:enabled{background:#b32b38;border-color:#e35d69}.disconnect-route:focus-visible{outline:2px solid #ff8993;outline-offset:2px}.disconnect-route:disabled{opacity:.4;cursor:default}
  .transform-library{padding:12px 16px}.lane-settings{padding:8px 16px;background:var(--surface-raised);border-top:1px solid var(--line);border-bottom:1px solid var(--line);min-width:0}fieldset{border:0;padding:0;min-width:0;margin:0}.empty{padding:16px;color:var(--muted)}.connection-footer{display:flex;flex-direction:column;align-items:center;justify-content:center;padding:4px 16px}.add-connection{margin:6px 0;display:inline-flex;align-items:center;gap:6px;padding:6px 10px;border:1px solid var(--line);border-radius:5px;background:var(--surface-raised);color:var(--text);cursor:pointer}.add-connection:disabled{opacity:.45;cursor:default}.pending-connection{min-height:44px;align-items:center}.pending-connection :global(> *){min-width:0}
  .pending-connection :global(.draft-destination),.incomplete-connection :global(.draft-destination){display:contents}.pending-connection :global(.connection-warning-slot){justify-self:center}
  @media(max-width:850px){.lane-head,.signal-lane{grid-template-columns:44px 110px minmax(260px,1fr) 130px 48px;min-width:788px}}
  .remove-transform{position:absolute;right:4px;top:50%;transform:translateY(-50%);display:grid;place-items:center;width:20px;height:20px;padding:0;border:1px solid #bc3441;border-radius:4px;background:#9b2430;color:#fff;cursor:pointer;opacity:0;pointer-events:none;z-index:1}.transform-chip-wrap:hover .remove-transform,.transform-chip-wrap:focus-within .remove-transform{opacity:1;pointer-events:auto}.remove-transform:focus-visible{outline:2px solid #ff8993;outline-offset:2px}.remove-transform:disabled{opacity:.4;cursor:default}.transform-chip-wrap:has(.remove-transform) .transform-chip{padding-right:25px}
.route-valid-icon{color:var(--accent)}
@media(hover:none){.remove-transform{opacity:1;pointer-events:auto}}
  .add-transform-input{display:inline-flex;align-items:center;align-self:flex-start;gap:5px;min-height:24px;padding:2px 7px;border:1px solid var(--line);border-radius:4px;background:var(--surface-raised);color:var(--soft);font:inherit;font-size:var(--ui-text-size);cursor:pointer}.add-transform-input:hover:enabled{border-color:var(--accent);color:var(--text)}.add-transform-input:disabled{opacity:.45;cursor:default}
</style>
