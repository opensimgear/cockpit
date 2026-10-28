<script lang="ts">
  import { controlLabel, inputLabel, type ControllerControl } from './controllers';
  let { controls, online, simple = false }: { controls: ControllerControl[]; online: boolean; simple?: boolean } = $props();
  const descriptionId = $props.id();
  let dismissedId = $state('');
  const buttons = $derived(controls.filter(control => control.capability.kind === 'button'));
  const otherControls = $derived(controls.filter(control => control.capability.kind !== 'button'));

  function expand(event: MouseEvent | FocusEvent) {
    dismissedId = '';
    const tile = event.currentTarget as HTMLButtonElement;
    const slot = tile.parentElement!, matrix = slot.parentElement!;
    const bounds = matrix.getBoundingClientRect(), position = slot.getBoundingClientRect();
    const width = Math.min(240, bounds.width);
    const bottom = Math.min(window.innerHeight, matrix.closest('main')?.getBoundingClientRect().bottom ?? window.innerHeight);
    const above = position.top + 140 > bottom && position.bottom > 140;
    tile.style.setProperty('--expanded-width', `${width}px`);
    tile.style.setProperty('--expanded-left', `${Math.max(bounds.left - position.left, Math.min(0, bounds.right - position.left - width))}px`);
    tile.style.setProperty('--expanded-top', above ? 'auto' : '0');
    tile.style.setProperty('--expanded-bottom', above ? '0' : 'auto');
  }
</script>

