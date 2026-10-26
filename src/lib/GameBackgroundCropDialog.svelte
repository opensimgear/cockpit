<script lang="ts">
  import Icon from './Icon.svelte';
  import type {GameCrop} from './games';
  import {boundedCrop,maximumCrop,positionedCrop,moveCrop,resizeCropCorner,cropImageStyle,type CropCorner} from './game-background-crop';
  let {image,name,initialCrop,positionX=50,positionY=50,pending=false,error='',save,close}:{image:string;name:string;initialCrop?:GameCrop;positionX?:number;positionY?:number;pending?:boolean;error?:string;save:(crop:GameCrop)=>void;close:()=>void}=$props();
  let width=$state(1),height=$state(1),ready=$state(false),loadError=$state('');
  let crop=$state<GameCrop>({x:0,y:0,width:1,height:1});
  const corners:[CropCorner,string][]=[['nw','top left'],['ne','top right'],['sw','bottom left'],['se','bottom right']];
  let drag:{id:number;x:number;y:number;rect:DOMRect;crop:GameCrop;corner:CropCorner|null}|null=null;
  function loaded(event:Event){const image=event.currentTarget as HTMLImageElement;width=image.naturalWidth;height=image.naturalHeight;crop=initialCrop?boundedCrop(initialCrop,width,height):positionedCrop(width,height,positionX,positionY);ready=true;}
  function start(event:PointerEvent,corner:CropCorner|null=null){if(pending||!ready||event.button!==0)return;event.preventDefault();const target=event.currentTarget as HTMLButtonElement;drag={id:event.pointerId,x:event.clientX,y:event.clientY,rect:target.closest('.crop-surface')!.getBoundingClientRect(),crop:{...crop},corner};target.setPointerCapture(event.pointerId);}
  function move(event:PointerEvent){if(!drag||drag.id!==event.pointerId||pending)return;const dx=(event.clientX-drag.x)/drag.rect.width,dy=(event.clientY-drag.y)/drag.rect.height;crop=drag.corner?resizeCropCorner(drag.crop,dx,dy,width,height,drag.corner):moveCrop(drag.crop,dx,dy,width,height);}
  function end(){drag=null;}
  function keyboard(event:KeyboardEvent,corner:CropCorner|null=null){const delta=event.shiftKey?.05:.01,dx=event.key==='ArrowRight'?delta:event.key==='ArrowLeft'?-delta:0,dy=event.key==='ArrowDown'?delta:event.key==='ArrowUp'?-delta:0;if(!dx&&!dy||pending)return;event.preventDefault();crop=corner?resizeCropCorner(crop,dx,dy,width,height,corner):moveCrop(crop,dx,dy,width,height);}
</script>

<section class="crop-editor" aria-labelledby="background-crop-title">
  <h2 id="background-crop-title">Position tile background</h2>
  <p id="crop-help">Drag the tile frame to move it. Drag its corners to scale. The frame keeps the tile’s 16:9 shape and stays inside the full image. Arrow keys move; Shift moves faster.</p>
  <div class="crop-workspace">
    <div class="crop-surface" style:width={`min(100%, 960px, ${45*width/height}vh)`} style:aspect-ratio={`${width}/${height}`}>
      <img class="full-image" src={image} alt="Complete background" onload={loaded} onerror={()=>loadError='The image preview could not be loaded. Choose the image again.'}/>
      {#if ready}<div class="crop-frame" style:left={`${100*crop.x}%`} style:top={`${100*crop.y}%`} style:width={`${100*crop.width}%`} style:height={`${100*crop.height}%`}>
        <button type="button" class="crop-move" aria-label="Move tile crop" aria-describedby="crop-help" disabled={pending} onpointerdown={event=>start(event)} onpointermove={move} onpointerup={end} onpointercancel={end} onlostpointercapture={end} onkeydown={keyboard}><span>Tile</span></button>
        {#each corners as [corner,label]}<button type="button" class={`crop-resize ${corner}`} aria-label={`Resize tile crop ${label}`} aria-describedby="crop-help" disabled={pending} onpointerdown={event=>start(event,corner)} onpointermove={move} onpointerup={end} onpointercancel={end} onlostpointercapture={end} onkeydown={event=>keyboard(event,corner)}></button>{/each}
      </div>{/if}
    </div>
  </div>
  <div class="crop-controls"><button type="button" class="secondary-button" disabled={!ready||pending} onclick={()=>crop=maximumCrop(width,height)}><Icon name="fit" size={14}/>Fit image</button></div>
  <div class="crop-footer"><div class="crop-preview" aria-label="Cropped tile preview"><img src={image} alt="" style={cropImageStyle(crop)}/><strong>{name}</strong></div><div class="actions"><button class="secondary-button" disabled={pending} onclick={close}><Icon name="close" size={14}/>Cancel</button><button class="primary-button" disabled={!ready||pending||!!loadError} onclick={()=>save(crop)}><Icon name="save" size={14}/>{pending?'Saving…':'Save background'}</button></div></div>
  {#if error||loadError}<p class="crop-error" role="alert">{error||loadError}</p>{/if}
</section>

<style>
  .crop-editor{margin-top:20px;border-top:1px solid var(--line);padding-top:20px}h2{font-size:18px;margin:0}p{font-size:12px;color:var(--muted);line-height:1.6;margin:12px 0 18px}.crop-workspace{display:grid;place-items:center;min-height:120px;padding:16px;background:var(--bg);border-radius:6px;overflow:hidden}.crop-surface{position:relative;overflow:hidden;line-height:0}.full-image{display:block;width:100%;height:100%;object-fit:contain;user-select:none;pointer-events:none}.crop-frame{position:absolute;box-sizing:border-box;border:2px solid var(--accent);box-shadow:0 0 0 2000px #0007}.crop-move{position:absolute;inset:0;width:100%;height:100%;background:transparent;cursor:move;touch-action:none;padding:0}.crop-move span{position:absolute;top:8px;left:8px;padding:5px 7px;border-radius:3px;background:#000a;color:white;font-size:10px;line-height:1}.crop-resize{position:absolute;width:14px;height:14px;background:var(--accent);border:2px solid white;cursor:nwse-resize;touch-action:none}.crop-resize.nw{left:-1px;top:-1px}.crop-resize.ne{right:-1px;top:-1px;cursor:nesw-resize}.crop-resize.sw{left:-1px;bottom:-1px;cursor:nesw-resize}.crop-resize.se{right:-1px;bottom:-1px}.crop-move:focus-visible,.crop-resize:focus-visible{outline:2px solid white;outline-offset:-4px}.crop-controls{display:flex;gap:16px;align-items:center;margin-top:20px}.crop-footer{display:flex;align-items:flex-end;justify-content:space-between;gap:20px;margin-top:18px}.crop-preview{position:relative;width:220px;aspect-ratio:16/9;overflow:hidden;border-radius:5px}.crop-preview img{display:block}.crop-preview strong{position:absolute;inset:auto 0 0;padding:18px 12px 12px;background:linear-gradient(transparent,#000b);font-size:13px;color:white;overflow-wrap:anywhere}.actions{display:flex;gap:10px;flex-wrap:wrap;justify-content:flex-end}.crop-error{color:#e5b19b;margin-bottom:0}@media(max-width:560px){.crop-workspace{padding:8px}.crop-footer{align-items:stretch;flex-direction:column}.crop-preview{width:160px}}
</style>
