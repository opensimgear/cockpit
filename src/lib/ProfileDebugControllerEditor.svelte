<script lang="ts">
  import {onDestroy,onMount,untrack} from 'svelte';
  import DebugControllerControls from './DebugControllerControls.svelte';
  import {debugControllerLayout,debugControllerPorts,createDebugControllerClient,type DebugControllerClient,type DebugControllerChange,type DebugControllerLayout,type DebugControllerRequest} from './profile-debug-controller';
  import {nativeProfileStatus} from './profile-runtime';
  import {gatewayRequest} from './gateway';
  import type {DeepReadonly,ProfileResource} from './profiles';
  let {resource,profileId,revision,active,disabled=false,update}:{resource:DeepReadonly<ProfileResource>;profileId:string;revision:number;active:boolean;disabled?:boolean;update:(resource:ProfileResource)=>void}=$props();
  const applyHint='Apply this profile to use these controls.';
  let layoutError=$state(''),values=$state<Record<string,boolean|number>>({}),interacting=$state(false),liveError=$state(''),ready=$state(false),mounted=$state(false);
  let client=$state.raw<DebugControllerClient|null>(null);
  const layout=$derived(debugControllerLayout(resource));
  const controlsActive=$derived(active&&!disabled&&ready&&!liveError);
  const limits={buttons:128,axes:16,hats:4} as const;
  function copyLayout():DebugControllerLayout{return JSON.parse(JSON.stringify(layout));}
  function emit(next:DebugControllerLayout){try{const ports=debugControllerPorts(next);update({...resource,ports});layoutError='';}catch(error){layoutError=error instanceof Error?error.message:String(error);}}
  function resize(kind:keyof typeof limits,input:HTMLInputElement){
    const count=input.valueAsNumber,max=Math.min(limits[kind],128-Object.entries(layout).filter(([key])=>key!==kind).reduce((sum,[,items])=>sum+items.length,0));
    if(!input.value.trim()||!Number.isSafeInteger(count)||count<0||count>max){layoutError=`Choose a whole number from 0 to ${max}.`;input.value=String(layout[kind].length);return;}
    const next=copyLayout(),used=new Set([...next.buttons,...next.axes.map(axis=>axis.portId),...next.hats]);
    const fresh=(prefix:string)=>{let index=1;while(used.has(`${prefix}-${index}`))index++;const id=`${prefix}-${index}`;used.add(id);return id;};
    if(kind==='axes')next.axes=Array.from({length:count},(_,index)=>next.axes[index]??{portId:fresh('axis'),range:'bipolar',fallback:0});
    else next[kind]=Array.from({length:count},(_,index)=>next[kind][index]??fresh(kind==='buttons'?'button':'hat'));
    emit(next);input.value=String(next[kind].length);
  }
  function editAxis(index:number,input:HTMLSelectElement){const next=copyLayout(),axis=next.axes[index];if(!axis)return;axis.range=input.value==='unipolar'?'unipolar':'bipolar';axis.fallback=0;emit(next);}
  function label(value:string){update({...JSON.parse(JSON.stringify(resource)) as ProfileResource,name:value});}
  function send(changes:DebugControllerChange[]){const owner=client?.currentOwner;if(!controlsActive||!client||!owner)return;client.push(owner,changes);for(const change of changes)values[change.portId]=change.value;values={...values};}
  onMount(()=>{mounted=true;});
  $effect(()=>{
    // Read every identity field so changing the inspected resource fences the old stream.
    const id=resource.id,currentProfile=profileId,currentRevision=revision,visible=active,readOnly=disabled;
    if(!mounted||!visible||readOnly){untrack(()=>client)?.close();client=null;values={};ready=false;interacting=false;return;}
    const sendRequest=(request:DebugControllerRequest)=>gatewayRequest(request as never);
    const next=createDebugControllerClient(sendRequest,id,error=>{if(untrack(()=>client===next)){ready=false;liveError=error instanceof Error?error.message:'Debug input update failed.';}});
    untrack(()=>client)?.close();client=next;values={};ready=false;liveError=applyHint;
    let disposed=false,refreshing=false;
    async function refresh(){
      if(disposed||refreshing||untrack(()=>interacting))return;refreshing=true;
      try{
        const status=nativeProfileStatus((await gatewayRequest({command:'profile_status'})).profiles?.profileSession),owner=status.owner;
        if(disposed)return;
        if(!owner||owner.profileId!==currentProfile||owner.profileRevision!==currentRevision){next.clearOwner();values={};ready=false;liveError=applyHint;return;}
        const samples=await next.status(owner);if(disposed)return;
        if(!untrack(()=>interacting))values=Object.fromEntries(samples.map(sample=>[sample.portId,sample.value]));ready=true;liveError='';
      }catch(error){if(!disposed){next.clearOwner();ready=false;liveError=error instanceof Error?error.message:'Debug controller status is unavailable.';}}
      finally{refreshing=false;}
    }
    const refreshNow=()=>{if(!document.hidden)void refresh();};
    // The initial exact-owner check also works when the window is occluded or unfocused.
    untrack(()=>void refresh());const timer=window.setInterval(refreshNow,500);
    window.addEventListener('focus',refreshNow);document.addEventListener('visibilitychange',refreshNow);
    return()=>{disposed=true;window.clearInterval(timer);window.removeEventListener('focus',refreshNow);document.removeEventListener('visibilitychange',refreshNow);next.close();if(untrack(()=>client===next))client=null;};
  });
  onDestroy(()=>{client?.close();});
