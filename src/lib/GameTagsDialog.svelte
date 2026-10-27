<script lang="ts">
  import Icon from './Icon.svelte';
  import GameTagInput from './GameTagInput.svelte';
  import {tick,untrack} from 'svelte';
  import {tagError,type Game,type GameTags,type GameRequest,type GamesState} from './games';
  let {tags,games,request,close,saved}:{tags:GameTags;games:Pick<Game,'tagIds'>[];request:(value:GameRequest)=>Promise<GamesState|false>;close:()=>void;saved:(value:GamesState)=>void}=$props();
  const original=untrack(()=>({...tags}));
  let draft=$state({...original}),pending=$state(false),error=$state(''),deleting=$state<string|null>(null),dialog=$state<HTMLDialogElement>();
  const validation=$derived(tagError(draft));
  const affected=$derived(games.filter(game=>deleting&&game.tagIds?.includes(deleting)).length);
  const removed=$derived(Object.keys(original).filter(id=>!Object.hasOwn(draft,id)).length);
  function show(node:HTMLDialogElement){node.showModal();return()=>node.close();}
  function edit(id:string,name:string){const next={...draft,[id]:name},invalid=tagError(next);if(invalid)throw Error(invalid);draft=next;}
  function add(name:string){edit(crypto.randomUUID(),name);}
  function remove(ids:string[]){deleting=Object.keys(draft).find(id=>!ids.includes(id))??null;}
  async function dismissDelete(){deleting=null;await tick();dialog?.querySelector('textarea')?.focus();}
  function confirmDelete(){if(deleting){const next={...draft};delete next[deleting];draft=next;}void dismissDelete();}
  async function save(){
    if(pending||validation||deleting)return;pending=true;error='';
    try{const result=await request({command:'game_tag_definitions_save',tags:{...draft},expectedTags:original});if(!result||result.error)throw new Error(result&&result.error||'Tags were not saved. Try again.');saved(result);}
    catch(cause){error=cause instanceof Error?cause.message:String(cause);}finally{pending=false;}
  }
</script>

<dialog bind:this={dialog} class="tags-dialog" aria-labelledby="game-tags-title" oncancel={event=>{event.preventDefault();if(!pending)close();}} {@attach show}>
  <h2 id="game-tags-title">Game tags</h2>
  <p>Type a name and press Enter to add a tag. Select a badge to rename it, then press Enter. Renaming keeps existing assignments.</p>
  <form onsubmit={event=>{event.preventDefault();void save();}}>
    <GameTagInput tags={draft} values={Object.keys(draft)} label="Tags" placeholder="Add a tag…" multiline disabled={pending} removeLabel="Delete" onchange={remove} oncreate={add} onrename={edit}/>
    {#if !Object.keys(draft).length}<p>No tags yet. Add a name above to create one.</p>{/if}
    {#if removed}<p role="status">{removed} {removed===1?'tag will':'tags will'} be deleted when you save. Other assigned tags stay.</p>{/if}
    {#if validation}<p class="error">{validation}</p>{/if}
    {#if error}<p class="error" role="alert">{error}</p>{/if}
    <div class="actions"><button type="button" class="secondary-button" disabled={pending} onclick={close}><Icon name="close" size={13}/>Cancel</button><button class="primary-button" disabled={pending||!!validation}><Icon name="save" size={13}/>{pending?'Saving…':'Save'}</button></div>
  </form>
  {#if deleting}
    <dialog class="delete-dialog" role="alertdialog" aria-labelledby="delete-tag-title" aria-describedby="delete-tag-description" oncancel={event=>{event.preventDefault();void dismissDelete();}} {@attach show}>
      <h2 id="delete-tag-title">Delete “{draft[deleting]}”?</h2>
      <p id="delete-tag-description">This will remove the tag from {affected} {affected===1?'game':'games'} when you save. Other tags assigned to those games will stay.</p>
      <div class="actions"><button type="button" class="secondary-button" onclick={()=>void dismissDelete()}><Icon name="close" size={13}/>Cancel</button><button type="button" class="secondary-button destructive-button" onclick={confirmDelete}><Icon name="trash" size={13}/>Delete</button></div>
    </dialog>
  {/if}
</dialog>

<style>
  dialog{position:fixed;inset:0;margin:auto;width:min(620px,calc(100% - 32px));max-height:calc(100% - 40px);overflow:auto;box-sizing:border-box;padding:24px;background:var(--surface);color:var(--text);border:1px solid var(--line);border-radius:10px;box-shadow:0 20px 80px #0005}dialog::backdrop{background:#0009}h2{font-size:var(--ui-section-title-size);margin:0}p{color:var(--muted);font-size:var(--ui-text-size);line-height:var(--ui-line-height);margin:16px 0}form{display:grid;gap:14px}button{display:inline-flex;align-items:center;justify-content:center;gap:6px}.actions{display:flex;justify-content:flex-end;gap:10px;margin-top:10px}.error{color:#e5b19b;margin:0}.delete-dialog{width:min(440px,calc(100% - 32px))}
</style>
