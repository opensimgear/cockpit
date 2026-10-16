<script lang="ts">
  import Notification from './Notification.svelte';
  import {scriptExample,scriptExamples} from './scriptExamples';
  import {gatewayRequest} from './gateway';
  import {builtins, operationPreset, draftDocument, type MappingProfile, type RuleNode, type RuleInput} from './mappings';
  let {profile, save, validate}: {profile: MappingProfile; save: (profile: MappingProfile) => Promise<void>; validate: (text:string)=>Promise<MappingProfile>} = $props();
  let draft = $state<MappingProfile>((()=>JSON.parse(JSON.stringify(profile)))());
  let selected = $state(0);
  let error = $state('');
  let saving = $state(false);
  let advanced = $state('');
  let example = $state<keyof typeof scriptExamples>('Timed pulse');
  let compileNotice = $state('');
  let dirty = $state(false);
  const nodes = $derived(draft.nodes as RuleNode[]);
  const node = $derived(nodes[selected]);
  const inputOptions = $derived([...draft.roles.flatMap(role => role.controls.map(c => ({label:role.name+' / '+c.id,input:{kind:'source',role:role.id,control:c.id} as RuleInput}))),...nodes.filter(n => n.id !== node?.id).map(n => ({label:'Rule / '+n.id,input:{kind:'node',node:n.id} as RuleInput}))]);
  function changed(){dirty=true;error='';}
  function setOperation(value:string){if(!node)return;node.operation=operationPreset(value);changed();}
  function add(){if(nodes.length>=256)return;const id='rule-'+crypto.randomUUID().slice(0,8);draft.nodes=[...nodes,{id,inputs:[inputOptions[0]?.input],output:null,operation:operationPreset('direct')}];selected=nodes.length-1;changed();}
  async function updateJson(value:string){try{draftDocument(value,profile.id);draft=await validate(value);selected=0;changed();}catch(e){error=String(e);}}
  async function persist(){saving=true;try{await save(draft);dirty=false;error='';}catch(e){error=String(e);}finally{saving=false;}}
  async function checkScript(source:string){compileNotice='Compiling…';try{const result=await gatewayRequest({command:'mapping_script_validate',sourceCode:source});if(!result.mappings?.compiled)throw new Error('No compiler result');compileNotice='Compiled successfully. Start preview to activate.';}catch(e){compileNotice=String(e);}}
  function resizeAxes(index:number,count:number){const layout=draft.targets[index].layout;layout.axes=Array.from({length:Math.min(16,Math.max(0,count))},(_,i)=>layout.axes[i]??{range:'bipolar',fallback:0});changed();}
