<script lang="ts">
  import type { GamesState } from './games';
  import type { DeepReadonly, ProfileSummary } from './profiles';
  import type { RuntimeControlState } from './profile-runtime-controller';

  let { games, gamesStale, profileState, profiles }: {
    games: GamesState | undefined;
    gamesStale: boolean;
    profileState: DeepReadonly<RuntimeControlState>;
    profiles: DeepReadonly<ProfileSummary[]>;
  } = $props();

  const detected = $derived(gamesStale ? [] : games?.detectedGames ?? []);
  const gameUnavailable = $derived(gamesStale || !games?.detectedGames || !detected.length && !!games?.detectionErrors?.length);
  const gameLabel = $derived(gameUnavailable ? 'Unavailable' : detected.length ? detected.map(game => game.name).join(', ') : 'None');
  const status = $derived(profileState.stale ? null : profileState.status);
  const applied = $derived(status?.owner ? status.appliedSnapshot : null);
  const profile = $derived(profiles.find(item => item.id === applied?.profileId));
  const stateLabels = { stopped: 'Stopped', running: 'Running', degraded: 'Degraded', faulted: 'Faulted', 'recovery-needed': 'Recovery needed', transitioning: 'Applying…', 'awaiting-game': 'Awaiting game' };
  const profileStatus = $derived(profileState.pending ? 'Updating…' : status ? stateLabels[status.state] : 'Unavailable');
  const profileLabel = $derived(applied ? profile?.name ?? `Profile ${applied.profileId.slice(0, 8)}` : status ? 'None' : 'Unavailable');
  const profileTitle = $derived(applied ? `${profileLabel} · ${profileStatus}` : profileState.statusError || profileStatus);
</script>

<div class="header-statuses" aria-live="polite" aria-atomic="true">
  <span class="header-status" title={gameUnavailable ? 'Current native game detection is unavailable.' : detected.length ? `${gameLabel} · native process detected` : 'No game is running.'}>
    <span class="badge-dot" class:active={detected.length > 0}></span>
    <span class="badge-copy"><span class="badge-label">Active game</span><strong>{gameLabel}</strong></span>
  </span>
  <span class="header-status profile-status" title={profileTitle}>
    <span class="badge-dot" class:active={status?.state === 'running'} class:attention={!!status && ['degraded', 'faulted', 'recovery-needed', 'awaiting-game'].includes(status.state)}></span>
    <span class="badge-copy"><span class="badge-label">Active profile · {profileStatus}</span><strong>{profileLabel}</strong></span>
  </span>
</div>

<style>
  .header-statuses { display: flex; align-items: center; gap: 8px; min-width: 0; }
  .header-status { display: inline-flex; align-items: center; gap: 8px; min-width: 0; max-width: 220px; border: 1px solid var(--line); border-radius: 6px; padding: 6px 10px; background: var(--surface); }
  .profile-status { max-width: 290px; }
  .badge-dot { width: 6px; height: 6px; flex-shrink: 0; border-radius: 50%; background: var(--muted); }
  .badge-dot.active { background: var(--accent); }
  .badge-dot.attention { background: #d8ad65; }
  .badge-copy { display: flex; flex-direction: column; gap: 2px; min-width: 0; }
  .badge-label { color: var(--muted); font-size: 8px; white-space: nowrap; }
  strong { color: var(--soft); font-size: 10px; font-weight: 500; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  @media (max-width: 800px) { .header-status { max-width: 145px; } .profile-status { max-width: 190px; } }
  @media (max-width: 520px) { .header-statuses { gap: 4px; } .header-status { padding: 5px 6px; gap: 5px; max-width: 100px; } .profile-status { max-width: 125px; } .badge-label { font-size: 7px; } strong { font-size: 9px; } }
</style>
