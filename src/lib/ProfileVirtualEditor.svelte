<script lang="ts">
  import{untrack}from'svelte';
  import{graphVirtualPorts}from'./profile-graph';
  import type{DeepReadonly,ProfileControllerLayout,ProfileVirtualOutput}from'./profiles';
  let{output,change}:{output:DeepReadonly<ProfileVirtualOutput>;change:(output:Omit<ProfileVirtualOutput,'ports'>)=>void}=$props();
  const copy=<T,>(value:T):T=>JSON.parse(JSON.stringify(value));
  let draft=$state(copy(untrack(()=>output)) as ProfileVirtualOutput),signature=$state(JSON.stringify(untrack(()=>output)));
  const error=$derived.by(()=>{try{graphVirtualPorts(draft.layout);return '';}catch(error){return error instanceof Error?error.message:String(error);}});
  $effect(()=>{const next=JSON.stringify(output);if(next!==signature){signature=next;draft=copy(output) as ProfileVirtualOutput;}});
  function resize(kind:'buttons'|'axes'|'hats',requested:number){
    const count=Math.min(kind==='axes'?16:kind==='hats'?4:128,Math.max(0,Math.floor(requested))),layout=copy(draft.layout);
    const used=new Set([...layout.buttons,...layout.axes.map(axis=>axis.portId),...layout.hats]);
    const fresh=(prefix:string)=>{let n=1;while(used.has(`${prefix}-${n}`))n++;const id=`${prefix}-${n}`;used.add(id);return id;};
    if(kind==='axes')layout.axes=Array.from({length:count},(_,index)=>layout.axes[index]??{portId:fresh('axis'),range:'bipolar',fallback:0});
    else layout[kind]=Array.from({length:count},(_,index)=>layout[kind][index]??fresh(kind==='buttons'?'button':'hat'));
    draft={...draft,layout};
  }
  function axis(index:number,patch:Partial<ProfileControllerLayout['axes'][number]>){draft={...draft,layout:{...draft.layout,axes:draft.layout.axes.map((axis,i)=>i===index?{...axis,...patch}:axis)}};}
</script>
<section class="virtual-editor" aria-label="Virtual controller layout editor"><h4>Logical controller layout</h4><p>Stable logical control IDs stay attached to existing bindings. Native preflight checks the actual backend; editing creates no controller.</p>
  <label>Controller label<input maxlength="80" value={draft.name} oninput={event=>draft={...draft,name:event.currentTarget.value}}/></label><label class="check"><input type="checkbox" checked={draft.required} onchange={event=>draft={...draft,required:event.currentTarget.checked}}/> Required controller</label>
  <div class="counts">{#each ['buttons','axes','hats'] as kind}<label>{kind}<input type="number" min="0" max={kind==='axes'?16:kind==='hats'?4:128} value={draft.layout[kind as 'buttons'|'axes'|'hats'].length} onchange={event=>resize(kind as 'buttons'|'axes'|'hats',Number(event.currentTarget.value))}/></label>{/each}</div>
  <p>At most 128 total controls, 16 axes and four hats per controller; eight controllers per native plan. Removing a connected control requires removing its route explicitly.</p>
  {#each draft.layout.axes as item,index (item.portId)}<fieldset><legend>Axis {index+1} · {item.portId}</legend><label>Native range<select value={item.range} onchange={event=>axis(index,{range:event.currentTarget.value==='unipolar'?'unipolar':'bipolar'})}><option value="bipolar">Bipolar · −32768…32767</option><option value="unipolar">Unipolar · 0…32767</option></select></label><label>Neutral fallback<input type="number" min={item.range==='bipolar'?-32768:0} max="32767" value={item.fallback} oninput={event=>axis(index,{fallback:Number(event.currentTarget.value)})}/></label></fieldset>{/each}
  {#if draft.layout.buttons.length}<details><summary>Button IDs</summary><p>{draft.layout.buttons.join(', ')}</p></details>{/if}{#if draft.layout.hats.length}<details><summary>Hat IDs · −1 neutral, 0…7 directions</summary><p>{draft.layout.hats.join(', ')}</p></details>{/if}
  {#if error}<p class="error" role="status">{error}</p>{/if}<button class="primary-button" disabled={!!error||!draft.name.trim()} onclick={()=>change({id:draft.id,name:draft.name,required:draft.required,layout:copy(draft.layout)})}>Update controller layout</button>
</section>
<style>
  .virtual-editor{border-top:1px solid var(--line);padding-top:15px;margin-top:16px}h4{font-size:12px;color:var(--soft)}p{font-size:10px;line-height:1.75;color:var(--muted);margin:10px 0;overflow-wrap:anywhere}.virtual-editor label{display:grid;gap:6px;font-size:9px;color:var(--muted);margin:10px 0}.virtual-editor input:not([type="checkbox"]),select{width:100%;min-width:0;box-sizing:border-box;background:var(--input-bg);border:1px solid var(--line);border-radius:5px;padding:8px;color:var(--text);font-size:10px}.check{display:flex!important;align-items:center;gap:8px!important}.counts{display:grid;grid-template-columns:repeat(3,1fr);gap:8px}fieldset{border:1px solid var(--line);border-radius:6px;padding:10px;margin:12px 0;min-width:0}legend,summary{font-size:10px;color:var(--soft)}.virtual-editor button{font-size:10px}.error{color:#d3a078}
</style>
