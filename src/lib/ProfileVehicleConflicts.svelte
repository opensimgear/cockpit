<script lang="ts">
  import {onDestroy,untrack} from 'svelte';
  import type {ProfileCompilerClient} from './profile-compilation';
  import type {DeepReadonly,ProfileDocument,ProfileLocalAssignment} from './profiles';
  import {ProfileVehicleConflictController} from './profile-vehicle-conflicts';
  let {document,assignments,activeVehicleTabId='',compiler,inventoryRevision=0,active=true}:{
    document:DeepReadonly<ProfileDocument>;assignments:readonly DeepReadonly<ProfileLocalAssignment>[];activeVehicleTabId?:string;
    compiler:Pick<ProfileCompilerClient,'compile'|'inspect'>;inventoryRevision?:number;active?:boolean;
  }=$props();
  const controller=new ProfileVehicleConflictController(untrack(()=>compiler));
  let previewState=$state.raw(controller.state);
  const unsubscribe=controller.subscribe(value=>previewState=value);
  const scope=$derived(activeVehicleTabId?`Base + ${document.vehicleTabs?.find(tab=>tab.id===activeVehicleTabId)?.vehicleName??'vehicle'}`:'Base');
  $effect(()=>{controller.update(document,assignments,activeVehicleTabId,inventoryRevision,active);});
  onDestroy(()=>{unsubscribe();controller.dispose();});
  function location(nodeId:string|null,resourceId:string|null,portId:string|null):string{
    const id=nodeId??resourceId,node=[...document.resources,...document.processingNodes,...document.virtualOutputs,...(document.desktopOutputs??[])].find(node=>node.id===id);
    return[node?.name??id,portId].filter(Boolean).join(' · ');
  }
</script>

{#if active&&previewState.diagnostics.length>0}
  <section class="vehicle-conflicts" aria-label="Vehicle configuration conflicts">
    <div class="conflict-heading"><strong>{scope} conflicts</strong><span role="status">{previewState.diagnostics.length} {previewState.diagnostics.length===1?'conflict':'conflicts'} found</span></div>
    <ul>{#each previewState.diagnostics as diagnostic,index (`${diagnostic.code}/${index}`)}{@const endpoint=location(diagnostic.location.nodeId,diagnostic.location.resourceId,diagnostic.location.portId)}<li><p>{diagnostic.message}</p>{#if endpoint}<small>{endpoint}</small>{/if}</li>{/each}</ul>
  </section>
{/if}

<style>
  .vehicle-conflicts{margin:0 0 12px;padding:12px 14px;border:1px solid #ad875440;border-radius:6px;background:var(--surface);font-size:10px;color:var(--muted)}.conflict-heading{display:flex;align-items:center;justify-content:space-between;gap:12px;flex-wrap:wrap}.conflict-heading strong{font-weight:500;color:var(--soft)}.conflict-heading span{color:#d3a078}.vehicle-conflicts ul{margin:12px 0 0;padding-left:18px;display:grid;gap:9px}.vehicle-conflicts p{margin:0;line-height:1.7}.vehicle-conflicts small{font-size:9px;color:var(--soft)}
</style>
