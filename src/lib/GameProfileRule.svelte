<script lang="ts">
  import {onDestroy,onMount,untrack} from 'svelte';
  import {gatewayRequest} from './gateway';
  import {ProfileAutomaticClient} from './profile-automatic';
  import {ProfileAutomaticController} from './profile-automatic-controller';
  import {DEFAULT_PROFILE_ID,type DeepReadonly,type ProfileSummary} from './profiles';
  import type {Game} from './games';
  let{game,active=true,busy=false,client:suppliedClient,controller:suppliedController}:{game:Game;active?:boolean;busy?:boolean;client?:ProfileAutomaticClient;controller?:ProfileAutomaticController}=$props();
  const client=untrack(()=>suppliedClient)??new ProfileAutomaticClient(async request=>{const result=(await gatewayRequest(request)).profiles;if(!result)throw new Error('Native automatic profile rules are unavailable.');return result;});
  const provided=untrack(()=>suppliedController),controller=provided??new ProfileAutomaticController(client);
  let view=$state.raw(controller.state),profiles=$state.raw<DeepReadonly<ProfileSummary[]>>([]),profileId=$state(DEFAULT_PROFILE_ID),priority=$state('0'),enabled=$state(false),base=$state(''),initialized=$state(false),mounted=$state(false),loadingProfiles=$state(false),profilesError=$state(''),notice=$state('');
  let profileEpoch=0,disposed=false;
  const savedRule=$derived(view.status?.rules.find(rule=>rule.identity.gameId===game.id));
  function ruleDraftSignature(){return JSON.stringify([profileId,Number(priority),enabled]);}
  const draftSignature=$derived(ruleDraftSignature());
  const validPriority=$derived(priority.trim()!==''&&Number.isInteger(Number(priority))&&Number(priority)>=-32768&&Number(priority)<=32767);
  const dirty=$derived(initialized&&(!validPriority||draftSignature!==base));
  const savedChoice=$derived(profiles.some(profile=>profile.id===profileId));
  const currentInstallation=$derived(!!game.installed&&!!game.executable&&game.processMonitoringSupported===true);
  const ready=$derived(active&&!!view.status&&!view.stale&&!view.status.readOnly&&!view.pending&&!busy);
  const canSave=$derived(ready&&dirty&&validPriority&&savedChoice&&(currentInstallation||!enabled&&!!savedRule));
  function syncDraft(){if(!view.status||dirty)return;const rule=savedRule;profileId=rule?.profileId??DEFAULT_PROFILE_ID;priority=String(rule?.priority??0);enabled=rule?.enabled??false;base=JSON.stringify([profileId,Number(priority),enabled]);initialized=true;}
  $effect(syncDraft);
  function mountRule(){mounted=true;const stop=controller.subscribe(next=>view=next);void refreshProfiles();return()=>{mounted=false;profileEpoch++;stop();};}
  onMount(mountRule);onDestroy(()=>{disposed=true;profileEpoch++;if(!provided)controller.dispose();});
  function watchStatus(){if(!mounted||!active)return;const refresh=()=>{if(!document.hidden)void controller.refresh();};refresh();document.addEventListener('visibilitychange',refresh);const timer=window.setInterval(refresh,1000);return()=>{window.clearInterval(timer);document.removeEventListener('visibilitychange',refresh);};}
  $effect(watchStatus);
  async function refreshProfiles(){const epoch=++profileEpoch;loadingProfiles=true;profilesError='';try{const saved=await client.profiles();if(!disposed&&epoch===profileEpoch)profiles=saved;}catch(error){if(!disposed&&epoch===profileEpoch)profilesError=error instanceof Error?error.message:String(error);}finally{if(!disposed&&epoch===profileEpoch)loadingProfiles=false;}}
  async function save(){if(!canSave)return;notice='';const captured=ruleDraftSignature(),gameId=game.id;const ok=await controller.saveRule(gameId,profileId,Number(priority),enabled);if(!ok||disposed||game.id!==gameId)return;const saved=controller.state.status?.rules.find(rule=>rule.identity.gameId===gameId);if(!saved)return;base=JSON.stringify([saved.profileId,saved.priority,saved.enabled]);notice=ruleDraftSignature()===captured?'Game rule saved. Automatic mode uses this saved rule.':'Captured game rule saved. Newer local edits remain unsaved.';}
  async function remove(){if(!ready||!savedRule)return;notice='';const captured=ruleDraftSignature(),gameId=game.id;const ok=await controller.removeRule(gameId);if(!ok||disposed||game.id!==gameId)return;base=JSON.stringify([DEFAULT_PROFILE_ID,0,false]);if(ruleDraftSignature()===captured){profileId=DEFAULT_PROFILE_ID;priority='0';enabled=false;}notice='Game rule removed.';}
