<script lang="ts">
  import {onDestroy,untrack} from 'svelte';
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
  const needsReconciliation=$derived(target!==assignment?.sourceId&&(resource?.ports.length??0)>0);
  function companionInUse(sourceId:string){
    const companionId=inventory.find(item=>item.id===sourceId)?.attributes.companionId;
    return graph.assignments.some(other=>other.resourceId!==resourceId&&(other.sourceId===sourceId||!!companionId&&inventory.find(item=>item.id===other.sourceId)?.attributes.companionId===companionId));
  }
  const changingToUsedCompanion=$derived(target!==assignment?.sourceId&&companionInUse(target));
  const companionChoices=$derived(inventory.filter(item=>item.resourceType==='companion'&&(item.attributes.componentKind==='paired-companion'||item.id===assignment?.sourceId||!inventory.some(candidate=>candidate.attributes.componentKind==='paired-companion'))&&(item.id===assignment?.sourceId||!companionInUse(item.id))));
  const mappingOptions=$derived.by(()=>{const seen=new Set<string>();return [...(page?.ports.map(observed=>({observed,page}))??[]),...Object.values(mappings)].filter(item=>{const key=optionKey(item.observed);if(seen.has(key))return false;seen.add(key);return true;});});
  const replacing=$derived(choice?.publicationHash!==captured?.publicationHash&&!(choice?.panelId&&choice.panelId===selectedPanel?.panelId)&&(resource?.ports.length??0)>0);
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

