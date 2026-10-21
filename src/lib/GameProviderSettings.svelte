<script lang="ts">
  import {onDestroy,untrack} from 'svelte';
  import Icon from './Icon.svelte';
  import {pickGameIntegrationFolder} from './gateway';
  import {notify} from './notifications';
  import {gameDisplayPath,gameIntegrationUserFolder,type Game,type GameRequest,type GamesState} from './games';
  import {prepareProviderSettingsSave,providerSettingsDraft,providerSettingsEqual,providerSettingsError,rebaseProviderSettingsDraft,settleProviderSettingsSave,type DcsConnection,type ProviderSettingsDraft,type XPlaneConnection} from './game-provider-settings';
  let {game,busy,userFolder=$bindable(''),request,onBusyChange}:{game:Game;busy:boolean;userFolder?:string;request:(request:GameRequest)=>Promise<GamesState|false>;onBusyChange?:(busy:boolean)=>void}=$props();
  function settingsForGame(value:Game):ProviderSettingsDraft{return {...providerSettingsDraft(value),userFolder:value.id==='dcs'?gameIntegrationUserFolder(value):''};}
  let base=$state<ProviderSettingsDraft>(settingsForGame(untrack(()=>game))),draft=$state<ProviderSettingsDraft>(settingsForGame(untrack(()=>game)));
  let saving=$state(false),picking=$state(false),attempted=$state(false);
  let queued:ProviderSettingsDraft|null=null,disposed=false,epoch=0,contextId=untrack(()=>game.id);
  const error=$derived(attempted?providerSettingsError(draft):'');
  const dcs=$derived(draft.gameId==='dcs'?draft.connection as DcsConnection:null);
  const xplane=$derived(draft.gameId.startsWith('x-plane-')?draft.connection as XPlaneConnection:null);
  function reportBusy(){onBusyChange?.(saving||picking);}
  onDestroy(()=>{disposed=true;epoch++;queued=null;onBusyChange?.(false);});
  function syncSettings(){
    const saved=settingsForGame(game);
    if(contextId!==game.id){contextId=game.id;epoch++;queued=null;saving=false;picking=false;attempted=false;draft=saved;base=saved;reportBusy();return;}
    if(!saving&&saved.baseRevision!==base.baseRevision){draft=rebaseProviderSettingsDraft(draft,base,saved);if(queued)queued=rebaseProviderSettingsDraft(queued,base,saved);base=saved;}
  }
  $effect(syncSettings);
  $effect(()=>{if(game.id==='dcs')userFolder=draft.userFolder;});
  $effect(()=>{if(!busy&&!saving&&!picking)void flush();});
  function commit(){
    if(disposed||busy||picking)return;
    syncSettings();attempted=true;
    queued=providerSettingsError(draft)||!saving&&providerSettingsEqual(draft,base)?null:structuredClone($state.snapshot(draft));
    void flush();
  }
  async function flush(){
    if(disposed||saving||picking||busy||!queued)return;
    syncSettings();const submitted=queued;queued=null;
    if(providerSettingsEqual(submitted,base))return;
    let captured;try{captured=prepareProviderSettingsSave(submitted);}catch{return;}
    const requestEpoch=epoch,gameId=game.id;saving=true;reportBusy();
    try{
      const result=await request(captured.request);
      if(disposed||requestEpoch!==epoch||game.id!==gameId)return;
      if(!result||result.error)throw new Error(result&&result.error||'Game settings were not saved. Check the connection and try again.');
      const updated=result.catalogue.find(item=>item.id===gameId);
      if(!updated)throw new Error('The response did not include this game. Refresh before changing settings again.');
      const returned=settingsForGame(updated),remote=settingsForGame(game);
      const newerRemote=remote.baseRevision!==captured.baseRevision&&remote.baseRevision!==returned.baseRevision;
      const next=newerRemote?remote:returned;
      if(newerRemote)draft=rebaseProviderSettingsDraft(draft,base,next);
      else draft=settleProviderSettingsSave(draft,captured,updated);
      if(queued)queued=rebaseProviderSettingsDraft(queued,captured.submitted,next);
      base=next;
    }catch(cause){if(!disposed&&requestEpoch===epoch&&game.id===gameId){queued=null;notify(cause instanceof Error?cause.message:String(cause),'error');}}
    finally{if(!disposed&&requestEpoch===epoch){saving=false;reportBusy();if(queued)void flush();}}
  }
  async function browseFolder(kind:'saved-games'|'catalogue'){
    if(disposed||busy||saving||picking||!dcs)return;
    const requestEpoch=epoch,gameId=game.id,current=kind==='saved-games'?draft.userFolder:dcs.cataloguePath;picking=true;reportBusy();
    try{const path=await pickGameIntegrationFolder(kind,current);if(disposed||requestEpoch!==epoch||game.id!==gameId||!path)return;if(kind==='saved-games')draft.userFolder=path;else (draft.connection as DcsConnection).cataloguePath=path;}
    catch(cause){if(!disposed&&requestEpoch===epoch&&game.id===gameId)notify(cause instanceof Error?cause.message:String(cause),'error');}
    finally{if(!disposed&&requestEpoch===epoch){picking=false;reportBusy();if(kind==='saved-games'?draft.userFolder!==current:(draft.connection as DcsConnection).cataloguePath!==current)commit();}}
  }
