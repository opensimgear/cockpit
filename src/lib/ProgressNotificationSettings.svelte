<script lang="ts">
  import { onMount } from 'svelte';
  import { invoke, isTauri } from '@tauri-apps/api/core';
  import { createProgressNotificationSettings, PROGRESS_NOTIFICATION_POSITIONS, type ProgressNotificationSettingsState } from './progress-notification-settings';

  const nativeApp = isTauri();
  let preferences = $state.raw<ProgressNotificationSettingsState>({
    delivery: 'system', position: 'bottom-right', native: nativeApp, loaded: false, busy: '', error: '', retry: '', message: '',
  });
  const settings = createProgressNotificationSettings(invoke, nativeApp, next => { preferences = next; });
  onMount(() => { void settings.load(); return () => settings.dispose(); });
</script>

<section class="panel preferences-panel progress-notification-settings" aria-label="Progress notifications">
  <div class="panel-heading"><h2>Progress notifications</h2></div>
  <div class="preference-row">
    <div><strong>Notification style</strong><p id="progress-notification-style-help">Show activation, deactivation, and switching progress using system notifications or a Cockpit window. If system notifications are unavailable, Cockpit uses its window automatically. Style changes are saved automatically and apply to the next operation.</p></div>
    <label class="position-field"><span class="field-label">Notification style</span><select aria-label="Progress notification style" aria-describedby="progress-notification-style-help" value={preferences.delivery} disabled={!preferences.native || !preferences.loaded || !!preferences.busy} onchange={event => { void settings.setDelivery(event.currentTarget.value); }}><option value="system">System notifications</option><option value="window">Cockpit window</option></select></label>
  </div>
  <div class="preference-row">
    <div><strong>Window position</strong><p id="progress-notification-help">Choose the corner for Cockpit progress windows. System notification placement follows your operating system settings.</p></div>
    <label class="position-field"><span class="field-label">Window position</span><select aria-label="Progress notification position" aria-describedby="progress-notification-help" value={preferences.position} disabled={!preferences.native || !preferences.loaded || !!preferences.busy} onchange={event => { void settings.save(event.currentTarget.value); }}>{#each PROGRESS_NOTIFICATION_POSITIONS as [position, label]}<option value={position}>{label}</option>{/each}</select></label>
  </div>
  {#if !preferences.native}<div class="preference-row"><p>Progress notification settings are available in the Cockpit desktop app.</p></div>
  {:else if preferences.busy}<div class="preference-row"><p role="status">{preferences.busy === 'load' ? 'Loading notification settings…' : 'Saving notification settings…'}</p></div>
  {:else if preferences.error}<div class="preference-row"><p class="settings-error" role="alert">{preferences.error}</p><button class="secondary-button" onclick={() => { void settings.retry(); }}>Retry</button></div>
  {:else if preferences.message}<div class="preference-row"><p role="status">{preferences.message}</p></div>{/if}
</section>

<style>
  .progress-notification-settings { margin-top: 24px; }
  .preference-row p { line-height: 1.6; max-width: 620px; }
  .position-field { display: grid; gap: 8px; flex-shrink: 0; min-width: 160px; }
  select { width: 100%; background: var(--input-bg); }
  select:disabled { cursor: not-allowed; opacity: .65; }
  .preference-row .settings-error { color: #d3a078; }
  @media (max-width: 520px) { .position-field { width: 100%; } }
</style>
