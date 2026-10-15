<script lang="ts">
  import { onMount, tick } from 'svelte';
  import { gatewayRequest } from './gateway';
  import { savedAssignments, type MappingResult, type MappingProfile } from './mappings';
  import Icon from './Icon.svelte';
  import GameConfigEditor from './GameConfigEditor.svelte';
  import type { DcsTarget } from './dcs-profile';
  import type { IntegrationConfig } from './integration-profile';
  import type { RuntimeConfig } from './runtime-config';
  import { cloneGameProfile, defaultGameProfile, gameDeviceKey, gameIntegrationLabel, gameIntegrationLiveLabel, gameIntegrationNativeVerified, gameIntegrationReady, gameIntegrationSetupBlockedReason, gameIntegrationSetupRequest, gameIntegrationTargetError, gameIntegrationVerificationLabel, gameIntegrationUserFolder, gameProfileEqual, gameProfileError, gameSessionLabel, newGameProfile, profileChangesBindings, supportedGames, visibleGames, type Game, type GameId, type GameProfile, type GameRequest, type GamesState } from './games';

  let mappingChoices = $state<MappingProfile[]>([]);
  let mappingChoice = $state('');
  let visibilityMode = $state('hidhide-session');
  let { status, busy, request, targets, integration = null, integrationActive = false, readConfig, openIntegrations }: {
    status?: GamesState; busy: boolean; request: (request: GameRequest) => Promise<GamesState | false>;
    targets: DcsTarget[]; integration?: IntegrationConfig | null; integrationActive?: boolean;
    readConfig: (gatewayId: string, moduleId: string) => Promise<RuntimeConfig | null>; openIntegrations: () => void;
  } = $props();
  let response = $state<GamesState>();
  let showAll = $state(false), selectedGameId = $state<GameId | ''>(''), draft = $state<GameProfile | null>(null);
  let pending = $state(false), notice = $state(''), executablePath = $state(''), addTargetKey = $state(''), acceptBindings = $state(false);
  let deleting = $state(false), integrationFolder = $state(''), integrationFolderEdited = $state(false);
  let launchDialog: HTMLDialogElement | undefined;
  let recoveryDialog: HTMLDialogElement | undefined;
  let reviewProfile = $state<GameProfile | null>(null);
  function attachLaunchDialog(node: HTMLDialogElement) { launchDialog = node; return () => { launchDialog = undefined; }; }
  function attachRecoveryDialog(node: HTMLDialogElement) { recoveryDialog = node; return () => { recoveryDialog = undefined; }; }
  const gameState: GamesState = $derived(status ?? response ?? { catalogue: supportedGames, profiles: [], defaultProfiles: {}, session: { phase: 'idle' as const } });
  const games = $derived(visibleGames(gameState.catalogue, showAll));
  const selected = $derived(gameState.catalogue.find(game => game.id === selectedGameId));
  const profiles = $derived(gameState.profiles.filter(profile => profile.gameId === selectedGameId));
  const saved = $derived(gameState.profiles.find(profile => profile.id === draft?.id));
  const dirty = $derived(!!draft && !gameProfileEqual(draft, saved));
  const error = $derived(draft ? gameProfileError(draft) : '');
  const changesBindings = $derived(!!draft && profileChangesBindings(draft));
  const occupied = $derived(!['idle', 'failed'].includes(gameState.session.phase));
  const setupRunning = $derived(gameState.catalogue.some(game => game.integrationSetup?.state === 'setting-up'));
  const locked = $derived(busy || pending || occupied || setupRunning);
  const setupBlocked = $derived(selected ? gameIntegrationSetupBlockedReason(selected, gameState.session, integrationActive) : '');
  const folderError = $derived(selected ? gameIntegrationTargetError(selected, integrationFolder) : '');
  const setupDisabled = $derived(locked || !!setupBlocked || !!folderError);
  $effect(() => { if (selected && !integrationFolderEdited) integrationFolder = gameIntegrationUserFolder(selected); });
  const compatibleIntegration = $derived(!!selected && integration?.document.provider === selected.provider && (selected.provider !== 'x-plane' || integration.connection.version === Number(selected.id.slice(-2))));
  const availableTargets = $derived(targets.filter(target => !draft?.devices.some(device => gameDeviceKey(device) === gameDeviceKey({ gatewayId: target.gatewayId, moduleId: target.module.id }))));
  const canActivate = $derived(!!selected?.installed && !!draft && !!saved && !dirty && !error && !locked && (!changesBindings || acceptBindings));
  const canLaunch = $derived(canActivate && !!selected?.installed && !!selected?.launchSupported);

  onMount(() => { if (!status) void send({ command: 'game_get_state' }); });

  async function send(command: GameRequest): Promise<GamesState | false> {
    pending = true; notice = '';
    try {
      const result = await request(command);
      if (result) { response = result; notice = result.error ?? ''; }
      else notice = 'The request did not complete. Check the connection and try again.';
      return result;
    } catch (error) { notice = error instanceof Error ? error.message : String(error); return false; }
    finally { pending = false; }
  }
  function chooseProfile(profile: GameProfile) { draft = cloneGameProfile(profile); acceptBindings = false; deleting = false; notice = ''; }
  function chooseGame(game: Game) {
    selectedGameId = game.id; integrationFolderEdited = false; integrationFolder = gameIntegrationUserFolder(game); executablePath = game.executable ?? ''; addTargetKey = '';
    chooseProfile(defaultGameProfile(gameState, game.id) ?? newGameProfile(game.id, gameState.profiles));
  }
  async function integrationCommand(command: 'game_integration_check' | 'game_integration_setup' | 'game_integration_cancel' | 'game_integration_recover') {
    if (!selected) return;
    if (command === 'game_integration_setup' && setupDisabled) return;
    await send(gameIntegrationSetupRequest(selected, command, integrationFolder));
  }
  function createProfile() { if (selected) chooseProfile(newGameProfile(selected.id, gameState.profiles)); }
  async function saveProfile(): Promise<boolean> {
    if (!draft || error) return false;
    const profile = cloneGameProfile(draft);
    const result = await send({ command: 'game_profile_save', gameProfile: profile });
    if (!result || result.error) return false;
    chooseProfile(result.profiles.find(item => item.id === profile.id) ?? profile);
    notice = 'Profile saved.'; return true;
  }
  async function deleteProfile() {
    if (!draft) return;
    const result = await send({ command: 'game_profile_delete', profileId: draft.id });
    if (result && !result.error && selected) chooseProfile(defaultGameProfile(result, selected.id) ?? newGameProfile(selected.id, result.profiles));
  }
  async function launchTile(game: Game) {
    const profile = defaultGameProfile(gameState, game.id);
    if (!profile) {
      chooseGame(game); notice = 'Save your first profile, then launch the game.';
      return;
    }
    if (profileChangesBindings(profile)) {
      reviewProfile = cloneGameProfile(profile);
      await tick(); launchDialog?.showModal();
      return;
    }
    await send({ command: 'game_launch', gameId: game.id, profileId: profile.id });
  }
  async function confirmTileLaunch() {
    const profile = reviewProfile;
    launchDialog?.close();
    if (profile) await send({ command: 'game_launch', gameId: profile.gameId, profileId: profile.id, acceptBindingChanges: true });
  }
  async function keepCurrentSetup() {
    recoveryDialog?.close();
    await send({ command: 'game_abandon', preserveCurrentSetup: true });
  }
  async function activate(launch: boolean) {
    if (!selected || !draft || !canActivate) return;
    await send({ command: launch ? 'game_launch' : 'game_activate', gameId: selected.id, profileId: draft.id, acceptBindingChanges: acceptBindings });
  }
  function addPeripheral() {
    const target = targets.find(item => gameDeviceKey({ gatewayId: item.gatewayId, moduleId: item.module.id }) === addTargetKey);
    if (!target || !draft) return;
    draft.devices.push({ gatewayId: target.gatewayId, moduleId: target.module.id, controller: 'keep' });
    addTargetKey = ''; acceptBindings = false;
  }
  async function capture(gatewayId: string, moduleId: string) {
    pending = true; notice = '';
    const profileId = draft?.id;
    try {
      const config = await readConfig(gatewayId, moduleId);
      if (draft?.id !== profileId) return;
      const device = draft?.devices.find(item => item.gatewayId === gatewayId && item.moduleId === moduleId);
      if (config && device) { device.config = JSON.parse(JSON.stringify(config)); acceptBindings = false; }
      else notice = 'Could not read this peripheral. Reconnect it and capture its settings again.';
    } catch (error) { notice = error instanceof Error ? error.message : String(error); }
    finally { pending = false; }
  }
  function copyIntegration() {
    if (draft && integration && compatibleIntegration) { draft.integration = JSON.parse(JSON.stringify(integration)); notice = 'Integration copied into this profile. Save to keep this snapshot.'; }
  }
  async function loadMappings() {
    pending=true;notice='';
    try { const list:MappingProfile[]=[];let offset:number|null=0;while(offset!==null){const result:MappingResult|undefined=(await gatewayRequest({command:'mapping_profiles',offset})).mappings;for(const id of result?.ids ?? []){const profile=(await gatewayRequest({command:'mapping_export',profileId:id})).mappings?.document;if(profile)list.push(profile);}offset=typeof result?.next==='number'?result.next:null;}mappingChoices=list; }
    catch(error){notice=String(error);}finally{pending=false;}
  }
  async function captureMapping() {
    if(!draft || !mappingChoice)return;pending=true;notice='';const id=draft.id;
    try {
      const profile=mappingChoices.find(p=>p.id===mappingChoice);if(!profile)throw new Error('Choose a saved mapping.');
      const assignments=await savedAssignments(profile.id,async value=>(await gatewayRequest(value)).mappings ?? {});
      const visibility=profile.requirements.deviceHiding ? {mode:visibilityMode,sourceIds:[...new Set(assignments.filter(a=>a.source.origin==='host-controller').map(a=>a.source.id))]} : null;
      const capture=(await gatewayRequest({command:'mapping_capture',profileId:mappingChoice,visibility})).mappings?.capture;
      if(!capture)throw new Error('Mapping capture is unavailable.');
      if(draft?.id===id){draft.schemaVersion=2;draft.mapping=capture;notice='Mapping captured in schema 2. Save this profile to keep the snapshot.';}
    }catch(error){notice=String(error);}finally{pending=false;}
  }
  async function configureIntegration() { if (!dirty || await saveProfile()) openIntegrations(); }
