<script lang="ts">
  import { onMount } from 'svelte';
  import { exampleUi, validatePortableUi } from '@opensimgear/companion-contracts';
  import PanelRenderer from './lib/PanelRenderer.svelte';
  import { createPreviewRecorder } from './lib/preview-recorder.mjs';
  const recorder = createPreviewRecorder();
  let snapshot = $state(recorder.snapshot());
  let nativeStatus = $state('Browser preview');
  let epoch: number | null = null;
  const validation = validatePortableUi(exampleUi);
  function refresh() { snapshot = recorder.snapshot(); }
  function disarm(reason: string) { snapshot = recorder.disarm(reason); }
  function togglePreview() { snapshot = snapshot.armed ? recorder.disarm() : recorder.arm(); }
  function visibility() { if (document.visibilityState === 'hidden') disarm('Page hidden'); }
  onMount(() => {
    let disposed = false;
    let unlisten: (() => void) | undefined;
    async function synchronizeNative() {
      try {
        const { invoke } = await import('@tauri-apps/api/core');
        const state = await invoke<{ invalidationEpoch: number }>('companion_lifecycle');
        if (disposed) return;
        nativeStatus = 'Tauri 2 · recording only';
        if (epoch === null || epoch !== state.invalidationEpoch) disarm('Native lifecycle changed');
        epoch = state.invalidationEpoch;
      } catch { /* Browser mode has the same local-only recorder and no native transport. */ }
    }
    async function initializeNative() {
      try {
        const { listen } = await import('@tauri-apps/api/event');
        const stop = await listen('companion-lifecycle-invalidated', () => { disarm('Native lifecycle changed'); void synchronizeNative(); });
        if (disposed) stop(); else unlisten = stop;
      } catch { /* Plain web preview does not provide Tauri events. */ }
      await synchronizeNative();
    }
    const focus = () => { void synchronizeNative(); };
    window.addEventListener('focus', focus);
    void initializeNative();
    return () => { disposed = true; unlisten?.(); window.removeEventListener('focus', focus); recorder.disarm('Renderer unmounted'); };
  });
</script>

<svelte:window onblur={() => disarm('Window lost focus')} onpagehide={() => disarm('Page unloading')} />
<svelte:document onvisibilitychange={visibility} />

<div class="app-shell">
  <header class="app-header"><a class="brand" href="#panel" aria-label="OpenSimGear Companion"><span class="brand-mark">OSG</span><span>OpenSimGear <strong>Companion</strong></span></a><span class="prototype-badge">C0 FEASIBILITY</span></header>
  <main id="panel">
    <div class="intro"><div><span class="eyebrow">YOUR PANEL. ANY SCREEN.</span><h1>A cockpit within reach.</h1><p>Explore a portable control panel on this PC or touch screen.</p></div><div class="target-badges"><span class="target-active">Desktop</span><span>Android</span><span>iOS</span></div></div>
    <div class="session-bar"><div class="session-label"><span class="status-dot" class:live={snapshot.armed}></span><div><strong>{snapshot.armed ? 'Local preview running' : 'Disarmed'}</strong><span>{snapshot.armed ? 'Inputs are recorded on this device only.' : 'Start a local preview to explore the controls.'}</span></div></div><button class="preview-toggle" class:running={snapshot.armed} onclick={togglePreview} disabled={!validation.ok}>{snapshot.armed ? 'Stop preview' : 'Start local preview'}<span>{snapshot.armed ? '■' : '↗'}</span></button></div>
    {#if validation.ok}<PanelRenderer {recorder} {snapshot} {refresh} />{:else}<p class="validation-error">Panel validation failed: {validation.error}</p>{/if}
    <div class="bottom-grid">
      <section class="connection-card"><div class="card-heading"><h3>Connection</h3><span class="pill">OFFLINE</span></div><p>Run your panel on another PC, Android or iOS device.</p><div class="pairing-placeholder"><span>⌁</span><div><strong>Pair with Cockpit</strong><small>Available in a later milestone</small></div><span>→</span></div><p class="fine-print">This prototype has no network listener, credentials or virtual controller output.</p></section>
      <section class="diagnostics-card"><div class="card-heading"><h3>Input recorder</h3><span class="pill">{snapshot.totalChanges} CHANGES</span></div><div class="diagnostic-stats"><div><strong>{snapshot.activePointers}</strong><span>active pointers</span></div><div><strong>{snapshot.armed ? 'Preview' : 'Disarmed'}</strong><span>session state</span></div></div><p class="last-reason">{snapshot.lastReason}</p><div class="event-log" role="log" aria-label="Recent input changes">{#each snapshot.records.slice(-3).reverse() as entry}<div><span>#{entry.sequence}</span><code>{entry.changes.map((change) => `${change.id}: ${change.value}`).join(' · ')}</code></div>{:else}<div class="empty-log">Control transitions will appear here.</div>{/each}</div></section>
    </div>
  </main>
  <footer><span>{nativeStatus}</span><span>Controls return to fallback when released, cancelled or backgrounded.</span></footer>
</div>
