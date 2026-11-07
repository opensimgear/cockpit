<script lang="ts">
  import Icon from './Icon.svelte';
  import { uiPreference, uiPreferences } from './persistence/ui-preferences';
  const tray = uiPreference('tray');
  import Notification from './Notification.svelte';
  import { onMount, untrack } from 'svelte';
  import { invoke, isTauri } from '@tauri-apps/api/core';
  import { SharedObservations } from './streams/shared';

  type Preferences = {
    closeToTray: boolean;
    minimizeToTray: boolean;
    trayAvailable: boolean;
    trayError: string | null;
    settingsError: string | null;
    autostartSupported: boolean;
    autostartObserved: boolean;
    autostartEnabled: boolean;
    autostartError: string | null;
  };
  let { active }: { active: boolean } = $props();
  let preferences = $state<Preferences>();
  let desktop = $state(false);
  let documentVisible = $state(false);
  let loading = $state(true);
  let busy = $state(false);
  let error = $state('');
  let message = $state('');
  const visible = $derived(desktop && active && documentVisible);
  let windowStateError = $state<string | null>(null);
  let observedAutostart = $state<{ enabled: boolean | null; error: string | null }>();
  const autostartEnabled = $derived(preferences?.autostartObserved ? observedAutostart?.enabled : preferences?.autostartEnabled);
  const autostartError = $derived(preferences?.autostartObserved ? observedAutostart?.error : preferences?.autostartError);
  let readEpoch = 0;

  async function load() {
    if (!visible || busy) return;
    const epoch = ++readEpoch;
    loading = true;
    error = '';
    try {
      const current = await invoke<Preferences>('desktop_preferences');
      if (epoch === readEpoch) preferences = current;
    } catch (e) {
      if (epoch === readEpoch) error = String(e);
    } finally {
      if (epoch === readEpoch) loading = false;
    }
  }

  onMount(() => {
    desktop = isTauri();
    if (!desktop) {
      loading = false;
      return;
    }
    const syncVisibility = () => { documentVisible = !document.hidden; };
    syncVisibility();
    document.addEventListener('visibilitychange', syncVisibility);
    return () => {
      documentVisible = false;
      readEpoch++;
      document.removeEventListener('visibilitychange', syncVisibility);
    };
  });

  $effect(() => {
    if (visible) untrack(() => { void load(); });
    else if (desktop) {
      readEpoch++;
      preferences = undefined;
      loading = true;
    }
  });

  $effect(() => {
    if (!visible) return;
    const observations = new SharedObservations();
    observations.subscribe('settings.window-state', state => {
      const payload = state.frame?.payload as {error?: unknown} | undefined;
      windowStateError = state.status === 'ready' && state.frame?.source === 'window-persistence'
        && typeof payload?.error === 'string' ? payload.error : null;
    });
    return () => { observations.close(); windowStateError = null; };
  });

  $effect(() => {
    if (!visible || !preferences?.autostartObserved) return;
    const observations = new SharedObservations();
    observations.subscribe('settings.autostart', state => {
      if (state.status === 'connecting') return;
      const payload = state.frame?.payload as Partial<{ enabled: boolean | null; error: string | null }> | undefined;
      if (state.status === 'ready' && state.frame?.source === 'desktop-autostart' && payload
          && (typeof payload.enabled === 'boolean' || payload.enabled === null)
          && (typeof payload.error === 'string' || payload.error === null)) {
        observedAutostart = { enabled: payload.enabled, error: payload.error };
      } else {
        observedAutostart = { enabled: null, error: state.error ?? 'The auto-start observation is unavailable.' };
      }
    });
    return () => { observations.close(); observedAutostart = undefined; };
  });

  async function toggle(key: 'closeToTray' | 'minimizeToTray' | 'autostartEnabled') {
    const current = preferences;
    if (!visible || !current || busy) return;
    busy = true;
    error = ''; message = '';
    try {
      if (key === 'autostartEnabled') {
        if (autostartEnabled == null) return;
        const enabled = !autostartEnabled;
        const applied = await invoke<boolean>('set_autostart', { enabled });
        if (!current.autostartObserved && preferences === current) preferences = { ...current, autostartEnabled: applied, autostartError: null };
        if (applied !== enabled) throw new Error('The operating system did not apply the auto-start change.');
      } else {
        const next = { ...$tray.value, [key]: !$tray.value[key] };
        await tray.save(next);
        if (preferences === current) current.settingsError = null;
      }
      message = 'Preference saved.';
    } catch (e) { error = String(e); }
    finally {
      busy = false;
      if (!preferences) void load();
    }
  }
