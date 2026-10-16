<script lang="ts">
  import Notification from './Notification.svelte';
  import Icon from './Icon.svelte';
  import { gameSessionLabel, type GamesState } from './games';
  import type { IntegrationState } from './integration-profile';
  import { dcsStatusLabel, type DcsBiosStatus } from './dcs-bios';

  let { games, integration, dcsBios, gateways, onlineDevices, totalDevices, inventoryError, openGames, openIntegrations, openDevices }: {
    games?: GamesState; integration?: IntegrationState; dcsBios?: DcsBiosStatus;
    gateways: number; onlineDevices: number; totalDevices: number; inventoryError: string;
    openGames: () => void; openIntegrations: () => void; openDevices: () => void;
  } = $props();
  const session = $derived(games?.session);
  const profile = $derived(games?.profiles.find(profile => profile.id === session?.profileId));
  const hasProfile = $derived(!!session && ['running', 'waiting', 'launching', 'applying', 'validating', 'restoring'].includes(session.phase));
  const activeGame = $derived(games?.catalogue.find(game => game.id === session?.gameId));
  const detected = $derived(games?.detectedGames);
  const telemetryLabel = $derived(integration?.running
    ? integration.telemetry?.fresh ? 'Receiving telemetry' : 'Waiting for telemetry'
    : dcsBios?.running ? dcsStatusLabel(dcsBios) : 'Not running');
</script>

<div class="page-heading"><div><div class="eyebrow">YOUR COCKPIT AT A GLANCE</div><h1>Overview</h1><p>Your current game, active profile and connected hardware.</p></div><span class="quiet-tag">Live status</span></div>
{#if games?.error || session?.error || inventoryError}
  <Notification message={String(session?.error || games?.error || inventoryError)} kind="error" />
{/if}
<div class="overview-grid">
  <section class="panel overview-card">
    <div class="overview-label"><Icon name="play" size={18} /><h2>Detected game</h2></div>
    {#if detected?.length}
      {#each detected as game (game.gameId)}<div class="detected-game"><strong>{game.name}</strong><span><span class="status-dot"></span>Running · PID {game.pid}</span></div>{/each}
    {:else}<strong class="overview-value">{!games ? 'Checking games…' : detected === undefined || games.detectionErrors?.length ? 'Detection unavailable' : 'No game running'}</strong><p>Supported games appear here when they are detected.</p>{/if}
    {#each games?.detectionErrors ?? [] as error}<Notification message={String(error)} kind="error" />{/each}
    <button class="text-button" onclick={openGames}>Open games <Icon name="chevron" size={13} /></button>
  </section>
  <section class="panel overview-card">
    <div class="overview-label"><Icon name="settings" size={18} /><h2>Active profile</h2></div>
    <strong class="overview-value">{hasProfile ? profile?.name ?? 'Loading profile…' : session?.phase === 'recovery-needed' ? 'Recovery needed' : 'No active game profile'}</strong>
    <p>{hasProfile ? `${activeGame?.name ?? 'Game'} · ${gameSessionLabel(session!)}` : session?.phase === 'recovery-needed' ? 'Review the previous session before activating a profile.' : 'Activate a profile in Games to apply your hardware setup.'}</p>
    <button class="text-button" onclick={openGames}>Manage profiles <Icon name="chevron" size={13} /></button>
  </section>
  <section class="panel overview-card">
    <div class="overview-label"><Icon name="activity" size={18} /><h2>Telemetry & integrations</h2></div>
    <strong class="overview-value">{telemetryLabel}</strong>
    <p>{integration?.running ? integration.config?.document.name ?? 'Integration running' : dcsBios?.running ? 'DCS-BIOS panel profile' : 'Start an integration to connect your game to your devices.'}</p>
    {#if integration?.error}<Notification message={String(integration.error)} kind="error" />{/if}
    <button class="text-button" onclick={openIntegrations}>Open integrations <Icon name="chevron" size={13} /></button>
  </section>
  <section class="panel overview-card">
    <div class="overview-label"><Icon name="devices" size={18} /><h2>Connected hardware</h2></div>
    <div class="hardware-counts"><div><strong>{gateways}</strong><span>{gateways === 1 ? 'gateway' : 'gateways'} connected</span></div><div><strong>{onlineDevices}<small> / {totalDevices}</small></strong><span>devices online</span></div></div>
    <p>{gateways ? 'Connected gateways discover and manage their devices.' : 'Connect a gateway to discover your OpenSimGear devices.'}</p>
    <button class="text-button" onclick={openDevices}>View devices <Icon name="chevron" size={13} /></button>
  </section>
</div>

<style>
  .overview-grid{ display: grid; grid-template-columns: repeat(2, minmax(0, 1fr)); gap: 20px; }.overview-card{ padding: 25px; display: flex; flex-direction: column; align-items: flex-start; gap: 15px; min-width: 0; }.overview-label{ display: flex; align-items: center; gap: 10px; color: var(--muted); }.overview-label h2{ font-size: 12px; font-weight: 600; }.overview-value,.detected-game > strong{ font-size: 21px; line-height: 1.35; font-weight: 650; overflow-wrap: anywhere; }.overview-card p{ color: var(--muted); font-size: 12px; line-height: 1.7; }.overview-card > button{ margin-top: auto; padding-top: 12px; display: flex; align-items: center; gap: 6px; }.detected-game{ display: flex; flex-direction: column; gap: 10px; }.detected-game > span{ color: var(--accent); font-size: 11px; display: flex; align-items: center; gap: 7px; }.hardware-counts{ display: flex; flex-wrap: wrap; gap: 30px; }.hardware-counts > div{ display: flex; flex-direction: column; gap: 6px; }.hardware-counts strong{ font-size: 29px; font-weight: 650; }.hardware-counts small{ font-size: 16px; color: var(--muted); font-weight: 400; }.hardware-counts span{ color: var(--muted); font-size: 11px; }
  @media (max-width: 900px) {.overview-grid{ grid-template-columns: 1fr; } }
</style>
