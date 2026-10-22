<script lang="ts">
  import type {Snippet} from 'svelte';
  import Icon from './Icon.svelte';
  import type {CanvasNode} from './profile-canvas';
  import type {SignalBundle} from './profile-signal-overview';
  import {processingSummary,PROCESSING_PRESETS} from './profile-processing';
  let {bundle,nodes,activeTransform=$bindable(''),close,transformSettings,insert,disconnect,readOnly=false,insertionPending=false}:
    {bundle?:SignalBundle;nodes:CanvasNode[];activeTransform?:string;close:()=>void;
      transformSettings?:Snippet<[CanvasNode]>;insert?:(id:string)=>void;disconnect?:(id:string)=>void;readOnly?:boolean;insertionPending?:boolean}=$props();
  let query=$state('');
  let choosingInsertTarget=$state(false);
  function addTransform(){if(lanes.length===1)insert?.(lanes[0].id);else choosingInsertTarget=!choosingInsertTarget;}
  function insertInto(id:string){choosingInsertTarget=false;insert?.(id);}
  const byId=$derived(new Map(nodes.map(node=>[node.id,node])));
  const selected=$derived(byId.get(activeTransform));
  const lanes=$derived(bundle?.lanes.filter(lane=>[...lane.sources.map(source=>`${byId.get(source.nodeId)?.name} ${port(source.nodeId,source.portId)}`),port(lane.destination.nodeId,lane.destination.portId),...lane.transforms.map(id=>byId.get(id)?.name)].join(' ').toLowerCase().includes(query.toLowerCase()))??[]);
  const transforms=$derived(nodes.filter(node=>node.processing&&`${node.name} ${processingSummary(node.processing)}`.toLowerCase().includes(query.toLowerCase())));
  function port(id:string,portId:string){const node=byId.get(id);return [...(node?.resource?.ports??[]),...(node?.virtual?.ports??[]),...(node?.desktop?.ports??[])].find(port=>port.id===portId)?.name??portId;}
  function edit(id:string){activeTransform=activeTransform===id?'':id;}
  function label(node:CanvasNode){const operation=node.processing!.operation;const kind=operation.kind==='builtin'?operation.rule.kind:operation.kind;return PROCESSING_PRESETS.find(([preset])=>preset===kind)?.[1]??kind.replaceAll('-',' ');}
</script>

