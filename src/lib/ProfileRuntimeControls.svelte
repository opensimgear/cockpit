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

<section class="profile-runtime panel" aria-label="Native profile runtime">
  <div class="runtime-heading"><div><strong>Profile runtime</strong><span class:stale={view.stale} aria-live="polite">{native?native.state:'Waiting for native status'}{#if view.stale&&native} · last captured status{/if}</span></div><button class="text-button" disabled={view.loading||!!view.pending} onclick={()=>runtime.refresh()}>{view.loading?'Refreshing…':'Refresh status'}</button></div>
  {#if native}
    <dl><div><dt>Applied snapshot</dt><dd>{native.appliedSnapshot?`${appliedName} · revision ${native.appliedSnapshot.revision}`:'None'}</dd></div><div><dt>Current run</dt><dd>{native.owner?`Generation ${native.owner.generation} · ${native.owner.gameId??'Hardware only'}`:'No running session'}</dd></div></dl>
    {#if native.owner}<p class="run-identity">Session <code>{native.owner.session}</code> · Editing or saving another revision leaves this run unchanged.</p>{/if}
    {#if native.paused}<p class="runtime-notice">Pause is persisted. Clearing it permits Automatic selection to resume. In Manual mode, explicitly Apply a saved revision to restart.</p>{/if}
    {#if native.state==='awaiting-game'}<p class="runtime-notice">Hardware is prepared and held neutral while the game context is verified. Processing has not started. Stop or Pause cancels this launch.</p>{/if}
    {#if native.fault}<p class="runtime-error" role="alert">{native.fault}</p>{/if}
    {#if native.degradation.length}<ul class="runtime-notice" aria-label="Native runtime degradation">{#each native.degradation as reason,index (index)}<li>{reason}</li>{/each}</ul>{/if}
  {/if}
  <div class="runtime-actions">
    {#if editor}<label>Game context<select aria-label="Game context for Apply" bind:value={gameId} disabled={!!view.pending}><option value="">Hardware only</option>{#each choices as game (game.id)}<option value={game.id}>{game.name}</option>{/each}</select></label><button class="primary-button" disabled={applyDisabled} onclick={apply}>Apply saved revision {editor.baseRevision??'—'}</button>{/if}
    <button class="secondary-button" disabled={native?.paused?!!view.pending:view.pending==='pause'} onclick={pause}>{native?.paused?'Clear pause':'Pause'}</button>
    <button class="secondary-button stop-runtime" disabled={view.pending==='stop'} onclick={stop}>Stop</button>
    {#if native?.state==='recovery-needed'||native?.state==='faulted'}<button class="secondary-button" disabled={view.pending==='recover'} onclick={recover}>Recover outputs</button>{/if}
  </div>
  {#if editor&&!saved}<p class="runtime-help">Save the exact draft revision before Apply. Unsaved graph edits and machine assignments are never applied.</p>{:else if editor}<p class="runtime-help">Apply replaces the current run with this saved revision. A game context must match the actual running game; unrelated branches stay dormant.</p>{:else}<p class="runtime-help">Open a saved profile to Apply its exact revision. Opening an editor does not start a run.</p>{/if}
  {#if invalidGame}<p class="runtime-notice">Choose a game context eligible for this profile, or explicitly choose Hardware only.</p>{/if}
  {#if view.pending}<p class="runtime-notice" role="status">Native {view.pending==='clear-pause'?'pause clearing':view.pending} in progress…{#if view.pending==='apply'} Stop or Pause can interrupt this request.{/if}</p>{/if}
  {#if view.error}<p class="runtime-error" role="alert">{view.error}</p>{/if}
  {#if view.statusError}<p class="runtime-error" role="status">Current native status is unavailable: {view.statusError}</p>{/if}
</section>

<style>
  .profile-runtime{padding:17px;margin-bottom:22px;}.runtime-heading,.runtime-heading>div,.runtime-actions{display:flex;align-items:center;gap:12px;flex-wrap:wrap;}.runtime-heading{justify-content:space-between;}.runtime-heading strong{font-size:12px;}.runtime-heading span{font-size:10px;color:var(--accent);}.runtime-heading span.stale{color:var(--muted);}.profile-runtime button{font-size:10px;}.profile-runtime dl{display:flex;gap:24px;flex-wrap:wrap;margin:17px 0;}.profile-runtime dl>div{display:grid;gap:6px;}.profile-runtime dt{font-size:9px;color:var(--muted);}.profile-runtime dd{margin:0;font-size:11px;overflow-wrap:anywhere;}.runtime-actions{align-items:end;}.runtime-actions label{display:grid;gap:6px;font-size:9px;color:var(--muted);}.runtime-actions select{font-size:10px;max-width:220px;}.runtime-help,.run-identity{font-size:9px;color:var(--muted);line-height:1.8;margin:12px 0 0;overflow-wrap:anywhere;}.runtime-notice,.runtime-error{font-size:10px;line-height:1.8;margin:12px 0;padding:10px 12px;border:1px solid var(--line);border-radius:5px;}.runtime-notice{color:var(--soft);}.runtime-notice li{margin-left:14px;}.runtime-error{color:#c9ab7b;border-color:#ad875440;}.stop-runtime{border-color:#ad875460;}
</style>