</script>

<section class="games-workspace" aria-label="Games">
  {#if gameState.session.phase !== 'idle'}
    <div class={['session-banner', { 'session-warning': ['failed', 'recovery-needed'].includes(gameState.session.phase) }]} role="status" aria-live="polite">
      <div class="session-icon"><Icon name={gameState.session.phase === 'running' ? 'play' : 'refresh'} size={20} /></div>
      <div class="session-copy"><strong>{gameSessionLabel(gameState.session)}</strong><p>{gameState.catalogue.find(game => game.id === gameState.session.gameId)?.name ?? 'Game session'}{#if gameState.session.pid} · Process {gameState.session.pid}{/if}</p>
        {#if gameState.session.error}<p class="session-error">{gameState.session.error}</p>{/if}
        {#if gameState.session.bindingWarning}<p class="session-error">{typeof gameState.session.bindingWarning === 'string' ? gameState.session.bindingWarning : 'Controller changes can reconnect USB and affect game bindings.'}</p>{/if}
        {#if gameState.session.steps?.length}<details><summary>Session progress</summary><ol>{#each gameState.session.steps as step, index (index)}<li>{step}</li>{/each}</ol></details>{/if}
      </div>
      {#if gameState.session.phase === 'recovery-needed' || gameState.session.phase === 'failed'}<button class="secondary-button" disabled={busy || pending} onclick={() => send({ command: 'game_recover' })}><Icon name="refresh" size={14} /> Recover setup</button>
      {:else if ['validating', 'applying', 'launching', 'waiting', 'running'].includes(gameState.session.phase)}<button class="secondary-button" disabled={busy || pending} onclick={() => send({ command: 'game_deactivate' })}><Icon name="stop" size={14} /> {gameState.session.phase === 'running' ? 'Restore setup' : 'Cancel activation'}</button>{/if}
      {#if gameState.session.phase === 'recovery-needed'}<button class="secondary-button" disabled={busy || pending} onclick={() => recoveryDialog?.showModal()}>Keep current setup</button>{/if}
    </div>
  {/if}
  {#if notice || gameState.error}<p class="games-notice" role="status">{notice || gameState.error}</p>{/if}

  {#if selected}
    <button class="text-button back-link" onclick={() => { selectedGameId = ''; notice = ''; }}><span aria-hidden="true">←</span> All games</button>
    <div class="game-details-heading">
      <div class={['game-detail-mark', { rally: selected.id === 'assetto-corsa-rally' }]}><Icon name={selected.provider !== 'assetto-corsa-rally' ? 'command' : 'wheel'} size={32} /></div>
      <div><p class="game-kicker">{selected.provider !== 'assetto-corsa-rally' ? 'Flight simulation' : 'Rally simulation'}</p><h2>{selected.name}</h2><p class="detail-subtitle">{selected.installed ? 'Installation found' : 'Installation not found'} · {selected.provider === 'dcs-bios' ? 'DCS-BIOS' : selected.provider === 'x-plane' ? 'X-Plane RREF telemetry' : 'Rally telemetry'}</p></div>
      <button class="primary-button detail-launch" disabled={!canLaunch} onclick={() => activate(true)}><Icon name="play" size={15} /> Launch</button>
    </div>

    <div class="games-detail-layout">
      <div class="profile-editor panel">
        <header class="editor-heading"><div><h3>Game profile</h3><p>Your peripherals and integration, ready together.</p></div><span class={['small-status', { changed: dirty }]}>{dirty ? 'Unsaved changes' : 'Saved'}</span></header>
        <div class="editor-body">
          <div class="profile-picker"><label>Profile<select aria-label="Select game profile" value={draft?.id ?? ''} disabled={locked} onchange={event => { const profile = profiles.find(item => item.id === event.currentTarget.value); if (profile) chooseProfile(profile); }}>
            {#if draft && !saved}<option value={draft.id}>New · {draft.name}</option>{/if}
            {#each profiles as profile (profile.id)}<option value={profile.id}>{profile.name}{gameState.defaultProfiles[selected.id] === profile.id ? ' · Default' : ''}</option>{/each}
          </select></label><button class="secondary-button" disabled={locked} onclick={createProfile}><Icon name="plus" size={14} /> New profile</button></div>
          {#if draft}
            <label class="editor-label">Profile name<input class="text-input" bind:value={draft.name} disabled={locked} maxlength="80" /></label>
            <div class="profile-actions"><button class="text-button" disabled={locked || !saved || dirty || gameState.defaultProfiles[selected.id] === draft.id} onclick={() => send({ command: 'game_profile_default', gameId: selected.id, profileId: draft?.id })}><Icon name="check" size={12} /> {gameState.defaultProfiles[selected.id] === draft.id ? 'Default launch profile' : 'Make default'}</button><button class="text-button danger-link" disabled={locked || !saved || gameState.defaultProfiles[selected.id] === draft.id} title={gameState.defaultProfiles[selected.id] === draft.id ? 'Choose another default profile before deleting this one.' : 'Delete this profile'} onclick={() => { deleting = !deleting; }}>Delete profile</button></div>
            {#if deleting}<div class="delete-confirm"><p>Delete “{draft.name}”? Its saved settings will be removed.</p><button class="secondary-button" disabled={locked} onclick={() => { deleting = false; }}>Keep profile</button><button class="secondary-button" disabled={locked} onclick={deleteProfile}>Delete</button></div>{/if}

            <div class="section-heading"><Icon name="devices" size={17} /><div><h3>Peripherals</h3><p>Choose controller visibility and optional configuration overrides.</p></div><span>{draft.devices.length}</span></div>
            {#if !draft.devices.length}<p class="section-empty">Your current peripheral setup stays in place. Add a peripheral to customize it for this game.</p>{/if}
            {#each draft.devices as device, index (gameDeviceKey(device))}
              {@const target = targets.find(item => item.gatewayId === device.gatewayId && item.module.id === device.moduleId)}
              <section class="game-peripheral" aria-label={target?.label ?? device.moduleId}>
                <div class="peripheral-heading"><div><strong>{target?.label ?? device.config?.name ?? 'Saved peripheral'}</strong><p>{device.gatewayId} · {device.moduleId}</p></div><span class={['small-status', { changed: !target?.module.online }]}>{target?.module.online ? 'Online' : 'Offline'}</span><button class="icon-button" aria-label={`Remove ${target?.label ?? device.moduleId} from profile`} disabled={locked} onclick={() => { draft?.devices.splice(index, 1); acceptBindings = false; }}><Icon name="close" size={15} /></button></div>
                <div class="peripheral-controls"><label>Controller<select aria-label={`Controller mode for ${target?.label ?? device.moduleId}`} bind:value={device.controller} disabled={locked} onchange={() => { acceptBindings = false; }}><option value="keep">Keep current state</option><option value="enabled">Enable</option><option value="disabled">Disable</option></select></label><button class="secondary-button" disabled={locked || !target?.module.online || !!target?.busy} onclick={() => capture(device.gatewayId, device.moduleId)}><Icon name="copy" size={13} /> {device.config ? 'Recapture settings' : 'Capture settings'}</button></div>
                {#if device.config}<GameConfigEditor bind:config={device.config} disabled={locked} /><button class="text-button remove-override" disabled={locked} onclick={() => { delete device.config; acceptBindings = false; }}>Remove configuration override</button>{/if}
              </section>
            {/each}
            <div class="add-peripheral"><select aria-label="Peripheral to add" bind:value={addTargetKey} disabled={locked || !availableTargets.length}><option value="">{availableTargets.length ? 'Choose a peripheral…' : 'No additional peripherals found'}</option>{#each availableTargets as target (gameDeviceKey({ gatewayId: target.gatewayId, moduleId: target.module.id }))}<option value={gameDeviceKey({ gatewayId: target.gatewayId, moduleId: target.module.id })}>{target.label} · {target.gatewayId}</option>{/each}</select><button class="secondary-button" disabled={locked || !addTargetKey || draft.devices.length >= 32} onclick={addPeripheral}><Icon name="plus" size={14} /> Add</button></div>

            <div class="section-heading"><Icon name="link" size={17} /><div><h3>Integration</h3><p>Game state updates delivered to your hardware.</p></div></div>
            {#if draft.integration}<div class="integration-summary"><span class="integration-dot"></span><div><strong>{draft.integration.document.name}</strong><p>{draft.integration.document.provider === 'dcs-bios' ? 'DCS-BIOS' : draft.integration.document.provider === 'x-plane' ? 'X-Plane RREF' : 'Rally telemetry'} · {draft.integration.document.inputs.length} inputs · {draft.integration.document.outputs.length} outputs</p></div><button class="icon-button" aria-label="Remove integration from game profile" disabled={locked} onclick={() => { if (draft) draft.integration = null; }}><Icon name="close" size={15} /></button></div><p class="field-help">This profile stores its own copy of the integration settings and peripheral assignments. Later changes in Integrations are copied only when you choose Update.</p>
              {#if draft.integration.document.provider === 'assetto-corsa-rally'}<label class="editor-label">Telemetry source<select aria-label="Game profile Rally telemetry source" bind:value={draft.integration.connection.source} disabled={locked}><option value="shared-memory">Live game · shared memory</option><option value="synthetic">Synthetic test telemetry</option></select></label><p class="field-help">{draft.integration.connection.source === 'shared-memory' ? 'Launch automatically connects telemetry to the game process on Windows.' : 'Synthetic mode uses test telemetry from Integrations. Choose live game for hardware updates while playing.'}</p>{/if}
            {:else}<p class="section-empty">No integration selected. HID controllers still work with the selected peripheral settings.</p>{/if}
            <div class="integration-actions"><button class="secondary-button" disabled={locked || !compatibleIntegration} onclick={copyIntegration}><Icon name="copy" size={13} /> {draft.integration ? 'Update from saved integration' : 'Use saved integration'}</button><button class="text-button" disabled={locked || (!!error && dirty)} onclick={configureIntegration}>{dirty ? 'Save and open integrations' : 'Open integrations'} <Icon name="arrow" size={12} /></button></div>
            {#if !compatibleIntegration}<p class="field-help">Configure and save a {selected.provider === 'dcs-bios' ? 'DCS-BIOS' : selected.provider === 'x-plane' ? 'X-Plane RREF telemetry' : 'Rally telemetry'} integration, then return here to attach it.</p>{/if}

            <section class="captured-mapping"><h3>Captured mapping</h3>
            <p class="field-help">Capture upgrades this profile to schema 2. Later mapping edits do not change this snapshot. Virtual devices and hiding are requested independently and must pass qualification before activation.</p>
            {#if draft.mapping}<p>{draft.mapping.profile.name} · {draft.mapping.profile.requirements.virtualDevices ? 'Virtual output required' : 'Recording output'} · {draft.mapping.profile.requirements.deviceHiding ? 'Hiding required' : 'No hiding'}</p><button class="text-button" disabled={locked} onclick={()=>{if(draft)draft.mapping=null;}}>Remove captured mapping</button>{/if}
            <button class="secondary-button" disabled={locked} onclick={loadMappings}>Load saved mappings</button>
            <label class="editor-label">Mapping<select bind:value={mappingChoice} disabled={locked}><option value="">Choose mapping</option>{#each mappingChoices as mapping}<option value={mapping.id}>{mapping.name}</option>{/each}</select></label>
            {#if mappingChoices.find(p=>p.id===mappingChoice)?.requirements.deviceHiding}<label class="editor-label">Requested visibility<select bind:value={visibilityMode} disabled={locked}><option value="hidhide-session">Windows HidHide session</option><option value="linux-grab">Linux input suppression</option><option value="macos-seize">macOS input suppression</option></select></label><p class="field-help">Save an explicit visibility policy for each mapped controller in Devices before capturing it here. Linux isolated launch is available separately in Devices.</p>{/if}
            <button class="secondary-button" disabled={locked || !mappingChoice} onclick={captureMapping}>Capture mapping and upgrade profile</button></section>

            {#if changesBindings}<label class="binding-warning"><input type="checkbox" bind:checked={acceptBindings} disabled={locked} /><span>Controller and configuration changes can reconnect USB and affect game bindings. I have reviewed these changes.</span></label>{/if}
            {#if error}<p class="profile-error" role="alert">{error}</p>{/if}
            <footer class="editor-footer"><button class="text-button" disabled={locked || !dirty} onclick={() => chooseProfile(saved ?? newGameProfile(selected.id, gameState.profiles))}>Discard changes</button><button class="secondary-button" disabled={!canActivate} onclick={() => activate(false)}>Activate profile</button><button class="primary-button" disabled={locked || !dirty || !!error} onclick={saveProfile}><Icon name="save" size={14} /> Save profile</button></footer>
            {#if dirty}<p class="field-help">Save your changes before activating or launching this profile.</p>{/if}
          {/if}
        </div>
      </div>

      <aside class="game-sidebar">
        <section class="panel installation-panel"><div class="sidebar-title"><Icon name="search" size={17} /><h3>Installation</h3></div><span class={['installation-status', { found: selected.installed }]}><span></span>{selected.installed ? 'Found on this computer' : 'Not found'}</span>
          {#if selected.source}<p class="field-help">Source: {selected.source}</p>{/if}
          {#if selected.provider === 'x-plane' && selected.installations?.length}
            <label class="editor-label">Detected installations<select aria-label="Detected X-Plane installations" bind:value={executablePath} disabled={locked}><option value="">Choose an installation</option>{#each selected.installations as install (install.executable)}<option value={install.executable}>{install.installPath} · {install.source}</option>{/each}</select></label>
          {/if}
          <label class="editor-label">Game executable<input class="text-input" aria-label="Game executable" bind:value={executablePath} disabled={locked} placeholder={selected.id === 'dcs' ? 'C:\\…\\DCS World\\bin\\DCS.exe' : 'C:\\…\\acr.exe'} /></label>
          <p class="field-help">Choose the executable in your installation folder for custom locations.</p>
          <div class="installation-actions"><button class="secondary-button" disabled={locked || !executablePath.trim()} onclick={() => send({ command: 'game_installation', gameId: selected.id, executable: executablePath.trim() })}>Use this path</button><button class="text-button" disabled={locked} onclick={() => send({ command: 'game_scan' })}><Icon name="refresh" size={12} /> Rescan</button></div>
          {#if selected.executable}<button class="text-button automatic-discovery" disabled={locked} onclick={() => send({ command: 'game_installation', gameId: selected.id, executable: null })}>Use automatic discovery</button>{/if}
          {#if !selected.launchSupported}<p class="platform-note">{selected.launchReason ?? selected.launchUnavailableReason ?? 'Native game launch is unavailable on this platform.'}{#if selected.installed} You can still prepare and activate profiles.{/if}</p>{/if}
        </section>
        <section class="panel setup-panel" aria-label="Game integration setup">
          <div class="sidebar-title"><Icon name="link" size={17} /><h3>Integration setup</h3></div>
          <div class={['setup-readiness', { ready: gameIntegrationReady(selected), attention: selected.integrationSetup?.state === 'needs-attention' }]} role="status" aria-live="polite"><Icon name={gameIntegrationReady(selected) ? 'check' : selected.integrationSetup?.state === 'needs-attention' ? 'alert' : 'info'} size={15} /><strong>{gameIntegrationLabel(selected)}</strong></div>
          <p class="field-help">{selected.id === 'assetto-corsa-rally' && selected.integrationSetup?.connection?.source === 'synthetic' ? 'Synthetic test telemetry does not verify the native game connection.' : selected.integrationSetup?.message ?? 'Check whether this game is ready to send data to your hardware.'}</p>
          {#if selected.id === 'dcs'}
            <p class="setup-version">DCS-BIOS <strong>{selected.integrationSetup?.version ?? '0.11.7'}</strong></p>
            {#if selected.integrationSetup?.candidates?.length}
              <label class="editor-label">Detected Saved Games folders<select aria-label="Detected DCS Saved Games folders" value={selected.integrationSetup.candidates.includes(integrationFolder) ? integrationFolder : ''} disabled={locked || integrationActive} onchange={event => { integrationFolder = event.currentTarget.value; integrationFolderEdited = true; }}><option value="">Choose a folder or enter a path below</option>{#each selected.integrationSetup.candidates as folder (folder)}<option value={folder}>{folder}</option>{/each}</select></label>
            {/if}
            <label class="editor-label">DCS Saved Games folder<input class="text-input" aria-label="DCS Saved Games folder" bind:value={integrationFolder} disabled={locked || integrationActive} oninput={() => { integrationFolderEdited = true; }} placeholder="C:\Users\…\Saved Games\DCS" /></label>
            <p class="field-help">Set up installs DCS-BIOS and connects its export script here. Existing scripts are backed up.</p>
            {#if folderError && (integrationFolder || (selected.integrationSetup?.candidates?.length ?? 0) > 1)}<p class="setup-warning">{folderError}</p>{/if}
          {:else if selected.provider === 'x-plane'}
            <p class="setup-version">RREF UDP · X-Plane {selected.id.endsWith('12') ? '12' : '11'}</p>
            <p class="field-help">Set up integration preserves local UDP settings and changes no game files. Configure host, ports and update rate in Integrations, then capture that profile here. Launch uses this exact installation on Windows. Live simulator and hardware qualification is pending.</p>
            {#if selected.integrationSetup?.connection}<p class="field-help">{String(selected.integrationSetup.connection.host)}:{String(selected.integrationSetup.connection.port)} · {String(selected.integrationSetup.connection.updateHz)} updates/sec</p>{/if}
          {:else}
            <p class="setup-version">{selected.integrationSetup?.connection?.source === 'synthetic' ? 'Synthetic test telemetry' : 'Native shared memory'}{#if selected.integrationSetup?.version && selected.integrationSetup.connection?.source !== 'synthetic'} · Build <strong>{selected.integrationSetup.version}</strong>{/if}</p>
            <p class="field-help">Set up configures Cockpit to read the running game's telemetry. Cockpit connects automatically when you launch a game profile.</p>
            <div class={['native-verification', { verified: gameIntegrationNativeVerified(selected) }]}><Icon name={gameIntegrationNativeVerified(selected) ? 'check' : 'info'} size={13} /><span>{gameIntegrationVerificationLabel(selected)}</span></div>
            {#if !gameIntegrationNativeVerified(selected)}<p class="field-help">Start ACR and enter a driving session, then recheck to verify compatible native telemetry.</p>{/if}
            {#if selected.integrationSetup?.connection?.source === 'shared-memory' && selected.integrationSetup.probeChannels && Object.keys(selected.integrationSetup.probeChannels).length}
              <details class="telemetry-channel-details"><summary>Last native check</summary><p>Channel availability from the last compatibility check. Live values appear separately when telemetry is receiving.</p><dl>{#each Object.entries(selected.integrationSetup.probeChannels) as [channel, value] (channel)}<div><dt>{channel.replaceAll('_', ' ')}</dt><dd>{value.status}</dd></div>{/each}</dl></details>
            {/if}
            {#if selected.integrationSetup?.connection?.source === 'shared-memory' && selected.integrationSetup.channels && Object.keys(selected.integrationSetup.channels).length}
              <details class="telemetry-channel-details"><summary>Telemetry channels</summary><dl>{#each Object.entries(selected.integrationSetup.channels) as [channel, value] (channel)}<div><dt>{channel.replaceAll('_', ' ')}</dt><dd>{value.status}{#if typeof value.value === 'number'} · {Number.isInteger(value.value) ? value.value : value.value.toFixed(1)}{:else if typeof value.value === 'string'} · {value.value}{/if}</dd></div>{/each}</dl></details>
            {/if}
            {#if selected.integrationSetup?.connection?.source === 'shared-memory' && selected.integrationSetup.optionalMaxRpm === false}<p class="field-help">Maximum RPM is unavailable in this game build. Other supported telemetry can still work.</p>{/if}
          {/if}
          {#if selected.integrationSetup?.state === 'setting-up'}
            <div class="setup-progress" role="status" aria-live="polite"><Icon name="refresh" size={14} /><span>{selected.integrationSetup.progress ?? 'Preparing integration…'}</span></div>
            <button class="secondary-button setup-main-action" disabled={busy || pending} onclick={() => integrationCommand('game_integration_cancel')}>Cancel setup</button>
          {:else if selected.integrationSetup?.recoveryNeeded}
            <p class="setup-warning">Recover the interrupted setup before trying again.</p>
            <button class="secondary-button setup-main-action" disabled={locked || integrationActive} onclick={() => integrationCommand('game_integration_recover')}><Icon name="refresh" size={14} /> Recover integration setup</button>
          {:else}
            <button class="primary-button setup-main-action" disabled={setupDisabled} onclick={() => integrationCommand('game_integration_setup')}><Icon name={selected.integrationSetup?.state === 'ready' || selected.integrationSetup?.state === 'needs-attention' ? 'refresh' : selected.id === 'dcs' ? 'download' : 'link'} size={14} /> {selected.integrationSetup?.state === 'ready' || selected.integrationSetup?.state === 'needs-attention' ? 'Repair integration' : selected.id === 'dcs' ? 'Set up integration' : 'Set up telemetry'}</button>
            {#if setupBlocked}<p class="field-help">{setupBlocked}</p>{/if}
          {/if}
          <button class="text-button setup-recheck" disabled={busy || pending || setupRunning || (!!integrationFolder.trim() && !!folderError)} onclick={() => integrationCommand('game_integration_check')}><Icon name="refresh" size={12} /> Recheck readiness</button>
          <div class="setup-live"><span class={['live-dot', { receiving: selected.integrationSetup?.live === 'receiving', stale: selected.integrationSetup?.live === 'stale' }]}></span><div><strong>{gameIntegrationLiveLabel(selected)}</strong><p>{selected.integrationSetup?.liveMessage ?? 'Game data arrives when its integration is running and a mission or session is active.'}</p></div></div>
        </section>
        <section class="session-explainer"><Icon name="zap" size={20} /><h3>One click, your setup</h3><p>Launching activates the selected profile, starts its integration and opens the game.</p><p>When the game exits, Cockpit restores the previous peripheral and integration setup.</p><p>Keep Cockpit open while the game profile is active.</p></section>
      </aside>
    </div>
  {:else}
    <div class="page-heading games-toolbar"><div><div class="eyebrow">YOUR GAMES</div><h1>Games</h1><p>{gameState.catalogue.filter(game => game.installed).length} found · {gameState.catalogue.length} supported</p></div><div class="toolbar-controls"><button class="text-button" disabled={locked} onclick={() => send({ command: 'game_scan' })}><Icon name="refresh" size={13} /> Rescan</button><label class="supported-switch"><input type="checkbox" role="switch" bind:checked={showAll} /><span class="switch-track" aria-hidden="true"></span> Show all supported</label></div></div>
    {#if !games.length}<div class="games-empty"><div class="empty-icon"><Icon name="search" size={31} /></div><h2>No supported games found</h2><p>Rescan your installations, or show all supported games to select a custom location.</p><div><button class="primary-button" disabled={locked} onclick={() => send({ command: 'game_scan' })}><Icon name="refresh" size={14} /> Rescan games</button><button class="secondary-button" onclick={() => { showAll = true; }}>Show supported games</button></div></div>{/if}
    <div class="game-grid">
      {#each games as game (game.id)}
        {@const profile = defaultGameProfile(gameState, game.id)}
        <article class={['game-tile', { unavailable: !game.installed, rally: game.id === 'assetto-corsa-rally' }]}>
          <button class="tile-details" aria-label={`Open ${game.name} details${game.installed ? '' : ', installation not found'}`} onclick={() => chooseGame(game)}>
            <div class="game-art"><div class="art-grid"></div><span class="art-category">{game.id === 'dcs' ? 'Flight' : 'Rally'}</span>
              {#if game.id === 'dcs'}<svg class="game-vector" viewBox="0 0 340 160" fill="none" aria-hidden="true"><path d="M0 135 340 100M0 155l340-35M30 0v160M100 0v160M170 0v160M240 0v160M310 0v160" stroke="currentColor" opacity=".1"/><circle cx="170" cy="83" r="58" stroke="currentColor" opacity=".16"/><circle cx="170" cy="83" r="38" stroke="currentColor" opacity=".16"/><path d="m170 24 12 41 82 45-2 10-77-22 7 40-22-10-22 10 7-40-77 22-2-10 82-45z" fill="currentColor" fill-opacity=".14" stroke="currentColor" stroke-width="1.4"/><path d="M170 24v104M160 78h20" stroke="currentColor" stroke-width="1.4"/></svg>
              {:else}<svg class="game-vector" viewBox="0 0 340 160" fill="none" aria-hidden="true"><path d="m0 107 65-67 48 45 64-65 90 72 39-33 34 35" stroke="currentColor" opacity=".2"/><path d="M100 160c0-32 131-30 131-50 0-18-104-9-104-25 0-13 50-27 69-39" stroke="currentColor" stroke-width="36" opacity=".08"/><path d="M100 160c0-32 131-30 131-50 0-18-104-9-104-25 0-13 50-27 69-39" stroke="currentColor" stroke-width="1.2" stroke-dasharray="8 7" opacity=".3"/><path d="m139 92 13-19h38l14 19 4 28h-74z" fill="var(--surface)" stroke="currentColor" stroke-width="1.5"/><path d="m151 91 7-12h26l8 12zM139 105h12M192 105h11M150 118h43" stroke="currentColor" stroke-width="1.5"/><path d="M140 122v8h10v-8M192 122v8h10v-8" stroke="currentColor" stroke-width="3"/></svg>{/if}
              <div class="game-wordmark">{game.id === 'dcs' ? 'DCS' : 'ACR'}<span>{game.id === 'dcs' ? 'WORLD' : 'RALLY'}</span></div>
            </div>
            <div class="tile-copy"><div class="tile-title"><h2>{game.name}</h2><Icon name="chevron" size={16} /></div><p>{game.provider === 'x-plane' ? 'Flight telemetry · Indicators & displays' : game.id === 'dcs' ? 'Aircraft controls · DCS-BIOS feedback' : 'Rally telemetry · Rev lights & displays'}</p><div class="tile-footer"><span class="tile-status"><span></span>{game.installed ? 'Installed' : 'Not found'}</span><span>{profile?.name ?? 'Set up a profile'}</span></div><div class={['tile-integration', { ready: gameIntegrationReady(game), attention: game.integrationSetup?.state === 'needs-attention' }]}><Icon name={gameIntegrationReady(game) ? 'check' : game.integrationSetup?.state === 'needs-attention' ? 'alert' : 'link'} size={12} /><span>{gameIntegrationLabel(game)}</span></div>{#if game.id === 'assetto-corsa-rally' && game.installed}<div class={['tile-verification', { verified: gameIntegrationNativeVerified(game) }]}>{gameIntegrationVerificationLabel(game)}</div>{/if}{#if game.installed}<div class="tile-live"><span class={['live-dot', { receiving: game.integrationSetup?.live === 'receiving', stale: game.integrationSetup?.live === 'stale' }]}></span>{gameIntegrationLiveLabel(game)}</div>{/if}</div>
          </button>
          {#if game.installed}<button class="primary-button tile-launch" disabled={locked || !game.launchSupported} aria-label={`Launch ${game.name}`} onclick={() => launchTile(game)}><Icon name="play" size={14} /> Launch</button>{/if}
        </article>
      {/each}
    </div>
    {#if games.length}<p class="library-help"><Icon name="info" size={13} /> Open a game to configure its peripherals, choose a profile and attach an integration.</p>{/if}
  {/if}
</section>

<dialog class="launch-review" {@attach attachLaunchDialog} onclose={() => { reviewProfile = null; }} aria-labelledby="launch-review-title">
  {#if reviewProfile}
    <div class="review-icon"><Icon name="play" size={22} /></div>
    <p class="game-kicker">Ready to play</p>
    <h2 id="launch-review-title">Launch {gameState.catalogue.find(game => game.id === reviewProfile?.gameId)?.name}?</h2>
    <p class="review-description">Cockpit will activate <strong>{reviewProfile.name}</strong> before opening the game.</p>
    <div class="review-summary"><div><span>Peripherals</span><strong>{reviewProfile.devices.length} configured</strong></div><div><span>Integration</span><strong>{reviewProfile.integration?.document.name ?? 'None'}</strong></div></div>
    <p class="review-warning">Controller and configuration changes can reconnect USB and affect your game bindings. Your previous setup will be restored when the game exits.</p>
    <div class="review-actions"><button class="secondary-button" disabled={busy || pending} onclick={() => launchDialog?.close()}>Cancel</button><button class="primary-button" disabled={busy || pending} onclick={confirmTileLaunch}><Icon name="play" size={14} /> Confirm launch</button></div>
  {/if}
</dialog>

<dialog class="launch-review" {@attach attachRecoveryDialog} aria-labelledby="recovery-review-title">
  <div class="review-icon"><Icon name="alert" size={22} /></div>
  <p class="game-kicker">Pending recovery</p>
  <h2 id="recovery-review-title">Keep current setup?</h2>
  <p class="review-description">Pending recovery cannot currently restore your previous settings.</p>
  <p class="review-warning">Keeping the current setup releases game session ownership and archives the recovery record. Your hardware and integration settings stay as they are.</p>
  <div class="review-actions"><button class="secondary-button" disabled={busy || pending} onclick={() => recoveryDialog?.close()}>Cancel</button><button class="primary-button" disabled={busy || pending} onclick={keepCurrentSetup}>Keep current setup</button></div>
</dialog>

<style>
  .captured-mapping {margin-top:24px;padding-top:24px;border-top:1px solid var(--border);display:grid;gap:12px;}
  .captured-mapping h3 {font-size:14px;}
  .captured-mapping .field-help {margin:0;line-height:1.6;}
  .launch-review { color: var(--text); background: var(--surface); border: 1px solid var(--line); border-radius: 12px; width: 430px; max-width: calc(100% - 32px); padding: 28px; box-shadow: 0 20px 80px #0005; }
  .launch-review::backdrop { background: #0008; }
  .review-icon { color: var(--accent); background: var(--accent-bg); display: grid; place-items: center; width: 46px; height: 46px; border-radius: 11px; margin-bottom: 19px; }
  .launch-review h2 { font-size: 21px; letter-spacing: -.3px; margin-top: 8px; }
  .review-description, .review-warning { font-size: 11px; color: var(--muted); line-height: 1.8; margin-top: 14px; }
  .review-description strong { color: var(--soft); }
  .review-warning { color: #c5a26e; padding: 13px; background: #b18b550a; border: 1px solid #b18b5540; border-radius: 6px; }
  .review-summary { border: 1px solid var(--line); border-radius: 7px; padding: 14px; margin-top: 19px; }
  .review-summary > div { display: flex; align-items: start; justify-content: space-between; gap: 14px; font-size: 10px; }
  .review-summary > div + div { margin-top: 12px; }
  .review-summary span { color: var(--muted); }
  .review-summary strong { color: var(--soft); text-align: right; }
  .review-actions { display: flex; justify-content: end; gap: 10px; margin-top: 23px; }
  .games-workspace { padding-bottom: 30px; }
  button { display: inline-flex; align-items: center; justify-content: center; gap: 7px; }
  .games-toolbar { display: flex; align-items: center; justify-content: space-between; gap: 20px; margin-bottom: 27px; }
  .game-kicker { color: var(--muted); font-size: 8px; letter-spacing: 1.7px; text-transform: uppercase; font-weight: 600; }
  .toolbar-controls { display: flex; align-items: center; gap: 21px; }
  .supported-switch { display: flex; gap: 8px; align-items: center; color: var(--soft); font-size: 10px; cursor: pointer; position: relative; }
  .supported-switch input { position: absolute; opacity: 0; width: 30px; height: 20px; margin: 0; }
  .switch-track { width: 28px; height: 16px; border: 1px solid var(--line); background: var(--surface-raised); border-radius: 12px; display: block; transition: background .15s; }
  .switch-track::after { content: ''; display: block; height: 10px; width: 10px; margin: 2px; border-radius: 50%; background: var(--muted); transition: transform .15s; }
  .supported-switch input:checked + .switch-track { background: var(--accent-bg); border-color: var(--accent); }
  .supported-switch input:checked + .switch-track::after { background: var(--accent); transform: translateX(12px); }
  .supported-switch input:focus-visible + .switch-track { outline: 2px solid var(--accent); outline-offset: 4px; }
  .game-grid { display: grid; grid-template-columns: repeat(auto-fill, minmax(280px, 1fr)); gap: 20px; max-width: 980px; }
  .game-tile { position: relative; border: 1px solid var(--line); border-radius: 9px; background: var(--surface); overflow: hidden; transition: border-color .2s, transform .2s; }
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
  .tile-footer { border-top: 1px solid var(--faint-line); margin-top: 19px; padding-top: 14px; display: flex; justify-content: space-between; color: var(--muted); font-size: 9px; gap: 12px; }
  .tile-footer > span:last-child { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .tile-status { display: flex; align-items: center; gap: 6px; color: var(--accent); }
  .tile-status span, .installation-status span { width: 5px; height: 5px; border-radius: 50%; background: currentColor; }
  .tile-integration, .tile-live { display: flex; align-items: center; gap: 6px; font-size: 9px; color: var(--muted); margin-top: 11px; }
  .tile-integration.ready { color: var(--accent); }
  .tile-integration.attention { color: #c5a26e; }
  .tile-live { margin-top: 8px; }
  .tile-verification { color: var(--muted); font-size: 9px; margin-top: 8px; }
  .tile-verification.verified { color: var(--accent); }
  .live-dot { width: 5px; height: 5px; flex-shrink: 0; border-radius: 50%; background: var(--muted); }
  .live-dot.receiving { background: var(--accent); }
  .live-dot.stale { background: #c5a26e; }
  .tile-launch { position: absolute; right: 18px; top: 169px; opacity: 0; transform: translateY(3px); transition: opacity .15s, transform .15s; pointer-events: none; }
  .game-tile:hover .tile-launch, .game-tile:focus-within .tile-launch { opacity: 1; transform: none; pointer-events: auto; }
  .unavailable .game-art { opacity: .42; filter: grayscale(1); }
  .unavailable .tile-copy { opacity: .5; }
  .unavailable .tile-status { color: var(--muted); }
  .unavailable:hover, .unavailable:focus-within { border-color: var(--soft); }
  .library-help { display: flex; align-items: center; gap: 8px; color: var(--muted); font-size: 10px; margin-top: 20px; line-height: 1.6; }
  .games-empty { padding: 65px 25px; border: 1px dashed var(--line); border-radius: 9px; text-align: center; }
  .empty-icon { color: var(--accent); background: var(--accent-bg); width: 64px; height: 64px; margin: 0 auto 21px; border-radius: 15px; display: grid; place-items: center; }
  .games-empty h2 { font-size: 19px; letter-spacing: -.3px; }
  .games-empty p { color: var(--muted); font-size: 11px; margin: 12px auto 22px; max-width: 360px; line-height: 1.8; }
  .games-empty > div:last-child { display: flex; justify-content: center; gap: 10px; flex-wrap: wrap; }
  .back-link { margin-bottom: 23px; padding-left: 0; }
  .game-details-heading { display: flex; gap: 17px; align-items: center; margin-bottom: 27px; }
  .game-detail-mark { color: #a1c5d7; width: 65px; height: 65px; border-radius: 12px; border: 1px solid var(--line); background: var(--surface-raised); display: grid; place-items: center; }
  .game-detail-mark.rally { color: #c8b292; }
  .game-details-heading h2 { font-size: 23px; letter-spacing: -.5px; margin-top: 7px; }
  .detail-subtitle { font-size: 10px; color: var(--muted); margin-top: 8px; }
  .detail-launch { margin-left: auto; }
  .games-detail-layout { display: grid; grid-template-columns: minmax(0, 1fr) 290px; gap: 23px; }
  .editor-heading { display: flex; justify-content: space-between; align-items: center; gap: 12px; padding: 21px; border-bottom: 1px solid var(--line); }
  h3 { font-size: 12px; font-weight: 600; }
  .editor-heading p, .section-heading p { color: var(--muted); font-size: 10px; margin-top: 7px; line-height: 1.6; }
  .small-status { font-size: 8px; color: var(--muted); white-space: nowrap; }
  .small-status.changed { color: #c5a26e; }
  .editor-body { padding: 21px; }
  .profile-picker { display: flex; gap: 11px; align-items: end; }
  .profile-picker label, .editor-label, .peripheral-controls label { display: flex; flex-direction: column; gap: 8px; color: var(--soft); font-size: 10px; }
  .profile-picker label { flex: 1; min-width: 0; }
  .editor-label { margin-top: 16px; }
  .text-input { width: 100%; }
  .profile-actions { display: flex; justify-content: space-between; margin-top: 9px; }
  .profile-actions .text-button { padding-left: 0; padding-right: 0; font-size: 9px; }
  .danger-link { color: var(--muted); }
  .delete-confirm { display: flex; gap: 10px; align-items: center; padding: 12px; border: 1px solid #b18b5570; border-radius: 6px; font-size: 10px; color: var(--soft); margin-top: 10px; flex-wrap: wrap; }
  .delete-confirm p { flex: 1; min-width: 160px; }
  .section-heading { display: flex; align-items: center; gap: 10px; padding-top: 25px; margin-top: 21px; border-top: 1px solid var(--line); color: var(--accent); }
  .section-heading h3 { color: var(--text); }
  .section-heading > span { margin-left: auto; color: var(--muted); font-size: 10px; }
  .section-empty { color: var(--muted); font-size: 10px; line-height: 1.8; margin-top: 16px; }
  .game-peripheral { padding: 15px; border: 1px solid var(--line); border-radius: 7px; background: var(--subtle); margin-top: 15px; }
  .peripheral-heading { display: flex; align-items: center; gap: 9px; }
  .peripheral-heading > div { flex: 1; min-width: 0; }
  .peripheral-heading strong { color: var(--text); font-size: 11px; }
  .peripheral-heading p { color: var(--muted); font-size: 8px; margin-top: 6px; overflow-wrap: anywhere; }
  .peripheral-controls { display: flex; align-items: end; gap: 12px; margin-top: 16px; }
  .peripheral-controls label { flex: 1; }
  .peripheral-controls button { font-size: 9px; }
  .add-peripheral { display: flex; gap: 10px; margin-top: 16px; }
  .add-peripheral select { flex: 1; min-width: 0; }
  .remove-override { margin-top: 15px; font-size: 9px; padding: 0; }
  .integration-summary { display: flex; align-items: center; gap: 10px; margin-top: 16px; padding: 14px; background: var(--accent-bg); border-radius: 6px; }
  .integration-dot { width: 6px; height: 6px; background: var(--accent); border-radius: 50%; }
  .integration-summary > div { flex: 1; min-width: 0; }
  .integration-summary strong { font-size: 11px; color: var(--soft); }
  .integration-summary p { color: var(--muted); font-size: 9px; margin-top: 6px; }
  .integration-actions { display: flex; align-items: center; gap: 12px; flex-wrap: wrap; margin-top: 14px; }
  .integration-actions button { font-size: 9px; }
  .binding-warning { display: flex; align-items: start; gap: 10px; color: #c5a26e; padding: 15px; background: #b18b550a; border: 1px solid #b18b5540; border-radius: 6px; margin-top: 23px; font-size: 10px; line-height: 1.8; }
  .binding-warning input { margin-top: 3px; flex-shrink: 0; }
  .profile-error, .games-notice { color: #d6a663; font-size: 11px; line-height: 1.7; }
  .profile-error { margin-top: 15px; }
  .games-notice { padding: 13px 16px; border: 1px solid #b18b5540; border-radius: 6px; margin-bottom: 20px; }
  .editor-footer { display: flex; align-items: center; gap: 10px; margin-top: 24px; padding-top: 20px; border-top: 1px solid var(--line); }
  .editor-footer > button:first-child { margin-right: auto; padding-left: 0; }
  .game-sidebar { min-width: 0; }
  .installation-panel { padding: 21px; }
  .sidebar-title { display: flex; gap: 9px; align-items: center; color: var(--soft); }
  .installation-status { display: flex; align-items: center; gap: 7px; color: var(--muted); font-size: 10px; margin-top: 20px; }
  .installation-status.found { color: var(--accent); }
  .installation-panel input { font-family: ui-monospace, monospace; font-size: 9px; }
  .installation-actions { display: flex; gap: 12px; align-items: center; margin-top: 14px; }
  .installation-actions button, .automatic-discovery { font-size: 9px; }
  .automatic-discovery { padding-left: 0; margin-top: 15px; }
  .platform-note { color: #c5a26e; font-size: 10px; line-height: 1.8; margin-top: 17px; }
  .setup-panel { margin-top: 20px; padding: 21px; }
  .setup-readiness { display: flex; align-items: center; gap: 7px; color: var(--muted); font-size: 10px; margin-top: 19px; line-height: 1.5; }
  .setup-readiness.ready { color: var(--accent); }
  .setup-readiness.attention, .setup-warning { color: #c5a26e; }
  .setup-version { font-size: 9px; color: var(--muted); margin-top: 16px; }
  .setup-version strong { color: var(--soft); }
  .setup-panel input, .setup-panel select { font-family: ui-monospace, monospace; font-size: 9px; }
  .native-verification { display: flex; align-items: center; gap: 6px; font-size: 9px; color: #c5a26e; margin-top: 15px; }
  .native-verification.verified { color: var(--accent); }
  .telemetry-channel-details { font-size: 9px; color: var(--muted); margin-top: 15px; }
  .telemetry-channel-details p { font-size: 9px; line-height: 1.8; margin-top: 10px; }
  .telemetry-channel-details summary { cursor: pointer; color: var(--soft); }
  .telemetry-channel-details dl { margin-top: 10px; }
  .telemetry-channel-details dl > div { display: flex; justify-content: space-between; gap: 10px; line-height: 1.8; padding: 4px 0; }
  .telemetry-channel-details dt { text-transform: capitalize; }
  .telemetry-channel-details dd { text-align: right; margin: 0; }
  .setup-main-action { width: 100%; margin-top: 17px; font-size: 10px; }
  .setup-warning { font-size: 10px; line-height: 1.8; margin-top: 12px; }
  .setup-recheck { padding-left: 0; font-size: 9px; margin-top: 13px; }
  .setup-progress { display: flex; align-items: center; gap: 8px; color: var(--accent); font-size: 10px; line-height: 1.8; margin-top: 15px; }
  .setup-live { border-top: 1px solid var(--line); padding-top: 17px; margin-top: 15px; display: flex; align-items: start; gap: 8px; }
  .setup-live .live-dot { margin-top: 4px; }
  .setup-live strong { font-size: 10px; font-weight: 500; color: var(--soft); }
  .setup-live p { font-size: 9px; color: var(--muted); line-height: 1.8; margin-top: 7px; }
  .session-explainer { padding: 24px 4px; color: var(--muted); }
  .session-explainer :global(svg) { color: var(--accent); }
  .session-explainer h3 { color: var(--soft); margin-top: 12px; }
  .session-explainer p { font-size: 10px; line-height: 1.9; margin-top: 12px; }
  .session-banner { display: flex; align-items: center; gap: 13px; padding: 17px; background: var(--accent-bg); border: 1px solid var(--line); border-radius: 8px; margin-bottom: 22px; }
  .session-icon { color: var(--accent); }
  .session-copy { flex: 1; min-width: 0; }
  .session-copy strong { font-size: 12px; }
  .session-copy p { color: var(--muted); font-size: 10px; margin-top: 6px; line-height: 1.7; }
  .session-copy .session-error { color: #d6a663; }
  .session-copy details { color: var(--soft); font-size: 10px; margin-top: 12px; }
  .session-copy summary { cursor: pointer; }
  .session-copy li { padding: 3px 0; }
  .session-warning { background: #b18b550a; border-color: #b18b5560; }
  .session-warning .session-icon { color: #c5a26e; }
  @media (hover: none) { .tile-launch { opacity: 1; transform: none; pointer-events: auto; } }
  @media (max-width: 1050px) { .games-detail-layout { grid-template-columns: minmax(0, 1fr); } .game-sidebar { display: grid; grid-template-columns: 1fr 1fr; gap: 20px; } .setup-panel { margin-top: 0; } .session-explainer { padding: 20px 4px; } }
  @media (max-width: 600px) { .games-toolbar { align-items: start; } .toolbar-controls { align-items: end; flex-direction: column-reverse; gap: 12px; } .game-grid { grid-template-columns: 1fr; } .game-details-heading { flex-wrap: wrap; } .game-details-heading h2 { font-size: 19px; } .game-detail-mark { width: 50px; height: 50px; } .detail-launch { margin-left: 0; width: 100%; } .game-sidebar { grid-template-columns: 1fr; } .profile-picker, .peripheral-controls { flex-wrap: wrap; } .profile-picker label, .peripheral-controls label { flex-basis: 100%; } .editor-heading, .editor-body { padding: 16px; } .editor-footer { flex-wrap: wrap; } .editor-footer > button:first-child { flex-basis: 100%; justify-content: start; } .session-banner { flex-wrap: wrap; } .session-banner > button { margin-left: 33px; } }
  @media (prefers-reduced-motion: reduce) { .game-tile, .tile-launch, .switch-track, .switch-track::after { transition: none; } }
</style>
