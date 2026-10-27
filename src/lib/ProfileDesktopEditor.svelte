<script lang="ts">
  import{untrack,onDestroy,tick}from'svelte';
  import { desktopActionLabel,desktopActionValid,desktopActionPort,DESKTOP_KEYS,DESKTOP_KEY_CODES,type DesktopAction,type ProfileDesktopOutput } from './profile-desktop';
  import {nativeProfileStatus}from'./profile-runtime';
  import type {DeepReadonly} from './profiles';
  import {gatewayRequest}from'./gateway';
  let{output,change}:{output:DeepReadonly<ProfileDesktopOutput>;change:(output:ProfileDesktopOutput)=>void}=$props();
  let draft=$state.raw<ProfileDesktopOutput>(JSON.parse(JSON.stringify(untrack(()=>output)))),error=$state(''),capture=$state(''),captureBusy=$state(false),captureEpoch=0;
  let picker=$state<HTMLInputElement|null>(null);
  // Inventory polling can replace equal parent data. Keep local edits and the
  // active picker until the authored endpoint actually changes.
  const authoredOutput=$derived(JSON.stringify(output));
  $effect(()=>{draft=JSON.parse(authoredOutput);captureEpoch++;capture='';captureBusy=false;error='';});
  onDestroy(()=>{captureEpoch++;});
  const valid=$derived(draft.actions.every(definition=>desktopActionValid(definition.action))&&draft.actions.length>0);
  const kinds:DesktopAction['kind'][]=['key-hold','key-tap','chord','mouse-hold','mouse-click','wheel','pointer','repeat'];
  function initial(kind:DesktopAction['kind']):DesktopAction{switch(kind){case'key-hold':return{kind,key:44};case'key-tap':return{kind,key:44,downMs:30,upMs:30};case'chord':return{kind,keys:[224,6]};case'mouse-hold':return{kind,button:'left'};case'mouse-click':return{kind,button:'left',downMs:30,upMs:30};case'wheel':return{kind,horizontal:0,vertical:1};case'pointer':return{kind,axis:'x',speed:900,curve:1};case'repeat':return{kind,key:44,delayMs:400,intervalMs:80};}}
  function edit(index:number,action:DesktopAction){draft={...draft,actions:draft.actions.map((definition,i)=>i===index?{...definition,action}:definition)};}
  function field(index:number,key:string,value:unknown){edit(index,{...draft.actions[index].action,[key]:value} as DesktopAction);}
  function add(){let n=1;while(draft.actions.some(action=>action.portId===`action-${n}`))n++;const portId=`action-${n}`,action=initial('key-hold');draft={...draft,ports:[...draft.ports,desktopActionPort(portId,portId,action)],actions:[...draft.actions,{portId,action}]};}
  async function beginCapture(portId:string){const epoch=++captureEpoch;capture='';captureBusy=true;error='';try{const status=nativeProfileStatus((await gatewayRequest({command:'profile_stop'})).profiles?.profileSession);if(status.state!=='stopped'||status.owner||status.appliedSnapshot||status.fault)throw Error('Deactivate and recover all profile outputs before capturing a physical key.');if(epoch===captureEpoch){capture=portId;await tick();if(epoch===captureEpoch&&capture===portId)picker?.focus();}}catch(reason){if(epoch===captureEpoch)error=reason instanceof Error?reason.message:String(reason);}finally{if(epoch===captureEpoch)captureBusy=false;}}
  function keyCapture(event:KeyboardEvent,index:number){if(capture!==draft.actions[index].portId)return;event.preventDefault();event.stopPropagation();const usage=Object.hasOwn(DESKTOP_KEY_CODES,event.code)?DESKTOP_KEY_CODES[event.code]:undefined;
    if(usage===undefined){error='This physical key has no supported USB HID mapping. Choose a supported key.';return;}
    const action=draft.actions[index].action;if('key'in action)field(index,'key',usage);else if(action.kind==='chord')field(index,'keys',[...new Set([...action.keys,usage])]);capture='';
  }
  function save(){error='';try{change(draft);}catch(reason){error=reason instanceof Error?reason.message:String(reason);}}
  function scope(kind:'selected-game'|'desktop-wide'){draft={...draft,scope:{kind}};save();}
