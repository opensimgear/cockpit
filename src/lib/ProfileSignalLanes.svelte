<script lang="ts">
  import type {Snippet} from 'svelte';
  import type {ProfileEndpoint,DeepReadonly} from './profiles';
  import {canvasStyling} from './canvas-styling';
  import {laneEndpointWires,type LaneRect} from './profile-lane-wires';
  import Icon from './Icon.svelte';
  import type {CanvasNode} from './profile-canvas';
  import type {SignalBundle,SignalLane} from './profile-signal-overview';
  import {processingSummary,PROCESSING_PRESETS} from './profile-processing';
  let {bundle,nodes,activeTransform=$bindable(''),close,transformSettings,insert,transfer,disconnect,routeEndpoint,readOnly=false}:
    {bundle?:SignalBundle;nodes:CanvasNode[];activeTransform?:string;close:()=>void;
      transformSettings?:Snippet<[CanvasNode]>;insert?:(id:string,before?:string)=>void;transfer?:(sourceLane:string,nodeId:string,targetLane:string,before:string|null)=>boolean|void;disconnect?:(id:string)=>void;routeEndpoint?:Snippet<[string,'from'|'to',DeepReadonly<ProfileEndpoint>]>;readOnly?:boolean}=$props();
  const wireUid=$props.id();
  const wireMarker=`${wireUid}-lane-arrow`;
  function connectLane(row:HTMLElement,settings:{style:'smooth'|'orthogonal';radius:number;version:string}){
    let frame=0;const svg=row.querySelector<SVGSVGElement>('.lane-wires')!;
    function draw(){
      frame=0;const origin=row.getBoundingClientRect(),track=row.querySelector<HTMLElement>('.chain-track'),destination=row.querySelector<HTMLElement>('[data-endpoint-side="to"]');
      if(!track||!destination||!origin.width)return;
      const rect=(element:Element):LaneRect=>{const box=element.getBoundingClientRect();return{left:box.left-origin.left,right:box.right-origin.left,top:box.top-origin.top,bottom:box.bottom-origin.top};};
      const wires=laneEndpointWires([...row.querySelectorAll('[data-endpoint-side="from"]')].map(rect),[...row.querySelectorAll('.transform-chip')].map(rect),rect(destination),rect(track),settings.style,settings.radius);
      svg.setAttribute('viewBox',`0 0 ${origin.width} ${origin.height}`);svg.replaceChildren();
      for(const wire of wires){const path=document.createElementNS('http://www.w3.org/2000/svg','path');path.setAttribute('d',wire.path);path.setAttribute('marker-end',`url(#${wireMarker})`);path.dataset.laneConnection=wire.side;path.dataset.arrowStyle=settings.style;svg.append(path);const length=path.getTotalLength();path.style.strokeDasharray=`${Math.max(0,length-4)} 4`;}
    }
    function schedule(){if(!frame)frame=requestAnimationFrame(draw);}
    const observer=new ResizeObserver(schedule);observer.observe(row);const track=row.querySelector('.chain-track');if(track)observer.observe(track);
    row.addEventListener('scroll',schedule,true);schedule();
    return{update(next:typeof settings){settings=next;schedule();},destroy(){observer.disconnect();row.removeEventListener('scroll',schedule,true);if(frame)cancelAnimationFrame(frame);}};
  }
  const dragType='application/x-osg-chain-transform';
  let dragged=$state<{laneId:string;nodeId:string}|null>(null),dropTarget=$state(''),dragError=$state('');
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
    try{const value=JSON.parse(event.dataTransfer.getData(dragType));if(typeof value.laneId!=='string'||typeof value.nodeId!=='string')throw Error('Choose a transformation from this chain editor.');
      if(transfer(value.laneId,value.nodeId,laneId,before)===false)dragError='This position has incompatible ports or branched inputs. Choose another position.';else activeTransform='';
    }catch(error){dragError=error instanceof Error?error.message:String(error);}finally{dragged=null;dropTarget='';}
  }
  function moveKey(event:KeyboardEvent,lane:SignalLane,id:string){
    if(!event.altKey||!['ArrowLeft','ArrowRight'].includes(event.key)||readOnly||!transfer)return;
    event.preventDefault();event.stopPropagation();const index=lane.transforms.indexOf(id),to=event.key==='ArrowLeft'?index-1:index+2;
    if(to<0||index===lane.transforms.length-1&&event.key==='ArrowRight')return;
    transfer(lane.id,id,lane.id,lane.transforms[to]??null);
  }
  const byId=$derived(new Map(nodes.map(node=>[node.id,node])));
  const selected=$derived(byId.get(activeTransform));
  const lanes=$derived(bundle?.lanes??[]);
  const transforms=$derived(nodes.filter(node=>node.processing));
  function port(id:string,portId:string){const node=byId.get(id);return [...(node?.resource?.ports??[]),...(node?.virtual?.ports??[]),...(node?.desktop?.ports??[])].find(port=>port.id===portId)?.name??portId;}
  function edit(id:string){activeTransform=activeTransform===id?'':id;}
  function label(node:CanvasNode){const operation=node.processing!.operation;const kind=operation.kind==='builtin'?operation.rule.kind:operation.kind;return PROCESSING_PRESETS.find(([preset])=>preset===kind)?.[1]??kind.replaceAll('-',' ');}
