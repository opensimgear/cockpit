<script lang="ts">
  import {onMount} from 'svelte';
  import {invoke,isTauri} from '@tauri-apps/api/core';
  import Icon from '../Icon.svelte';
  import {SharedObservations} from '../streams/shared';
  import {visibleObservation} from '../visible-observation';
  type Backup={id:string;createdAt:number;manifestHash:string};
  type Status={revision:number;epoch:string;backups:Backup[];maximumBackups:number;pendingRestore:string|null;restartRequired:boolean};
  let status=$state<Status|null>(null),busy=$state(false),error=$state(''),message=$state('');
  let selected=$state<Backup|null>(null),action=$state<'restore'|'remove'>('restore');
  let phase=$state('idle'),createId=$state<string|null>(null),cancelRequested=$state<string|null>(null);
  let disposed=false,viewEpoch=0;
  let restoreQueued=$state(false);
  const storageBusy=$derived(['creating','removing','cleaning-up'].includes(phase));
  const native=isTauri();
  const date=(value:number)=>new Date(value).toLocaleString();
  async function request(command:string,fields:Record<string,unknown>={}){
    const reply=await invoke<{ok:boolean;error?:string;state?:{storage:unknown}}>('gateway_request',{request:{command,clientId:'storage-settings-ui',...fields}});
    if(!reply.ok)throw Error(reply.error??'Storage operation failed.');
    return reply.state?.storage;
  }
  function accept(next:Status){
    if(!status||next.epoch!==status.epoch||next.revision>status.revision
      ||next.revision===status.revision&&(!status.restartRequired||next.restartRequired))status=next;
  }
  function currentView(epoch:number){return !disposed&&!document.hidden&&epoch===viewEpoch;}
  function retireView(){
    viewEpoch++;status=null;phase='idle';createId=null;cancelRequested=null;selected=null;error='';message='';
  }
  async function run(command:(current:()=>boolean)=>Promise<void>){
    const epoch=viewEpoch;
    const current=()=>currentView(epoch);
    if(!native||busy||!current())return;
    busy=true;error='';
    try{await command(current);}
    catch(reason){if(current())error=String(reason);}
    finally{busy=false;}
  }
  async function readStatus(current:()=>boolean){
    const next=await request('storage_status') as Status;
    if(current())accept(next);
  }
  function refresh(){return run(readStatus);}
  function create(){
    if(!status||storageBusy||restoreQueued||status.restartRequired)return;
    const expectedRevision=status.revision;
    return run(async current=>{
      message='';
      try{
        const result=await request('storage_create',{expectedRevision}) as {status:Status};
        if(current()){accept(result.status);message='Backup saved and verified.';}
      }catch(reason){
        if(String(reason)!=='Error: Backup creation was cancelled.')throw reason;
        if(current())message='Backup creation cancelled.';
      }
    });
  }
  function confirm(){
    if(!status||!selected||storageBusy||restoreQueued||status.restartRequired)return;
    const id=selected.id,expectedRevision=status.revision,selectedAction=action;
    return run(async current=>{
      message='';
      if(selectedAction==='restore'){
        await request('storage_restore',{id,expectedRevision,confirmed:true});
        // The successful restore command stops the observation broker.
        if(!disposed){restoreQueued=true;error='';}
      }else{
        const result=await request('storage_remove',{id,expectedRevision}) as {status:Status;filesRemoved:boolean};
        if(current()){
          accept(result.status);
          message=result.filesRemoved?'Backup removed.':'Backup retired. Some files remain in its private export folder.';
        }
      }
      if(current())selected=null;
    });
  }
  async function cancelCreate(){
    const epoch=viewEpoch;
    if(!currentView(epoch)||!createId||cancelRequested===createId)return;
    const id=createId;
    cancelRequested=id;
    error='';
    try{await request('storage_cancel_create',{createId:id});}
    catch(reason){if(currentView(epoch)&&createId===id){cancelRequested=null;error=String(reason);}}
  }
  function restart(){return run(async current=>{
    await request('storage_restart');
    if(current())message='Restarting Cockpit…';
  });}
  function show(dialog:HTMLDialogElement){dialog.showModal();return()=>dialog.close();}
  onMount(()=>{
    if(!native)return;
    const observations=new SharedObservations();
    let received=false;
    const reader=visibleObservation(observations,'storage.backups',state=>{
      if(state.status==='ready'){
        received=true;
        const payload=state.frame?.payload as {status?:Status|null;phase?:string;createId?:string};
        if(payload?.status) accept(payload.status);
        phase=payload?.phase??'idle';
        createId=payload?.createId??null;
        if(phase==='cancelled')message='Backup creation cancelled.';
      }else if(state.status==='error'||state.status==='lost'){
        phase='idle';
        createId=null;
        if(!status?.restartRequired&&!restoreQueued)error=state.error??'Backup observation is unavailable.';
        if(!received){
          received=true;
          const epoch=viewEpoch;
          const current=()=>currentView(epoch);
          // A stopped runtime can still report a queued restore.
          void readStatus(current).then(()=>{
            if(current()&&status?.restartRequired)error='';
          },()=>{});
        }
      }
    },()=>{received=false;retireView();});
    reader.setVisible(true);
    return ()=>{disposed=true;reader.close();observations.close();};
  });