</script>

<section class="provider-settings" aria-label={`${game.name} provider settings`}>
  {#if dcs}
    <section class="settings-section" aria-label="DCS-BIOS locations">
      <h3><Icon name="folder" size={18}/>DCS-BIOS locations</h3>
      {#if (game.integrationSetup?.candidates?.length??0)>1}<label><span class="provider-field-label"><Icon name="folder" size={14}/>Detected Saved Games folders</span><select aria-label="Detected DCS Saved Games folders" disabled={busy||picking} value={game.integrationSetup!.candidates!.includes(draft.userFolder)?draft.userFolder:''} onchange={event=>{draft.userFolder=event.currentTarget.value;commit();}}><option value="">Choose a folder</option>{#each game.integrationSetup!.candidates! as folder (folder)}<option value={folder}>{gameDisplayPath(folder)}</option>{/each}</select></label>{/if}
      <div class="path-field"><span class="provider-field-label"><Icon name="folder" size={14}/>DCS Saved Games folder</span><button class="path-picker" type="button" aria-label="DCS Saved Games folder" title={gameDisplayPath(draft.userFolder)||'Choose the DCS Saved Games folder'} disabled={busy||saving||picking} onclick={()=>browseFolder('saved-games')}><span class:placeholder={!draft.userFolder}>{gameDisplayPath(draft.userFolder)||'Choose Saved Games folder…'}</span><Icon name="folder" size={16}/></button></div>
      <div class="path-field"><span class="provider-field-label"><Icon name="folder" size={14}/>DCS-BIOS doc/json directory</span><button class="path-picker" type="button" aria-label="DCS-BIOS doc/json directory" title={gameDisplayPath(dcs.cataloguePath)||'Choose the installed doc/json directory'} disabled={busy||saving||picking} onclick={()=>browseFolder('catalogue')}><span class:placeholder={!dcs.cataloguePath}>{gameDisplayPath(dcs.cataloguePath)||'Choose doc/json directory…'}</span><Icon name="folder" size={16}/></button></div>
      <p class="help" aria-label="Automatic DCS-BIOS module">DCS-BIOS follows the loaded aircraft automatically. Active module: <strong>{game.integrationSetup?.activeAircraftModule??'Waiting for aircraft'}</strong>. Aircraft tabs browse their own controls and telemetry; Base uses shared flight telemetry.</p>
    </section>
    <section class="settings-section aircraft-hooks" aria-label="Aircraft mod export hooks">
      <h3><Icon name="command" size={18}/>Aircraft mod export hooks</h3>
      {#if game.integrationSetup?.aircraftExports?.length}
        <table aria-label="Aircraft mod export hooks">
          <thead><tr><th scope="col">Name</th><th scope="col">Available</th><th scope="col">Status</th></tr></thead>
          <tbody>{#each game.integrationSetup.aircraftExports as hook (hook.aircraft)}
            <tr><th scope="row">{hook.aircraft==='OH-6A'?'OH-6A Cayuse':hook.aircraft}</th><td>{hook.capability==='common-data-only'?'Common flight telemetry':'Controls and cockpit instruments'}</td><td><span class="hook-status" class:installed={hook.installed} class:attention={hook.detected&&!hook.installed} title={hook.message}><Icon name={hook.installed?'check':hook.detected?'alert':'info'} size={14}/><span>{hook.installed?'Installed':!hook.detected?'Not detected':hook.compatible?'Not installed':'Needs review'}</span></span></td></tr>
          {/each}</tbody>
        </table>
      {:else}<p class="help">No aircraft mod export hooks detected.</p>{/if}
    </section>
  {/if}
  <section class="settings-section" aria-label="Network and timing">
    <div class="settings-heading"><h3><Icon name="network" size={18}/>Network and timing</h3>{#if saving}<span class="saving" role="status"><Icon name="refresh" size={14}/>Saving…</span>{/if}</div>
    <div class="fields">
      {#if dcs}
        <label><span class="provider-field-label"><Icon name="command" size={14}/>Command host</span><input class="text-input" aria-label="Command host" disabled={busy} bind:value={dcs.commandHost} onchange={commit}/></label>
        <label><span class="provider-field-label"><Icon name="command" size={14}/>Command UDP port</span><input class="text-input" aria-label="Command UDP port" disabled={busy} type="number" min="1" max="65535" bind:value={dcs.commandPort} onchange={commit}/></label>
        <label><span class="provider-field-label"><Icon name="network" size={14}/>Listen address</span><input class="text-input" aria-label="Listen address" disabled={busy} bind:value={dcs.listenAddress} onchange={commit}/></label>
        <label><span class="provider-field-label"><Icon name="network" size={14}/>Listen UDP port</span><input class="text-input" aria-label="Listen UDP port" disabled={busy} type="number" min="1" max="65535" bind:value={dcs.listenPort} onchange={commit}/></label>
        <label><span class="provider-field-label"><Icon name="network" size={14}/>Multicast group</span><input class="text-input" aria-label="Multicast group" disabled={busy} bind:value={dcs.multicastGroup} placeholder="Empty for unicast" onchange={commit}/></label>
        <label><span class="provider-field-label"><Icon name="network" size={14}/>Multicast interface</span><input class="text-input" aria-label="Multicast interface" disabled={busy} bind:value={dcs.multicastInterface} onchange={commit}/></label>
      {:else if xplane}
        <label><span class="provider-field-label"><Icon name="network" size={14}/>Simulator host</span><input class="text-input" aria-label="Simulator host" disabled={busy} bind:value={xplane.host} onchange={commit}/></label>
        <label><span class="provider-field-label"><Icon name="network" size={14}/>Simulator UDP port</span><input class="text-input" aria-label="Simulator UDP port" disabled={busy} type="number" min="1" max="65535" bind:value={xplane.port} onchange={commit}/></label>
        <label><span class="provider-field-label"><Icon name="network" size={14}/>Listen UDP port</span><input class="text-input" aria-label="Listen UDP port" disabled={busy} type="number" min="0" max="65535" bind:value={xplane.listenPort} onchange={commit}/></label>
        <label><span class="provider-field-label"><Icon name="activity" size={14}/>Updates per second</span><input class="text-input" aria-label="Updates per second" disabled={busy} type="number" min="1" max="20" bind:value={xplane.updateHz} onchange={commit}/></label>
      {/if}
      <label><span class="provider-field-label"><Icon name="activity" size={14}/>Telemetry freshness (ms)</span><input class="text-input" aria-label="Telemetry freshness (ms)" disabled={busy} type="number" min="100" max="10000" bind:value={draft.connection.freshnessMs} onchange={commit}/></label>
    </div>
    {#if xplane}<p class="help">X-Plane {xplane.version} uses RREF telemetry on a local IPv4 loopback host (127.x.x.x). A listen port of 0 chooses an available local port.</p>{/if}
  </section>
  {#if error}<p class="warning" role="alert"><Icon name="alert" size={14}/><span>{error}</span></p>{/if}
</section>

<style>
  .provider-settings{display:grid;gap:20px;min-width:0;border-top:1px solid var(--line);margin-top:20px;padding-top:20px;font-size:var(--game-detail-text-size,12px);line-height:var(--game-detail-line-height,1.6)}
  .settings-section{display:grid;gap:12px;min-width:0}.settings-heading{display:flex;justify-content:space-between;align-items:center;gap:10px;min-width:0}h3{display:flex;align-items:center;gap:8px;font-size:var(--game-detail-heading-size,14px);font-weight:600;line-height:var(--game-detail-line-height,1.6);margin:0}p{margin:0}
  label,.path-field{display:grid;gap:7px;min-width:0;font-size:var(--game-detail-text-size,12px);color:var(--muted)}.provider-field-label{display:flex;align-items:center;gap:6px}input,select{min-width:0;width:100%;max-width:100%;font-size:var(--game-detail-text-size,12px);line-height:var(--game-detail-line-height,1.6)}
  .path-picker{display:flex;align-items:center;justify-content:space-between;gap:10px;width:100%;min-width:0;padding:10px 12px;border:1px solid var(--line);border-radius:5px;background:var(--bg);color:var(--text);text-align:left;font-size:var(--game-detail-text-size,12px);line-height:var(--game-detail-line-height,1.6);cursor:pointer}.path-picker>span{overflow:hidden;text-overflow:ellipsis;white-space:nowrap;min-width:0}.path-picker:hover:not(:disabled){border-color:var(--accent)}.path-picker:focus-visible{outline:2px solid var(--accent);outline-offset:2px}.path-picker:disabled{cursor:default;opacity:.5}.placeholder{color:var(--muted)}
  .aircraft-hooks table{width:100%;table-layout:fixed;border-collapse:collapse;font-size:var(--game-detail-text-size,12px);line-height:var(--game-detail-line-height,1.6);text-align:left}
  .aircraft-hooks th,.aircraft-hooks td{padding:10px 12px;vertical-align:top;overflow-wrap:anywhere;border-bottom:1px solid var(--line)}
  .aircraft-hooks thead th{color:var(--muted);font-weight:500;background:var(--surface-raised)}
  .aircraft-hooks thead th:first-child{width:22%}.aircraft-hooks thead th:nth-child(2){width:34%}
  .aircraft-hooks tbody th{color:var(--soft);font-weight:500}.aircraft-hooks td{color:var(--muted)}
  .hook-status{display:inline-flex;align-items:center;gap:5px;max-width:100%;padding:2px 7px;border:1px solid var(--line);border-radius:999px;background:var(--surface-raised);color:var(--soft);line-height:1.5}.hook-status>span{min-width:0;overflow-wrap:normal}.hook-status.installed{color:var(--accent);border-color:color-mix(in srgb,var(--accent) 35%,var(--line));background:var(--accent-bg)}.hook-status.attention{color:var(--warn,#e2b76a);border-color:color-mix(in srgb,var(--warn,#e2b76a) 35%,var(--line));background:color-mix(in srgb,var(--warn,#e2b76a) 8%,var(--surface))}
  @media(max-width:650px){.aircraft-hooks th,.aircraft-hooks td{padding:8px 5px}}
  .fields{display:grid;grid-template-columns:repeat(2,minmax(0,1fr));gap:12px}.help{font-size:var(--game-detail-text-size,12px);color:var(--muted);line-height:var(--game-detail-line-height,1.6);overflow-wrap:anywhere}.saving{display:inline-flex;align-items:center;gap:6px;color:var(--muted);white-space:nowrap}.warning{display:flex;align-items:flex-start;gap:7px;color:var(--warn,#e2b76a)}.warning span{min-width:0;overflow-wrap:anywhere}.provider-settings :global(svg){flex-shrink:0}@media(max-width:650px){.fields{grid-template-columns:1fr}}
</style>
