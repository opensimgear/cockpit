<script lang="ts">
  import Icon from './Icon.svelte';
  import {gameCatalogue, type GameCommandName} from './games';
  import {gatewayRequest} from './gateway';
  let {gameId}:{gameId:string}=$props();
  let busy=$state(false),error=$state('');
  const game=$derived($gameCatalogue.find(game=>game.id===gameId));
  const monitor=$derived(game?.processMonitor);
  const pending=$derived(monitor?.state==='starting'||monitor?.state==='checking');
  const ready=$derived(monitor?.state==='ready');
  async function request(command:GameCommandName){
    busy=true;error='';
    try{await gatewayRequest({command,gameId});}
    catch(reason){error=reason instanceof Error?reason.message:String(reason);}
    finally{busy=false;}
  }
</script>

{#if game?.provider==='x-plane'}
  <section class="process-monitor" aria-label="Process monitoring">
    <div><h3>Process monitoring</h3><p role="status">{monitor?.message??'Start the game to prepare native process monitoring.'}</p></div>
    {#if pending||ready}
      <button class="secondary-button" disabled={busy} onclick={()=>request('game_process_monitor_cancel')}><Icon name={pending?'close':'stop'} size={14}/>{pending?'Cancel':'Stop'}</button>
    {:else}
      <button class="secondary-button" disabled={busy||!game.installed||game.processMonitoringSupported===false} onclick={()=>request('game_process_monitor_enable')}><Icon name="check" size={14}/>Allow</button>
    {/if}
    {#if error}<p class="monitor-error" role="alert">{error}</p>{/if}
    {#if monitor?.state==='needs-permission'||monitor?.state==='failed'}<p class="monitor-help">Windows may ask for permission. Monitoring ends with the game or Cockpit.</p>{/if}
    {#if monitor?.state==='failed'&&monitor.details}<details><summary>Details</summary><p>{monitor.details}</p></details>{/if}
  </section>
{/if}

<style>
  .process-monitor{display:flex;align-items:center;gap:12px;flex-wrap:wrap;border-top:1px solid var(--line);padding-top:14px;margin-top:14px}
  .process-monitor>div{flex:1;min-width:180px}.process-monitor h3{font-size:var(--ui-text-size);margin:0}
  .process-monitor p{font-size:var(--ui-text-size);color:var(--muted);line-height:var(--ui-line-height);margin:6px 0 0}
  .process-monitor .monitor-error{color:var(--danger,#d49386);flex-basis:100%}.monitor-help{flex-basis:100%}
  details{flex-basis:100%;font-size:var(--ui-text-size);color:var(--muted)}
</style>
