<script lang="ts">
  import Icon from '../Icon.svelte';
  import { gatewayRequest } from '../gateway';
  import { SharedObservations } from '../streams/shared';
  import { visibleObservation } from '../visible-observation';
  import { ActivityClient, type ActivitySettings } from './model';
  const client = new ActivityClient(gatewayRequest);
  let { active }: { active: boolean } = $props();
  let stopObservation: (() => void) | undefined;
  let committed = $state.raw<ActivitySettings>();
  let draft = $state<number | undefined>();
  let busy = $state(false), streamError = $state(''), error = $state(''), message = $state('');
  const retentionDays = $derived(draft ?? committed?.retentionDays);
  function observe() {
    stopObservation?.();
    streamError = '';
    const observations = new SharedObservations();
    const reader = visibleObservation(observations, 'settings.activity', state => {
      if (state.status === 'ready') {
        committed = state.frame?.payload as ActivitySettings;
        streamError = '';
      } else if (state.status === 'error' || state.status === 'lost') {
        committed = undefined;
        streamError = state.error ?? 'Activity retention settings are unavailable.';
      }
    }, () => { committed = undefined; });
    stopObservation = () => {
      reader.close();
      observations.close();
    };
    reader.setVisible(true);
  }
  async function save() {
    if (retentionDays === undefined || !committed || busy) return;
    const before = committed;
    busy = true;
    error = '';
    message = '';
    try {
      const saved = await client.saveSettings(retentionDays);
      if (committed === before) committed = saved;
      draft = undefined;
      message = 'Retention saved. Older activity has been deleted.';
    } catch (reason) {
      error = reason instanceof Error ? reason.message : String(reason);
    } finally {
      busy = false;
    }
  }
  $effect(() => {
    if (!active) return;
    observe();
    return () => {
      stopObservation?.();
      stopObservation = undefined;
    };
  });
</script>

<section class="panel preferences-panel activity-settings" aria-label="Activity retention">
  <div class="panel-heading"><h2>Activity</h2></div>
  <div class="preference-row"><div><strong>Retention period</strong><p id="activity-retention-help">Keep activity for this many days. The default is 7 days. Older events are deleted automatically; shortening this period deletes older events immediately.</p></div><div class="retention-field"><label><span class="field-label">Days</span><input class="text-input" aria-label="Activity retention days" aria-describedby="activity-retention-help" type="number" min="1" max="3650" step="1" disabled={!committed || busy} value={retentionDays} oninput={event => { draft = event.currentTarget.valueAsNumber; }}/></label><button class="secondary-button" disabled={!committed || busy} onclick={save}><Icon name="save" size={14}/>{busy ? 'Saving…' : 'Save'}</button></div></div>
  {#if streamError}<div class="preference-row"><p class="settings-error" role="alert">{streamError}</p><button class="secondary-button" disabled={busy} onclick={observe}><Icon name="refresh" size={14}/>Retry</button></div>{:else if error}<div class="preference-row"><p class="settings-error" role="alert">{error}</p></div>{:else if message}<div class="preference-row"><p role="status">{message}</p></div>{/if}
</section>

<style>
  .activity-settings{margin-top:var(--ui-section-gap)}.preference-row p{font-size:var(--ui-text-size);line-height:var(--ui-line-height);max-width:560px}.retention-field{display:flex;gap:10px;align-items:flex-end;flex-shrink:0}.retention-field label{display:grid;gap:8px;width:90px}.retention-field input{width:100%;box-sizing:border-box}.settings-error{color:var(--danger,#e5b19b)}@media(max-width:600px){.preference-row{flex-wrap:wrap}}
</style>
