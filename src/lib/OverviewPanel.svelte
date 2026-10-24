<script lang="ts">
  import PageHeader from './PageHeader.svelte';
  import Notification from './Notification.svelte';
  import Icon from './Icon.svelte';
  import { type GamesState } from './games';
  import { gameContextLabel } from './game-provider-settings';
  import { DEFAULT_PROFILE_ID, type DeepReadonly, type ProfileSummary } from './profiles';
  import type { RuntimeControlState } from './profile-runtime-controller';

  let { games, profileState, profiles = [], gateways, onlineDevices, totalDevices, inventoryError, openGames, openProfiles, openDevices }: {
    games?: GamesState; profileState: DeepReadonly<RuntimeControlState>; profiles?: DeepReadonly<ProfileSummary[]>;
    gateways: number; onlineDevices: number; totalDevices: number; inventoryError: string;
    openGames: () => void; openProfiles: () => void; openDevices: () => void;
  } = $props();
  const native = $derived(profileState.stale ? null : profileState.status);
  const snapshot = $derived(native?.appliedSnapshot);
  const appliedName = $derived(snapshot ? profiles.find(profile => profile.id === snapshot.profileId)?.name ?? (snapshot.profileId === DEFAULT_PROFILE_ID ? 'Default' : snapshot.profileId) : null);
  const stateLabels = { stopped: 'No profile active', running: 'Active', degraded: 'Degraded · available branches active', faulted: 'Faulted', 'recovery-needed': 'Recovery needed', transitioning: 'Activating profile', 'awaiting-game': 'Waiting for the verified game' };
  const profileLabel = $derived(!native ? profileState.statusError ? 'Native status unavailable' : 'Checking profile…' : native.state === 'stopped' ? 'No profile active' : appliedName ?? stateLabels[native.state]);
  const selectedContext = $derived(native?.owner?.gameId ? games?.catalogue.find(game => game.id === native.owner!.gameId) : undefined);
  const contextLabel = $derived(!native ? 'Context status unavailable' : !native.owner ? 'No applied game context' : !native.owner.gameId ? 'Hardware-only profile' : selectedContext ? gameContextLabel(selectedContext) : 'Waiting for native game context');
</script>

<PageHeader title="Overview" eyebrow="YOUR COCKPIT AT A GLANCE" subtitle="Your detected games, active profile and connected hardware.">{#snippet actions()}<span class="quiet-tag">Native status</span>{/snippet}</PageHeader>
{#if games?.error || inventoryError}<Notification message={String(games?.error || inventoryError)} kind="error" />{/if}

{#if profileState.statusError}<Notification message={profileState.statusError} kind="error" />{/if}
<div class="overview-grid">
  <section class="panel overview-card">
    <div class="overview-label"><Icon name="play" size={18} /><h2>Detected game</h2></div>
    {#if games?.detectedGames?.length}
      {#each games.detectedGames as game (game.gameId)}<div class="detected-game"><strong>{game.name}</strong><span><span class="status-dot"></span>Native process detected</span></div>{/each}
    {:else}<strong class="overview-value">{!games ? 'Checking games…' : games.detectedGames === undefined || games.detectionErrors?.length ? 'Detection unavailable' : 'No game running'}</strong><p>Detection does not apply a profile or confirm fresh telemetry.</p>{/if}
    {#each games?.detectionErrors ?? [] as error}<Notification message={String(error)} kind="error" />{/each}
    <button class="text-button" onclick={openGames}>Open games <Icon name="chevron" size={13} /></button>
  </section>
  <section class="panel overview-card" aria-label="Active profile">
    <div class="overview-label"><Icon name="settings" size={18} /><h2>Active profile</h2></div>
    <strong class="overview-value">{profileLabel}</strong>
    {#if native && native.state !== 'stopped'}<p>{stateLabels[native.state]}{#if native.owner?.gameId} · {games?.catalogue.find(game => game.id === native.owner!.gameId)?.name ?? native.owner.gameId}{/if}</p>{:else if !native}<p>Waiting for the current native profile status. Open a profile to activate it. Saving an active profile applies its changes.</p>{/if}
    {#if native?.state === 'awaiting-game'}<p>Hardware remains neutral while the native launch waits for its exact verified game.</p>{/if}
    {#if native?.fault}<Notification message={native.fault} kind="error" />{/if}
    {#if native?.degradation.length}<ul class="degradation" aria-label="Unavailable profile branches">{#each native.degradation as message,index (index)}<li>{message}</li>{/each}</ul>{/if}
    <button class="text-button" onclick={openProfiles}>Open profiles <Icon name="chevron" size={13} /></button>
  </section>
  <section class="panel overview-card">
    <div class="overview-label"><Icon name="activity" size={18} /><h2>Game context</h2></div>
    <strong class="overview-value">{contextLabel}</strong>
    <p>{selectedContext?.integrationContext?.message ?? 'Each game owns its provider setup. Applied profile inspection shows the actual observed samples.'}</p>
    <p>Context readiness does not confirm fresh telemetry or delivered output.</p>
    <button class="text-button" onclick={openGames}>Open games <Icon name="chevron" size={13} /></button>
  </section>
  <section class="panel overview-card">
    <div class="overview-label"><Icon name="devices" size={18} /><h2>Connected hardware</h2></div>
    <div class="hardware-counts"><div><strong>{gateways}</strong><span>{gateways === 1 ? 'gateway' : 'gateways'} connected</span></div><div><strong>{onlineDevices}<small> / {totalDevices}</small></strong><span>devices online</span></div></div>
    <p>{gateways ? 'Connected gateways discover and manage their devices.' : 'Connect a gateway to discover your OpenSimGear devices.'}</p>
    <button class="text-button" onclick={openDevices}>View devices <Icon name="chevron" size={13} /></button>
  </section>
</div>

<style>
  .overview-grid{display:grid;grid-template-columns:repeat(2,minmax(0,1fr));gap:20px}.overview-card{padding:25px;display:flex;flex-direction:column;align-items:flex-start;gap:15px;min-width:0}.overview-label{display:flex;align-items:center;gap:10px;color:var(--muted)}.overview-label h2{font-size:12px;font-weight:600}.overview-value,.detected-game>strong{font-size:21px;line-height:1.35;font-weight:650;overflow-wrap:anywhere}.overview-card p,.degradation{color:var(--muted);font-size:12px;line-height:1.7}.overview-card>button{margin-top:auto;padding-top:12px;display:flex;align-items:center;gap:6px}.detected-game{display:flex;flex-direction:column;gap:10px}.detected-game>span{color:var(--accent);font-size:11px;display:flex;align-items:center;gap:7px}.hardware-counts{display:flex;flex-wrap:wrap;gap:30px}.hardware-counts>div{display:flex;flex-direction:column;gap:6px}.hardware-counts strong{font-size:29px;font-weight:650}.hardware-counts small{font-size:16px;color:var(--muted);font-weight:400}.hardware-counts span{color:var(--muted);font-size:11px}.degradation{padding-left:18px;margin:0}
  @media(max-width:900px){.overview-grid{grid-template-columns:1fr}}
</style>
