<script lang="ts">
  import { uiPreference } from './persistence/ui-preferences';
  const theme = uiPreference('theme');
  import { onMount } from 'svelte';
  import { invoke } from '@tauri-apps/api/core';
  import {profileProgressCanCancel, type ActivationWindowSnapshot} from './profile-activation-window';

  let snapshot = $state<ActivationWindowSnapshot | null>(null);
  let loading = false;
  let cancelPending = $state(false);
  let error = $state('');
  const cancelling = $derived(cancelPending || snapshot?.cancelRequested || snapshot?.progress.phase === 'cancelling');
  const canCancel = $derived(!!snapshot && profileProgressCanCancel(snapshot.progress) && !cancelling);
  const showCancel = $derived(!!snapshot && (profileProgressCanCancel(snapshot.progress) || cancelling));
  const status = $derived(error || snapshot?.progress.message || 'Preparing profile…');
  const light = $derived($theme.value);

  async function refresh() {
    if (loading) return;
    loading = true;
    try {
      const next = await invoke<ActivationWindowSnapshot | null>('profile_activation_window_state');
      if (next?.progress.operationId !== snapshot?.progress.operationId || (!next?.cancelRequested && next?.progress.phase !== 'cancelling')) cancelPending = false;
      snapshot = next;
      error = '';
    } catch { error = 'Waiting for Cockpit to reconnect…'; }
    finally { loading = false; }
  }

  async function cancel() {
    if (!snapshot || !canCancel) return;
    const operationId = snapshot.progress.operationId;
    cancelPending = true;
    try {
      const accepted = await invoke<boolean>('profile_activation_window_cancel', { operationId });
      if (!accepted && snapshot?.progress.operationId === operationId) cancelPending = false;
    } catch {
      cancelPending = false;
      error = 'Could not cancel. Please try again.';
    }
  }

  onMount(() => {
    void refresh();
    const requestClose = () => { void cancel(); };
    window.addEventListener('profile-activation-close', requestClose);
    const timer = window.setInterval(() => { void refresh(); }, 200);
    return () => { window.clearInterval(timer); window.removeEventListener('profile-activation-close', requestClose); };
  });
</script>

<svelte:window onkeydown={event => { if (event.key === 'Escape' && canCancel) { event.preventDefault(); void cancel(); } }} />

<div class:light class="cockpit activation-window">
  <main class="progress-notification" class:warning={!!error} aria-label="Profile operation progress">
    <span class="progress-indicator" aria-hidden="true"></span>
    <div class="notification-copy">
      <h1>{snapshot?.progress.title || 'Profile progress'}</h1>
      {#if snapshot?.progress.profileName}<p class="profile-name" title={snapshot.progress.profileName}>{snapshot.progress.profileName}</p>{/if}
      <p class="progress-message" role="status" aria-live="polite" title={status}>{status}</p>
    </div>
    {#if showCancel}<button class="secondary-button" disabled={!canCancel} onclick={() => void cancel()}>{cancelling ? 'Cancelling…' : 'Cancel'}</button>{/if}
  </main>
</div>

<style>
  .activation-window { background: var(--surface); }
  main { height: 100%; padding: 11px 14px; display: flex; align-items: flex-start; gap: 12px; overflow: hidden; border: 1px solid var(--line); border-left: 3px solid var(--accent); border-radius: 8px; background: var(--surface); }
  .progress-indicator { flex: 0 0 18px; height: 18px; margin-top: 2px; border: 2px solid var(--line); border-top-color: var(--accent); border-radius: 50%; animation: turn 1s linear infinite; }
  .notification-copy { flex: 1; min-width: 0; }
  h1 { margin: 0; font-size: 12px; line-height: 17px; font-weight: 600; letter-spacing: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .profile-name { margin-top: 1px; font-size: 10px; line-height: 14px; color: var(--muted); overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .progress-message { margin-top: 4px; font-size: 11px; line-height: 17px; color: var(--soft); overflow-wrap: anywhere; display: -webkit-box; -webkit-box-orient: vertical; -webkit-line-clamp: 2; line-clamp: 2; overflow: hidden; }
  .warning { border-left-color: #dfb96c; }
  .warning .progress-message { color: #dfb96c; }
  .warning .progress-indicator { border-top-color: #dfb96c; }
  button { flex-shrink: 0; align-self: center; min-width: 58px; min-height: 28px; padding: 5px 9px; font-size: 10px; }
  @keyframes turn { to { transform: rotate(360deg); } }
  @media (prefers-reduced-motion: reduce) { .progress-indicator { animation: none; border-color: var(--accent); border-right-color: transparent; } }
</style>
