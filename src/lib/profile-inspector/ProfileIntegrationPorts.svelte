<script lang="ts">
  import Icon from '../Icon.svelte';
  import ProfilePortTile from './ProfilePortTile.svelte';
  import ProfilePortPager from './ProfilePortPager.svelte';
  import ProfilePortFilter from './ProfilePortFilter.svelte';
  import {integrationPortPage,integrationPortTypeLabel,portContractLabel} from '../profile-canvas';
  import type {DeepReadonly} from '../profiles';
  import type {ProfileDiscoveredPort} from '../profile-discovery';

  let {ports,direction='output',busy=false,error='',readOnly=false,select,retry}: {
    ports:readonly DeepReadonly<ProfileDiscoveredPort>[];busy?:boolean;error?:string;readOnly?:boolean;
    direction?:'input'|'output';
    select:(port:DeepReadonly<ProfileDiscoveredPort>)=>void;retry:()=>void;
  }=$props();
  let query=$state(''),page=$state(0);
  const label=$derived(direction==='input'?'Inputs':'Outputs');
  const results=$derived(integrationPortPage(ports,query,page,10,direction));
  function filter(value:string){query=value;page=0;}
  function choose(port:DeepReadonly<ProfileDiscoveredPort>){if(!readOnly&&!busy&&!error)select(port);}
</script>

<section class="integration-ports" aria-label={`Integration ${label.toLowerCase()}`}>
  <div class="heading"><h4>{label}</h4><span>{results.total}</span></div>
  <ProfilePortFilter {direction} {query} change={filter}/>
  {#if busy}<p class="status" role="status">Loading {label.toLowerCase()}…</p>{/if}
  {#if error}<div class="error" role="alert"><span>{error}</span><button type="button" class="retry" disabled={busy} onclick={retry}><Icon name="refresh" size={14}/>Retry</button></div>{/if}
  <div class="port-list" aria-label={direction==='input'?'Input signals':'Output signals'} aria-busy={busy}>
    {#each results.rows as port (port.key)}
      <ProfilePortTile name={port.name} type={integrationPortTypeLabel(port.contract)} portKey={port.key}
        unavailable={!port.current} title={`${port.name} · ${port.observed?.descriptor.id??port.id} · ${portContractLabel(port.contract)}${port.current?'':' · Unavailable'}`} disabled={readOnly||busy||!!error} action={()=>choose(port)}>
        <Icon name="link" size={13}/>
      </ProfilePortTile>
    {:else}{#if !busy}<p class="status" role="status">{query.trim()?`No matching ${label.toLowerCase()}.`:`No ${label.toLowerCase()} available.`}</p>{/if}{/each}
  </div>
  <ProfilePortPager label={label.toLowerCase()} page={results.page} pages={results.pages}
    offset={results.offset} count={results.rows.length} total={results.total} change={value=>page=value}/>
</section>

<style>
  .integration-ports{display:grid;grid-template-columns:minmax(0,1fr);gap:8px;min-width:0;padding-top:16px;border-top:1px solid var(--line)}
  .heading{display:flex;flex-wrap:wrap;align-items:center;justify-content:space-between;gap:2px 8px;min-width:0;color:var(--muted);font-size:var(--ui-text-size)}
  h4{margin:0;color:var(--soft);font-size:var(--ui-heading-size);font-weight:570}
  button{font-size:var(--ui-text-size);cursor:pointer}
  .port-list{display:grid;grid-template-columns:minmax(0,1fr);gap:var(--ui-input-cell-gap);min-width:0}
  button:focus-visible{outline:2px solid var(--accent);outline-offset:2px}button:disabled{cursor:default;opacity:.5}
  .status{min-width:0;margin:0;color:var(--muted);font-size:var(--ui-text-size);line-height:var(--ui-line-height);overflow-wrap:anywhere}.error{display:grid;gap:6px;color:var(--danger,#e67979);font-size:var(--ui-text-size);overflow-wrap:anywhere}.retry{display:inline-flex;align-items:center;justify-self:start;gap:5px;padding:3px 0;border:0;background:transparent;color:var(--accent)}
</style>
