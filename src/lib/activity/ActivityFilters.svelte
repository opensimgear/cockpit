<script lang="ts">
  import Icon from '../Icon.svelte';
  import MultiSelectFilter from '../filters/MultiSelectFilter.svelte';
  import { activityFilterCount, activityLabel, emptyActivityFilters, type ActivityFilters, type ActivityPage } from './model';
  let { filters = $bindable<ActivityFilters>(emptyActivityFilters()), page }: { filters?: ActivityFilters; page: ActivityPage | null } = $props();
  const options = (values: string[]) => values.map(value => ({ value, label: activityLabel(value) }));
</script>

<section class="activity-filters panel" aria-label="Activity filters">
  <div class="filters-heading"><div><h2>Filters</h2><p>Select multiple options in each group. An empty group includes all.</p></div><button class="text-button" disabled={!activityFilterCount(filters)} onclick={() => filters = emptyActivityFilters()}><Icon name="close" size={13}/>Clear</button></div>
  <div class="time-filters">
    <label><span>Time</span><select aria-label="Activity time" bind:value={filters.time}><option value="all">All retained activity</option><option value="hour">Last hour</option><option value="today">Today</option><option value="day">Last 24 hours</option><option value="week">Last 7 days</option><option value="custom">Custom range</option></select></label>
    {#if filters.time === 'custom'}<label><span>From</span><input aria-label="Activity from" type="datetime-local" bind:value={filters.from}/></label><label><span>Until</span><input aria-label="Activity until" type="datetime-local" bind:value={filters.until}/></label>{/if}
    <label class="entity-filter"><span>Entity ID</span><input aria-label="Activity entity ID" placeholder="Exact game, profile, or device ID" bind:value={filters.entityId}/></label>
  </div>
  <div class="filter-groups">
    <MultiSelectFilter label="Category" options={options(page?.categories ?? [])} bind:values={filters.categories}/>
    <MultiSelectFilter label="Severity" options={options(page?.levels ?? [])} bind:values={filters.levels}/>
    <MultiSelectFilter label="Type" options={options(page?.eventTypes ?? [])} bind:values={filters.eventTypes}/>
  </div>
</section>

<style>
  .activity-filters{padding:20px;margin-bottom:22px}.filters-heading{display:flex;align-items:flex-start;justify-content:space-between;gap:16px;margin-bottom:20px}h2{font-size:12px;margin:0 0 7px;font-weight:570}p{font-size:10px;color:var(--muted);line-height:1.7;margin:0}.time-filters{display:flex;flex-wrap:wrap;gap:18px;margin-bottom:22px}label{display:grid;gap:10px;font-size:11px;color:var(--soft)}input,select{min-height:38px;box-sizing:border-box;border:1px solid var(--line);border-radius:5px;background:var(--input-bg);color:var(--text);padding:9px;font:inherit;font-size:11px}.entity-filter{flex:1;min-width:220px}.filter-groups{display:grid;gap:22px}.filters-heading button{display:inline-flex;gap:5px;align-items:center}@media(max-width:620px){.activity-filters{padding:16px}.time-filters>label{width:100%;min-width:0}}
</style>
