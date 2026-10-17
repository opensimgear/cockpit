<script lang="ts">
  import type { DeepReadonly, ProfileScalarSchema, ProfileValue } from './profiles';
  let { schema, value, label = 'Value', allowUnknown = false, change }: {schema:DeepReadonly<ProfileScalarSchema>;value:ProfileValue;label?:string;allowUnknown?:boolean;change:(value:ProfileValue)=>void}=$props();
  const initial=()=>schema.type==='boolean'?false:schema.type==='rgb'?'000000':schema.type==='text'?'':schema.type==='enum'?schema.values[0]:schema.min;
</script>
<div class="typed-value">
  {#if allowUnknown}<label class="unknown"><input type="checkbox" checked={value===null} onchange={event=>change(event.currentTarget.checked?null:initial())}/> {label}: unknown</label>{/if}
  {#if value!==null}
    {#if schema.type==='boolean'}<label><input type="checkbox" checked={value===true} onchange={event=>change(event.currentTarget.checked)}/> {label}</label>
    {:else if schema.type==='enum'}<label>{label}<select value={String(value)} onchange={event=>change(event.currentTarget.value)}>{#each schema.values as item (item)}<option value={item}>{item}</option>{/each}</select></label>
    {:else if schema.type==='rgb'}<label>{label}<input type="color" value={`#${String(value)}`} oninput={event=>change(event.currentTarget.value.slice(1))}/></label>
    {:else if schema.type==='text'}<label>{label}<input value={String(value)} oninput={event=>change(event.currentTarget.value)}/><small>{schema.encoding} · maximum {schema.maxBytes} bytes</small></label>
    {:else}<label>{label}<input type="number" min={schema.min} max={schema.max} step={schema.type==='integer'?1:'any'} value={Number(value)} oninput={event=>change(Number(event.currentTarget.value))}/><small>{schema.min}…{schema.max} {schema.unit??''}</small></label>{/if}
  {/if}
</div>
<style>
  .typed-value label{display:grid;gap:6px;font-size:10px;color:var(--muted);margin:9px 0}.typed-value label:has(input[type="checkbox"]){display:flex;align-items:center;gap:8px}.typed-value input:not([type="checkbox"]),select{width:100%;min-width:0;box-sizing:border-box;background:var(--input-bg);border:1px solid var(--line);border-radius:5px;padding:7px;color:var(--text);font-size:11px}small{font-size:9px;line-height:1.5;color:var(--muted)}
</style>
