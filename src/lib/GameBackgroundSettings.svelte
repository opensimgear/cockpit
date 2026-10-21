<script lang="ts">
  import { untrack } from 'svelte';
  import Icon from './Icon.svelte';
  import { pickGameImage } from './gateway';
  import { resolveGameArtwork } from './game-artwork';
  import type { Game, GameRequest, GamesState } from './games';
  let { game, busy = false, request }: { game: Game; busy?: boolean; request: (value: GameRequest) => Promise<GamesState | false> } = $props();
  let path = $state(''), pending = $state(false), error = $state('');
  let positionX = $state(untrack(() => resolveGameArtwork(game).positionX ?? 50)), positionY = $state(untrack(() => resolveGameArtwork(game).positionY ?? 50));
  const art = $derived(resolveGameArtwork(game));
  const dirty = $derived(positionX !== art.positionX || positionY !== art.positionY);
  let savedImage = untrack(() => art.image);
  $effect(() => { if (art.image !== savedImage) { savedImage = art.image; positionX = art.positionX ?? 50; positionY = art.positionY ?? 50; } });
  async function save(value: GameRequest) {
    if (pending || busy) return;
    pending = true; error = '';
    try {
      const result = await request(value);
      if (!result || result.error) throw new Error(result && result.error || 'The tile background was not saved.');
      path = '';
      const updated = result.catalogue.find(item => item.id === game.id);
      if (updated) { const next = resolveGameArtwork(updated); positionX = next.positionX ?? 50; positionY = next.positionY ?? 50; }
    } catch (cause) { error = cause instanceof Error ? cause.message : String(cause); }
    finally { pending = false; }
  }
  async function browse() {
    pending = true; error = '';
    try { const chosen = await pickGameImage(path); if (chosen) path = chosen; }
    catch (cause) { error = cause instanceof Error ? cause.message : String(cause); }
    finally { pending = false; }
  }
</script>

<section class="panel background-settings" aria-label="Tile background">
  <div class="background-heading"><h3><Icon name="image" size={18}/>Tile background</h3><button class="secondary-button" disabled={busy||pending||!game.background} onclick={()=>save({command:'game_background_save',gameId:game.id,path:null})}>Reset to default</button></div>
  <div class="background-layout">
    <div class="background-preview" aria-label="Tile background preview">
      {#if art.image}{#key art.image}<img src={art.image} alt="" style:object-position={`${positionX}% ${positionY}%`} onerror={event=>{(event.currentTarget as HTMLImageElement).style.display='none';}}/>{/key}{/if}
      <div class="preview-shade"></div><strong>{game.name}</strong>
    </div>
    <div class="background-fields">
      <label>Image path<div class="path-row"><input aria-label="Tile background image path" bind:value={path} disabled={busy||pending} placeholder="Choose a PNG, JPEG or WebP image"/><button class="secondary-button" disabled={busy||pending} onclick={browse}><Icon name="folder" size={14}/>Browse</button></div></label>
      <div class="image-actions"><p>Images are copied into Cockpit. The original file can be moved after saving.</p><button class="secondary-button" disabled={busy||pending||!path.trim()} onclick={()=>save({command:'game_background_save',gameId:game.id,path:path.trim(),positionX,positionY})}>Use image</button></div>
      <label>Horizontal position<input type="range" aria-label="Background horizontal position" min="0" max="100" bind:value={positionX} disabled={busy||pending||!(game.background||game.defaultBackground)}/></label>
      <label>Vertical position<input type="range" aria-label="Background vertical position" min="0" max="100" bind:value={positionY} disabled={busy||pending||!(game.background||game.defaultBackground)}/></label>
      <button class="secondary-button save-position" disabled={busy||pending||!dirty||!(game.background||game.defaultBackground)} onclick={()=>save({command:'game_background_save',gameId:game.id,positionX,positionY})}>{pending?'Saving…':'Save position'}</button>
    </div>
  </div>
  {#if error}<p class="background-error" role="alert">{error}</p>{/if}
</section>

<style>
  .background-settings{padding:20px;margin-bottom:20px;min-width:0}.background-heading{display:flex;align-items:center;justify-content:space-between;gap:12px;flex-wrap:wrap;margin-bottom:16px}h3{display:flex;align-items:center;gap:10px;font-size:14px;margin:0}.background-layout{display:grid;grid-template-columns:minmax(180px,320px) minmax(0,1fr);gap:20px}.background-preview{position:relative;aspect-ratio:16/9;overflow:hidden;border-radius:6px;background:linear-gradient(135deg,var(--accent-bg),var(--surface-raised));align-self:start}.background-preview img{width:100%;height:100%;object-fit:cover}.preview-shade{position:absolute;inset:0;background:linear-gradient(transparent,#000b)}.background-preview strong{position:absolute;bottom:18px;left:18px;right:18px;font-size:20px;color:white;overflow-wrap:anywhere}.background-fields{display:grid;gap:12px;min-width:0}label{display:grid;gap:7px;font-size:12px;color:var(--soft)}.path-row{display:flex;gap:8px;min-width:0}.path-row input{width:100%;min-width:0;padding:9px;border:1px solid var(--line);border-radius:5px;color:var(--text);background:var(--input-bg);font-size:12px}.path-row button{display:flex;align-items:center;gap:6px;flex-shrink:0}.image-actions{display:flex;gap:12px;align-items:center;justify-content:space-between}.image-actions p{font-size:11px;color:var(--muted);line-height:1.6;margin:0}.image-actions button{flex-shrink:0}button{font-size:12px}.save-position{justify-self:end}.background-error{font-size:12px;color:#e5b19b;margin:12px 0 0}@media(max-width:700px){.background-layout{grid-template-columns:1fr}.background-preview{width:100%;max-width:400px}}
</style>
