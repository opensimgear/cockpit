<script lang="ts">
  import {onDestroy,onMount,untrack} from 'svelte';
  import {gatewayRequest} from './gateway';
  import {guardUiSettings} from './ui-settings-guard';
  import {ProfileAutomaticClient} from './profile-automatic';
  import {ProfileAutomaticController} from './profile-automatic-controller';
  import {gameCatalogue,gameName} from './games';
  import Icon from './Icon.svelte';
  import {notify} from './notifications';
  import type {ProfileSelectionMode} from './profiles';
  let{active=true,controller:supplied}:{active?:boolean;controller?:ProfileAutomaticController}=$props();
  const settingsRequest=guardUiSettings(gatewayRequest);
  const provided=untrack(()=>supplied),controller=provided??new ProfileAutomaticController(new ProfileAutomaticClient(async request=>{const result=(await settingsRequest(request)).profiles;if(!result)throw new Error('Native automatic profile selection is unavailable.');return result;}));
  let view=$state.raw(controller.state),mounted=$state(false);
  const blocked=$derived(!active||!view.status||view.stale||view.status.readOnly||!!view.pending);
  const modeLabel=$derived(!view.status || view.stale && !view.pending ? 'Mode unavailable' : view.status.mode==='automatic' ? 'Auto' : 'Manual');
  const modeTitle=$derived(view.pending ? 'Saving profile mode…' : view.error || (view.status?.readOnly ? 'Profile mode is read-only. Resolve the native storage error before changing it.' : !view.status || view.stale ? 'Current native profile selection mode is unavailable.' : [
    `Switch to ${view.status.mode==='automatic'?'Manual':'Auto'} profile selection.`,
    view.status.conflicts.length ? `Conflicting games: ${view.status.conflicts.map(id=>$gameCatalogue.find(game=>game.id===id)?.name??gameName(id)).join(', ')}. Review matching profiles and remembered choices in game settings.` : view.status.status==='failed' ? view.status.message : '',
  ].filter(Boolean).join(' ')));
  function mountStatus(){mounted=true;const stop=controller.subscribe(next=>view=next);return()=>{mounted=false;stop();};}
  onMount(mountStatus);onDestroy(()=>{if(!provided)controller.dispose();});
  function watchStatus(){if(!mounted||!active)return;const refresh=()=>{if(!document.hidden)void controller.refresh();};refresh();document.addEventListener('visibilitychange',refresh);const timer=window.setInterval(refresh,1000);return()=>{window.clearInterval(timer);document.removeEventListener('visibilitychange',refresh);};}
  $effect(watchStatus);
  async function select(mode:ProfileSelectionMode){if(blocked)return;if(!await controller.selectionMode(mode))notify(controller.state.error,'error');}
  async function toggleMode(){await select(view.status?.mode==='automatic'?'manual':'automatic');}
</script>
<button type="button" class="profile-mode-status" aria-label="Automatic profile selection" aria-pressed={view.status?.mode==='automatic'} aria-busy={!!view.pending} title={modeTitle} disabled={blocked} onclick={toggleMode}>
  <span class="badge-copy"><span class="badge-label">Profile mode</span><strong class="mode-value">{modeLabel}</strong></span><Icon name="swap" size={13}/>
</button>
<style>
  .profile-mode-status{display:inline-flex;align-items:center;gap:12px;min-width:0;max-width:145px;flex-shrink:0;border:0;border-radius:0 5px 5px 0;padding:6px 10px;background:transparent;color:var(--muted);text-align:left;cursor:pointer;}
  .profile-mode-status:hover:enabled{background:var(--surface-raised);color:var(--accent);}
  .profile-mode-status[aria-pressed=true]{color:var(--accent);}
  .profile-mode-status:focus-visible{outline:2px solid var(--accent);outline-offset:-3px;}
  .profile-mode-status:disabled{cursor:not-allowed;opacity:.6;}
  .badge-copy{display:flex;flex-direction:column;gap:2px;min-width:0;}
  .badge-label{color:var(--muted);font-size:8px;white-space:nowrap;}
  strong{color:var(--soft);font-size:10px;font-weight:500;overflow:hidden;text-overflow:ellipsis;white-space:nowrap;}
  @media(max-width:520px){.profile-mode-status{padding:5px 6px;gap:5px;max-width:100px;}.badge-label{font-size:7px;}strong{font-size:9px;}}
</style>
