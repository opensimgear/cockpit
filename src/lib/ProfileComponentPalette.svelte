<script lang="ts">
  import Icon from './Icon.svelte';
  import type {DeepReadonly} from './profiles';
  import type {ResourceSummary, ResourceType} from './profile-inventory';
  import {profileCategory} from './profile-editor';
  import {PROFILE_PALETTE_MIME,type ProfilePaletteDrag} from './profile-palette';

  let {inventory, assignedIds, resourceCount, virtualCount, desktopAdded, disabled=false, refreshing=false, error='', refresh, addResource, addVirtual, addDesktop, startDrag=()=>{}, endDrag=()=>{}}: {
    inventory:readonly DeepReadonly<ResourceSummary>[]; assignedIds:ReadonlySet<string>; resourceCount:number; virtualCount:number; desktopAdded:boolean;
    disabled?:boolean; refreshing?:boolean; error?:string; refresh:()=>void;
    addResource:(resource:DeepReadonly<ResourceSummary>)=>void; addVirtual:()=>void; addDesktop:()=>void;
    startDrag?:(drag:ProfilePaletteDrag)=>void;endDrag?:()=>void;
  }=$props();
  let search=$state('');
  let dragImage:HTMLSpanElement;
  function beginDrag(event:DragEvent,drag:ProfilePaletteDrag){
    if(disabled||(event.currentTarget as HTMLButtonElement).disabled||!event.dataTransfer){event.preventDefault();return;}
    event.dataTransfer.effectAllowed='copy';event.dataTransfer.setData(PROFILE_PALETTE_MIME,JSON.stringify(drag.component));
    if(dragImage)event.dataTransfer.setDragImage(dragImage,0,0);
    startDrag(drag);
  }
  const groups:readonly {type:ResourceType;label:string;icon:string}[]=[
    {type:'host-controller',label:'Controllers',icon:'gamepad'},
    {type:'osg-module',label:'OSG devices',icon:'chip'},
    {type:'companion',label:'Companions',icon:'monitor'},
    {type:'game-integration',label:'Games',icon:'gamepad'},
  ];
  const query=$derived(search.trim().toLocaleLowerCase());
  const companionsInProfile=$derived(new Set(inventory.filter(resource=>assignedIds.has(resource.id)).map(resource=>resource.attributes.companionId).filter(Boolean)));
  const components=$derived(inventory.filter(resource=>resource.resourceType!=='companion'||resource.attributes.componentKind==='paired-companion')
    .filter(resource=>!query||`${resource.name} ${groups.find(group=>group.type===resource.resourceType)?.label}`.toLocaleLowerCase().includes(query))
    .slice().sort((a,b)=>a.name.localeCompare(b.name)));
  const showVirtual=$derived(!query||'virtual controller outputs'.includes(query));
  const showDesktop=$derived(!query||'keyboard mouse desktop outputs'.includes(query));
  function added(resource:DeepReadonly<ResourceSummary>){return assignedIds.has(resource.id)||!!resource.attributes.companionId&&companionsInProfile.has(resource.attributes.companionId);}
</script>

