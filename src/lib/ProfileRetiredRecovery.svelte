<script lang="ts">
 import{onDestroy,onMount,untrack}from'svelte';
 import{gatewayRequest}from'./gateway';
 import{RetiredRecoveryClient,RetiredRecoveryController,needsRetiredRecovery,type RetiredJournalRef,type RetiredProvisionRef}from'./profile-retired-recovery';
 import type{DeepReadonly}from'./profiles';
 let{active=true,controller:supplied}:{active?:boolean;controller?:RetiredRecoveryController}=$props();
 const provided=untrack(()=>supplied),controller=provided??new RetiredRecoveryController(new RetiredRecoveryClient(async request=>(await gatewayRequest(request)).retiredRecovery));
 let view=$state.raw(controller.state),mounted=$state(false),acknowledged=$state<string[]>([]);
 const visible=$derived(needsRetiredRecovery(view.status)||!!view.error);
 const blocked=$derived(!active||view.stale||!!view.pending);
 const refKey=(ref:DeepReadonly<RetiredJournalRef>)=>JSON.stringify(ref);
 function confirm(ref:DeepReadonly<RetiredJournalRef>,checked:boolean){const key=refKey(ref);acknowledged=checked?[...acknowledged.filter(v=>v!==key),key]:acknowledged.filter(v=>v!==key);}
 async function output(ref:DeepReadonly<RetiredJournalRef>,acknowledge=false){if(blocked||!view.status?.outputQualified||acknowledge&&!acknowledged.includes(refKey(ref)))return;await controller.output(ref,acknowledge);acknowledged=[];}
 async function recoverProvision(ref:DeepReadonly<RetiredProvisionRef>){if(blocked||!view.status?.provisioningQualified)return;await controller.recoverProvision(ref);}
 function mountStatus(){mounted=true;const stop=controller.subscribe(next=>view=next);return()=>{mounted=false;stop();};}
 onMount(mountStatus);onDestroy(()=>{if(!provided)controller.dispose();});
 function watchStatus(){if(!mounted||!active)return;const refresh=()=>{if(!document.hidden)void controller.refresh();};refresh();document.addEventListener('visibilitychange',refresh);const timer=window.setInterval(refresh,1000);return()=>{window.clearInterval(timer);document.removeEventListener('visibilitychange',refresh);};}
 $effect(watchStatus);
</script>
{#if visible}
 <section class="retired-recovery panel" aria-label="Previous output setup recovery">
  <div class="recovery-heading"><div><h2>Previous output setup recovery</h2><p>These retained ownership records belong to the previous mapping system. Cleanup cannot create or apply a profile.</p></div><button class="text-button" disabled={view.loading||!!view.pending} onclick={()=>controller.refresh()}>Refresh cleanup records</button></div>
  {#if view.stale}<p class="recovery-warning" role="status">Native cleanup status is unavailable or changing. Refresh before choosing a record.</p>{/if}
  {#each view.status?.diagnostics??[] as message,index(index)}<p class="recovery-warning" role="status">{message}</p>{/each}
  {#each view.status?.outputs??[] as record(refKey(record.reference))}
   <article><h3>{record.reference.id.kind==='manual'?'Previous manual output':'Previous companion output'}</h3><p>Recorded slots: {record.slots.join(', ')} · {record.phase}</p><p class="record-id">Owner {record.reference.owner.session} · generation {record.reference.owner.generation}</p>
    {#if record.phase==='confirmed'}<button class="secondary-button" disabled={blocked||!view.status?.outputQualified} onclick={()=>output(record.reference)}>Recover recorded output</button>
    {:else}<p>Native ownership was not confirmed. Acknowledgement removes only this exact unchanged journal and sends no output.</p><label><input type="checkbox" checked={acknowledged.includes(refKey(record.reference))} disabled={blocked||!view.status?.outputQualified} onchange={event=>confirm(record.reference,event.currentTarget.checked)}/> I confirm this uncertain record should be acknowledged without sending output.</label><button class="secondary-button" disabled={blocked||!view.status?.outputQualified||!acknowledged.includes(refKey(record.reference))} onclick={()=>output(record.reference,true)}>Acknowledge without output</button>{/if}
   </article>
  {/each}
  {#if view.status?.outputs.length&&!view.status.outputQualified}<p class="recovery-warning">Native output recovery is unavailable until platform qualification is complete. The ownership records remain retained.</p>{/if}
  {#each view.status?.provisioning??[] as record(record.operation)}<article><h3>Previous virtual-controller setup</h3><p>Target slot {record.target}{record.refreshUncertain?' · previous refresh outcome is uncertain':''}</p><p class="record-id">Operation {record.operation}</p><button class="secondary-button" disabled={blocked||!view.status?.provisioningQualified} onclick={()=>recoverProvision(record)}>Recover recorded setup</button></article>{/each}
  {#if view.status?.provisioning.length&&!view.status.provisioningQualified}<p class="recovery-warning">Native setup recovery is unavailable until platform qualification is complete.</p>{/if}
  {#if view.status?.job}<article><h3>Native cleanup job · {view.status.job.state}</h3><p role="status">{view.status.job.message}</p>{#if view.status.job.active}<button class="secondary-button" disabled={!active||view.pending==='cancel'} onclick={()=>controller.cancel()}>Cancel cleanup job</button>{/if}</article>{/if}
  {#if view.pending}<p role="status">Waiting for native cleanup…</p>{/if}{#if view.error}<p class="recovery-warning" role="status">{view.error}</p>{/if}
 </section>
{/if}
<style>
 .retired-recovery{padding:20px;margin:20px 0}.recovery-heading{display:flex;justify-content:space-between;gap:18px;align-items:start}.retired-recovery h2{font-size:13px}.retired-recovery h3{font-size:11px}.retired-recovery p,.retired-recovery label{font-size:11px;line-height:1.8;color:var(--muted);margin:8px 0}.retired-recovery article{padding:16px 0;border-top:1px solid var(--line);margin-top:16px}.record-id{overflow-wrap:anywhere}.retired-recovery label{display:flex;align-items:start;gap:8px}.retired-recovery button{font-size:10px}.retired-recovery input{margin-top:5px}.recovery-warning{color:#c9ab7b!important}
</style>