{#snippet information(control: ControllerControl)}
  <span class="input-value">{inputLabel(control)}</span>
  <small>Report {control.capability.reportId} · collection {control.capability.collection} · field {control.capability.dataIndex}</small>
  <small>Native {control.value?.native ?? '—'} · range {control.capability.min}…{control.capability.max}</small>
{/snippet}

{#if controls.length}
  <div class="live-input-layout" class:simple class:buttons-only={!otherControls.length} class:analog-only={!buttons.length}>
    {#if otherControls.length}
      <section class="analog-inputs" aria-label="Axes, hats and other inputs">
        {#each otherControls as control, index (control.capability.id)}
          <div class="analog-control" data-control-id={control.capability.id}>
            <strong title={simple ? controlLabel(control) : undefined}>{simple && control.capability.kind === 'diagnostic' ? `Input ${index + 1}` : controlLabel(control)}</strong>
            {#if control.capability.kind === 'axis' && control.value?.normalized != null}<meter min={control.capability.min < 0 ? -32768 : 0} max="32767" value={control.value.normalized} aria-label={controlLabel(control)}></meter>{/if}
            {#if simple}<span class="input-value">{inputLabel(control)}</span>{:else}{@render information(control)}{/if}
          </div>
        {/each}
      </section>
    {/if}
    {#if buttons.length}
      <section class="button-inputs" aria-label="Buttons">
        <div class="button-matrix">
          {#each buttons as control (control.capability.id)}
            {#if simple}
              <span class="button-indicator" class:pressed={control.value?.normalized === 1} class:waiting={!control.value} data-control-id={control.capability.id}
                role="img" aria-label={`${controlLabel(control)}: ${inputLabel(control)}`} title={`${controlLabel(control)}: ${inputLabel(control)}`}>{control.capability.usage}</span>
            {:else}<div class="button-slot">
              <button type="button" class="button-control" class:pressed={control.value?.normalized === 1} class:waiting={!control.value} class:dismissed={dismissedId === control.capability.id}
                data-control-id={control.capability.id} aria-label={`${controlLabel(control)}: ${inputLabel(control)}`} aria-describedby={`${descriptionId}-${control.capability.id}`}
                onmouseenter={expand} onfocus={expand} onkeydown={event => { if (event.key === 'Escape') { dismissedId = control.capability.id; event.currentTarget.blur(); } }}>
                <span class="button-number">{control.capability.usage}</span>
                <span class="button-information" id={`${descriptionId}-${control.capability.id}`}><strong>{controlLabel(control)}</strong>{@render information(control)}</span>
              </button>
            </div>{/if}
          {/each}
        </div>
      </section>
    {/if}
  </div>
{:else}
  <p class="controls-empty">{online ? 'No input controls reported by this controller.' : 'Live input values will appear when this controller reconnects.'}</p>
{/if}

<style>
  .button-matrix :global(.button-cell > .connection-badge), .button-matrix :global(.button-slot > .connection-badge) { right: 0; }
  .live-input-layout { display: grid; grid-template-columns: minmax(0, 1fr) minmax(0, 1fr); align-items: start; gap: var(--ui-section-gap); }
  .live-input-layout.buttons-only, .live-input-layout.analog-only { grid-template-columns: minmax(0, 1fr); }
  .analog-inputs, .button-inputs { min-width: 0; }
  .analog-inputs { display: grid; gap: 8px; }
  .analog-control { display: grid; grid-template-columns: minmax(48px, auto) minmax(0, 1fr) auto; gap: 5px 12px; align-items: center; padding: 10px 12px; border: 1px solid var(--line); border-radius: 6px; background: var(--input-bg); }
  strong { font-size: 11px; font-weight: 570; color: var(--text); }
  .input-value { font-size: 10px; color: var(--soft); line-height: 1.6; }
  .analog-control > :global(.input-value) { grid-column: 3; grid-row: 1; text-align: right; }
  small { font-size: 9px; color: var(--muted); line-height: 1.6; overflow-wrap: anywhere; }
  .analog-control > :global(small) { grid-column: 1 / -1; }
  meter { width: 100%; min-width: 0; accent-color: var(--accent); }
  .button-matrix { display: grid; grid-template-columns: repeat(auto-fill, minmax(min(100%, var(--ui-input-cell-min-width)), 1fr)); gap: var(--ui-input-cell-gap); }
  .button-slot { position: relative; min-width: 0; height: var(--ui-input-cell-height); }
  .button-control { position: absolute; top: 0; left: 0; width: 100%; height: var(--ui-input-cell-height); min-height: var(--ui-input-cell-height); box-sizing: border-box; padding: 0; border: 1px solid var(--line); border-radius: 6px; color: var(--soft); background: var(--input-bg); text-align: left; cursor: help; }
  .button-number { display: flex; align-items: center; justify-content: center; gap: 4px; height: calc(var(--ui-input-cell-height) - 2px); font-size: 11px; font-weight: 570; font-variant-numeric: tabular-nums; }
  .button-information { display: none; flex-direction: column; gap: 5px; padding: 12px; }
  .pressed { border-color: var(--accent); background: var(--accent-bg); color: var(--accent); }
  .waiting { border-style: dashed; color: var(--muted); }
  .button-control:is(:hover, :focus):not(.dismissed) { top: var(--expanded-top, 0); bottom: var(--expanded-bottom, auto); left: var(--expanded-left, 0); width: var(--expanded-width, 240px); z-index: 5; border-color: var(--accent); background: var(--surface); box-shadow: 0 8px 28px #0004; }
  .button-control.pressed:is(:hover, :focus):not(.dismissed) { background: var(--accent-bg); }
  .button-control:is(:hover, :focus):not(.dismissed) .button-number { display: none; }
  .button-control:is(:hover, :focus):not(.dismissed) .button-information { display: flex; }
  .button-control:focus-visible { outline: 2px solid var(--accent); outline-offset: 2px; }
  .controls-empty { padding: 16px; border: 1px dashed var(--line); border-radius: 6px; font-size: var(--ui-text-size); color: var(--muted); line-height: var(--ui-line-height); }
  .simple { grid-template-columns: minmax(0, 1fr); gap: 14px; }
  .simple .analog-control { padding: 6px 0; border: 0; background: transparent; grid-template-columns: 64px minmax(0, 1fr) auto; gap: 8px; }
  .simple .analog-control strong, .simple .input-value { font-size: var(--ui-text-size); }
  .simple .analog-control > .input-value { grid-column: 3; grid-row: 1; text-align: right; font-variant-numeric: tabular-nums; }
  .button-indicator { display: grid; place-items: center; width: 100%; height: var(--ui-input-cell-height); box-sizing: border-box; border: 1px solid var(--line); border-radius: 4px; background: var(--input-bg); color: var(--soft); font-size: 11px; font-variant-numeric: tabular-nums; }
  .button-indicator.pressed { border-color: var(--accent); background: var(--accent-bg); color: var(--accent); }
  .button-indicator.waiting { border-style: dashed; color: var(--muted); }
  @media (max-width: 700px) { .live-input-layout { grid-template-columns: minmax(0, 1fr); gap: 16px; } }
</style>