</script>
<section class="game-profile-rule panel" aria-label="Automatic profile rule">
  <h2>Automatic profile rule</h2><p>Choose a saved unified profile for {game.name}. Enable Automatic explicitly in Profiles. Saving an enabled rule may switch the active profile immediately while Automatic is enabled. Higher priority wins; equal highest priorities require a choice. Pause, Stop and recovery continue to inhibit automatic activation.</p>
  <dl><div><dt>Current installation executable</dt><dd>{game.executable??'Choose a validated game installation first.'}</dd></div>{#if savedRule}<div><dt>Saved native executable identity</dt><dd>{savedRule.identity.executable}</dd></div>{/if}</dl>
  <fieldset disabled={busy||!initialized||view.status?.readOnly===true}><label class="rule-enabled"><input type="checkbox" bind:checked={enabled}/> Enable this game rule</label><label>Saved unified profile<select aria-label="Automatic unified profile" bind:value={profileId} disabled={loadingProfiles||!profiles.length}>{#if !savedChoice}<option value={profileId}>Saved profile unavailable · {profileId}</option>{/if}{#each profiles as profile (profile.id)}<option value={profile.id}>{profile.name} · revision {profile.revision}</option>{/each}</select></label><label>Priority<input class="text-input" type="number" min="-32768" max="32767" step="1" value={priority} oninput={event=>priority=event.currentTarget.value}/></label></fieldset>
  {#if !currentInstallation}<p class="rule-help">Enabling a rule requires a current validated installation with native process monitoring. Existing rules can still be disabled or removed.</p>{/if}{#if !validPriority}<p class="rule-warning">Choose an integer priority from -32768 to 32767.</p>{/if}{#if !savedChoice&&profiles.length}<p class="rule-warning">Choose an existing saved unified profile or remove the stale rule.</p>{/if}
  <div class="rule-actions"><button class="primary-button" disabled={!canSave} onclick={save}>{view.pending==='save-rule'?'Saving rule…':'Save game rule'}</button><button class="secondary-button" disabled={!ready||!savedRule} onclick={remove}>{view.pending==='remove-rule'?'Removing…':'Remove game rule'}</button><button class="text-button" disabled={loadingProfiles} onclick={refreshProfiles}>Refresh saved profiles</button><button class="text-button" disabled={view.loading||!!view.pending} onclick={()=>controller.refresh()}>Refresh rule status</button></div>
  {#if view.status}<p class="rule-help">Mode: {view.status.mode} · Native automatic status: {view.status.status}{view.stale?' · last status is stale':''}. {view.status.message}</p>{:else}<p class="rule-help">Waiting for native rule status.</p>{/if}{#if dirty}<p class="rule-help">Unsaved rule edits.</p>{/if}{#if notice}<p class="rule-help" role="status">{notice}</p>{/if}{#if profilesError||view.error}<p class="rule-warning" role="status">{profilesError||view.error}</p>{/if}
</section>
<style>
  .game-profile-rule{padding:20px;margin-bottom:20px;}.game-profile-rule h2{font-size:14px;}.game-profile-rule>p,.rule-help{font-size:10px;color:var(--muted);line-height:1.8;margin:12px 0;}.game-profile-rule dl{font-size:9px;margin:17px 0;display:grid;gap:11px;}.game-profile-rule dt{color:var(--muted);margin-bottom:5px;}.game-profile-rule dd{margin:0;color:var(--soft);overflow-wrap:anywhere;}.game-profile-rule fieldset{border:0;padding:0;display:grid;grid-template-columns:minmax(170px,1fr) 100px;gap:14px;}.game-profile-rule label{display:grid;gap:7px;font-size:10px;color:var(--muted);}.game-profile-rule .rule-enabled{display:flex;align-items:center;gap:8px;grid-column:1/-1;}.game-profile-rule select,.game-profile-rule input[type=number]{width:100%;min-width:0;}.rule-actions{display:flex;gap:10px;flex-wrap:wrap;margin-top:16px;}.rule-actions button{font-size:10px;}.rule-warning{font-size:10px!important;line-height:1.8;color:#c9ab7b!important;padding:10px;border:1px solid #ad875440;border-radius:5px;}@media(max-width:600px){.game-profile-rule fieldset{grid-template-columns:1fr;}}
</style>
