<script lang="ts">
  import {onDestroy,untrack,type Snippet} from 'svelte';
  import Icon from './Icon.svelte';
  import {gatewayRequest} from './gateway';
  import {nativeProfileStatus,type NativeProfileOwner,type NativeProfileStatus} from './profile-runtime';
  import {captureGameProfileCandidates,type GameProfileCandidates,type GameProfileContextRequest} from './game-profile-context';
  import {DEFAULT_PROFILE_ID,type DeepReadonly} from './profiles';
  import type {Game,GameId} from './games';
  type Request={command:'profile_game_launch';gameId:GameId;expectedActiveOwner:DeepReadonly<NativeProfileOwner>|null;profileId?:string;rememberChoice?:boolean}|{command:'profile_status'}|GameProfileContextRequest;
  let {game,busy=false,compact=false,send,actions}: {game:Game;busy?:boolean;compact?:boolean;send?:(request:Request)=>Promise<unknown>;actions?:Snippet}=$props();
  const transport=untrack(()=>send)??(async(request:Request)=>{const result=(await gatewayRequest(request)).profiles;if(!result)throw new Error('Native profile launch is unavailable. Reconnect the service.');return result;});
  let launching=$state(false),reading=$state(false),error=$state(''),message=$state(''),status=$state.raw<DeepReadonly<NativeProfileStatus>|null>(null);
  let switchOpen=$state(false),switchOwner=$state.raw<DeepReadonly<NativeProfileOwner>|null>(null);
  let choiceOpen=$state(false),candidates=$state.raw<DeepReadonly<GameProfileCandidates>|null>(null),profileId=$state(''),rememberChoice=$state(false);
  function gameKey():string{return JSON.stringify([game.id,game.executable??null,game.installed,game.launchSupported]);}
  let epoch=0,readEpoch=0,disposed=false,contextId=untrack(gameKey);
  const unavailable=$derived(!game.installed?'Choose the game executable first.':!game.launchSupported?game.launchUnavailableReason??game.launchReason??'Launching is unavailable on this platform.':'');
  const disabled=$derived(busy||launching||switchOpen||choiceOpen||!!unavailable);
  onDestroy(()=>{disposed=true;epoch++;readEpoch++;});
  function syncGame(){const key=gameKey();if(key!==contextId){contextId=key;epoch++;readEpoch++;launching=false;reading=false;switchOpen=false;switchOwner=null;choiceOpen=false;candidates=null;profileId='';rememberChoice=false;error='';message='';status=null;}}
  $effect(syncGame);
  function current(requestEpoch:number,context:string):boolean{return !disposed&&requestEpoch===epoch&&gameKey()===context;}
  const object=(value:unknown):value is Record<string,unknown>=>value!==null&&typeof value==='object'&&!Array.isArray(value);
  function response(value:unknown):DeepReadonly<NativeProfileStatus>{if(!object(value))throw new Error('Native profile status is unavailable.');return nativeProfileStatus(value.profileSession);}
  function statusLabel(value:DeepReadonly<NativeProfileStatus>):string{return {stopped:'No profile active',running:'Profile active',degraded:'Profile active with unavailable branches',faulted:'Profile needs attention','recovery-needed':'Profile recovery needed',transitioning:'Activating profile…','awaiting-game':'Waiting for the verified game'}[value.state];}
  function showDialog(node:HTMLDialogElement){node.showModal();return()=>node.close();}
  function cancelSwitch(){switchOpen=false;switchOwner=null;choiceOpen=false;profileId='';rememberChoice=false;epoch++;message='';}
  async function launch(){
    if(disposed||busy||launching||switchOpen||choiceOpen||!game.installed||!game.launchSupported)return;
    const requestEpoch=++epoch,context=gameKey();readEpoch++;reading=false;launching=true;error='';message='Finding matching profiles…';rememberChoice=false;profileId='';
    try{const matching=captureGameProfileCandidates(await transport({command:'profile_game_candidates',gameId:game.id}),game.id);if(!current(requestEpoch,context))return;candidates=matching;
      profileId=matching.preferredProfileId??(matching.profiles.length===1?matching.profiles[0].id:'');
      if(matching.profiles.length>1&&!profileId){choiceOpen=true;message='';return;}
      await checkActiveProfile(requestEpoch,context);
    }catch(cause){if(current(requestEpoch,context)){error=cause instanceof Error?cause.message:String(cause);message='Launch did not complete successfully. Refresh profile status before trying again.';}}
    finally{if(current(requestEpoch,context))launching=false;}
  }
  async function confirmChoice(){
    if(disposed||!choiceOpen||busy||launching||!candidates?.profiles.some(profile=>profile.id===profileId))return;
    const requestEpoch=++epoch,context=gameKey();choiceOpen=false;launching=true;error='';
    try{await checkActiveProfile(requestEpoch,context);}catch(cause){if(current(requestEpoch,context)){error=cause instanceof Error?cause.message:String(cause);message='Launch did not complete successfully. Refresh profile status before trying again.';}}
    finally{if(current(requestEpoch,context))launching=false;}
  }
  async function checkActiveProfile(requestEpoch:number,context:string){
    message='Checking the active profile…';const checked=response(await transport({command:'profile_status'}));if(!current(requestEpoch,context))return;status=checked;
    if(checked.owner&&checked.owner.profileId!==(profileId||DEFAULT_PROFILE_ID)){switchOwner=checked.owner;switchOpen=true;message='';return;}
    await launchWithOwner(checked.owner,requestEpoch,context);
  }
  async function confirmSwitch(){
    if(disposed||!switchOpen||!switchOwner||busy||launching)return;
    const expected=switchOwner,requestEpoch=++epoch,context=gameKey();readEpoch++;reading=false;switchOpen=false;switchOwner=null;launching=true;error='';
    try{await launchWithOwner(expected,requestEpoch,context);}
    finally{if(current(requestEpoch,context))launching=false;}
  }
  async function launchWithOwner(expectedActiveOwner:DeepReadonly<NativeProfileOwner>|null,requestEpoch:number,context:string){
    const gameId=game.id;message='Preparing the saved profile and waiting for the verified game…';status=null;
    try{const chosenId=profileId;const result=await transport({command:'profile_game_launch',gameId,expectedActiveOwner,...(chosenId?{profileId:chosenId,rememberChoice}:{})});if(!current(requestEpoch,context))return;
      const ack=object(result)?result.launch:undefined;
      if(!object(ack)||Object.keys(ack).length!==4||ack.gameId!==gameId||ack.state!=='started'||typeof ack.profileId!=='string'||!/^[a-f0-9]{8}-[a-f0-9]{4}-[a-f0-9]{4}-[a-f0-9]{4}-[a-f0-9]{12}$/.test(ack.profileId)||chosenId&&ack.profileId!==chosenId||!(ack.selectedGameId===null||ack.selectedGameId===gameId))throw new Error('The native launch completion did not match this game. Refresh profile status before trying again.');
      const started=response(result);
      if(!['running','degraded'].includes(started.state)||started.owner?.profileId!==ack.profileId||started.appliedSnapshot?.profileId!==ack.profileId)throw new Error('The native launch has no matching active applied profile. Refresh profile status for cleanup or recovery.');
      status=started;message=`${status.state==='degraded'?'Game started with unavailable branches.':'Game launch completed.'} ${statusLabel(status)}. ${ack.selectedGameId===null?'Hardware context only.':`Selected profile context: ${ack.selectedGameId}.`}`;
    }catch(cause){if(current(requestEpoch,context)){error=cause instanceof Error?cause.message:String(cause);message='Launch did not complete successfully. A spawned game may remain open; refresh profile status for owned cleanup or recovery.';}}
  }
  async function refreshStatus(){
    if(disposed||reading)return;const requestRead=++readEpoch,requestEpoch=epoch,context=gameKey();reading=true;
    try{const result=await transport({command:'profile_status'});if(!current(requestEpoch,context)||requestRead!==readEpoch)return;status=response(result);error='';}
    catch(cause){if(current(requestEpoch,context)&&requestRead===readEpoch)error=cause instanceof Error?cause.message:String(cause);}
    finally{if(requestRead===readEpoch)reading=false;}
  }
