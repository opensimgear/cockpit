<script lang="ts">
  import Icon from './Icon.svelte';
  import GameArtworkPicker from './GameArtworkPicker.svelte';
  import {pickGameImage} from './gateway';
  import {resolveGameArtwork} from './game-artwork';
  import {cropImageStyle} from './game-background-crop';
  import GameBackgroundCropDialog from './GameBackgroundCropDialog.svelte';
  import type {Game,GameCrop,GameRequest,GamesState} from './games';
  let {game,busy=false,request}:{game:Game;busy?:boolean;request:(value:GameRequest)=>Promise<GamesState|false>}=$props();
  let formOpen=$state(false),libraryPending=$state(false);
  let path=$state(''),url=$state(''),source=$state<'library'|'disk'|'url'>('library'),pending=$state(false),error=$state(''),editor=$state<{image:string;crop?:GameCrop;positionX?:number;positionY?:number}|null>(null);
  const art=$derived(resolveGameArtwork(game));
  function show(node:HTMLDialogElement){node.showModal();return()=>node.close();}
  async function save(value:GameRequest){
    if(pending||busy||libraryPending)return;pending=true;error='';
    try{const result=await request(value);if(!result||result.error)throw new Error(result&&result.error||'The tile background was not saved.');path='';url='';editor=null;formOpen=false;}
    catch(cause){error=cause instanceof Error?cause.message:String(cause);}finally{pending=false;}
  }
  async function load(){
    if(pending||busy)return;pending=true;error='';
    try{const result=await request({command:'game_background_preview',...(source==='disk'?{path:path.trim()}:{url:url.trim()})});if(!result||result.error||!result.backgroundPreview?.image)throw new Error(result&&result.error||'The image could not be loaded.');editor={image:result.backgroundPreview.image};}
    catch(cause){error=cause instanceof Error?cause.message:String(cause);}finally{pending=false;}
  }
  async function browse(){
    if(pending||busy)return;pending=true;error='';
    try{const chosen=await pickGameImage(path);if(chosen)path=chosen;}
    catch(cause){error=cause instanceof Error?cause.message:String(cause);}finally{pending=false;}
  }
  async function selectArtwork(sourceImage:string,crop?:GameCrop){
    if(pending||busy||!sourceImage)return;error='';
    const positionX=sourceImage===art.image?art.positionX:50,positionY=sourceImage===art.image?art.positionY:50;
    if(sourceImage.startsWith('data:image/jpeg;base64,')){editor={image:sourceImage,crop,positionX,positionY};return;}
    // Normalize other raster sources to the bounded preview format.
    pending=true;
    try{
      const image=new Image();image.src=sourceImage;await image.decode();let size=640,data='';
      do{const ratio=Math.min(1,size/Math.max(image.naturalWidth,image.naturalHeight)),canvas=document.createElement('canvas');canvas.width=Math.max(1,Math.round(image.naturalWidth*ratio));canvas.height=Math.max(1,Math.round(image.naturalHeight*ratio));const context=canvas.getContext('2d');if(!context)throw new Error('Image previews are unavailable.');context.drawImage(image,0,0,canvas.width,canvas.height);data=canvas.toDataURL('image/jpeg',.68);size=Math.floor(size*.75);}while(data.length>16407&&size>=48);
      if(data.length>16407)throw new Error('This image cannot fit its preview budget.');editor={image:data,crop,positionX,positionY};
    }catch(cause){error=cause instanceof Error?cause.message:String(cause);}finally{pending=false;}
  }
  function position(){return selectArtwork(art.image,art.crop);}
