<script lang="ts">
  import {onDestroy,untrack} from 'svelte';
  import Icon from './Icon.svelte';
  import {gatewayRequest} from './gateway';
  import {portContractLabel} from './profile-canvas';
  import {ProfileCompanionAuthoringClient,ProfileCompanionPortBrowser,graphCaptureCompanionPanel,graphReconcileCompanionPanel,graphReleaseCompanionPanel,graphSelectCompanionPort,latestCompanionPanels,selectedCompanionPanel,type ProfileCompanionCatalogue,type ProfileCompanionPage,type ProfileCompanionPanel} from './profile-companion-authoring';
  import {graphAssignResource,type ProfileGraphDraft} from './profile-graph';
  import type {ObservedResourcePort,ResourceSummary} from './profile-inventory';
  import type {DeepReadonly} from './profiles';

  let {graph,resourceId,inventory=[],readOnly=false,active=true,client,change}:{
    graph:DeepReadonly<ProfileGraphDraft>;resourceId:string;readOnly?:boolean;active?:boolean;
    inventory?:readonly DeepReadonly<ResourceSummary>[];
    client?:Pick<ProfileCompanionAuthoringClient,'catalogue'|'inspect'>;
    change:(draft:ProfileGraphDraft,label:string)=>void;
  }=$props();
  const native=untrack(()=>client)??new ProfileCompanionAuthoringClient(async request=>(await gatewayRequest(request)).profileCompanionAuthoring);
  const browser=new ProfileCompanionPortBrowser(native);
  let catalogue=$state.raw<DeepReadonly<ProfileCompanionCatalogue>|null>(null),page=$state.raw<DeepReadonly<ProfileCompanionPage>|null>(null);
  let chosen=$state(untrack(()=>graph.document.companionAssignments.find(item=>item.resourceId===resourceId)?.publicationHash??'')),target=$state(untrack(()=>graph.assignments.find(item=>item.resourceId===resourceId)?.sourceId??'')),clearPorts=$state(false),group=$state<'input'|'output'>('input'),busy=$state(false),error=$state('');
  let mappings=$state.raw<Record<string,{observed:DeepReadonly<ObservedResourcePort>;page:DeepReadonly<ProfileCompanionPage>}>>({});
  let catalogueEpoch=0,portEpoch=0,context='';
  let catalogueContext='';
  const resource=$derived(graph.document.resources.find(resource=>resource.id===resourceId));
  const assignment=$derived(graph.assignments.find(assignment=>assignment.resourceId===resourceId));
  const captured=$derived(graph.document.companionAssignments.find(item=>item.resourceId===resourceId));
  const panels=$derived(latestCompanionPanels(catalogue?.panels??[]));
  const selectedPanel=$derived(selectedCompanionPanel(panels,captured));
  const choice=$derived(panels.find(panel=>panel.publicationHash===chosen)??(chosen===captured?.publicationHash?selectedPanel:undefined));
  const source=$derived(inventory.find(item=>item.id===target&&item.resourceType==='companion'));
  function panelReason(panel:DeepReadonly<ProfileCompanionPanel>):string {
    if(!panel.compatibility)return '';
    const companionId=source?.attributes.companionId;
    if(!companionId)return 'Choose a paired companion to see its compatible panels.';
    const match=panel.compatibility.find(item=>item.companionId===companionId);
    return match?.compatible?'':match?.reason??'This panel is not compatible with the selected companion.';
  }
  const compatiblePanels=$derived(panels.filter(panel=>!panelReason(panel)));
  const choiceReason=$derived(choice?panelReason(choice):'');
  const needsReconciliation=$derived(!!target&&target!==assignment?.sourceId&&(resource?.ports.length??0)>0);
  function companionInUse(sourceId:string){
    const companionId=inventory.find(item=>item.id===sourceId)?.attributes.companionId;
    return graph.assignments.some(other=>other.resourceId!==resourceId&&(other.sourceId===sourceId||!!companionId&&inventory.find(item=>item.id===other.sourceId)?.attributes.companionId===companionId));
  }
  const changingToUsedCompanion=$derived(target!==assignment?.sourceId&&companionInUse(target));
  const companionChoices=$derived(inventory.filter(item=>item.resourceType==='companion'&&(item.attributes.componentKind==='paired-companion'||item.id===assignment?.sourceId||!inventory.some(candidate=>candidate.attributes.componentKind==='paired-companion'))&&(item.id===assignment?.sourceId||!companionInUse(item.id))));
  const mappingOptions=$derived.by(()=>{const seen=new Set<string>();return [...(page?.ports.map(observed=>({observed,page}))??[]),...Object.values(mappings)].filter(item=>{const key=optionKey(item.observed);if(seen.has(key))return false;seen.add(key);return true;});});
  const replacing=$derived(!!choice&&choice.publicationHash!==captured?.publicationHash&&!(choice?.panelId&&choice.panelId===selectedPanel?.panelId)&&(resource?.ports.length??0)>0);
  const routeCount=$derived(graph.document.bindings.filter(binding=>binding.from.nodeId===resourceId||binding.to.nodeId===resourceId).length);
  const visiblePorts=$derived(page?.ports.filter(port=>port.descriptor.direction===group)??[]);
  function currentContext(){return JSON.stringify([resourceId,graph.assignments.find(item=>item.resourceId===resourceId)?.sourceId,graph.document.companionAssignments.find(item=>item.resourceId===resourceId)?.publicationHash,active]);}
  function syncContext(){const key=currentContext();if(key!==context){context=key;catalogueEpoch++;portEpoch++;browser.clear();page=null;chosen=graph.document.companionAssignments.find(item=>item.resourceId===resourceId)?.publicationHash??'';target=graph.assignments.find(item=>item.resourceId===resourceId)?.sourceId??'';mappings={};clearPorts=false;busy=false;error='';}}
  $effect(syncContext);
  function autoBrowsePanels(){
    const key=JSON.stringify([resourceId,assignment?.sourceId,captured?.panelId,captured?.publicationHash,active]);
    if(!active||resource?.kind!=='companion'||catalogueContext===key)return;
    catalogueContext=key;
    return untrack(refresh);
  }
  $effect(()=>{void autoBrowsePanels();});
  onDestroy(()=>{catalogueEpoch++;portEpoch++;browser.clear();});
  async function refresh(){
    if(!active||busy)return;const epoch=++catalogueEpoch,key=currentContext();busy=true;error='';
    try{const next=await native.catalogue();if(epoch===catalogueEpoch&&key===currentContext())catalogue=next;}
    catch(value){if(epoch===catalogueEpoch&&key===currentContext())error=value instanceof Error?value.message:String(value);}
    finally{if(epoch===catalogueEpoch&&key===currentContext())busy=false;}
  }
  function clearPage(){clearPorts=false;portEpoch++;browser.clear();page=null;mappings={};error='';}
  function choose(value:string){chosen=value;clearPage();}
  function chooseTarget(value:string){
    if(value!==assignment?.sourceId&&companionInUse(value)){error='This paired companion is already used by another profile block. Choose an unused companion.';return;}
    target=value;clearPage();
  }
  async function browse(operation:'open'|'next'|'previous'){
    const publication=choice?.publicationHash??chosen;
    if(!active||busy||!target||!publication||choiceReason)return;const epoch=++portEpoch,key=currentContext(),source=target;busy=true;error='';if(operation==='open'){mappings={};page=null;}
    try{const next=operation==='open'?await browser.open(source,publication):operation==='next'?await browser.next():await browser.previous();if(epoch===portEpoch&&key===currentContext()&&publication===(choice?.publicationHash??chosen)&&next)page=next;}
    catch(value){if(epoch===portEpoch&&key===currentContext()){error=value instanceof Error?value.message:String(value);page=null;browser.clear();mappings={};}}
    finally{if(epoch===portEpoch&&key===currentContext())busy=false;}
  }
  function edit(operation:()=>ProfileGraphDraft,label:string){if(readOnly||!active)return;error='';try{change(operation(),label);}catch(value){error=value instanceof Error?value.message:String(value);}}
  function capture(){
    if(!choice||!target||choiceReason||needsReconciliation||changingToUsedCompanion)return;
    if(target!==assignment?.sourceId&&!source)return;
    edit(()=>graphCaptureCompanionPanel(target===assignment?.sourceId?graph:graphAssignResource(graph,resourceId,source!,[]),resourceId,choice!,clearPorts),'Select companion panel');
  }
  function release(){edit(()=>graphReleaseCompanionPanel(graph,resourceId),'Remove captured companion panel');}
  function optionKey(port:DeepReadonly<ObservedResourcePort>){return JSON.stringify([port.descriptor.id,port.contractHash]);}
  function chooseMapping(portId:string,key:string){const port=mappingOptions.find(item=>optionKey(item.observed)===key),next={...mappings};if(port?.page)next[portId]={observed:port.observed,page:port.page};else delete next[portId];mappings=next;}
  function reconcile(){if(!choice||!source||!page||choiceReason)return;edit(()=>{
    if(target!==assignment?.sourceId&&companionInUse(target))throw new Error('This paired companion is already used by another profile block. Choose an unused companion.');
    return graphReconcileCompanionPanel(graph,resourceId,source!,choice!,page!,resource!.ports.flatMap(port=>mappings[port.id]?[{profilePortId:port.id,...mappings[port.id]}]:[]));
  },'Reconcile exact companion instance and panel ports');}
  function addPort(port:DeepReadonly<ObservedResourcePort>){if(!page)return;edit(()=>graphSelectCompanionPort(graph,resourceId,page!,port),'Select exact companion panel port');}
  function pin(port:DeepReadonly<ObservedResourcePort>){return assignment?.ports.find(pin=>pin.observedPortId===port.descriptor.id);}
