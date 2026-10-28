<script lang="ts">
  import Icon from './Icon.svelte';
  import type {DeepReadonly, ProfileEditorGroup} from './profiles';
  import type {ResourceSummary, ResourceType} from './profile-inventory';
  import {profileCategory} from './profile-editor';
  import {PROFILE_PALETTE_MIME,profilePalettePanelReason,type ProfilePaletteDrag} from './profile-palette';
  import {PROCESSING_PRESETS,type ProcessingPreset} from './profile-processing';
  import {latestCompanionPanels,type ProfileCompanionPanel} from './profile-companion-authoring';

  let {inventory, assignedIds, resourceCount, virtualCount, desktopAdded, processingCount=0, capturedCompanionIds=new Set<string>(), companionPanelCount=0, companionPanels=[], companionPanelsLoaded=false, companionPanelsBusy=false, companionPanelsError='', refreshPanels=()=>{}, addCompanionPanel=()=>{}, addTransformation=()=>{}, editorGroups=[], canAddGroup=true, disabled=false, refreshing=false, error='', refresh, addResource, addVirtual, addDebug=()=>{}, addDesktop, addGroup=()=>{}, focusGroup=()=>{}, renameGroup, deleteGroup=()=>{}, startDrag=()=>{}, endDrag=()=>{}}: {
    inventory:readonly DeepReadonly<ResourceSummary>[]; assignedIds:ReadonlySet<string>; resourceCount:number; virtualCount:number; desktopAdded:boolean;
    editorGroups?:readonly DeepReadonly<ProfileEditorGroup>[];canAddGroup?:boolean;
    disabled?:boolean; refreshing?:boolean; error?:string; refresh:()=>void;
    addResource:(resource:DeepReadonly<ResourceSummary>)=>void; addVirtual:()=>void; addDebug?:()=>void; addDesktop:()=>void;
    processingCount?:number;addTransformation?:(preset:ProcessingPreset)=>void;
    capturedCompanionIds?:ReadonlySet<string>;companionPanelCount?:number;companionPanels?:readonly DeepReadonly<ProfileCompanionPanel>[];companionPanelsLoaded?:boolean;companionPanelsBusy?:boolean;companionPanelsError?:string;refreshPanels?:()=>void;addCompanionPanel?:(sourceId:string,publicationHash:string)=>void;
    addGroup?:()=>void;focusGroup?:(id:string)=>void;renameGroup?:(id:string,name:string)=>void;deleteGroup?:(id:string)=>void;
    startDrag?:(drag:ProfilePaletteDrag)=>void;endDrag?:()=>void;
  }=$props();
  let search=$state('');
  let searchInput:HTMLInputElement;
  let expanded=$state<Record<string,boolean>>({});
  let editingGroup=$state<string|null>(null), groupName=$state('');
  let dragImage:HTMLSpanElement;
  function beginDrag(event:DragEvent,drag:ProfilePaletteDrag){
    if(disabled||(event.currentTarget as HTMLButtonElement).disabled||!event.dataTransfer){event.preventDefault();return;}
    event.dataTransfer.effectAllowed='copy';event.dataTransfer.setData(PROFILE_PALETTE_MIME,JSON.stringify(drag.component));
    if(dragImage)event.dataTransfer.setDragImage(dragImage,0,0);
    startDrag(drag);
  }
  const categories:readonly {type:ResourceType;label:string;icon:string}[]=[
    {type:'host-controller',label:'Controllers',icon:'gamepad'},
    {type:'osg-module',label:'OSG devices',icon:'chip'},
    {type:'companion',label:'Companions',icon:'monitor'},
  ];
  const query=$derived(search.trim().toLocaleLowerCase());
  const companionsInProfile=$derived(new Set(inventory.filter(resource=>assignedIds.has(resource.id)).map(resource=>resource.attributes.companionId).filter(Boolean)));
  const components=$derived(inventory.filter(resource=>resource.resourceType!=='game-integration').filter(resource=>resource.resourceType!=='companion'||resource.attributes.componentKind==='paired-companion')
    .filter(resource=>!query||`${resource.name} ${categories.find(category=>category.type===resource.resourceType)?.label}`.toLocaleLowerCase().includes(query))
    .slice().sort((a,b)=>a.name.localeCompare(b.name)));
  const visibleGroups=$derived(editorGroups.filter(group=>!query||`${group.name} groups`.toLocaleLowerCase().includes(query)));
  const showVirtual=$derived(!query||'virtual controller outputs'.includes(query));
  const showDebug=$derived(!query||'debug controller axes buttons hats input'.includes(query));
  const showDesktop=$derived(!query||'keyboard mouse desktop outputs'.includes(query));
  const transformations=$derived([
    {preset:'identity' as ProcessingPreset,label:'Transformation',description:'Choose a type after dropping'},
    ...PROCESSING_PRESETS.filter(([preset])=>preset!=='identity').map(([preset,label])=>({preset,label:preset==='scale'?'Multiply':preset==='range-map'?'Map range':preset==='format'?'Format text':label,description:preset==='scale'?'Multiply and offset a numeric signal':'Configure on the canvas'})),
  ].filter(item=>!query||`${item.label} ${item.description} transformation`.toLocaleLowerCase().includes(query)));
  const pairedCompanions=$derived(inventory.filter(resource=>resource.resourceType==='companion'&&resource.attributes.componentKind==='paired-companion'));
  const panelComponents=$derived(pairedCompanions.flatMap(source=>latestCompanionPanels(companionPanels).filter(panel=>!profilePalettePanelReason(panel,source)).map(panel=>({source,panel}))).filter(({source,panel})=>!query||`${source.name} ${panel.title} companion panels`.toLocaleLowerCase().includes(query)));
  function isOpen(category:string){return !!query||!!expanded[category];}
  function rememberOpen(event:Event,category:string){const open=(event.currentTarget as HTMLDetailsElement).open;if(!query)expanded={...expanded,[category]:open};if(category==='panels'&&open&&!companionPanelsLoaded&&!companionPanelsBusy&&!companionPanelsError)refreshPanels();}
  function added(resource:DeepReadonly<ResourceSummary>){return assignedIds.has(resource.id)||!!resource.attributes.companionId&&companionsInProfile.has(resource.attributes.companionId);}
  function editGroup(group:DeepReadonly<ProfileEditorGroup>){editingGroup=group.id;groupName=group.name;}
  function saveGroupName(){if(editingGroup&&groupName.trim()&&renameGroup)renameGroup(editingGroup,groupName.trim());editingGroup=null;}
  function groupNameKey(event:KeyboardEvent){if(event.key==='Enter'){event.preventDefault();saveGroupName();}else if(event.key==='Escape'){event.preventDefault();editingGroup=null;}}
  function focusGroupName(input:HTMLInputElement){input.focus();input.select();}
  function clearSearch(){search='';searchInput?.focus();}
