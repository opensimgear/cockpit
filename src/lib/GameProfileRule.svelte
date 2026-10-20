<script lang="ts">
  import {onDestroy,onMount,untrack} from 'svelte';
  import {gatewayRequest} from './gateway';
  import {GameProfileContextClient,type GameProfileCandidates} from './game-profile-context';
  import type {DeepReadonly} from './profiles';
  import type {Game} from './games';
  import Icon from './Icon.svelte';
  let{game,active=true,busy=false,client:suppliedClient}:{game:Game;active?:boolean;busy?:boolean;client?:GameProfileContextClient}=$props();
  const client=untrack(()=>suppliedClient)??new GameProfileContextClient(async request=>{const result=(await gatewayRequest(request)).profiles;if(!result)throw new Error('Matching profiles are unavailable.');return result;});
  let candidates=$state.raw<DeepReadonly<GameProfileCandidates>|null>(null),profileId=$state(''),loading=$state(false),pending=$state(false),error=$state(''),notice=$state(''),mounted=$state(false);
  let epoch=0,disposed=false,contextId=untrack(()=>game.id);
  const savedId=$derived(candidates?.preferredProfileId??'');
  const dirty=$derived(!!candidates&&profileId!==savedId);
  const ready=$derived(active&&!busy&&!pending&&!!candidates);
  function syncGame(){if(game.id===contextId)return;contextId=game.id;epoch++;candidates=null;profileId='';loading=false;pending=false;error='';notice='';}
  $effect(syncGame);
  function accept(value:DeepReadonly<GameProfileCandidates>,preserveDraft=false){const edited=dirty;candidates=value;if(!preserveDraft||!edited)profileId=value.preferredProfileId??'';}
  async function refresh(){if(disposed||!active||loading||pending)return;const requestEpoch=++epoch,gameId=game.id;loading=true;try{const value=await client.candidates(gameId);if(disposed||epoch!==requestEpoch||game.id!==gameId)return;accept(value,true);error='';}catch(cause){if(!disposed&&epoch===requestEpoch&&game.id===gameId)error=cause instanceof Error?cause.message:String(cause);}finally{if(!disposed&&epoch===requestEpoch)loading=false;}}
  async function chooseProfile(value:string){
    if(disposed||!ready||value===savedId||value&&!candidates!.profiles.some(profile=>profile.id===value))return;
    const requestEpoch=++epoch,gameId=game.id;profileId=value;loading=false;pending=true;notice='';error='';
    try{const result=value?await client.savePreference(gameId,value):await client.removePreference(gameId);if(disposed||epoch!==requestEpoch||game.id!==gameId)return;accept(result);notice=value?'Profile choice remembered.':'Remembered choice removed. You will be asked when multiple profiles match.';}
    catch(cause){if(!disposed&&epoch===requestEpoch&&game.id===gameId){profileId=savedId;error=cause instanceof Error?cause.message:String(cause);}}
    finally{if(!disposed&&epoch===requestEpoch)pending=false;}
  }
  function mountProfiles(){mounted=true;return()=>{mounted=false;epoch++;};}
  onMount(mountProfiles);onDestroy(()=>{disposed=true;epoch++;});
  function watchProfiles(){syncGame();if(!mounted||!active)return;const read=()=>{if(!document.hidden)untrack(()=>{void refresh();});};read();document.addEventListener('visibilitychange',read);const timer=window.setInterval(read,2000);return()=>{window.clearInterval(timer);document.removeEventListener('visibilitychange',read);};}
  $effect(watchProfiles);
