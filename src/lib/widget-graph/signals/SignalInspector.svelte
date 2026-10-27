<script lang="ts">
  import Icon from '../../Icon.svelte';
  import type { WidgetSource } from '../../../../packages/widget-compiler/src/types.ts';
  import type { BehaviorSignal, BehaviorOutput, Scalar } from '../../../../packages/widget-runtime/src/behavior.ts';
  import { renameSignal, removeSignal, renamePort, removePort, associatePort, changeSignalType, syncSignal, sampling, type Selection } from './authoring.ts';
  let {source,selected,onchange,onselect}: {source:WidgetSource;selected:string;onchange:(source:WidgetSource)=>void;onselect?:(id:string)=>void}=$props();
  let error=$state('');
  const signal=$derived(source.behavior.signals.find(s=>s.id===selected));
  const direction=$derived(selected.startsWith('feedback:')?'outputs':'inputs');
  const endpoint=$derived(selected.startsWith('output:')||selected.startsWith('feedback:')?selected.slice(selected.indexOf(':')+1):'');
  const port=$derived(source[direction].find(p=>p.id===endpoint));
  const output=$derived(selected.startsWith('output:')?source.behavior.outputs?.find(p=>p.id===endpoint):undefined);
  const visual=$derived(selected.startsWith('domain:')?source.visualDomains.find(d=>d.signal===selected.slice(7)):undefined);
  const owned=$derived(source.design?.owners.find(o=>o.signal===selected));
  $effect(()=>{selected;error='';});
  function attempt(work:()=>WidgetSource|Selection){try{const result=work();error='';if('source'in result){onchange(result.source);onselect?.(result.selected);}else onchange(result);}catch(cause){error=cause instanceof Error?cause.message:'Change could not be applied.';}}
  function edit(work:(next:WidgetSource)=>void){attempt(()=>{const next=structuredClone(source);work(next);if(signal)syncSignal(next,signal.id);return next;});}
  function patchSignal(patch:Partial<BehaviorSignal>){edit(next=>Object.assign(next.behavior.signals.find(s=>s.id===selected)!,patch));}
  function patchOutput(patch:Partial<BehaviorOutput>){edit(next=>{Object.assign(next.behavior.outputs!.find(o=>o.id===endpoint)!,patch);if(port){const item=next.behavior.signals.find(s=>s.id===port.signal);if(item){const {neutral,...shape}=patch;Object.assign(item,shape,neutral===undefined?{}:{initial:neutral});syncSignal(next,item.id);}}});}
  function scalar(type:string,value:string):Scalar{return type==='boolean'?value==='true':type==='number'?Number(value):value;}
  function numeric(value:string){if(!value.trim()||!Number.isFinite(Number(value)))throw new Error('Enter a finite number.');return Number(value);}
  function setInitial(value:Scalar){edit(next=>{const item=next.behavior.signals.find(s=>s.id===selected)!;item.initial=value;for(const owner of next.design?.owners??[])if(owner.signal===selected){const region=next.behavior.regions.find(r=>r.id===owner.region);if(region)owner.values[region.initial]=value;}});}
  function setSource(value:BehaviorSignal['source']){edit(next=>{const item=next.behavior.signals.find(s=>s.id===selected)!;if(value!=='local'&&next.design?.owners.some(o=>o.signal===selected))throw new Error('Remove state ownership before changing the source.');if(next.inputs.some(p=>p.signal===selected)&&value!=='local'||next.outputs.some(p=>p.signal===selected)&&value!=='feedback')throw new Error('Disconnect the profile endpoint before changing the source.');item.source=value;if(value!=='feedback'){delete item.freshnessMs;delete item.qualitySignal;delete item.requireGameConfirmed;}});}
  function outputType(type:BehaviorOutput['type']){if(port){attempt(()=>changeSignalType(source,port.signal,type));return;}edit(next=>{const index=next.behavior.outputs!.findIndex(o=>o.id===endpoint);next.behavior.outputs![index]={id:endpoint,...type==='number'?{type,min:0,max:100,neutral:0}:type==='enum'?{type,values:['off','on'],neutral:'off'}:{type,neutral:false}} as BehaviorOutput;});}
  function remove(){if(signal)attempt(()=>removeSignal(source,signal.id));else if(endpoint)attempt(()=>removePort(source,direction,endpoint));else if(visual){edit(next=>next.visualDomains=next.visualDomains.filter(d=>d.signal!==visual.signal));onselect?.('');}}
</script>

