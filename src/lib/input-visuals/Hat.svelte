<script lang="ts">
  import type { Snippet } from 'svelte';
  import Frame from './Frame.svelte';
  import { HAT_CELLS, hatVisual } from './model';
  let { label, controlId, value, interactive = false, disabled = false, change, labelSuffix, details }: {
    label: string; controlId?: string; value: number | null | undefined; interactive?: boolean; disabled?: boolean;
    change?: (value: number) => void; labelSuffix?: Snippet; details?: Snippet;
  } = $props();
  const visual = $derived(hatVisual(value));
</script>

<Frame {label} {controlId} valueText={visual.valueText} {labelSuffix} {details}>
  <div class="hat" class:waiting={visual.status === 'waiting'} class:unknown={visual.status === 'unknown'} role={interactive ? 'group' : 'img'} aria-label={`${label}: ${visual.valueText}`}>
    {#each HAT_CELLS as cell (cell.value)}
      {#if interactive}
        <button type="button" class="cell" class:selected={visual.selected === cell.value} {disabled}
          aria-label={`${label}: ${cell.text}`} aria-pressed={visual.selected === cell.value}
          onpointerdown={event => event.stopPropagation()} onpointerup={event => event.stopPropagation()}
          onkeydown={event => event.stopPropagation()} onkeyup={event => event.stopPropagation()}
          onclick={event => { event.stopPropagation(); if (!disabled) change?.(cell.value); }}><span aria-hidden="true">{cell.icon}</span></button>
      {:else}<span class="cell" class:selected={visual.selected === cell.value} aria-hidden="true">{cell.icon}</span>{/if}
    {/each}
  </div>
</Frame>

<style>
  .hat { display: grid; grid-template-columns: repeat(3, minmax(0, 1fr)); gap: var(--ui-hat-gap, 3px); width: min(100%, var(--ui-hat-size, 90px)); justify-self: center; min-width: 0; }
  .cell { display: grid; place-items: center; box-sizing: border-box; min-width: 0; min-height: 0; width: 100%; aspect-ratio: 1; margin: 0; padding: 0; border: 1px solid var(--line); border-radius: 4px; background: var(--surface); color: var(--muted); font: inherit; font-size: var(--ui-hat-icon-size, 15px); line-height: 1; }
  .cell.selected { border-color: var(--accent); background: var(--accent-bg); color: var(--accent); }
  button { cursor: pointer; touch-action: manipulation; }
  button:hover:enabled { border-color: var(--accent); color: var(--accent); }
  button:focus-visible { outline: 2px solid var(--accent); outline-offset: 1px; }
  button:disabled { opacity: .45; cursor: not-allowed; }
  .waiting .cell, .unknown .cell { border-style: dashed; }
</style>