</script>
<section class="game-profile-rule panel" aria-label="Matching profiles">
  <div class="rule-heading">
    <h2><Icon name="network" size={18}/>Matching profiles</h2>
    {#if candidates&&(candidates.profiles.length>1||savedId)}<div class="preference-choice"><label><span class="rule-label"><Icon name="zap" size={14}/>Auto select{#if pending}<span class="choice-saving" role="status"><Icon name="refresh" size={14}/>Saving…</span>{/if}</span><select aria-label="Auto select" value={profileId} onchange={event=>chooseProfile(event.currentTarget.value)} disabled={!ready}><option value="">Ask me each time</option>{#each candidates.profiles as profile (profile.id)}<option value={profile.id}>{profile.name}</option>{/each}</select></label></div>{/if}
  </div>
  {#if candidates}
    {#if candidates.profiles.length}<ul class="matching-profiles" aria-label={`Profiles for ${game.name}`}>
      {#each candidates.profiles as profile (profile.id)}<li class="profile-tile" class:remembered={profile.id===savedId}><span class="profile-mark"><Icon name="network" size={18}/></span><div class="profile-copy"><span class="profile-name" title={profile.name}>{profile.name}</span>{#if profile.id===savedId}<span class="choice-badge"><Icon name="check" size={14}/>Remembered</span>{:else if candidates.profiles.length===1}<span class="choice-badge"><Icon name="zap" size={14}/>Automatic</span>{/if}</div></li>{/each}
    </ul>{:else}<p class="rule-help">No matching profiles. Launch uses Default for hardware only. Set a profile’s game context in the profile editor to add it here.</p>{/if}
  {:else}<p class="rule-help rule-status" role="status"><Icon name={loading?'refresh':'info'} size={14}/><span>{loading?'Loading matching profiles…':'Matching profiles will appear here.'}</span></p>{/if}
  {#if notice}<p class="rule-help rule-status" role="status"><Icon name="check" size={14}/><span>{notice}</span></p>{/if}{#if error}<p class="rule-warning" role="alert"><Icon name="alert" size={14}/><span>{error}</span><button class="text-button" disabled={loading||pending} onclick={refresh}><Icon name="refresh" size={14}/>Retry</button></p>{/if}
</section>
<style>
  .game-profile-rule{padding:20px;margin-bottom:20px;min-width:0;font-size:var(--game-detail-text-size,12px);line-height:var(--game-detail-line-height,1.6)}
  .rule-heading{display:flex;align-items:center;justify-content:space-between;flex-wrap:wrap;gap:16px}
  .game-profile-rule h2{display:flex;align-items:center;gap:10px;font-size:var(--game-detail-heading-size,14px);font-weight:600;line-height:var(--game-detail-line-height,1.6)}
  .game-profile-rule>p,.rule-help{font-size:var(--game-detail-text-size,12px);color:var(--muted);line-height:var(--game-detail-line-height,1.6);margin:12px 0;overflow-wrap:anywhere}
  .matching-profiles{list-style:none;padding:0;margin:16px 0 0;display:grid;grid-template-columns:repeat(auto-fill,minmax(min(100%,180px),1fr));gap:10px}
  .profile-tile{display:flex;align-items:center;gap:10px;min-height:66px;padding:10px 12px;border:1px solid var(--line);border-radius:6px;background:var(--bg);font-size:var(--game-detail-text-size,12px);min-width:0}
  .profile-tile.remembered{border-color:var(--accent);background:var(--accent-bg)}
  .profile-mark{display:grid;place-items:center;flex-shrink:0;width:30px;height:30px;border-radius:5px;background:var(--surface-raised);color:var(--muted)}
  .remembered .profile-mark{color:var(--accent)}
  .profile-copy{display:flex;flex-direction:column;gap:3px;min-width:0}
  .profile-name{min-width:0;overflow:hidden;text-overflow:ellipsis;white-space:nowrap}
  .choice-badge{display:inline-flex;align-items:center;gap:6px;font-size:var(--game-detail-text-size,12px);color:var(--accent);white-space:nowrap}
  .preference-choice{display:grid;width:400px;max-width:100%;margin-left:auto}
  .preference-choice label{display:grid;grid-template-columns:auto minmax(0,1fr);align-items:center;gap:10px;min-width:0;font-size:var(--game-detail-text-size,12px);color:var(--muted)}
  .preference-choice select{width:100%;min-width:0;font-size:var(--game-detail-text-size,12px);line-height:var(--game-detail-line-height,1.6)}
  .rule-label,.rule-status{display:flex;align-items:center;gap:7px}
  .rule-label{white-space:nowrap}
  .rule-status{align-items:flex-start}
  .choice-saving{display:inline-flex;align-items:center;gap:5px;margin-left:auto;color:var(--soft)}
  .rule-warning button{display:inline-flex;align-items:center;justify-content:center;gap:7px;font-size:var(--game-detail-text-size,12px);line-height:var(--game-detail-line-height,1.6)}
  .rule-warning{display:flex;align-items:flex-start;flex-wrap:wrap;gap:8px;font-size:var(--game-detail-text-size,12px);line-height:var(--game-detail-line-height,1.6);color:#c9ab7b!important;padding:10px;border:1px solid #ad875440;border-radius:5px}
  .rule-warning>span{flex:1;min-width:0;overflow-wrap:anywhere}
  .game-profile-rule :global(svg){flex-shrink:0}
</style>
