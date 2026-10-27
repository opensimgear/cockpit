<script lang="ts">
  import Icon from '../../Icon.svelte';
  import GenericExpression from '../../GenericExpression.svelte';
  import GenericSceneNode from '../../GenericSceneNode.svelte';
  import ActionEditor from './ActionEditor.svelte';
  import {lowerWidgetDesign} from '../../../../packages/widget-compiler/src/index.ts';
  import type {WidgetSource} from '../../../../packages/widget-compiler/src/types.ts';
  import type {BehaviorAction,BehaviorTransition} from '../../../../packages/widget-runtime/src/behavior.ts';
  import {assertId,renameEvent,renameTimer,removeEvent,removeTimer,removeTransition,createAction} from './authoring';
  let {source,selected,regionId,transitionId,onchange,onselect=()=>{},ontransition=()=>{}}:{source:WidgetSource;selected:string;regionId:string;transitionId:string;onchange:(source:WidgetSource)=>void;onselect?:(id:string)=>void;ontransition?:(id:string)=>void}=$props();
  let error=$state('');
  const region=$derived(source.behavior.regions.find(r=>r.id===regionId));
  const transition=$derived(region?.transitions.find(t=>t.id===transitionId));
  const event=$derived(selected.startsWith('event:')?source.behavior.events.find(e=>e.id===selected.slice(6)):undefined);
  const timer=$derived(selected.startsWith('timer:')?source.behavior.timers?.find(t=>t.id===selected.slice(6)):undefined);
  const hit=$derived(selected.startsWith('hit:')?source.hitRegions.find(h=>h.id===selected.slice(4)):undefined);
  const references=$derived([...source.parameters.map(p=>p.id),...source.behavior.signals.map(s=>s.id),...(source.design?.nodes.map(n=>n.id)??[])]);
  const regions=$derived(source.behavior.regions.map(r=>r.id));
  const values=$derived(Object.fromEntries([...source.parameters.map(p=>[p.id,p.default]),...source.behavior.signals.map(s=>[s.id,s.initial])]));
  const initialRegions=$derived(Object.fromEntries(source.behavior.regions.map(r=>[r.id,r.initial])));
  const preview=$derived.by(()=>{try{return lowerWidgetDesign(source).scene;}catch{return source.scene;}});
  const external=$derived(source.behavior.signals.filter(s=>s.source!=='local'&&!source.behavior.signals.some(f=>f.qualitySignal===s.id)));
  function commit(fn:(next:WidgetSource)=>WidgetSource|void,after?:()=>void){try{const next=structuredClone(source),result=fn(next)??next;onchange(result);after?.();error='';}catch(e){error=e instanceof Error?e.message:'Could not apply change.';}}
  function patchTransition(patch:Partial<BehaviorTransition>){commit(next=>{const t=next.behavior.regions.find(r=>r.id===regionId)!.transitions.find(t=>t.id===transitionId)!;Object.assign(t,patch);if(patch.to==='')delete t.to;});}
  function renameTransition(name:string){commit(next=>{const r=next.behavior.regions.find(r=>r.id===regionId)!;assertId(name,r.transitions.map(t=>t.id),transitionId);r.transitions.find(t=>t.id===transitionId)!.id=name;for(const effect of next.design?.effects??[])if(effect.region===regionId&&effect.transition===transitionId)effect.transition=name;},()=>ontransition(name));}
  function addAction(type:BehaviorAction['type']){try{if((transition?.actions?.length??0)>=8)throw new Error('A transition supports up to eight actions.');const action=createAction(source,type,transition?.actions??[]);patchTransition({actions:[...(transition?.actions??[]),action]});}catch(e){error=e instanceof Error?e.message:'Could not add action.';}}
  function patchTimer(key:'event'|'delayMs'|'repeat',value:string|number|boolean){commit(next=>{Object.assign(next.behavior.timers!.find(t=>t.id===timer!.id)!,{[key]:value});});}
  function geometry(key:'x'|'y'|'width'|'height',value:number){commit(next=>{next.hitRegions.find(h=>h.id===hit!.id)![key]=value;});}
