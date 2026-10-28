<script lang="ts">
  import Icon from '../Icon.svelte';
  import {graphPublicationCapability} from '../profile-graph';
  import {portContractLabel,type CanvasNode} from '../profile-canvas';
  import type {DeepReadonly,ProfileDocument} from '../profiles';
  import type {ObservedResourcePort,ResourceDetails,ResourceSummary} from '../profile-inventory';
  import {gameIntegrationSourceStatus} from '../profile-game-source';
  import type {Game} from '../games';

  let {node,document,controller=false,source='',resources=[],games=[],page=null,mappings={},options=[],busy=false,error='',hasPrevious=false,hasNext=false,
    policy,visibility,publication,chooseSource,chooseMapping,browse,assign}: {
    node:CanvasNode;document:DeepReadonly<ProfileDocument>;controller?:boolean;source?:string;
    resources?:readonly DeepReadonly<ResourceSummary>[];games?:readonly Game[];page?:DeepReadonly<ResourceDetails>|null;
    mappings?:Record<string,DeepReadonly<ObservedResourcePort>>;options?:readonly DeepReadonly<ObservedResourcePort>[];
    busy?:boolean;error?:string;hasPrevious?:boolean;hasNext?:boolean;
    policy:(key:'name'|'required',value:string|boolean)=>void;visibility:(value:string)=>void;publication:(value:string)=>void;
    chooseSource:(value:string)=>void;chooseMapping:(portId:string,key:string)=>void;
    browse:(operation:'previous'|'next')=>void;assign:()=>void;
  }=$props();
  const resource=$derived(node.resource);
  const capability=$derived(graphPublicationCapability(node.observation));
  const publicationMode=$derived(document.overrides.find(item=>item.resourceId===node.id)?.enabled);
  const selectedSource=$derived(resources.some(item=>item.id===source)?source:resources.some(item=>item.id===node.assignment?.sourceId)?node.assignment!.sourceId:'');
  const unavailableAssignment=$derived(!!node.assignment&&!resources.some(item=>item.id===node.assignment?.sourceId));
  function sourceStatus(item:DeepReadonly<ResourceSummary>){
    if(item.resourceType!=='game-integration')return item.availability;
    const game=games.find(game=>game.id===item.attributes.gameId&&(game.integrationKey??game.id)===item.attributes.integrationKey);
    return gameIntegrationSourceStatus(game,[item]);
  }
  function key(port:DeepReadonly<ObservedResourcePort>){return JSON.stringify([port.descriptor.id,port.contractHash]);}
</script>

{#if resource}
  <section class="resource-settings" aria-label="Resource settings">
    <label>Label<input maxlength="80" value={resource.name} onchange={event=>policy('name',event.currentTarget.value)}/></label>
    <label class="check"><input type="checkbox" checked={resource.required} onchange={event=>policy('required',event.currentTarget.checked)}/> Required</label>
    {#if resource.kind==='device'}
      <label>Visibility<select value={document.visibility.find(item=>item.resourceId===node.id)?.mode??'visible'} onchange={event=>visibility(event.currentTarget.value)}>
        <option value="visible">Visible</option><option value="hidden-while-active">Hidden while active</option>
      </select></label>
    {/if}
    {#if !controller&&resource.kind!=='companion'}
      <label>Source<select aria-label="Inventory resource" value={selectedSource} disabled={busy} onchange={event=>chooseSource(event.currentTarget.value)}>
        <option value="" disabled>Choose a source</option>
        {#each resources as item (item.id)}<option value={item.id}>{item.name} · {sourceStatus(item)}</option>{/each}
      </select></label>
      {#if unavailableAssignment}<p class="error" role="status">The saved source is unavailable. Choose a current source to reassign this resource.</p>{/if}
      {#if node.assignment&&!source}<button class="text-button" onclick={()=>chooseSource(node.assignment!.sourceId)}><Icon name="link" size={14}/>Map ports</button>{/if}
      {#if busy}<p role="status">Loading ports…</p>{/if}
      {#if error}<p class="error" role="alert">{error}</p>{/if}
      {#if source}
        <details open class="mapping"><summary>Port mapping <span>{resource.ports.length}</span></summary>
          {#each resource.ports as port (port.id)}
            <label>{port.name}{resource.ports.filter(item=>item.name===port.name).length>1?` · ${port.id}`:''}<select aria-label={`Map ${port.name} (${port.id})`} value={mappings[port.id]?key(mappings[port.id]):''} onchange={event=>chooseMapping(port.id,event.currentTarget.value)}>
              <option value="">Choose a matching port</option>
              {#each options.filter(item=>item.descriptor.direction===port.direction) as item (key(item))}<option value={key(item)}>{item.descriptor.name} · {item.descriptor.id}{item.current?'':' · cached'}</option>{/each}
            </select><small>{portContractLabel(port.contract)}</small></label>
          {/each}
          {#if page}<div class="paging"><span>{page.total?`${page.offset+1}–${page.offset+page.ports.length}`:'0'} / {page.total}</span><div>
            <button class="text-button" aria-label="Previous assignment ports" disabled={busy||!hasPrevious} onclick={()=>browse('previous')}><span class="previous"><Icon name="chevron" size={14}/></span></button>
            <button class="text-button" aria-label="Next assignment ports" disabled={busy||!hasNext} onclick={()=>browse('next')}><Icon name="chevron" size={14}/></button>
          </div></div>{/if}
          <button class="secondary-button" disabled={busy||resource.ports.some(port=>!mappings[port.id])} onclick={assign}><Icon name="check" size={14}/>Assign</button>
        </details>
      {/if}
    {/if}
    {#if !controller&&resource.kind==='device'}
      <details class="publication"><summary>Publication</summary>
        <label>Policy<select aria-label="Controller publication policy" value={publicationMode===undefined?'configured':publicationMode?'enabled':'disabled'} onchange={event=>publication(event.currentTarget.value)}>
          <option value="configured">Keep configured</option><option value="enabled" disabled={capability.status==='unsupported'}>Enable while active</option><option value="disabled" disabled={capability.status==='unsupported'}>Disable while active</option>
        </select></label>
        <p role="status">{capability.message}{#if capability.configuredEnabled!==null} Configured: {capability.configuredEnabled?'enabled':'disabled'}.{/if}</p>
        <p>Changes may require a USB reconnect and game rebinding. Device flash stays unchanged.</p>
      </details>
    {/if}
  </section>
{/if}

<style>
  .resource-settings{display:grid;gap:12px;min-width:0}label{display:grid;gap:7px;color:var(--muted);font-size:var(--ui-text-size)}input:not([type="checkbox"]),select{box-sizing:border-box;width:100%;min-width:0;min-height:var(--ui-control-height);padding:8px;border:1px solid var(--line);border-radius:5px;background:var(--input-bg);color:var(--text);font-size:var(--ui-text-size)}.check{display:flex;align-items:center;gap:8px}details{border-top:1px solid var(--line);padding-top:12px}summary{cursor:pointer;color:var(--soft);font-size:var(--ui-text-size)}summary span{float:right;color:var(--muted)}details label{margin:12px 0}p,small{margin:0;color:var(--muted);font-size:var(--ui-text-size);line-height:var(--ui-line-height)}.publication p{margin-top:8px}.error{color:var(--danger,#e67979)}button{font-size:var(--ui-action-size);justify-self:start}.paging{display:flex;align-items:center;justify-content:space-between;gap:8px;margin:12px 0;color:var(--muted);font-size:var(--ui-text-size)}.paging>div{display:flex;gap:5px}.previous{display:inline-flex;transform:rotate(180deg)}
</style>
