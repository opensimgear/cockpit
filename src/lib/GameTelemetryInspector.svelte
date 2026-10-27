<script lang="ts">
  import {onDestroy,onMount,untrack} from 'svelte';
  import {gatewayRequest} from './gateway';
  import {gameCatalogue,type GameId} from './games';
  import {GameTelemetryClient,GameTelemetryController,preferredTelemetryGame,telemetryAge,telemetryContext,telemetryStatus,telemetryValue} from './game-telemetry';
  let {active=true,controller:supplied}:{active?:boolean;controller?:GameTelemetryController}=$props();
  const provided=untrack(()=>supplied),inspector=provided??new GameTelemetryController(new GameTelemetryClient(async request=>(await gatewayRequest(request)).gameTelemetry));
  let view=$state.raw(inspector.state),mounted=$state(false),gameId=$state<GameId|''>(untrack(()=>inspector.state.query.gameId??''));
  let search=$state(untrack(()=>inspector.state.query.search??'')),group=$state(untrack(()=>inspector.state.query.group??'')),offset=$state(untrack(()=>inspector.state.query.offset));
  const limit=64;
  const game=$derived(view.report?.games.find(game=>game.id===(gameId||preferredTelemetryGame(view.report?.games??[]))));
  const context=$derived(telemetryContext(game?.context??null));
  const pageEnd=$derived((game?.offset??0)+(game?.channels.length??0));
  const canRead=$derived(active&&view.active);
  onMount(()=>{const stop=inspector.subscribe(next=>view=next);mounted=true;return()=>{mounted=false;stop();};});
  onDestroy(()=>{if(!provided)inspector.dispose();});
  $effect(()=>{if(!gameId&&view.report){const preferred=preferredTelemetryGame(view.report.games);if(preferred)gameId=preferred;}});
  $effect(()=>{if(game&&game.offset>0&&game.offset>=game.total)offset=0;});
  function watchTelemetry(){
    const enabled=mounted&&active,query={gameId:gameId||undefined,offset,limit,search,group};
    if(!mounted)return;
    inspector.configure(query);
    if(!enabled){inspector.setActive(false);return;}
    const refresh=()=>{inspector.setActive(enabled&&!document.hidden);if(inspector.state.active)void inspector.refresh();};
    refresh();const timer=window.setInterval(refresh,1000);document.addEventListener('visibilitychange',refresh);
    return()=>{window.clearInterval(timer);document.removeEventListener('visibilitychange',refresh);inspector.setActive(false);};
  }
  $effect(watchTelemetry);
  function chooseGame(value:string){gameId=value as GameId;group='';search='';offset=0;}
  function chooseGroup(value:string){group=value;offset=0;}
  function details(channel:NonNullable<typeof game>['channels'][number]):string{return[channel.description,channel.nativeField?`Native field: ${channel.nativeField}`:channel.nativeId?`Native ID: ${channel.nativeId}`:'',channel.provider?`Provider: ${channel.provider}`:''].filter(Boolean).join('\n');}
</script>