<!-- svelte-ignore a11y_no_noninteractive_element_interactions (Escape dismisses this region from its child controls.) -->
<section class="signal-lanes" aria-label="Signal lanes" onkeydown={event=>{if(event.key==='Escape'){event.preventDefault();event.stopPropagation();close();}}}>
  <header>{#if bundle}<h3 class="source-device">{byId.get(bundle.binding.from.nodeId)?.name}</h3><span class="connection-direction"><Icon name="arrow" size={24}/></span><h3 class="destination-device">{byId.get(bundle.binding.to.nodeId)?.name}</h3>{:else}<h3>Transform settings</h3>{/if}</header>
  <div class="lane-toolbar"><input aria-label="Filter signal lanes" placeholder={bundle?'Find a signal or transform…':'Find a transform…'} bind:value={query}/>{#if query}<button class="text-button" aria-label="Clear signal lane search" onclick={()=>query=''}><Icon name="close" size={14}/></button>{/if}<span>{insertionPending?'Use + in Transform chain to choose a signal for the dropped transform.':bundle?'Select a transform to edit it. Shared stages update every consumer.':'Includes shared stages and unfinished chains.'}</span></div>
  <div class="lane-scroll">
    {#if bundle}
      <div class="lane-head"><span>Source</span><div class="chain-heading"><span>Transform chain</span>{#if insert}<button class="insert-transform" title="Add transformation" aria-label="Add transformation" aria-expanded={choosingInsertTarget} disabled={readOnly || !lanes.length} onclick={addTransform}><Icon name="plus" size={16}/></button>{/if}
        {#if choosingInsertTarget}<div class="insert-targets" role="group" aria-label="Choose signal for transformation">{#each lanes as lane (lane.id)}<button onclick={()=>insertInto(lane.id)}>{port(lane.destination.nodeId,lane.destination.portId)}</button>{/each}</div>{/if}
      </div><span>Destination</span></div>
      {#each lanes as lane (lane.id)}
        <div class="signal-lane" data-lane-id={lane.id}>
          <div class="lane-endpoint">{#each lane.sources as source (`${source.nodeId}/${source.portId}`)}<strong>{port(source.nodeId,source.portId)}</strong>{/each}</div>
          <div class="lane-chain">{#each lane.transforms as id (id)}{@const node=byId.get(id)}{#if node?.processing}<button class="transform-chip" class:active={activeTransform===id} aria-pressed={activeTransform===id} data-transform-id={id} title={`${node.name} · ${processingSummary(node.processing)}${node.processing.inputs.length>1?' · Combined inputs':''}`} onclick={()=>edit(id)}><strong>{label(node)}</strong><small>{processingSummary(node.processing)}</small>{#if node.processing.inputs.length>1}<em>Combined inputs</em>{/if}</button>{/if}{:else}<span class="direct">Direct</span>{/each}</div>
          <div class="lane-endpoint"><strong>{port(lane.destination.nodeId,lane.destination.portId)}</strong><div class="lane-actions">{#if disconnect}<button class="disconnect-route danger" aria-label={`Disconnect ${port(lane.destination.nodeId,lane.destination.portId)}`} title="Disconnect route" disabled={readOnly} onclick={()=>disconnect?.(lane.id)}><Icon name="unlink" size={16}/></button>{/if}</div></div>
        </div>
      {:else}<p class="empty">No matching signals.</p>{/each}
    {:else}<div class="transform-library">{#each transforms as node (node.id)}<button class="transform-chip" class:active={activeTransform===node.id} aria-pressed={activeTransform===node.id} data-transform-id={node.id} onclick={()=>edit(node.id)}><strong>{node.name}</strong><small>{node.processing?.draft?'Needs input · ':''}{processingSummary(node.processing!)}</small></button>{:else}<p>No matching transforms. Drop one from the component palette to start a chain.</p>{/each}</div>{/if}
  </div>
  {#if selected?.processing&&transformSettings}<div class="lane-settings" data-transform-settings={selected.id}><div><h4>{selected.name}</h4><p>Edits apply wherever this transform is used.</p></div><fieldset disabled={readOnly}>{@render transformSettings(selected)}</fieldset></div>{/if}
</section>

<style>
  .signal-lanes{background:var(--surface)}
  .signal-lanes header{position:sticky;top:0;z-index:2;background:var(--surface);display:grid;grid-template-columns:minmax(0,1fr) 40px minmax(0,1fr);align-items:center;gap:16px;padding:10px 16px;border-bottom:1px solid var(--line)}
  .connection-direction{display:grid;place-items:center;color:var(--accent)}.destination-device{text-align:right}h3,h4{margin:3px 0;font-size:15px}
  .lane-toolbar{padding:8px 16px;display:flex;align-items:center;gap:10px}.lane-toolbar input{background:var(--input-bg);border:1px solid var(--line);border-radius:5px;color:var(--text);padding:6px 8px;width:240px;max-width:50%}.lane-toolbar>span{color:var(--muted);font-size:12px;margin-left:auto}
  .lane-scroll{max-height:340px;overflow:auto}.lane-head,.signal-lane{display:grid;grid-template-columns:minmax(110px,.8fr) minmax(280px,2.5fr) minmax(130px,.9fr);gap:16px;padding:5px 16px;box-sizing:border-box}
  .lane-head{align-items:center;font-size:11px;text-transform:uppercase;letter-spacing:.12em;color:var(--muted);border-bottom:1px solid var(--line)}.chain-heading{position:relative;display:flex;align-items:center;justify-content:space-between;gap:8px;min-width:0}
  .signal-lane{align-items:center;min-height:38px;border-bottom:1px solid var(--line)}.lane-endpoint{display:flex;flex-direction:column;gap:3px;min-width:0}.lane-endpoint strong{font-size:12px;overflow-wrap:anywhere}
  .lane-chain,.transform-library{display:flex;gap:6px;align-items:center;flex-wrap:wrap;min-width:0}
  .transform-chip{box-sizing:border-box;background:color-mix(in srgb,#9b80d8 14%,var(--surface));border:1px solid color-mix(in srgb,#9b80d8 40%,var(--line));color:var(--text);border-radius:5px;padding:6px 8px;display:flex;flex-direction:column;gap:2px;max-width:220px;min-width:0;text-align:left;cursor:pointer}
  .transform-chip strong{font-size:12px;line-height:14px}.transform-chip small{font-size:10px;line-height:12px;color:var(--soft);overflow-wrap:anywhere}.transform-chip em{font-size:10px;color:var(--muted)}
  .lane-chain .transform-chip{flex:0 0 144px;width:144px;max-width:100%;height:38px;padding:4px 7px}.lane-chain .transform-chip strong,.lane-chain .transform-chip small{display:block;width:100%;overflow:hidden;text-overflow:ellipsis;white-space:nowrap}.lane-chain .transform-chip em{display:none}
  .transform-chip.active,.transform-chip:hover{border-color:#bfa1ff;background:color-mix(in srgb,#9b80d8 24%,var(--surface))}
  .insert-transform{display:grid;place-items:center;flex:0 0 24px;width:24px;height:24px;padding:0;background:transparent;border:1px solid var(--line);color:var(--accent);border-radius:4px;cursor:pointer}.insert-transform:disabled{opacity:.4;cursor:default}
  .insert-targets{position:absolute;top:calc(100% + 5px);right:0;z-index:3;display:grid;max-height:200px;min-width:160px;overflow:auto;padding:4px;border:1px solid var(--line);border-radius:5px;background:var(--surface-raised);box-shadow:0 4px 16px #0004;letter-spacing:normal;text-transform:none}.insert-targets button{text-align:left;font:inherit;color:var(--text);background:transparent;border:0;padding:7px 9px;cursor:pointer}.insert-targets button:hover,.insert-targets button:focus-visible{background:var(--surface);outline:1px solid var(--accent)}
  .direct{color:var(--muted);font-size:12px;font-style:italic}.lane-endpoint:has(.lane-actions){display:grid;grid-template-columns:minmax(0,1fr) 26px;align-items:center}.lane-actions{display:flex}
  .disconnect-route{display:grid;place-items:center;width:26px;height:26px;padding:0;border:1px solid #bc3441;border-radius:4px;background:#9b2430;color:#fff;cursor:pointer}.disconnect-route:hover:enabled{background:#b32b38;border-color:#e35d69}.disconnect-route:focus-visible{outline:2px solid #ff8993;outline-offset:2px}.disconnect-route:disabled{opacity:.4;cursor:default}
  .transform-library{padding:12px 16px}.lane-settings{padding:14px 16px;display:grid;grid-template-columns:minmax(160px,1fr) minmax(260px,3fr);gap:20px;background:var(--surface-raised);border-top:1px solid var(--line)}.lane-settings p{font-size:12px;color:var(--muted)}fieldset{border:0;padding:0;min-width:0;margin:0}.empty{padding:16px;color:var(--muted)}
  @media(max-width:850px){.lane-head,.signal-lane{grid-template-columns:110px minmax(260px,1fr) 130px;min-width:580px}.lane-toolbar>span{display:none}.lane-settings{grid-template-columns:1fr}}
</style>
