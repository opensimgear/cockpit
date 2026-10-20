<script lang="ts">
  import {onDestroy,onMount,untrack} from 'svelte';
  import {gatewayRequest} from './gateway';
  import {GameProfileContextClient,type GameProfileCandidates} from './game-profile-context';
  import type {DeepReadonly} from './profiles';
  import type {Game} from './games';
  let{game,active=true,busy=false,client:suppliedClient}:{game:Game;active?:boolean;busy?:boolean;client?:GameProfileContextClient}=$props();
  const client=untrack(()=>suppliedClient)??new GameProfileContextClient(async request=>{const result=(await gatewayRequest(request)).profiles;if(!result)throw new Error('Matching profiles are unavailable.');return result;});
  let candidates=$state.raw<DeepReadonly<GameProfileCandidates>|null>(null),profileId=$state(''),loading=$state(false),pending=$state(false),error=$state(''),notice=$state(''),mounted=$state(false);
  let epoch=0,disposed=false,contextId=untrack(()=>game.id);
  const savedId=$derived(candidates?.preferredProfileId??'');
  const dirty=$derived(!!candidates&&profileId!==savedId);
  const ready=$derived(active&&!busy&&!pending&&!!candidates);
  const canSave=$derived(ready&&dirty&&!!profileId&&candidates!.profiles.some(profile=>profile.id===profileId));
  function syncGame(){if(game.id===contextId)return;contextId=game.id;epoch++;candidates=null;profileId='';loading=false;pending=false;error='';notice='';}
  $effect(syncGame);
  function accept(value:DeepReadonly<GameProfileCandidates>,preserveDraft=false){const edited=dirty;candidates=value;if(!preserveDraft||!edited)profileId=value.preferredProfileId??'';}
  async function refresh(){if(disposed||!active||loading||pending)return;const requestEpoch=++epoch,gameId=game.id;loading=true;try{const value=await client.candidates(gameId);if(disposed||epoch!==requestEpoch||game.id!==gameId)return;accept(value,true);error='';}catch(cause){if(!disposed&&epoch===requestEpoch&&game.id===gameId)error=cause instanceof Error?cause.message:String(cause);}finally{if(!disposed&&epoch===requestEpoch)loading=false;}}
  async function save(){if(disposed||!canSave)return;const requestEpoch=++epoch,gameId=game.id,captured=profileId;loading=false;pending=true;notice='';error='';try{const value=await client.savePreference(gameId,captured);if(disposed||epoch!==requestEpoch||game.id!==gameId)return;const newer=profileId!==captured;candidates=value;if(!newer)profileId=value.preferredProfileId??'';notice='Profile choice remembered.';}catch(cause){if(!disposed&&epoch===requestEpoch&&game.id===gameId)error=cause instanceof Error?cause.message:String(cause);}finally{if(!disposed&&epoch===requestEpoch)pending=false;}}
  async function remove(){if(disposed||!ready||!savedId)return;const requestEpoch=++epoch,gameId=game.id;loading=false;pending=true;notice='';error='';try{const value=await client.removePreference(gameId);if(disposed||epoch!==requestEpoch||game.id!==gameId)return;accept(value);notice='Remembered choice removed. You will be asked when multiple profiles match.';}catch(cause){if(!disposed&&epoch===requestEpoch&&game.id===gameId)error=cause instanceof Error?cause.message:String(cause);}finally{if(!disposed&&epoch===requestEpoch)pending=false;}}
  function mountProfiles(){mounted=true;return()=>{mounted=false;epoch++;};}
  onMount(mountProfiles);onDestroy(()=>{disposed=true;epoch++;});
  function watchProfiles(){syncGame();if(!mounted||!active)return;const read=()=>{if(!document.hidden)untrack(()=>{void refresh();});};read();document.addEventListener('visibilitychange',read);const timer=window.setInterval(read,2000);return()=>{window.clearInterval(timer);document.removeEventListener('visibilitychange',read);};}
  $effect(watchProfiles);
</script>
<section class="game-profile-rule panel" aria-label="Matching profiles">
  <h2>Matching profiles</h2>
  <p>Profiles with {game.name} as their saved game context are activated before launch. One match is selected automatically; with multiple matches, you choose a profile unless you have remembered a choice.</p>
  {#if candidates}
    {#if candidates.profiles.length}<ul class="matching-profiles" aria-label={`Profiles for ${game.name}`}>
      {#each candidates.profiles as profile (profile.id)}<li><span class="profile-name" title={profile.name}>{profile.name}</span>{#if profile.id===savedId}<span class="choice-badge">Remembered</span>{:else if candidates.profiles.length===1}<span class="choice-badge">Automatic</span>{/if}</li>{/each}
    </ul>{:else}<p class="rule-help">No matching profiles. Launch uses Default for hardware only. Set a profile’s game context in the profile editor to add it here.</p>{/if}
    {#if candidates.profiles.length>1||savedId}<div class="preference-choice"><label>Remembered profile<select aria-label="Remembered profile" bind:value={profileId} disabled={!ready}><option value="">Ask me each time</option>{#each candidates.profiles as profile (profile.id)}<option value={profile.id}>{profile.name}</option>{/each}</select></label><div class="rule-actions"><button class="primary-button" disabled={!canSave} onclick={save}>{pending?'Saving…':'Remember choice'}</button><button class="secondary-button" disabled={!ready||!savedId} onclick={remove}>Remove choice</button></div></div>{/if}
  {:else}<p class="rule-help" role="status">{loading?'Loading matching profiles…':'Matching profiles will appear here.'}</p>{/if}
  {#if notice}<p class="rule-help" role="status">{notice}</p>{/if}{#if error}<p class="rule-warning" role="alert">{error} <button class="text-button" disabled={loading||pending} onclick={refresh}>Retry</button></p>{/if}
</section>
<style>
  .game-profile-rule{padding:20px;margin-bottom:20px}.game-profile-rule h2{font-size:14px}.game-profile-rule>p,.rule-help{font-size:10px;color:var(--muted);line-height:1.8;margin:12px 0}.matching-profiles{list-style:none;padding:0;margin:16px 0;display:grid;gap:8px}.matching-profiles li{display:flex;align-items:center;gap:12px;padding:10px 12px;border:1px solid var(--line);border-radius:5px;font-size:11px;min-width:0}.profile-name{flex:1;overflow:hidden;text-overflow:ellipsis;white-space:nowrap}.choice-badge{font-size:9px;color:var(--accent);white-space:nowrap}.preference-choice{display:flex;align-items:flex-end;flex-wrap:wrap;gap:12px;margin-top:18px}.preference-choice label{display:grid;gap:7px;flex:1;min-width:180px;font-size:10px;color:var(--muted)}.preference-choice select{width:100%;min-width:0}.rule-actions{display:flex;gap:10px;flex-wrap:wrap}.rule-actions button{font-size:10px}.rule-warning{font-size:10px!important;line-height:1.8;color:#c9ab7b!important;padding:10px;border:1px solid #ad875440;border-radius:5px}.rule-warning button{font-size:10px}
</style>
