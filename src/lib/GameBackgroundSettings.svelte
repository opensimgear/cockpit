<script lang="ts">
  import Icon from './Icon.svelte';
  import {pickGameImage} from './gateway';
  import {resolveGameArtwork} from './game-artwork';
  import {cropImageStyle} from './game-background-crop';
  import GameBackgroundCropDialog from './GameBackgroundCropDialog.svelte';
  import type {Game,GameCrop,GameRequest,GamesState} from './games';
  let {game,busy=false,request}:{game:Game;busy?:boolean;request:(value:GameRequest)=>Promise<GamesState|false>}=$props();
  let path=$state(''),url=$state(''),source=$state<'disk'|'url'>('disk'),pending=$state(false),error=$state(''),editor=$state<{image:string;crop?:GameCrop}|null>(null);
  const art=$derived(resolveGameArtwork(game));
  async function save(value:GameRequest){
    if(pending||busy)return;pending=true;error='';
    try{const result=await request(value);if(!result||result.error)throw new Error(result&&result.error||'The tile background was not saved.');path='';url='';editor=null;}
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
  async function position(){
    if(pending||busy||!art.image)return;error='';
    if(art.image.startsWith('data:image/jpeg;base64,')){editor={image:art.image,crop:art.crop};return;}
    // Bundled assets work in packaged builds without native filesystem URLs.
    pending=true;
    try{
      const image=new Image();image.src=art.image;await image.decode();let size=640,data='';
      do{const ratio=Math.min(1,size/Math.max(image.naturalWidth,image.naturalHeight)),canvas=document.createElement('canvas');canvas.width=Math.max(1,Math.round(image.naturalWidth*ratio));canvas.height=Math.max(1,Math.round(image.naturalHeight*ratio));const context=canvas.getContext('2d');if(!context)throw new Error('Image previews are unavailable.');context.drawImage(image,0,0,canvas.width,canvas.height);data=canvas.toDataURL('image/jpeg',.68);size=Math.floor(size*.75);}while(data.length>16407&&size>=48);
      if(data.length>16407)throw new Error('This image cannot fit its preview budget.');editor={image:data,crop:art.crop};
    }catch(cause){error=cause instanceof Error?cause.message:String(cause);}finally{pending=false;}
  }
</script>

<section class="panel background-settings" aria-label="Tile background">
  <div class="background-heading"><h3><Icon name="image" size={18}/>Tile background</h3><button class="secondary-button" disabled={busy||pending||!game.background} onclick={()=>save({command:'game_background_save',gameId:game.id,path:null})}>Reset to default</button></div>
  <div class="background-layout">
    <div class="background-preview" aria-label="Tile background preview">{#if art.image}{#key art.image}<img src={art.image} alt="" style={cropImageStyle(art.crop)} style:object-position={`${art.positionX}% ${art.positionY}%`}/>{/key}{/if}<div class="preview-shade"></div><strong>{game.name}</strong></div>
    <div class="background-fields">
      <div class="source-options" aria-label="Background image source"><button class:active={source==='disk'} class="secondary-button" aria-pressed={source==='disk'} disabled={pending||busy} onclick={()=>source='disk'}><Icon name="folder" size={14}/>From disk</button><button class:active={source==='url'} class="secondary-button" aria-pressed={source==='url'} disabled={pending||busy} onclick={()=>source='url'}><Icon name="link" size={14}/>From URL</button></div>
      {#if source==='disk'}<label>Image path<div class="path-row"><input aria-label="Tile background image path" bind:value={path} disabled={busy||pending} placeholder="Choose a PNG, JPEG or WebP image"/><button class="secondary-button" disabled={busy||pending} onclick={browse}><Icon name="folder" size={14}/>Browse</button></div></label>{:else}<label>Image URL<input aria-label="Tile background image URL" type="url" bind:value={url} disabled={busy||pending} placeholder="https://example.com/background.jpg"/></label>{/if}
      <p>Images are copied into Cockpit when you save. Moving the file or changes to the URL will not affect the saved background.</p>
      <div class="image-actions"><button class="secondary-button" disabled={busy||pending||!art.image} onclick={position}><Icon name="sliders" size={14}/>Position image</button><button class="secondary-button" disabled={busy||pending||!(source==='disk'?path.trim():url.trim())} onclick={load}>{pending?'Loading…':'Use image'}</button></div>
    </div>
  </div>
  {#if error&&!editor}<p class="background-error" role="alert">{error}</p>{/if}
</section>
{#if editor}<GameBackgroundCropDialog image={editor.image} name={game.name} initialCrop={editor.crop} positionX={art.positionX} positionY={art.positionY} {pending} {error} close={()=>{editor=null;error='';}} save={crop=>void save({command:'game_background_save',gameId:game.id,imageData:editor!.image,crop})}/>{/if}

<style>
  .background-settings{padding:20px;margin-bottom:20px;min-width:0}.background-heading{display:flex;align-items:center;justify-content:space-between;gap:12px;flex-wrap:wrap;margin-bottom:16px}h3{display:flex;align-items:center;gap:10px;font-size:14px;margin:0}.background-layout{display:grid;grid-template-columns:minmax(180px,320px) minmax(0,1fr);gap:20px}.background-preview{position:relative;aspect-ratio:16/9;overflow:hidden;border-radius:6px;background:linear-gradient(135deg,var(--accent-bg),var(--surface-raised));align-self:start}.background-preview img{width:100%;height:100%;object-fit:cover}.preview-shade{position:absolute;inset:0;background:linear-gradient(transparent,#000b)}.background-preview strong{position:absolute;bottom:18px;left:18px;right:18px;font-size:20px;color:white;overflow-wrap:anywhere}.background-fields{display:grid;gap:12px;min-width:0}label{display:grid;gap:7px;font-size:12px;color:var(--soft)}input:not([type=range]){width:100%;min-width:0;box-sizing:border-box;padding:9px;border:1px solid var(--line);border-radius:5px;color:var(--text);background:var(--input-bg);font-size:12px}.path-row{display:flex;gap:8px;min-width:0}.path-row input{flex:1}.path-row button{flex-shrink:0}.source-options,.image-actions{display:flex;gap:10px;flex-wrap:wrap}.source-options .active{border-color:var(--accent);color:var(--accent)}.image-actions{justify-content:flex-end}.background-fields p{font-size:11px;color:var(--muted);line-height:1.6;margin:0}button{font-size:12px}.background-error{font-size:12px;color:#e5b19b;margin:12px 0 0}@media(max-width:700px){.background-layout{grid-template-columns:1fr}.background-preview{width:100%;max-width:400px}}
</style>
