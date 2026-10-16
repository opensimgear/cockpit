<script lang="ts">
  import Notification from './Notification.svelte';
  import { onMount } from 'svelte';
  import { invoke, isTauri } from '@tauri-apps/api/core';

  type Preferences = {
    closeToTray: boolean;
    minimizeToTray: boolean;
    trayAvailable: boolean;
    trayError: string | null;
    settingsError: string | null;
    autostartSupported: boolean;
    autostartEnabled: boolean;
    autostartError: string | null;
  };
  let preferences = $state<Preferences>();
  let desktop = $state(false);
  let loading = $state(true);
  let busy = $state(false);
  let error = $state('');
  let message = $state('');

  async function load() {
    loading = true;
    error = '';
    try { preferences = await invoke<Preferences>('desktop_preferences'); }
    catch (e) { error = String(e); }
    finally { loading = false; }
  }

  onMount(() => {
    desktop = isTauri();
    if (desktop) void load();
    else loading = false;
  });

  async function toggle(key: 'closeToTray' | 'minimizeToTray' | 'autostartEnabled') {
    if (!preferences || busy) return;
    busy = true;
    error = ''; message = '';
    try {
      if (key === 'autostartEnabled') {
        const enabled = !preferences.autostartEnabled;
        preferences.autostartEnabled = await invoke<boolean>('set_autostart', { enabled });
        if (preferences.autostartEnabled !== enabled) throw new Error('The operating system did not apply the auto-start change.');
        preferences.autostartError = null;
      } else {
        const next = { closeToTray: preferences.closeToTray, minimizeToTray: preferences.minimizeToTray, [key]: !preferences[key] };
        await invoke('set_tray_preferences', { preferences: next });
        preferences = { ...preferences, ...next, settingsError: null };
      }
      message = 'Preference saved.';
    } catch (e) { error = String(e); }
    finally { busy = false; }
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
      <button class="toggle" class:on={preferences.closeToTray} role="switch" aria-label="Close to tray" aria-checked={preferences.closeToTray} disabled={busy || !preferences.trayAvailable} onclick={() => toggle('closeToTray')}><span></span></button>
    </div>
    <div class="preference-row">
      <div><strong>Minimize to tray</strong><p>Hide the window in the system tray when minimized. Open it again from the tray menu.</p></div>
      <button class="toggle" class:on={preferences.minimizeToTray} role="switch" aria-label="Minimize to tray" aria-checked={preferences.minimizeToTray} disabled={busy || !preferences.trayAvailable} onclick={() => toggle('minimizeToTray')}><span></span></button>
    </div>
    <div class="preference-row">
      <div><strong>Start Cockpit when I sign in</strong><p>{preferences.autostartSupported ? 'Automatically launch Cockpit when you sign in to this computer.' : 'Auto-start is not supported on this platform.'}</p></div>
      <button class="toggle" class:on={preferences.autostartEnabled} role="switch" aria-label="Start Cockpit when I sign in" aria-checked={preferences.autostartEnabled} disabled={busy || !preferences.autostartSupported} onclick={() => toggle('autostartEnabled')}><span></span></button>
    </div>
    {#if preferences.trayError}<Notification message={'The system tray is unavailable: ' + String(preferences.trayError)} kind="error" />{/if}
    {#if preferences.settingsError}<Notification message={'Saved desktop preferences could not be read: ' + String(preferences.settingsError) + '. Normal window behavior is active.'} kind="error" />{/if}
    {#if preferences.autostartError}<div class="preference-row"><Notification message={'Auto-start status could not be read: ' + String(preferences.autostartError)} kind="error" /><button class="secondary-button" disabled={busy} onclick={load}>Retry</button></div>{/if}
  {/if}
  {#if error}<div class="preference-row"><Notification message={String(error)} kind="error" />{#if !preferences}<button class="secondary-button" onclick={load}>Retry</button>{/if}</div>{/if}
  {#if message}<Notification message={String(message)} />{/if}
</section>

<style>
  .desktop-preferences { margin-top: 24px; }
  .preference-row p { line-height: 1.6; max-width: 620px; }
  .toggle:disabled { opacity: .45; cursor: not-allowed; }
</style>
