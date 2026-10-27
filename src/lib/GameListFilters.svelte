<script lang="ts">
  import Icon from './Icon.svelte';
  import GameTagInput from './GameTagInput.svelte';
  import MultiSelectFilter from './filters/MultiSelectFilter.svelte';
  import {emptyGameListFilters,gameListLibraries,UNTAGGED_TAG_FILTER,type Game,type GameTags,type GameListFilterState} from './games';
  let {games,tags,filters=$bindable<GameListFilterState>(emptyGameListFilters()),busy=false,manageTags,onclear=()=>{}}:{games:Game[];tags:GameTags;filters?:GameListFilterState;busy?:boolean;manageTags:()=>void;onclear?:()=>void}=$props();
  const count=$derived(Number(!!filters.name.trim())+filters.tags.length+filters.libraries.length+filters.integrations.length+filters.profiles.length);
  const tagOptions=$derived({...tags,[UNTAGGED_TAG_FILTER]:'Untagged'});
  const libraries=$derived(gameListLibraries(games));
  const integrations:{value:GameListFilterState['integrations'][number];label:string}[]=[{value:'available',label:'Available'},{value:'unavailable',label:'Not available'}];
  const profiles:{value:GameListFilterState['profiles'][number];label:string}[]=[{value:'yes',label:'Yes'},{value:'no',label:'No'}];
</script>

<section class="game-filters" aria-label="Game filters">
  <div class="filters-heading"><div><h2>Filters</h2><p>Select multiple options in each group. An empty group includes all.</p></div><div class="filter-actions"><button class="text-button" disabled={!count} onclick={()=>{filters=emptyGameListFilters();onclear();}}><Icon name="close" size={13}/>Clear</button></div></div>
  <div class="filter-groups primary-filters">
    <label class="name-filter"><span>Name</span><input type="search" placeholder="Search games…" bind:value={filters.name}/></label>
    <div class="tag-filter"><GameTagInput label="Tags" tags={tagOptions} values={filters.tags} disabled={busy} onchange={values=>{filters={...filters,tags:values};}}/><button class="text-button manage-tags" disabled={busy} aria-label="Manage tags" onclick={manageTags}><Icon name="settings" size={13}/>Manage</button></div>
  </div>
  <div class="filter-groups secondary-filters">
    <MultiSelectFilter label="Library" options={libraries} bind:values={filters.libraries}/>
    <MultiSelectFilter label="Integration available" options={integrations} bind:values={filters.integrations}/>
    <MultiSelectFilter label="Has profile" options={profiles} bind:values={filters.profiles}/>
  </div>
</section>

<style>
  .game-filters{margin-bottom:24px;padding:20px;border:1px solid var(--line);border-radius:8px;background:var(--surface)}
  .filters-heading{display:flex;align-items:flex-start;justify-content:space-between;gap:16px;margin-bottom:20px}
  h2{font-size:var(--ui-text-size);font-weight:570;margin:0 0 7px}p{font-size:var(--ui-text-size);color:var(--muted);line-height:var(--ui-line-height);margin:0}
  .filter-actions{display:flex;align-items:center;gap:14px;flex-wrap:wrap}.filter-actions button{display:inline-flex;align-items:center;gap:5px}
  .filter-groups{display:grid;gap:22px 28px}.primary-filters{grid-template-columns:repeat(2,minmax(0,1fr))}.secondary-filters{display:flex;flex-wrap:wrap;align-items:flex-start;justify-content:flex-start;margin-top:22px}
  .name-filter{min-width:0}.name-filter>span{display:block;font-size:var(--ui-text-size);font-weight:570;color:var(--soft);margin-bottom:10px}.name-filter input{width:100%;min-height:40px;box-sizing:border-box;padding:10px;border:1px solid var(--line);border-radius:5px;background:var(--input-bg);color:var(--text);font:inherit;font-size:var(--ui-text-size)}.name-filter input:focus-visible{outline:2px solid var(--accent);outline-offset:2px}
  .tag-filter{position:relative;min-width:0}.manage-tags{position:absolute;top:0;right:0;padding:0;line-height:13px}
  @media(max-width:800px){.filter-groups{grid-template-columns:1fr}.secondary-filters{flex-direction:column}.filters-heading{flex-wrap:wrap}.game-filters{padding:16px}}
</style>
