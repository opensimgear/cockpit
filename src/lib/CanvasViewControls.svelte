<script lang="ts">
  import Icon from './Icon.svelte';
  let {zoom,layoutBusy=false,readOnly=false,onfit,onlayout,onzoom}: {
    zoom:number;layoutBusy?:boolean;readOnly?:boolean;
    onfit:()=>void;onlayout:()=>void;onzoom:(factor:number)=>void;
  }=$props();
</script>

<div class="canvas-view-controls">
  <button class="canvas-icon-button" type="button" aria-label="Fit all" title="Fit all" onclick={onfit}><Icon name="fit" size={16}/></button>
  <button class="canvas-icon-button" type="button" aria-label="Auto-layout" title={layoutBusy?'Arranging with ELK…':'Auto-layout with ELK Layered'} aria-busy={layoutBusy} disabled={readOnly||layoutBusy} onclick={onlayout}><Icon name="layout" size={16}/></button>
  <div class="zoom-controls">
    <button type="button" aria-label="Zoom out" title="Zoom out" onclick={()=>onzoom(1/1.2)}><Icon name="minus" size={14}/></button>
    <output aria-label="Canvas zoom">{Math.round(zoom*100)}%</output>
    <button type="button" aria-label="Zoom in" title="Zoom in" onclick={()=>onzoom(1.2)}><Icon name="plus" size={14}/></button>
  </div>
</div>

<style>
  .canvas-view-controls{display:flex;align-items:center;gap:8px;margin-left:auto}
  button{display:grid;place-items:center;flex-shrink:0;padding:0;background:transparent;color:var(--soft);cursor:pointer}
  button:hover:enabled{background:var(--surface-raised);color:var(--text)}
  button:focus-visible{outline:2px solid var(--accent);outline-offset:2px}
  button:disabled{opacity:.45;cursor:default}
  .canvas-icon-button{width:32px;height:32px;border:1px solid var(--line);border-radius:6px}
  .zoom-controls{display:flex;align-items:center;border:1px solid var(--line);border-radius:6px}
  .zoom-controls button{width:30px;height:30px;border:0;border-radius:5px}
  output{min-width:44px;text-align:center;font-size:10px;color:var(--muted)}
</style>