</script>

<section class="debug-editor" aria-label="Debug controller settings">
  <label class="name-field">Label<input maxlength="80" value={resource.name} disabled={disabled} onchange={event=>label(event.currentTarget.value)}/></label>
  <div class="count-grid">{#each [{key:'buttons' as const,label:'Buttons'},{key:'axes' as const,label:'Axes'},{key:'hats' as const,label:'Hats'}] as item}<label>{item.label}<input type="number" min="0" max={limits[item.key]} step="1" value={layout[item.key].length} disabled={disabled} aria-label={item.label} onchange={event=>resize(item.key,event.currentTarget)}/></label>{/each}</div>
  {#each layout.axes as axis,index (axis.portId)}<div class="axis-settings"><span>Axis {index+1}</span><label>Range<select value={axis.range} disabled={disabled} aria-label={`Axis ${index+1} range`} onchange={event=>editAxis(index,event.currentTarget)}><option value="bipolar">−32768…32767</option><option value="unipolar">0…32767</option></select></label></div>{/each}
  {#if layoutError}<p class="layout-error" role="alert">{layoutError}</p>{/if}
  <section class="live-section" aria-label="Interactive debug controls"><div class="live-heading"><h3>Live controls</h3><span class:active={controlsActive}>{controlsActive?'Active':'Inactive'}</span></div><!-- The inactive hint and transport errors share one status line. -->{#if liveError&&active}<p class="live-message" class:live-error={liveError!==applyHint} role="status">{liveError}</p>{/if}<DebugControllerControls {layout} active={controlsActive} {disabled} {values} change={send} onInteract={value=>interacting=value}/></section>
</section>

<style>
  .debug-editor{display:flex;flex-direction:column;min-width:0;min-height:0;gap:10px}.debug-editor label{display:grid;gap:6px;color:var(--muted);font-size:var(--ui-text-size)}input:not([type="checkbox"]),select{width:100%;min-width:0;min-height:var(--ui-control-height);box-sizing:border-box;padding:7px 8px;border:1px solid var(--line);border-radius:5px;background:var(--input-bg);color:var(--text);font-size:var(--ui-text-size)}.count-grid{display:grid;grid-template-columns:repeat(3,minmax(0,1fr));gap:8px}.axis-settings{display:grid;grid-template-columns:auto minmax(0,1fr);gap:8px;align-items:center}.axis-settings>span{color:var(--soft);font-size:var(--ui-text-size)}.axis-settings label{margin:3px 0}.live-message{margin:0;color:var(--muted);font-size:var(--ui-text-size);line-height:var(--ui-line-height)}.layout-error,.live-error{margin:0;color:var(--danger,#e67979);font-size:var(--ui-text-size);line-height:var(--ui-line-height)}.live-section{display:grid;gap:10px;margin-top:4px;padding-top:12px;border-top:1px solid var(--line);min-height:0}.live-heading{display:flex;align-items:center;justify-content:space-between;gap:8px}.live-heading h3{margin:0;font-size:var(--ui-heading-size);font-weight:570;color:var(--soft)}.live-heading span{font-size:var(--ui-text-size);color:var(--muted)}.live-heading span.active{color:var(--accent)}
</style>
