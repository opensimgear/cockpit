<script lang="ts">
  import {onMount} from 'svelte';
  import {invoke,isTauri} from '@tauri-apps/api/core';
  import Icon from '../Icon.svelte';
  type Backup={id:string;createdAt:number;manifestHash:string};
  type Status={revision:number;epoch:string;backups:Backup[];maximumBackups:number;pendingRestore:string|null;restartRequired:boolean};
  let status=$state<Status|null>(null),busy=$state(false),error=$state(''),message=$state('');
  let selected=$state<Backup|null>(null),action=$state<'restore'|'remove'>('restore');
  const native=isTauri();
  const date=(value:number)=>new Date(value).toLocaleString();
  async function request(command:string,fields:Record<string,unknown>={}){
    const reply=await invoke<{ok:boolean;error?:string;state?:{storage:unknown}}>('gateway_request',{request:{command,clientId:'storage-settings-ui',...fields}});
    if(!reply.ok)throw Error(reply.error??'Storage operation failed.');
    return reply.state?.storage;
  }
  async function refresh(){if(!native)return;busy=true;error='';try{status=await request('storage_status') as Status;}catch(reason){error=String(reason);}finally{busy=false;}}
  async function create(){if(!status||busy)return;busy=true;error='';message='';try{const result=await request('storage_create',{expectedRevision:status.revision}) as {status:Status};status=result.status;message='Backup saved and verified.';}catch(reason){error=String(reason);}finally{busy=false;}}
  async function confirm(){if(!status||!selected||busy)return;busy=true;error='';message='';try{if(action==='restore'){await request('storage_restore',{id:selected.id,expectedRevision:status.revision,confirmed:true});status={...status,pendingRestore:selected.id,restartRequired:true};message='Runtime stopped. Restart to finish restoring this backup.';}else{const result=await request('storage_remove',{id:selected.id,expectedRevision:status.revision}) as {status:Status;filesRemoved:boolean};status=result.status;message=result.filesRemoved?'Backup removed.':'Backup retired. Some files remain in its private export folder.';}selected=null;}catch(reason){error=String(reason);}finally{busy=false;}}
  async function restart(){busy=true;error='';try{await request('storage_restart');message='Restarting Cockpit…';}catch(reason){error=String(reason);busy=false;}}
  function show(dialog:HTMLDialogElement){dialog.showModal();return()=>dialog.close();}
  onMount(()=>{void refresh();});
</script>

<section class="panel preferences-panel backup-settings" aria-label="Backups">
  <div class="panel-heading"><h2>Backups</h2><button class="secondary-button" disabled={!native||busy||!status||status.restartRequired||status.backups.length>=status.maximumBackups} onclick={create}><Icon name="plus" size={14}/>Create</button></div>
  <div class="preference-row"><div><strong>Saved settings and content</strong><p>Each backup includes a consistent database snapshot and verified managed assets. Protection keys stay with your operating system. Keep this computer’s backups private.</p></div><button class="secondary-button" disabled={!native||busy} onclick={refresh}><Icon name="refresh" size={14}/>Refresh</button></div>
  {#if !native}<div class="preference-row"><p>Open the desktop Cockpit to manage backups.</p></div>{/if}
  {#if status?.restartRequired}<div class="preference-row" role="status"><div><strong>Restore ready</strong><p>Native workers are stopped. Restart to replace saved settings and content. Pair companions again and issue new AI credentials afterward.</p></div><button class="primary-button" disabled={busy} onclick={restart}><Icon name="refresh" size={14}/>Restart</button></div>{/if}
  {#if status}
    {#each status.backups as backup (backup.id)}<div class="preference-row backup-row" data-backup-id={backup.id}><div><strong>{date(backup.createdAt)}</strong><p>{backup.id}</p></div><div class="actions"><button class="secondary-button" disabled={busy||status.restartRequired} onclick={()=>{selected=backup;action='restore';}}><Icon name="refresh" size={14}/>Restore</button><button class="secondary-button" aria-label={'Remove backup '+backup.id} disabled={busy||status.restartRequired} onclick={()=>{selected=backup;action='remove';}}><Icon name="trash" size={14}/>Remove</button></div></div>{/each}
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
  .actions{display:flex;gap:8px;align-items:center}.backup-row p{overflow-wrap:anywhere}dialog{position:fixed;inset:0;margin:auto;width:min(520px,calc(100% - 32px));box-sizing:border-box;padding:24px;background:var(--surface);color:var(--text);border:1px solid var(--line);border-radius:10px;box-shadow:0 20px 80px #0005}dialog::backdrop{background:#0009}dialog h2{font-size:18px;margin:0}dialog p{color:var(--muted);font-size:12px;line-height:1.6;margin:16px 0}dialog .actions{justify-content:flex-end}.storage-error{color:var(--danger,#e5b19b)}
</style>