</script>

{#snippet connector()}<svg class="lane-arrow" viewBox="0 0 24 16" width="24" height="16" aria-hidden="true"><path d="M0,8 H24" marker-end={`url(#${wireMarker})`}/></svg>{/snippet}

{#snippet addButton(lane:SignalLane,before?:string)}{#if insert}<button class="insert-transform" title="Add transformation" aria-label={`Add transformation to ${port(lane.destination.nodeId,lane.destination.portId)}`} disabled={readOnly} onclick={()=>insert?.(lane.id,before)}><Icon name="plus" size={16}/></button>{/if}{/snippet}

<!-- svelte-ignore a11y_no_noninteractive_element_interactions (Escape dismisses this region from its child controls.) -->
<section class="signal-lanes" aria-label="Signal lanes" onkeydown={event=>{if(event.key==='Escape'){event.preventDefault();event.stopPropagation();close();}}}>
  <svg class="lane-wire-defs" width="0" height="0" aria-hidden="true"><defs><marker id={wireMarker} markerUnits="userSpaceOnUse" markerWidth="8" markerHeight="8" viewBox="0 0 10 10" refX="10" refY="5" orient="auto"><path d="M0,0 L10,5 L0,10 Z"/></marker></defs></svg>
  <header>{#if bundle}<h3 class="source-device">{byId.get(bundle.binding.from.nodeId)?.name}</h3><span class="connection-direction">{@render connector()}</span><h3 class="destination-device">{byId.get(bundle.binding.to.nodeId)?.name}</h3>{:else}<h3>Transform settings</h3>{/if}</header>
  <div class="lane-scroll">
    {#if bundle}
      <div class="lane-head"><span>Source</span><span>Transform chain</span><span>Destination</span></div>
      {#each lanes as lane (lane.id)}
        <div class="signal-lane" data-lane-id={lane.id} use:connectLane={{style:$canvasStyling.arrowStyle,radius:$canvasStyling.cornerRadius,version:`${lane.transforms.join(',')}/${lane.sources.map(source=>`${source.nodeId}/${source.portId}`).join(',')}/${lane.destination.portId}`}}>
          <svg class="lane-wires" aria-hidden="true"></svg>
          <div class="lane-endpoint">{#each lane.sources as source (`${source.nodeId}/${source.portId}`)}<div data-endpoint-side="from">{#if routeEndpoint}{@render routeEndpoint(lane.id,'from',source)}{:else}<strong>{port(source.nodeId,source.portId)}</strong>{/if}</div>{/each}</div>
          <div class="lane-chain" role="group" aria-label={`Transform chain for ${port(lane.destination.nodeId,lane.destination.portId)}`}>
            <div class="chain-track">
            {#each lane.transforms as id,index (id)}{@const node=byId.get(id)}
              {#if node?.processing}
                {#if index>0}<button class="chain-router" class:drop-target={dropTarget===`${lane.id}:${id}`} aria-label={`Drop transformation before ${node.name}`} title="Insert here, or drop to move within this row or copy from another row" disabled={readOnly} onclick={()=>insert?.(lane.id,id)} ondragover={event=>over(event,lane.id,id)} ondrop={event=>drop(event,lane.id,id)}>{@render connector()}</button>{/if}
                <button class="transform-chip" class:active={activeTransform===id} class:dragging={dragged?.nodeId===id&&dragged.laneId===lane.id} class:drop-target={dropTarget===`${lane.id}:${id}`} draggable={movable(node)} aria-pressed={activeTransform===id} data-transform-id={id} title={`${node.name} · ${processingSummary(node.processing)}${movable(node)?' · Drag to move; drop in another row to copy. Alt + arrows moves within this chain.':''}`} onclick={()=>edit(id)} onkeydown={event=>moveKey(event,lane,id)} ondragstart={event=>startDrag(event,lane.id,node)} ondragend={()=>{dragged=null;dropTarget='';}} ondragover={event=>over(event,lane.id,id)} ondrop={event=>drop(event,lane.id,id)}><strong>{label(node)}</strong><small>{processingSummary(node.processing)}</small></button>
              {/if}
            {:else}<span class="direct">Direct</span>{/each}
            {#if dragged}<button class="chain-router chain-end" class:drop-target={dropTarget===`${lane.id}:`} aria-label="Drop transformation at end of chain" title="Drop at end of chain" disabled={readOnly} ondragover={event=>over(event,lane.id,null)} ondrop={event=>drop(event,lane.id,null)}>{@render connector()}</button>{/if}
          </div>{@render addButton(lane)}</div>
          <div class="lane-endpoint"><div data-endpoint-side="to">{#if routeEndpoint}{@render routeEndpoint(lane.id,'to',lane.destination)}{:else}<strong>{port(lane.destination.nodeId,lane.destination.portId)}</strong>{/if}</div><div class="lane-actions">{#if disconnect}<button class="disconnect-route danger" aria-label={`Disconnect ${port(lane.destination.nodeId,lane.destination.portId)}`} title="Disconnect route" disabled={readOnly} onclick={()=>disconnect?.(lane.id)}><Icon name="unlink" size={16}/></button>{/if}</div></div>
        </div>
      {:else}<p class="empty">No signals.</p>{/each}
    {:else}<div class="transform-library">{#each transforms as node (node.id)}<button class="transform-chip" class:active={activeTransform===node.id} aria-pressed={activeTransform===node.id} data-transform-id={node.id} onclick={()=>edit(node.id)}><strong>{node.name}</strong><small>{node.processing?.draft?'Needs input · ':''}{processingSummary(node.processing!)}</small></button>{:else}<p>No transforms. Drop one from the component palette to start a chain.</p>{/each}</div>{/if}
  </div>
  {#if dragError}<p class="chain-error" role="alert">{dragError}</p>{/if}
  {#if selected?.processing&&transformSettings}<div class="lane-settings" data-transform-settings={selected.id}><div><h4>{selected.name}</h4></div><fieldset disabled={readOnly}>{@render transformSettings(selected)}</fieldset></div>{/if}
</section>

<style>
  .signal-lanes{background:var(--surface)}
  .signal-lanes header{position:sticky;top:0;z-index:2;background:var(--surface);display:grid;grid-template-columns:minmax(0,1fr) 40px minmax(0,1fr);align-items:center;gap:16px;padding:10px 16px;border-bottom:1px solid var(--line)}
  .connection-direction{display:grid;place-items:center;color:var(--accent)}.destination-device{text-align:right}h3,h4{margin:3px 0;font-size:15px}
  .lane-scroll{max-height:340px;overflow:auto}.lane-head,.signal-lane{display:grid;grid-template-columns:minmax(110px,.8fr) minmax(280px,2.5fr) minmax(130px,.9fr);gap:16px;padding:5px 16px;box-sizing:border-box}
  .lane-head{align-items:center;font-size:11px;text-transform:uppercase;letter-spacing:.12em;color:var(--muted);border-bottom:1px solid var(--line)}
  .signal-lane{position:relative;align-items:center;min-height:38px;border-bottom:1px solid var(--line)}.lane-endpoint{display:flex;flex-direction:column;gap:3px;min-width:0}
  .lane-wire-defs{position:absolute;pointer-events:none}.lane-wire-defs path{fill:var(--accent)}.lane-wires{position:absolute;inset:0;width:100%;height:100%;pointer-events:none;overflow:visible}.lane-wires :global(path),.lane-arrow path{fill:none;stroke:var(--accent);stroke-width:2;stroke-linejoin:round;stroke-linecap:round}.lane-arrow{overflow:visible;pointer-events:none}.lane-endpoint,.lane-chain{position:relative}.lane-endpoint{z-index:1}
  .lane-chain,.transform-library{display:flex;gap:6px;align-items:center;flex-wrap:wrap;min-width:0}
  .transform-chip{box-sizing:border-box;background:color-mix(in srgb,#9b80d8 14%,var(--surface));border:1px solid color-mix(in srgb,#9b80d8 40%,var(--line));color:var(--text);border-radius:5px;padding:6px 8px;display:flex;flex-direction:column;gap:2px;max-width:220px;min-width:0;text-align:left;cursor:pointer}
  .transform-chip strong{font-size:12px;line-height:14px}.transform-chip small{font-size:10px;line-height:12px;color:var(--soft);overflow-wrap:anywhere}
  .lane-chain .transform-chip{flex:0 0 112px;width:112px;max-width:100%;height:30px;padding:2px 6px}.lane-chain .transform-chip strong{font-size:11px;line-height:12px}.lane-chain .transform-chip small{font-size:9px;line-height:11px}.lane-chain .transform-chip strong,.lane-chain .transform-chip small{display:block;width:100%;overflow:hidden;text-overflow:ellipsis;white-space:nowrap}
  .transform-chip.active,.transform-chip:hover{border-color:#bfa1ff;background:color-mix(in srgb,#9b80d8 24%,var(--surface))}
  .insert-transform{display:grid;place-items:center;flex:0 0 24px;width:24px;height:24px;padding:0;background:var(--surface);border:1px solid var(--line);color:var(--accent);border-radius:4px;cursor:pointer}.insert-transform:disabled{opacity:.4;cursor:default}
  .lane-chain{flex-wrap:nowrap}.chain-track{display:flex;align-items:center;gap:6px;flex:0 1 auto;min-width:0;overflow-x:auto;scrollbar-width:thin;padding-bottom:2px}.chain-router{display:grid;place-items:center;flex:0 0 24px;width:24px;height:24px;padding:0;border:0;background:transparent;color:var(--muted)}.chain-router:enabled{cursor:copy}.chain-router:disabled{opacity:.5}.transform-chip[draggable=true]{cursor:grab}.transform-chip.dragging{opacity:.35}.drop-target{outline:2px solid var(--accent);outline-offset:-2px}.chain-error{margin:8px 16px;color:#ff8993;font-size:12px}

  .direct{position:relative;background:var(--surface);padding:0 4px;color:var(--muted);font-size:12px;font-style:italic}.lane-endpoint:has(.lane-actions){display:grid;grid-template-columns:minmax(0,1fr) 26px;align-items:center}.lane-actions{display:flex}
  .disconnect-route{display:grid;place-items:center;width:26px;height:26px;padding:0;border:1px solid #bc3441;border-radius:4px;background:#9b2430;color:#fff;cursor:pointer}.disconnect-route:hover:enabled{background:#b32b38;border-color:#e35d69}.disconnect-route:focus-visible{outline:2px solid #ff8993;outline-offset:2px}.disconnect-route:disabled{opacity:.4;cursor:default}
  .transform-library{padding:12px 16px}.lane-settings{padding:14px 16px;display:grid;grid-template-columns:minmax(160px,1fr) minmax(260px,3fr);gap:20px;background:var(--surface-raised);border-top:1px solid var(--line)}fieldset{border:0;padding:0;min-width:0;margin:0}.empty{padding:16px;color:var(--muted)}
  @media(max-width:850px){.lane-head,.signal-lane{grid-template-columns:110px minmax(260px,1fr) 130px;min-width:580px}.lane-settings{grid-template-columns:1fr}}
</style>
