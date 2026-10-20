<script lang="ts">
  import {onMount} from 'svelte';
  import {gatewayRequest} from './gateway';
  import {nativeProfileActivation} from './profile-runtime';
  import {NativeProfileActivationWindow,type ActivationWindowProgress} from './profile-activation-window';
  import {ActivationProgressPresenter} from './activation-progress-presenter';
  let fallback=$state<ActivationWindowProgress|null>(null);
  async function cancel(activationId:string){
    try{return !!(await gatewayRequest({command:'profile_activation_cancel',activationId})).profileActivation?.cancelRequested;}
    catch{return false;}
  }
  function show(node:HTMLDialogElement){node.showModal();return()=>node.close();}

  // One application-wide owner also sees native automatic selection and game
  // launch while the Profiles page has never been opened.
  onMount(()=>{
    if(!('__TAURI_INTERNALS__' in window))return;
    let reading=false,disposed=false;
    const presenter=new ActivationProgressPresenter(new NativeProfileActivationWindow(),cancel,progress=>fallback=progress);
    const refresh=async()=>{
      if(reading||disposed)return;reading=true;
      try{
        const result=(await gatewayRequest({command:'profile_activation_current'})).profileActivation;
        if(!disposed)await presenter.sync(result?nativeProfileActivation(result,result.activationId):null);
      }catch{/* Retain the current operation across a transient progress read failure. */}
      finally{reading=false;}
    };
    void refresh();const timer=window.setInterval(()=>{void refresh();},150);
    return()=>{disposed=true;window.clearInterval(timer);presenter.dispose();};
  });
</script>

{#if fallback}<dialog aria-label="Profile activation progress" oncancel={event=>{event.preventDefault();if(fallback?.canCancel)void cancel(fallback.operationId);}} {@attach show}>
  <p role="status">{fallback.message}</p><button class="secondary-button" disabled={!fallback.canCancel} onclick={()=>fallback&&cancel(fallback.operationId)}>{fallback.phase==='cancelling'?'Cancelling…':'Cancel'}</button>
</dialog>{/if}
<style>
  dialog{position:fixed;left:50%;top:50%;transform:translate(-50%,-50%);margin:0;display:flex;align-items:center;gap:18px;width:600px;max-width:calc(100vw - 40px);padding:16px 18px;box-sizing:border-box;background:var(--surface);color:var(--text);border:1px solid var(--line);border-radius:6px}dialog::backdrop{background:#0005}p{flex:1;min-width:0;font-size:13px;line-height:20px;overflow-wrap:anywhere}button{flex-shrink:0}
</style>
