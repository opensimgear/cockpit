<script lang="ts">
  import type { Snippet } from 'svelte';
  import Frame from './Frame.svelte';
  import { axisVisual } from './model';
  let { label, controlId, value, min, max, interactive = false, disabled = false, change, onstart, onend, labelSuffix, details }: {
    label: string; controlId?: string; value: number | null | undefined; min: number; max: number;
    interactive?: boolean; disabled?: boolean; change?: (value: number) => void;
    onstart?: (event: PointerEvent) => void; onend?: (event: PointerEvent) => void; labelSuffix?: Snippet; details?: Snippet;
  } = $props();
  const visual = $derived(axisVisual(value, min, max));
  function start(event: PointerEvent) {
    event.stopPropagation();
    if (disabled || event.button !== 0) return;
    event.currentTarget instanceof HTMLElement && event.currentTarget.setPointerCapture(event.pointerId);
    onstart?.(event);
  }
  function end(event: PointerEvent) { event.stopPropagation(); onend?.(event); }
</script>

<Frame {label} {controlId} valueText={visual.valueText} {labelSuffix} {details}>
  <div class="axis" class:disabled class:waiting={visual.status === 'waiting'} class:unknown={visual.status === 'unknown'}>
    <div class="track" aria-hidden="true">
      {#if visual.status === 'value'}<span class="fill" style:left={`${visual.fillStart}%`} style:width={`${visual.fillSize}%`}></span>{/if}
      {#if visual.bipolar}<span class="zero" style:left={`${visual.zero}%`}></span>{/if}
      {#if visual.status === 'value'}<span class="marker" style:left={`${visual.position}%`}></span>{/if}
    </div>
    {#if interactive}
      <input type="range" {min} {max} step="1" value={visual.clamped} {disabled} aria-label={label} aria-valuetext={visual.valueText}
        oninput={event => { event.stopPropagation(); if (!disabled) change?.(event.currentTarget.valueAsNumber); }}
        onpointerdown={start} onpointerup={end} onpointercancel={end} onlostpointercapture={end}
        onkeydown={event => event.stopPropagation()} onkeyup={event => event.stopPropagation()} onclick={event => event.stopPropagation()} />
    {:else}<span class="accessible" role="img" aria-label={`${label}: ${visual.valueText}`}></span>{/if}
  </div>
</Frame>

<style>
  .axis { position: relative; min-width: 0; height: var(--ui-axis-height, 24px); border-radius: 4px; }
  .track { position: absolute; left: 5px; right: 5px; top: calc(50% - 3px); height: 6px; border-radius: 3px; background: var(--line); }
  .fill { position: absolute; height: 100%; border-radius: 3px; background: var(--accent); }
  .zero { position: absolute; top: -3px; height: 12px; width: 1px; background: var(--muted); transform: translateX(-50%); }
  .marker { position: absolute; top: -3px; height: 12px; width: 5px; border-radius: 2px; background: var(--accent); box-shadow: 0 0 0 1px var(--input-bg); transform: translateX(-50%); }
  input { position: absolute; inset: 0; margin: 0; width: 100%; height: 100%; opacity: 0; cursor: ew-resize; }
  .axis:has(input:focus-visible) { outline: 2px solid var(--accent); outline-offset: 2px; }
  .disabled { opacity: .45; } .disabled input { cursor: not-allowed; }
  .waiting .track, .unknown .track { background: transparent; border: 1px dashed var(--muted); box-sizing: border-box; }
  .accessible { position: absolute; width: 1px; height: 1px; overflow: hidden; clip-path: inset(50%); }
</style>
