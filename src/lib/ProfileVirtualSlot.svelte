<script lang="ts">
  import {onDestroy,onMount,untrack} from 'svelte';
  import Icon from './Icon.svelte';
  import {gatewayRequest} from './gateway';
  import {virtualBackendSlots,virtualLayoutForSlot,virtualSlotAssignments,virtualSlotLayoutIssue,virtualSlotSummary,type VirtualBackendSlot} from './profile-virtual-slots';
  import type {DeepReadonly,ProfileControllerLayout,ProfileVirtualOutput} from './profiles';
  let {output,profileId='',revision=0,disabled=false,changeLayout}:{output:DeepReadonly<ProfileVirtualOutput>;profileId?:string;revision?:number;disabled?:boolean;changeLayout:(layout:ProfileControllerLayout)=>void|boolean}=$props();
  let slots=$state.raw<VirtualBackendSlot[]>([]),selected=$state<number|null>(null),busy=$state(false),loaded=$state(false),readOnly=$state(false),saved=$state(false),error=$state(''),mounted=$state(false);
  let epoch=0,disposed=false;
  const slotId=$props.id();
  const slot=$derived(slots.find(slot=>slot.slot===selected));
  const issue=$derived(slot?virtualSlotLayoutIssue(slot,output.layout):selected!==null?'The selected slot is unavailable. Refresh or choose another slot.':'');
  const canUseLayout=$derived.by(()=>{if(!slot||!issue)return false;try{virtualLayoutForSlot(slot,output.layout);return true;}catch{return false;}});
  async function refresh(){
    const requestEpoch=++epoch,id=profileId,nodeId=output.id;
    busy=true;loaded=false;error='';
    const results=await Promise.allSettled([gatewayRequest({command:'profile_virtual_slots_get',id}),gatewayRequest({command:'system_inventory'})]);
    if(disposed||requestEpoch!==epoch)return;
    try{
      if(results[1].status==='fulfilled')slots=virtualBackendSlots(results[1].value.systemInventory);
      else throw results[1].reason;
      if(results[0].status!=='fulfilled')throw results[0].reason;
      const settings=virtualSlotAssignments(results[0].value.profiles,id);
      selected=settings.virtualSlots.find(pin=>pin.nodeId===nodeId)?.slot??null;
      saved=settings.virtualOutputIds.includes(nodeId);readOnly=settings.readOnly;loaded=true;
    }catch(reason){error=reason instanceof Error?reason.message:String(reason);}
    finally{busy=false;}
  }
  async function select(value:string){
    const previous=selected,desired=value===''?null:Number(value),requestEpoch=++epoch,id=profileId,nodeId=output.id;
    selected=desired;busy=true;error='';
    try{
      const response=await gatewayRequest({command:'profile_virtual_slot_set',id,expectedRevision:revision,nodeId,slot:desired});
      if(disposed||requestEpoch!==epoch)return;
      const settings=virtualSlotAssignments(response.profiles,id);
      selected=settings.virtualSlots.find(pin=>pin.nodeId===nodeId)?.slot??null;
    }catch(reason){if(!disposed&&requestEpoch===epoch){selected=previous;error=reason instanceof Error?reason.message:String(reason);}}
    finally{if(!disposed&&requestEpoch===epoch)busy=false;}
  }
  function useLayout(){
    if(!slot)return;
    try{if(changeLayout(virtualLayoutForSlot(slot,output.layout))===false)throw Error('Disconnect affected inputs before reducing this layout.');error='';}
    catch(reason){error=reason instanceof Error?reason.message:String(reason);}
  }
  onMount(()=>{mounted=true;});onDestroy(()=>{disposed=true;epoch++;});
  $effect(()=>{const key=`${profileId}/${revision}/${output.id}`;if(mounted&&key)untrack(()=>void refresh());});
</script>

<section class="virtual-slot" aria-label="Virtual controller slot">
  <div class="heading"><label for={slotId}>Slot</label><button type="button" class="text-button" aria-label="Refresh slots" title="Refresh slots" disabled={busy} onclick={()=>void refresh()}><Icon name="refresh" size={14}/></button></div>
  <select id={slotId} aria-label="Slot" value={selected??''} disabled={disabled||busy||!loaded||readOnly||!saved} onchange={event=>void select(event.currentTarget.value)}>
    <option value="">Automatic</option>
    {#if selected!==null&&!slot}<option value={selected}>Slot {selected} · Unavailable</option>{/if}
    {#each slots as choice(choice.slot)}<option value={choice.slot}>Slot {choice.slot} · {choice.status} · {virtualSlotSummary(choice)}</option>{/each}
  </select>
  {#if loaded&&!saved}<p>Save this block before choosing a slot.</p>{:else if loaded}<p>Saved on this computer. Takes effect on the next activation.</p>{/if}
  {#if loaded&&!slots.length}<p>No configured slots. Configure one in Settings → Controller and desktop output.</p>{/if}
  {#if issue}<p class="slot-issue" role="status">{issue}</p>{/if}
  {#if canUseLayout}<button type="button" class="secondary-button" disabled={disabled||busy||readOnly} onclick={useLayout}><Icon name="settings" size={14}/>Use layout</button>{/if}
  {#if error}<p class="slot-error" role="alert">{error}</p>{/if}
</section>

<style>
  .virtual-slot { margin:12px 0 16px; min-width:0; }
  .heading { display:flex; align-items:center; justify-content:space-between; gap:8px; margin-bottom:7px; }
  label,p { color:var(--muted);font-size:var(--ui-text-size); }
  select { width:100%;min-width:0;box-sizing:border-box;min-height:var(--ui-control-height);background:var(--input-bg);border:1px solid var(--line);border-radius:5px;padding:8px;color:var(--text);font-size:var(--ui-text-size); }
  p { margin:8px 0;line-height:var(--ui-line-height); }
  .slot-issue { color:var(--soft); }
  .slot-error { color:var(--danger,#e67979); }
  button { font-size:var(--ui-action-size); }
</style>
