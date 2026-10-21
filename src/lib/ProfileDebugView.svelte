<script lang="ts">
  import {onDestroy,onMount,untrack} from 'svelte';
  import ProfileLiveInspector from './ProfileLiveInspector.svelte';
  import ProfilePathActivity from './ProfilePathActivity.svelte';
  import ProfileDesktopPreview from './ProfileDesktopPreview.svelte';
  import ProfileRetiredRecovery from './ProfileRetiredRecovery.svelte';
  import ProfileRuntimeControls from './ProfileRuntimeControls.svelte';
  import GameTelemetryInspector from './GameTelemetryInspector.svelte';
  import {gatewayRequest} from './gateway';
  import {graphInventorySignature} from './profile-graph';
  import type {ProfileDraft} from './profile-editor';
  import type {DeepReadonly,ProfileDocument,ProfileLocalAssignment,ProfileResult,ProfileSummary} from './profiles';
  import type {ResourceSummary} from './profile-inventory';
  import type {CompilationSummary,CompileDiagnostic,ProfileCompilerClient} from './profile-compilation';
  import type {ProfileRuntimeController,RuntimeControlState} from './profile-runtime-controller';
  import type {ProfileInspectionController} from './profile-inspection-controller';
  import type {ProfileActivityController} from './profile-activity-controller';

  let {editor=null,profiles,inventory,inventoryRevision,compiler,runtimeState,runtime,onRuntimeState,inspection,activity,selectedId='',profileName,active=true,loadProfile,close,locate}: {
    editor?:DeepReadonly<ProfileDraft>|null; profiles:readonly DeepReadonly<ProfileSummary>[]; inventory:readonly DeepReadonly<ResourceSummary>[];
    inventoryRevision:number; compiler:Pick<ProfileCompilerClient,'compile'|'inspect'>; runtimeState:DeepReadonly<RuntimeControlState>;
    runtime?:ProfileRuntimeController;onRuntimeState?:(state:DeepReadonly<RuntimeControlState>)=>void;
    inspection:ProfileInspectionController; activity:ProfileActivityController; selectedId?:string;profileName?:string;active?:boolean;
    loadProfile:(id:string)=>Promise<ProfileResult>;close:()=>void;locate:(id:string)=>void;
  }=$props();
  let source=$state(untrack(()=>editor?'draft':runtimeState.status?.owner?.profileId??profiles[0]?.id??''));
  let saved=$state.raw<{document:ProfileDocument;assignments:ProfileLocalAssignment[]}|null>(null),loading=$state(false),error=$state(''),epoch=0;
  let preview=$state.raw<DeepReadonly<CompilationSummary>|null>(null),signature=$state(''),environment=$state(''),busy=$state(false);
  let diagnostics=$state.raw<readonly DeepReadonly<CompileDiagnostic>[]>([]),offset=$state(0),offsets=$state([0]);
  let desktop=$state.raw<unknown>(null);
  let companions=$state.raw<unknown>(null),companionDetailsOpen=$state(false),companionLoading=false;
  const debugId=$props.id();
  const sections=[{id:'telemetry',label:'Incoming telemetry'},{id:'profile',label:'Profile inspection'},{id:'runtime',label:'Runtime & recovery'}] as const;
  let section=$state<'telemetry'|'profile'|'runtime'>('telemetry'),mounted=$state(false),desktopRead=false;
  const draft=$derived(source==='draft'?editor:saved);
  const graphSignature=$derived(JSON.stringify(draft?{document:draft.document,assignments:draft.assignments}:null));
  const inventorySignature=$derived(`${inventoryRevision}/${graphInventorySignature(inventory)}`);
  const savedCurrent=$derived(source==='draft'||profiles.find(profile=>profile.id===source)?.revision===saved?.document.revision);
  const current=$derived(!!preview&&signature===graphSignature&&environment===inventorySignature&&savedCurrent);
  const owner=$derived(runtimeState.stale?null:runtimeState.status?.owner??null);
  const requirements=$derived(preview?.requirements?[preview.requirements.desktopInput?'keyboard and mouse':'',preview.requirements.virtualDevices?'virtual controllers':'',preview.requirements.visibility?'device visibility':''].filter(Boolean):[]);
  onMount(()=>{mounted=true;return()=>{mounted=false;};});
  onDestroy(()=>{epoch++;});
  $effect(()=>{if(mounted&&active&&section==='profile'&&source&&source!=='draft'&&!saved&&!loading&&!error)untrack(()=>void read());});
  $effect(()=>{
    if(!mounted||!active||section!=='runtime')return;
    const refresh=()=>{if(document.hidden)return;if(!desktopRead){desktopRead=true;void readDesktop();}if(companionDetailsOpen)void readCompanions();};
    refresh();const timer=window.setInterval(refresh,1000);document.addEventListener('visibilitychange',refresh);
    return()=>{window.clearInterval(timer);document.removeEventListener('visibilitychange',refresh);};
  });
  function sectionKey(event:KeyboardEvent){const index=sections.findIndex(item=>item.id===section);let next=index;if(event.key==='ArrowRight')next=(index+1)%sections.length;else if(event.key==='ArrowLeft')next=(index+sections.length-1)%sections.length;else if(event.key==='Home')next=0;else if(event.key==='End')next=sections.length-1;else return;event.preventDefault();section=sections[next].id;document.getElementById(`${debugId}-${section}-tab`)?.focus();}
  async function read(){const captured=++epoch;preview=null;error='';saved=null;if(source==='draft')return;loading=true;try{const result=await loadProfile(source);if(captured!==epoch)return;if(!result.document||!result.assignments)throw Error('The saved profile and its assignments are unavailable.');saved={document:result.document,assignments:result.assignments};}catch(reason){if(captured===epoch)error=reason instanceof Error?reason.message:String(reason);}finally{if(captured===epoch)loading=false;}}
  async function readDesktop(){try{desktop=(await gatewayRequest({command:'profile_desktop_status'})).desktop;}catch(reason){desktop={error:reason instanceof Error?reason.message:String(reason)};}}
  async function readCompanions(){if(companionLoading)return;companionLoading=true;try{companions=(await gatewayRequest({command:'companion_status'})).companions;}catch(reason){companions={error:reason instanceof Error?reason.message:String(reason)};}finally{companionLoading=false;}}
  async function compile(){if(!draft||busy||loading)return;if(source!=='draft'){await read();if(!draft||error)return;}const captured=++epoch,graph=JSON.parse(JSON.stringify(draft)),key=graphSignature,env=inventorySignature;busy=true;error='';try{const result=await compiler.compile(graph.document,graph.assignments);if(captured!==epoch)return;preview=result;signature=key;environment=env;diagnostics=result.diagnostics;offset=0;offsets=[0];}catch(reason){if(captured===epoch)error=reason instanceof Error?reason.message:String(reason);}finally{if(captured===epoch)busy=false;}}
  async function page(direction:'next'|'previous'){if(!preview||busy)return;const next=direction==='next'?offset+diagnostics.length:offsets[offsets.length-2];if(next===undefined)return;const captured=++epoch;busy=true;try{const result=await compiler.inspect(preview,'diagnostics',next,32);if(captured!==epoch)return;diagnostics=result.items;offset=result.offset;offsets=direction==='next'?[...offsets,offset]:offsets.slice(0,-1);}catch(reason){if(captured===epoch)error=reason instanceof Error?reason.message:String(reason);}finally{if(captured===epoch)busy=false;}}