</script>
<section class="desktop-editor" aria-label="Keyboard and mouse action editor">
  <h4>Keyboard and mouse actions</h4>
  <p>Exact physical key and mouse inputs for this graph. Each input needs an explicit route.</p>
  <label>Endpoint label<input value={draft.name} maxlength="80" oninput={event=>draft={...draft,name:event.currentTarget.value}}/></label>
  <label>Output scope<select aria-label="Keyboard and mouse scope" value={draft.scope?.kind??'legacy'} onchange={event=>scope(event.currentTarget.value as 'selected-game'|'desktop-wide')}>
    {#if !draft.scope}<option value="legacy" disabled>Legacy scope · choose a scope</option>{:else if draft.scope.kind==='application'}<option value="application" disabled>Saved application (legacy)</option>{/if}
    <option value="selected-game">Context scoped</option><option value="desktop-wide">Global</option>
  </select></label>
  {#if draft.scope?.kind==='application'}<p>This block retains its saved application scope. Choose Context scoped or Global to update it.</p>{:else if draft.scope?.kind==='selected-game'}<p>Bindings work only while this profile's game is in front. They wait while the game is closed and resume with neutral controls when it returns.</p>{:else if draft.scope?.kind==='desktop-wide'}<p>Bindings work everywhere while this profile is active.</p>{/if}
  <p>Scope is saved with this keyboard and mouse block. Save the profile to use the change.</p>
  <label class="check"><input type="checkbox" checked={draft.required} onchange={event=>draft={...draft,required:event.currentTarget.checked}}/>Required desktop backend</label>
  {#each draft.actions as definition,index (definition.portId)}
    {@const action=definition.action}
    <fieldset><legend>{definition.portId}</legend><p>{desktopActionLabel(action)}</p>
      <label>Action<select value={action.kind} onchange={event=>edit(index,initial(event.currentTarget.value as DesktopAction['kind']))}>{#each kinds as kind}<option value={kind}>{kind}</option>{/each}</select></label>
      {#if 'key'in action}<label>Physical key<select aria-label="Physical key" value={action.key} onchange={event=>field(index,'key',Number(event.currentTarget.value))}>{#each DESKTOP_KEYS as key (key.usage)}<option value={key.usage}>{key.label} · HID {key.usage}</option>{/each}</select></label>{/if}
      {#if action.kind==='chord'}<label>Ordered physical HID usages (comma separated)<input value={action.keys.join(', ')} onchange={event=>field(index,'keys',event.currentTarget.value.split(',').map(value=>Number(value.trim())))}/></label><p>Modifiers press before ordinary keys and release after them. Shared owners retain their holds.</p>{/if}
      {#if 'key'in action||action.kind==='chord'}<button type="button" class="secondary-button" disabled={captureBusy} onclick={()=>beginCapture(definition.portId)}>{captureBusy?'Deactivating profile…':capture===definition.portId?'Picker ready below':'Capture a physical key'}</button>{#if capture===definition.portId}<input bind:this={picker} class="key-picker" aria-label="Physical key capture" placeholder="Focus here and press a key" onkeydown={event=>keyCapture(event,index)} onblur={()=>capture=''}/><p>No profile active. Activate the profile after editing.</p>{/if}{/if}
      {#if 'button'in action}<label>Mouse button<select value={action.button} onchange={event=>field(index,'button',event.currentTarget.value)}>{#each ['left','right','middle','x1','x2'] as button}<option value={button}>{button}</option>{/each}</select></label>{/if}
      {#if 'downMs'in action}<div class="pair"><label>Down dwell (ms)<input type="number" min="1" max="60000" value={action.downMs} onchange={event=>field(index,'downMs',Number(event.currentTarget.value))}/></label><label>Up dwell (ms)<input type="number" min="1" max="60000" value={action.upMs} onchange={event=>field(index,'upMs',Number(event.currentTarget.value))}/></label></div><p>Ordered taps use a bounded queue. Shared holds never release to manufacture another tap edge.</p>{/if}
      {#if action.kind==='wheel'}<div class="pair"><label>Horizontal steps<input type="number" min="-120" max="120" value={action.horizontal} onchange={event=>field(index,'horizontal',Number(event.currentTarget.value))}/></label><label>Vertical steps<input type="number" min="-120" max="120" value={action.vertical} onchange={event=>field(index,'vertical',Number(event.currentTarget.value))}/></label></div>{/if}
      {#if action.kind==='pointer'}<label>Pointer axis<select value={action.axis} onchange={event=>field(index,'axis',event.currentTarget.value)}><option value="x">Horizontal X</option><option value="y">Vertical Y</option></select></label><div class="pair"><label>Maximum speed (px/s)<input type="number" min="1" max="20000" value={action.speed} onchange={event=>field(index,'speed',Number(event.currentTarget.value))}/></label><label>Velocity exponent<input type="number" min="0.1" max="5" step="0.1" value={action.curve} onchange={event=>field(index,'curve',Number(event.currentTarget.value))}/></label></div><p>Route normalized −1…1 displacement through visible dead zone and curve blocks. Motion integrates monotonic time; OS pointer settings still affect distance.</p>{/if}
      {#if action.kind==='repeat'}<div class="pair"><label>Repeat delay (ms)<input type="number" min="1" max="60000" value={action.delayMs} onchange={event=>field(index,'delayMs',Number(event.currentTarget.value))}/></label><label>Repeat interval (ms)<input type="number" min="8" max="60000" value={action.intervalMs} onchange={event=>field(index,'intervalMs',Number(event.currentTarget.value))}/></label></div>{/if}
      <button class="text-button" disabled={draft.actions.length<=1} onclick={()=>draft={...draft,actions:draft.actions.filter((_,i)=>i!==index),ports:draft.ports.filter(port=>port.id!==definition.portId)}}>Remove action</button>
    </fieldset>
  {/each}
  <button class="secondary-button" disabled={draft.actions.length>=128} onclick={add}>Add exact action port</button>
  {#if error}<p role="alert">{error}</p>{/if}{#if !valid}<p role="status">Choose supported keys, bounded timings and valid action values before updating.</p>{/if}
  <button class="primary-button" disabled={!valid} onclick={save}>Update desktop actions</button>
  <p>Save updates an active profile. Saving an inactive profile sends no input; activate it when ready. Import and preview send no input. Native controller bindings may also react to the original control; visibility is a separate capability.</p>
</section>
<style>
 .desktop-editor{border-top:1px solid var(--line);padding-top:14px;margin-top:16px;min-width:0}h4{font-size:var(--ui-heading-size)}p{font-size:var(--ui-text-size);color:var(--muted);line-height:var(--ui-line-height)}label{display:grid;gap:6px;font-size:var(--ui-text-size);margin:12px 0}fieldset{border:1px solid var(--line);margin:16px 0;padding:10px;min-width:0}legend{font-size:var(--ui-text-size)}.check{display:flex;align-items:center;gap:7px}.pair{display:grid;grid-template-columns:repeat(2,minmax(0,1fr));gap:8px}input:not([type="checkbox"]),select{width:100%;min-width:0;box-sizing:border-box;background:var(--input-bg);color:var(--text);padding:8px;border:1px solid var(--line);border-radius:5px;font-size:var(--ui-text-size);min-height:var(--ui-control-height)}button{font-size:var(--ui-action-size);margin:6px 6px 6px 0}.key-picker{margin-top:10px}
</style>