<section class="game-telemetry" aria-label="Incoming game telemetry">
  <div class="telemetry-toolbar">
    <label>Game<select aria-label="Telemetry game" value={gameId} onchange={event=>chooseGame(event.currentTarget.value)}>{#if !gameId}<option value="">Choose automatically</option>{/if}{#each $gameCatalogue.filter(item=>item.provider) as supported (supported.id)}{@const summary=view.report?.games.find(game=>game.id===supported.id)}<option value={supported.id}>{supported.name}{summary?` · ${telemetryStatus(summary.status)}`:''}</option>{/each}</select></label>
    <label class="telemetry-search">Find a channel<input type="search" aria-label="Search game telemetry" placeholder="Channel, label, or native field" maxlength="200" bind:value={search} oninput={()=>offset=0}/></label>
    <button class="secondary-button" disabled={!canRead||view.loading} onclick={()=>inspector.refresh()}>{view.loading?'Reading…':'Refresh telemetry'}</button>
  </div>
  <p class="telemetry-help">Incoming channels from the game integration, including channels outside your profile. Common channels use shared names and units; native fields retain their documented units. This view updates once a second while visible.</p>
  {#if view.error}<p class="telemetry-error" role="alert">{view.error} Last captured values are unavailable until a fresh reply arrives.</p>{/if}
  {#if game}
    <section class="telemetry-summary panel" aria-label="Game telemetry source">
      <div class="source-heading"><div><h2>{game.name}</h2><p>{game.source||'Source not available'}</p></div><span class="source-status" class:live={!view.stale&&['live','ready','receiving'].includes(game.status)} class:paused={!view.stale&&game.status==='paused'}>{view.stale?'Stale capture':telemetryStatus(game.status)}</span></div>
      {#if game.message}<p class="source-message">{game.message}</p>{/if}
      <dl class="source-facts"><div><dt>Last packet</dt><dd>{game.packet??'Not observed'}</dd></div><div><dt>Packet age</dt><dd>{telemetryAge(game.ageMs)}</dd></div>{#each context as entry (entry.label)}<div><dt>{entry.label}</dt><dd>{entry.value}</dd></div>{/each}</dl>
    </section>
    <div class="channel-layout">
      <nav class="telemetry-groups" aria-label="Telemetry categories"><button class:chosen={!group} aria-pressed={!group} onclick={()=>chooseGroup('')}>All channels <span>{game.groups.reduce((total,group)=>total+group.total,0)}</span></button>{#each game.groups as category (category.id)}<button class:chosen={group===category.id} aria-pressed={group===category.id} onclick={()=>chooseGroup(category.id)}>{category.label} <span>{category.total}</span></button>{/each}</nav>
      <section class="telemetry-channels panel" aria-label="Game telemetry channels" aria-busy={view.loading}>
        <div class="channel-heading"><h2>{group?game.groups.find(category=>category.id===group)?.label??'Selected category':'All incoming channels'}</h2><span>{game.total} {search?'matching ':''}channels</span></div>
        <div class="channel-table"><table aria-label="Incoming telemetry channel values"><thead><tr><th scope="col">Channel</th><th scope="col">Type / unit</th><th scope="col">Value</th><th scope="col">Status / age</th></tr></thead><tbody>
          {#each game.channels as channel (channel.id)}<tr class:unknown={view.stale||channel.status!=='known'}><th scope="row" title={details(channel)}><code>{channel.id}</code><span>{channel.label}</span>{#if channel.nativeField||channel.nativeId}<small>Native: {channel.nativeField??channel.nativeId}</small>{/if}</th><td>{channel.type}<small>{channel.unit||'Not specified'}</small></td><td class="channel-value">{telemetryValue(channel,view.stale)}</td><td>{telemetryStatus(view.stale?'stale':channel.status)}<small>{telemetryAge(channel.ageMs)}</small></td></tr>
          {:else}<tr><td colspan="4" class="empty-channels">{search||group?'No incoming channels match this selection.':'No incoming channels are available yet. Start a driving or flight session with its game integration configured.'}</td></tr>{/each}
        </tbody></table></div>
        <div class="telemetry-pages"><span>{game.total&&game.channels.length?game.offset+1:0}–{pageEnd} of {game.total}</span><button class="text-button" disabled={!canRead||view.loading||game.offset===0} onclick={()=>offset=Math.max(0,game.offset-limit)}>Previous channels</button><button class="text-button" disabled={!canRead||view.loading||!game.channels.length||pageEnd>=game.total} onclick={()=>offset=pageEnd}>Next channels</button></div>
      </section>
    </div>
    <p class="telemetry-help">An em dash means no current known value. Zero, false, and an empty string remain distinct values. Hover over a channel for its native field and provider context.</p>
  {:else}<div class="telemetry-summary panel"><p role="status">{view.loading?'Reading game telemetry…':view.error?'Game telemetry is unavailable. Refresh to try again.':'Waiting for the first game telemetry capture.'}</p></div>{/if}
</section>

<style>
  .game-telemetry{min-width:0}.telemetry-toolbar{display:flex;align-items:end;gap:14px;flex-wrap:wrap}.telemetry-toolbar label{display:grid;gap:8px;font-size:var(--ui-text-size);color:var(--muted)}.telemetry-toolbar select{max-width:100%;min-width:235px;font-size:var(--ui-text-size)}.telemetry-search{flex:1;min-width:200px}.telemetry-search input{width:100%;font-size:var(--ui-text-size);box-sizing:border-box}.telemetry-toolbar button,.telemetry-pages button{font-size:var(--ui-action-size)}.telemetry-help,.source-message{font-size:var(--ui-text-size);color:var(--muted);line-height:1.8;margin:14px 0 20px}.telemetry-summary{padding:20px;margin:18px 0}.source-heading{display:flex;align-items:start;justify-content:space-between;gap:16px}.source-heading h2,.channel-heading h2{font-size:var(--ui-text-size);margin:0}.source-heading p{font-size:var(--ui-text-size);color:var(--muted);line-height:var(--ui-line-height);margin:7px 0 0;overflow-wrap:anywhere}.source-status{font-size:var(--ui-text-size);color:var(--soft);border:1px solid var(--line);border-radius:5px;padding:6px 10px;white-space:nowrap}.source-status.live{color:#8bbda0;border-color:#8bbda044}.source-status.paused{color:#c9ab7b;border-color:#c9ab7b44}.source-message{margin:12px 0}.source-facts{display:flex;gap:12px 28px;flex-wrap:wrap;margin:18px 0 0}.source-facts div{min-width:90px;max-width:100%}.source-facts dt{font-size:var(--ui-text-size);color:var(--muted);text-transform:capitalize}.source-facts dd{font-size:var(--ui-text-size);margin:7px 0 0;overflow-wrap:anywhere}.channel-layout{display:grid;grid-template-columns:170px minmax(0,1fr);gap:18px;align-items:start}.telemetry-groups{display:grid;gap:5px}.telemetry-groups button{font:inherit;font-size:var(--ui-action-size);display:flex;justify-content:space-between;gap:12px;text-align:left;padding:10px;border:1px solid transparent;background:none;border-radius:5px;color:var(--muted);cursor:pointer}.telemetry-groups button.chosen{border-color:var(--line);background:var(--surface);color:var(--text)}.telemetry-groups span{font-variant-numeric:tabular-nums;opacity:.7}.telemetry-channels{min-width:0;overflow:hidden}.channel-heading{display:flex;align-items:center;justify-content:space-between;gap:14px;padding:18px}.channel-heading>span{font-size:var(--ui-text-size);color:var(--muted)}.channel-table{overflow:auto;max-height:520px}.channel-table table{width:100%;min-width:570px;border-collapse:collapse;text-align:left;font-size:var(--ui-text-size)}.channel-table th,.channel-table td{padding:12px 16px;border-bottom:1px solid var(--line);vertical-align:top;overflow-wrap:anywhere}.channel-table thead th{font-size:var(--ui-text-size);color:var(--muted);font-weight:400;background:var(--surface);position:sticky;top:0;z-index:1}.channel-table tbody th{font-weight:400;min-width:170px;max-width:350px}.channel-table code{display:block;font-size:var(--ui-text-size);color:var(--text);overflow-wrap:anywhere}.channel-table tbody th>span,.channel-table small{display:block;font-size:var(--ui-text-size);color:var(--muted);line-height:var(--ui-line-height);margin-top:5px}.channel-table th>small{font-size:var(--ui-text-size)}.channel-value{font-variant-numeric:tabular-nums;min-width:70px;max-width:200px}.unknown .channel-value{color:var(--muted)}.telemetry-pages{display:flex;align-items:center;justify-content:end;gap:14px;flex-wrap:wrap;padding:14px 18px;font-size:var(--ui-text-size);color:var(--muted)}.telemetry-pages>span{margin-right:auto}.empty-channels{color:var(--muted);line-height:1.8;padding:24px!important}.telemetry-error{font-size:var(--ui-text-size);line-height:1.8;border:1px solid #ad875440;border-radius:5px;padding:12px;color:#c9ab7b}.telemetry-summary>p{font-size:var(--ui-text-size);color:var(--muted);line-height:1.8}
  .telemetry-search input{background:var(--input-bg);border:1px solid var(--line);border-radius:5px;padding:10px 12px;min-height:36px;color:var(--text)}
  @media(max-width:850px){.channel-layout{grid-template-columns:minmax(0,1fr)}.telemetry-groups{display:flex;gap:6px;overflow:auto;padding-bottom:4px}.telemetry-groups button{white-space:nowrap}.telemetry-toolbar select{min-width:0;width:100%}.telemetry-toolbar>label:first-child{flex:1;min-width:210px}.source-heading{flex-wrap:wrap}}
</style>
