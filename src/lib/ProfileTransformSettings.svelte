<script lang="ts">
  import ProfileProcessingEditor from './ProfileProcessingEditor.svelte';
  import {PROCESSING_PRESETS,processingSummary,type ProcessingPreset}from'./profile-processing';
  import {portContractLabel}from'./profile-canvas';
  import type{DeepReadonly,ProfilePortContract,ProfileProcessingNode}from'./profiles';
  let{node,change,configure,actions=[]}:{node:DeepReadonly<ProfileProcessingNode>;change:(node:ProfileProcessingNode)=>boolean;configure:(kind:ProcessingPreset,parameters?:Record<string,unknown>)=>boolean;actions?:{label:string;contract:DeepReadonly<ProfilePortContract>}[]}=$props();
  function parameter(key:string,input:HTMLInputElement|HTMLSelectElement){const previous=(node.operation as unknown as Record<string,unknown>)[key],value=typeof previous==='number'?Number(input.value):input.value,accepted=node.draft?configure(node.draft.preset as ProcessingPreset,{[key]:value}):change({...JSON.parse(JSON.stringify(node)),operation:{...node.operation,[key]:value}});if(!accepted)input.value=String(previous);}
  function type(input:HTMLSelectElement){const previous=node.draft?.preset;if(!configure(input.value as ProcessingPreset)&&previous)input.value=previous;}
  const heading=(key:string)=>key==='factor'?'Multiply by':key.replace(/([A-Z])/g,' $1').replace(/^./,letter=>letter.toUpperCase());
</script>

<section class="transform-settings" aria-label="Transformation settings">
  <h4>Transformation</h4>
  {#if node.draft}
    <label>Transformation type<select aria-label="Transformation type" value={node.draft.preset} onchange={event=>type(event.currentTarget)}>{#each PROCESSING_PRESETS as [kind,label]}<option value={kind}>{label}</option>{/each}</select></label>
    <p class="pending" role="status">Needs input. Connect a source to establish the signal type, unit and range. Settings are saved with this block.</p>
  {:else}<p>{processingSummary(node)}</p>{#if node.inputs[0]}<p class="contract">{portContractLabel(node.inputs[0].contract)}</p>{/if}{/if}
  <div class="parameters">
    {#each Object.entries(node.operation).filter(([key])=>!['kind','rule','inputs','points','clamp','parameters','code','source','value'].includes(key)) as [key,value]}
      {#if typeof value==='number'}<label>{heading(key)}<input aria-label={heading(key)} type="number" step="any" value={value} onchange={event=>parameter(key,event.currentTarget)}/></label>
      {:else if typeof value==='string'}<label>{heading(key)}{#if ['rounding','comparison','edge','overflow'].includes(key)}<select value={value} onchange={event=>parameter(key,event.currentTarget)}>{#each key==='rounding'?['none','nearest','floor','ceil','truncate']:key==='comparison'?['greater','greater-equal','less','less-equal','equal','not-equal']:key==='edge'?['rising','falling','change']:['reject','truncate'] as option}<option value={option}>{option}</option>{/each}</select>{:else}<input value={value} onchange={event=>parameter(key,event.currentTarget)}/>{/if}</label>{/if}
    {/each}
  </div>
  <details><summary>Advanced transformation settings</summary>{#if node.draft}<p>Connect the first input before editing declared ports.</p>{/if}<ProfileProcessingEditor {node} {change} {actions} portsEditable={!node.draft}/></details>
</section>

<style>
  h4{font-size:12px;color:var(--soft);margin:16px 0 12px}.transform-settings{min-width:0}label{display:grid;gap:6px;font-size:10px;color:var(--muted);margin:10px 0}input,select{box-sizing:border-box;min-width:0;width:100%;padding:8px;border:1px solid var(--line);border-radius:5px;background:var(--input-bg);color:var(--text);font:inherit;font-size:11px}p{font-size:10px;color:var(--muted);line-height:1.7;overflow-wrap:anywhere}.pending{color:var(--accent)}.parameters{display:grid;grid-template-columns:repeat(2,minmax(0,1fr));gap:0 10px}details{margin:16px 0;border-top:1px solid var(--line);padding-top:12px}summary{font-size:10px;color:var(--soft);cursor:pointer}
</style>
