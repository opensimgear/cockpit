<script lang="ts">
  import { onMount } from 'svelte';
  import { companionInstanceLabel, companionSessionLabel, companionStartAllowed, companionTargetBindings, createCompanionOperationGuard, targetLayoutLabel } from './companion-mappings';
  import type { CompanionMappingSource, CompanionRoute, CompanionRouteAction, MappingResult, MappingSend, RecordedMapping } from './mappings';
  interface Props { send: MappingSend; sources: CompanionMappingSource[] }
  let {send,sources}:Props=$props();
  let selected=$state(''), route=$state<CompanionRoute|null>(null), output=$state<MappingResult|null>(null);
  let slots=$state<Record<string,number>>({}), axes=$state<Record<string,number[]>>({}), dirty=$state(false);
  let error=$state(''), notice=$state(''), records=$state<RecordedMapping[]>([]), pending=$state<Record<string,number>>({}), inspectingOutputs=$state(false);
  const operations=createCompanionOperationGuard();
  let disposed=false,cursor='0',recordingToken='';
  const instance=$derived(sources.find(source=>source.id===selected));
  const busy=$derived((pending[selected]??0)>0);
  const draft=$derived(route?companionTargetBindings(route.profile,slots,output,axes):{bindings:[],issues:[]});
  const templateBackend=$derived(['uinput','macos-hid'].includes(output?.backend??''));
  async function request(instanceId:string,action:CompanionRouteAction,extra:Record<string,unknown>={}):Promise<CompanionRoute>{
    const result=await send({command:'mapping_companion_route',companion:{action,instanceId,...extra}});
    if(!result.companionRoute||result.companionRoute.instanceId!==instanceId)throw new Error('Native companion mapping response belongs to another instance.');
    return result.companionRoute;
  }
  function apply(next:CompanionRoute,reset=false){
    const nextToken=next.session.token?JSON.stringify(next.session.token):null;
    if(nextToken&&nextToken!==recordingToken){records=[];cursor='0';recordingToken=nextToken;}
    route=next;
    if(reset||!dirty){slots=Object.fromEntries(next.bindings.map(binding=>[binding.target,binding.slot]));axes=Object.fromEntries(next.bindings.map(binding=>[binding.target,[...binding.axes]]));dirty=false;}
  }
  async function inspectOutputs(){if(inspectingOutputs)return;inspectingOutputs=true;try{output=await send({command:'mapping_virtual_inspect'});}catch(value){error=value instanceof Error?value.message:String(value);}finally{inspectingOutputs=false;}}
  async function choose(value:string){selected=value;operations.select(value);route=null;records=[];cursor='0';recordingToken='';dirty=false;slots={};axes={};error='';notice='';await refresh();if(!output)void inspectOutputs();}
  async function refresh(){
    if(disposed||busy||!selected)return;const instanceId=selected,epoch=operations.beginPoll(instanceId);if(!epoch)return;
    try{
      const next=await request(instanceId,'inspect');if(disposed||!operations.current(epoch)||instanceId!==selected)return;apply(next);
      for(let page=0;page<4;page++){
        const recording=await request(instanceId,'recording',{after:cursor});if(disposed||!operations.current(epoch)||instanceId!==selected)return;
        if(recording.gap)notice='Older reports expired. This view shows bounded native mapping records, not game confirmation.';
        const updates=recording.records??[];if(updates.length)records=[...records,...updates].slice(-64);
        if(typeof recording.next==='string')cursor=recording.next;
        if(updates.length<32)break;
      }
    }catch(value){if(!disposed&&operations.current(epoch))error=value instanceof Error?value.message:String(value);}finally{operations.endPoll(epoch);}
  }
  async function act(action:CompanionRouteAction,extra:Record<string,unknown>={}){
    const instanceId=selected;if(!instanceId)return;const epoch=operations.begin(instanceId);
    pending={...pending,[instanceId]:(pending[instanceId]??0)+1};error='';notice='';
    try{
      const next=await request(instanceId,action,extra);if(disposed||!operations.current(epoch)||instanceId!==selected)return;
      apply(next,action==='save'||action==='reconcile');
      if(action==='start'){records=[];cursor='0';notice=next.session.running&&!next.session.starting&&!next.session.busy?(extra.mode==='live'?'Native live session started. Game confirmation remains separate.':'Recording session started. No virtual output was requested.'):'Start requested; waiting for native preflight, acquisition and baseline status. Game confirmation is not claimed.';}
      else if(action==='stop')notice='Stop requested for this instance. Wait for native cleanup status; other instances and the ordinary mapping keep their own sessions.';
      else if(action==='recover')notice='Recovery requested for this instance. Native status determines whether cleanup has finished; Start remains explicit.';
      else notice='Instance assignment and immutable publication saved. No output was activated.';
    }catch(value){if(!disposed&&operations.current(epoch))error=value instanceof Error?value.message:String(value);}finally{pending={...pending,[instanceId]:Math.max(0,(pending[instanceId]??1)-1)};}
    await refresh();
  }
  function chooseSlot(target:string,slot:number){slots={...slots,[target]:slot};const capabilities=output?.slots?.find(value=>value.id===slot)?.axes;axes={...axes,[target]:capabilities?.map(axis=>axis.usage)??route?.profile.targets.find(value=>value.id===target)?.layout.axes.map((_,index)=>0x30+index)??[]};dirty=true;}
  function chooseAxis(target:string,index:number,usage:number){const selectedAxes=[...(axes[target]??[])];selectedAxes[index]=usage;axes={...axes,[target]:selectedAxes};dirty=true;}
  function axisOptions(target:string){return templateBackend?Array.from({length:output?.limits?.axes??8},(_,index)=>0x30+index):output?.slots?.find(slot=>slot.id===slots[target])?.axes.map(axis=>axis.usage)??[];}
  onMount(()=>{disposed=false;const timer=setInterval(()=>{if(!selected&&sources[0])void choose(sources[0].id);else void refresh();},500);return()=>{disposed=true;operations.dispose();clearInterval(timer);};});
