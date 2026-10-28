<script lang="ts">
  import {tick} from 'svelte';
  import {desktopBlockLabel,desktopInputLabel,desktopKeycap} from './profile-desktop-blocks';
  import {desktopActionValid,type DesktopAction,type ProfileDesktopOutput,type ProfileDesktopScope} from './profile-desktop';
  import type {DeepReadonly} from './profiles';
  let {output,change}:{output:DeepReadonly<ProfileDesktopOutput>;change:(output:ProfileDesktopOutput)=>void|boolean}=$props();
  let selectedId=$state(''),error=$state('');
  const title=$derived(desktopBlockLabel(output));
  const selected=$derived(output.actions.find(item=>item.portId===selectedId));
  const groups=$derived([
    {name:'Keys',items:output.actions.filter(({action})=>'key' in action||action.kind==='chord')},
    {name:'Buttons',items:output.actions.filter(({action})=>'button' in action)},
    {name:'Pointer',items:output.actions.filter(({action})=>action.kind==='pointer')},
    {name:'Scroll',items:output.actions.filter(({action})=>action.kind==='wheel')},
  ].filter(group=>group.items.length));
  function update(patch:Partial<ProfileDesktopOutput>,restore?:()=>void){
    try{if(change({...JSON.parse(JSON.stringify(output)),...patch})===false)throw Error('Disconnect this input before changing its signal type.');error='';}
    catch(reason){error=reason instanceof Error?reason.message:String(reason);}
    if(restore)void tick().then(restore);
  }
  function field(key:string,value:unknown,restore?:()=>void){
    if(!selected)return;
    const action={...selected.action,[key]:value} as DesktopAction;
    if(!desktopActionValid(action)){error='Choose a value within the displayed limits.';if(restore)void tick().then(restore);return;}
    update({actions:output.actions.map(item=>item.portId===selectedId?{...item,action}:JSON.parse(JSON.stringify(item)))},restore);
  }
  function numeric(key:string,input:HTMLInputElement,magnitude=false){
    if(!selected)return;
    const previous=Number((selected.action as unknown as Record<string,unknown>)[key]);
    const value=input.value.trim()?Number(input.value):NaN;
    field(key,magnitude?Math.sign(previous)*value:value,()=>{
      const saved=Number((selected!.action as unknown as Record<string,unknown>)[key]);
      input.value=String(magnitude?Math.abs(saved):saved);
    });
  }
  function mode(kind:DesktopAction['kind'],restore?:()=>void){
    const current=selected?.action;if(!current)return;
    if(!('button' in current)||kind!=='mouse-click'&&kind!=='mouse-hold')return;
    const button=current.button;
    const action:DesktopAction=kind==='mouse-click'?{kind,button,downMs:30,upMs:30}:{kind,button};
    update({actions:output.actions.map(item=>item.portId===selectedId?{...item,action}:JSON.parse(JSON.stringify(item)))},restore);
  }
  function scope(kind:ProfileDesktopScope['kind']){update({scope:{kind} as ProfileDesktopScope});}
