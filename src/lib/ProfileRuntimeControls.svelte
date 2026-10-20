<script lang="ts">
  import { onMount, untrack } from 'svelte';
  import { ProfileRuntimeController, type RuntimeControlState, type RuntimeEditorTarget } from './profile-runtime-controller';
  import type { ProfileGameId } from './profile-runtime';
  import type { DeepReadonly, ProfileSummary } from './profiles';
  import type {ActivationWindowProgress} from './profile-activation-window';
  let { controller, editor = null, profiles = [], gameIds = [], active = true, onState, compact = false,manageActivationProgress=true }: {
    controller: ProfileRuntimeController; editor?: RuntimeEditorTarget|null; profiles?: readonly DeepReadonly<ProfileSummary>[];
    gameIds?: readonly string[]; active?: boolean; onState?: (state: DeepReadonly<RuntimeControlState>)=>void; compact?:boolean;manageActivationProgress?:boolean;
  } = $props();
  const runtime = untrack(()=>controller);
  let view = $state.raw<DeepReadonly<RuntimeControlState>>(runtime.state);
  let mounted = $state(false), gameId = $state('');
  let nativeOpened=$state(false),nativeResolved=$state(false),elapsed=$state(0);
  const games: {id:ProfileGameId;name:string}[] = [{id:'dcs',name:'DCS World'},{id:'x-plane-11',name:'X-Plane 11'},{id:'x-plane-12',name:'X-Plane 12'},{id:'assetto-corsa-rally',name:'Assetto Corsa Rally'}];
  const choices = $derived(games.filter(game=>!gameIds.length||gameIds.includes(game.id)));
  const native = $derived(view.status);
  const recoveryNeeded=$derived(native?.state==='recovery-needed'||native?.state==='faulted');
  const cancelledRecovery=$derived(recoveryNeeded&&view.progress?.phase==='cancelled'&&view.progress.cancelRequested);
  const runtimeErrors=$derived([...new Set([cancelledRecovery?view.progress?.message:native?.fault,view.error].filter((message):message is string=>!!message).map(message=>message.trim()).filter(Boolean))]);
  const selectedGame = $derived(choices.some(game=>game.id===gameId)?gameId as ProfileGameId:undefined);
  const invalidGame = $derived(gameId!==''&&!selectedGame);
  const saved = $derived(!!editor&&!editor.readOnly&&!editor.dirty&&editor.baseRevision!==null&&editor.baseRevision===editor.revision);
  const applyDisabled = $derived(!saved||invalidGame||!native||view.stale||!!view.pending||!!view.activationConfirmation||native.state==='recovery-needed'||native.state==='transitioning'||native.state==='awaiting-game');
  const appliedName = $derived(profiles.find(profile=>profile.id===native?.appliedSnapshot?.profileId)?.name??native?.appliedSnapshot?.profileId??'None');
  const activeEditor = $derived(!!editor&&native?.owner?.profileId===editor.id);
  const savedChangesPending = $derived(activeEditor&&native?.appliedSnapshot?.revision!==editor?.baseRevision);
  const activating=$derived(view.pending==='apply'&&!!view.activation);
  const activationStarted=$derived(view.activation?.startedAt??0);
  const cancelling=$derived(!!view.activation?.cancelling||view.progress?.phase==='cancelling');
  const targetName=$derived(profiles.find(profile=>profile.id===view.activation?.profileId)?.name??'profile');
  const progressPayload=$derived.by(()=>{
    if(!activating||!view.activation)return null;
    const progress=view.progress,phase=cancelling?'cancelling':progress?.phase==='activating'?'activating':'preparing';
    const details=(progress?.companions??[]).flatMap(companion=>{
      const operation=companion.operation;if(!operation||operation.phase==='ready')return [];
      const labels:Record<string,string>={'opening':'Connecting','checking-cache':'Checking cached images','downloading':'Downloading images','rendering':'Drawing panel','awaiting-neutral':'Release touch controls','failed':'Panel could not load'};
      const completed=operation.completedBytes??0,total=operation.totalBytes??0;
      const download=operation.phase==='downloading'&&total>0?` · ${Math.floor(completed/total*100)}% (${Math.ceil(completed/1024)} / ${Math.ceil(total/1024)} KiB)`:'';
      return [`${companion.name}: ${labels[operation.phase]??'Preparing panel'}${download}`];
    });
    if(view.progressError)details.push('Waiting for the next progress update…');
    return {operationId:view.activation.id,profileName:targetName,title:cancelling?'Cancelling activation':'Activating profile',phase,
      message:cancelling?'Cancelling this activation and restoring safe outputs. Please wait.':progress?.message??'Checking the saved profile and preparing its devices.',details,
      canCancel:!cancelling&&(progress?.canCancel??true)} satisfies ActivationWindowProgress;
  });
  onMount(()=>{
    mounted=true;
    const stop=runtime.subscribe(next=>{view=next;onState?.(next);});
    nativeOpened='__TAURI_INTERNALS__' in window;nativeResolved=manageActivationProgress;
    return()=>{mounted=false;stop();};
  });
  $effect(()=>{
    if(!manageActivationProgress||!mounted||!activating||!activationStarted)return;
    const started=activationStarted;
    const refresh=()=>{elapsed=Math.floor((Date.now()-started)/1000);void runtime.refreshProgress();};
    refresh();const timer=window.setInterval(refresh,400);return()=>window.clearInterval(timer);
  });
  function watchStatus(){
    if(!mounted||!active)return;
    const refresh=()=>{if(!document.hidden)void runtime.refresh();};
    refresh();const timer=window.setInterval(refresh,1500);document.addEventListener('visibilitychange',refresh);
    return()=>{window.clearInterval(timer);document.removeEventListener('visibilitychange',refresh);};
  }
  $effect(watchStatus);
  async function apply(){if(!applyDisabled&&editor)await runtime.apply(editor,selectedGame);}
  async function stop(){await runtime.stop();}
  async function recover(){await runtime.recover();}
  async function cancelPending(){await runtime.cancelPendingActivation();}
  function showProgress(node:HTMLDialogElement){node.showModal();return()=>node.close();}
