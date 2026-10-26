<script lang="ts">
  import Icon from './Icon.svelte';
  import {tick,untrack} from 'svelte';
  import {tagError,type GameTags,type GameRequest,type GamesState} from './games';
  let {tags,request,close,saved}:{tags:GameTags;request:(value:GameRequest)=>Promise<GamesState|false>;close:()=>void;saved:(value:GamesState)=>void}=$props();
  const original=untrack(()=>({...tags}));
  let entries=$state(Object.entries(original).map(([id,name])=>({id,name}))),pending=$state(false),error=$state(''),search=$state(''),editingId=$state<string|null>(null);
  let list:HTMLDivElement;
  async function add(){search='';editingId=null;entries=[...entries,{id:crypto.randomUUID(),name:''}];await tick();list.scrollTop=list.scrollHeight;list.querySelector<HTMLInputElement>('.tag-row:last-child input')?.focus();}
  const visible=$derived(entries.filter(entry=>entry.id===editingId||entry.name.toLowerCase().includes(search.trim().toLowerCase())));
  const draft=$derived(Object.fromEntries(entries.map(entry=>[entry.id,entry.name.trim()])));
  const validation=$derived(tagError(draft));
  const removed=$derived(Object.keys(original).filter(id=>!entries.some(entry=>entry.id===id)).length);
  function show(node:HTMLDialogElement){node.showModal();return()=>node.close();}
  async function save(){
    if(pending||validation)return;pending=true;error='';
    try{const result=await request({command:'game_tag_definitions_save',tags:draft,expectedTags:original});if(!result||result.error)throw new Error(result&&result.error||'Tags were not saved. Try again.');saved(result);}
    catch(cause){error=cause instanceof Error?cause.message:String(cause);}finally{pending=false;}
  }
</script>

<dialog class="tags-dialog" aria-labelledby="game-tags-title" oncancel={event=>{event.preventDefault();if(!pending)close();}} {@attach show}>
  <h2 id="game-tags-title">Game tags</h2>
  <p>Create and rename tags to organize and filter games. Games can have several tags. Renaming keeps existing assignments.</p>
  <form onsubmit={event=>{event.preventDefault();void save();}}>
    <label class="search"><span>Search tags</span><input aria-label="Search tags" type="search" bind:value={search} oninput={()=>editingId=null} placeholder="Search tags…" disabled={pending}/></label>
    <div class="tag-list" bind:this={list}>{#each visible as entry,index (entry.id)}
      <div class="tag-row" data-tag-id={entry.id}><label><span>Tag {index+1}</span><input aria-label={`Tag ${index+1} name`} bind:value={entry.name} onfocus={()=>editingId=entry.id} maxlength="48" disabled={pending}/></label><button type="button" class="secondary-button" disabled={pending} aria-label={`Delete tag ${index+1}`} onclick={()=>entries=entries.filter(item=>item.id!==entry.id)}><Icon name="trash" size={13}/>Delete</button></div>
    {/each}</div>
    {#if !entries.length}<p>No tags yet. Games stay untagged until you assign one.</p>{/if}
    {#if entries.length&&!visible.length}<p>No matching tags</p>{/if}
    <button type="button" class="secondary-button add-tag" disabled={pending||entries.length>=128} onclick={()=>void add()}><Icon name="plus" size={13}/>Add</button>
    {#if removed}<p role="status">Deleting {removed===1?'this tag':'these tags'} removes only those tags from games when you save. Other assigned tags stay.</p>{/if}
    {#if validation}<p class="validation">{validation}</p>{/if}
    {#if error}<p class="error" role="alert">{error}</p>{/if}
    <div class="actions"><button type="button" class="secondary-button" disabled={pending} onclick={close}><Icon name="close" size={13}/>Cancel</button><button class="primary-button" disabled={pending||!!validation}><Icon name="save" size={13}/>{pending?'Saving…':'Save'}</button></div>
  </form>
</dialog>

<style>
  .tags-dialog{position:fixed;inset:0;margin:auto;width:min(560px,calc(100% - 32px));max-height:calc(100% - 40px);overflow:auto;box-sizing:border-box;padding:24px;background:var(--surface);color:var(--text);border:1px solid var(--line);border-radius:10px;box-shadow:0 20px 80px #0005}.tags-dialog::backdrop{background:#0009}h2{font-size:18px;margin:0}p{color:var(--muted);font-size:12px;line-height:1.6;margin:16px 0}form,.tag-list{display:grid;gap:14px}.tag-list{max-height:340px;overflow-y:auto;padding:2px}button{display:inline-flex;align-items:center;justify-content:center;gap:6px}.tag-row{display:flex;gap:10px;align-items:flex-end}label{display:grid;gap:7px;flex:1;min-width:0;font-size:12px;color:var(--soft)}input{width:100%;min-width:0;box-sizing:border-box;padding:10px;border:1px solid var(--line);border-radius:5px;background:var(--input-bg);color:var(--text)}.add-tag{justify-self:start}.actions{display:flex;justify-content:flex-end;gap:10px;margin-top:10px}.error,.validation{color:#e5b19b;margin:0}
</style>
