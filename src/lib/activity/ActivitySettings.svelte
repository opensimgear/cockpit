<script lang="ts">
  import { onMount } from 'svelte';
  import Icon from '../Icon.svelte';
  import { gatewayRequest } from '../gateway';
  import { ActivityClient } from './model';
  const client = new ActivityClient(gatewayRequest);
  let retentionDays = $state(7), ready = $state(false), busy = $state(false), error = $state(''), message = $state('');
  async function read() { busy = true; error = ''; try { retentionDays = (await client.settings()).retentionDays; ready = true; } catch (reason) { error = String(reason); } finally { busy = false; } }
  async function save() { busy = true; error = ''; message = ''; try { retentionDays = (await client.saveSettings(retentionDays)).retentionDays; message = 'Retention saved. Older activity has been deleted.'; } catch (reason) { error = reason instanceof Error ? reason.message : String(reason); } finally { busy = false; } }
  onMount(() => { void read(); });
</script>

<section class="panel preferences-panel activity-settings" aria-label="Activity retention">
  <div class="panel-heading"><h2>Activity</h2></div>
  <div class="preference-row"><div><strong>Retention period</strong><p id="activity-retention-help">Keep activity for this many days. The default is 7 days. Older events are deleted automatically; shortening this period deletes older events immediately.</p></div><div class="retention-field"><label><span class="field-label">Days</span><input class="text-input" aria-label="Activity retention days" aria-describedby="activity-retention-help" type="number" min="1" max="3650" step="1" disabled={!ready || busy} bind:value={retentionDays}/></label><button class="secondary-button" disabled={!ready || busy} onclick={save}><Icon name="save" size={14}/>{busy ? 'Saving…' : 'Save'}</button></div></div>
  {#if error}<div class="preference-row"><p class="settings-error" role="alert">{error}</p>{#if !ready}<button class="secondary-button" disabled={busy} onclick={read}><Icon name="refresh" size={14}/>Retry</button>{/if}</div>{:else if message}<div class="preference-row"><p role="status">{message}</p></div>{/if}
</section>

<style>
  .activity-settings{margin-top:var(--ui-section-gap)}.preference-row p{font-size:var(--ui-text-size);line-height:var(--ui-line-height);max-width:560px}.retention-field{display:flex;gap:10px;align-items:flex-end;flex-shrink:0}.retention-field label{display:grid;gap:8px;width:90px}.retention-field input{width:100%;box-sizing:border-box}.settings-error{color:var(--danger,#e5b19b)}@media(max-width:600px){.preference-row{flex-wrap:wrap}}
</style>