</script>

<div class="behavior-inspector">
{#if error}<p class="error" role="alert">{error}</p>{/if}
{#if transition && region}
  <h3>Transition</h3>
  <label>ID<input value={transition.id} onchange={e=>renameTransition(e.currentTarget.value.trim())}/></label>
  <label>From<select value={transition.from} onchange={e=>patchTransition({from:e.currentTarget.value})}><option value="*">Any state</option>{#each region.states as state}<option value={state}>{state}</option>{/each}</select></label>
  <label>To<select value={transition.to??''} onchange={e=>patchTransition({to:e.currentTarget.value})}><option value="">Stay in current state</option>{#each region.states as state}<option value={state}>{state}</option>{/each}</select></label>
  <label>Event<select value={transition.event} onchange={e=>patchTransition({event:e.currentTarget.value})}>{#each source.behavior.events as ev}<option value={ev.id}>{ev.id}</option>{/each}</select></label>
  <label>Priority<input type="number" min="-32768" max="32767" step="1" value={transition.priority??0} onchange={e=>patchTransition({priority:e.currentTarget.valueAsNumber})}/></label>
  <p class="hint">Higher priority wins. Overlapping transitions for the same event need different priorities.</p>
  <label class="check"><input type="checkbox" checked={!!transition.guard} onchange={e=>commit(next=>{const t=next.behavior.regions.find(r=>r.id===regionId)!.transitions.find(t=>t.id===transitionId)!;if(e.currentTarget.checked)t.guard={literal:true};else delete t.guard;})}/>Guard</label>
  {#if transition.guard}<GenericExpression value={transition.guard} {references} {regions} label="Condition" onchange={guard=>patchTransition({guard})}/>{/if}
  <h4>Actions</h4><div class="chips">{#each ['set','emit','cue','start-timer','cancel-timer'] as kind}<button onclick={()=>addAction(kind as BehaviorAction['type'])}><Icon name="plus" size={12}/>{kind}</button>{/each}</div>
  <ActionEditor {source} actions={transition.actions??[]} onchange={actions=>patchTransition({actions})}/>
  <p class="hint">State-owned signals and outputs are written by their state machine and cannot be explicit action targets.</p>
  <button class="danger" onclick={()=>commit(()=>removeTransition(source,regionId,transition.id),()=>ontransition(''))}><Icon name="trash" size={14}/>Delete</button>
{:else if event}
  <h3>Event</h3><label>ID<input value={event.id} onchange={e=>{const name=e.currentTarget.value.trim();commit(()=>renameEvent(source,event.id,name),()=>onselect(`event:${name}`));}}/></label>
  <h4>Allowed updates</h4><p class="hint">Only these external signals may be supplied with this event.</p>
  {#each external as signal}<label class="check"><input type="checkbox" checked={event.updates?.includes(signal.id)??false} onchange={e=>{const checked=e.currentTarget.checked;commit(next=>{const ev=next.behavior.events.find(ev=>ev.id===event.id)!;ev.updates=checked?[...(ev.updates??[]),signal.id]:(ev.updates??[]).filter(id=>id!==signal.id);});}}/>{signal.id}</label>{/each}
  {#if !external.length}<p class="hint">Add an input or feedback signal in Signals to allow updates.</p>{/if}
  <p class="hint">Deleting also removes dependent transitions, timers and hit assignments.</p><button class="danger" onclick={()=>commit(()=>removeEvent(source,event.id),()=>onselect(''))}><Icon name="trash" size={14}/>Delete</button>
{:else if timer}
  <h3>Timer</h3><label>ID<input value={timer.id} onchange={e=>{const name=e.currentTarget.value.trim();commit(()=>renameTimer(source,timer.id,name),()=>onselect(`timer:${name}`));}}/></label>
  <label>Event<select value={timer.event} onchange={e=>patchTimer('event',e.currentTarget.value)}>{#each source.behavior.events as ev}<option value={ev.id}>{ev.id}</option>{/each}</select></label>
  <label>Delay (ms)<input type="number" min="1" max="86400000" step="1" value={timer.delayMs} onchange={e=>patchTimer('delayMs',e.currentTarget.valueAsNumber)}/></label>
  <label class="check"><input type="checkbox" checked={timer.repeat??false} onchange={e=>patchTimer('repeat',e.currentTarget.checked)}/>Repeat</label>
  <p class="hint">Start or cancel this timer from a transition action. Deleting removes its start and cancel actions.</p><button class="danger" onclick={()=>commit(()=>removeTimer(source,timer.id),()=>onselect(''))}><Icon name="trash" size={14}/>Delete</button>
{:else if hit}
  <h3>Hit region</h3><label>ID<input value={hit.id} onchange={e=>{const name=e.currentTarget.value.trim();commit(next=>{assertId(name,next.hitRegions.map(h=>h.id),hit.id);next.hitRegions.find(h=>h.id===hit.id)!.id=name;},()=>onselect(`hit:${name}`));}}/></label>
  <svg class="preview" viewBox={`0 0 ${source.size.width} ${source.size.height}`} role="img" aria-label="Hit region on widget artwork">{#each preview as node}<GenericSceneNode {node} {values} regions={initialRegions} assets={source.assets??[]}/>{/each}<rect x={hit.x} y={hit.y} width={hit.width} height={hit.height} fill="#67dfc433" stroke="#83f5da" stroke-width={Math.max(source.size.width,source.size.height)/150}/></svg>
  <div class="geometry">{#each ['x','y','width','height'] as key}<label>{key}<input type="number" min={key==='width'||key==='height'?1:0} value={hit[key as 'x']} onchange={e=>geometry(key as 'x',e.currentTarget.valueAsNumber)}/></label>{/each}</div>
  {#each ['press','release','activate','cancel'] as kind}<label>{kind}<select value={hit.events[kind as 'press']??''} onchange={e=>{const value=e.currentTarget.value;commit(next=>{const events=next.hitRegions.find(h=>h.id===hit.id)!.events;if(value)events[kind as 'press']=value;else delete events[kind as 'press'];});}}><option value="">None</option>{#each source.behavior.events as ev}<option value={ev.id}>{ev.id}</option>{/each}</select></label>{/each}
  <button class="danger" onclick={()=>commit(next=>{next.hitRegions=next.hitRegions.filter(h=>h.id!==hit.id);},()=>onselect(''))}><Icon name="trash" size={14}/>Delete</button>
{/if}
</div>
<style>
  .behavior-inspector{min-width:0;font-size:var(--ui-text-size)}h3{font-size:var(--ui-heading-size);color:#e0eeee;margin:0 0 12px}h4{font-size:var(--ui-heading-size);color:#c1d9dd;margin:14px 0 8px}label{display:grid;gap:4px;color:#b6cbd0;font-size:var(--ui-text-size);margin:7px 0}input,select{box-sizing:border-box;width:100%;min-width:0;color:#e0eef0;background:#14232d;border:1px solid #3b5360;border-radius:4px;padding:6px;font:inherit}.check{display:flex;align-items:center;gap:7px}.check input{width:auto}.chips{display:flex;flex-wrap:wrap;gap:5px}button{display:inline-flex;align-items:center;gap:5px;color:#c5dbde;background:#243945;border:1px solid #3b5360;border-radius:4px;padding:6px;font:inherit;font-size:var(--ui-action-size);cursor:pointer}.hint{font-size:var(--ui-text-size);color:#96aeb7;line-height:var(--ui-line-height)}.error,.danger{color:#ffb6a6}.error{font-size:var(--ui-text-size);line-height:var(--ui-line-height)}.geometry{display:grid;grid-template-columns:1fr 1fr;gap:0 7px}.preview{width:100%;max-height:180px;background:#0a171f;border:1px solid #3b5360;border-radius:4px}button:focus-visible,input:focus-visible,select:focus-visible{outline:2px solid #83d9c1;outline-offset:2px}
</style>
