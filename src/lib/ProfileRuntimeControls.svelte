<script lang="ts">
  import { onMount, untrack } from 'svelte';
  import { ProfileRuntimeController, type RuntimeControlState, type RuntimeEditorTarget } from './profile-runtime-controller';
  import type { ProfileGameId } from './profile-runtime';
  import type { DeepReadonly, ProfileSummary } from './profiles';
  let { controller, editor = null, profiles = [], gameIds = [], active = true, onState, compact = false }: {
    controller: ProfileRuntimeController; editor?: RuntimeEditorTarget|null; profiles?: readonly DeepReadonly<ProfileSummary>[];
    gameIds?: readonly string[]; active?: boolean; onState?: (state: DeepReadonly<RuntimeControlState>)=>void; compact?:boolean;
  } = $props();
  const runtime = untrack(()=>controller);
  let view = $state.raw<DeepReadonly<RuntimeControlState>>(runtime.state);
  let mounted = $state(false), gameId = $state('');
  const games: {id:ProfileGameId;name:string}[] = [{id:'dcs',name:'DCS World'},{id:'x-plane-11',name:'X-Plane 11'},{id:'x-plane-12',name:'X-Plane 12'},{id:'assetto-corsa-rally',name:'Assetto Corsa Rally'}];
  const choices = $derived(games.filter(game=>!gameIds.length||gameIds.includes(game.id)));
  const native = $derived(view.status);
  const recoveryNeeded=$derived(native?.state==='recovery-needed'||native?.state==='faulted');
  const runtimeErrors=$derived([...new Set([native?.fault,view.error].filter((message):message is string=>!!message).map(message=>message.trim()).filter(Boolean))]);
  const selectedGame = $derived(choices.some(game=>game.id===gameId)?gameId as ProfileGameId:undefined);
  const invalidGame = $derived(gameId!==''&&!selectedGame);
  const saved = $derived(!!editor&&!editor.readOnly&&!editor.dirty&&editor.baseRevision!==null&&editor.baseRevision===editor.revision);
  const applyDisabled = $derived(!saved||invalidGame||!native||view.stale||!!view.pending||!!view.activationConfirmation||native.state==='recovery-needed'||native.state==='transitioning'||native.state==='awaiting-game');
  const appliedName = $derived(profiles.find(profile=>profile.id===native?.appliedSnapshot?.profileId)?.name??native?.appliedSnapshot?.profileId??'None');
  const activeEditor = $derived(!!editor&&native?.owner?.profileId===editor.id);
  const savedChangesPending = $derived(activeEditor&&native?.appliedSnapshot?.revision!==editor?.baseRevision);
  onMount(()=>{ mounted=true;const stop=runtime.subscribe(next=>{view=next;onState?.(next);});return()=>{mounted=false;stop();}; });
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
</script>

<section class="profile-runtime" class:panel={!compact} class:compact aria-label="Runtime profile status">
  {#if !compact}<div class="runtime-heading"><strong>{native?.owner?`Active profile · ${appliedName}`:native?'No profile active':'Checking active profile…'}</strong></div>{/if}
  {#if native}
    {#if native.state==='awaiting-game'}<p class="runtime-notice">Outputs remain neutral while the game context is verified. Deactivate cancels this activation.</p>{/if}
    {#if native.degradation.length}<p class="runtime-notice">Some profile resources are unavailable. Open Debug for details.</p>{/if}
  {/if}
  {#if runtimeErrors.length||recoveryNeeded}<div class="runtime-error runtime-error-banner" role="alert"><div class="runtime-error-copy">{#each runtimeErrors as message (message)}<p>{message}</p>{:else}<p>Recover this profile before activating it again.</p>{/each}</div>{#if recoveryNeeded}<button class="secondary-button recover-runtime" disabled={view.pending==='recover'} onclick={recover}>Recover</button>{/if}</div>{/if}
  {#if !compact||view.pending==='apply'||native?.state==='transitioning'}<div class="runtime-actions">
    {#if editor&&!activeEditor}<label>Game context<select aria-label="Game context" bind:value={gameId} disabled={!!view.pending}><option value="">Hardware only</option>{#each choices as game (game.id)}<option value={game.id}>{game.name}</option>{/each}</select></label><button class="primary-button" disabled={applyDisabled} onclick={apply}>Activate profile</button>{:else if savedChangesPending}<button class="primary-button" disabled={applyDisabled} onclick={()=>editor&&runtime.apply(editor,native?.owner?.gameId??undefined)}>Retry saved changes</button>{/if}
    {#if native?.owner||view.pending==='apply'||native?.state==='transitioning'}<button class="secondary-button stop-runtime" aria-label="Deactivate current profile" disabled={view.pending==='stop'} onclick={stop}>Deactivate</button>{/if}
  </div>{/if}
  {#if !compact}{#if activeEditor}<p class="runtime-help">Saving this profile updates its active configuration.</p>{:else if editor&&!saved}<p class="runtime-help">Save your changes before activating this profile.</p>{:else if editor}<p class="runtime-help">Activate this profile when ready. Saving it keeps the current active profile.</p>{:else}<p class="runtime-help">Activate a saved profile from its tile for hardware only, or open it to choose a game context.</p>{/if}{/if}
  {#if invalidGame}<p class="runtime-notice">Choose a game context eligible for this profile, or explicitly choose Hardware only.</p>{/if}
  {#if view.pending}<p class="runtime-notice" role="status">{view.pending==='apply'?'Activating profile…':view.pending==='stop'?'Deactivating profile…':'Recovering outputs…'}{#if view.pending==='apply'} Deactivate cancels this activation.{/if}</p>{/if}
  {#if view.statusError}<p class="runtime-error" role="status">Current native status is unavailable: {view.statusError}</p>{/if}
</section>

<style>
  .runtime-error-banner{display:flex;align-items:center;justify-content:space-between;gap:16px;flex-wrap:wrap}.runtime-error-copy{min-width:0;flex:1;overflow-wrap:anywhere}.runtime-error-copy p{margin:0}.runtime-error-copy p+p{margin-top:8px}.recover-runtime{flex-shrink:0;margin-left:auto}
  .profile-runtime.compact{padding:0;margin:0}.profile-runtime.compact:has(>*){margin-bottom:18px}
  .profile-runtime{padding:17px;margin-bottom:22px;}.runtime-heading,.runtime-actions{display:flex;align-items:center;gap:12px;flex-wrap:wrap;}.runtime-heading{justify-content:space-between;}.runtime-heading strong{font-size:12px;}.profile-runtime button{font-size:10px;}.runtime-actions{align-items:end;}.runtime-actions label{display:grid;gap:6px;font-size:9px;color:var(--muted);}.runtime-actions select{font-size:10px;max-width:220px;}.runtime-help{font-size:9px;color:var(--muted);line-height:1.8;margin:12px 0 0;overflow-wrap:anywhere;}.runtime-notice,.runtime-error{font-size:10px;line-height:1.8;margin:12px 0;padding:10px 12px;border:1px solid var(--line);border-radius:5px;}.runtime-notice{color:var(--soft);}.runtime-error{color:#c9ab7b;border-color:#ad875440;}.stop-runtime{border-color:#ad875460;}
</style>
