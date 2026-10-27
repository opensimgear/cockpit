<script lang="ts">
  import Icon from './Icon.svelte';
  import MultiSelectFilter from './filters/MultiSelectFilter.svelte';
  import { emptySystemFilters, presenceLabels, systemTypeLabels, type SystemFilters, type SystemType, type SystemPresence, type SystemConnection } from './system-inventory';
  let { filters = $bindable<SystemFilters>(emptySystemFilters()) }: { filters?: SystemFilters } = $props();
  const count = $derived(Object.values(filters).reduce((sum, values) => sum + values.length, 0));
  const types = (Object.entries(systemTypeLabels) as [SystemType, string][]).filter(([type]) => type !== 'host').map(([value, label]) => ({ value, label }));
  const presence = (Object.entries(presenceLabels) as [SystemPresence, string][]).map(([value, label]) => ({ value, label }));
  const connections: { value: SystemConnection['transport']; label: string }[] = [
    { value: 'usb', label: 'USB' }, { value: 'can', label: 'CAN' }, { value: 'network', label: 'Network' },
    { value: 'bluetooth', label: 'Bluetooth' }, { value: 'software', label: 'Virtual' }, { value: 'unknown', label: 'Unknown' },
  ];
  const visibility: { value: SystemFilters['visibility'][number]; label: string }[] = [
    { value: 'hidden', label: 'Hidden' }, { value: 'visible', label: 'Visible' }, { value: 'unknown', label: 'Unknown' },
  ];
</script>

<div class="device-filters">
  <div class="filters-heading"><div><h2>Filters</h2><p>Select multiple options in each group. An empty group includes all.</p></div><button class="text-button" disabled={!count} onclick={() => { filters = emptySystemFilters(); }}><Icon name="close" size={13} />Clear</button></div>
  <div class="filter-groups">
    <MultiSelectFilter label="Type" options={types} bind:values={filters.types} />
    <MultiSelectFilter label="Connection type" options={connections} bind:values={filters.connections} />
    <MultiSelectFilter label="Status" options={presence} bind:values={filters.presence} />
    <MultiSelectFilter label="Visibility" options={visibility} bind:values={filters.visibility} />
  </div>
</div>

<style>
  .device-filters { margin-bottom: var(--ui-section-gap); padding: var(--ui-panel-padding); border: 1px solid var(--line); border-radius: 8px; background: var(--surface); }
  .filters-heading { display: flex; align-items: flex-start; justify-content: space-between; gap: 16px; margin-bottom: var(--ui-section-gap); }
  h2 { font-size: var(--ui-heading-size); font-weight: 570; margin: 0 0 7px; }
  p { font-size: var(--ui-text-size); color: var(--muted); line-height: var(--ui-line-height); }
  .filters-heading button { display: inline-flex; align-items: center; gap: 5px; flex-shrink: 0; }
  .filter-groups { display: grid; grid-template-columns: repeat(2, minmax(0, 1fr)); gap: var(--ui-section-gap); }
  @media(max-width:700px) { .filter-groups { grid-template-columns: minmax(0, 1fr); } .device-filters { padding: 16px; } }
</style>
