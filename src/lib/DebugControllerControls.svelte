<script lang="ts">
  import Axis from './input-visuals/Axis.svelte';
  import Hat from './input-visuals/Hat.svelte';
  import {onDestroy,onMount} from 'svelte';
  import Icon from './Icon.svelte';
  import type {DebugControllerChange,DebugControllerLayout} from './profile-debug-controller';
  let {layout,active,disabled=false,values={},change,onInteract=()=>{}}:{layout:DebugControllerLayout;active:boolean;disabled?:boolean;values?:Readonly<Record<string,number|boolean>>;change:(changes:DebugControllerChange[])=>void;onInteract?:(value:boolean)=>void}=$props();
  const held=new Set<string>(),keys=new Map<string,Set<string>>(),pointers=new Map<number,string>(),drags=new Set<number>();
  const range=(axis:DebugControllerLayout['axes'][number])=>({min:axis.range==='bipolar'?-32768:0,max:32767});
  const clamp=(value:number,min:number,max:number)=>Math.max(min,Math.min(max,Math.round(value)));
  const axisValue=(axis:DebugControllerLayout['axes'][number])=>{const limits=range(axis),value=values[axis.portId];return typeof value==='number'&&Number.isFinite(value)?clamp(value,limits.min,limits.max):0;};
  function interaction(){onInteract(!!(held.size||drags.size));}
  function press(id:string){if(!active||disabled||held.has(id))return;held.add(id);interaction();change([{portId:id,value:true}]);}
  function release(id:string){if(keys.has(id)||[...pointers.values()].includes(id)||!held.delete(id))return;change([{portId:id,value:false}]);interaction();}
  function pointerEnd(event:PointerEvent){const id=pointers.get(event.pointerId);pointers.delete(event.pointerId);drags.delete(event.pointerId);if(id)release(id);interaction();}
  function releaseAll(){keys.clear();pointers.clear();drags.clear();for(const id of [...held])release(id);interaction();}
  function buttonDown(event:PointerEvent,id:string){event.preventDefault();event.stopPropagation();if(!active||disabled||event.button!==0)return;pointers.set(event.pointerId,id);event.currentTarget instanceof HTMLElement&&event.currentTarget.setPointerCapture(event.pointerId);press(id);}
  function keyDown(event:KeyboardEvent,id:string){if(event.key===' '||event.key==='Enter'){event.preventDefault();event.stopPropagation();if(!active||disabled)return;const pressed=keys.get(id)??new Set<string>();pressed.add(event.key);keys.set(id,pressed);press(id);}}
  function keyUp(event:KeyboardEvent,id:string){if(event.key===' '||event.key==='Enter'){event.preventDefault();event.stopPropagation();const pressed=keys.get(id);pressed?.delete(event.key);if(!pressed?.size)keys.delete(id);release(id);}}
  function axisInput(axis:DebugControllerLayout['axes'][number],value:number){if(!active||disabled)return;const limits=range(axis);if(Number.isFinite(value))change([{portId:axis.portId,value:clamp(value,limits.min,limits.max)}]);}
  function dragStart(event:PointerEvent){event.stopPropagation();if(active&&!disabled){drags.add(event.pointerId);interaction();}}
  function hatValue(id:string,value:number){if(!active||disabled)return;if(Number.isInteger(value)&&value>=-1&&value<=7)change([{portId:id,value}]);}
  function resetAxes(){if(active&&!disabled&&layout.axes.length)change(layout.axes.map(axis=>({portId:axis.portId,value:0})));}
  function padPosition(index:number){const axis=layout.axes[index];if(!axis)return 0;const limits=range(axis);return 100*(axisValue(axis)-limits.min)/(limits.max-limits.min);}
  function padMove(event:PointerEvent){
    event.stopPropagation();if(!active||disabled||!drags.has(event.pointerId)||layout.axes.length<2)return;
    const bounds=(event.currentTarget as HTMLElement).getBoundingClientRect();if(!bounds.width||!bounds.height)return;
    const fractions=[(event.clientX-bounds.left)/bounds.width,(event.clientY-bounds.top)/bounds.height];
    change(layout.axes.slice(0,2).map((axis,index)=>{const limits=range(axis);return{portId:axis.portId,value:clamp(limits.min+fractions[index]*(limits.max-limits.min),limits.min,limits.max)};}));
  }
  function padDown(event:PointerEvent){event.preventDefault();event.stopPropagation();if(!active||disabled||event.button!==0)return;(event.currentTarget as HTMLElement).setPointerCapture(event.pointerId);dragStart(event);padMove(event);}
  function padKey(event:KeyboardEvent){
    if(!['ArrowLeft','ArrowRight','ArrowUp','ArrowDown','Home'].includes(event.key))return;event.preventDefault();event.stopPropagation();if(!active||disabled)return;
    change(layout.axes.slice(0,2).map((axis,index)=>{const limits=range(axis),direction=index===0?(event.key==='ArrowLeft'?-1:event.key==='ArrowRight'?1:0):(event.key==='ArrowUp'?-1:event.key==='ArrowDown'?1:0);return{portId:axis.portId,value:event.key==='Home'?0:clamp(axisValue(axis)+direction*(event.shiftKey?1:512),limits.min,limits.max)};}));
  }
  $effect(()=>{if(!active||disabled)releaseAll();});
  onMount(()=>{
    const hidden=()=>{if(document.hidden)releaseAll();};window.addEventListener('pointerup',pointerEnd);window.addEventListener('pointercancel',pointerEnd);window.addEventListener('blur',releaseAll);document.addEventListener('visibilitychange',hidden);
    return()=>{window.removeEventListener('pointerup',pointerEnd);window.removeEventListener('pointercancel',pointerEnd);window.removeEventListener('blur',releaseAll);document.removeEventListener('visibilitychange',hidden);releaseAll();};
  });
  onDestroy(releaseAll);
