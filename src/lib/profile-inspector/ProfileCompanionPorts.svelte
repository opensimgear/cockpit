<script lang="ts">
  import type {Snippet} from 'svelte';
  import Icon from '../Icon.svelte';
  import {integrationPortPage,integrationPortTypeLabel,portContractLabel} from '../profile-canvas';
  import type {DeepReadonly,ProfilePort} from '../profiles';

  let {nodeId,ports,direction,readOnly=false,removePort,connections}: {
    nodeId:string;ports:readonly DeepReadonly<ProfilePort>[];direction:'input'|'output';readOnly?:boolean;
    removePort?:(nodeId:string,portId:string)=>void;connections?:Snippet<[string]>;
  }=$props();
  let query=$state(''),page=$state(0),filterInput:HTMLInputElement;
  const label=$derived(direction==='input'?'Inputs':'Outputs');
  const results=$derived(integrationPortPage(ports,query,page,10,direction));
  function filter(event:Event){query=(event.currentTarget as HTMLInputElement).value;page=0;}
  function clear(){query='';page=0;filterInput?.focus();}
</script>

<section class="companion-ports" aria-label={`Companion ${label.toLowerCase()}`}>
  <div class="heading"><h4>{label}</h4><span>{results.total}</span></div>
  <div class="quick-filter"><Icon name="search" size={14}/><input bind:this={filterInput} aria-label={`Filter ${label.toLowerCase()}`} placeholder={`Filter ${label.toLowerCase()}…`} value={query} oninput={filter}/>{#if query}<button type="button" class="clear" aria-label={`Clear ${direction} filter`} onclick={clear}><Icon name="close" size={13}/></button>{/if}</div>
  <div class="port-list" aria-label={direction==='input'?'Feedback inputs':'Control outputs'}>
    {#each results.rows as port (port.id)}
      <div class="port-tile" data-port-id={port.id} title={`${port.name} · ${port.id} · ${portContractLabel(port.contract)}`}>
        <strong>{port.name}</strong><span class="type">{integrationPortTypeLabel(port.contract)}</span>
        {@render connections?.(port.id)}
        {#if removePort}<button type="button" class="remove" aria-label={`Remove ${port.name} port and routes`} title={`Remove ${port.name} port and routes`} disabled={readOnly} onclick={()=>removePort?.(nodeId,port.id)}><Icon name="trash" size={14}/></button>{/if}
      </div>
    {:else}<p class="status">{query.trim()?`No matching ${label.toLowerCase()}.`:`No ${label.toLowerCase()} selected.`}</p>{/each}
  </div>
  {#if results.pages>1}<div class="paging"><span>{results.offset+1}–{results.offset+results.rows.length} / {results.total}</span><div>
    <button type="button" class="previous" aria-label={`Previous ${label.toLowerCase()}`} disabled={results.page===0} onclick={()=>page=results.page-1}><Icon name="chevron" size={14}/></button>
    <button type="button" aria-label={`Next ${label.toLowerCase()}`} disabled={results.page+1>=results.pages} onclick={()=>page=results.page+1}><Icon name="chevron" size={14}/></button>
  </div></div>{/if}
</section>

<style>
  .companion-ports{display:grid;grid-template-columns:minmax(0,1fr);gap:8px;min-width:0;padding-top:16px;border-top:1px solid var(--line)}
  .heading{display:flex;align-items:center;justify-content:space-between;gap:8px;color:var(--muted);font-size:var(--ui-text-size)}
  h4{margin:0;color:var(--soft);font-size:var(--ui-heading-size);font-weight:570}
  .quick-filter{display:flex;align-items:center;gap:6px;min-width:0;min-height:28px;padding:0 7px;border:1px solid var(--line);border-radius:5px;background:var(--input-bg);color:var(--muted)}
  .quick-filter:focus-within{border-color:var(--accent)}input{min-width:0;width:100%;height:28px;box-sizing:border-box;padding:0;border:0;outline:0;background:transparent;color:var(--text);font:inherit;font-size:var(--ui-text-size)}input:focus-visible{outline:0}input::placeholder{color:var(--muted)}
  button{font-size:var(--ui-text-size);cursor:pointer}.clear,.remove{display:grid;place-items:center;flex:0 0 22px;width:22px;height:24px;padding:0;border:0;background:transparent;color:var(--muted)}.remove:hover:enabled{color:var(--danger,#e67979)}
  .port-list{display:grid;grid-template-columns:minmax(0,1fr);gap:var(--ui-input-cell-gap);min-width:0}
  .port-tile{display:flex;align-items:center;gap:7px;box-sizing:border-box;width:100%;min-width:0;min-height:var(--ui-input-cell-height);padding:5px 8px;border:1px solid var(--line);border-radius:4px;background:var(--input-bg);color:var(--soft);text-align:left;font-size:var(--ui-text-size)}
  .port-tile strong{flex:1;min-width:0;overflow:hidden;text-overflow:ellipsis;white-space:nowrap;font-weight:570}.type{max-width:40%;overflow:hidden;text-overflow:ellipsis;white-space:nowrap;color:var(--muted);font-size:var(--ui-action-size)}
  button:focus-visible{outline:2px solid var(--accent);outline-offset:2px}button:disabled{cursor:default;opacity:.5}
  .status{margin:0;color:var(--muted);font-size:var(--ui-text-size);line-height:var(--ui-line-height)}
  .paging{display:flex;align-items:center;justify-content:space-between;gap:8px;color:var(--muted);font-size:var(--ui-text-size)}.paging>div{display:flex;gap:4px}.paging button{display:grid;place-items:center;width:26px;height:26px;padding:0;border:1px solid var(--line);border-radius:4px;background:transparent;color:var(--soft)}.previous :global(svg){transform:rotate(180deg)}
</style>
