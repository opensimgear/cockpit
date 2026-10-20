<script lang="ts">
  import { onMount } from 'svelte';
  import { invoke } from '@tauri-apps/api/core';
  import Icon from './Icon.svelte';
  import type { ActivationWindowSnapshot } from './profile-activation-window';

  let snapshot = $state<ActivationWindowSnapshot | null>(null);
  let loading = false;
  let cancelPending = $state(false);
  let error = $state('');
  const cancelling = $derived(cancelPending || snapshot?.cancelRequested || snapshot?.progress.phase === 'cancelling');
  const canCancel = $derived(!!snapshot?.progress.canCancel && !cancelling);
  const light = (() => { try { return localStorage.getItem('osg-light') === 'true'; } catch { return false; } })();

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
      error = 'Could not request cancellation. You can also cancel from the main Cockpit window.';
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
  <main aria-label="Profile activation progress">
    <div class="activation-heading">
      <span class="activation-icon"><Icon name={cancelling ? 'stop' : 'play'} size={20} /></span>
      <div class="heading-copy">
        <h1>{cancelling ? 'Cancelling activation' : snapshot?.progress.title ?? 'Activating profile'}</h1>
        <p class="profile-name" title={snapshot?.progress.profileName ?? ''}>{snapshot?.progress.profileName ?? 'Preparing…'}</p>
      </div>
    </div>
    <div class="progress-track" role="progressbar" aria-label={cancelling ? 'Cancelling activation' : 'Activation in progress'}><span></span></div>
    <div class="progress-copy" role="status" aria-live="polite">
      <p>{cancelling ? 'Finishing safe cleanup. Please wait…' : snapshot?.progress.message ?? 'Contacting Cockpit…'}</p>
      {#if !cancelling && snapshot?.progress.details.length}
        <ul>{#each snapshot.progress.details as detail}<li>{detail}</li>{/each}</ul>
      {/if}
      {#if error}<p class="warning">{error}</p>{/if}
    </div>
    <footer>
      <span>{cancelling ? 'This window closes when cleanup finishes.' : 'You can keep using Cockpit while this completes.'}</span>
      <button class="secondary-button" disabled={!canCancel} onclick={() => void cancel()}><Icon name="close" size={15} />{cancelling ? 'Cancelling…' : 'Cancel activation'}</button>
    </footer>
  </main>
</div>

<style>
  .activation-window { height: 100dvh; }
  main { min-height: 100%; padding: 26px 28px 22px; display: flex; flex-direction: column; gap: 22px; overflow: auto; }
  .activation-heading { display: flex; align-items: center; gap: 14px; }
  .activation-icon { background: var(--accent-bg); color: var(--accent); padding: 12px; border-radius: 10px; display: flex; }
  .heading-copy { min-width: 0; }
  h1 { margin: 0; font-size: 20px; line-height: 1.3; letter-spacing: -.3px; }
  .profile-name { color: var(--soft); font-size: 13px; margin-top: 5px; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .progress-track { height: 4px; border-radius: 4px; overflow: hidden; background: var(--line); flex-shrink: 0; }
  .progress-track span { display: block; width: 34%; height: 100%; background: var(--accent); border-radius: inherit; animation: pending 1.5s ease-in-out infinite alternate; }
  .progress-copy { font-size: 13px; color: var(--soft); line-height: 1.5; flex: 1; overflow-wrap: anywhere; }
  ul { padding: 0 0 0 17px; margin: 8px 0 0; font-size: 12px; color: var(--muted); }
  li + li { margin-top: 5px; }
  .warning { margin-top: 10px; color: #dfb96c; }
  footer { display: flex; align-items: center; justify-content: space-between; gap: 20px; }
  footer > span { font-size: 11px; line-height: 1.4; color: var(--muted); max-width: 210px; }
  @keyframes pending { from { transform: translateX(-20%); } to { transform: translateX(215%); } }
  @media (prefers-reduced-motion: reduce) { .progress-track span { animation: none; width: 100%; opacity: .55; } }
</style>
