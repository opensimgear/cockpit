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
  function mountStatus(){mounted=true;const stop=controller.subscribe(next=>view=next);return()=>{mounted=false;stop();};}
  onMount(mountStatus);onDestroy(()=>{if(!provided)controller.dispose();});
  function watchStatus(){if(!mounted||!active)return;const refresh=()=>{if(!document.hidden)void controller.refresh();};refresh();document.addEventListener('visibilitychange',refresh);const timer=window.setInterval(refresh,1000);return()=>{window.clearInterval(timer);document.removeEventListener('visibilitychange',refresh);};}
  $effect(watchStatus);
  async function select(mode:ProfileSelectionMode){if(blocked)return;await controller.selectionMode(mode);}
</script>
<section class="automatic-controls panel" aria-label="Automatic profile selection">
  <div><h2>Profile selection</h2><p>Manual keeps your selection pinned. Automatic applies matching saved profiles for verified running games, with Default as the fallback. Opening or saving an editor does not change the mode.</p></div>
  <div class="mode-choice" role="group" aria-label="Profile selection mode"><button class="secondary-button" aria-pressed={view.status?.mode==='manual'} disabled={blocked} onclick={()=>select('manual')}>Manual</button><button class="secondary-button" aria-pressed={view.status?.mode==='automatic'} disabled={blocked} onclick={()=>select('automatic')}>Automatic</button><button class="text-button" disabled={view.loading||!!view.pending} onclick={()=>controller.refresh()}>Refresh selection status</button></div>
  {#if view.status}<dl><div><dt>Mode</dt><dd>{view.status.mode}{view.stale?' · last status is stale':''}</dd></div><div><dt>Manual pin</dt><dd>{profileName(view.status.manualProfileId)}</dd></div><div><dt>Native automatic status</dt><dd>{view.status.status}</dd></div>{#if view.status.target}<div><dt>Automatic target</dt><dd>{profileName(view.status.target.profileId)}{view.status.target.gameId?` · ${view.status.target.gameId}`:' · Default fallback'}</dd></div>{/if}</dl><p role="status">{view.status.message}</p>{#if view.status.conflicts.length}<p class="automatic-warning">Conflicting games: {view.status.conflicts.join(', ')}. Edit priorities in the game details.</p>{/if}{#if view.status.readOnly}<p class="automatic-warning">Automatic selection settings are read-only. Resolve the native storage error before changing policy.</p>{/if}{:else}<p role="status">Waiting for native automatic-selection status.</p>{/if}
  {#if view.pending}<p role="status">Saving explicit selection mode…</p>{/if}{#if view.error}<p class="automatic-warning" role="status">{view.error}</p>{/if}
</section>
<style>
  .automatic-controls{padding:18px;margin-bottom:20px;}.automatic-controls h2{font-size:13px;}.automatic-controls p{font-size:10px;color:var(--muted);line-height:1.8;margin-top:10px;}.mode-choice{display:flex;gap:10px;align-items:center;flex-wrap:wrap;margin:16px 0;}.mode-choice button{font-size:10px;}.mode-choice button[aria-pressed=true]{color:var(--accent);border-color:var(--accent);}.automatic-controls dl{display:flex;gap:18px;flex-wrap:wrap;font-size:10px;}.automatic-controls dt{color:var(--muted);font-size:9px;margin-bottom:6px;}.automatic-controls dd{margin:0;color:var(--soft);overflow-wrap:anywhere;}.automatic-warning{color:#c9ab7b!important;border:1px solid #ad875440;padding:10px;border-radius:5px;}
</style>