</script>

<!-- svelte-ignore a11y_no_noninteractive_element_interactions (Contain input events so they cannot move the surrounding canvas.) -->
<section class="controls" aria-label="Debug controller controls" onpointerdown={event=>event.stopPropagation()} onpointermove={event=>event.stopPropagation()} onpointerup={event=>{event.stopPropagation();pointerEnd(event);}} onkeydown={event=>event.stopPropagation()} onkeyup={event=>event.stopPropagation()} onwheel={event=>event.stopPropagation()}>
  {#if layout.axes.length}<section class="axis-panel" aria-label="Axes">
    <div class="section-heading"><h4>Axes</h4><button type="button" class="reset" disabled={!active||disabled} aria-label="Reset axes to neutral" title="Reset axes to neutral" onclick={resetAxes}><Icon name="refresh" size={14}/></button></div>
    {#if layout.axes.length>=2}<button type="button" class="xy-pad" aria-label="XY pad for axes 1 and 2" title="Drag to move axes 1 and 2. Arrow keys move; Shift makes fine adjustments; Home resets." disabled={!active||disabled} onpointerdown={padDown} onpointermove={padMove} onpointerup={pointerEnd} onpointercancel={pointerEnd} onlostpointercapture={pointerEnd} onkeydown={padKey}><span class="pad-cross horizontal"></span><span class="pad-cross vertical"></span><span class="pad-dot" style:left={`${padPosition(0)}%`} style:top={`${padPosition(1)}%`}></span><span class="pad-label">X / Y</span></button>{/if}
    <div class="axis-list">{#each layout.axes as axis,index (axis.portId)}{@const limits=range(axis)}

      <Axis label={`Axis ${index+1}`} controlId={axis.portId} value={axisValue(axis)} min={limits.min} max={limits.max} interactive disabled={!active||disabled} change={value=>axisInput(axis,value)} onstart={dragStart} onend={pointerEnd} />
    {/each}</div>
  </section>{/if}
  {#if layout.hats.length}<section class="hat-panel" aria-label="Hats"><h4>Hats</h4><div class="hat-grid">{#each layout.hats as id,index (id)}

      <Hat label={`Hat ${index+1}`} controlId={id} value={typeof values[id]==='number'?values[id]:-1} interactive disabled={!active||disabled} change={value=>hatValue(id,value)} />
    {/each}</div></section>{/if}
  {#if layout.buttons.length}<section class="button-panel" aria-label="Buttons"><h4>Buttons</h4><div class="button-grid">{#each layout.buttons as id,index (id)}<button type="button" class="button-control" class:pressed={values[id]===true} disabled={!active||disabled} aria-label={`Button ${index+1}${values[id]===true?', pressed':''}`} aria-pressed={values[id]===true} onpointerdown={event=>buttonDown(event,id)} onpointerup={pointerEnd} onpointercancel={pointerEnd} onlostpointercapture={pointerEnd} onkeydown={event=>keyDown(event,id)} onkeyup={event=>keyUp(event,id)} onblur={()=>{keys.delete(id);for(const [pointer,port] of pointers)if(port===id)pointers.delete(pointer);release(id);}}><Icon name="gamepad" size={10}/>{index+1}</button>{/each}</div></section>{/if}
</section>

<style>
  .axis-list{display:grid;gap:8px;min-width:0}.hat-grid{display:grid;grid-template-columns:repeat(auto-fit,minmax(min(100%,112px),1fr));gap:8px}

  .button-grid :global(.button-cell > .connection-badge) { right: 0; }
  .controls{display:grid;gap:14px;min-width:0}.controls section{min-width:0}.section-heading{display:flex;align-items:center;justify-content:space-between;gap:8px}h4{margin:0 0 10px;font-size:var(--ui-heading-size);font-weight:570;color:var(--soft)}.section-heading h4{margin:0}
  .axis-panel,.hat-panel,.button-panel{padding-top:12px;border-top:1px solid var(--line)}.reset{display:grid;place-items:center;width:26px;height:26px;padding:0;border:0;border-radius:4px;background:transparent;color:var(--muted);cursor:pointer}.reset:hover:enabled{background:var(--surface-raised);color:var(--accent)}.reset:disabled{opacity:.45}
  .button-grid{display:grid;grid-template-columns:repeat(auto-fill,minmax(min(100%,var(--ui-input-cell-min-width)),1fr));gap:var(--ui-input-cell-gap)}.button-control{display:flex;align-items:center;justify-content:center;gap:4px;min-width:0;height:var(--ui-input-cell-height);min-height:var(--ui-input-cell-height);box-sizing:border-box;border:1px solid var(--line);border-radius:5px;background:var(--input-bg);color:var(--soft);font-size:var(--ui-text-size);font-variant-numeric:tabular-nums;touch-action:none;cursor:pointer}.button-control:hover:enabled{border-color:var(--accent)}.button-control.pressed{border-color:var(--accent);background:var(--accent-bg);color:var(--accent)}button:focus-visible{outline:2px solid var(--accent);outline-offset:2px}button:disabled{opacity:.45;cursor:not-allowed}
  .xy-pad{display:block;position:relative;width:100%;height:140px;margin:10px 0;padding:0;overflow:hidden;touch-action:none;border:1px solid var(--line);border-radius:5px;background:var(--input-bg);cursor:crosshair}.pad-cross{position:absolute;background:var(--line);pointer-events:none}.horizontal{left:0;right:0;top:50%;height:1px}.vertical{top:0;bottom:0;left:50%;width:1px}.pad-dot{position:absolute;width:12px;height:12px;border-radius:50%;background:var(--accent);transform:translate(-50%,-50%);pointer-events:none}.pad-label{position:absolute;bottom:6px;right:8px;color:var(--muted);font-size:var(--ui-text-size);pointer-events:none}
</style>