</script>

<section class="companion-authoring" aria-label="Companion panel">
  <label>Companion<select value={target} disabled={readOnly||!active||busy} onchange={event=>chooseTarget(event.currentTarget.value)}><option value="">Choose companion</option>{#each companionChoices as item (item.id)}<option value={item.id}>{item.name} · {item.availability}</option>{/each}{#if assignment&&!inventory.some(item=>item.id===assignment.sourceId)}<option value={assignment.sourceId}>Saved companion · refresh inventory</option>{/if}</select></label>
  <div class="panel-select-row">
    <label>Panel<select value={choice?.publicationHash??chosen} disabled={!catalogue||!active||readOnly||busy} onchange={event=>choose(event.currentTarget.value)}><option value="">Choose panel</option>{#if captured&&!selectedPanel&&chosen===captured.publicationHash}<option value={chosen} disabled>Selected panel unavailable</option>{/if}{#each compatiblePanels as panel (panel.panelId??panel.publicationHash)}<option value={panel.publicationHash}>{panel.title} · {panel.controlCount} controls</option>{/each}{#if choiceReason&&choice}<option value={choice.publicationHash} disabled>{choice.title} · incompatible</option>{/if}</select></label>
    <button type="button" class="icon-button" aria-label="Refresh panels" title="Refresh panels" disabled={!active||busy} onclick={refresh}><Icon name="refresh" size={15}/></button>
  </div>
  {#if captured&&!selectedPanel&&chosen}<p class="panel-warning" role="status">Selected panel is missing from the local library.</p>{/if}
  {#if catalogue&&!panels.length&&!captured}<p class="inspector-note">No saved panels. Create one in the panel designer.</p>{/if}
  {#if catalogue&&panels.length&&!compatiblePanels.length}<p class="panel-warning" role="status">No compatible panels for this companion.</p>{/if}
  {#if choiceReason}<p class="panel-warning" role="status">{choiceReason}</p>{/if}
  {#if needsReconciliation}<p class="panel-warning" role="status">Map existing ports before replacing this companion.</p>{/if}
  {#if replacing}<label class="check"><input type="checkbox" bind:checked={clearPorts} disabled={readOnly||!active}/> Clear this panel and remove {resource?.ports.length??0} ports and {routeCount} routes before replacement</label>{/if}
  {#if choice||captured}<div class="panel-actions">
    {#if choice}<button type="button" class="secondary-button" disabled={readOnly||!active||busy||!target||(target!==assignment?.sourceId&&!source)||needsReconciliation||changingToUsedCompanion||!!choiceReason||(replacing&&!clearPorts)} onclick={capture}><Icon name="check" size={14}/>Use</button>
    <button type="button" class="text-button" disabled={!active||busy||!target||!!choiceReason} onclick={()=>browse('open')}><Icon name="search" size={14}/>Inspect</button>{/if}
    {#if captured}<button type="button" class="text-button" aria-label="Clear selected panel while retaining graph ports and routes" title="Clear selected panel; retain ports and routes" disabled={readOnly||!active} onclick={release}><Icon name="close" size={14}/>Clear</button>{/if}
    {#if choice}<span class="control-count">{choice.controlCount} controls</span>{/if}
  </div>{/if}
  {#if error}<p class="panel-warning" role="alert">{error}</p>{/if}
  {#each catalogue?.diagnostics??[] as diagnostic,index (index)}<p class="panel-warning" role="status">{diagnostic}</p>{/each}
  {#if page}
    {#if captured?.publicationHash!==page.publicationHash}<p class="panel-warning" role="status">Use this panel before adding its ports.</p>{/if}
    {#if source&&choice}<details class="mapping-details"><summary>Port mapping</summary><p>Map every selected port. Compatible routes are retained.</p>{#each resource?.ports??[] as logical (logical.id)}<label>{logical.id}<select value={mappings[logical.id]?optionKey(mappings[logical.id].observed):''} disabled={readOnly||!active} onchange={event=>chooseMapping(logical.id,event.currentTarget.value)}><option value="">Choose matching port</option>{#each mappingOptions.filter(item=>item.observed.descriptor.direction===logical.direction) as item (optionKey(item.observed))}<option value={optionKey(item.observed)}>{item.observed.descriptor.name} · {item.observed.descriptor.id}</option>{/each}</select><small>{portContractLabel(logical.contract)}</small></label>{/each}<button type="button" class="secondary-button" disabled={readOnly||!active||busy||changingToUsedCompanion||!!choiceReason||resource?.ports.some(port=>!mappings[port.id])} onclick={reconcile}><Icon name="link" size={14}/>Apply mapping</button></details>{/if}
    <label>Ports<select bind:value={group}><option value="input">Feedback inputs</option><option value="output">Control outputs</option></select></label>
    <p>Only selected ports are added.</p>
    <ul class="panel-ports">{#each visiblePorts as port (port.descriptor.id)}{@const selectedPin=pin(port)}<li><div class="port-row"><strong>{port.descriptor.name}</strong>{#if selectedPin}<span class="port-state" title={selectedPin.expectedContractHash===port.contractHash?'Selected exact port':'Contract changed; reconcile this port'}><Icon name={selectedPin.expectedContractHash===port.contractHash?'check':'info'} size={13}/>{selectedPin.expectedContractHash===port.contractHash?'Selected':'Changed'}</span>{:else}<button type="button" class="text-button" aria-label={`Add ${port.descriptor.name}`} disabled={readOnly||!active||assignment?.sourceId!==page.resourceId||captured?.publicationHash!==page.publicationHash||(resource?.ports.length??0)>=128} onclick={()=>addPort(port)}><Icon name="plus" size={13}/>Add</button>{/if}</div><details class="port-details"><summary>Port details</summary><code>{port.descriptor.id}</code><span>{portContractLabel(port.descriptor.contract)}</span></details></li>{:else}<li class="empty-ports">No ports in this group on this page.</li>{/each}</ul>
    <div class="page-actions"><span>{#if page.total}{page.offset+1}–{page.offset+page.ports.length} of {page.total}{:else}0 ports{/if}</span><button type="button" class="text-button" disabled={!active||busy||!browser.hasPrevious} onclick={()=>browse('previous')}><Icon name="chevron" size={13}/><span>Previous</span></button><button type="button" class="text-button next" disabled={!active||busy||!browser.hasNext} onclick={()=>browse('next')}><span>Next</span><Icon name="chevron" size={13}/></button></div>
  {/if}
</section>

<style>
  .companion-authoring{border-top:1px solid var(--line);padding-top:12px;margin-top:14px;min-width:0}
  label{display:grid;gap:6px;margin:8px 0;color:var(--muted);font-size:var(--ui-text-size)}
  select{width:100%;box-sizing:border-box;min-width:0;min-height:var(--ui-control-height);padding:7px 8px;border:1px solid var(--line);border-radius:5px;background:var(--input-bg);color:var(--text);font-size:var(--ui-text-size)}
  button{font-size:var(--ui-action-size)}
  .panel-select-row{display:grid;grid-template-columns:minmax(0,1fr) var(--ui-control-height);gap:8px;align-items:end}
  .panel-select-row label{min-width:0}
  .icon-button{display:grid;place-items:center;width:var(--ui-control-height);height:var(--ui-control-height);margin-bottom:8px;padding:0;border:1px solid var(--line);border-radius:5px;background:var(--input-bg);color:var(--muted);cursor:pointer}
  .icon-button:hover:enabled{border-color:var(--accent);color:var(--accent)}
  .icon-button:disabled{opacity:.5;cursor:default}
  .panel-actions{display:flex;align-items:center;gap:8px;flex-wrap:wrap;margin:10px 0}
  .panel-actions button,.page-actions button{display:inline-flex;align-items:center;gap:5px}
  .control-count,.page-actions>span{margin-left:auto;color:var(--muted);font-size:var(--ui-text-size)}
  p{margin:7px 0;color:var(--muted);font-size:var(--ui-text-size);line-height:var(--ui-line-height)}
  .panel-warning{color:#d3a078}
  .inspector-note{color:var(--muted)}
  .check{display:flex;align-items:flex-start;gap:8px;line-height:var(--ui-line-height)}
  .mapping-details{margin:10px 0;border-top:1px solid var(--line);padding-top:8px}
  details>summary{color:var(--soft);font-size:var(--ui-text-size);cursor:pointer}
  .mapping-details label{margin:8px 0}
  small{font-size:var(--ui-text-size);line-height:var(--ui-line-height);color:var(--muted)}
  .panel-ports{list-style:none;padding:0;margin:8px 0}
  .panel-ports li{padding:8px 0;border-bottom:1px solid var(--line);font-size:var(--ui-text-size);color:var(--muted)}
  .port-row{display:flex;align-items:center;justify-content:space-between;gap:8px;min-width:0}
  .port-row strong{min-width:0;overflow:hidden;text-overflow:ellipsis;color:var(--soft);font-weight:500}
  .port-row button,.port-state{display:inline-flex;align-items:center;gap:5px;flex-shrink:0}
  .port-state{color:var(--muted)}
  .port-details{margin-top:5px;color:var(--muted)}
  .port-details summary{font-size:calc(var(--ui-text-size) - 1px)}
  code{display:block;overflow-wrap:anywhere;margin-top:5px;font-size:var(--ui-text-size)}
  .port-details span{display:block;margin-top:4px;line-height:var(--ui-line-height)}
  .empty-ports{color:var(--muted)}
  .page-actions{display:flex;align-items:center;gap:8px}
  .page-actions>span{margin:0 auto 0 0}
  .page-actions button:not(.next) :global(svg){transform:rotate(180deg)}
</style>
