<script lang="ts">
  import ProfileValueEditor from './ProfileValueEditor.svelte';
  import {portContractLabel} from './profile-canvas';
  import type{DeepReadonly,ProfilePort,ProfilePortContract,ProfileScalarSchema}from'./profiles';
  let {port,change,identityEditable=true}:{port:DeepReadonly<ProfilePort>;change:(port:ProfilePort)=>void;identityEditable?:boolean}=$props();
  function update(patch:Partial<ProfilePort>){change({...JSON.parse(JSON.stringify(port)),...patch});}
  function contract(next:ProfilePortContract){update({contract:next});}
  function schema(next:ProfileScalarSchema){if(port.contract.delivery==='state')contract({...port.contract,schema:next,fallback:null});else if(port.contract.delivery==='event')contract({delivery:'event',schema:next});}
  function scalarType(type:string){const next:ProfileScalarSchema=type==='boolean'?{type:'boolean'}:type==='rgb'?{type:'rgb'}:type==='text'?{type:'text',maxBytes:256,encoding:'utf8'}:type==='enum'?{type:'enum',values:['on','off']}:{type:type==='integer'?'integer':'number',min:0,max:type==='integer'?32767:100,unit:null};schema(next);}
  function delivery(value:string){if(!('schema'in port.contract))return;const nextSchema=JSON.parse(JSON.stringify(port.contract.schema)) as ProfileScalarSchema;contract(value==='event'?{delivery:'event',schema:nextSchema}:{delivery:'state',schema:nextSchema,freshnessMs:1000,fallback:null});}
</script>
<fieldset class="port-editor"><legend>{port.direction==='input'?'Input':'Output'}: {port.id}</legend>
  <label>Logical port ID<input value={port.id} readonly={!identityEditable} oninput={event=>update({id:event.currentTarget.value})}/></label>
  <label>Port label<input value={port.name} maxlength="80" oninput={event=>update({name:event.currentTarget.value})}/></label>
  {#if 'schema'in port.contract}
    <label>Signal delivery<select value={port.contract.delivery} onchange={event=>delivery(event.currentTarget.value)}><option value="state">State</option><option value="event">Ordered event</option></select></label>
    <label>Value type<select value={port.contract.schema.type} onchange={event=>scalarType(event.currentTarget.value)}>{#each ['boolean','integer','number','text','enum','rgb'] as type}<option value={type}>{type}</option>{/each}</select></label>
    {@const current=port.contract.schema}
    {#if current.type==='integer'||current.type==='number'}<div class="pair"><label>Minimum<input type="number" value={current.min} step={current.type==='integer'?1:'any'} oninput={event=>schema({...current,min:Number(event.currentTarget.value)})}/></label><label>Maximum<input type="number" value={current.max} step={current.type==='integer'?1:'any'} oninput={event=>schema({...current,max:Number(event.currentTarget.value)})}/></label></div><label>Unit (empty means none)<input value={current.unit??''} oninput={event=>schema({...current,unit:event.currentTarget.value||null})}/></label>
    {:else if current.type==='text'}<label>Maximum bytes<input type="number" min="1" max="4096" value={current.maxBytes} oninput={event=>schema({...current,maxBytes:Number(event.currentTarget.value)})}/></label><label>Encoding<select value={current.encoding} onchange={event=>schema({...current,encoding:event.currentTarget.value==='ascii'?'ascii':'utf8'})}><option value="utf8">UTF-8</option><option value="ascii">ASCII</option></select></label>
    {:else if current.type==='enum'}<label>Enum values (one per line)<textarea rows="3" value={current.values.join('\n')} oninput={event=>schema({type:'enum',values:event.currentTarget.value.split('\n')})}></textarea></label>{/if}
    {#if port.contract.delivery==='state'}<label>Freshness (ms)<input type="number" min="1" max="600000" value={port.contract.freshnessMs} oninput={event=>{if(port.contract.delivery==='state')contract({...port.contract,freshnessMs:Number(event.currentTarget.value),schema:JSON.parse(JSON.stringify(port.contract.schema))});}}/></label><ProfileValueEditor schema={current} value={port.contract.fallback} label="Fallback" allowUnknown change={value=>{if(port.contract.delivery==='state')contract({...port.contract,schema:JSON.parse(JSON.stringify(port.contract.schema)),fallback:value});}}/>{/if}
  {:else}<p>{portContractLabel(port.contract)}</p><p>Ordered action arguments come from the selected advertised endpoint. Map their named inputs with explicit routes.</p>{/if}
</fieldset>
<style>
  .port-editor{min-width:0;border:1px solid var(--line);border-radius:6px;padding:11px;margin:12px 0}.port-editor legend{font-size:10px;color:var(--soft);overflow-wrap:anywhere}.port-editor label{display:grid;gap:6px;font-size:9px;color:var(--muted);margin:9px 0}.port-editor input,.port-editor select,.port-editor textarea{width:100%;min-width:0;box-sizing:border-box;background:var(--input-bg);border:1px solid var(--line);border-radius:5px;padding:7px;color:var(--text);font-size:10px}.pair{display:grid;grid-template-columns:1fr 1fr;gap:8px}p{font-size:10px;line-height:1.7;color:var(--muted)}
</style>