</script>
<section class="panel editor" aria-label="Mapping draft editor">
  <div class="row"><h2>Draft editor</h2><span>{dirty?'Unsaved edits':'Saved draft'}</span><button class="primary-button" disabled={saving} onclick={persist}>Save draft</button></div>
  <p>Saving keeps the current run unchanged. Start a recording preview to replace it through the normal release and baseline transition.</p>
  {#if error}<Notification message={String(error)} kind="error" />{/if}
  <label>Profile name<input bind:value={draft.name} maxlength="80" oninput={changed}/></label>
  <h3>Logical target layouts</h3>
  {#each draft.targets as target,index}<fieldset><legend>{target.name}</legend><div class="row">
    <label>Buttons<input type="number" min="0" max="128" bind:value={target.layout.buttons} oninput={changed}/></label>
    <label>Axes<input type="number" min="0" max="16" value={target.layout.axes.length} onchange={e=>resizeAxes(index,Number(e.currentTarget.value))}/></label>
    <label>Hats<input type="number" min="0" max="4" bind:value={target.layout.hats} oninput={changed}/></label>
    {#each target.layout.axes as axis,i}<label>Axis {i+1}<select bind:value={axis.range} onchange={changed}><option value="bipolar">Bipolar</option><option value="unipolar">Unipolar</option></select><input aria-label={'Axis '+(i+1)+' fallback'} type="number" min={axis.range==='bipolar'?-32768:0} max="32767" bind:value={axis.fallback} oninput={changed}/></label>{/each}
  </div></fieldset>{/each}
  <h3>Rule chains and modes</h3>
  <p>Use a Gate rule with a button or Toggle rule as its second input for modifiers and layers. Re-enabling a held momentary button waits for release. Buttons combine with OR; axes and hats with multiple writers require an explicit priority list.</p>
  <div class="row"><label>Rule<select bind:value={selected}>{#each nodes as item,i}<option value={i}>{item.id}</option>{/each}</select></label><button class="secondary-button" onclick={add} disabled={nodes.length>=256}>Add rule</button><button class="secondary-button" disabled={nodes.length<=1} onclick={()=>{draft.nodes=nodes.filter((_,i)=>i!==selected);selected=0;changed();}}>Remove rule</button></div>
  {#if node}
    <div class="row"><label>Rule ID<input bind:value={node.id} maxlength="64" oninput={changed}/></label><label>Operation<select value={node.operation.kind==='direct'?'direct':node.operation.rule.kind==='gesture'?String(node.operation.rule.gesture):node.operation.rule.kind} onchange={e=>setOperation(e.currentTarget.value)}><option value="direct">Direct</option>{#each builtins as kind}<option value={kind}>{kind}</option>{/each}</select></label></div>
    <div class="row">{#each node.inputs as input,i}<label>Input {i+1}<select value={JSON.stringify(input)} onchange={e=>{node.inputs[i]=JSON.parse(e.currentTarget.value);changed();}}>{#each inputOptions as option}<option value={JSON.stringify(option.input)}>{option.label}</option>{/each}</select></label>{/each}<button class="secondary-button" disabled={node.inputs.length>=8} onclick={()=>{node.inputs=[...node.inputs,inputOptions[0].input];changed();}}>Add input</button><button class="secondary-button" disabled={node.inputs.length<=1} onclick={()=>{node.inputs=node.inputs.slice(0,-1);changed();}}>Remove input</button></div>
    {#if node.operation.kind==='direct'}<label>Activation<select bind:value={node.operation.activation} onchange={changed}><option value="momentary">Momentary · wait for release after baseline</option><option value="level">Level · synchronize held state</option></select></label>
    {:else}
      <div class="row">{#each Object.entries(node.operation.rule).filter(([key])=>!['kind','curve','steps'].includes(key)) as [key,value]}
        {#if typeof value==='boolean'}<label>{key}<input type="checkbox" checked={value} onchange={e=>{if(node.operation.kind==='transform')node.operation.rule[key]=e.currentTarget.checked;changed();}}/></label>
        {:else if typeof value==='number'}<label>{key}<input type="number" value={value} onchange={e=>{if(node.operation.kind==='transform')node.operation.rule[key]=Number(e.currentTarget.value);changed();}}/></label>
        {:else}<label>{key}<input value={String(value)} onchange={e=>{if(node.operation.kind==='transform')node.operation.rule[key]=e.currentTarget.value;changed();}}/></label>{/if}
      {/each}</div>
      {#each ['curve','steps'] as field}{#if field in node.operation.rule}<label>{field==='curve'?'Curve points [{input, output}] · include both endpoints':'Ordered macro steps [{afterUs, pressed}] · start at zero and end released'}<textarea rows="4" value={JSON.stringify(node.operation.rule[field],null,2)} onchange={e=>{try{if(node.operation.kind==='transform')node.operation.rule[field]=JSON.parse(e.currentTarget.value);changed();}catch(e){error=String(e);}}}></textarea></label>{/if}{/each}
      <p class="timing">Times are microseconds. Single tap waits for the double-tap window; a second tap cancels it. Hold starts at its threshold. Pulses keep their full duration after release; repeat stops on release. A new macro press replaces that rule's sequence. Stop, source loss and layer exit cancel owned timing.</p>
    {/if}
    <div class="row"><label>Output<select value={node.output?.target??''} onchange={e=>{node.output=e.currentTarget.value?{target:e.currentTarget.value,control:{kind:'button',index:0}}:null;changed();}}><option value="">Intermediate rule only</option>{#each draft.targets as target}<option value={target.id}>{target.name}</option>{/each}</select></label>{#if node.output}<label>Control kind<select bind:value={node.output.control.kind} onchange={changed}><option value="button">Button</option><option value="axis">Axis</option><option value="hat">Hat</option></select></label><label>Control index (starts at 0)<input type="number" min="0" max="127" bind:value={node.output.control.index} oninput={changed}/></label>{/if}</div>
  {/if}
  <h3>Custom Rhai logic</h3>
  <p>Scripts run in a separate process. Each callback returns a complete replacement state, output contribution and timer set. Stop or source loss clears all three. Preview is available; live script output awaits qualification.</p>
  <div class="row"><label>Example<select bind:value={example}>{#each Object.keys(scriptExamples) as item}<option value={item}>{item}</option>{/each}</select></label><button class="secondary-button" disabled={(draft.scripts?.length??0)>=16} onclick={()=>{try{draft.scripts=[...(draft.scripts??[]),scriptExample(draft,example)];changed();}catch(e){error=String(e);}}}>Add script example</button></div>
  {#each draft.scripts??[] as script,index}
    <fieldset><legend>Script {index+1}</legend><label>Script ID<input bind:value={script.id} oninput={changed}/></label>
    <label>Rhai source<textarea aria-label={'Rhai source '+(index+1)} rows="14" bind:value={script.source} oninput={()=>{changed();compileNotice='';}}></textarea></label>
    <div class="row"><button class="secondary-button" onclick={()=>checkScript(script.source)}>Check script syntax</button><button class="secondary-button" onclick={()=>{draft.scripts=draft.scripts?.filter((_,i)=>i!==index);changed();}}>Remove script</button></div>
    <label>Declared source inputs JSON<textarea rows="3" value={JSON.stringify(script.inputs,null,2)} onchange={e=>{try{script.inputs=JSON.parse(e.currentTarget.value);changed();}catch(e){error=String(e);}}}></textarea></label>
    <label>Named output bindings JSON<textarea rows="3" value={JSON.stringify(script.outputs,null,2)} onchange={e=>{try{script.outputs=JSON.parse(e.currentTarget.value);changed();}catch(e){error=String(e);}}}></textarea></label>
    <label>Parameters JSON<textarea rows="2" value={JSON.stringify(script.parameters,null,2)} onchange={e=>{try{script.parameters=JSON.parse(e.currentTarget.value);changed();}catch(e){error=String(e);}}}></textarea></label></fieldset>
  {/each}
  {#if compileNotice}<Notification message={String(compileNotice)} />{/if}
  <p>Define on_start(ctx), on_event(ctx) and on_timer(ctx). Use ctx.inputs, ctx.parameters, ctx.state and the deterministic ctx.nowUs clock. Return transaction(state, outputs, timers); outputs use button, axis or hat, and timer(id, microseconds) schedules a callback. Limits: 5 ms evaluation, 100,000 operations, depth 16, 16 KiB state, 64 timers, 128 outputs and 16 log messages per second.</p>
  <label>Axis/hat writer priority JSON<textarea rows="3" value={JSON.stringify(draft.arbitration,null,2)} onchange={e=>{try{draft.arbitration=JSON.parse(e.currentTarget.value);changed();}catch(e){error=String(e);}}}></textarea></label>
  <details><summary>Full draft JSON · roles, targets and rules</summary><button class="secondary-button" onclick={()=>advanced=JSON.stringify(draft,null,2)}>Load current draft</button><textarea aria-label="Full mapping draft JSON" rows="15" bind:value={advanced}></textarea><button class="secondary-button" onclick={()=>updateJson(advanced)}>Apply JSON to draft</button></details>
</section>
<style>
 .editor{margin:20px 0;padding:24px}.row{display:flex;align-items:end;flex-wrap:wrap;gap:12px;margin:14px 0}label{display:flex;flex-direction:column;gap:6px;margin:8px 0}input,select,textarea{background:var(--input-bg);border:1px solid var(--line);border-radius:5px;padding:9px;color:inherit}textarea{width:100%;box-sizing:border-box;font-family:monospace}fieldset{border:1px solid var(--line);margin:12px 0}.timing{font-size:13px;line-height:1.5;opacity:.8}
</style>
