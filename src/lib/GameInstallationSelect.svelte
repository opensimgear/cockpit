<script lang="ts">
  import {tick} from 'svelte';
  import Icon from './Icon.svelte';
  import {gameDisplayPath,gameLibraryClientName,type GameInstallation} from './games';

  let {installations,value='',disabled=false,onselect}:{installations:GameInstallation[];value?:string;disabled?:boolean;onselect:(id:string)=>void}=$props();
  const uid=$props.id();
  const selected=$derived(installations.find(installation=>installation.id===value));
  let trigger=$state<HTMLButtonElement>(),menu=$state<HTMLDivElement>();
  let open=$state(false),activeIndex=$state(0);
  let position=$state({left:0,top:0,bottom:0,width:0,height:280,above:false});
  $effect(()=>{if(disabled&&open)close();});

  function close(){menu?.hidePopover();open=false;}
  function focusOption(index:number){
    activeIndex=index;
    void tick().then(()=>{if(open)menu?.querySelector<HTMLElement>(`[data-index="${index}"]`)?.scrollIntoView({block:'nearest'});});
  }
  function beforeToggle(event:Event){
    open=(event as ToggleEvent).newState==='open';
    if(!open||!trigger)return;
    const rect=trigger.getBoundingClientRect(),below=window.innerHeight-rect.bottom-12,above=rect.top-12;
    const aboveMenu=below<Math.min(180,installations.length*44+12)&&above>below;
    const width=Math.min(rect.width,window.innerWidth-24);
    position={left:Math.max(12,Math.min(rect.left,window.innerWidth-width-12)),top:rect.bottom+6,bottom:window.innerHeight-rect.top+6,width,height:Math.max(44,Math.min(280,aboveMenu?above:below)),above:aboveMenu};
    focusOption(Math.max(0,installations.findIndex(installation=>installation.id===value)));
  }
  function choose(index:number){
    const installation=installations[index];
    if(disabled||!installation)return;
    close();trigger?.focus();
    if(installation.id!==value)onselect(installation.id);
  }
  function keydown(event:KeyboardEvent){
    if(disabled)return;
    if(event.key==='ArrowDown'||event.key==='ArrowUp'){
      event.preventDefault();
      if(!open){menu?.showPopover();if(event.key==='ArrowUp'&&!selected)focusOption(installations.length-1);}
      else if(installations.length)focusOption((activeIndex+(event.key==='ArrowDown'?1:-1)+installations.length)%installations.length);
    }else if(open&&(event.key==='Home'||event.key==='End')){
      event.preventDefault();focusOption(event.key==='Home'?0:installations.length-1);
    }else if(open&&(event.key==='Enter'||event.key===' ')){
      event.preventDefault();choose(activeIndex);
    }else if(open&&event.key==='Escape'){
      event.preventDefault();event.stopPropagation();close();
    }else if(open&&event.key==='Tab')close();
  }
</script>

<svelte:window onresize={()=>{if(open)close();}}/>
<svelte:document onscrollcapture={event=>{if(open&&event.target instanceof Node&&!menu?.contains(event.target))close();}}/>
<div class="installation-select">
  <button class="installation-trigger" type="button" role="combobox" aria-label="Detected installations" aria-haspopup="listbox" aria-expanded={open} aria-controls={`${uid}-listbox`} aria-activedescendant={open?`${uid}-option-${activeIndex}`:undefined} data-value={value} {disabled} bind:this={trigger} popovertarget={`${uid}-listbox`} onkeydown={keydown} title={selected?gameDisplayPath(selected.installPath):undefined}>
    <span class="selected-path" class:placeholder={!selected}>{selected?gameDisplayPath(selected.installPath):'Choose an installation'}</span><span class="chevron" class:open aria-hidden="true"><Icon name="chevron" size={14}/></span>
  </button>
  <div class="installation-menu" id={`${uid}-listbox`} role="listbox" aria-label="Detected installations" bind:this={menu} popover="auto" onbeforetoggle={beforeToggle} style:left={`${position.left}px`} style:top={position.above?'auto':`${position.top}px`} style:bottom={position.above?`${position.bottom}px`:'auto'} style:width={`${position.width}px`} style:max-height={`${position.height}px`}>
    {#each installations as installation,index (installation.id)}
      <button class="installation-option" class:active={activeIndex===index} id={`${uid}-option-${index}`} type="button" role="option" aria-selected={installation.id===value} aria-label={`${gameDisplayPath(installation.installPath)} · ${gameLibraryClientName(installation.source)}`} tabindex="-1" data-index={index} title={gameDisplayPath(installation.installPath)} onpointerdown={event=>event.preventDefault()} onpointermove={()=>activeIndex=index} onclick={()=>choose(index)} {disabled}>
        <span class="option-platform" aria-hidden="true"><Icon name={installation.source==='local'?'disk':installation.source} size={18}/></span><span class="option-path">{gameDisplayPath(installation.installPath)}</span><span class="selection-mark" aria-hidden="true">{#if installation.id===value}<Icon name="check" size={16}/>{/if}</span>
      </button>
    {/each}
  </div>
</div>

<style>
  .installation-select{flex:1;min-width:0}
  .installation-trigger{display:flex;align-items:center;justify-content:space-between;gap:12px;box-sizing:border-box;width:100%;min-width:0;min-height:40px;padding:10px 12px;border:1px solid var(--line);border-radius:5px;background:var(--bg);color:var(--soft);font:inherit;font-size:12px;line-height:1.6;text-align:left;cursor:pointer}
  .installation-trigger:hover:enabled{border-color:var(--muted)}
  .installation-trigger:focus-visible{outline:2px solid var(--accent);outline-offset:2px}
  .installation-trigger:disabled{opacity:.5;cursor:default}
  .selected-path,.option-path{overflow:hidden;text-overflow:ellipsis;white-space:nowrap;min-width:0}
  .placeholder{color:var(--muted)}
  .chevron{display:flex;flex-shrink:0;color:var(--muted);transform:rotate(90deg)}
  .chevron.open{transform:rotate(-90deg)}
  .installation-menu{position:fixed;box-sizing:border-box;margin:0;padding:6px;border:1px solid var(--line);border-radius:8px;background:var(--surface);color:var(--text);box-shadow:0 8px 24px #0005;overflow-y:auto;overscroll-behavior:contain}
  .installation-option{display:flex;align-items:center;gap:10px;box-sizing:border-box;width:100%;min-width:0;min-height:44px;padding:7px 10px;border:0;border-radius:5px;background:transparent;color:var(--soft);font:inherit;font-size:12px;line-height:1.6;text-align:left;cursor:pointer}
  .installation-option.active,.installation-option:hover{background:var(--surface-raised);color:var(--text)}
  .installation-option[aria-selected=true]{background:var(--accent-bg);color:var(--text)}
  .option-platform{display:flex;align-items:center;justify-content:center;flex-shrink:0;width:24px;height:24px;color:var(--soft)}
  .option-path{flex:1}
  .selection-mark{display:flex;flex-shrink:0;width:16px;color:var(--accent)}
</style>
