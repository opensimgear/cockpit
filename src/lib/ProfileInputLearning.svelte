<script lang="ts">
  import{onMount,untrack}from'svelte';
  import type{DeepReadonly}from'./profiles';
  import type{ObservedResourcePort,ResourceSummary}from'./profile-inventory';
  import{ProfileLearningController}from'./profile-learning-controller';
  let{controller,resource,context,active=true,disabled=false,runtimeIdle=false,choose}:{controller:ProfileLearningController;resource:DeepReadonly<ResourceSummary>;context:string;active?:boolean;disabled?:boolean;runtimeIdle?:boolean;choose:(candidate:DeepReadonly<ObservedResourcePort>)=>void}=$props();
  const learner=untrack(()=>controller);
  let view=$state.raw(learner.state),mounted=$state(false),notice=$state('');
  const captureContext=$derived(JSON.stringify([context,resource.id,resource.generation,resource.capabilityRevision,resource.availability,resource.connections.map(connection=>[connection.scope,connection.key,connection.route.id,connection.route.providerGeneration,connection.route.baselineReady])]));
  const physical=$derived(resource.resourceType==='host-controller'&&resource.availability==='online'&&resource.connections.length===1&&resource.connections[0].route.transport==='hid'&&resource.connections[0].route.baselineReady);
  const blocked=$derived(disabled||!active||!runtimeIdle||!physical);
  const pollKey=$derived(view.status&&view.status.phase!=='stopped'?view.status.learnId:'');
  function mountLearning(){mounted=true;const stop=learner.subscribe(next=>view=next);return()=>{mounted=false;stop();learner.clear();};}
  onMount(mountLearning);
  function syncContext(){learner.setContext(active&&runtimeIdle?captureContext:'inactive-or-owned');}
  $effect(syncContext);
  function watchLearning(){if(!mounted||!active||!runtimeIdle||!pollKey)return;const refresh=()=>{if(!document.hidden)void learner.refresh();};refresh();document.addEventListener('visibilitychange',refresh);const timer=window.setInterval(refresh,400);return()=>{window.clearInterval(timer);document.removeEventListener('visibilitychange',refresh);};}
  $effect(watchLearning);
  async function start(){if(blocked||view.pending)return;notice='';await learner.start({resourceId:resource.id,generation:resource.generation,capabilityRevision:resource.capabilityRevision});}
  async function cancel(){notice='';await learner.cancel();}
  async function select(port:DeepReadonly<ObservedResourcePort>){if(blocked||view.pending)return;const captured=captureContext;notice='';try{const candidate=await learner.choose(port.descriptor.id,port.contractHash,resource);if(captured!==captureContext||blocked)return;choose(candidate);notice='Added the selected input port to this draft. Connect an exact route and Save explicitly.';}catch(error){notice=error instanceof Error?error.message:String(error);}}
</script>

<section class="input-learning" aria-label="Learn physical input without output">
  <h3>Learn physical input</h3><p>Move one control deliberately within 30 seconds. Native capture sends no output and changes no profile. Choose the intended candidate explicitly; simultaneous changes can produce several candidates.</p>
  {#if !runtimeIdle}<p class="learning-notice">Deactivate and recover active outputs before learning. Native also checks pending game, mapping, virtual-controller and visibility ownership.</p>{:else if !physical}<p class="learning-notice">Learning requires a current physical HID controller with a complete native baseline. Use the exact port catalogue for other resource types.</p>{/if}
  <div class="learning-actions"><button class="secondary-button" disabled={blocked||!!view.pending||view.status?.phase==='listening'} onclick={start}>{view.pending==='start'?'Starting native capture…':'Learn input without output'}</button><button class="text-button" disabled={view.pending==='cancel'||!view.pending&&(!view.status||view.status.phase==='stopped')} onclick={cancel}>{view.pending==='cancel'?'Cancelling…':'Cancel learning'}</button></div>
  {#if view.status}<p class="learning-state" role="status">{view.status.phase}{view.stale?' · last capture is stale':''}. {view.status.reason}</p>{#if view.status.phase==='learned'}<ul aria-label="Explicit learned input candidates">{#each view.status.candidates as port (JSON.stringify([port.descriptor.id,port.contractHash]))}<li><div><strong>{port.descriptor.name}</strong><code>{port.descriptor.id}</code><small>{port.descriptor.contract.delivery} · {port.descriptor.contract.delivery==='state'||port.descriptor.contract.delivery==='event'?port.descriptor.contract.schema.type:''}</small></div><button class="secondary-button" disabled={blocked||view.stale||view.loading||!!view.pending} onclick={()=>select(port)}>Add this input port</button></li>{/each}</ul>{/if}{/if}
  {#if view.error}<p class="learning-error" role="status">{view.error}</p>{/if}
  {#if notice}<p class="learning-notice" role="status">{notice}</p>{/if}
</section>

<style>
  .input-learning{border-top:1px solid var(--line);padding-top:15px;margin-top:16px;}.input-learning h3{font-size:11px;color:var(--soft);margin:0 0 9px;}.input-learning p{font-size:9px;line-height:1.8;color:var(--muted);}.learning-actions{display:flex;gap:10px;flex-wrap:wrap;align-items:center;}.input-learning button{font-size:9px;}.learning-notice,.learning-error{padding:9px;border:1px solid var(--line);border-radius:5px;}.learning-error{color:#c9ab7b!important;border-color:#ad875440;}.input-learning ul{list-style:none;padding:0;margin:13px 0;max-height:320px;overflow:auto;}.input-learning li{padding:11px 0;border-bottom:1px solid var(--line);display:grid;gap:9px;}.input-learning li strong{font-size:10px;color:var(--soft);}.input-learning code,.input-learning small{display:block;font-size:8px;color:var(--muted);margin-top:6px;overflow-wrap:anywhere;}.input-learning li button{justify-self:start;}
</style>
