<script lang="ts">
  import {onMount,untrack} from 'svelte';
  import Icon from './Icon.svelte';
  import {gatewayRequest} from './gateway';
  import {virtualBackendSlots,virtualLayoutForSlot,virtualSlotAssignments,virtualSlotLayoutIssue,virtualSlotSummary,type VirtualBackendSlot} from './profile-virtual-slots';
  import type {DeepReadonly,ProfileControllerLayout,ProfileVirtualOutput} from './profiles';
  import {observeProfileLibrary} from './profile-read-models/shared';
  import type {LibraryModel,ModelState} from './profile-read-models/models';
  import {SharedObservations} from './streams/shared';
  let {output,profileId='',revision=0,disabled=false,active=true,changeLayout}:{output:DeepReadonly<ProfileVirtualOutput>;profileId?:string;revision?:number;disabled?:boolean;active?:boolean;changeLayout:(layout:ProfileControllerLayout)=>void|boolean}=$props();
  let slots=$state.raw<VirtualBackendSlot[]>([]),selected=$state<number|null>(null);
  let pinBusy=$state(false),inventoryBusy=$state(false),saving=$state(false);
  let loaded=$state(false),readOnly=$state(false),saved=$state(false),inventoryReady=$state(false);
  let pinError=$state(''),inventoryError=$state(''),visible=$state(false);
  let library=$state.raw<ModelState<LibraryModel>>({status:'connecting'});
  let pinEpoch=0,inventoryEpoch=0,lifetime=0,disposed=true;
  const context=$derived(`${profileId}/${revision}/${output.id}`);
  const version=$derived(library.status==='ready'?library.value?.versions[profileId]:undefined);
  const publication=$derived(library.status==='ready'
    ? JSON.stringify([library.clockId,library.session,library.generation,version,library.value?.readOnly])
    : '');
  const busy=$derived(pinBusy||inventoryBusy||saving);
  const error=$derived([pinError,inventoryError].filter(Boolean).join(' '));
  const slotId=$props.id();
  const slot=$derived(slots.find(slot=>slot.slot===selected));
  const issue=$derived(slot?virtualSlotLayoutIssue(slot,output.layout):selected!==null?'The selected slot is unavailable. Refresh or choose another slot.':'');
  const canUseLayout=$derived.by(()=>{if(!slot||!issue)return false;try{virtualLayoutForSlot(slot,output.layout);return true;}catch{return false;}});
  async function loadPins(){
    const requestEpoch=++pinEpoch,id=profileId,nodeId=output.id,requestContext=context,source=publication;
    const current=()=>!disposed&&requestEpoch===pinEpoch&&requestContext===context&&source===publication;
    pinBusy=true;loaded=false;pinError='';
    try{
      const result=await gatewayRequest({command:'profile_virtual_slots_get',id});
      if(!current())return;
      const settings=virtualSlotAssignments(result.profiles,id);
      selected=settings.virtualSlots.find(pin=>pin.nodeId===nodeId)?.slot??null;
      saved=settings.virtualOutputIds.includes(nodeId);readOnly=settings.readOnly;loaded=true;
    }catch(reason){if(current())pinError=reason instanceof Error?reason.message:String(reason);}
    finally{if(current())pinBusy=false;}
  }
  async function loadInventory(){
    const requestEpoch=++inventoryEpoch;
    inventoryBusy=true;inventoryError='';
    try{
      const result=await gatewayRequest({command:'system_inventory'});
      if(disposed||requestEpoch!==inventoryEpoch)return;
      slots=virtualBackendSlots(result.systemInventory);inventoryReady=true;
    }catch(reason){
      if(!disposed&&requestEpoch===inventoryEpoch){
        inventoryReady=false;inventoryError=reason instanceof Error?reason.message:String(reason);
      }
    }finally{if(!disposed&&requestEpoch===inventoryEpoch)inventoryBusy=false;}
  }
  function refresh(){
    if(version)void loadPins();
    void loadInventory();
  }
  async function select(value:string){
    const previous=selected,desired=value===''?null:Number(value),id=profileId,nodeId=output.id,requestContext=context,source=publication,scope=lifetime;
    const current=()=>!disposed&&scope===lifetime&&requestContext===context&&source===publication;
    selected=desired;saving=true;pinError='';
    try{
      const response=await gatewayRequest({command:'profile_virtual_slot_set',id,expectedRevision:revision,nodeId,slot:desired});
      if(!current())return;
      const settings=virtualSlotAssignments(response.profiles,id);
      selected=settings.virtualSlots.find(pin=>pin.nodeId===nodeId)?.slot??null;
    }catch(reason){if(current()){selected=previous;pinError=reason instanceof Error?reason.message:String(reason);}}
    finally{if(!disposed&&scope===lifetime)saving=false;}
  }
  function useLayout(){
    if(!slot)return;
    try{if(changeLayout(virtualLayoutForSlot(slot,output.layout))===false)throw Error('Disconnect affected inputs before reducing this layout.');pinError='';}
    catch(reason){pinError=reason instanceof Error?reason.message:String(reason);}
  }
  onMount(()=>{
    const update=()=>{visible=!document.hidden;};
    update();
    document.addEventListener('visibilitychange',update);
    return()=>document.removeEventListener('visibilitychange',update);
  });
  $effect(()=>{
    if(!active||!visible)return;
    disposed=false;
    const stopLibrary=observeProfileLibrary(state=>library=state);
    const observations=new SharedObservations();
    // The same native reader serves every visible vJoy status view.
    observations.subscribe('system.inventory.vjoy',()=>{});
    observations.subscribe('system.inventory',state=>{
      if(state.status==='ready')void loadInventory();
      else if(state.status==='lost'||state.status==='error'){
        inventoryEpoch++;inventoryBusy=false;inventoryReady=false;
        inventoryError=state.error??'Waiting for a current virtual controller inventory.';
      }
    });
    return()=>{
      disposed=true;lifetime++;pinEpoch++;inventoryEpoch++;
      library={status:'connecting'};inventoryReady=false;pinBusy=false;inventoryBusy=false;saving=false;
      stopLibrary();observations.close();
    };
  });
  $effect(()=>{
    const key=context,source=publication;
    if(!active||!visible||!key)return;
    if(source&&version){untrack(()=>void loadPins());return;}
    pinEpoch++;pinBusy=false;loaded=!!source;saved=false;selected=null;pinError='';
  });
