<script lang="ts">
  import type {Snippet} from 'svelte';
  import Icon from '../Icon.svelte';
  import ProfilePortTile from './ProfilePortTile.svelte';
  import ProfilePortPager from './ProfilePortPager.svelte';
  import ProfilePortFilter from './ProfilePortFilter.svelte';
  import {integrationPortPage,integrationPortTypeLabel,portContractLabel} from '../profile-canvas';
  import type {DeepReadonly,ProfilePort} from '../profiles';

  let {nodeId,ports,direction,readOnly=false,removePort,connections}: {
    nodeId:string;ports:readonly DeepReadonly<ProfilePort>[];direction:'input'|'output';readOnly?:boolean;
    removePort?:(nodeId:string,portId:string)=>void;connections?:Snippet<[string]>;
  }=$props();
  let query=$state(''),page=$state(0);
  const label=$derived(direction==='input'?'Inputs':'Outputs');
  const results=$derived(integrationPortPage(ports,query,page,10,direction));
  function filter(value:string){query=value;page=0;}
</script>

<section class="companion-ports" aria-label={`Companion ${label.toLowerCase()}`}>
  <div class="heading"><h4>{label}</h4><span>{results.total}</span></div>
  <ProfilePortFilter {direction} {query} change={filter}/>
  <div class="port-list" aria-label={direction==='input'?'Feedback inputs':'Control outputs'}>
    {#each results.rows as port (port.id)}
      <ProfilePortTile name={port.name} type={integrationPortTypeLabel(port.contract)} id={port.id} title={`${port.name} · ${port.id} · ${portContractLabel(port.contract)}`}>
        {@render connections?.(port.id)}
        {#if removePort}<button type="button" class="remove" aria-label={`Remove ${port.name} port and routes`} title={`Remove ${port.name} port and routes`} disabled={readOnly} onclick={()=>removePort?.(nodeId,port.id)}><Icon name="trash" size={14}/></button>{/if}
      </ProfilePortTile>
    {:else}<p class="status">{query.trim()?`No matching ${label.toLowerCase()}.`:`No ${label.toLowerCase()} selected.`}</p>{/each}
  </div>
  <ProfilePortPager label={label.toLowerCase()} page={results.page} pages={results.pages}
    offset={results.offset} count={results.rows.length} total={results.total} change={value=>page=value}/>
</section>

<style>
  .companion-ports{display:grid;grid-template-columns:minmax(0,1fr);gap:8px;min-width:0;padding-top:16px;border-top:1px solid var(--line)}
  .heading{display:flex;align-items:center;justify-content:space-between;gap:8px;color:var(--muted);font-size:var(--ui-text-size)}
  h4{margin:0;color:var(--soft);font-size:var(--ui-heading-size);font-weight:570}
  button{font-size:var(--ui-text-size);cursor:pointer}.remove{display:grid;place-items:center;flex:0 0 22px;width:22px;height:24px;padding:0;border:0;background:transparent;color:var(--muted)}.remove:hover:enabled{color:var(--danger,#e67979)}
  .port-list{display:grid;grid-template-columns:minmax(0,1fr);gap:var(--ui-input-cell-gap);min-width:0}
  button:focus-visible{outline:2px solid var(--accent);outline-offset:2px}button:disabled{cursor:default;opacity:.5}
  .status{margin:0;color:var(--muted);font-size:var(--ui-text-size);line-height:var(--ui-line-height)}
</style>
