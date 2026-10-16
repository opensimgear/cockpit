<script lang="ts">
  import Notification from './Notification.svelte';
  import { configurationError, mappingFields, mappingLimits, resizeMappings, type MappingKind, type RuntimeConfig } from './runtime-config';
  let { config = $bindable(), disabled = false }: { config: RuntimeConfig; disabled?: boolean } = $props();
  const kinds: MappingKind[] = ['buttons', 'axes', 'hats', 'encoders', 'selectors'];
  const error = $derived(configurationError(config));
  function expansion(bank: number, enabled: boolean) { config.expanderMask = enabled ? config.expanderMask | (1 << bank) : config.expanderMask & ~(1 << bank); }
</script>

<section class="config-editor" aria-label="Game peripheral configuration">
  <p class="field-help">These settings belong to this game profile. Activation applies them temporarily and recovery restores the previous settings.</p>
  <label class="config-name">Peripheral name <input class="text-input" bind:value={config.name} {disabled} maxlength="63" /></label>
  <div class="mapping-counts">
    {#each kinds as kind (kind)}
      <label>{kind}<input class="text-input" type="number" min="0" max={mappingLimits[kind]} value={config[kind].length} aria-label={`Game ${kind} count`} {disabled} onchange={event => resizeMappings(config, kind, Number(event.currentTarget.value))} /></label>
    {/each}
  </div>
  <details><summary>Expansion banks</summary><div class="expansion-banks">
    {#each Array.from({ length: 8 }, (_, index) => index) as bank (bank)}
      <label><input type="checkbox" checked={!!(config.expanderMask & (1 << bank))} {disabled} onchange={event => expansion(bank, event.currentTarget.checked)} /> Bank {bank + 1}</label>
    {/each}
  </div></details>
  {#each kinds as kind (kind)}
    {#if config[kind].length}
      <details open={kind === 'axes'}><summary>{kind.charAt(0).toUpperCase() + kind.slice(1)} · {config[kind].length}</summary>
        {#each config[kind] as row, index (index)}
          <fieldset {disabled}><legend>{kind} {index + 1}</legend><div class="mapping-fields">
            {#each mappingFields[kind] as field, column (column)}
              {#if field.toggle}<label class="toggle-field"><input type="checkbox" checked={row[column] === 1} aria-label={`Game ${kind} ${index + 1} ${field.label}`} onchange={event => { row[column] = event.currentTarget.checked ? 1 : 0; }} /> {field.label}</label>
              {:else}<label>{field.label}<input class="text-input" type="number" min={field.min} max={field.max} bind:value={row[column]} aria-label={`Game ${kind} ${index + 1} ${field.label}`} /></label>{/if}
            {/each}
          </div></fieldset>
        {/each}
        {#if kind === 'axes'}<p class="field-help">Raw calibration: 0–4095. Neutral: 0–65535. Deadzone: 0–32767. Usage 48–56 selects X, Y, Z, Rx, Ry, Rz, slider, dial or wheel.</p>{/if}
      </details>
    {/if}
  {/each}
  {#if error}<Notification message={String(error)} kind="error" />{/if}
</section>

<style>
  .config-editor{ border-top: 1px solid var(--line); padding-top: 14px; margin-top: 14px; }.config-name,.mapping-counts label,.mapping-fields label{ display: flex; flex-direction: column; gap: 7px; font-size: 10px; color: var(--soft); }.config-name{ margin-top: 12px; }.mapping-counts,.mapping-fields{ display: grid; grid-template-columns: repeat(auto-fit, minmax(115px, 1fr)); gap: 10px; margin-top: 12px; }.mapping-counts label{ text-transform: capitalize; }.text-input{ width: 100%; }details{ margin-top: 16px; }summary{ cursor: pointer; font-size: 11px; color: var(--soft); }summary:focus-visible{ outline: 2px solid var(--accent); outline-offset: 4px; }fieldset{ border: 1px solid var(--line); border-radius: 6px; padding: 10px; margin: 12px 0; min-width: 0; }legend{ color: var(--muted); font-size: 10px; padding: 0 5px; text-transform: capitalize; }.mapping-fields .toggle-field{ flex-direction: row; align-items: center; }.expansion-banks{ display: flex; flex-wrap: wrap; gap: 12px; padding-top: 12px; font-size: 10px; color: var(--soft); }
</style>
