<script lang="ts">
  import ProfileProcessingEditor from './ProfileProcessingEditor.svelte';
  import Icon from './Icon.svelte';
  import {PROCESSING_PRESETS,type ProcessingPreset}from'./profile-processing';
  import type{DeepReadonly,ProfilePortContract,ProfileProcessingNode}from'./profiles';
  let{node,change,configure,actions=[],typeOptions,changeType,remove}:{
    node:DeepReadonly<ProfileProcessingNode>;change:(node:ProfileProcessingNode)=>boolean;
    configure:(kind:ProcessingPreset,parameters?:Record<string,unknown>)=>boolean;
    actions?:{label:string;contract:DeepReadonly<ProfilePortContract>}[];
    typeOptions?:{value:ProcessingPreset;label:string}[];changeType?:(kind:ProcessingPreset)=>boolean|void;
    remove?:()=>void;
  }=$props();
  const preset=$derived(node.draft?.preset??(node.operation.kind==='builtin'?(node.operation.rule.kind==='gesture'?node.operation.rule.gesture:node.operation.rule.kind):node.operation.kind));
  const options=$derived(typeOptions??(node.draft?PROCESSING_PRESETS.map(([value,label])=>({value,label})):[]));
  const label=$derived(PROCESSING_PRESETS.find(([value])=>value===preset)?.[1]??node.name);
  function type(input:HTMLSelectElement){const accepted=changeType?changeType(input.value as ProcessingPreset):node.draft?configure(input.value as ProcessingPreset):false;if(accepted===false)input.value=preset;}
</script>

<section class="transform-settings" aria-label="Transformation settings">
  <div class="transform-type-controls"><label class="transform-type">Type<select aria-label="Transformation type" value={preset} disabled={!options.length} onchange={event=>type(event.currentTarget)}>
    {#if !options.some(option=>option.value===preset)}<option value={preset}>{label}</option>{/if}
    {#each options as option}<option value={option.value}>{option.label}</option>{/each}
  </select></label>
  {#if remove}<button class="delete-transform" aria-label={`Delete ${node.name}`} onclick={remove}><Icon name="trash" size={14}/>Delete</button>{/if}</div>
  <ProfileProcessingEditor compact {node} {change} {actions} portsEditable={!node.draft}/>
</section>

<style>
  .transform-settings{display:grid;grid-template-columns:minmax(140px,190px) minmax(0,1fr);align-items:stretch;gap:12px;min-width:0}.transform-type-controls{display:flex;flex-direction:column;align-items:stretch;gap:8px;padding-right:12px;border-right:1px solid var(--line);min-width:0}.transform-type{display:grid;gap:4px;margin:0;font-size:var(--ui-text-size);color:var(--muted)}select{box-sizing:border-box;min-width:0;width:100%;min-height:30px;height:30px;padding:4px 8px;border:1px solid var(--line);border-radius:5px;background:var(--input-bg);color:var(--text);font:inherit}select:disabled{opacity:.65}.delete-transform{display:inline-flex;align-items:center;justify-content:center;gap:6px;align-self:flex-start;min-height:30px;padding:4px 8px;border:1px solid #cd4552;border-radius:4px;background:#b32b38;color:#fff;font:inherit;font-size:var(--ui-text-size);cursor:pointer}.delete-transform:hover:enabled{background:#98232e;border-color:#e35d69}.delete-transform:disabled{opacity:.4;cursor:default}
  @media(max-width:600px){.transform-settings{grid-template-columns:minmax(110px,140px) minmax(0,1fr);gap:8px}}
</style>