</script>
<section class="background-settings" aria-label="Tile background">
  <div class="background-preview" aria-label="Game tile preview">{#if art.image}{#key art.image}<img src={art.image} alt="" style={cropImageStyle(art.crop)} style:object-position={`${art.positionX}% ${art.positionY}%`}/>{/key}{/if}<div class="preview-shade"></div><strong>{game.name}</strong></div>
  <button class="secondary-button update-image" disabled={busy||pending||libraryPending} onclick={()=>{formOpen=true;source='library';error='';void position();}}><Icon name="image" size={14}/>Update image</button>
</section>
{#if formOpen}
  <dialog class="image-form" aria-labelledby="update-background-title" oncancel={event=>{event.preventDefault();if(!pending&&!libraryPending){formOpen=false;editor=null;error='';}}} {@attach show}>
    <h2 id="update-background-title">Update tile background</h2><p>Choose artwork from your library, disk or a URL, then align the tile frame.</p>
    <div class="background-fields">
      <div class="source-options" aria-label="Background image source"><button class:active={source==='library'} class="secondary-button" aria-pressed={source==='library'} disabled={pending||busy||libraryPending} onclick={()=>source='library'}><Icon name="image" size={14}/>From library</button><button class:active={source==='disk'} class="secondary-button" aria-pressed={source==='disk'} disabled={pending||busy||libraryPending} onclick={()=>source='disk'}><Icon name="folder" size={14}/>From disk</button><button class:active={source==='url'} class="secondary-button" aria-pressed={source==='url'} disabled={pending||busy||libraryPending} onclick={()=>source='url'}><Icon name="link" size={14}/>From URL</button><button class="secondary-button" disabled={busy||pending||libraryPending||!game.background} onclick={()=>save({command:'game_background_save',gameId:game.id,path:null})}><Icon name="refresh" size={14}/>Reset to default</button></div>
      {#if source==='library'}<GameArtworkPicker {game} current={art.image} currentCrop={art.crop} selected={editor?.image} disabled={pending||busy} selecting={value=>libraryPending=value} choose={selectArtwork}/>{:else if source==='disk'}<label>Image path<div class="path-row"><input aria-label="Tile background image path" bind:value={path} disabled={busy||pending||libraryPending} placeholder="Choose a PNG, JPEG or WebP image"/><button class="secondary-button" disabled={busy||pending||libraryPending} onclick={browse}><Icon name="folder" size={14}/>Browse</button></div></label>{:else}<label>Image URL<input aria-label="Tile background image URL" type="url" bind:value={url} disabled={busy||pending||libraryPending} placeholder="https://example.com/background.jpg"/></label>{/if}
      <p>Images are copied into Cockpit when you save. Moving the file or changes to the URL will not affect the saved background.</p>
      {#if error&&!editor}<p class="background-error" role="alert">{error}</p>{/if}
      <div class="image-actions">{#if !editor}<button class="secondary-button" disabled={pending||libraryPending} onclick={()=>{formOpen=false;error='';}}><Icon name="close" size={14}/>Cancel</button>{/if}{#if source!=='library'}<button class="primary-button" disabled={busy||pending||!(source==='disk'?path.trim():url.trim())} onclick={load}><Icon name="image" size={14}/>{pending?'Loading…':'Use image'}</button>{/if}</div>
    </div>
    {#if editor}{#key editor}<GameBackgroundCropDialog name={game.name} image={editor.image} initialCrop={editor.crop} positionX={editor.positionX??art.positionX} positionY={editor.positionY??art.positionY} pending={pending||libraryPending} {error} close={()=>{editor=null;formOpen=false;error='';}} save={crop=>void save({command:'game_background_save',gameId:game.id,imageData:editor!.image,crop})}/>{/key}{/if}
  </dialog>
{/if}


<style>
  .background-settings{min-width:0;display:grid;gap:12px;align-self:start}.background-preview{position:relative;aspect-ratio:16/9;overflow:hidden;border-radius:8px;background:linear-gradient(135deg,var(--accent-bg),var(--surface-raised));border:1px solid var(--line)}.background-preview img{width:100%;height:100%;object-fit:cover}.preview-shade{position:absolute;inset:0;background:linear-gradient(transparent,#000b)}.background-preview strong{position:absolute;bottom:18px;left:18px;right:18px;font-size:22px;color:white;overflow-wrap:anywhere}.update-image{width:100%}.image-form{position:fixed;inset:0;margin:auto;width:min(1000px,calc(100% - 32px));max-height:calc(100% - 32px);overflow:auto;box-sizing:border-box;padding:24px;background:var(--surface);color:var(--text);border:1px solid var(--line);border-radius:10px;box-shadow:0 20px 80px #0005}.image-form::backdrop{background:#000b}h2{font-size:18px;margin:0}.background-fields{display:grid;gap:18px;min-width:0}label{display:grid;gap:7px;font-size:12px;color:var(--soft)}input{width:100%;min-width:0;box-sizing:border-box;padding:10px;border:1px solid var(--line);border-radius:5px;color:var(--text);background:var(--input-bg);font-size:12px}.path-row{display:flex;gap:8px;min-width:0}.path-row input{flex:1}.path-row button{flex-shrink:0}.source-options,.image-actions{display:flex;gap:10px;flex-wrap:wrap}.source-options .active{border-color:var(--accent);color:var(--accent)}.image-actions{justify-content:flex-end;margin-top:8px}p{font-size:12px;color:var(--muted);line-height:1.6;margin:14px 0 20px}.background-fields p{margin:0}button{font-size:12px}.background-error{color:#e5b19b}
</style>