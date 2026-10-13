<script lang="ts">
  import { onMount } from 'svelte';
  import { actionError, initialValues, integerInputValue, outputSummary, type ActionValue, type DeviceAction, type DeviceActions } from './device-actions';
  let { action, snapshot, ready, busy, invoke }: {
    action: DeviceAction; snapshot: DeviceActions; ready: boolean; busy: boolean;
    invoke: (action: DeviceAction, values: ActionValue[]) => Promise<boolean>;
  } = $props();
  let values = $state<ActionValue[]>([]);
  let pending = $state(false);
  const error = $derived(actionError(action, values));
  onMount(() => { values = initialValues(action, snapshot); });
  async function apply() {
    if (!ready || busy || pending || error) return;
    pending = true;
    try { await invoke(action, [...values]); }
    finally { pending = false; }
  }
</script>

<fieldset disabled={!ready || busy || pending || snapshot.status !== 'ready'}>
  <legend>{action.label}</legend>
  <p class="action-kind">{action.endpoint} · {action.semantics === 'setter' ? 'Set output state' : 'Run once'}</p>
  {#each action.parameters as parameter, index (parameter.id)}
    {@const fieldId = `action-${action.id}-${parameter.id}`}
    <label for={fieldId}>{parameter.label}</label>
    {#if parameter.type === 'boolean'}
      <select id={fieldId} value={String(values[index])} onchange={event => { values[index] = event.currentTarget.value === 'true'; }}><option value="false">Off</option><option value="true">On</option></select>
    {:else if parameter.type === 'enum'}
      <select id={fieldId} value={String(values[index] ?? '')} onchange={event => { values[index] = event.currentTarget.value; }}>{#each parameter.options ?? [] as option (option)}<option value={option}>{option}</option>{/each}</select>
    {:else if parameter.type === 'integer'}
      <input id={fieldId} type="number" min={parameter.minimum} max={parameter.maximum} step="1" value={Number(values[index])} oninput={event => { values[index] = integerInputValue(event.currentTarget.value); }} />
    {:else if parameter.type === 'rgb'}
      <div class="color-value"><input id={fieldId} type="color" value={`#${values[index] ?? '000000'}`} oninput={event => { values[index] = event.currentTarget.value.slice(1).toUpperCase(); }} /><span>#{values[index]}</span></div>
    {:else}
      <input id={fieldId} type="text" value={String(values[index] ?? '')} oninput={event => { values[index] = event.currentTarget.value; }} /><p class="field-help">Up to {parameter.maxBytes} UTF-8 bytes.</p>
    {/if}
  {/each}
  {#if error && values.length}<p class="action-error" role="alert">{error}</p>{/if}
  <div class="action-footer"><span role="status">{pending ? 'Applying…' : outputSummary(action, ready ? snapshot : undefined)}</span><button class="secondary-button" disabled={!!error} onclick={apply}>{action.semantics === 'setter' ? 'Apply' : 'Run action'}</button></div>
</fieldset>

<style>
  fieldset { min-width: 0; margin: 12px 0; padding: 14px; border: 1px solid var(--line); border-radius: 5px; background: var(--input-bg); }
  legend { padding: 0 6px; font-size: 12px; font-weight: 600; }
  .action-kind { margin: 0 0 12px; color: var(--muted); font-size: 10px; }
  label { display: block; margin: 10px 0 5px; color: var(--soft); font-size: 11px; }
  input, select { width: 100%; min-height: 32px; padding: 6px 8px; border: 1px solid var(--line); border-radius: 4px; background: var(--panel); color: var(--text); }
  input:focus-visible, select:focus-visible { outline: 2px solid var(--accent); outline-offset: 2px; }
  input[type='color'] { width: 64px; height: 36px; padding: 2px; cursor: pointer; }
  .color-value { display: flex; align-items: center; gap: 10px; font-size: 12px; }
  .action-footer { display: flex; justify-content: space-between; align-items: center; gap: 12px; margin-top: 12px; }
  .action-footer span, .field-help, .action-error { font-size: 11px; color: var(--soft); }
  .action-error { color: var(--amber); }
  .secondary-button { padding: 7px 12px; border: 1px solid var(--line); border-radius: 4px; background: var(--panel); color: var(--text); cursor: pointer; white-space: nowrap; }
  :disabled { opacity: .55; cursor: default; }
</style>