</script>

<section class="virtual-slot" aria-label="Virtual controller slot">
  <div class="heading"><label for={slotId}>Slot</label><button type="button" class="text-button" aria-label="Refresh slots" title="Refresh slots" disabled={busy} onclick={()=>void refresh()}><Icon name="refresh" size={14}/></button></div>
  <select id={slotId} aria-label="Slot" value={selected??''} disabled={disabled||busy||!loaded||!inventoryReady||readOnly||!saved} onchange={event=>void select(event.currentTarget.value)}>
    <option value="">Automatic</option>
    {#if selected!==null&&!slot}<option value={selected}>Slot {selected} · Unavailable</option>{/if}
    {#each slots as choice(choice.slot)}<option value={choice.slot}>Slot {choice.slot} · {choice.status} · {virtualSlotSummary(choice)}</option>{/each}
  </select>
  {#if loaded&&!saved}<p>Save this block before choosing a slot.</p>{:else if loaded}<p>Saved on this computer. Takes effect on the next activation.</p>{/if}
  {#if inventoryReady&&!slots.length}<p>No configured slots. Configure one in Settings → Controller and desktop output.</p>{/if}
  {#if issue}<p class="slot-issue" role="status">{issue}</p>{/if}
  {#if canUseLayout}<button type="button" class="secondary-button" disabled={disabled||busy||!inventoryReady||readOnly} onclick={useLayout}><Icon name="settings" size={14}/>Use layout</button>{/if}
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
