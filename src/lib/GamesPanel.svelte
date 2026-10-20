<script lang="ts">
  import Notification from './Notification.svelte';
  import {onMount} from 'svelte';
  import {pickGameExecutable} from './gateway';
  import Icon from './Icon.svelte';
  import GameProviderSettings from './GameProviderSettings.svelte';
  import GameProfileRule from './GameProfileRule.svelte';
  import GameProfileLaunch from './GameProfileLaunch.svelte';
  import type {DeepReadonly,ProfileSummary} from './profiles';
  import {gameContextLabel} from './game-provider-settings';
  import {gameDisplayPath,gameExecutableName,gameIntegrationInstalled,gameIntegrationLabel,gameIntegrationLiveLabel,gameIntegrationNativeVerified,gameIntegrationReady,gameIntegrationSetupBlockedReason,gameIntegrationSetupRequest,gameIntegrationTargetError,gameIntegrationVerificationLabel,gameIntegrationUserFolder,gameCleanupLabel,supportedGames,visibleGames,type Game,type GameId,type GameRequest,type GamesState} from './games';
  let {status,busy,request,profiles=[],profileBlockedReason='',returnLabel,returnToEditor,selectedGameId=$bindable<GameId|''>(''),onNavigate}: {
    status?:GamesState;busy:boolean;request:(request:GameRequest)=>Promise<GamesState|false>;
    profiles?:DeepReadonly<ProfileSummary[]>;
    profileBlockedReason?:string;selectedGameId?:GameId|'';returnLabel?:string;returnToEditor?:()=>void;
    onNavigate?:(gameId:GameId|'')=>void;
  }=$props();
  let response=$state<GamesState>(),initializedGameId:GameId|''='';
  let showAll=$state(false),pending=$state(false),notice=$state(''),executablePath=$state(''),integrationFolder=$state('');
  let providerBusy=$state(false);
  let recoveryDialog:HTMLDialogElement|undefined;
  function attachRecoveryDialog(node:HTMLDialogElement){recoveryDialog=node;return()=>{recoveryDialog=undefined;};}
  const receivedStatus=$derived(status??response);
  const gameState:GamesState=$derived(receivedStatus??{catalogue:supportedGames});
  const games=$derived(visibleGames(gameState.catalogue,showAll));
  const selected=$derived(gameState.catalogue.find(game=>game.id===selectedGameId));
  const selectedExecutablePath=$derived(gameDisplayPath(selected?.executable));
  const selectedLiveLabel=$derived(selected?gameIntegrationLiveLabel(selected):'');
  const cleanup=$derived(gameState.cleanup);
  const occupied=$derived(!!cleanup&&!['idle','failed'].includes(cleanup.phase));
  const setupRunning=$derived(gameState.catalogue.some(game=>game.integrationSetup?.state==='setting-up'));
  const locked=$derived(busy||pending||occupied||setupRunning||providerBusy);
  const integrationInstalled=$derived(selected?gameIntegrationInstalled(selected):false);
  const setupBlocked=$derived(!receivedStatus?'Loading game status…':selected?gameIntegrationSetupBlockedReason(selected,cleanup,profileBlockedReason):'');
  const folderError=$derived(selected?gameIntegrationTargetError(selected,integrationFolder):'');
  const setupDisabled=$derived(locked||!!setupBlocked||!!folderError);
  onMount(()=>{if(!status)void send({command:'game_get_state'});});
  $effect(()=>{if(selected&&initializedGameId!==selected.id)initializeGame(selected);});
  async function send(command:GameRequest):Promise<GamesState|false>{
    const passive=command.command==='game_get_state';
    if(!passive){pending=true;notice='';}
    try{const result=await request(command);if(result){response=result;if(!passive)notice=result.error??'';}else if(!passive)notice='The request did not complete. Check the connection and try again.';return result;}
    catch(error){if(!passive)notice=error instanceof Error?error.message:String(error);return false;}
    finally{if(!passive)pending=false;}
  }
  async function saveSettings(command:GameRequest):Promise<GamesState>{
    const result=await request(command);
    if(!result)throw new Error('Game integration settings were not saved. Check the connection and try again.');
    if(result.error)throw new Error(result.error);
    response=result;return result;
  }
  function initializeGame(game:Game){initializedGameId=game.id;selectedGameId=game.id;integrationFolder=gameIntegrationUserFolder(game);executablePath=game.executable??'';notice='';}
  function chooseGame(game:Game){initializeGame(game);onNavigate?.(game.id);}
  function allGames(){selectedGameId='';notice='';onNavigate?.('');}
  async function browseExecutable(){
    if(!selected||locked)return;const gameId=selected.id,currentPath=selected.executable;pending=true;notice='';
    try{const path=await pickGameExecutable(gameId,currentPath);if(path){const result=await send({command:'game_installation',gameId,executable:path});if(result&&!result.error&&selected?.id===gameId)executablePath=path;}}
    catch(error){notice=error instanceof Error?error.message:String(error);}
    finally{pending=false;}
  }
  async function discoverInstallation(){
    if(!selected||locked)return;const gameId=selected.id;
    const result=await send({command:'game_installation',gameId,executable:null});
    if(result&&!result.error&&selected?.id===gameId)executablePath=result.catalogue.find(game=>game.id===gameId)?.executable??'';
  }
  async function integrationCommand(command:'game_integration_check'|'game_integration_setup'|'game_integration_cancel'|'game_integration_recover'|'game_integration_remove'){
    if(!selected||command==='game_integration_setup'&&setupDisabled)return;await send(gameIntegrationSetupRequest(selected,command,integrationFolder));
  }
  async function keepCurrentSetup(){recoveryDialog?.close();await send({command:'game_abandon',preserveCurrentSetup:true});}
