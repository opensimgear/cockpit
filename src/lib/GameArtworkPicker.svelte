<script lang="ts">
  import Icon from './Icon.svelte';
  import {gatewayRequest} from './gateway';
  import type {Game, GameCrop, GameArtworkOption} from './games';
  let {game,current,currentCrop,selected='',disabled=false,selecting,choose}:{game:Game;current:string;currentCrop?:GameCrop;selected?:string;disabled?:boolean;selecting:(value:boolean)=>void;choose:(image:string,crop?:GameCrop)=>void}=$props();
  let items=$state<GameArtworkOption[]>([]),loading=$state(true),pending=$state(false),error=$state(''),selectedId=$state('');
  const gameId=$derived(game.id);
  let disposed=false;
  let generation=0;
  async function load(id:string){
    const requestGeneration=++generation;
    loading=true;error='';
    try{
      const result=await gatewayRequest({command:'game_artwork_list',gameId:id,limit:64});
      if(!result.gameArtwork?.items)throw new Error('Library images could not be loaded.');
      if(!disposed&&requestGeneration===generation)items=result.gameArtwork.items;
    }catch(cause){if(!disposed&&requestGeneration===generation)error=cause instanceof Error?cause.message:String(cause);}
    finally{if(!disposed&&requestGeneration===generation)loading=false;}
  }
  $effect(()=>{disposed=false;void load(gameId);return()=>{disposed=true;generation++;};});
  async function select(item:GameArtworkOption){
    if(pending||disabled)return;pending=true;selecting(true);error='';
    try{
      const result=await gatewayRequest({command:'game_artwork_preview',gameId:game.id,artworkId:item.id});
      if(!result.gameArtwork?.image)throw new Error('This library image could not be loaded.');
      if(!disposed){selectedId=item.id;choose(result.gameArtwork.image);}
    }catch(cause){if(!disposed)error=cause instanceof Error?cause.message:String(cause);}
    finally{if(!disposed){pending=false;selecting(false);}}
  }
</script>

<section class="artwork-picker" aria-label="Library artwork" aria-busy={loading||pending}>
  <p>Choose artwork, then move and scale the tile frame below.</p>
  <div class="artwork-grid">
    {#if current}<button class="artwork-option" class:selected={selected===current} aria-pressed={selected===current} disabled={disabled||pending} onclick={()=>{selectedId='';choose(current,currentCrop);}}><img src={current} alt=""/><span><Icon name="image" size={14}/>Current image</span></button>{/if}
    {#if game.background&&game.defaultBackground?.image&&game.defaultBackground.image!==current}<button class="artwork-option" class:selected={selected===game.defaultBackground.image} aria-pressed={selected===game.defaultBackground.image} disabled={disabled||pending} onclick={()=>{selectedId='';choose(game.defaultBackground!.image,game.defaultBackground!.crop);}}><img src={game.defaultBackground.image} alt=""/><span><Icon name="image" size={14}/>Default image</span></button>{/if}
    {#each items as item (item.id)}<button data-artwork-id={item.id} class="artwork-option" class:selected={selectedId===item.id} aria-pressed={selectedId===item.id} disabled={disabled||pending} onclick={()=>select(item)}><img src={item.image} alt="" loading="lazy"/><span><Icon name="image" size={14}/>{item.label}</span></button>{/each}
  </div>
  {#if loading}<p role="status">Loading library images…</p>{:else if !items.length&&!error}<p>No additional artwork is available in this library cache. You can also choose an image from disk or a URL.</p>{/if}
  {#if pending}<p role="status">Loading selected image…</p>{/if}
  {#if error}<div class="artwork-error" role="alert">{error}<button class="secondary-button" disabled={loading||pending||disabled} onclick={()=>load(gameId)}><Icon name="refresh" size={14}/>Retry</button></div>{/if}
</section>

<style>
  .artwork-picker{min-width:0}.artwork-picker p{font-size:var(--ui-text-size);color:var(--muted);line-height:var(--ui-line-height);margin:0 0 12px}.artwork-grid{display:grid;grid-template-columns:repeat(auto-fill,minmax(140px,1fr));gap:10px;max-height:260px;overflow:auto;padding:2px}.artwork-option{display:flex;flex-direction:column;gap:8px;min-width:0;text-align:left;padding:7px;background:var(--surface-raised);color:var(--text);border:1px solid var(--line);border-radius:6px}.artwork-option:hover,.artwork-option:focus-visible,.artwork-option.selected{border-color:var(--accent);outline-color:var(--accent)}.artwork-option img{width:100%;height:88px;object-fit:contain;background:var(--bg);border-radius:3px}.artwork-option span{display:flex;align-items:center;gap:6px;font-size:var(--ui-text-size);line-height:1.4;overflow-wrap:anywhere}.artwork-option span :global(svg){flex-shrink:0}.artwork-error{display:flex;align-items:center;gap:12px;color:#e5b19b;font-size:var(--ui-text-size);margin-top:12px}
</style>
