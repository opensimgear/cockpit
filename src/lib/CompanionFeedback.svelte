<script lang="ts">
  import Notification from './Notification.svelte';
  import { onMount } from 'svelte';
  import type { CompanionSend } from './companions';
  import { feedbackBindingError, feedbackSourceFromKey, feedbackSourceKey, type FeedbackRouteBinding, type FeedbackRouteState } from './companion-feedback';
  let { send }: { send: CompanionSend } = $props();
  let routesState = $state.raw<FeedbackRouteState>({ instances: [], sources: [] }), selected = $state(''), bindings = $state<FeedbackRouteBinding[]>([]), dirty = $state(false), busy = $state(false), error = $state(''), notice = $state('');
  let operation=0,requests=0;
  const instance = $derived(routesState.instances.find(value => value.instanceId === selected));
  const invalid = $derived(instance ? feedbackBindingError(instance, bindings, routesState.sources) : 'Choose an assigned controller instance.');
  function choose(id: string) { selected=id;bindings=[];dirty=false;notice='';void request('inspect'); }
  async function request(action: 'inspect'|'save'|'reset'|'resync') {
    if(busy&&action!=='inspect')return;
    const token=++operation,selectedId=selected,identity=instance;
    requests++;busy=true;error='';notice='';
    try {
      const result=await send('companion_feedback_routes',{action,...(selectedId?{instanceId:selectedId}:{}),...(action==='inspect'?{}:{publicationHash:identity?.publicationHash}),...(action==='save'?{bindings:$state.snapshot(bindings)}:{})});
      if(token!==operation||selectedId!==selected)return;
      if(!result.feedbackRoutes)throw new Error('Typed feedback routing requires the updated native host.');
      routesState=result.feedbackRoutes;
      if(!routesState.instances.some(value=>value.instanceId===selected)){selected=routesState.instances[0]?.instanceId??'';dirty=false;}
      if(action==='save'){dirty=false;notice='Feedback routes saved. Source integration and input mapping keep their current state.';}
      else if(action==='reset'||action==='resync')notice='Visual reset/resync requested. No game input was sent.';
      if(!dirty)bindings=structuredClone(routesState.instances.find(value=>value.instanceId===selected)?.bindings??[]);
    }catch(value){if(token===operation)error=value instanceof Error?value.message:String(value);}finally{requests--;busy=requests>0;}
  }
  function source(outputId:string,channelId:string,value:string){bindings=bindings.filter(binding=>binding.channelId!==channelId);const selected=feedbackSourceFromKey(value);if(selected)bindings=[...bindings,{outputId,channelId,source:selected}];dirty=true;}
  function number(channelId:string,field:'scale'|'offset',value:string){bindings=bindings.map(binding=>binding.channelId===channelId?{...binding,[field]:Number(value)}:binding);dirty=true;}
  onMount(()=>{void request('inspect');return()=>{operation++;};});
</script>
<section class="feedback-routing" aria-label="Typed companion feedback">
  <div class="heading"><div><h4>Panel feedback</h4><p>Route visual state separately from controls. Requested state and host acknowledgements do not establish game confirmation. Confirmed values require a fresh supported simulator source.</p></div><button disabled={busy} onclick={()=>request('inspect')}>Refresh feedback routes</button></div>
  {#if error}<Notification message={String(error)} kind="error" />{/if}{#if notice}<Notification message={String(notice)} />{/if}
  <label>Controller instance<select value={selected} onchange={event=>choose(event.currentTarget.value)} ><option value="">Choose an assigned instance</option>{#each routesState.instances as item}<option value={item.instanceId}>{item.instanceId}</option>{/each}</select></label>
  {#if instance}
    <p class="revision">Immutable revision <code>{instance.publicationHash}</code> · {instance.pinned?'Pinned':'Save explicit bindings for this revision'}{dirty?' · Unsaved edits':''}</p>
    {#if instance.error}<Notification message={String(instance.error)} kind="error" />{/if}
    {#if !routesState.sources.length}<p>No logical telemetry sources are saved. Add them under Simulator integrations, validate and save, then start that integration explicitly to obtain fresh values.</p>{/if}
    {#if !instance.outputs.length}<p>This publication has no feedback widgets. Add indicators, text, numeric displays or gauges in the designer and publish a new revision.</p>{/if}
    {#each instance.outputs as output}
      {@const binding=bindings.find(value=>value.channelId===output.channelId)}
      <div class="output"><div><strong>{output.outputId}</strong><span>{output.kind} · {output.unit}{output.range?` · ${output.range.min}…${output.range.max}`:''}</span><code>{output.channelId}</code></div>
        <label>Visual source<select value={binding?feedbackSourceKey(binding.source):''} disabled={busy} onchange={event=>source(output.outputId,output.channelId,event.currentTarget.value)}><option value="">Unknown / unbound</option>
          {#each ['requested','host-accepted','host-submitted'] as stage}<optgroup label={stage}>{#each instance.controls??[] as control}<option value={`${stage}:${control.id}`}>{control.label} · {control.id}</option>{/each}</optgroup>{/each}
          <optgroup label="Logical telemetry">{#each routesState.sources as source}<option value={`integration:${source.bindingId}`}>{source.label} · {source.unit} · {source.confirmed?'Simulator source':'Not game confirmed'}</option>{/each}</optgroup>
        </select></label>
        {#if binding&&output.kind==='gauge'}<label>Scale<input type="number" step="any" value={binding.scale??1} disabled={busy} oninput={event=>number(output.channelId,'scale',event.currentTarget.value)}/></label><label>Offset<input type="number" step="any" value={binding.offset??0} disabled={busy} oninput={event=>number(output.channelId,'offset',event.currentTarget.value)}/></label>{/if}
      </div>
    {/each}
    {#if invalid}<Notification message={String(invalid)} kind="error" />{/if}
    <div class="actions"><button disabled={busy||!!invalid} onclick={()=>request('save')}>Save visual routes</button><button disabled={busy||dirty||!instance.configured} onclick={()=>request('reset')}>Reset visual state</button><button disabled={busy||dirty||!instance.configured} onclick={()=>request('resync')}>Resync visuals</button></div>
    <p class="small">Reset/resync affects feedback only. It cannot arm an input session, replay controls or activate an integration. Stale, unknown and disconnected feedback stays visibly unavailable.</p>
  {/if}
</section>
<style>
.feedback-routing{border-top:1px solid #344452;margin-top:18px;padding-top:8px}.heading{display:flex;align-items:center;gap:12px;justify-content:space-between}h4{margin:12px 0 6px}p{line-height:1.5;color:#9aafbf}label{display:flex;flex-direction:column;gap:5px;font-size:12px}.output{display:flex;gap:12px;align-items:end;border-top:1px solid #344452;padding:12px 0}.output>div{display:flex;flex-direction:column;gap:4px;min-width:130px;max-width:250px;overflow-wrap:anywhere}.output span,.small,.revision{font-size:11px;color:#a7b9c5}.output input{width:85px}.actions{display:flex;gap:8px;flex-wrap:wrap}button,input,select{font:inherit;color:#dce7ed;border:1px solid #344452;border-radius:5px;background:#14212c;padding:8px}button{cursor:pointer}button:disabled{opacity:.4;cursor:default}code{overflow-wrap:anywhere}.revision{max-width:100%}@media(max-width:900px){.output,.heading{flex-wrap:wrap}.output>div{width:100%;max-width:none}.output select{max-width:100%}}
</style>
