<script lang="ts">
  import {onMount,untrack} from 'svelte';
  import type {DeepReadonly} from './profiles';
  import type {NativeProfileOwner} from './profile-runtime';
  import type {ProfileInspectedPort} from './profile-inspection';
  import {ProfileInspectionController} from './profile-inspection-controller';
  let {controller,owner=null,active=true,selectedId='',editorProfileId,editorRevision,editorDirty=false,profileName}: {
    controller:ProfileInspectionController;owner?:DeepReadonly<NativeProfileOwner>|null;active?:boolean;selectedId?:string;
    editorProfileId?:string;editorRevision?:number;editorDirty?:boolean;profileName?:string;
  }=$props();
  const inspector=untrack(()=>controller);
  let view=$state.raw(inspector.state),mounted=$state(false),followSelection=$state(true);
  const ownerKey=$derived(owner?JSON.stringify([owner.session,owner.generation,owner.profileId,owner.profileRevision,owner.gameId]):'');
  const nodeId=$derived(followSelection&&owner?.profileId===editorProfileId&&selectedId?selectedId:undefined);
  const differentRevision=$derived(!!owner&&(editorProfileId!==owner.profileId||editorRevision!==owner.profileRevision||editorDirty));
  onMount(()=>{mounted=true;const stop=inspector.subscribe(next=>view=next);return()=>{mounted=false;stop();};});
  function watchInspection(){
    const key=ownerKey,filter=nodeId;
    if(!mounted||!active||!key){inspector.clear();return;}
    const captured=untrack(()=>owner)!;
    const refresh=()=>{if(!document.hidden){if(!inspector.state.owner)void inspector.open(captured,filter);else void inspector.refresh();}};
    refresh();const timer=window.setInterval(refresh,1000);document.addEventListener('visibilitychange',refresh);
    return()=>{window.clearInterval(timer);document.removeEventListener('visibilitychange',refresh);inspector.clear();};
  }
  $effect(watchInspection);
  function value(port:DeepReadonly<ProfileInspectedPort>):string{
    if(port.sample.status!=='known')return port.sample.status;
    if(port.sample.value.kind==='action-pack')return port.sample.value.value.arguments.map(argument=>`${argument.id} = ${JSON.stringify(argument.value)}`).join(', ')||'No arguments';
    return JSON.stringify(port.sample.value.value);
  }
  function contract(port:DeepReadonly<ProfileInspectedPort>):string{
    const contract=port.contract;
    if(contract.delivery==='state'||contract.delivery==='event')return `${contract.delivery} · ${contract.schema.type}${'unit' in contract.schema&&contract.schema.unit?` · ${contract.schema.unit}`:''}`;
    return `${contract.delivery} · ${contract.semantics} · ${contract.parameters.length} ordered arguments`;
  }
</script>

<section class="profile-live panel" aria-label="Applied profile port inspection">
  <div class="live-heading"><div><strong>Applied port samples</strong><p>{#if owner}{profileName??owner.profileId} · applied revision {owner.profileRevision} · run {owner.generation}{:else}No current native run is available to inspect.{/if}</p></div>{#if owner}<button class="text-button" disabled={view.loading} onclick={()=>inspector.refresh()}>{view.loading?'Reading…':'Refresh samples'}</button>{/if}</div>
  {#if owner}
    <p class="live-help">Read-only graph samples from the applied run. Inputs and action tuples show upstream/requested values; they do not confirm that a device or simulator accepted an output. Event values are the last observed sample.</p>
    {#if differentRevision}<p class="live-notice">The editor differs from this applied revision. These samples describe the running graph; current draft edits are excluded.</p>{/if}
    {#if selectedId&&editorProfileId===owner.profileId}<label class="follow-selection"><input type="checkbox" bind:checked={followSelection}/> Follow selected block <code>{selectedId}</code></label>{/if}
    <p class="sample-capture" role="status">{#if view.page}{view.stale?'Last capture · stale':'Native capture'} {view.page.capturedUs} μs · {nodeId?`block ${nodeId}`:'all declared applied ports'}{:else if view.loading}Waiting for the native applied capture…{:else}No port capture loaded.{/if}</p>
    {#if view.error}<p class="live-error" role="status">{view.error}</p>{/if}
    {#if view.page}<div class="sample-table"><table aria-label="Native applied port samples"><thead><tr><th scope="col">Exact graph port</th><th scope="col">Contract</th><th scope="col">Native sample</th><th scope="col">Observed at</th></tr></thead><tbody>{#each view.page.ports as port (JSON.stringify([port.endpoint.nodeId,port.endpoint.portId]))}<tr class:stale={view.stale}><th scope="row"><code>{port.endpoint.nodeId} / {port.endpoint.portId}</code><small>{port.direction==='input'?'Input · upstream/requested':'Output · graph producer'}</small></th><td>{contract(port)}</td><td><span class:unknown={port.sample.status!=='known'}>{value(port)}</span></td><td>{port.observedUs===null?'Never observed':`${port.observedUs} μs`}</td></tr>{:else}<tr><td colspan="4">No declared ports exist in this applied selection.</td></tr>{/each}</tbody></table></div><div class="sample-pages"><span>{view.page.total?view.page.offset+1:0}–{view.page.offset+view.page.ports.length} of {view.page.total} ports · one native capture per page</span><button class="text-button" disabled={view.loading||view.stale||!view.hasPrevious} onclick={()=>inspector.previous()}>Previous ports</button><button class="text-button" disabled={view.loading||view.stale||!view.hasNext} onclick={()=>inspector.next()}>Next ports</button></div>{/if}
  {:else}<p class="live-help">Apply a saved profile to inspect its native run. Browsing ports never starts acquisition or changes a profile.</p>{/if}
</section>

<style>
  .profile-live{padding:17px;margin-bottom:22px;}.live-heading{display:flex;align-items:start;justify-content:space-between;gap:14px;}.live-heading strong{font-size:12px;}.live-heading p,.live-help,.sample-capture,.follow-selection,.sample-pages{font-size:9px;color:var(--muted);line-height:1.8;}.live-heading p{margin:7px 0 0;overflow-wrap:anywhere;}.profile-live button{font-size:9px;}.live-help{margin:12px 0;}.live-notice,.live-error{font-size:10px;line-height:1.8;border:1px solid var(--line);border-radius:5px;padding:10px 12px;color:var(--soft);}.live-error{color:#c9ab7b;border-color:#ad875440;}.follow-selection{display:flex;gap:8px;align-items:center;margin:12px 0;}.sample-table{overflow:auto;max-height:360px;}.sample-table table{border-collapse:collapse;min-width:590px;width:100%;font-size:10px;text-align:left;}.sample-table th,.sample-table td{border-bottom:1px solid var(--line);padding:11px 9px;vertical-align:top;overflow-wrap:anywhere;}.sample-table thead th{font-size:9px;color:var(--muted);font-weight:400;}.sample-table tbody th{font-weight:400;}.sample-table code{display:block;}.sample-table small{display:block;font-size:8px;color:var(--muted);margin-top:5px;}.sample-table tr.stale,.sample-table .unknown{color:var(--muted);}.sample-pages{display:flex;gap:13px;align-items:center;justify-content:end;flex-wrap:wrap;margin-top:13px;}.sample-pages>span{margin-right:auto;}
</style>
