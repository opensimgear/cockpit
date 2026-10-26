<script lang="ts">
  import Icon from './Icon.svelte';
  import GameTagInput from './GameTagInput.svelte';
  import MultiSelectFilter from './filters/MultiSelectFilter.svelte';
  import {emptyGameListFilters,gameListLibraries,UNTAGGED_TAG_FILTER,type Game,type GameTags,type GameListFilterState} from './games';
  let {games,tags,filters=$bindable<GameListFilterState>(emptyGameListFilters()),busy=false,manageTags}:{games:Game[];tags:GameTags;filters?:GameListFilterState;busy?:boolean;manageTags:()=>void}=$props();
  const count=$derived(filters.tags.length+filters.libraries.length+filters.integrations.length);
  const tagOptions=$derived({...tags,[UNTAGGED_TAG_FILTER]:'Untagged'});
  const libraries=$derived(gameListLibraries(games));
  const integrations:{value:GameListFilterState['integrations'][number];label:string}[]=[{value:'available',label:'Available'},{value:'unavailable',label:'Not available'}];
</script>

<section class="game-filters" aria-label="Game filters">
  <div class="filters-heading"><div><h2>Filters</h2><p>Select multiple options in each group. An empty group includes all.</p></div><div class="filter-actions"><button class="text-button" disabled={busy} aria-label="Manage tags" onclick={manageTags}><Icon name="settings" size={13}/>Manage</button><button class="text-button" disabled={!count} onclick={()=>filters=emptyGameListFilters()}><Icon name="close" size={13}/>Clear</button></div></div>
  <div class="filter-groups">
    <GameTagInput label="Tags" tags={tagOptions} values={filters.tags} disabled={busy} onchange={values=>{filters={...filters,tags:values};}}/>
    <MultiSelectFilter label="Library" options={libraries} bind:values={filters.libraries}/>
    <MultiSelectFilter label="Integration available" options={integrations} bind:values={filters.integrations}/>
  </div>
</section>

<style>
  .game-filters{margin-bottom:24px;padding:20px;border:1px solid var(--line);border-radius:8px;background:var(--surface)}
  .filters-heading{display:flex;align-items:flex-start;justify-content:space-between;gap:16px;margin-bottom:20px}
  h2{font-size:12px;font-weight:570;margin:0 0 7px}p{font-size:10px;color:var(--muted);line-height:1.7;margin:0}
  .filter-actions{display:flex;align-items:center;gap:14px;flex-wrap:wrap}.filter-actions button{display:inline-flex;align-items:center;gap:5px}
  .filter-groups{display:grid;grid-template-columns:repeat(3,minmax(0,1fr));gap:22px 28px}
  @media(max-width:800px){.filter-groups{grid-template-columns:1fr}.filters-heading{flex-wrap:wrap}.game-filters{padding:16px}}
</style>
