<script lang="ts">
  import {onDestroy,untrack} from 'svelte';
  import Icon from './Icon.svelte';
  import {gatewayRequest} from './gateway';
  import {nativeProfileStatus,type NativeProfileStatus} from './profile-runtime';
  import type {DeepReadonly} from './profiles';
  import type {Game,GameId} from './games';
  type Request={command:'profile_game_launch';gameId:GameId}|{command:'profile_stop'|'profile_status'};
  let {game,busy=false,compact=false,send}: {game:Game;busy?:boolean;compact?:boolean;send?:(request:Request)=>Promise<unknown>}=$props();
  const transport=untrack(()=>send)??(async(request:Request)=>{const result=(await gatewayRequest(request)).profiles;if(!result)throw new Error('Native profile launch is unavailable. Reconnect the service.');return result;});
  let launching=$state(false),stopping=$state(false),reading=$state(false),error=$state(''),message=$state(''),status=$state.raw<DeepReadonly<NativeProfileStatus>|null>(null);
  function gameKey():string{return JSON.stringify([game.id,game.executable??null,game.installed,game.launchSupported]);}
  let epoch=0,readEpoch=0,disposed=false,contextId=untrack(gameKey);
  const unavailable=$derived(!game.installed?'Choose the game executable first.':!game.launchSupported?game.launchUnavailableReason??game.launchReason??'Launching is unavailable on this platform.':'');
  const disabled=$derived(busy||launching||stopping||!!unavailable);
  onDestroy(()=>{disposed=true;epoch++;readEpoch++;});
  function syncGame(){const key=gameKey();if(key!==contextId){contextId=key;epoch++;readEpoch++;launching=false;stopping=false;reading=false;error='';message='';status=null;}}
  $effect(syncGame);
  function current(requestEpoch:number,context:string):boolean{return !disposed&&requestEpoch===epoch&&gameKey()===context;}
  const object=(value:unknown):value is Record<string,unknown>=>value!==null&&typeof value==='object'&&!Array.isArray(value);
  function response(value:unknown):DeepReadonly<NativeProfileStatus>{if(!object(value))throw new Error('Native profile status is unavailable.');return nativeProfileStatus(value.profileSession);}
  async function launch(){
    if(disposed||busy||launching||stopping||!game.installed||!game.launchSupported)return;
    const requestEpoch=++epoch,gameId=game.id,context=gameKey();readEpoch++;reading=false;launching=true;error='';message='Preparing the saved profile and waiting for the verified game…';status=null;
    try{const result=await transport({command:'profile_game_launch',gameId});if(!current(requestEpoch,context))return;
      const ack=object(result)?result.launch:undefined;
      if(!object(ack)||Object.keys(ack).length!==4||ack.gameId!==gameId||ack.state!=='started'||typeof ack.profileId!=='string'||!/^[a-f0-9]{8}-[a-f0-9]{4}-[a-f0-9]{4}-[a-f0-9]{4}-[a-f0-9]{12}$/.test(ack.profileId)||!(ack.selectedGameId===null||['dcs','assetto-corsa-rally','x-plane-11','x-plane-12'].includes(String(ack.selectedGameId))))throw new Error('The native launch completion did not match this game. Refresh profile status before trying again.');
      const started=response(result);
      if(!['running','degraded'].includes(started.state)||started.owner?.profileId!==ack.profileId||started.appliedSnapshot?.profileId!==ack.profileId)throw new Error('The native launch has no matching active applied profile. Refresh profile status for cleanup or recovery.');
      status=started;message=`${status.state==='degraded'?'Game started with unavailable branches.':'Game launch completed.'} Native profile state: ${status.state}. ${ack.selectedGameId===null?'Hardware context only.':`Selected profile context: ${ack.selectedGameId}.`}`;
    }catch(cause){if(current(requestEpoch,context)){error=cause instanceof Error?cause.message:String(cause);message='Launch did not complete successfully. A spawned game may remain open; refresh profile status for owned cleanup or recovery.';}}
    finally{if(current(requestEpoch,context))launching=false;}
  }
  async function stop(){
    if(disposed||stopping)return;const requestEpoch=++epoch,context=gameKey();readEpoch++;reading=false;launching=false;stopping=true;error='';message='Stopping the native profile and any pending launch…';status=null;
    try{const result=await transport({command:'profile_stop'});if(!current(requestEpoch,context))return;status=response(result);message=`Native profile state: ${status.state}.`;
    }catch(cause){if(current(requestEpoch,context)){error=cause instanceof Error?cause.message:String(cause);message='Stop was not confirmed. Refresh native profile status and recover owned setup if needed.';}}
    finally{if(current(requestEpoch,context))stopping=false;}
  }
  async function refreshStatus(){
    if(disposed||reading)return;const requestRead=++readEpoch,requestEpoch=epoch,context=gameKey();reading=true;
    try{const result=await transport({command:'profile_status'});if(!current(requestEpoch,context)||requestRead!==readEpoch)return;status=response(result);error='';}
    catch(cause){if(current(requestEpoch,context)&&requestRead===readEpoch)error=cause instanceof Error?cause.message:String(cause);}
    finally{if(requestRead===readEpoch)reading=false;}
  }
</script>

<div class="unified-launch" class:compact aria-label={`Unified profile launch for ${game.name}`}>
  <div class="launch-actions"><button class="primary-button" disabled={disabled} aria-label={`Launch ${game.name}`} onclick={launch}><Icon name={launching?'refresh':'play'} size={14}/>{launching?'Preparing launch…':'Launch'}</button><button class="secondary-button" disabled={stopping} onclick={stop}><Icon name="stop" size={13}/>{stopping?'Stopping…':'Stop profile'}</button></div>
  {#if !compact}<p class="launch-help">Uses the saved Manual pin or Automatic rule/default. Provider setup is required only for the matching game branches that the selected profile needs. Profile editing and game settings stay separate.</p>{/if}
  {#if unavailable}<p class="launch-help">{unavailable}</p>{/if}
  {#if message}<p class="launch-message" role="status" aria-live="polite">{message}</p>{/if}
  {#if status}<p class="launch-help">Last native status: {status.state}{#if status.appliedSnapshot} · applied revision {status.appliedSnapshot.revision}{/if}{#if status.paused} · paused{/if}</p>{#if status.degradation.length}<ul class="launch-degradation" aria-label="Unavailable profile branches">{#each status.degradation as diagnostic,index (index)}<li>{diagnostic}</li>{/each}</ul>{/if}{#if status.fault}<p class="launch-error" role="status">{status.fault}</p>{/if}{/if}
  {#if error}<p class="launch-error" role="alert">{error}</p>{/if}
  {#if error||status}<button class="text-button status-read" disabled={reading} onclick={refreshStatus}>{reading?'Reading status…':'Refresh profile status'}</button>{/if}
</div>

<style>
  .unified-launch{min-width:0;}.launch-actions{display:flex;flex-wrap:wrap;align-items:center;gap:9px;}.launch-actions button{display:inline-flex;align-items:center;justify-content:center;gap:7px;font-size:10px;}.launch-help,.launch-message,.launch-error{font-size:10px;line-height:1.75;margin-top:10px;max-width:480px;overflow-wrap:anywhere;}.launch-help{color:var(--muted);}.launch-message{color:var(--soft);}.launch-error{color:#d3a078;}.launch-degradation{font-size:10px;color:#d3a078;line-height:1.75;padding-left:16px;margin-top:9px;}.status-read{font-size:9px;margin-top:7px;padding-left:0;}.compact .launch-help,.compact .launch-message,.compact .launch-error{font-size:9px;}
</style>
