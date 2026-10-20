<script lang="ts">
  import { onMount, untrack } from 'svelte';
  import { ProfileRuntimeController, type RuntimeControlState, type RuntimeEditorTarget } from './profile-runtime-controller';
  import type { ProfileGameId } from './profile-runtime';
  import type { DeepReadonly, ProfileSummary } from './profiles';
  let { controller, editor = null, profiles = [], gameIds = [], active = true, onState }: {
    controller: ProfileRuntimeController; editor?: RuntimeEditorTarget|null; profiles?: readonly DeepReadonly<ProfileSummary>[];
    gameIds?: readonly string[]; active?: boolean; onState?: (state: DeepReadonly<RuntimeControlState>)=>void;
  } = $props();
  const runtime = untrack(()=>controller);
  let view = $state.raw<DeepReadonly<RuntimeControlState>>(runtime.state);
  let mounted = $state(false), gameId = $state('');
  const games: {id:ProfileGameId;name:string}[] = [{id:'dcs',name:'DCS World'},{id:'x-plane-11',name:'X-Plane 11'},{id:'x-plane-12',name:'X-Plane 12'},{id:'assetto-corsa-rally',name:'Assetto Corsa Rally'}];
  const choices = $derived(games.filter(game=>!gameIds.length||gameIds.includes(game.id)));
  const native = $derived(view.status);
  const selectedGame = $derived(choices.some(game=>game.id===gameId)?gameId as ProfileGameId:undefined);
  const invalidGame = $derived(gameId!==''&&!selectedGame);
  const saved = $derived(!!editor&&!editor.readOnly&&!editor.dirty&&editor.baseRevision!==null&&editor.baseRevision===editor.revision);
  const applyDisabled = $derived(!saved||invalidGame||!native||view.stale||!!view.pending||native.paused||native.state==='recovery-needed'||native.state==='transitioning'||native.state==='awaiting-game');
  const appliedName = $derived(profiles.find(profile=>profile.id===native?.appliedSnapshot?.profileId)?.name??native?.appliedSnapshot?.profileId??'None');
  const runningEditor = $derived(!!editor&&native?.owner?.profileId===editor.id);
  const savedChangesPending = $derived(runningEditor&&native?.appliedSnapshot?.revision!==editor?.baseRevision);
  onMount(()=>{ mounted=true;const stop=runtime.subscribe(next=>{view=next;onState?.(next);});return()=>{mounted=false;stop();}; });
  function watchStatus(){
    if(!mounted||!active)return;
    const refresh=()=>{if(!document.hidden)void runtime.refresh();};
    refresh();const timer=window.setInterval(refresh,1500);document.addEventListener('visibilitychange',refresh);
    return()=>{window.clearInterval(timer);document.removeEventListener('visibilitychange',refresh);};
  }
  $effect(watchStatus);
  async function apply(){if(!applyDisabled&&editor)await runtime.apply(editor,selectedGame);}
  async function pause(){await runtime.pause(!native?.paused);}
  async function stop(){await runtime.stop();}
  async function recover(){await runtime.recover();}
</script>

<section class="profile-runtime panel" aria-label="Runtime profile status">
  <div class="runtime-heading"><div><strong>Active profile · {native?.owner?appliedName:'None'}</strong><span class:stale={view.stale} aria-live="polite">{native?native.state:'Checking status…'}{#if view.stale&&native} · updating{/if}</span></div></div>
  {#if native}
    {#if native.paused}<p class="runtime-notice">Profiles are paused. Clear pause to allow Automatic selection, or start a saved profile in Manual mode.</p>{/if}
    {#if native.state==='awaiting-game'}<p class="runtime-notice">Hardware is prepared and held neutral while the game context is verified. Processing has not started. Stop or Pause cancels this launch.</p>{/if}
    {#if native.fault}<p class="runtime-error" role="alert">{native.fault}</p>{/if}
    {#if native.degradation.length}<p class="runtime-notice">Some profile resources are unavailable. Open Debug for details.</p>{/if}
  {/if}
  <div class="runtime-actions">
    {#if editor&&!runningEditor}<label>Game context<select aria-label="Game context" bind:value={gameId} disabled={!!view.pending}><option value="">Hardware only</option>{#each choices as game (game.id)}<option value={game.id}>{game.name}</option>{/each}</select></label><button class="primary-button" disabled={applyDisabled} onclick={apply}>Start profile</button>{:else if savedChangesPending}<button class="primary-button" disabled={applyDisabled} onclick={()=>editor&&runtime.apply(editor,native?.owner?.gameId??undefined)}>Retry saved changes</button>{/if}
    <button class="secondary-button" disabled={native?.paused?!!view.pending:view.pending==='pause'} onclick={pause}>{native?.paused?'Clear pause':'Pause'}</button>
    <button class="secondary-button stop-runtime" aria-label="Stop current profile" disabled={view.pending==='stop'} onclick={stop}>Stop</button>
    {#if native?.state==='recovery-needed'||native?.state==='faulted'}<button class="secondary-button" disabled={view.pending==='recover'} onclick={recover}>Recover outputs</button>{/if}
  </div>
  {#if runningEditor}<p class="runtime-help">Saving this profile applies its changes while it is running.</p>{:else if editor&&!saved}<p class="runtime-help">Save your changes before starting this profile.</p>{:else if editor}<p class="runtime-help">Start this profile when ready. Saving it leaves the current profile running.</p>{:else}<p class="runtime-help">Open a profile to edit or start it.</p>{/if}
  {#if invalidGame}<p class="runtime-notice">Choose a game context eligible for this profile, or explicitly choose Hardware only.</p>{/if}
  {#if view.pending}<p class="runtime-notice" role="status">Native {view.pending==='clear-pause'?'pause clearing':view.pending} in progress…{#if view.pending==='apply'} Stop or Pause can interrupt this request.{/if}</p>{/if}
  {#if view.error}<p class="runtime-error" role="alert">{view.error}</p>{/if}
  {#if view.statusError}<p class="runtime-error" role="status">Current native status is unavailable: {view.statusError}</p>{/if}
</section>

<style>
  .profile-runtime{padding:17px;margin-bottom:22px;}.runtime-heading,.runtime-heading>div,.runtime-actions{display:flex;align-items:center;gap:12px;flex-wrap:wrap;}.runtime-heading{justify-content:space-between;}.runtime-heading strong{font-size:12px;}.runtime-heading span{font-size:10px;color:var(--accent);}.runtime-heading span.stale{color:var(--muted);}.profile-runtime button{font-size:10px;}.runtime-actions{align-items:end;}.runtime-actions label{display:grid;gap:6px;font-size:9px;color:var(--muted);}.runtime-actions select{font-size:10px;max-width:220px;}.runtime-help{font-size:9px;color:var(--muted);line-height:1.8;margin:12px 0 0;overflow-wrap:anywhere;}.runtime-notice,.runtime-error{font-size:10px;line-height:1.8;margin:12px 0;padding:10px 12px;border:1px solid var(--line);border-radius:5px;}.runtime-notice{color:var(--soft);}.runtime-error{color:#c9ab7b;border-color:#ad875440;}.stop-runtime{border-color:#ad875460;}
</style>
