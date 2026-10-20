<script lang="ts">
  import { onMount } from 'svelte';
  import { invoke } from '@tauri-apps/api/core';
  import type { ActivationWindowSnapshot } from './profile-activation-window';

  let snapshot = $state<ActivationWindowSnapshot | null>(null);
  let loading = false;
  let cancelPending = $state(false);
  let error = $state('');
  const cancelling = $derived(cancelPending || snapshot?.cancelRequested || snapshot?.progress.phase === 'cancelling');
  const canCancel = $derived(!!snapshot?.progress.canCancel && !cancelling);
  const status = $derived(error || snapshot?.progress.message || 'Preparing profile…');
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
  <main aria-label="Profile activation progress">
    <p class:warning={!!error} role="status" aria-live="polite" title={status}>{status}</p>
    <button class="secondary-button" disabled={!canCancel} onclick={() => void cancel()}>{cancelling ? 'Cancelling…' : 'Cancel'}</button>
  </main>
</div>

<style>
  main { height: 100%; padding: 16px 18px; display: flex; align-items: center; gap: 18px; overflow: hidden; border: 1px solid var(--line); }
  p { flex: 1; min-width: 0; font-size: 13px; line-height: 20px; color: var(--soft); overflow-wrap: anywhere; display: -webkit-box; -webkit-box-orient: vertical; -webkit-line-clamp: 2; line-clamp: 2; overflow: hidden; }
  .warning { color: #dfb96c; }
  button { flex-shrink: 0; min-width: 78px; }
</style>