</script>
<section class="desktop-editor" aria-label={`${title} editor`}>
  <label>Label<input maxlength="80" value={output.name} onchange={event=>update({name:event.currentTarget.value})}/></label>
  <label class="check"><input type="checkbox" checked={output.required} onchange={event=>update({required:event.currentTarget.checked})}/> Required</label>
  <label>Scope<select aria-label={`${title} scope`} value={output.scope?.kind??'application'} onchange={event=>scope(event.currentTarget.value as ProfileDesktopScope['kind'])}>
    {#if !output.scope||output.scope.kind==='application'}<option value="application" disabled>Saved application</option>{/if}
    <option value="selected-game">Context scoped</option><option value="desktop-wide">Global</option>
  </select></label>
  <p class="scope-hint">{output.scope?.kind==='desktop-wide'?'Active everywhere.':'Active while the game is in front.'}</p>
  <section class="inputs" aria-label={`${title} inputs`}>
    <div class="heading"><h4>Inputs</h4><span>{output.actions.length}</span></div>
    {#each groups as group (group.name)}
      <div class="input-group"><h5>{group.name}</h5><div class="input-grid" class:wide={group.name==='Pointer'||group.name==='Scroll'}>
        {#each group.items as item (item.portId)}<button type="button" class="input-tile" class:selected={selectedId===item.portId} aria-pressed={selectedId===item.portId} aria-label={`Configure ${desktopInputLabel(item.action)}`} title={desktopInputLabel(item.action)} onclick={()=>selectedId=selectedId===item.portId?'':item.portId}>{desktopKeycap(item.action)}</button>{/each}
      </div></div>
    {/each}
    {#if selected&&('button' in selected.action||selected.action.kind==='pointer'||selected.action.kind==='wheel')}
      {@const action=selected.action}
      <div class="input-settings" aria-label={`${desktopInputLabel(action)} settings`}>
        <div class="settings-heading"><strong>{desktopInputLabel(action)}</strong></div>
        {#if 'button' in action}<label>Behavior<select aria-label="Behavior" value={action.kind} onchange={event=>{const input=event.currentTarget;mode(input.value as DesktopAction['kind'],()=>input.value=selected?.action.kind??'');}}>
          <option value="mouse-hold">Hold</option><option value="mouse-click">Click</option>
        </select></label>{/if}
        {#if 'downMs' in action}<div class="pair"><label>Down (ms)<input type="number" min="1" max="60000" value={action.downMs} onchange={event=>numeric('downMs',event.currentTarget)}/></label><label>Up (ms)<input type="number" min="1" max="60000" value={action.upMs} onchange={event=>numeric('upMs',event.currentTarget)}/></label></div>{/if}
        {#if action.kind==='pointer'}<div class="pair"><label>Speed (px/s)<input type="number" min="1" max="20000" value={action.speed} onchange={event=>numeric('speed',event.currentTarget)}/></label><label>Curve<input type="number" min="0.1" max="5" step="0.1" value={action.curve} onchange={event=>numeric('curve',event.currentTarget)}/></label></div>{/if}
        {#if action.kind==='wheel'}<label>Steps<input type="number" min="1" max="120" value={Math.abs(action.horizontal||action.vertical)} onchange={event=>numeric(action.horizontal?'horizontal':'vertical',event.currentTarget,true)}/></label>{/if}
      </div>
    {/if}
  </section>
  {#if error}<p class="error" role="alert">{error}</p>{/if}
</section>
<style>
  .desktop-editor{min-width:0}label{display:grid;gap:7px;margin:12px 0;color:var(--muted);font-size:var(--ui-text-size)}input:not([type="checkbox"]),select{width:100%;min-width:0;min-height:var(--ui-control-height);box-sizing:border-box;background:var(--input-bg);border:1px solid var(--line);border-radius:5px;padding:8px;color:var(--text);font-size:var(--ui-text-size)}.check{display:flex;align-items:center;gap:8px}p{font-size:var(--ui-text-size);color:var(--muted);line-height:var(--ui-line-height)}.scope-hint{margin:0}.inputs{margin-top:18px;padding-top:16px;border-top:1px solid var(--line)}.heading,.settings-heading{display:flex;align-items:center;justify-content:space-between;gap:12px}h4{margin:0;font-size:var(--ui-heading-size);font-weight:570;color:var(--soft)}.heading span{font-size:var(--ui-text-size);color:var(--muted)}h5{margin:12px 0 8px;font-size:var(--ui-text-size);font-weight:500;color:var(--muted)}.input-grid{display:grid;grid-template-columns:repeat(auto-fill,minmax(48px,1fr));gap:5px}.input-grid.wide{grid-template-columns:repeat(2,minmax(0,1fr))}.input-tile{min-width:0;height:32px;padding:5px 3px;border:1px solid var(--line);border-radius:4px;background:var(--input-bg);color:var(--muted);font-size:var(--ui-text-size);white-space:nowrap;overflow:hidden;text-overflow:ellipsis}.input-tile:hover,.input-tile.selected{border-color:var(--accent);color:var(--accent);background:color-mix(in srgb,var(--accent) 8%,var(--input-bg))}.input-tile:focus-visible{outline:2px solid var(--accent);outline-offset:2px}.input-settings{margin-top:14px;padding:12px;border:1px solid var(--line);border-radius:5px;background:var(--surface-raised)}.settings-heading strong{font-size:var(--ui-text-size);font-weight:570;color:var(--soft)}.input-settings label:last-child{margin-bottom:0}.pair{display:grid;grid-template-columns:repeat(2,minmax(0,1fr));gap:8px}.error{color:var(--danger,#e67979)}
</style>
