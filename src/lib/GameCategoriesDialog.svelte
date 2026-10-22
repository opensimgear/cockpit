<script lang="ts">
  import {untrack} from 'svelte';
  import {categoryError,type GameCategories,type GameRequest,type GamesState} from './games';
  let {categories,request,close,saved}:{categories:GameCategories;request:(value:GameRequest)=>Promise<GamesState|false>;close:()=>void;saved:(value:GamesState)=>void}=$props();
  const original=untrack(()=>({...categories}));
  let entries=$state(Object.entries(original).map(([id,name])=>({id,name}))),pending=$state(false),error=$state('');
  const draft=$derived(Object.fromEntries(entries.map(entry=>[entry.id,entry.name.trim()])));
  const validation=$derived(categoryError(draft));
  const removed=$derived(Object.keys(original).filter(id=>!entries.some(entry=>entry.id===id)).length);
  function show(node:HTMLDialogElement){node.showModal();return()=>node.close();}
  async function save(){
    if(pending||validation)return;pending=true;error='';
    try{const result=await request({command:'game_categories_save',categories:draft,expectedCategories:original});if(!result||result.error)throw new Error(result&&result.error||'Categories were not saved. Try again.');saved(result);}
    catch(cause){error=cause instanceof Error?cause.message:String(cause);}finally{pending=false;}
  }
</script>

<dialog class="categories-dialog" aria-labelledby="game-categories-title" oncancel={event=>{event.preventDefault();if(!pending)close();}} {@attach show}>
  <h2 id="game-categories-title">Game categories</h2>
  <p>Create your own categories to organize and filter games. Renaming a category keeps its games assigned.</p>
  <form onsubmit={event=>{event.preventDefault();void save();}}>
    <div class="category-list">{#each entries as entry,index (entry.id)}
      <div class="category-row"><label><span>Category {index+1}</span><input aria-label={`Category ${index+1} name`} bind:value={entry.name} maxlength="48" disabled={pending}/></label><button type="button" class="secondary-button" disabled={pending} aria-label={`Delete category ${index+1}`} onclick={()=>entries=entries.filter(item=>item.id!==entry.id)}>Delete</button></div>
    {/each}</div>
    {#if !entries.length}<p>No categories yet. Games stay uncategorized until you assign one.</p>{/if}
    <button type="button" class="secondary-button add-category" disabled={pending||entries.length>=64} onclick={()=>entries=[...entries,{id:crypto.randomUUID(),name:''}]}>Add category</button>
    {#if removed}<p role="status">Deleting {removed===1?'this category':'these categories'} will leave its games uncategorized when you save.</p>{/if}
    {#if validation}<p class="validation">{validation}</p>{/if}
    {#if error}<p class="error" role="alert">{error}</p>{/if}
    <div class="actions"><button type="button" class="secondary-button" disabled={pending} onclick={close}>Cancel</button><button class="primary-button" disabled={pending||!!validation}>{pending?'Saving…':'Save categories'}</button></div>
  </form>
</dialog>

<style>
  .categories-dialog{position:fixed;inset:0;margin:auto;width:min(560px,calc(100% - 32px));max-height:calc(100% - 40px);overflow:auto;box-sizing:border-box;padding:24px;background:var(--surface);color:var(--text);border:1px solid var(--line);border-radius:10px;box-shadow:0 20px 80px #0005}.categories-dialog::backdrop{background:#0009}h2{font-size:18px;margin:0}p{color:var(--muted);font-size:12px;line-height:1.6;margin:16px 0}form,.category-list{display:grid;gap:14px}.category-row{display:flex;gap:10px;align-items:flex-end}label{display:grid;gap:7px;flex:1;min-width:0;font-size:12px;color:var(--soft)}input{width:100%;min-width:0;box-sizing:border-box;padding:10px;border:1px solid var(--line);border-radius:5px;background:var(--input-bg);color:var(--text)}.add-category{justify-self:start}.actions{display:flex;justify-content:flex-end;gap:10px;margin-top:10px}.error,.validation{color:#e5b19b;margin:0}
</style>
