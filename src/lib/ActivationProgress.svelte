<script lang="ts">
  import {onMount} from 'svelte';
  import {gatewayRequest,subscribeProfileCleanup} from './gateway';
  import {nativeProfileActivation,type NativeProfileActivation} from './profile-runtime';
  import type {DeepReadonly} from './profiles';
  import {observeProfileActivation} from './profile-activation-observation';
  import {NativeProfileActivationWindow,type ActivationWindowProgress} from './profile-activation-window';
  import {observeActivationWindow} from './activation-window-observation';
  import {ActivationProgressPresenter,ApplicationProfileProgress} from './activation-progress-presenter';
  let fallback=$state<ActivationWindowProgress|null>(null);
  async function cancel(activationId:string){
    try{return !!(await gatewayRequest({command:'profile_activation_cancel',activationId})).profileActivation?.cancelRequested;}
    catch{return false;}
  }
  function show(node:HTMLDialogElement){node.showModal();return()=>node.close();}

  // One application-wide owner also sees native automatic selection and game
  // launch while the Profiles page has never been opened.
  onMount(()=>{
    let applying=false,disposed=false;
    let pending:DeepReadonly<NativeProfileActivation>|null|undefined;
    const presenter=new ActivationProgressPresenter(new NativeProfileActivationWindow({observe:observeActivationWindow}),cancel,progress=>fallback=progress);
    const progress=new ApplicationProfileProgress(presenter,async activationId=>{
      const result=(await gatewayRequest({command:'profile_activation_status',activationId})).profileActivation;
      return result?nativeProfileActivation(result,activationId):null;
    });
    const unsubscribe=subscribeProfileCleanup(event=>{void progress.cleanup(event);});
    const apply=async()=>{
      if(applying||disposed)return;
      applying=true;
      try{
        while(!disposed&&pending!==undefined){
          const next=pending;
          pending=undefined;
          await progress.sync(next);
        }
      }finally{applying=false;}
    };
    const stop=observeProfileActivation(next=>{
      pending=next;
      void apply().catch(()=>{});
    });
    return()=>{disposed=true;stop();unsubscribe();progress.dispose();};
  });
</script>

{#if fallback}<dialog aria-label="Profile operation progress" oncancel={event=>{event.preventDefault();if(fallback?.canCancel)void cancel(fallback.operationId);}} {@attach show}>
  <p role="status">{fallback.message}</p>{#if fallback.canCancel || fallback.phase==='cancelling'}<button class="secondary-button" disabled={!fallback.canCancel} onclick={()=>fallback?.canCancel&&cancel(fallback.operationId)}>{fallback.phase==='cancelling'?'Cancelling…':'Cancel'}</button>{/if}
</dialog>{/if}
<style>
  dialog{position:fixed;left:50%;top:50%;transform:translate(-50%,-50%);margin:0;display:flex;align-items:center;gap:18px;width:600px;max-width:calc(100vw - 40px);padding:16px 18px;box-sizing:border-box;background:var(--surface);color:var(--text);border:1px solid var(--line);border-radius:6px}dialog::backdrop{background:#0005}p{flex:1;min-width:0;font-size:var(--ui-text-size);line-height:20px;overflow-wrap:anywhere}button{flex-shrink:0}
</style>