<section class="signal-inspector" aria-label="Signal properties">
  {#if signal}
    <h3>Signal</h3>
    <label>ID<input value={signal.id} onchange={e=>attempt(()=>renameSignal(source,signal.id,e.currentTarget.value))}/></label>
    <label>Source<select value={signal.source} onchange={e=>setSource(e.currentTarget.value as BehaviorSignal['source'])}><option value="local">Local state</option><option value="input">External input</option><option value="feedback">Feedback</option></select></label>
    <label>Type<select value={signal.type} disabled={!!owned} onchange={e=>attempt(()=>changeSignalType(source,signal.id,e.currentTarget.value as BehaviorSignal['type']))}><option value="boolean">Boolean</option><option value="number">Number</option><option value="enum">Enum</option></select></label>
    {#if owned}<p>Owned by {owned.region}. Initial value follows its initial state.</p>{/if}
    {#if signal.type==='boolean'}<label>Initial<select value={String(signal.initial)} onchange={e=>setInitial(e.currentTarget.value==='true')}><option value="false">False</option><option value="true">True</option></select></label>
    {:else}<label>Initial<input type={signal.type==='number'?'number':'text'} value={String(signal.initial)} onchange={e=>{try{setInitial(signal.type==='number'?numeric(e.currentTarget.value):e.currentTarget.value);}catch(cause){error=cause instanceof Error?cause.message:'Enter a value.';}}}/></label>{/if}
    {#if signal.type==='number'}<div class="two">{#each ['min','max'] as field}<label>{field==='min'?'Minimum':'Maximum'}<input type="number" value={signal[field as 'min'|'max']} onchange={e=>attempt(()=>{const value=numeric(e.currentTarget.value);const next=structuredClone(source);Object.assign(next.behavior.signals.find(s=>s.id===selected)!,{[field]:value});syncSignal(next,selected);return next;})}/></label>{/each}</div>
    {:else if signal.type==='enum'}<label>Values (comma separated)<input value={signal.values.join(', ')} onchange={e=>patchSignal({values:e.currentTarget.value.split(',').map(v=>v.trim()).filter(Boolean)} as Partial<BehaviorSignal>)}/></label>{/if}
    {#if signal.source==='feedback'}
      <label>Fresh for (ms)<input type="number" value={signal.freshnessMs??1000} onchange={e=>attempt(()=>{const next=structuredClone(source);next.behavior.signals.find(s=>s.id===selected)!.freshnessMs=numeric(e.currentTarget.value);return next;})}/></label>
      <label>Quality signal<select value={signal.qualitySignal??''} onchange={e=>edit(next=>{const item=next.behavior.signals.find(s=>s.id===selected)!;if(e.currentTarget.value)item.qualitySignal=e.currentTarget.value;else delete item.qualitySignal;})}><option value="">None</option>{#each source.behavior.signals.filter(s=>s.type==='enum') as item}<option value={item.id}>{item.id}</option>{/each}</select></label>
      <label class="check"><input type="checkbox" checked={signal.requireGameConfirmed??false} onchange={e=>patchSignal({requireGameConfirmed:e.currentTarget.checked})}/>Require game confirmation</label>
    {/if}
  {:else if port||output}
    <h3>{direction==='inputs'?'To profile':'From profile'}</h3>
    <label>ID<input value={endpoint} onchange={e=>attempt(()=>renamePort(source,direction,endpoint,e.currentTarget.value))}/></label>
    {#if port}
      <label>Label<input value={port.label} onchange={e=>edit(next=>{next[direction].find(p=>p.id===endpoint)!.label=e.currentTarget.value;})}/></label>
      <label>Signal<select value={port.signal} onchange={e=>attempt(()=>associatePort(source,direction,endpoint,e.currentTarget.value))}>{#if !source.behavior.signals.some(s=>s.id===port.signal)}<option value={port.signal}>{port.signal} (missing)</option>{/if}{#each source.behavior.signals.filter(s=>s.source===(direction==='inputs'?'local':'feedback')) as item}<option value={item.id}>{item.id}</option>{/each}</select></label>
      <p>{port.type} · {port.signal}</p>
    {/if}
    {#if output}
      <h4>Output binding</h4><p>Emit actions publish this value. Reset sends its neutral value.</p>
      <label>Type<select value={output.type} onchange={e=>outputType(e.currentTarget.value as BehaviorOutput['type'])}><option value="boolean">Boolean</option><option value="number">Number</option><option value="enum">Enum</option></select></label>
      {#if output.type==='number'}<div class="two">{#each ['min','max'] as field}<label>{field==='min'?'Minimum':'Maximum'}<input type="number" value={output[field as 'min'|'max']} onchange={e=>attempt(()=>{const next=structuredClone(source);const value=numeric(e.currentTarget.value);Object.assign(next.behavior.outputs!.find(o=>o.id===endpoint)!,{[field]:value});if(port){Object.assign(next.behavior.signals.find(s=>s.id===port.signal)!,{[field]:value});syncSignal(next,port.signal);}return next;})}/></label>{/each}</div>{:else if output.type==='enum'}<label>Values<input value={output.values.join(', ')} onchange={e=>patchOutput({values:e.currentTarget.value.split(',').map(v=>v.trim()).filter(Boolean)} as Partial<BehaviorOutput>)}/></label>{/if}
      <label>Neutral{#if output.type==='boolean'}<select value={String(output.neutral)} onchange={e=>patchOutput({neutral:e.currentTarget.value==='true'})}><option value="false">False</option><option value="true">True</option></select>{:else}<input type={output.type==='number'?'number':'text'} value={String(output.neutral)} onchange={e=>patchOutput({neutral:scalar(output.type,e.currentTarget.value)})}/>{/if}</label>
    {:else if direction==='inputs'}<button onclick={()=>edit(next=>{const item=next.behavior.signals.find(s=>s.id===port?.signal);if(item){const {initial,id,source:kind,...shape}=item;const typed=shape.type==='number'?{type:shape.type,min:shape.min,max:shape.max}:shape.type==='enum'?{type:shape.type,values:shape.values}:{type:shape.type};(next.behavior.outputs??=[]).push({id:endpoint,...typed,neutral:initial});}})}><Icon name="plus" size={14}/>Add binding</button>{/if}
  {:else if visual}
    <h3>Rendering samples</h3><p>Sampled values produce images. Behavior receives the original value.</p>
    <label>Signal<select value={visual.signal} onchange={e=>{const id=e.currentTarget.value;attempt(()=>{const next=structuredClone(source),item=next.behavior.signals.find(s=>s.id===id);if(!item)throw new Error('Choose an existing signal.');if(next.visualDomains.some(d=>d.signal===id&&d!==next.visualDomains.find(d=>d.signal===visual.signal)))throw new Error('This signal already has rendering samples.');next.visualDomains[next.visualDomains.findIndex(d=>d.signal===visual.signal)]=sampling(item);return {source:next,selected:`domain:${id}`};});}}>{#if !source.behavior.signals.some(s=>s.id===visual.signal)}<option value={visual.signal}>{visual.signal} (missing)</option>{/if}{#each source.behavior.signals as item}<option value={item.id}>{item.id}</option>{/each}</select></label>
    {#if source.behavior.signals.find(s=>s.id===visual.signal)?.type==='number'}<div class="two">{#each ['min','max','samples','maxErrorPixels','travelPixels','smoothingMs'] as field}<label>{({min:'Minimum',max:'Maximum',samples:'Samples',maxErrorPixels:'Maximum error (px)',travelPixels:'Travel (px)',smoothingMs:'Smoothing (ms)'} as Record<string,string>)[field]}<input type="number" value={(visual as unknown as Record<string,number>)[field]??0} onchange={e=>attempt(()=>{const next=structuredClone(source);Object.assign(next.visualDomains.find(d=>d.signal===visual.signal)!,{[field]:numeric(e.currentTarget.value)});return next;})}/></label>{/each}</div>{:else}<label>Values (comma separated)<input value={(visual.values??[]).map(String).join(', ')} onchange={e=>edit(next=>{const item=next.visualDomains.find(d=>d.signal===visual.signal)!;const type=next.behavior.signals.find(s=>s.id===visual.signal)?.type;item.values=e.currentTarget.value.split(',').map(v=>scalar(type??'enum',v.trim()));})}/></label>{/if}
  {/if}
  {#if signal||port||output||visual}<button onclick={remove}><Icon name="trash" size={14}/>Delete</button>{/if}
  {#if error}<p class="error" role="alert">{error}</p>{/if}
</section>

<style>
  .signal-inspector{font-size:var(--ui-text-size);min-width:0}h3{font-size:var(--ui-heading-size);margin:0 0 12px}h4{font-size:var(--ui-heading-size);margin:16px 0 8px}label{display:grid;gap:5px;color:#bed0d5;margin:9px 0}input,select{min-width:0;width:100%;box-sizing:border-box;padding:7px;border:1px solid #3a5360;border-radius:4px;background:var(--bg,#0d1820);color:var(--text,#deecee);font:inherit}.two{display:grid;grid-template-columns:1fr 1fr;gap:9px}.check{display:flex;align-items:center;gap:7px}.check input{width:auto}p{line-height:var(--ui-line-height);color:#8ea9b5}button{display:inline-flex;align-items:center;gap:6px;font:inherit;font-size:var(--ui-action-size);color:inherit;background:var(--surface,#17252d);border:1px solid var(--line,#30434e);border-radius:5px;padding:7px 10px;cursor:pointer;margin-top:8px}.error{color:#ffccbd}button:focus-visible,input:focus-visible,select:focus-visible{outline:2px solid #9adbc8;outline-offset:2px}button:disabled{opacity:.4}
</style>