</script>

<aside class="component-palette" aria-label="Component palette">
  <span class="drag-image" bind:this={dragImage} aria-hidden="true"></span>
  <header><h2>Components</h2><button class="refresh" aria-label="Refresh components" title="Refresh components" disabled={refreshing} onclick={refresh}><Icon name="refresh" size={15}/></button></header>
  <div class="palette-search"><Icon name="search" size={14}/><input aria-label="Search components" placeholder="Find a component…" bind:value={search} bind:this={searchInput}/>{#if search}<button type="button" class="clear-search" aria-label="Clear component search" title="Clear component search" onclick={clearSearch}><Icon name="close" size={13}/></button>{/if}</div>
  {#if error}<p class="palette-error" role="status">{error}</p>{/if}
  <div class="palette-items">
    {#if !query||visibleGroups.length}
      <details class="editor-accordion" aria-label="Profile groups" open={isOpen('groups')} ontoggle={event=>rememberOpen(event,'groups')}>
        <summary>Groups <span class="category-count">{editorGroups.length}</span></summary>
        <div class="accordion-content group-list">
          {#each visibleGroups as group (group.id)}
            <div class="group-row">
              {#if editingGroup===group.id}
                <input class="group-name" aria-label="Group name" maxlength={80} bind:value={groupName} use:focusGroupName onkeydown={groupNameKey} onblur={saveGroupName}/>
              {:else}
                <button class="group-focus" title={group.name} aria-label={`Focus ${group.name}`} onclick={()=>focusGroup(group.id)}><Icon name="group" size={16}/><span>{group.name}</span><small>{group.members.length}</small></button>
                {#if renameGroup}<button class="group-action" title="Rename group" aria-label={`Rename ${group.name}`} disabled={disabled} onclick={()=>editGroup(group)}><Icon name="pencil" size={13}/></button>{/if}
              {/if}
              <button class="group-action" title="Remove group; keep its components" aria-label={`Remove ${group.name} group`} disabled={disabled} onclick={()=>deleteGroup(group.id)}><Icon name="trash" size={14}/></button>
            </div>
          {/each}
          {#if !editorGroups.length}<p class="group-hint">Drag components into a group on the canvas to organize them.</p>{/if}
          {#if canAddGroup}<button class="add-group" disabled={disabled||editorGroups.length>=32} onclick={addGroup}><Icon name="plus" size={14}/>Add group</button>{/if}
        </div>
      </details>
    {/if}
    {#if transformations.length}<details class="editor-accordion" aria-label="Transformation components" open={isOpen('transformations')} ontoggle={event=>rememberOpen(event,'transformations')}><summary>Transformations<span class="category-count">{transformations.length}</span></summary><div class="accordion-content">
      {#each transformations as item (item.preset)}
        <button class="component" data-processing-preset={item.preset} aria-label={`Add ${item.label}`} disabled={disabled||processingCount>=256} draggable={!disabled&&processingCount<256} ondragstart={event=>beginDrag(event,{component:{kind:'transformation',preset:item.preset},name:item.label,category:'Transformation'})} ondragend={endDrag} onclick={()=>addTransformation(item.preset)}><span class="component-icon"><Icon name="sliders" size={17}/></span><span class="component-copy"><strong>{item.label}</strong><small>{item.description}</small></span><Icon name="plus" size={14}/></button>
      {/each}
    </div></details>{/if}
    {#if pairedCompanions.length&&(!query||panelComponents.length||'companion panels'.includes(query))}<details class="editor-accordion" aria-label="Companion panel components" open={isOpen('panels')} ontoggle={event=>rememberOpen(event,'panels')}><summary>Companion panels<span class="category-count">{panelComponents.length}</span></summary><div class="accordion-content">
      <button class="refresh-panels" disabled={companionPanelsBusy} onclick={refreshPanels}><Icon name="refresh" size={13}/>{companionPanelsBusy?'Reading saved panels…':companionPanelsError?'Retry saved panels':'Refresh saved panels'}</button>
      {#if companionPanelsError}<p class="group-hint palette-error" role="status">{companionPanelsError}</p>{/if}
      {#each panelComponents as {source,panel} (`${source.id}/${panel.publicationHash}`)}
        {@const inProfile=added(source)||capturedCompanionIds.has(source.id)}
        <button class="component" data-panel-source-id={source.id} data-panel-publication={panel.publicationHash} aria-label={`Add ${panel.title} to ${source.name}`} title={`${panel.title} · ${source.name}`} disabled={disabled||inProfile||resourceCount>=64||companionPanelCount>=8} draggable={!disabled&&!inProfile&&resourceCount<64&&companionPanelCount<8} ondragstart={event=>beginDrag(event,{component:{kind:'companion-panel',sourceId:source.id,publicationHash:panel.publicationHash},name:panel.title,category:`Companion · ${source.name}`})} ondragend={endDrag} onclick={()=>addCompanionPanel(source.id,panel.publicationHash)}><span class="component-icon"><Icon name="monitor" size={17}/></span><span class="component-copy"><strong>{panel.title}</strong><small>{inProfile?'Companion already added':source.name} · {panel.controlCount} controls</small></span><Icon name={inProfile?'check':'plus'} size={14}/></button>
      {/each}
      {#if companionPanelsLoaded&&!panelComponents.length&&!companionPanelsBusy}<p class="group-hint">No compatible saved panels. Save a panel in the panel designer, then refresh.</p>{/if}
    </div></details>{/if}
      {#if showVirtual||showDesktop}<details class="editor-accordion" aria-label="Output components" open={isOpen('outputs')} ontoggle={event=>rememberOpen(event,'outputs')}><summary>Outputs<span class="category-count">{Number(showVirtual)+Number(showDesktop)}</span></summary><div class="accordion-content">
        {#if showVirtual}<button class="component" aria-label="Add virtual controller" disabled={disabled||virtualCount>=8} draggable={!disabled&&virtualCount<8} ondragstart={event=>beginDrag(event,{component:{kind:'virtual'},name:'Virtual controller',category:'Virtual controller'})} ondragend={endDrag} onclick={addVirtual}><span class="component-icon"><Icon name="gamepad" size={17}/></span><span class="component-copy"><strong>Virtual controller</strong><small>Buttons, axes and hats</small></span><Icon name="plus" size={14}/></button>{/if}
        {#if showDesktop}<button class="component" aria-label="Add keyboard and mouse" disabled={disabled||desktopAdded} draggable={!disabled&&!desktopAdded} ondragstart={event=>beginDrag(event,{component:{kind:'desktop'},name:'Keyboard and mouse',category:'Keyboard and mouse'})} ondragend={endDrag} onclick={addDesktop}><span class="component-icon"><Icon name="devices" size={17}/></span><span class="component-copy"><strong>Keyboard and mouse</strong><small>{desktopAdded?'Added to profile':'Keys, pointer and buttons'}</small></span><Icon name={desktopAdded?'check':'plus'} size={14}/></button>{/if}
      </div></details>{/if}
    {#each categories as category (category.type)}
      {@const items=components.filter(resource=>resource.resourceType===category.type)}
      {#if items.length||category.type==='host-controller'&&showDebug}<details class="editor-accordion" aria-label={category.label} open={isOpen(category.type)} ontoggle={event=>rememberOpen(event,category.type)}><summary>{category.label}<span class="category-count">{items.length+(category.type==='host-controller'&&showDebug?1:0)}</span></summary><div class="accordion-content">
        {#if category.type==='host-controller'&&showDebug}<button class="component" aria-label="Add debug controller" disabled={disabled||resourceCount>=64} draggable={!disabled&&resourceCount<64} ondragstart={event=>beginDrag(event,{component:{kind:'debug-controller'},name:'Debug controller',category:'Debug controller'})} ondragend={endDrag} onclick={addDebug}><span class="component-icon"><Icon name="bug" size={17}/></span><span class="component-copy"><strong>Debug controller</strong><small>Interactive axes, buttons and hats</small></span><Icon name="plus" size={14}/></button>{/if}
        {#each items as resource (resource.id)}
          {@const inProfile=added(resource)}
          <button class="component" data-resource-id={resource.id} title={resource.name} aria-label={`Add ${resource.name}`} disabled={disabled||inProfile||resourceCount>=64} draggable={!disabled&&!inProfile&&resourceCount<64} ondragstart={event=>beginDrag(event,{component:{kind:'resource',sourceId:resource.id},name:resource.name,category:profileCategory(resource)})} ondragend={endDrag} onclick={()=>addResource(resource)}>
            <span class="component-icon"><Icon name={category.icon} size={17}/></span>
            <span class="component-copy"><strong>{resource.name}</strong><small><span class="presence" class:online={resource.availability==='online'}></span>{inProfile?'Added to profile':resource.availability==='online'?'Online':resource.availability==='offline'?'Offline':'Unavailable'}</small></span>
            <Icon name={inProfile?'check':'plus'} size={14}/>
          </button>
        {/each}
      </div></details>{/if}

    {/each}
    {#if !components.length&&!showVirtual&&!showDebug&&!showDesktop&&!visibleGroups.length&&!transformations.length&&!panelComponents.length}<p class="palette-empty">No matching components.</p>{/if}
    {#if !inventory.length&&!refreshing&&!query&&!showDebug}<p class="palette-empty">No devices or companions found.</p>{/if}
  </div>
  <footer>{resourceCount} / 64 resources <span>Drag components onto the canvas. Drop a transformation onto a connection to insert it.</span></footer>
</aside>

<style>
  .drag-image{position:fixed;top:0;left:0;width:1px;height:1px;opacity:0;pointer-events:none}.component[draggable="true"]{cursor:grab}.component[draggable="true"]:active{cursor:grabbing}
  .refresh-panels{display:flex;align-items:center;gap:6px;justify-content:flex-start;padding:7px 6px;border:0;background:transparent;color:var(--muted);font-size:var(--ui-action-size)}.refresh-panels:hover:enabled{color:var(--accent)}.refresh-panels:disabled{opacity:.5}
  .component-palette{position:sticky;top:12px;display:flex;flex-direction:column;min-width:0;max-height:calc(100dvh - 130px);border:1px solid var(--line);border-radius:8px;background:var(--surface);overflow:hidden}
  header{display:flex;align-items:center;justify-content:space-between;gap:12px;padding:15px 14px 11px}h2{font-size:var(--ui-heading-size)}button{font:inherit;color:var(--text)}.refresh{display:grid;place-items:center;padding:5px;background:transparent;color:var(--muted);border-radius:4px}.refresh:hover:enabled{color:var(--accent);background:var(--surface-raised)}
  .palette-search{display:flex;align-items:center;gap:8px;margin:0 12px 12px;padding:9px;border:1px solid var(--line);border-radius:5px;background:var(--input-bg);color:var(--muted)}.palette-search input{width:100%;min-width:0;flex:1;font:inherit;font-size:var(--ui-text-size);border:0;background:transparent;color:var(--text);outline:none}.palette-search:focus-within{outline:2px solid var(--accent);outline-offset:2px}.clear-search{display:grid;place-items:center;flex-shrink:0;width:20px;height:20px;margin:-3px 0;padding:0;border:0;border-radius:3px;background:transparent;color:var(--muted)}.clear-search:hover{background:var(--surface-raised);color:var(--text)}.clear-search:focus-visible{outline:2px solid var(--accent);outline-offset:1px}
  .palette-items{min-height:0;overflow:auto}.editor-accordion{margin:0;border-top:1px solid var(--line);interpolate-size:allow-keywords}.editor-accordion>summary{position:relative;padding:14px;font-size:var(--ui-heading-size);font-weight:600;cursor:pointer;color:var(--text);padding-right:36px}.category-count{position:absolute;right:14px;top:14px;font-size:10px;font-weight:400;color:var(--muted)}.accordion-content{padding:0 8px 10px;display:grid;gap:3px;min-width:0}.editor-accordion::details-content{height:0;overflow:clip;transition:height 180ms ease,content-visibility 180ms allow-discrete}.editor-accordion[open]::details-content{height:auto}
  .component{display:flex;align-items:center;gap:9px;width:100%;min-width:0;padding:10px 6px;border:1px solid transparent;border-radius:6px;background:transparent;text-align:left}.component:hover:enabled{border-color:var(--line);background:var(--surface-raised)}.component:focus-visible{outline:2px solid var(--accent);outline-offset:-2px}.component:disabled{opacity:.5}.component-icon{display:grid;place-items:center;flex-shrink:0;width:28px;height:30px;color:var(--accent)}.component-copy{display:grid;gap:5px;min-width:0;flex:1}.component-copy strong{overflow:hidden;text-overflow:ellipsis;white-space:nowrap;font-size:var(--ui-heading-size);font-weight:600}.component-copy small{display:flex;align-items:center;gap:5px;font-size:var(--ui-text-size);color:var(--muted)}.component :global(>svg){flex-shrink:0;color:var(--muted)}.presence{width:5px;height:5px;flex-shrink:0;border-radius:50%;background:var(--muted)}.presence.online{background:var(--accent)}
  footer{display:grid;gap:7px;padding:12px 14px;border-top:1px solid var(--line);font-size:var(--ui-text-size);color:var(--muted);line-height:var(--ui-line-height)}footer span{font-size:var(--ui-text-size)}.palette-error,.palette-empty{padding:0 14px;font-size:var(--ui-text-size);line-height:var(--ui-line-height);color:var(--muted)}.palette-error{color:#d3a078}
  .group-list{gap:6px;padding:0 12px 12px}.group-row{display:flex;align-items:center;gap:3px;min-width:0}.group-focus{display:flex;align-items:center;gap:8px;min-width:0;flex:1;padding:8px 3px;border:0;background:transparent;text-align:left;font-size:var(--ui-action-size)}.group-focus>span{min-width:0;flex:1;overflow:hidden;text-overflow:ellipsis;white-space:nowrap}.group-focus>small{color:var(--muted);font-size:var(--ui-text-size)}.group-focus:hover{color:var(--accent)}.group-action{display:grid;place-items:center;flex-shrink:0;width:24px;height:28px;padding:0;background:transparent;color:var(--muted);border:0;border-radius:4px}.group-action:hover:enabled{background:var(--surface-raised);color:var(--text)}.group-action:disabled{opacity:.4}.group-name{min-width:0;flex:1;width:0;padding:6px;font:inherit;font-size:var(--ui-text-size);background:var(--input-bg);color:var(--text);border:1px solid var(--line);border-radius:4px}.group-hint{margin:0 0 3px;font-size:var(--ui-text-size);line-height:var(--ui-line-height);color:var(--muted)}.add-group{display:flex;align-items:center;justify-content:center;gap:7px;padding:9px;border:1px solid var(--line);border-radius:5px;background:transparent;font-size:var(--ui-action-size)}.add-group:hover:enabled{background:var(--surface-raised);border-color:var(--accent)}.add-group:disabled{opacity:.4}
  @media(prefers-reduced-motion:reduce){.editor-accordion::details-content{transition:none}}
  @media(max-width:1050px){.component-palette{position:static;max-height:360px}}
</style>