</script>

<section class="panel preferences-panel desktop-preferences" aria-label="Desktop behavior">
  <div class="panel-heading"><h2>Desktop behavior</h2></div>
  {#if !desktop && !loading}
    <div class="preference-row"><p>Tray and auto-start settings are available in the Cockpit desktop app.</p></div>
  {:else if loading}
    <div class="preference-row"><p role="status">Loading desktop preferences…</p></div>
  {:else if preferences}
    <div class="preference-row">
      <div><strong>Close to tray</strong><p>Keep Cockpit and your connections running when you close the window. Use the tray menu to open Cockpit or quit.</p></div>
      <button class="toggle" class:on={$tray.value.closeToTray} role="switch" aria-label="Close to tray" aria-checked={$tray.value.closeToTray} disabled={busy || $tray.busy || !$tray.ready || !preferences.trayAvailable} onclick={() => toggle('closeToTray')}><span></span></button>
    </div>
    <div class="preference-row">
      <div><strong>Minimize to tray</strong><p>Hide the window in the system tray when minimized. Open it again from the tray menu.</p></div>
      <button class="toggle" class:on={$tray.value.minimizeToTray} role="switch" aria-label="Minimize to tray" aria-checked={$tray.value.minimizeToTray} disabled={busy || $tray.busy || !$tray.ready || !preferences.trayAvailable} onclick={() => toggle('minimizeToTray')}><span></span></button>
    </div>
    <div class="preference-row">
      <div><strong>Start Cockpit when I sign in</strong><p>{preferences.autostartSupported ? 'Automatically launch Cockpit when you sign in to this computer.' : 'Auto-start is not supported on this platform.'}</p></div>
      <button class="toggle" class:on={autostartEnabled === true} role="switch" aria-label="Start Cockpit when I sign in" aria-checked={autostartEnabled ?? false} disabled={busy || !preferences.autostartSupported || autostartEnabled == null} onclick={() => toggle('autostartEnabled')}><span></span></button>
    </div>
    {#if windowStateError}
      <div class="preference-row"><p class="geometry-error" role="status">Window geometry could not be saved or restored: {windowStateError}</p></div>
    {/if}
    {#if preferences.trayError}<Notification message={'The system tray is unavailable: ' + String(preferences.trayError)} kind="error" />{/if}
    {#if preferences.settingsError}<Notification message={'Saved desktop preferences could not be read: ' + String(preferences.settingsError) + '. Normal window behavior is active.'} kind="error" />{/if}
    {#if autostartError}<div class="preference-row"><Notification message={'Auto-start status could not be read: ' + autostartError} kind="error" /><button class="secondary-button" disabled={busy} onclick={load}><Icon name="refresh" size={14} />Retry</button></div>{/if}
  {/if}
  {#if $tray.error}<div class="preference-row"><Notification message={$tray.error} kind="error" /><button class="secondary-button" onclick={() => { void uiPreferences.hydrate(); }}><Icon name="refresh" size={14} />Retry</button></div>{/if}
  {#if error}<div class="preference-row"><Notification message={String(error)} kind="error" />{#if !preferences}<button class="secondary-button" onclick={load}><Icon name="refresh" size={14} />Retry</button>{/if}</div>{/if}
  {#if message}<Notification message={String(message)} />{/if}
</section>

<style>
  .desktop-preferences { margin-top: var(--ui-section-gap); }
  .preference-row p { line-height: var(--ui-line-height); max-width: 620px; }
  .geometry-error { color: var(--danger, #e5b19b); }
  .toggle:disabled { opacity: .45; cursor: not-allowed; }
</style>