</script>

<section class="profile-debug" aria-label="Profile debug">
  <button class="text-button back-debug" onclick={close}>← Back to {editor?'profile':'Profiles'}</button>
  <div class="page-heading"><div><p class="eyebrow">DIAGNOSTICS</p><h1>Debug</h1><p>Watch incoming game telemetry, inspect profiles, and investigate output recovery.</p></div></div>
  <div class="debug-sections" role="tablist" aria-label="Debug sections">{#each sections as item (item.id)}<button id={`${debugId}-${item.id}-tab`} role="tab" aria-selected={section===item.id} aria-controls={`${debugId}-${item.id}-panel`} tabindex={section===item.id?0:-1} onclick={()=>section=item.id} onkeydown={sectionKey}>{item.label}</button>{/each}</div>
  <div id={`${debugId}-telemetry-panel`} role="tabpanel" aria-labelledby={`${debugId}-telemetry-tab`} hidden={section!=='telemetry'}><GameTelemetryInspector active={active&&section==='telemetry'}/></div>
  <div id={`${debugId}-profile-panel`} role="tabpanel" aria-labelledby={`${debugId}-profile-tab`} hidden={section!=='profile'}>
  <section class="panel preflight" aria-label="Native graph preflight">
    <div class="debug-toolbar"><label>Profile to inspect<select aria-label="Debug profile" bind:value={source} onchange={read} disabled={busy||loading}>{#if editor}<option value="draft">Current editor draft · {editor.document.name}</option>{/if}{#each profiles as profile (profile.id)}<option value={profile.id}>{profile.name}</option>{/each}</select></label><button class="secondary-button" disabled={!draft||busy||loading} onclick={compile}>{busy?'Inspecting…':preview?'Refresh preflight':'Inspect draft preflight'}</button></div>
    <p>Preflight checks this profile and its local assignments without saving or starting it.</p>
    {#if loading}<p role="status">Loading saved profile…</p>{/if}
    {#if error}<p class="debug-error" role="alert">{error}</p>{/if}
    {#if preview}<p role="status">Readiness: <strong>{preview.readiness}</strong>{#if !current} · Profile or inventory changed; refresh preflight.{/if}</p>
      {#if preview.budgets}<p>{preview.budgets.usedSources}/16 source resources · {preview.budgets.virtualTargets}/8 virtual controllers · {preview.budgets.bindings}/512 routes · {preview.budgets.timerCapacity} timers</p>{/if}
      <ul class="diagnostics">{#each diagnostics as diagnostic,index (`${offset}/${index}`)}<li><strong>{diagnostic.code}</strong> · {diagnostic.message}{#if current&&draft?.document.id===editor?.document.id&&(diagnostic.location.nodeId||diagnostic.location.resourceId)} <button class="text-button" onclick={()=>locate(diagnostic.location.nodeId??diagnostic.location.resourceId!)}>Show block in editor</button>{/if}</li>{:else}<li>No diagnostics in this captured page.</li>{/each}</ul>
      {#if preview.sectionTotals.diagnostics>diagnostics.length}<div class="debug-toolbar"><button class="text-button" disabled={busy||offset===0} onclick={()=>page('previous')}>Previous diagnostics</button><span>{offset+1}–{offset+diagnostics.length} / {preview.sectionTotals.diagnostics}</span><button class="text-button" disabled={busy||offset+diagnostics.length>=preview.sectionTotals.diagnostics} onclick={()=>page('next')}>Next diagnostics</button></div>{/if}
      <p aria-label="Captured output requirements">Output requirements: {requirements.join(' · ')||'none'}</p>
    {/if}
  </section>
  {#if preview&&draft?.document.desktopOutputs?.length}<ProfileDesktopPreview {preview} {compiler} {current}/>{/if}
  <ProfileLiveInspector controller={inspection} {owner} active={active&&section==='profile'} {selectedId} editorProfileId={editor?.document.id} editorRevision={editor?.document.revision} editorDirty={!!editor&&(JSON.stringify(editor.document)!==JSON.stringify(editor.baseDocument)||JSON.stringify(editor.assignments)!==JSON.stringify(editor.baseAssignments))} {profileName}/>
  <ProfilePathActivity controller={activity} {owner} active={active&&section==='profile'} {selectedId} editorProfileId={editor?.document.id}/>
  </div>
  <div id={`${debugId}-runtime-panel`} role="tabpanel" aria-labelledby={`${debugId}-runtime-tab`} hidden={section!=='runtime'}>
  {#if runtime}<ProfileRuntimeControls controller={runtime} {profiles} active={active&&section==='runtime'} onState={onRuntimeState} manageActivationProgress={false}/>{/if}
  <details class="panel native-details"><summary>Native runtime and desktop output details</summary><button class="text-button" onclick={readDesktop}>Refresh desktop details</button><pre>{JSON.stringify({profile:runtimeState.status,desktop},null,2)}</pre></details>
  <details class="panel native-details" ontoggle={event=>{companionDetailsOpen=event.currentTarget.open;if(companionDetailsOpen)void readCompanions();}}><summary>Companion loading and image cache details</summary><button class="text-button" onclick={readCompanions}>Refresh companion details</button><pre>{JSON.stringify(companions,null,2)}</pre></details>
  <ProfileRetiredRecovery active={active&&section==='runtime'}/>
  </div>
</section>

<style>
  .back-debug{font-size:11px;margin-bottom:20px;padding-left:0}.page-heading{margin-bottom:24px}h1{font-size:26px;margin:8px 0 10px}.eyebrow{font-size:8px;letter-spacing:1.5px}.page-heading p,.preflight p{font-size:11px;color:var(--muted);line-height:1.8}.debug-sections{display:flex;gap:6px;border-bottom:1px solid var(--line);padding-bottom:10px;margin-bottom:22px;overflow:auto}.debug-sections button{background:none;border:1px solid transparent;border-radius:5px;padding:9px 13px;color:var(--muted);white-space:nowrap;cursor:pointer}.debug-sections button[aria-selected=true]{border-color:var(--line);background:var(--panel);color:var(--text)}[role=tabpanel][hidden]{display:none}.preflight,.native-details{padding:20px;margin-bottom:20px}.debug-toolbar{display:flex;align-items:end;gap:14px;flex-wrap:wrap}.debug-toolbar label{display:grid;gap:8px;font-size:10px;color:var(--muted)}select{min-width:220px;max-width:100%;font-size:11px}.profile-debug button{font-size:10px}.debug-error{color:#d3a078!important}.diagnostics{font-size:11px;line-height:1.8;padding-left:20px}.diagnostics li{margin:10px 0}.native-details summary{font-size:12px;cursor:pointer}.native-details button{margin-top:16px}pre{max-height:420px;overflow:auto;font-size:10px;line-height:1.7;white-space:pre-wrap;overflow-wrap:anywhere}
</style>