</script>

<div class="unified-launch" class:compact aria-label={`Unified profile launch for ${game.name}`}>
  <div class="launch-actions"><button class="primary-button" disabled={disabled} aria-label={`Launch ${game.name}`} onclick={launch}><Icon name={launching?'refresh':'play'} size={14}/>{launching?'Preparing launch…':'Launch'}</button>{@render actions?.()}</div>
  {#if unavailable}<p class="launch-help"><Icon name="info" size={14}/><span>{unavailable}</span></p>{/if}
  {#if message}<p class="launch-message" role="status" aria-live="polite"><Icon name={launching?'refresh':'info'} size={14}/><span>{message}</span></p>{/if}
  {#if status}<p class="launch-help"><Icon name={status.state==='running'?'check':'info'} size={14}/><span>{statusLabel(status)}</span></p>{#if status.degradation.length}<ul class="launch-degradation" aria-label="Unavailable profile branches">{#each status.degradation as diagnostic,index (index)}<li>{diagnostic}</li>{/each}</ul>{/if}{#if status.fault}<p class="launch-error" role="status"><Icon name="alert" size={14}/><span>{status.fault}</span></p>{/if}{/if}
  {#if error}<p class="launch-error" role="alert"><Icon name="alert" size={14}/><span>{error}</span></p>{/if}
  {#if error||status}<button class="text-button status-read" disabled={reading} onclick={refreshStatus}><Icon name="refresh" size={14}/>{reading?'Reading status…':'Refresh profile status'}</button>{/if}
</div>
{#if choiceOpen}
  <dialog class="profile-switch-dialog profile-choice-dialog" aria-modal="true" aria-labelledby="game-profile-choice-title" oncancel={event=>{event.preventDefault();cancelSwitch();}} {@attach showDialog}>
    <h2 id="game-profile-choice-title"><Icon name="network" size={18}/>Choose a profile for {game.name}</h2>
    <p>These profiles share this game context. The selected profile will be activated before the game launches.</p>
    <label class="profile-choice-label"><span><Icon name="network" size={14}/>Profile</span><select aria-label="Profile for game launch" bind:value={profileId}><option value="" disabled>Choose a profile</option>{#each candidates?.profiles??[] as profile (profile.id)}<option value={profile.id}>{profile.name}</option>{/each}</select></label>
    <label class="remember-choice"><input type="checkbox" bind:checked={rememberChoice}/> Remember my choice</label>
    <div class="launch-actions"><button class="secondary-button" onclick={cancelSwitch}><Icon name="close" size={14}/>Cancel</button><button class="primary-button" disabled={busy||launching||!profileId} onclick={confirmChoice}><Icon name="play" size={14}/>Launch</button></div>
  </dialog>
{/if}
{#if switchOpen}
  <dialog class="profile-switch-dialog" role="alertdialog" aria-modal="true" aria-labelledby="game-profile-switch-title" oncancel={event=>{event.preventDefault();cancelSwitch();}} {@attach showDialog}>
    <h2 id="game-profile-switch-title"><Icon name="refresh" size={18}/>Switch active profile?</h2>
    <p>A profile is already active. Launching {game.name} will deactivate it and activate the profile for this game.</p>
    <div class="launch-actions"><button class="secondary-button" onclick={cancelSwitch}><Icon name="close" size={14}/>Cancel</button><button class="primary-button" disabled={busy||launching} onclick={confirmSwitch}><Icon name="refresh" size={14}/>Switch profile</button></div>
  </dialog>
{/if}

<style>
  .profile-switch-dialog{position:fixed;left:50%;top:50%;transform:translate(-50%,-50%);width:calc(100% - 40px);max-width:480px;padding:24px;margin:0;border:1px solid var(--line);border-radius:10px;background:var(--surface);color:var(--text);box-shadow:0 20px 80px #0005}.profile-switch-dialog::backdrop{background:#0009}.profile-switch-dialog h2{display:flex;align-items:center;gap:9px;font-size:var(--game-detail-heading-size,14px);line-height:var(--game-detail-line-height,1.6)}.profile-switch-dialog p{font-size:var(--game-detail-text-size,12px);line-height:var(--game-detail-line-height,1.6);color:var(--muted);margin:16px 0}.profile-switch-dialog .launch-actions{justify-content:flex-end}
  .profile-choice-label{display:grid;gap:8px;font-size:var(--game-detail-text-size,12px);line-height:var(--game-detail-line-height,1.6);color:var(--muted)}.profile-choice-label>span{display:flex;align-items:center;gap:7px}.profile-choice-label select{width:100%;min-width:0;font-size:var(--game-detail-text-size,12px)}.remember-choice{display:flex;align-items:center;gap:9px;font-size:var(--game-detail-text-size,12px);line-height:var(--game-detail-line-height,1.6);color:var(--soft);margin:18px 0 24px}
  .unified-launch{min-width:0;}.launch-actions{display:flex;flex-wrap:wrap;align-items:center;gap:9px;}.unified-launch>.launch-actions{justify-content:flex-end;align-items:stretch;}.launch-actions button,.unified-launch>.launch-actions :global(button){display:inline-flex;align-items:center;justify-content:center;gap:7px;font-size:var(--game-detail-text-size,12px);line-height:var(--game-detail-line-height,1.6);}.launch-help,.launch-message,.launch-error{display:flex;align-items:flex-start;gap:7px;font-size:var(--game-detail-text-size,12px);line-height:var(--game-detail-line-height,1.6);margin-top:10px;max-width:480px;overflow-wrap:anywhere;}.launch-help :global(svg),.launch-message :global(svg),.launch-error :global(svg){flex-shrink:0;margin-top:3px;}.profile-switch-dialog :global(svg){flex-shrink:0;}.launch-help{color:var(--muted);}.launch-message{color:var(--soft);}.launch-error{color:#d3a078;}.launch-degradation{font-size:var(--game-detail-text-size,12px);color:#d3a078;line-height:var(--game-detail-line-height,1.6);padding-left:16px;margin-top:9px;}.status-read{font-size:var(--game-detail-text-size,12px);margin-top:7px;padding-left:0;}.compact .launch-actions button{font-size:10px;}.compact .launch-help,.compact .launch-message,.compact .launch-error{font-size:9px;}
</style>
