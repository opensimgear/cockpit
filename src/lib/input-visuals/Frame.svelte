<script lang="ts">
  import type { Snippet } from 'svelte';
  let { label, controlId, valueText, labelSuffix, details, children }: {
    label: string; controlId?: string; valueText: string; labelSuffix?: Snippet; details?: Snippet; children: Snippet;
  } = $props();
</script>

<div class="input-visual" data-control-id={controlId}>
  <div class="heading"><span class="label"><strong title={label}>{label}</strong>{@render labelSuffix?.()}</span><span class="value">{valueText}</span></div>
  {@render children()}
  {#if details}<div class="details">{@render details()}</div>{/if}
</div>

<style>
  .input-visual { min-width: 0; display: grid; gap: var(--ui-analog-gap, 7px); padding: var(--ui-analog-padding, 8px); border: 1px solid var(--line); border-radius: 6px; background: var(--input-bg); }
  .heading { display: flex; align-items: baseline; justify-content: space-between; gap: 6px; min-width: 0; font-size: var(--ui-text-size); line-height: 1.4; }
  .label { display: flex; align-items: center; gap: 2px; min-width: 0; }
  strong { min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; color: var(--text); font-weight: 570; }
  .value { min-width: 0; max-width: 65%; text-align: right; overflow-wrap: anywhere; color: var(--soft); font-variant-numeric: tabular-nums; }
  .details { display: grid; gap: 3px; min-width: 0; overflow-wrap: anywhere; font-size: var(--ui-action-size); color: var(--muted); }
</style>
