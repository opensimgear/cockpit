<script lang="ts" generics="T extends string">
  let { label, options, values = $bindable<T[]>([]) }: {
    label: string; options: readonly { value: T; label: string }[]; values?: T[];
  } = $props();
</script>

<fieldset>
  <legend>{label}</legend>
  <div class="filter-options">
    {#each options as option (option.value)}
      <label class:selected={values.includes(option.value)}><input type="checkbox" value={option.value} bind:group={values} /><span>{option.label}</span></label>
    {/each}
  </div>
</fieldset>

<style>
  fieldset { min-width: 0; border: 0; margin: 0; padding: 0; }
  legend { margin-bottom: 10px; padding: 0; font-size:var(--ui-text-size); font-weight: 570; color: var(--soft); }
  .filter-options { display: flex; flex-wrap: wrap; gap: 8px; }
  label { display: inline-flex; align-items: center; gap: 7px; min-height: 34px; padding: 7px 10px; border: 1px solid var(--line); border-radius: 5px; background: var(--input-bg); font-size:var(--ui-text-size); color: var(--soft); cursor: pointer; }
  label:hover { border-color: var(--muted); background: var(--surface-raised); }
  label.selected { border-color: var(--accent); color: var(--text); background: var(--accent-bg); }
  label:focus-within { outline: 2px solid var(--accent); outline-offset: 2px; }
  input { width: 13px; height: 13px; margin: 0; flex-shrink: 0; accent-color: var(--accent); cursor: pointer; }
</style>