</script>

<section class="games-workspace" aria-label="Games">
  {#if !receivedStatus}<p class="game-status-loading" role="status" aria-live="polite">Loading game status…</p>{:else if !cleanup}<Notification message="Native cleanup metadata is unavailable. Refresh game status before changing provider setup." kind="error" />{:else if cleanup.phase !== 'idle'}
    <div class={['session-banner', { 'session-warning': ['failed', 'recovery-needed'].includes(cleanup.phase) }]} role="status" aria-live="polite">
      <div class="session-icon"><Icon name={'refresh'} size={20} /></div>
      <div class="session-copy"><strong>Previous owned setup recovery · {gameCleanupLabel(cleanup)}</strong><p>Existing saved session only. New launches use unified Profiles.<br />{gameState.catalogue.find(game => game.id === cleanup.gameId)?.name ?? 'Game session'}</p>
        {#if cleanup.error}<Notification message={String(cleanup.error)} kind="error" />{/if}
        {#if cleanup.steps?.length}<details><summary>Session progress</summary><ol>{#each cleanup.steps as step, index (index)}<li>{step}</li>{/each}</ol></details>{/if}
      </div>
      {#if cleanup.phase === 'recovery-needed' || cleanup.phase === 'failed'}<button class="secondary-button" disabled={busy || pending} onclick={() => send({ command: 'game_recover' })}><Icon name="refresh" size={14} /> Recover setup</button>
      {/if}
      {#if cleanup.phase === 'recovery-needed'}<button class="secondary-button" disabled={busy || pending} onclick={() => recoveryDialog?.showModal()}><Icon name="check" size={14} />Keep current setup</button>{/if}
    </div>
  {/if}
  {#if notice || gameState.error}<Notification message={String(notice || gameState.error)} kind="error" />{/if}

  {#if returnToEditor}<button class="text-button back-link" onclick={returnToEditor}><Icon name="back" size={14} /> {returnLabel ?? 'Return to profile'}</button>{/if}
  {#if selected}
    <button class="text-button back-link" onclick={allGames}><Icon name="back" size={14} /> All games</button>
    <div class="game-details-heading">
      <div class={['game-detail-mark', { rally: selected.id === 'assetto-corsa-rally' }]}><Icon name={selected.provider !== 'assetto-corsa-rally' ? 'command' : 'wheel'} size={32} /></div>
      <div><p class="game-kicker">{selected.provider !== 'assetto-corsa-rally' ? 'Flight simulation' : 'Rally simulation'}</p><h2>{selected.name}</h2><p class="detail-subtitle">{selected.installed ? 'Installation found' : 'Installation not found'} · {selected.provider === 'dcs-bios' ? 'DCS-BIOS' : selected.provider === 'x-plane' ? 'X-Plane RREF telemetry' : 'Rally telemetry'}</p></div>
      <div class="detail-launch">{#key selected.id}<GameProfileLaunch game={selected} busy={locked}/>{/key}</div>
    </div>

    <div class="game-content">
        <section class="panel installation-panel"><div class="sidebar-title"><Icon name="search" size={18} /><h3>Installation</h3></div><div class="installation-status-row"><span class={['installation-status', { found: selected.installed }]}><Icon name={selected.installed ? 'check' : 'info'} size={14} />{selected.installed ? 'Found on this computer' : 'Not found'}</span><button class="secondary-button" disabled={locked} onclick={discoverInstallation}><Icon name="search" size={14} />Discover automatically</button></div>
          {#if selected.provider === 'x-plane' && selected.installations?.length}
            <label class="editor-label"><span class="icon-text"><Icon name="search" size={14} />Detected installations</span><select aria-label="Detected X-Plane installations" bind:value={executablePath} disabled={locked}><option value="">Choose an installation</option>{#each selected.installations as install (install.executable)}<option value={install.executable}>{gameDisplayPath(install.installPath)} · {install.source}</option>{/each}</select></label>
          {/if}
          <div class="editor-label"><span id="game-executable-label" class="icon-text"><Icon name="play" size={14} />Game executable</span><div class="executable-row"><button type="button" class="executable-picker" aria-label={`Choose ${selected.name} executable`} aria-describedby="game-executable-label" title={selectedExecutablePath || `Choose ${gameExecutableName(selected.id)}`} disabled={locked} onclick={browseExecutable}><span class:empty={!selectedExecutablePath}>{selectedExecutablePath || `Choose ${gameExecutableName(selected.id)}…`}</span><Icon name="folder" size={14}/></button>{#if selected.source}<span class="installation-source-badge" title={selected.source} aria-label={`Installation source: ${selected.source}`}>{selected.source}</span>{/if}</div></div>
          <p class="field-help">Choose {gameExecutableName(selected.id)} in your installation folder.</p>
          {#if executablePath && executablePath !== selected.executable}<div class="installation-actions"><button class="secondary-button" disabled={locked} onclick={() => send({ command: 'game_installation', gameId: selected.id, executable: executablePath })}><Icon name="check" size={14} />Use this path</button></div>{/if}
          {#if !selected.launchSupported}<p class="platform-note">{selected.launchReason ?? selected.launchUnavailableReason ?? 'Native game launch is unavailable on this platform.'}{#if selected.installed} You can still edit and apply unified profiles from Profiles.{/if}</p>{/if}
        </section>
        {#key selected.id}<GameProfileRule game={selected} busy={busy||pending}/>{/key}
        <section class="panel setup-panel" id="game-integration-settings" aria-label="Game integration setup">
          <div class="integration-heading">
            <div class="sidebar-title"><Icon name="link" size={18} /><h3>Integration setup</h3></div>
            <div class="integration-actions">
              <button class="secondary-button" disabled={locked|| (!!integrationFolder.trim() && !!folderError)} onclick={() => integrationCommand('game_integration_check')}><Icon name="refresh" size={14}/>Refresh state</button>
              {#if selected.integrationSetup?.state === 'setting-up'}
                <button class="secondary-button" disabled={busy||pending} onclick={()=>integrationCommand('game_integration_cancel')}><Icon name="close" size={14}/>Cancel setup</button>
              {:else if selected.integrationSetup?.recoveryNeeded}
                <button class="secondary-button" disabled={locked||!!profileBlockedReason} onclick={()=>integrationCommand('game_integration_recover')}><Icon name="refresh" size={14}/>Recover integration setup</button>
              {:else if integrationInstalled}
                <button class="secondary-button remove-integration" disabled={locked||!!profileBlockedReason||!cleanup||!selected.integrationSetup?.removable} title={selected.integrationSetup?.removalMessage} onclick={()=>integrationCommand('game_integration_remove')}><Icon name="trash" size={14}/>Remove integration</button>
                <button class="secondary-button" disabled={setupDisabled} onclick={()=>integrationCommand('game_integration_setup')}><Icon name="refresh" size={14}/>Repair integration</button>
              {:else}
                <button class="primary-button" disabled={setupDisabled} onclick={()=>integrationCommand('game_integration_setup')}><Icon name="download" size={14}/>Install integration</button>
              {/if}
            </div>
          </div>
          <div class="integration-summary" aria-label="Integration details">
            <div class="integration-name"><Icon name={selected.provider==='x-plane'?'network':'chip'} size={18}/><strong>{selected.id==='dcs'?'DCS-BIOS':selected.provider==='x-plane'?'X-Plane RREF':'Native shared memory'}</strong>{#if selected.id==='dcs'||selected.integrationSetup?.version||selected.provider==='x-plane'}<span class="integration-version">{selected.id==='dcs'?selected.integrationSetup?.version??'0.11.7':selected.provider==='x-plane'?selected.id.endsWith('12')?'12':'11':selected.integrationSetup?.version}</span>{/if}</div>
            <div class={['setup-readiness', { ready: gameIntegrationReady(selected), attention: selected.integrationSetup?.state === 'needs-attention' }]} role="status" aria-live="polite"><Icon name={gameIntegrationReady(selected) ? 'check' : selected.integrationSetup?.state === 'needs-attention' ? 'alert' : 'info'} size={14} /><strong>{gameIntegrationLabel(selected)}</strong></div>
          </div>
          <p class="field-help">{selected.id === 'assetto-corsa-rally' && selected.integrationSetup?.connection?.source === 'synthetic' ? 'Synthetic test telemetry does not verify the native game connection.' : selected.integrationSetup?.message ?? 'Check whether this game is ready to send data to your hardware.'}</p>
          <dl class="provider-readiness" aria-label="Integration readiness">
            <div><dt><Icon name="gamepad" size={14} />Game context</dt><dd>{gameContextLabel(selected)}<small>{selected.integrationContext?.processVerified ? 'Actual game process verified' : 'No verified matching process'}</small></dd></div>
            {#if selectedLiveLabel}<div><dt><Icon name="activity" size={14} />Telemetry freshness</dt><dd>{selectedLiveLabel}</dd></div>{/if}
          </dl>
          <p class="field-help">{selected.integrationContext?.message ?? 'Game discovery identifies an installation. A verified running game and an active matching profile are required for live integration.'}</p>
          {#if selected.integrationSetup?.state==='setting-up'}<div class="setup-progress" role="status" aria-live="polite"><Icon name="refresh" size={14}/><span>{selected.integrationSetup.progress??'Preparing integration…'}</span></div>{:else if selected.integrationSetup?.recoveryNeeded}<p class="setup-warning">Recover the interrupted setup before trying again.</p>{:else if setupBlocked}<p class="field-help">{setupBlocked}</p>{/if}
          {#if integrationInstalled&&selected.integrationSetup?.removalMessage}<p class="field-help">{selected.integrationSetup.removalMessage}</p>{/if}
          {#if selected.provider === 'x-plane'}
            <p class="field-help">RREF telemetry uses the saved UDP connection. Integration does not change game files.</p>
          {:else if selected.id==='assetto-corsa-rally'}
            {#if !gameIntegrationNativeVerified(selected)}<div class="native-verification"><Icon name="info" size={14}/><span>{gameIntegrationVerificationLabel(selected)}</span></div><p class="field-help">Start ACR and enter a driving session, then recheck to verify compatible native telemetry.</p>{/if}
            {#if selected.integrationSetup?.connection?.source === 'shared-memory' && selected.integrationSetup.probeChannels && Object.keys(selected.integrationSetup.probeChannels).length}
              <details class="telemetry-channel-details"><summary><span class="icon-text"><Icon name="search" size={14} />Last native check</span></summary><p>Channel availability from the last compatibility check. Live values appear separately when telemetry is receiving.</p><dl>{#each Object.entries(selected.integrationSetup.probeChannels) as [channel, value] (channel)}<div><dt>{channel.replaceAll('_', ' ')}</dt><dd>{value.status}</dd></div>{/each}</dl></details>
            {/if}
            {#if selected.integrationSetup?.connection?.source === 'shared-memory' && selected.integrationSetup.channels && Object.keys(selected.integrationSetup.channels).length}
              <details class="telemetry-channel-details"><summary><span class="icon-text"><Icon name="activity" size={14} />Telemetry channels</span></summary><dl>{#each Object.entries(selected.integrationSetup.channels) as [channel, value] (channel)}<div><dt>{channel.replaceAll('_', ' ')}</dt><dd>{value.status}{#if typeof value.value === 'number'} · {Number.isInteger(value.value) ? value.value : value.value.toFixed(1)}{:else if typeof value.value === 'string'} · {value.value}{/if}</dd></div>{/each}</dl></details>
            {/if}
            {#if selected.integrationSetup?.connection?.source === 'shared-memory' && selected.integrationSetup.optionalMaxRpm === false}<p class="field-help">Maximum RPM is unavailable in this game build. Other supported telemetry can still work.</p>{/if}
          {/if}
          {#key selected.id}<GameProviderSettings game={selected} busy={busy||pending||occupied||setupRunning||!!profileBlockedReason} bind:userFolder={integrationFolder} request={saveSettings} onBusyChange={value=>{providerBusy=value;}} />{/key}
          {#if selectedLiveLabel}<div class="setup-live"><span class={['live-dot', { receiving: selected.integrationSetup?.live === 'receiving', stale: selected.integrationSetup?.live === 'stale' }]}></span><div><strong>{selectedLiveLabel}</strong><p>{selected.integrationSetup?.liveMessage ?? 'Game data arrives when its integration is running and a mission or session is active.'}</p></div></div>{/if}
        </section>
    </div>
  {:else}
    <div class="page-heading games-toolbar"><div><div class="eyebrow">YOUR GAMES</div><h1>Games</h1><p>{gameState.catalogue.filter(game => game.installed).length} found · {gameState.catalogue.length} supported</p></div><div class="toolbar-controls"><button class="text-button" disabled={locked} onclick={() => send({ command: 'game_scan' })}><Icon name="refresh" size={13} /> Rescan</button><label class="supported-switch"><input type="checkbox" role="switch" bind:checked={showAll} /><span class="switch-track" aria-hidden="true"></span> Show all supported</label></div></div>
    {#if receivedStatus&&!games.length}<div class="games-empty"><div class="empty-icon"><Icon name="search" size={31} /></div><h2>No supported games found</h2><p>Rescan your installations, or show all supported games to select a custom location.</p><div><button class="primary-button" disabled={locked} onclick={() => send({ command: 'game_scan' })}><Icon name="refresh" size={14} /> Rescan games</button><button class="secondary-button" onclick={() => { showAll = true; }}>Show supported games</button></div></div>{/if}
    <div class="game-grid">
      {#each games as game (game.id)}
        {@const liveLabel=gameIntegrationLiveLabel(game)}
        {@const matchingCount=profiles.filter(profile=>profile.gameIds?.includes(game.id)).length}
        <article class={['game-tile', { unavailable: !game.installed, rally: game.id === 'assetto-corsa-rally' }]}>
          <button class="tile-details" aria-label={`Open ${game.name} details${game.installed ? '' : ', installation not found'}`} onclick={() => chooseGame(game)}>
            <div class="game-art"><div class="art-grid"></div><span class="art-category">{game.provider === 'assetto-corsa-rally' ? 'Rally' : 'Flight'}</span>
              {#if game.provider !== 'assetto-corsa-rally'}<svg class="game-vector" viewBox="0 0 340 160" fill="none" aria-hidden="true"><path d="M0 135 340 100M0 155l340-35M30 0v160M100 0v160M170 0v160M240 0v160M310 0v160" stroke="currentColor" opacity=".1"/><circle cx="170" cy="83" r="58" stroke="currentColor" opacity=".16"/><circle cx="170" cy="83" r="38" stroke="currentColor" opacity=".16"/><path d="m170 24 12 41 82 45-2 10-77-22 7 40-22-10-22 10 7-40-77 22-2-10 82-45z" fill="currentColor" fill-opacity=".14" stroke="currentColor" stroke-width="1.4"/><path d="M170 24v104M160 78h20" stroke="currentColor" stroke-width="1.4"/></svg>
              {:else}<svg class="game-vector" viewBox="0 0 340 160" fill="none" aria-hidden="true"><path d="m0 107 65-67 48 45 64-65 90 72 39-33 34 35" stroke="currentColor" opacity=".2"/><path d="M100 160c0-32 131-30 131-50 0-18-104-9-104-25 0-13 50-27 69-39" stroke="currentColor" stroke-width="36" opacity=".08"/><path d="M100 160c0-32 131-30 131-50 0-18-104-9-104-25 0-13 50-27 69-39" stroke="currentColor" stroke-width="1.2" stroke-dasharray="8 7" opacity=".3"/><path d="m139 92 13-19h38l14 19 4 28h-74z" fill="var(--surface)" stroke="currentColor" stroke-width="1.5"/><path d="m151 91 7-12h26l8 12zM139 105h12M192 105h11M150 118h43" stroke="currentColor" stroke-width="1.5"/><path d="M140 122v8h10v-8M192 122v8h10v-8" stroke="currentColor" stroke-width="3"/></svg>{/if}
              <div class="game-wordmark">{game.provider === 'x-plane' ? 'X-PLANE' : game.id === 'dcs' ? 'DCS' : 'ACR'}<span>{game.provider === 'x-plane' ? game.id.slice(-2) : game.id === 'dcs' ? 'WORLD' : 'RALLY'}</span></div>
            </div>
            <div class="tile-copy">
              <div class="tile-title"><h2>{game.name}</h2><Icon name="chevron" size={16} /></div>
              <p>{game.provider === 'x-plane' ? 'Flight telemetry · Indicators & displays' : game.id === 'dcs' ? 'Aircraft controls · DCS-BIOS feedback' : 'Rally telemetry · Rev lights & displays'}</p>
              <div class="tile-footer">
                <span class="tile-status"><span></span>{game.installed ? 'Installed' : 'Not found'}</span>
                <span class={['tile-integration', { ready: gameIntegrationReady(game), attention: game.integrationSetup?.state === 'needs-attention' }]}><Icon name={gameIntegrationReady(game) ? 'check' : game.integrationSetup?.state === 'needs-attention' ? 'alert' : 'link'} size={12} /><span>{gameIntegrationLabel(game)}</span></span>
              </div>
              {#if game.installed&&liveLabel}<div class="tile-live"><span class={['live-dot', { receiving: game.integrationSetup?.live === 'receiving', stale: game.integrationSetup?.live === 'stale' }]}></span>{liveLabel}</div>{/if}
            </div>
          </button>
          <div class="tile-launch"><span class="tile-profile-count">{matchingCount} matching {matchingCount===1?'profile':'profiles'}</span>{#if game.installed}<GameProfileLaunch {game} busy={locked} compact/>{/if}</div>
        </article>
      {/each}
    </div>
  {/if}
</section>

<dialog class="launch-review" {@attach attachRecoveryDialog} aria-labelledby="recovery-review-title">
  <div class="review-icon"><Icon name="alert" size={22} /></div>
  <p class="game-kicker">Pending recovery</p>
  <h2 id="recovery-review-title">Keep current setup?</h2>
  <p class="review-description">This preexisting game-session record still needs explicit recovery.</p>
  <p class="review-warning">Keeping the current setup releases game session ownership and archives the recovery record. Your hardware and integration settings stay as they are.</p>
  <div class="review-actions"><button class="secondary-button" disabled={busy || pending} onclick={() => recoveryDialog?.close()}><Icon name="close" size={14} />Cancel</button><button class="primary-button" disabled={busy || pending} onclick={keepCurrentSetup}><Icon name="check" size={14} />Keep current setup</button></div>
</dialog>

<style>
  .game-content {min-width:0;font-size:var(--game-detail-text-size);line-height:var(--game-detail-line-height);}
  .game-content .field-help {font-size:var(--game-detail-text-size);line-height:var(--game-detail-line-height);overflow-wrap:anywhere;}
  .game-content button, .game-content select {font-size:var(--game-detail-text-size);line-height:var(--game-detail-line-height);}
  .game-content select {max-width:100%;min-width:0;}
  .icon-text {display:inline-flex;align-items:center;gap:7px;}
  .icon-text :global(svg), .sidebar-title :global(svg), .provider-readiness :global(svg), .installation-status :global(svg), .setup-readiness :global(svg), .native-verification :global(svg) {flex-shrink:0;}
  .game-content>.setup-panel {margin-top:0;margin-bottom:23px;}
  .provider-readiness {display:grid;gap:12px;margin:18px 0;padding:14px;border:1px solid var(--line);border-radius:6px;}
  .provider-readiness>div {display:grid;gap:5px;}
  .provider-readiness dt {display:flex;align-items:center;gap:7px;font-size:var(--game-detail-text-size);color:var(--muted);}
  .provider-readiness dd {margin:0;color:var(--soft);font-size:var(--game-detail-text-size);line-height:var(--game-detail-line-height);overflow-wrap:anywhere;}
  .provider-readiness small {display:block;color:var(--muted);font-size:var(--game-detail-text-size);}
  .launch-review { color: var(--text); background: var(--surface); border: 1px solid var(--line); border-radius: 12px; width: 430px; max-width: calc(100% - 32px); padding: 28px; box-shadow: 0 20px 80px #0005; }
  .launch-review::backdrop { background: #0008; }
  .review-icon { color: var(--accent); background: var(--accent-bg); display: grid; place-items: center; width: 46px; height: 46px; border-radius: 11px; margin-bottom: 19px; }
  .launch-review h2 {font-size:var(--game-detail-heading-size,14px);line-height:var(--game-detail-line-height,1.6);letter-spacing:-.3px;margin-top:8px;}
  .review-description, .review-warning {font-size:var(--game-detail-text-size,12px);color:var(--muted);line-height:var(--game-detail-line-height,1.6);margin-top:14px;}
  .review-warning { color: #c5a26e; padding: 13px; background: #b18b550a; border: 1px solid #b18b5540; border-radius: 6px; }
  .review-actions { display: flex; justify-content: end; gap: 10px; margin-top: 23px; }
  .review-actions button {font-size:var(--game-detail-text-size,12px);line-height:var(--game-detail-line-height,1.6);}
  .games-workspace {--game-detail-text-size:12px;--game-detail-heading-size:14px;--game-detail-line-height:1.6;padding-bottom:30px;}
  .game-status-loading {margin-bottom:20px;color:var(--muted);font-size:var(--game-detail-text-size);line-height:var(--game-detail-line-height);}
  button { display: inline-flex; align-items: center; justify-content: center; gap: 7px; }
  .games-toolbar { display: flex; align-items: center; justify-content: space-between; gap: 20px; margin-bottom: 27px; }
  .game-kicker {color:var(--muted);font-size:var(--game-detail-text-size,12px);line-height:var(--game-detail-line-height,1.6);letter-spacing:1.3px;text-transform:uppercase;font-weight:600;}
  .toolbar-controls { display: flex; align-items: center; gap: 21px; }
  .supported-switch { display: flex; gap: 8px; align-items: center; color: var(--soft); font-size: 10px; cursor: pointer; position: relative; }
  .supported-switch input { position: absolute; opacity: 0; width: 30px; height: 20px; margin: 0; }
  .switch-track { width: 28px; height: 16px; border: 1px solid var(--line); background: var(--surface-raised); border-radius: 12px; display: block; transition: background .15s; }
  .switch-track::after { content: ''; display: block; height: 10px; width: 10px; margin: 2px; border-radius: 50%; background: var(--muted); transition: transform .15s; }
  .supported-switch input:checked + .switch-track { background: var(--accent-bg); border-color: var(--accent); }
  .supported-switch input:checked + .switch-track::after { background: var(--accent); transform: translateX(12px); }
  .supported-switch input:focus-visible + .switch-track { outline: 2px solid var(--accent); outline-offset: 4px; }
  .game-grid { display: grid; grid-template-columns: repeat(auto-fill, minmax(280px, 1fr)); gap: 20px; max-width: 980px; }
  .game-tile { display: flex; flex-direction: column; position: relative; border: 1px solid var(--line); border-radius: 9px; background: var(--surface); overflow: hidden; transition: border-color .2s, transform .2s; }
  .game-tile:hover, .game-tile:focus-within { border-color: var(--accent); }
  .tile-details { display: block; width: 100%; padding: 0; text-align: left; color: var(--text); background: transparent; }
  .tile-details:focus-visible { outline-offset: -4px; }
  .game-art { height: 216px; background: linear-gradient(145deg, var(--surface-raised), var(--bg)); position: relative; overflow: hidden; color: #a1c5d7; }
  .rally .game-art { color: #c8b292; }
  .art-grid { position: absolute; inset: 0; background: radial-gradient(ellipse at 50% 110%, #6ca4c220, transparent 70%); }
  .rally .art-grid { background: radial-gradient(ellipse at 50% 110%, #cfa86420, transparent 70%); }
  .art-category { position: absolute; top: 17px; left: 19px; font-size: 8px; text-transform: uppercase; letter-spacing: 1.5px; color: var(--muted); }
  .game-vector { position: absolute; width: 100%; height: 160px; bottom: 36px; }
  .game-wordmark { position: absolute; bottom: 14px; left: 22px; font-size: 34px; line-height: 1; font-weight: 750; letter-spacing: 4px; color: var(--text); }
  .game-wordmark span { display: block; font-size: 8px; letter-spacing: 6px; font-weight: 500; margin-top: 5px; }
  .tile-copy { padding: 20px; }
  .tile-title { display: flex; justify-content: space-between; align-items: center; }
  .tile-title h2 { font-size: 15px; font-weight: 600; letter-spacing: -.2px; }
  .tile-title :global(svg) { color: var(--muted); }
  .tile-copy p { color: var(--muted); font-size: 10px; margin-top: 9px; line-height: 1.5; }
  .tile-footer { border-top: 1px solid var(--faint-line); margin-top: 19px; padding-top: 14px; display: flex; align-items: center; justify-content: space-between; color: var(--muted); font-size: 9px; gap: 12px; }
  .tile-status { display: flex; align-items: center; gap: 6px; color: var(--accent); flex-shrink: 0; white-space: nowrap; }
  .tile-status span { width: 5px; height: 5px; border-radius: 50%; background: currentColor; }
  .tile-integration, .tile-live { display: flex; align-items: center; gap: 6px; font-size: 9px; color: var(--muted); }
  .tile-integration { min-width: 0; margin-left: auto; justify-content: flex-end; text-align: right; line-height: 1.5; }
  .tile-integration :global(svg) { flex-shrink: 0; }
  .tile-integration.ready { color: var(--accent); }
  .tile-integration.attention { color: #c5a26e; }
  .tile-live { margin-top: 8px; }
  .live-dot { width: 5px; height: 5px; flex-shrink: 0; border-radius: 50%; background: var(--muted); }
  .live-dot.receiving { background: var(--accent); }
  .live-dot.stale { background: #c5a26e; }
  .tile-launch {display:grid;grid-template-columns:auto minmax(0,1fr);align-items:start;gap:12px;margin-top:auto;padding:0 20px 18px;}
  .tile-profile-count {padding-top:11px;color:var(--muted);font-size:9px;line-height:1.5;white-space:nowrap;}

  .unavailable .game-art { opacity: .42; filter: grayscale(1); }
  .unavailable .tile-copy { opacity: .5; }
  .unavailable .tile-status { color: var(--muted); }
  .unavailable:hover, .unavailable:focus-within { border-color: var(--soft); }
  .games-empty { padding: 65px 25px; border: 1px dashed var(--line); border-radius: 9px; text-align: center; }
  .empty-icon { color: var(--accent); background: var(--accent-bg); width: 64px; height: 64px; margin: 0 auto 21px; border-radius: 15px; display: grid; place-items: center; }
  .games-empty h2 { font-size: 19px; letter-spacing: -.3px; }
  .games-empty p { color: var(--muted); font-size: 11px; margin: 12px auto 22px; max-width: 360px; line-height: 1.8; }
  .games-empty > div:last-child { display: flex; justify-content: center; gap: 10px; flex-wrap: wrap; }
  .back-link {margin-bottom:23px;padding-left:0;font-size:var(--game-detail-text-size);}
  .game-details-heading { display: flex; gap: 17px; align-items: center; margin-bottom: 27px; }
  .game-detail-mark {color:#a1c5d7;width:65px;height:65px;flex-shrink:0;border-radius:12px;border:1px solid var(--line);background:var(--surface-raised);display:grid;place-items:center;}
  .game-detail-mark.rally { color: #c8b292; }
  .game-details-heading>div:not(.game-detail-mark):not(.detail-launch) {min-width:0;}
  .game-details-heading h2 {font-size:24px;letter-spacing:-.5px;margin-top:7px;overflow-wrap:anywhere;}
  .detail-subtitle {font-size:var(--game-detail-text-size);line-height:var(--game-detail-line-height);color:var(--muted);margin-top:8px;}
  .detail-launch { margin-left: auto; }
  h3 {font-size:var(--game-detail-heading-size);line-height:var(--game-detail-line-height);font-weight:600;}
  .editor-label {display:flex;flex-direction:column;gap:8px;color:var(--soft);font-size:var(--game-detail-text-size);}
  .editor-label { margin-top: 16px; }
  .executable-row {display:flex;align-items:center;gap:10px;min-width:0;}
  .executable-row>.executable-picker {flex:1;}
  .installation-source-badge {display:inline-block;flex-shrink:0;max-width:40%;overflow:hidden;text-overflow:ellipsis;white-space:nowrap;padding:3px 8px;border:1px solid var(--line);border-radius:999px;background:var(--surface-raised);color:var(--soft);font-size:var(--game-detail-text-size);line-height:var(--game-detail-line-height);}
  .executable-picker {width:100%;min-width:0;justify-content:space-between;text-align:left;padding:11px 12px;border:1px solid var(--line);border-radius:5px;background:var(--bg);color:var(--soft);}
  .executable-picker>span {min-width:0;overflow:hidden;text-overflow:ellipsis;white-space:nowrap;font-family:ui-monospace,monospace;font-size:var(--game-detail-text-size);}
  .executable-picker>span.empty {font-family:inherit;color:var(--muted);}
  .executable-picker :global(svg) {flex-shrink:0;color:var(--muted);}
  .executable-picker:not(:disabled):hover {border-color:var(--accent);}
  .installation-panel { padding: 21px; margin-bottom:20px; }
  .sidebar-title { display: flex; gap: 9px; align-items: center; color: var(--soft); }
  .installation-status-row {display:flex;flex-wrap:wrap;align-items:center;justify-content:space-between;gap:10px;margin-top:20px;}
  .installation-status-row>button {flex-shrink:0;margin-left:auto;padding:8px 10px;}
  .installation-status {display:flex;align-items:center;gap:7px;color:var(--muted);font-size:var(--game-detail-text-size);}
  .installation-status.found { color: var(--accent); }
  .installation-actions { display: flex; gap: 12px; align-items: center; margin-top: 14px; }
  .installation-actions button {font-size:var(--game-detail-text-size);}
  .platform-note {color:#c5a26e;font-size:var(--game-detail-text-size);line-height:var(--game-detail-line-height);margin-top:17px;}
  .setup-panel { margin-top: 20px; padding: 21px; }
  .integration-heading {display:flex;align-items:center;justify-content:space-between;flex-wrap:wrap;gap:16px;}
  .integration-actions {display:flex;align-items:center;justify-content:flex-end;flex-wrap:wrap;gap:8px;margin-left:auto;}
  .integration-actions button {white-space:normal;}
  .remove-integration {color:#e39b98;border-color:#b967674d;}
  .remove-integration:hover:enabled {border-color:#e39b98;color:#efb2ae;}
  .integration-summary {display:flex;align-items:center;justify-content:space-between;flex-wrap:wrap;gap:12px;margin-top:20px;}
  .integration-name {display:flex;align-items:center;flex-wrap:wrap;gap:8px;min-width:0;color:var(--soft);}
  .integration-name :global(svg) {flex-shrink:0;color:var(--muted);}
  .integration-version {padding:2px 8px;border:1px solid var(--line);border-radius:999px;background:var(--surface-raised);color:var(--soft);font-size:var(--game-detail-text-size);}
  .setup-readiness {display:flex;align-items:center;gap:7px;color:var(--muted);font-size:var(--game-detail-text-size);line-height:var(--game-detail-line-height);}
  .setup-readiness.ready { color: var(--accent); }
  .setup-readiness.attention, .setup-warning { color: #c5a26e; }
  .native-verification {display:flex;align-items:center;gap:7px;font-size:var(--game-detail-text-size);color:#c5a26e;margin-top:15px;}
  .telemetry-channel-details {font-size:var(--game-detail-text-size);color:var(--muted);margin-top:15px;}
  .telemetry-channel-details p {font-size:var(--game-detail-text-size);line-height:var(--game-detail-line-height);margin-top:10px;}
  .telemetry-channel-details summary { cursor: pointer; color: var(--soft); }
  .telemetry-channel-details dl { margin-top: 10px; }
  .telemetry-channel-details dl > div {display:flex;justify-content:space-between;gap:10px;line-height:var(--game-detail-line-height);padding:4px 0;overflow-wrap:anywhere;}
  .telemetry-channel-details dl > div>* {min-width:0;}
  .telemetry-channel-details dt { text-transform: capitalize; }
  .telemetry-channel-details dd { text-align: right; margin: 0; }
  .setup-warning {font-size:var(--game-detail-text-size);line-height:var(--game-detail-line-height);margin-top:12px;}
  .setup-progress {display:flex;align-items:center;gap:8px;color:var(--accent);font-size:var(--game-detail-text-size);line-height:var(--game-detail-line-height);margin-top:15px;}
  .setup-live { border-top: 1px solid var(--line); padding-top: 17px; margin-top: 15px; display: flex; align-items: start; gap: 8px; }
  .setup-live .live-dot { margin-top: 4px; }
  .setup-live strong {font-size:var(--game-detail-text-size);font-weight:500;color:var(--soft);}
  .setup-live p {font-size:var(--game-detail-text-size);color:var(--muted);line-height:var(--game-detail-line-height);margin-top:7px;}
  .session-banner { display: flex; align-items: center; gap: 13px; padding: 17px; background: var(--accent-bg); border: 1px solid var(--line); border-radius: 8px; margin-bottom: 22px; }
  .session-icon { color: var(--accent); }
  .session-copy { flex: 1; min-width: 0; }
  .session-copy strong { font-size: 12px; }
  .session-copy p {color:var(--muted);font-size:var(--game-detail-text-size);margin-top:6px;line-height:var(--game-detail-line-height);}
  .session-copy details {color:var(--soft);font-size:var(--game-detail-text-size);line-height:var(--game-detail-line-height);margin-top:12px;}
  .session-banner>button {font-size:var(--game-detail-text-size);line-height:var(--game-detail-line-height);}
  .session-copy summary { cursor: pointer; }
  .session-copy li { padding: 3px 0; }
  .session-warning { background: #b18b550a; border-color: #b18b5560; }
  .session-warning .session-icon { color: #c5a26e; }
  @media (hover: none) { .tile-launch {padding:0 20px 18px;} }
  @media (max-width: 600px) { .games-toolbar { align-items: start; }
  .toolbar-controls { align-items: end; flex-direction: column-reverse; gap: 12px; }
  .game-grid { grid-template-columns: 1fr; }
  .game-details-heading { flex-wrap: wrap; }
  .game-details-heading h2 {font-size:24px;}
  .game-detail-mark { width: 50px; height: 50px; }
  .detail-launch { margin-left: 0; width: 100%; }
  .session-banner { flex-wrap: wrap; }
  .session-banner > button { margin-left: 33px; } }
  @media (prefers-reduced-motion: reduce) { .game-tile, .tile-launch, .switch-track, .switch-track::after { transition: none; } }
</style>
