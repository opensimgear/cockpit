<script lang="ts">
  import Icon from './Icon.svelte';
  import {pickGameExecutable,pickGameImage,pickGameIntegrationFolder} from './gateway';
  import {gameAddError,gameAddRequest,gameDisplayPath,gameExecutableDisplayPath,type GameRequest,type GamesState} from './games';
  let {busy=false,request,close,added}:{busy?:boolean;request:(value:GameRequest)=>Promise<GamesState|false>;close:()=>void;added:(value:GamesState)=>void}=$props();
  let root=$state(''),executable=$state(''),name=$state(''),imagePath=$state(''),image=$state(''),pending=$state(false),error=$state('');
  const invalid=$derived(gameAddError(name,root,executable));
  function show(node:HTMLDialogElement){node.showModal();return()=>node.close();}
  const folderName=(value:string)=>value.replace(/[\\/]+$/,'').split(/[\\/]/).pop()??'';
  function setRoot(value:string){if(root===value)return;if(!name||name===folderName(root))name=folderName(value);root=value;executable='';error='';}
  async function browse(kind:'root'|'executable'|'image'){
    if(busy||pending)return;pending=true;error='';
    try{
      if(kind==='root'){const value=await pickGameIntegrationFolder('game',root);if(value)setRoot(value);}
      else if(kind==='executable'){const value=await pickGameExecutable('',executable||root);if(value)executable=value;}
      else{const value=await pickGameImage(imagePath);if(value){const result=await request({command:'game_background_preview',path:value});if(!result||result.error||!result.backgroundPreview?.image)throw new Error(result&&result.error||'The image could not be loaded.');imagePath=value;image=result.backgroundPreview.image;}}
    }catch(cause){error=cause instanceof Error?cause.message:String(cause);}finally{pending=false;}
  }
  async function add(){
    if(busy||pending)return;const validation=gameAddError(name,root,executable);if(validation){error=validation;return;}pending=true;error='';
    try{const result=await request(gameAddRequest(name,root,executable,imagePath));if(!result||result.error||!result.addedGameId)throw new Error(result&&result.error||'The game was not added. Try again.');added(result);}
    catch(cause){error=cause instanceof Error?cause.message:String(cause);}finally{pending=false;}
  }
</script>

<dialog class="add-dialog" aria-labelledby="add-game-title" oncancel={event=>{event.preventDefault();if(!pending&&!busy)close();}} {@attach show}>
  <h2 id="add-game-title">Add game</h2><p>Choose a game folder and its executable, then give it a name and image.</p>
  <form onsubmit={event=>{event.preventDefault();void add();}}>
    <label>Root folder<div class="path-row"><input aria-label="Game root folder" value={gameDisplayPath(root)} oninput={event=>setRoot(event.currentTarget.value)} disabled={busy||pending} placeholder="Choose the game folder" required/><button type="button" class="secondary-button" aria-label="Choose game folder" disabled={busy||pending} onclick={()=>browse('root')}><Icon name="folder" size={14}/>Browse</button></div></label>
    <label>Executable<div class="path-row"><input aria-label="New game executable" value={gameExecutableDisplayPath(executable,root)} oninput={event=>executable=event.currentTarget.value} disabled={busy||pending||!root} placeholder="Choose its executable" required/><button type="button" class="secondary-button" aria-label="Choose new game executable" disabled={busy||pending||!root} onclick={()=>browse('executable')}><Icon name="folder" size={14}/>Browse</button></div></label>
    <label>Name<input aria-label="New game name" bind:value={name} maxlength="80" disabled={busy||pending} required/></label>
    <div class="image-row"><div class="image-preview" aria-label="New game image preview">{#if image}<img src={image} alt="Selected game background"/>{:else}<Icon name="gamepad" size={32}/>{/if}</div><div><button type="button" class="secondary-button" disabled={busy||pending} onclick={()=>browse('image')}><Icon name="image" size={14}/>Upload image</button><p>Optional. You can adjust it after adding the game.</p>{#if imagePath}<button type="button" class="text-button" disabled={busy||pending} onclick={()=>{imagePath='';image='';}}><Icon name="trash" size={14}/>Remove image</button>{/if}</div></div>
    {#if error||executable&&invalid}<p class="error" role="alert">{error||invalid}</p>{/if}
    <div class="actions"><button type="button" class="secondary-button" disabled={busy||pending} onclick={close}><Icon name="close" size={14}/>Cancel</button><button class="primary-button" disabled={busy||pending||!!invalid}><Icon name="plus" size={14}/>{pending?'Adding…':'Add'}</button></div>
  </form>
</dialog>

<style>
  .add-dialog{position:fixed;inset:0;margin:auto;width:min(620px,calc(100% - 32px));max-height:calc(100% - 32px);overflow:auto;box-sizing:border-box;padding:24px;background:var(--surface);color:var(--text);border:1px solid var(--line);border-radius:10px;box-shadow:0 20px 80px #0005}.add-dialog::backdrop{background:#000b}h2{font-size:18px;margin:0}p{font-size:12px;color:var(--muted);line-height:1.6;margin:12px 0 20px}form{display:grid;gap:18px}label{display:grid;gap:7px;font-size:12px;color:var(--soft)}input{width:100%;min-width:0;box-sizing:border-box;padding:10px;border:1px solid var(--line);border-radius:5px;color:var(--text);background:var(--input-bg);font-size:12px}.path-row{display:flex;gap:8px;min-width:0}.path-row input{flex:1}.path-row button{flex-shrink:0}.image-row{display:flex;gap:16px;align-items:center}.image-preview{position:relative;display:grid;place-items:center;width:160px;aspect-ratio:16/9;flex-shrink:0;overflow:hidden;border:1px solid var(--line);border-radius:6px;background:var(--surface-raised);color:var(--muted)}.image-preview img{width:100%;height:100%;object-fit:cover}.image-row p{margin:8px 0}.actions{display:flex;justify-content:flex-end;gap:10px}.error{margin:0;color:#e5b19b}@media(max-width:500px){.image-row{align-items:start;flex-direction:column}}
</style>
