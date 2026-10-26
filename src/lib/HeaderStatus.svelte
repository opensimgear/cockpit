<script lang="ts">
  import type { GamesState } from './games';
  import type { DeepReadonly, ProfileSummary } from './profiles';
  import type { RuntimeControlState } from './profile-runtime-controller';
  import type { ProfileAutomaticController } from './profile-automatic-controller';
  import ProfileAutomaticControls from './ProfileAutomaticControls.svelte';

  let { games, gamesStale, profileState, profiles, automaticController }: {
    games: GamesState | undefined;
    gamesStale: boolean;
    profileState: DeepReadonly<RuntimeControlState>;
    profiles: DeepReadonly<ProfileSummary[]>;
    automaticController?: ProfileAutomaticController;
  } = $props();

  const detected = $derived(gamesStale ? [] : games?.detectedGames ?? []);
  const gameUnavailable = $derived(gamesStale || !games?.detectedGames || !detected.length && !!games?.detectionErrors?.length);
  const gameLabel = $derived(gameUnavailable ? 'Unavailable' : detected.length ? detected.map(game => game.name).join(', ') : 'None');
  const status = $derived(profileState.stale ? null : profileState.status);
  const applied = $derived(status?.owner ? status.appliedSnapshot : null);
  const profile = $derived(profiles.find(item => item.id === applied?.profileId));
  const stateLabels = { stopped: '', running: '', degraded: 'Degraded', faulted: 'Faulted', 'recovery-needed': 'Recovery needed', transitioning: 'Activating…', 'awaiting-game': 'Awaiting game' };
  const profileStatus = $derived(profileState.pending ? 'Updating…' : status ? stateLabels[status.state] : 'Unavailable');
  const profileLabel = $derived(applied ? profile?.name ?? `Profile ${applied.profileId.slice(0, 8)}` : status ? 'No profile active' : 'Unavailable');
  const profileTitle = $derived(applied ? [profileLabel, profileStatus].filter(Boolean).join(' · ') : profileState.statusError || profileStatus || 'No profile active.');
</script>

<div class="header-statuses" aria-live="polite" aria-atomic="true">
  <span class="header-status" title={gameUnavailable ? 'Current native game detection is unavailable.' : detected.length ? `${gameLabel} · native process detected` : 'No game is running.'}>
    <span class="badge-dot" class:active={detected.length > 0}></span>
    <span class="badge-copy"><span class="badge-label">Active game</span><strong>{gameLabel}</strong></span>
  </span>
  <div class="header-status profile-status" role="group" aria-label="Active profile and mode">
    <span class="profile-identity" title={profileTitle}>
      <span class="badge-dot" class:active={status?.state === 'running'} class:attention={!!status && ['degraded', 'faulted', 'recovery-needed', 'awaiting-game'].includes(status.state)}></span>
      <span class="badge-copy"><span class="badge-label">{profileStatus ? `Active profile · ${profileStatus}` : 'Active profile'}</span><strong>{profileLabel}</strong></span>
    </span>
    <span class="badge-separator" aria-hidden="true"></span>
    <ProfileAutomaticControls controller={automaticController}/>
  </div>
</div>

<style>
  .header-statuses { display: flex; align-items: center; gap: 8px; min-width: 0; }
  .header-status { display: inline-flex; align-items: center; gap: 8px; min-width: 0; max-width: 220px; border: 1px solid var(--line); border-radius: 6px; padding: 6px 10px; background: var(--surface); }
  .profile-status { max-width: 420px; padding: 0; gap: 0; align-items: stretch; }
  .profile-identity { display: inline-flex; align-items: center; gap: 8px; min-width: 0; max-width: 290px; padding: 6px 10px; }
  .badge-separator { width: 1px; flex-shrink: 0; align-self: stretch; margin: 6px 0; background: var(--line); }
  .badge-dot { width: 6px; height: 6px; flex-shrink: 0; border-radius: 50%; background: var(--muted); }
  .badge-dot.active { background: var(--accent); }
  .badge-dot.attention { background: #d8ad65; }
  .badge-copy { display: flex; flex-direction: column; gap: 2px; min-width: 0; }
  .badge-label { color: var(--muted); font-size: 8px; white-space: nowrap; }
  strong { color: var(--soft); font-size: 10px; font-weight: 500; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  @media (max-width: 800px) { .header-status { max-width: 145px; } .profile-status { max-width: 330px; } .profile-identity { max-width: 190px; } }
  @media (max-width: 520px) { .header-statuses { gap: 4px; } .header-status { padding: 5px 6px; gap: 5px; max-width: 100px; } .profile-status { max-width: 240px; padding: 0; gap: 0; } .profile-identity { max-width: 125px; padding: 5px 6px; gap: 5px; } .badge-label { font-size: 7px; } strong { font-size: 9px; } }
</style>