<aside class="component-palette" aria-label="Component palette">
  <span class="drag-image" bind:this={dragImage} aria-hidden="true"></span>
  <header><h2>Components</h2><button class="refresh" aria-label="Refresh components" title="Refresh components" disabled={refreshing} onclick={refresh}><Icon name="refresh" size={15}/></button></header>
  <label class="palette-search"><Icon name="search" size={14}/><input aria-label="Search components" placeholder="Find a component…" bind:value={search}/></label>
  {#if error}<p class="palette-error" role="status">{error}</p>{/if}
  <div class="palette-items">
    {#if showVirtual||showDesktop}<section aria-label="Output components"><h3>Outputs</h3>
      {#if showVirtual}<button class="component" aria-label="Add virtual controller" disabled={disabled||virtualCount>=8} draggable={!disabled&&virtualCount<8} ondragstart={event=>beginDrag(event,{component:{kind:'virtual'},name:'Virtual controller',category:'Virtual controller'})} ondragend={endDrag} onclick={addVirtual}><span class="component-icon"><Icon name="gamepad" size={17}/></span><span class="component-copy"><strong>Virtual controller</strong><small>Buttons, axes and hats</small></span><Icon name="plus" size={14}/></button>{/if}
      {#if showDesktop}<button class="component" aria-label="Add keyboard and mouse" disabled={disabled||desktopAdded} draggable={!disabled&&!desktopAdded} ondragstart={event=>beginDrag(event,{component:{kind:'desktop'},name:'Keyboard and mouse',category:'Keyboard and mouse'})} ondragend={endDrag} onclick={addDesktop}><span class="component-icon"><Icon name="devices" size={17}/></span><span class="component-copy"><strong>Keyboard and mouse</strong><small>{desktopAdded?'Added to profile':'Keys, pointer and buttons'}</small></span><Icon name={desktopAdded?'check':'plus'} size={14}/></button>{/if}
    </section>{/if}
    {#each groups as group (group.type)}
      {@const items=components.filter(resource=>resource.resourceType===group.type)}
      {#if items.length}<section aria-label={group.label}><h3>{group.label}<span>{items.length}</span></h3>
        {#each items as resource (resource.id)}
          {@const inProfile=added(resource)}
          <button class="component" data-resource-id={resource.id} title={resource.name} aria-label={`Add ${resource.name}`} disabled={disabled||inProfile||resourceCount>=64} draggable={!disabled&&!inProfile&&resourceCount<64} ondragstart={event=>beginDrag(event,{component:{kind:'resource',sourceId:resource.id},name:resource.name,category:profileCategory(resource)})} ondragend={endDrag} onclick={()=>addResource(resource)}>
            <span class="component-icon"><Icon name={group.icon} size={17}/></span>
            <span class="component-copy"><strong>{resource.name}</strong><small><span class="presence" class:online={resource.availability==='online'}></span>{inProfile?'Added to profile':resource.availability==='online'?'Online':resource.availability==='offline'?'Offline':'Unavailable'}</small></span>
            <Icon name={inProfile?'check':'plus'} size={14}/>
          </button>
        {/each}
      </section>{/if}
    {/each}
    {#if !components.length&&!showVirtual&&!showDesktop}<p class="palette-empty">No matching components.</p>{/if}
    {#if !inventory.length&&!refreshing&&!query}<p class="palette-empty">No devices or companions found.</p>{/if}
  </div>
  <footer>{resourceCount} / 64 resources <span>Drag a component onto the canvas or click to add. Click a connection to add a transformation.</span></footer>
</aside>

<style>
  .drag-image{position:fixed;top:0;left:0;width:1px;height:1px;opacity:0;pointer-events:none}.component[draggable="true"]{cursor:grab}.component[draggable="true"]:active{cursor:grabbing}
  .component-palette{position:sticky;top:12px;display:flex;flex-direction:column;min-width:0;max-height:calc(100dvh - 130px);border:1px solid var(--line);border-radius:8px;background:var(--surface);overflow:hidden}
  header{display:flex;align-items:center;justify-content:space-between;gap:12px;padding:15px 14px 11px}h2{font-size:13px}button{font:inherit;color:var(--text)}.refresh{display:grid;place-items:center;padding:5px;background:transparent;color:var(--muted);border-radius:4px}.refresh:hover:enabled{color:var(--accent);background:var(--surface-raised)}
  .palette-search{display:flex;align-items:center;gap:8px;margin:0 12px 12px;padding:9px;border:1px solid var(--line);border-radius:5px;background:var(--input-bg);color:var(--muted)}.palette-search input{width:100%;min-width:0;font:inherit;font-size:11px;border:0;background:transparent;color:var(--text);outline:none}.palette-search:focus-within{outline:2px solid var(--accent);outline-offset:2px}
  .palette-items{min-height:0;overflow:auto;padding:0 8px 12px}section+section{margin-top:16px}h3{display:flex;align-items:center;justify-content:space-between;padding:0 6px;margin:5px 0 8px;font-size:10px;color:var(--muted);font-weight:600}h3 span{font-size:9px;font-weight:400}
  .component{display:flex;align-items:center;gap:9px;width:100%;min-width:0;padding:10px 6px;border:1px solid transparent;border-radius:6px;background:transparent;text-align:left}.component:hover:enabled{border-color:var(--line);background:var(--surface-raised)}.component:focus-visible{outline:2px solid var(--accent);outline-offset:-2px}.component:disabled{opacity:.5}.component-icon{display:grid;place-items:center;flex-shrink:0;width:28px;height:30px;color:var(--accent)}.component-copy{display:grid;gap:5px;min-width:0;flex:1}.component-copy strong{overflow:hidden;text-overflow:ellipsis;white-space:nowrap;font-size:11px;font-weight:600}.component-copy small{display:flex;align-items:center;gap:5px;font-size:9px;color:var(--muted)}.component :global(>svg){flex-shrink:0;color:var(--muted)}.presence{width:5px;height:5px;flex-shrink:0;border-radius:50%;background:var(--muted)}.presence.online{background:var(--accent)}
  footer{display:grid;gap:7px;padding:12px 14px;border-top:1px solid var(--line);font-size:9px;color:var(--muted);line-height:1.6}footer span{font-size:10px}.palette-error,.palette-empty{padding:0 14px;font-size:10px;line-height:1.7;color:var(--muted)}.palette-error{color:#d3a078}
  @media(max-width:1050px){.component-palette{position:static;max-height:360px}.palette-items{display:grid;grid-template-columns:repeat(auto-fit,minmax(200px,1fr));align-items:start;gap:16px}section+section{margin-top:0}}
</style>
