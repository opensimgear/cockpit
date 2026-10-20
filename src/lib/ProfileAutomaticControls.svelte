<script lang="ts">
  import {onDestroy,onMount,untrack} from 'svelte';
  import {gatewayRequest} from './gateway';
  import {ProfileAutomaticClient} from './profile-automatic';
  import {ProfileAutomaticController} from './profile-automatic-controller';
  import type {DeepReadonly,ProfileSelectionMode,ProfileSummary} from './profiles';
  let{profiles,active=true,controller:supplied}:{profiles:readonly DeepReadonly<ProfileSummary>[];active?:boolean;controller?:ProfileAutomaticController}=$props();
  const provided=untrack(()=>supplied),controller=provided??new ProfileAutomaticController(new ProfileAutomaticClient(async request=>{const result=(await gatewayRequest(request)).profiles;if(!result)throw new Error('Native automatic profile selection is unavailable.');return result;}));
  let view=$state.raw(controller.state),mounted=$state(false);
  const blocked=$derived(!active||!view.status||view.stale||view.status.readOnly||!!view.pending);
  const profileName=(id:string)=>profiles.find(profile=>profile.id===id)?.name??id;
  const description=$derived(view.status?.mode==='manual'?`Manual keeps ${profileName(view.status.manualProfileId)} pinned. Automatic selects matching saved profiles for verified running games, with Default as the fallback.`:'Automatic selects matching saved profiles for verified running games, with Default as the fallback. Manual keeps your selection pinned.');
  function mountStatus(){mounted=true;const stop=controller.subscribe(next=>view=next);return()=>{mounted=false;stop();};}
  onMount(mountStatus);onDestroy(()=>{if(!provided)controller.dispose();});
  function watchStatus(){if(!mounted||!active)return;const refresh=()=>{if(!document.hidden)void controller.refresh();};refresh();document.addEventListener('visibilitychange',refresh);const timer=window.setInterval(refresh,1000);return()=>{window.clearInterval(timer);document.removeEventListener('visibilitychange',refresh);};}
  $effect(watchStatus);
  async function select(mode:ProfileSelectionMode){if(blocked)return;await controller.selectionMode(mode);}
  async function toggleMode(){await select(view.status?.mode==='automatic'?'manual':'automatic');}
</script>
<div class="automatic-controls">
  <div class="mode-choice" role="group" aria-label="Profile selection mode" title={description}><span class="mode-label">Profile mode</span><span class:current={view.status?.mode==='manual'}>Manual</span><button type="button" class="mode-switch" role="switch" aria-label="Automatic profile selection" aria-checked={view.status?.mode==='automatic'} disabled={blocked} onclick={toggleMode}><span class="switch-thumb"></span></button><span class:current={view.status?.mode==='automatic'}>Automatic</span></div>
  {#if !view.status}<p role="status">Waiting for native profile mode.</p>{/if}
  {#if view.status?.conflicts.length}<p class="automatic-warning" role="status">Conflicting games: {view.status.conflicts.join(', ')}. Edit priorities in the game details.</p>{:else if view.status?.status==='failed'}<p class="automatic-warning" role="status">{view.status.message}</p>{/if}
  {#if view.status?.readOnly}<p class="automatic-warning" role="status">Profile mode is read-only. Resolve the native storage error before changing it.</p>{/if}
  {#if view.pending}<p role="status">Saving profile mode…</p>{/if}{#if view.error}<p class="automatic-warning" role="status">{view.error} <button class="text-button" disabled={view.loading||!!view.pending} onclick={()=>controller.refresh()}>Refresh mode</button></p>{/if}
</div>
<style>
  .automatic-controls{max-width:440px;}.mode-choice{display:flex;gap:9px;align-items:center;flex-wrap:wrap;font-size:10px;color:var(--muted);}.mode-label{margin-right:7px;color:var(--soft);}.mode-choice .current{color:var(--accent);}.mode-switch{position:relative;flex-shrink:0;width:38px;height:22px;border:1px solid var(--line);border-radius:12px;background:var(--surface-raised);padding:2px;transition:background 120ms,border-color 120ms;}.mode-switch[aria-checked=true]{background:var(--accent-bg);border-color:var(--accent);}.switch-thumb{display:block;width:16px;height:16px;border-radius:50%;background:var(--muted);transition:transform 120ms,background 120ms;}.mode-switch[aria-checked=true] .switch-thumb{transform:translateX(16px);background:var(--accent);}.mode-switch:focus-visible{outline:2px solid var(--accent);outline-offset:3px;}.automatic-controls p{font-size:10px;color:var(--muted);line-height:1.8;margin:9px 0 0;}.automatic-warning{color:#c9ab7b!important;}.automatic-controls .text-button{font-size:10px;}
</style>