</script>

<section class="panel preferences-panel backup-settings" aria-label="Backups">
  <div class="panel-heading"><h2>Backups</h2><button class="secondary-button" disabled={!native||busy||storageBusy||!status||restoreQueued||status.restartRequired||status.backups.length>=status.maximumBackups} onclick={create}><Icon name="plus" size={14}/>Create</button></div>
  {#if storageBusy}<p role="status">{phase==='creating'?'Creating backup…':phase==='removing'?'Retiring backup…':'Removing backup files…'}</p>{/if}
  {#if createId}<button class="secondary-button" disabled={cancelRequested===createId} onclick={cancelCreate}><Icon name="close" size={14}/>{cancelRequested===createId?'Cancelling…':'Cancel'}</button>{/if}
  <div class="preference-row"><div><strong>Saved settings and content</strong><p>Each backup includes a consistent database snapshot and verified managed assets. Protection keys stay with your operating system. Keep this computer’s backups private.</p></div><button class="secondary-button" disabled={!native||busy} onclick={refresh}><Icon name="refresh" size={14}/>Refresh</button></div>
  {#if !native}<div class="preference-row"><p>Open the desktop Cockpit to manage backups.</p></div>{/if}
  {#if restoreQueued||status?.restartRequired}<div class="preference-row" role="status"><div><strong>Restore ready</strong><p>Native workers are stopped. Restart to replace saved settings and content. Pair companions again and issue new AI credentials afterward.</p></div><button class="primary-button" disabled={busy} onclick={restart}><Icon name="refresh" size={14}/>Restart</button></div>{/if}
  {#if status}
    {#each status.backups as backup (backup.id)}<div class="preference-row backup-row" data-backup-id={backup.id}><div><strong>{date(backup.createdAt)}</strong><p>{backup.id}</p></div><div class="actions"><button class="secondary-button" disabled={busy||storageBusy||restoreQueued||status.restartRequired} onclick={()=>{selected=backup;action='restore';}}><Icon name="refresh" size={14}/>Restore</button><button class="secondary-button" aria-label={'Remove backup '+backup.id} disabled={busy||storageBusy||restoreQueued||status.restartRequired} onclick={()=>{selected=backup;action='remove';}}><Icon name="trash" size={14}/>Remove</button></div></div>{/each}
    {#if !status.backups.length}<div class="preference-row"><p>No backups yet.</p></div>{/if}
  {/if}
  {#if message}<div class="preference-row"><p role="status">{message}</p></div>{/if}
  {#if error}<div class="preference-row"><p class="storage-error" role="alert">{error}</p></div>{/if}
</section>
{#if selected}<dialog aria-labelledby="backup-confirm-title" oncancel={event=>{event.preventDefault();if(!busy)selected=null;}} {@attach show}>
  <h2 id="backup-confirm-title">{action==='restore'?'Restore saved settings?':'Remove this backup?'}</h2>
  <p>{action==='restore'?'This replaces your current saved settings and content after a restart. Stop the profile and complete recovery first. A rollback snapshot is saved before replacement. All companion pairings and AI credentials will need to be issued again. Historical recovery records will stay inactive.':'This retires the saved backup and removes its private export files.'}</p>
  <p>{date(selected.createdAt)}</p>{#if error}<p class="storage-error" role="alert">{error}</p>{/if}
  <div class="actions"><button class="secondary-button" disabled={busy} onclick={()=>selected=null}><Icon name="close" size={14}/>Cancel</button><button class="primary-button" disabled={busy} onclick={confirm}><Icon name={action==='restore'?'refresh':'trash'} size={14}/>{busy?'Working…':action==='restore'?'Restore':'Remove'}</button></div>
</dialog>{/if}
<style>
  .backup-settings { margin-top: var(--ui-section-gap); }
  .actions{display:flex;gap:8px;align-items:center;flex-wrap:wrap}.backup-row p{overflow-wrap:anywhere}dialog{position:fixed;inset:0;margin:auto;width:min(520px,calc(100% - 32px));box-sizing:border-box;padding:var(--ui-panel-padding);background:var(--surface);color:var(--text);border:1px solid var(--line);border-radius:10px;box-shadow:0 20px 80px #0005}dialog::backdrop{background:#0009}dialog h2{font-size:var(--ui-section-title-size);margin:0}dialog p{color:var(--muted);font-size: var(--ui-text-size);line-height: var(--ui-line-height);margin:16px 0}dialog .actions{justify-content:flex-end}.storage-error{color:var(--danger,#e5b19b)}
</style>