<section class="companion-authoring" aria-label="Profile companion panel selection">
  <div class="panel-heading"><h3>Companion panel</h3><button class="text-button" disabled={!active||busy} onclick={refresh}>{busy?'Reading saved panels…':'Refresh panels'}</button></div>
  <p>Choose a saved panel for this companion. Saving changes to that panel updates profiles that use it.</p>
  {#if !assignment}<p class="panel-warning">Choose an exact paired companion and a compatible panel. Blocks with existing ports need explicit port reconciliation.</p>{/if}
  <label>Paired companion<select value={target} disabled={readOnly||!active||busy} onchange={event=>chooseTarget(event.currentTarget.value)}><option value="">Choose a companion explicitly</option>{#each companionChoices as item (item.id)}<option value={item.id}>{item.name} · {item.availability}</option>{/each}{#if assignment&&!inventory.some(item=>item.id===assignment.sourceId)}<option value={assignment.sourceId}>Saved companion · refresh inventory to reassign</option>{/if}</select></label>
  {#if captured}<p>Selected panel: <strong title={selectedPanel?.title}>{selectedPanel?.title??'Panel unavailable'}</strong></p><button class="text-button" disabled={readOnly||!active} onclick={release}>Remove selected panel · retain graph ports and routes</button>{:else}<p>No panel is selected for this block.</p>{/if}
  {#if error}<p class="panel-warning" role="alert">{error}</p>{/if}
  {#if catalogue}
    <label>Panel<select value={choice?.publicationHash??chosen} disabled={!active||readOnly||busy} onchange={event=>choose(event.currentTarget.value)}><option value="">Choose a saved panel</option>{#each compatiblePanels as panel (panel.panelId??panel.publicationHash)}<option value={panel.publicationHash}>{panel.title} · {panel.controlCount} controls</option>{/each}{#if choiceReason&&choice}<option value={choice.publicationHash} disabled>{choice.title} · incompatible</option>{/if}</select></label>
    {#if !panels.length}<p>No saved panels are available. Create and save a panel in the panel designer, then refresh this list.</p>{/if}
    {#if panels.length&&!compatiblePanels.length}<p>No saved panels are compatible with this companion. Check the panel size and the companion’s supported widgets.</p>{/if}
    {#if choiceReason}<p class="panel-warning">{choiceReason}</p>{/if}
    {#if captured&&!selectedPanel}<p class="panel-warning">The selected panel is missing from the local library. Save that panel again or choose a replacement and reconcile this block.</p>{/if}
    {#if choice}
      <p>{choice.controlCount} actual interactive controls in this panel. A feedback-only graph can select only its declared feedback inputs; no dummy controls or routes are added.</p>
      {#if needsReconciliation}<p class="panel-warning">Inspect the panel ports to map this block’s existing ports to the replacement companion.</p>{/if}
      {#if replacing}<label class="check"><input type="checkbox" bind:checked={clearPorts} disabled={readOnly||!active}/> Remove this companion’s {resource?.ports.length??0} selected ports and {routeCount} routes before using the replacement panel.</label>{/if}
      <div class="panel-actions"><button class="secondary-button" disabled={readOnly||!active||busy||!target||(target!==assignment?.sourceId&&!source)||needsReconciliation||changingToUsedCompanion||!!choiceReason||(replacing&&!clearPorts)} onclick={capture}>Use panel in profile</button><button class="text-button" disabled={!active||busy||!target||!!choiceReason} onclick={()=>browse('open')}>Inspect exact panel ports</button></div>
    {/if}
    {#each catalogue.diagnostics as diagnostic,index (index)}<p class="panel-warning">{diagnostic}</p>{/each}
  {/if}
  {#if page}
    <p>Prospective retained contracts · no live baseline or delivery tokens.</p>
    {#if captured?.publicationHash!==page.publicationHash}<p class="panel-warning">Use this panel in the profile before adding its ports to the graph.</p>{/if}
    {#if source&&choice}<details><summary>Explicit instance and port reconciliation</summary><p>Use this saved panel on the selected paired companion, including when it currently displays another panel. Map every saved logical port explicitly; compatible graph routes remain.</p>{#each resource?.ports??[] as logical (logical.id)}<label>Logical {logical.id} → exact panel port<select value={mappings[logical.id]?optionKey(mappings[logical.id].observed):''} disabled={readOnly||!active} onchange={event=>chooseMapping(logical.id,event.currentTarget.value)}><option value="">Choose an exact matching contract</option>{#each mappingOptions.filter(item=>item.observed.descriptor.direction===logical.direction) as item (optionKey(item.observed))}<option value={optionKey(item.observed)}>{item.observed.descriptor.name} · {item.observed.descriptor.id}</option>{/each}</select><small>{portContractLabel(logical.contract)}</small></label>{/each}<button class="secondary-button" disabled={readOnly||!active||busy||changingToUsedCompanion||!!choiceReason||resource?.ports.some(port=>!mappings[port.id])} onclick={reconcile}>Assign companion and reconcile panel ports</button></details>{/if}
    <label>Port group<select bind:value={group}><option value="input">Feedback / widget inputs</option><option value="output">Interactive control outputs</option></select></label>
    <ul class="panel-ports">{#each visiblePorts as port (port.descriptor.id)}<li><strong>{port.descriptor.name}</strong><code>{port.descriptor.id}</code><span>{portContractLabel(port.descriptor.contract)}</span><button class="text-button" disabled={readOnly||!active||assignment?.sourceId!==page.resourceId||captured?.publicationHash!==page.publicationHash||!!pin(port)||(resource?.ports.length??0)>=128} onclick={()=>addPort(port)}>{pin(port)?pin(port)?.expectedContractHash===port.contractHash?'Selected exact port':'Contract changed · reconcile selected port':port.descriptor.direction==='input'?'Add feedback input':'Add control output'}</button></li>{:else}<li>No ports from this group on this page. Browse another page or group.</li>{/each}</ul>
    <p>{#if page.total}{page.offset+1}–{page.offset+page.ports.length} of {page.total}{:else}0{/if} declared ports. Only explicitly selected ports are saved.</p>
    <div class="panel-actions"><button class="text-button" disabled={!active||busy||!browser.hasPrevious} onclick={()=>browse('previous')}>Previous panel ports</button><button class="text-button" disabled={!active||busy||!browser.hasNext} onclick={()=>browse('next')}>Next panel ports</button></div>
  {/if}
</section>

<style>
  .companion-authoring{border-top:1px solid var(--line);padding-top:16px;margin-top:18px;min-width:0}.panel-heading{display:flex;align-items:center;justify-content:space-between;gap:10px}h3{font-size:12px;color:var(--soft)}p{font-size:10px;line-height:1.8;color:var(--muted);margin:10px 0}code{display:block;overflow-wrap:anywhere;font-size:9px;margin-top:5px}button{font-size:10px}label{display:grid;gap:8px;font-size:9px;color:var(--muted);margin:14px 0}select{width:100%;box-sizing:border-box;min-width:0;background:var(--input-bg);color:var(--text);border:1px solid var(--line);border-radius:5px;padding:8px;font-size:10px}.check{display:flex;align-items:start;gap:8px;line-height:1.8}.panel-warning{color:#d3a078}.panel-actions{display:flex;gap:12px;align-items:center;flex-wrap:wrap}.panel-ports{list-style:none;padding:0;margin:12px 0}.panel-ports li{display:grid;gap:6px;padding:12px 0;border-bottom:1px solid var(--line);font-size:10px;color:var(--muted)}.panel-ports strong{color:var(--soft)}.panel-ports span{font-size:9px;line-height:1.8}.panel-ports button{justify-self:start;text-align:left}
</style>
