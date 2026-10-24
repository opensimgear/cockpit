<script lang="ts">
  import Icon from '../../Icon.svelte';
  import GenericExpression from '../../GenericExpression.svelte';
  import type {WidgetSource} from '../../../../packages/widget-compiler/src/types.ts';
  import type {BehaviorAction} from '../../../../packages/widget-runtime/src/behavior.ts';
  import {writableSignals,writableOutputs} from './authoring';
  let {source,actions,onchange}:{source:WidgetSource;actions:BehaviorAction[];onchange:(actions:BehaviorAction[])=>void}=$props();
  const references=$derived([...source.parameters.map(p=>p.id),...source.behavior.signals.map(s=>s.id),...(source.design?.nodes.map(n=>n.id)??[])]);
  const regions=$derived(source.behavior.regions.map(r=>r.id));
  function patch(index:number,action:BehaviorAction){onchange(actions.map((a,i)=>i===index?action:a));}
  function move(index:number,offset:number){const next=[...actions];[next[index],next[index+offset]]=[next[index+offset],next[index]];onchange(next);}
</script>
<p class="hint">Guards and action expressions read the same event snapshot. A set action does not change values read by another action during this event.</p>
{#each actions as action,i}
  <fieldset><legend>{i+1} · {action.type}</legend>
    <div class="tools"><button title="Move earlier" disabled={i===0} onclick={()=>move(i,-1)}><Icon name="upload" size={13}/></button><button title="Move later" disabled={i===actions.length-1} onclick={()=>move(i,1)}><Icon name="download" size={13}/></button><button title="Remove action" onclick={()=>onchange(actions.filter((_,j)=>i!==j))}><Icon name="trash" size={13}/></button></div>
    {#if action.type==='set'}
      <label>Signal<select value={action.signal} onchange={e=>{const s=writableSignals(source).find(s=>s.id===e.currentTarget.value)!;patch(i,{...action,signal:s.id,value:{literal:s.initial}});}}>{#each writableSignals(source) as s}<option value={s.id}>{s.id}</option>{/each}</select></label>
      {#if !writableSignals(source).some(s=>s.id===action.signal)}<p class="error">This signal has a state owner or is external. Remove this explicit write.</p>{/if}
      <GenericExpression value={action.value} {references} {regions} label="Value" onchange={value=>patch(i,{...action,value})}/>
    {:else if action.type==='emit'}
      <label>Output<select value={action.port} onchange={e=>{const p=writableOutputs(source).find(p=>p.id===e.currentTarget.value)!;patch(i,{...action,port:p.id,value:{literal:p.neutral}});}}>{#each writableOutputs(source) as p}<option value={p.id}>{p.id}</option>{/each}</select></label>
      {#if !writableOutputs(source).some(p=>p.id===action.port)}<p class="error">This output has a state owner. Remove this explicit write.</p>{/if}
      <GenericExpression value={action.value} {references} {regions} label="Value" onchange={value=>patch(i,{...action,value})}/>
    {:else if action.type==='cue'}<label>Cue<input list="behavior-cues" value={action.cue} onchange={e=>patch(i,{...action,cue:e.currentTarget.value})}/></label>
    {:else}<label>Timer<select value={action.timer} onchange={e=>patch(i,{...action,timer:e.currentTarget.value})}>{#each source.behavior.timers??[] as t}<option value={t.id}>{t.id}</option>{/each}</select></label>{/if}
  </fieldset>
{/each}
<datalist id="behavior-cues">{#each source.sounds??[] as sound}<option value={sound.cue}></option>{/each}</datalist>
<style>
  fieldset{border:1px solid #3b5360;border-radius:5px;margin:8px 0;padding:8px;min-width:0}legend{color:#c1d9dd;font-size:11px}.tools{display:flex;gap:4px;justify-content:flex-end}label{display:grid;gap:4px;color:#b6cbd0;font-size:11px;margin:5px 0}input,select{box-sizing:border-box;width:100%;min-width:0;color:#e0eef0;background:#14232d;border:1px solid #3b5360;border-radius:4px;padding:6px;font:inherit}button{display:inline-flex;align-items:center;color:#c5dbde;background:#243945;border:1px solid #3b5360;border-radius:4px;padding:5px;cursor:pointer}button:disabled{opacity:.4}.hint{font-size:11px;color:#96aeb7;line-height:1.5}.error{font-size:11px;color:#ffb6a6}button:focus-visible,input:focus-visible,select:focus-visible{outline:2px solid #83d9c1;outline-offset:2px}
</style>
