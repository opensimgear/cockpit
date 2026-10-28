<script lang="ts">
  import Icon from '../Icon.svelte';
  import {integrationPortPage,integrationPortTypeLabel,portContractLabel} from '../profile-canvas';
  import type {DeepReadonly} from '../profiles';
  import type {ProfileDiscoveredPort} from '../profile-discovery';

  let {ports,direction='output',busy=false,error='',readOnly=false,select,retry}: {
    ports:readonly DeepReadonly<ProfileDiscoveredPort>[];busy?:boolean;error?:string;readOnly?:boolean;
    direction?:'input'|'output';
    select:(port:DeepReadonly<ProfileDiscoveredPort>)=>void;retry:()=>void;
  }=$props();
  let query=$state(''),page=$state(0),filterInput:HTMLInputElement;
  const label=$derived(direction==='input'?'Inputs':'Outputs');
  const results=$derived(integrationPortPage(ports,query,page,32,direction));
  function filter(event:Event){query=(event.currentTarget as HTMLInputElement).value;page=0;}
  function clear(){query='';page=0;filterInput?.focus();}
  function choose(port:DeepReadonly<ProfileDiscoveredPort>){if(!readOnly&&!busy&&!error)select(port);}
</script>

<section class="integration-ports" aria-label={`Integration ${label.toLowerCase()}`}>
  <div class="heading"><h4>{label}</h4><span>{results.total}</span></div>
  <div class="quick-filter"><Icon name="search" size={14}/><input bind:this={filterInput} aria-label={`Filter ${label.toLowerCase()}`} placeholder={`Filter ${label.toLowerCase()}…`} value={query} oninput={filter}/>{#if query}<button type="button" class="clear" aria-label={`Clear ${direction} filter`} onclick={clear}><Icon name="close" size={13}/></button>{/if}</div>
  {#if busy}<p class="status" role="status">Loading {label.toLowerCase()}…</p>{/if}
  {#if error}<div class="error" role="alert"><span>{error}</span><button type="button" class="retry" disabled={busy} onclick={retry}><Icon name="refresh" size={14}/>Retry</button></div>{/if}
  <div class="port-list" aria-label={direction==='input'?'Input signals':'Output signals'} aria-busy={busy}>
    {#each results.rows as port (port.key)}
      <button type="button" class="port-tile" class:unavailable={!port.current} data-port-key={port.key} disabled={readOnly||busy||!!error}
        aria-label={`Connect ${port.name}`} title={`${port.name} · ${port.observed?.descriptor.id??port.id} · ${portContractLabel(port.contract)}${port.current?'':' · Unavailable'}`} onclick={()=>choose(port)}>
        <strong>{port.name}</strong><span class="type">{integrationPortTypeLabel(port.contract)}</span><Icon name="link" size={13}/>
      </button>
    {:else}{#if !busy}<p class="status" role="status">{query.trim()?`No matching ${label.toLowerCase()}.`:`No ${label.toLowerCase()} available.`}</p>{/if}{/each}
  </div>
  {#if results.pages>1}<div class="paging"><span>{results.offset+1}–{results.offset+results.rows.length} / {results.total}</span><div>
    <button type="button" class="previous" aria-label={`Previous ${label.toLowerCase()}`} disabled={results.page===0} onclick={()=>page=results.page-1}><Icon name="chevron" size={14}/></button>
    <button type="button" aria-label={`Next ${label.toLowerCase()}`} disabled={results.page+1>=results.pages} onclick={()=>page=results.page+1}><Icon name="chevron" size={14}/></button>
  </div></div>{/if}
</section>

<style>
  .integration-ports{display:grid;grid-template-columns:minmax(0,1fr);gap:8px;min-width:0;padding-top:16px;border-top:1px solid var(--line)}
  .heading{display:flex;flex-wrap:wrap;align-items:center;justify-content:space-between;gap:2px 8px;min-width:0;color:var(--muted);font-size:var(--ui-text-size)}
  h4{margin:0;color:var(--soft);font-size:var(--ui-heading-size);font-weight:570}
  .quick-filter{display:flex;align-items:center;gap:6px;min-width:0;min-height:28px;padding:0 7px;border:1px solid var(--line);border-radius:5px;background:var(--input-bg);color:var(--muted)}
  .quick-filter:focus-within{border-color:var(--accent)}input{min-width:0;width:100%;height:28px;box-sizing:border-box;padding:0;border:0;outline:0;background:transparent;color:var(--text);font:inherit;font-size:var(--ui-text-size)}input:focus-visible{outline:0}input::placeholder{color:var(--muted)}
  button{font-size:var(--ui-text-size);cursor:pointer}.clear{display:grid;place-items:center;flex:0 0 22px;width:22px;height:24px;padding:0;border:0;background:transparent;color:var(--muted)}
  .port-list{display:grid;grid-template-columns:minmax(0,1fr);gap:var(--ui-input-cell-gap);min-width:0}
  .port-tile{display:flex;align-items:center;gap:7px;box-sizing:border-box;width:100%;min-width:0;min-height:var(--ui-input-cell-height);padding:5px 8px;border:1px solid var(--line);border-radius:4px;background:var(--input-bg);color:var(--soft);text-align:left}
  .port-tile strong{flex:1;min-width:0;overflow:hidden;text-overflow:ellipsis;white-space:nowrap;font-weight:570}.type{max-width:40%;overflow:hidden;text-overflow:ellipsis;white-space:nowrap;color:var(--muted);font-size:var(--ui-action-size)}
  .port-tile:hover:enabled{border-color:var(--accent)}.port-tile.unavailable{border-style:dashed}.port-tile :global(svg){flex-shrink:0;color:var(--muted)}
  button:focus-visible{outline:2px solid var(--accent);outline-offset:2px}button:disabled{cursor:default;opacity:.5}
  .status{min-width:0;margin:0;color:var(--muted);font-size:var(--ui-text-size);line-height:var(--ui-line-height);overflow-wrap:anywhere}.error{display:grid;gap:6px;color:var(--danger,#e67979);font-size:var(--ui-text-size);overflow-wrap:anywhere}.retry{display:inline-flex;align-items:center;justify-self:start;gap:5px;padding:3px 0;border:0;background:transparent;color:var(--accent)}
  .paging{display:flex;flex-wrap:wrap;align-items:center;justify-content:space-between;gap:8px;color:var(--muted);font-size:var(--ui-text-size)}.paging>div{display:flex;gap:4px}.paging button{display:grid;place-items:center;width:26px;height:26px;padding:0;border:1px solid var(--line);border-radius:4px;background:transparent;color:var(--soft)}.previous :global(svg){transform:rotate(180deg)}
</style>