</script>

<section class="profile-runtime" class:panel={!compact} class:compact aria-label="Runtime profile status">
  {#if !compact}<div class="runtime-heading"><strong>{native?.owner?`Active profile · ${appliedName}`:native?'No profile active':'Checking active profile…'}</strong></div>{/if}
  {#if native}
    {#if native.state==='awaiting-game'}<p class="runtime-notice">Outputs remain neutral while the game context is verified. Deactivate cancels this activation.</p>{/if}
    {#if native.degradation.length}<p class="runtime-notice">Some profile resources are unavailable. Open Debug for details.</p>{/if}
  {/if}
  {#if runtimeErrors.length||recoveryNeeded}<div class="runtime-error runtime-error-banner" role="alert"><div class="runtime-error-copy">{#each runtimeErrors as message (message)}<p>{message}</p>{:else}<p>Recover this profile before activating it again.</p>{/each}</div>{#if recoveryNeeded}<button class="secondary-button recover-runtime" disabled={view.pending==='recover'} onclick={recover}>Recover</button>{/if}</div>{/if}
  {#if !activating&&(!compact||native?.state==='transitioning')}<div class="runtime-actions">
    {#if editor&&!activeEditor}<label>Game context<select aria-label="Game context" bind:value={gameId} disabled={!!view.pending}><option value="">Hardware only</option>{#each choices as game (game.id)}<option value={game.id}>{game.name}</option>{/each}</select></label><button class="primary-button" disabled={applyDisabled} onclick={apply}>Activate profile</button>{:else if savedChangesPending}<button class="primary-button" disabled={applyDisabled} onclick={()=>editor&&runtime.apply(editor,native?.owner?.gameId??undefined)}>Retry saved changes</button>{/if}
    {#if native?.owner||view.pending==='apply'||native?.state==='transitioning'}<button class="secondary-button stop-runtime" aria-label="Deactivate current profile" disabled={view.pending==='stop'} onclick={stop}>Deactivate</button>{/if}
  </div>{/if}
  {#if !compact}{#if activeEditor}<p class="runtime-help">Saving this profile updates its active configuration.</p>{:else if editor&&!saved}<p class="runtime-help">Save your changes before activating this profile.</p>{:else if editor}<p class="runtime-help">Activate this profile when ready. Saving it keeps the current active profile.</p>{:else}<p class="runtime-help">Activate a saved profile from its tile for hardware only, or open it to choose a game context.</p>{/if}{/if}
  {#if invalidGame}<p class="runtime-notice">Choose a game context eligible for this profile, or explicitly choose Hardware only.</p>{/if}
  {#if progressPayload}<div class="activation-status" role="status"><span class="activation-spinner" aria-hidden="true"></span><div><strong>{cancelling?'Cancelling activation':`Activating ${targetName}`}</strong><p>{progressPayload.message}</p></div><button class="secondary-button cancel-activation" disabled={!progressPayload.canCancel} onclick={cancelPending}>{cancelling?'Cancelling…':'Cancel activation'}</button></div>{:else if view.pending}<p class="runtime-notice" role="status">{view.pending==='stop'?'Deactivating profile…':view.pending==='recover'?'Recovering outputs…':'Activating profile…'}</p>{/if}
  {#if view.notice&&!cancelledRecovery}<p class="runtime-notice" role="status">{view.notice}</p>{/if}
  {#if view.statusError}<p class="runtime-error" role="status">Current native status is unavailable: {view.statusError}</p>{/if}
</section>

{#if progressPayload&&nativeResolved&&!nativeOpened}<dialog class="activation-dialog" aria-label="Profile activation progress" oncancel={event=>{event.preventDefault();void cancelPending();}} {@attach showProgress}>
  <div class="activation-dialog-heading"><span class="activation-spinner" aria-hidden="true"></span><div><p class="activation-eyebrow">PROFILE ACTIVATION</p><h2>{progressPayload.title}</h2><p class="activation-profile-name" title={targetName}>{targetName}</p></div></div>
  <p class="activation-message" role="status">{progressPayload.message}</p>
  {#if progressPayload.details.length}<ul>{#each progressPayload.details as detail,index (index)}<li>{detail}</li>{/each}</ul>{/if}
  <div class="activation-track" role="progressbar" aria-label="Profile activation in progress"><span></span></div>
  <footer><span>{elapsed}s elapsed</span><button class="secondary-button cancel-activation" disabled={!progressPayload.canCancel} onclick={cancelPending}>{cancelling?'Cancelling…':'Cancel activation'}</button></footer>
</dialog>{/if}

<style>
  .activation-status{display:flex;align-items:center;gap:14px;border:1px solid var(--line);border-radius:8px;padding:16px;background:var(--surface);margin:12px 0}.activation-status>div{flex:1;min-width:0}.activation-status strong{font-size:12px}.activation-status p{font-size:11px;color:var(--muted);line-height:1.7;margin:6px 0 0}.activation-status .cancel-activation{margin-left:auto;white-space:nowrap}.activation-spinner{display:inline-block;flex-shrink:0;width:20px;height:20px;border:2px solid var(--line);border-top-color:var(--accent);border-radius:50%;animation:activation-spin .9s linear infinite}.activation-dialog{position:fixed;left:50%;top:50%;transform:translate(-50%,-50%);margin:0;padding:26px;width:480px;max-width:calc(100vw - 48px);box-sizing:border-box;border:1px solid var(--line);border-radius:12px;background:var(--surface);color:var(--text);box-shadow:0 20px 70px #0006}.activation-dialog::backdrop{background:#0008}.activation-dialog-heading{display:flex;align-items:center;gap:16px}.activation-profile-name{font-size:12px;color:var(--muted);margin:6px 0 0;overflow-wrap:anywhere}.activation-eyebrow{font-size:9px;letter-spacing:1.4px;color:var(--muted);margin:0 0 8px}.activation-dialog h2{font-size:18px;line-height:1.4;margin:0;overflow-wrap:anywhere}.activation-message,.activation-dialog li{font-size:12px;line-height:1.8;color:var(--muted)}.activation-message{margin:20px 0 16px}.activation-dialog ul{padding-left:18px}.activation-track{height:3px;background:var(--line);border-radius:2px;overflow:hidden;margin:22px 0}.activation-track span{display:block;width:35%;height:100%;background:var(--accent);animation:activation-slide 1.5s ease-in-out infinite}.activation-dialog footer{display:flex;align-items:center;justify-content:space-between;gap:16px}.activation-dialog footer>span{font-size:10px;color:var(--muted)}.activation-dialog button{font-size:11px}@keyframes activation-spin{to{transform:rotate(360deg)}}@keyframes activation-slide{from{transform:translateX(-110%)}to{transform:translateX(400%)}}@media(prefers-reduced-motion:reduce){.activation-spinner,.activation-track span{animation:none}}@media(max-width:560px){.activation-status{flex-wrap:wrap}.activation-status .cancel-activation{margin-left:34px}}
  .runtime-error-banner{display:flex;align-items:center;justify-content:space-between;gap:16px;flex-wrap:wrap}.runtime-error-copy{min-width:0;flex:1;overflow-wrap:anywhere}.runtime-error-copy p{margin:0}.runtime-error-copy p+p{margin-top:8px}.recover-runtime{flex-shrink:0;margin-left:auto}
  .profile-runtime.compact{padding:0;margin:0}.profile-runtime.compact:has(>*){margin-bottom:18px}
  .profile-runtime{padding:17px;margin-bottom:22px;}.runtime-heading,.runtime-actions{display:flex;align-items:center;gap:12px;flex-wrap:wrap;}.runtime-heading{justify-content:space-between;}.runtime-heading strong{font-size:12px;}.profile-runtime button{font-size:10px;}.runtime-actions{align-items:end;}.runtime-actions label{display:grid;gap:6px;font-size:9px;color:var(--muted);}.runtime-actions select{font-size:10px;max-width:220px;}.runtime-help{font-size:9px;color:var(--muted);line-height:1.8;margin:12px 0 0;overflow-wrap:anywhere;}.runtime-notice,.runtime-error{font-size:10px;line-height:1.8;margin:12px 0;padding:10px 12px;border:1px solid var(--line);border-radius:5px;}.runtime-notice{color:var(--soft);}.runtime-error{color:#c9ab7b;border-color:#ad875440;}.stop-runtime{border-color:#ad875460;}
</style>