</script>
<section class="panel companion-routing" aria-label="Companion controller mappings">
  <div class="routing-heading"><div><div class="eyebrow">INDEPENDENT PANEL INSTANCES</div><h2>Companion controller mappings</h2></div><button class="secondary-button" disabled={inspectingOutputs} onclick={inspectOutputs}>{inspectingOutputs?'Inspecting...':'Inspect native targets'}</button></div>
  <p>Assign each retained panel instance to its authored logical targets. Identical panels keep separate identities, sources and mapping sessions. Saving, pairing and reconnecting never start game output.</p>
  <label>Controller instance<select value={selected} onchange={event=>choose(event.currentTarget.value)}><option value="">Choose a companion instance</option>{#each sources as source}<option value={source.id}>{companionInstanceLabel(source)}{source.online?'':' (offline)'}</option>{/each}</select></label>
  {#if error}<p class="notice amber-notice" role="alert">{error}</p>{/if}{#if notice}<p class="notice" role="status">{notice}</p>{/if}
  {#if !sources.length}<p class="routing-muted">Pair and assign a panel in Companion host. Source descriptors are retained for explicit local mapping after a session has connected.</p>{/if}
  {#if route}
    <div class="source-detail"><strong>{instance?.name??'Retained companion'}</strong><code>{route.instanceId}</code><span>{instance?.online?instance.baselineReady?'Fresh source baseline available':'Waiting for source baseline':'Source offline'}</span><span>{companionSessionLabel(route.session)}</span></div>
    <details><summary>Immutable publication and role assignment</summary><p>Publication <code>{route.publicationHash}</code></p><p>Saved revision <code>{route.expectedPublicationHash??'Not assigned yet'}</code></p><p>Capability fingerprint <code>{route.capabilityFingerprint}</code></p><table><thead><tr><th>Companion source role</th><th>Retained instance</th><th>Controls</th></tr></thead><tbody>{#each route.profile.roles as role}<tr><td>{role.name} · {role.id}</td><td><code>{route.instanceId}</code> · {role.origin}</td><td>{role.controls.length}</td></tr>{/each}</tbody></table><p>Source roles, target indices, ranges and each axis fallback come from the published document. Changing the publication requires explicit reconciliation.</p></details>
    <h3>Exact logical targets</h3>
    {#each route.profile.targets as target}<fieldset disabled={busy||route.session.running||route.session.starting||route.session.busy||!route.session.cleanupComplete||route.session.recoveryRequired}><legend>{target.name}</legend><p>{targetLayoutLabel(target.layout)}</p><label>Local output identity<select value={slots[target.id]??''} onchange={event=>chooseSlot(target.id,Number(event.currentTarget.value))}><option value="">Choose an identity</option>{#if templateBackend}{#each Array.from({length:16},(_,index)=>index+1) as slot}<option value={slot}>Controller {slot}</option>{/each}{:else}{#each output?.slots??[] as slot}<option value={slot.id}>Slot {slot.id} · {slot.status} · {slot.buttons} buttons / {slot.axes.length} axes / {slot.continuousHats} hats</option>{/each}{/if}</select></label><div class="axis-bindings">{#each target.layout.axes as axis,index}<label>Axis {index+1} → native usage<select value={axes[target.id]?.[index]??''} onchange={event=>chooseAxis(target.id,index,Number(event.currentTarget.value))}><option value="">Choose a usage</option>{#each axisOptions(target.id) as usage}<option value={usage}>{'0x'+usage.toString(16).toUpperCase()}</option>{/each}</select><small>{axis.range} · fallback {axis.fallback}</small></label>{/each}</div></fieldset>{/each}
    {#if dirty}<p class="notice" role="status">Target edits are unsaved. Start is disabled until the exact assignment is saved and native preflight passes.</p>{/if}
    {#each draft.issues as issue}<p class="routing-muted">{issue}</p>{/each}
    <div class="routing-actions"><button class="secondary-button" disabled={busy||route.session.running||route.session.starting||route.session.busy||!route.session.cleanupComplete||route.session.recoveryRequired} onclick={()=>act('save',{publicationHash:route?.publicationHash,bindings:[]})}>Save for recording only</button><button class="secondary-button" disabled={busy||route.session.running||route.session.starting||route.session.busy||!route.session.cleanupComplete||route.session.recoveryRequired||draft.issues.length>0} onclick={()=>act('save',{publicationHash:route?.publicationHash,bindings:draft.bindings})}>Save exact output assignments</button><button class="secondary-button" disabled={busy||route.session.running||route.session.starting||route.session.busy||!route.session.cleanupComplete||route.session.recoveryRequired} onclick={()=>act('reconcile',{publicationHash:route?.publicationHash})}>Reconcile current publication</button></div>
    <p class="routing-muted">Recording-only assignment has no virtual target bindings. Exact output assignments preserve the full authored shape; missing or extra controls, duplicate identities and fallback changes are rejected. Reconciliation never starts output.</p>
    <h3>Native preflight</h3><ul class="preflight">{#each route.preflight.checks as check}<li class:check-passed={check.passed}><strong>{check.passed?'Ready':'Blocked'} · {check.id}</strong><span>{check.reason}</span></li>{/each}</ul>
    <div class="routing-actions"><button class="primary-button" disabled={busy||!companionStartAllowed(route,'recording',dirty,instance)} onclick={()=>act('start',{mode:'recording',publicationHash:route?.publicationHash})}>Start selected recording</button><button class="primary-button" disabled={busy||!companionStartAllowed(route,'live',dirty,instance)} onclick={()=>act('start',{mode:'live',publicationHash:route?.publicationHash})}>Start selected live output</button><button class="secondary-button" disabled={!route.session.running&&!route.session.starting&&!route.session.busy&&route.session.cleanupComplete&&!busy} onclick={()=>act('stop')}>Stop selected instance</button><button class="secondary-button" disabled={busy||route.session.running||route.session.starting||route.session.busy||!output?.qualified} onclick={()=>act('recover')}>Recover selected outputs</button></div>
    {#if route.session.fault}<p class="notice amber-notice" role="status">{route.session.fault}</p>{/if}
    <p class="routing-muted">Live output remains unavailable while its native backend is absent, disabled or unqualified. Source loss, Stop and native recovery affect this instance's owned targets. Reconnect needs a new baseline and an explicit Start. Native reports and source acceptance do not establish game confirmation.</p>
    <h3>Selected instance report log</h3><div class="routing-recording"><table><thead><tr><th>Target</th><th>Sequence / time µs</th><th>Reason</th><th>Buttons</th><th>Axes</th><th>Hats</th></tr></thead><tbody>{#each records.slice(-16).reverse() as record}<tr><td>{record.report.target}</td><td>{record.report.sequence} / {record.report.atUs}</td><td>{record.report.reason}</td><td>{record.report.state.buttons.map((value,index)=>value?index+1:null).filter(value=>value!==null).join(', ')||'Released'}</td><td>{record.report.state.axes.join(', ')||'—'}</td><td>{record.report.state.hats.map(value=>value===null?'neutral':value).join(', ')||'—'}</td></tr>{:else}<tr><td colspan="6">Save the instance assignment, Start recording, then use its panel controls. No driver is required for recording.</td></tr>{/each}</tbody></table></div>
  {:else if selected}<p class="routing-muted">Inspecting the retained publication. No mapping is activated.</p>{/if}
</section>
<style>
  .companion-routing{padding:24px;margin-bottom:20px}.routing-heading{display:flex;align-items:end;justify-content:space-between;gap:16px}.routing-heading h2{margin:7px 0}.companion-routing>p{line-height:1.6}.companion-routing label{display:grid;gap:7px;min-width:160px;max-width:100%}.companion-routing select{max-width:100%}.source-detail{display:flex;gap:12px;align-items:center;flex-wrap:wrap;border:1px solid var(--line);border-radius:8px;padding:14px;margin:18px 0}.source-detail code{overflow-wrap:anywhere}.companion-routing details{margin:16px 0}.companion-routing details code{overflow-wrap:anywhere;font-size:.8rem}.companion-routing fieldset{border:1px solid var(--line);border-radius:8px;padding:16px;margin:14px 0}.axis-bindings{display:flex;flex-wrap:wrap;gap:14px;margin-top:16px}.axis-bindings small,.routing-muted{opacity:.72;font-size:.9rem}.routing-actions{display:flex;flex-wrap:wrap;gap:10px;margin:14px 0}.preflight{list-style:none;padding:0;display:grid;gap:8px}.preflight li{border-left:3px solid #997647;padding:9px 13px;background:var(--surface-raised,#19232c);display:grid;gap:5px;font-size:.9rem}.preflight li.check-passed{border-left-color:#5d9c84}.preflight span{opacity:.8}.routing-recording{max-height:340px;overflow:auto}table{width:100%;border-collapse:collapse;text-align:left;font-size:.85rem}th,td{padding:9px;border-bottom:1px solid var(--line)}@media(max-width:700px){.routing-heading{align-items:start;flex-direction:column}.companion-routing{padding:16px}.axis-bindings label{flex:1;min-width:120px}}
</style>
