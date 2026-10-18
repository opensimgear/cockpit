<script lang="ts">
  import { onDestroy, onMount, untrack } from 'svelte';
  import Icon from './Icon.svelte';
  import ProfileRoutingEditor from './ProfileRoutingEditor.svelte';
  import ProfileRuntimeControls from './ProfileRuntimeControls.svelte';
  import ProfileDesktopControls from './ProfileDesktopControls.svelte';
  import ProfileAutomaticControls from './ProfileAutomaticControls.svelte';
  import ProfileLiveInspector from './ProfileLiveInspector.svelte';
  import ProfilePathActivity from './ProfilePathActivity.svelte';
  import ProfileLibraryTools from './ProfileLibraryTools.svelte';
  import type{ProfileSummary}from'./profiles';
  import type{ProfileLibraryClient}from'./profile-library';
  import { gatewayRequest } from './gateway';
  import { DEFAULT_PROFILE_ID, ProfilesClient, profileDocumentError, type ProfileDocument, type ProfileLocalAssignment } from './profiles';
  import { ProfileInventoryClient, type ResourceSummary } from './profile-inventory';
  import { ProfileEditorStore, ProfileNavigationGuard, addProfileResources, profileCategory, profileEditorDirty } from './profile-editor';
  import {ProfileEditorUndo}from'./profile-graph-history';
  import {ProfileCompilerClient}from'./profile-compilation';
  import {ProfileRuntimeClient,nativeProfileStatus}from'./profile-runtime';
  import {ProfileRuntimeController,type RuntimeControlState}from'./profile-runtime-controller';
  import {ProfileInspectorClient}from'./profile-inspection';
  import {ProfileInspectionController}from'./profile-inspection-controller';
  import {ProfileTopologyClient}from'./profile-activity';
  import {ProfileActivityController}from'./profile-activity-controller';
  import {ProfileLearningClient}from'./profile-learning';
  import type {DeepReadonly} from './profiles';
  import type { GameId } from './games';
  let { editorStore: suppliedStore, resourceClient: suppliedResources, runtimeClient: suppliedRuntime, libraryClient:suppliedLibrary, openGameSettings, active=true }: { editorStore?: ProfileEditorStore; resourceClient?: ProfileInventoryClient; runtimeClient?: ProfileRuntimeClient;libraryClient?:ProfileLibraryClient; openGameSettings?: (gameId: GameId, returnState?: unknown) => void; active?:boolean } = $props();
  const supplied = untrack(() => suppliedStore);
  const resources = untrack(() => suppliedResources) ?? new ProfileInventoryClient(async request => { const result=(await gatewayRequest(request)).profileResources;if(!result)throw new Error('Resource inventory is unavailable. Reconnect the native service.');return result; });
  const profiles = new ProfilesClient(async request => { const result=(await gatewayRequest(request)).profiles;if(!result)throw new Error('Profiles are unavailable. Reconnect the native service.');return result; });
  const store = supplied ?? new ProfileEditorStore(profiles,resources);
  let libraryTools=$state.raw<{askDelete:(row:DeepReadonly<ProfileSummary>)=>void;askImport:()=>void;askExport:(row:DeepReadonly<ProfileSummary>)=>Promise<void>}>(),libraryBusy=$state(false);
  let libraryContext=0;
  const compiler=new ProfileCompilerClient(async request=>{const result=(await gatewayRequest(request)).profileCompilation;if(!result)throw new Error('Native compilation is unavailable. Reconnect the service.');return result;});
  let runtimeReplyEpoch=0;
  let runtimeSelection=$state.raw<{selectedProfileId:string;selectionMode:'manual'|'automatic'}|null>(null);
  const runtime=new ProfileRuntimeController(untrack(()=>suppliedRuntime)??new ProfileRuntimeClient(async request=>{
    const epoch=++runtimeReplyEpoch,result=(await gatewayRequest(request)).profiles;
    const status=nativeProfileStatus(result?.profileSession);
    const selection=result?.session;
    if(epoch===runtimeReplyEpoch&&selection&&/^[a-f0-9]{8}-[a-f0-9]{4}-[a-f0-9]{4}-[a-f0-9]{4}-[a-f0-9]{12}$/.test(selection.selectedProfileId)&&(selection.selectionMode==='manual'||selection.selectionMode==='automatic'))runtimeSelection={selectedProfileId:selection.selectedProfileId,selectionMode:selection.selectionMode};
    return status;
  }));
  let runtimeState=$state.raw<DeepReadonly<RuntimeControlState>>(runtime.state);
  const inspectionClient=new ProfileInspectorClient(async request=>{const result=(await gatewayRequest(request)).profileInspection;if(!result)throw new Error('Native applied port inspection is unavailable. Reconnect the service.');return result;});
  const inspection=new ProfileInspectionController(inspectionClient);
  const activity=new ProfileActivityController(new ProfileTopologyClient(async request=>{const result=(await gatewayRequest(request)).profileTopology;if(!result)throw new Error('Native applied topology is unavailable. Reconnect the service.');return result;}),inspectionClient);
  const learningClient=new ProfileLearningClient(async request=>{const result=(await gatewayRequest(request)).profileLearning;if(!result)throw new Error('Native input learning is unavailable. Reconnect the service.');return result;});
  let routingSelection=$state(''),runtimeOpen=$state(false);
  const navigationGuard = new ProfileNavigationGuard(store);
  let workspace = $state.raw(store.state);
  const history=new ProfileEditorUndo(store);
  let notice = $state(''), leaveDialog = $state(false), replaceSource = $state<string|null>(null), copyId = $state(DEFAULT_PROFILE_ID);
  let inventoryOpen = $state(false), inventorySelection = $state<string[]>([]), creatingSelection = $state(false);
  const editor = $derived(workspace.editor);
  const editorIdentity=$derived(editor?.document.id??'');
  const dirty = $derived(editor ? profileEditorDirty(editor) : false);
  const error = $derived(editor ? profileDocumentError(editor.document) : '');
  const rows = $derived([...workspace.profiles].sort((a,b)=>a.isDefault?-1:b.isDefault?1:a.name.localeCompare(b.name)));
  const assignedIds = $derived(new Set(editor?.assignments.map(assignment=>assignment.sourceId)??[]));
  const available = $derived(workspace.inventory.filter(resource=>!assignedIds.has(resource.id)));
  const selectedProfileId=$derived(runtimeSelection?.selectedProfileId??workspace.session?.selectedProfileId);
  const appliedSnapshot=$derived(runtimeState.status?runtimeState.status.appliedSnapshot:workspace.session?.appliedSnapshot);
  const runtimeName = $derived(workspace.profiles.find(row=>row.id===selectedProfileId)?.name??selectedProfileId??'Not loaded');
  const appliedName = $derived(workspace.profiles.find(row=>row.id===appliedSnapshot?.profileId)?.name??appliedSnapshot?.profileId);
  const runtimeTarget=$derived(editor?{id:editor.document.id,revision:editor.document.revision,baseRevision:editor.baseRevision,dirty,readOnly:workspace.readOnly||workspace.busy||libraryBusy||!!error}:null);
  const learningIdle=$derived(!!runtimeState.status&&!runtimeState.stale&&runtimeState.status.owner===null&&!['transitioning','recovery-needed'].includes(runtimeState.status.state));
  const editorContext=$derived.by(()=>{void workspace;return `${editorIdentity}/${store.editorEpoch}`;});
  const historyStatus=$derived.by(()=>{void workspace;return history.status;});
  onMount(() => { const stop=store.subscribe(next=>workspace=next);if(workspace.view==='list')void store.refresh();if(!workspace.inventoryLoaded)void store.refreshInventory();return stop; });
  onDestroy(() => { runtimeReplyEpoch++;runtime.dispose();inspection.dispose();activity.dispose();if(!supplied)store.dispose(); });
  $effect(()=>{void editorIdentity;routingSelection='';});
  function report(operation:()=>void) { notice='';try{operation();workspace=store.state;}catch(error){notice=error instanceof Error?error.message:String(error);} }
  async function add() { notice='';if(!workspace.inventoryLoaded){await store.refreshInventory();workspace=store.state;}if(workspace.inventory.length>64){creatingSelection=true;inventorySelection=[];return;}report(()=>store.beginBlank()); }
  function beginSelected() { report(()=>store.beginBlank(workspace.inventory.filter(resource=>inventorySelection.includes(resource.id))));if(!notice){creatingSelection=false;inventorySelection=[];} }
  function showConfirmation(node:HTMLDialogElement) { node.showModal();return()=>node.close(); }
  function change(document:ProfileDocument,assignments?:ProfileLocalAssignment[],label='Edit profile graph') { report(()=>history.edit(document,assignments,label)); }
  function name(value:string) { if(!editor)return;const next=JSON.parse(JSON.stringify(editor.document)) as ProfileDocument;next.name=value;change(next,undefined,'Rename profile'); }
  function historyStep(direction:'undo'|'redo'){report(()=>{history.step(direction);});}
  export function requestNavigation(continueNavigation:()=>void):void {
    notice='';
    if(libraryBusy){notice='Wait for the current library operation before leaving.';return;}
    const result=navigationGuard.request(continueNavigation);
    leaveDialog=result==='confirm';
    if(result==='busy')notice='Wait for the current profile operation to finish before leaving.';
    if(result==='left')inventoryOpen=false;
    workspace=store.state;
  }
  function back() { requestNavigation(()=>{}); }
  function keepEditing() { navigationGuard.keepEditing();leaveDialog=false; }
  async function save(backAfter=false) { notice='';if(backAfter){await navigationGuard.save();leaveDialog=navigationGuard.awaitingChoice;}else await store.save();workspace=store.state; }
  function cancel() { if(!navigationGuard.discard())return;workspace=store.state;notice='';leaveDialog=false;inventoryOpen=false; }
  function source(id:string) { if(!editor)return;if(JSON.stringify(editor.document)!==JSON.stringify(editor.baseDocument)||JSON.stringify(editor.assignments)!==JSON.stringify(editor.baseAssignments)){replaceSource=id;return;}void replace(id); }
  async function replace(id:string) { replaceSource=null;notice='';if(id==='blank')report(()=>store.beginBlank());else{await store.beginCopy(id);workspace=store.state;} }
  function toggleResource(id:string,checked:boolean) { inventorySelection=checked?[...inventorySelection,id]:inventorySelection.filter(value=>value!==id); }
  function addSelected() { if(!editor)return;report(()=>{const next=addProfileResources(JSON.parse(JSON.stringify(editor.document)),JSON.parse(JSON.stringify(editor.assignments)),available.filter(resource=>inventorySelection.includes(resource.id)));change(next.document,next.assignments,'Add resource blocks');});if(!notice){inventorySelection=[];inventoryOpen=false;} }
  function gameSettings(gameId:GameId,returnState?:unknown) { openGameSettings?.(gameId,{editorStore:store,profileId:editor?.document.id,selection:returnState}); }
  function prepareLibrary(next:()=>void){requestNavigation(()=>{libraryContext=store.editorEpoch;next();});}
  async function libraryDeleted(){if(store.editorEpoch!==libraryContext||store.state.view!=='list')return;await store.refresh();workspace=store.state;}
  async function libraryImported(document:ProfileDocument){if(store.editorEpoch!==libraryContext||store.state.view!=='list')return;await store.refresh();workspace=store.state;await store.open(document.id);workspace=store.state;}
  const savedEditor=$derived(editor?workspace.profiles.find(row=>row.id===editor.document.id):undefined);
</script>

{#snippet runtimeSummary()}
  <div class="profile-session" aria-label="Runtime profile status"><span><strong>Runtime selection</strong> {runtimeName}</span><span>{runtimeState.status?.state??workspace.session?.state??'Loading status'}{#if runtimeState.stale&&runtimeState.status} · last captured status{/if}</span><span><strong>Applied revision</strong> {appliedSnapshot?`${appliedName} · ${appliedSnapshot.revision}`:'None'}</span><button class="secondary-button summary-stop" aria-label="Stop current profile" disabled={runtimeState.pending==='stop'} onclick={()=>runtime.stop()}><Icon name="stop" size={12}/>{runtimeState.pending==='stop'?'Stopping…':'Stop'}</button><p>Opening and saving an editor leaves runtime selection and applied revision unchanged.</p>{#if runtimeState.error||runtimeState.statusError}<p class="summary-error" role="status">{runtimeState.error||runtimeState.statusError}</p>{/if}</div>
{/snippet}
{#snippet runtimeTools()}
<details class="runtime-tools" bind:open={runtimeOpen}>
  <summary>Apply, Pause and applied runtime inspection</summary>
  <ProfileRuntimeControls controller={runtime} editor={runtimeTarget} profiles={workspace.profiles} gameIds={editor?.document.eligibility.gameIds??[]} {active} onState={next=>runtimeState=next}/>
  <ProfileDesktopControls {active} requested={!!editor?.document.desktopOutputs?.length}/>
  <ProfileLiveInspector controller={inspection} owner={runtimeState.stale?null:runtimeState.status?.owner??null} active={active&&runtimeOpen} selectedId={routingSelection} editorProfileId={editor?.document.id} editorRevision={editor?.document.revision} editorDirty={dirty} profileName={appliedName}/>
  <ProfilePathActivity controller={activity} owner={runtimeState.stale?null:runtimeState.status?.owner??null} active={active&&runtimeOpen} selectedId={routingSelection} editorProfileId={editor?.document.id}/>
</details>
{/snippet}

<section class="profiles-workspace" aria-label="Profiles">
  {#if workspace.view==='list'}
    <div class="page-heading"><div><div class="eyebrow">YOUR OPERATING SETUPS</div><h1>Profiles</h1><p>Edit Default or create an independent profile for your rig.</p></div><button class="primary-button" disabled={workspace.busy||libraryBusy||workspace.inventoryBusy||workspace.readOnly} onclick={add}><Icon name="plus" size={14}/> Add profile</button></div>
  {:else if editor}
    <button class="text-button back-profiles" disabled={workspace.busy} onclick={back}>← Back to Profiles</button>
    <div class="editor-title"><div><p class="eyebrow">{editor.baseRevision===null?'NEW PROFILE DRAFT':'PROFILE EDITOR'}</p><h1>{editor.document.name||'Untitled profile'}</h1><p>{dirty?'Unsaved changes':`Saved revision ${editor.baseRevision}`} · Editing this profile does not activate it.</p></div><div class="editor-save-actions">{#if editor.baseRevision===null}<button class="text-button" disabled={workspace.busy} onclick={cancel}>Cancel creation</button>{/if}<button class="primary-button" disabled={workspace.busy||workspace.readOnly||!dirty||!!error} onclick={()=>save()}><Icon name="save" size={14}/> {workspace.busy?'Saving…':'Save profile'}</button></div></div>
  {/if}
  {@render runtimeSummary()}
  {#if workspace.diagnostics.length}<div class="profile-diagnostics" role="status">{#each workspace.diagnostics as diagnostic,index (index)}<p>{diagnostic}</p>{/each}</div>{/if}
  {#if workspace.readOnly}<p class="workspace-error" role="status">This library is read-only. Resolve its diagnostics before making changes.</p>{/if}
  {#if notice||workspace.error}<p class="workspace-error" role="alert">{notice||workspace.error}</p>{/if}
  {#if workspace.view==='list'}
    <div class="list-tools"><span>{rows.length} {rows.length===1?'profile':'profiles'} · {workspace.inventory.length} inventory resources</span><div><button class="text-button" disabled={workspace.busy||libraryBusy||workspace.readOnly||rows.length>=32} onclick={()=>libraryTools?.askImport()}>Import profile</button><button class="text-button" disabled={workspace.busy||libraryBusy} onclick={()=>store.refresh()}>Refresh profiles</button></div></div>
    {#if creatingSelection}<section class="inventory-picker panel" aria-label="Choose resources for new profile"><h2>Choose up to 64 resource blocks</h2><p class="inventory-help">The inventory has {workspace.inventory.length} resources. This exceeds the profile's 64-block capacity; select the resources for this draft explicitly. Nothing has been saved or omitted automatically.</p><div class="inventory-choices">{#each workspace.inventory as resource (resource.id)}<label><input type="checkbox" checked={inventorySelection.includes(resource.id)} onchange={event=>toggleResource(resource.id,event.currentTarget.checked)}/><span><strong>{resource.name}</strong><small>{profileCategory(resource)} · {resource.availability}</small></span></label>{/each}</div><button class="text-button" onclick={()=>creatingSelection=false}>Cancel creation</button><button class="secondary-button" disabled={!inventorySelection.length||inventorySelection.length>64} onclick={beginSelected}>Open draft with {inventorySelection.length} selected resources</button></section>{/if}
    <ul class="profile-list" aria-label="Saved profiles">{#each rows as profile (profile.id)}<li><button class="profile-row" disabled={workspace.busy||libraryBusy} onclick={()=>store.open(profile.id)} aria-label={`Edit ${profile.name}`}><span class="profile-mark"><Icon name="network" size={22}/></span><span class="profile-copy"><strong>{profile.name}{#if profile.isDefault}<small>Built-in · editable</small>{/if}</strong><span>Saved revision {profile.revision}</span></span><span class="profile-badges">{#if selectedProfileId===profile.id}<span>Selected for runtime</span>{/if}{#if appliedSnapshot?.profileId===profile.id}<span>Applied revision {appliedSnapshot.revision}</span>{/if}</span><Icon name="chevron" size={16}/></button><div class="profile-row-actions"><button class="text-button" aria-label={`Export ${profile.name}`} disabled={workspace.busy||libraryBusy} onclick={()=>libraryTools?.askExport(profile)}>Export</button>{#if !profile.isDefault}<button class="text-button" aria-label={`Delete ${profile.name}`} disabled={workspace.busy||libraryBusy||workspace.readOnly} onclick={()=>libraryTools?.askDelete(profile)}>Delete</button>{/if}</div></li>{:else}<li class="empty-profiles">{workspace.busy?'Loading profiles…':'No profile list is available. Refresh profiles to load Default.'}</li>{/each}</ul>
    {#if workspace.inventoryError}<p class="workspace-error" role="status">{workspace.inventoryError}</p><button class="secondary-button" disabled={workspace.inventoryBusy} onclick={()=>store.refreshInventory()}>Refresh resource inventory</button>{/if}
    <ProfileAutomaticControls profiles={workspace.profiles} {active}/>
    {@render runtimeTools()}
  {:else if editor}
    <div class="library-editor-tools"><button class="text-button" disabled={workspace.busy||libraryBusy||workspace.readOnly||rows.length>=32} onclick={()=>libraryTools?.askImport()}>Import profile</button><button class="text-button" disabled={workspace.busy||libraryBusy||!savedEditor} onclick={()=>savedEditor&&libraryTools?.askExport(savedEditor)}>Export saved profile</button>{#if savedEditor&&!savedEditor.isDefault}<button class="text-button" disabled={workspace.busy||libraryBusy||workspace.readOnly} onclick={()=>libraryTools?.askDelete(savedEditor!)}>Delete saved profile</button>{/if}<span>Export reads the saved revision; unsaved edits are excluded.</span></div>
    {@render runtimeTools()}
    <div class="editor-basics panel"><label>Profile name<input class="text-input" value={editor.document.name} readonly={editor.document.id===DEFAULT_PROFILE_ID} disabled={workspace.readOnly} oninput={event=>name(event.currentTarget.value)} maxlength="80"/></label>{#if editor.baseRevision===null}<fieldset class="draft-source"><legend>Start from</legend><button class="secondary-button" disabled={workspace.busy||workspace.readOnly||!workspace.inventoryLoaded} onclick={()=>source('blank')}>Blank · seed all resources</button><label>Copy saved profile<select aria-label="Copy source profile" bind:value={copyId} disabled={workspace.busy}>{#each rows as profile (profile.id)}<option value={profile.id}>{profile.name}</option>{/each}</select></label><button class="secondary-button" disabled={workspace.busy||workspace.readOnly||!copyId} onclick={()=>source(copyId)}>Use copy</button><p>Copies retain the graph and layout with a new identity. Select devices for the copy; machine assignments are excluded.</p></fieldset>{/if}{#if error}<p class="workspace-error">{error}</p>{/if}</div>
    <div class="inventory-heading"><p>Resources remain on the canvas when they disconnect. New discoveries are added only when selected.</p><button class="secondary-button" disabled={workspace.readOnly||workspace.busy} onclick={()=>inventoryOpen=!inventoryOpen}><Icon name="plus" size={13}/> Add from inventory</button></div>
    {#if inventoryOpen}<section class="inventory-picker panel" aria-label="Add resource blocks"><div><h2>Available inventory resources</h2><button class="text-button" disabled={workspace.inventoryBusy} onclick={()=>store.refreshInventory()}>Refresh inventory</button></div>{#if workspace.inventoryError}<p class="workspace-error">{workspace.inventoryError}</p>{/if}<p class="inventory-help">{editor.document.resources.length} of 64 resource blocks used. Port catalogues remain paged; adding a block creates no routes.</p><div class="inventory-choices">{#each available as resource (resource.id)}<label><input type="checkbox" checked={inventorySelection.includes(resource.id)} onchange={event=>toggleResource(resource.id,event.currentTarget.checked)}/><span><strong>{resource.name}</strong><small>{profileCategory(resource)} · {resource.availability} · {resource.portCount} advertised ports</small></span></label>{:else}<p>All current inventory resources already have assigned blocks.</p>{/each}</div><button class="secondary-button" disabled={!inventorySelection.length||workspace.readOnly||workspace.busy} onclick={addSelected}>Add {inventorySelection.length} selected resources</button></section>{/if}
    {#key editor.document.id}<ProfileRoutingEditor document={editor.document} assignments={editor.assignments} inventory={workspace.inventory} inventoryClient={resources} compilerClient={compiler} {learningClient} {editorContext} {active} runtimeIdle={learningIdle} inventoryRevision={store.inventoryRevision} readOnly={workspace.readOnly} bind:selectedId={routingSelection} change={(draft,label)=>change(draft.document,draft.assignments,label)} openGameSettings={openGameSettings?gameSettings:undefined} undo={()=>historyStep('undo')} redo={()=>historyStep('redo')} canUndo={historyStatus.canUndo} canRedo={historyStatus.canRedo} undoLabel={historyStatus.undoLabel} redoLabel={historyStatus.redoLabel}/>{/key}
  {/if}
  <ProfileLibraryTools bind:this={libraryTools} client={suppliedLibrary??profiles} profileCount={rows.length} readOnly={workspace.readOnly} busy={workspace.busy} prepare={prepareLibrary} resolveProfile={id=>workspace.profiles.find(row=>row.id===id)} onDeleted={libraryDeleted} onImported={libraryImported} onBusy={value=>libraryBusy=value}/>
  {#if leaveDialog}<dialog class="editor-confirmation" role="alertdialog" aria-modal="true" aria-labelledby="leave-profile-title" onclose={keepEditing} {@attach showConfirmation}><h2 id="leave-profile-title">Keep your unsaved changes?</h2><p>Save this profile before leaving, or discard this draft. Runtime selection stays unchanged.</p>{#if workspace.error}<p class="workspace-error" role="alert">{workspace.error}</p>{/if}<div><button class="secondary-button" onclick={keepEditing}>Keep editing</button><button class="secondary-button" disabled={workspace.busy} onclick={cancel}>Discard and continue</button><button class="primary-button" disabled={!!error||workspace.readOnly||workspace.busy} onclick={()=>save(true)}>Save and continue</button></div></dialog>{/if}
  {#if replaceSource}<dialog class="editor-confirmation" role="alertdialog" aria-modal="true" aria-labelledby="replace-profile-title" onclose={()=>replaceSource=null} {@attach showConfirmation}><h2 id="replace-profile-title">Replace this new draft?</h2><p>Your unsaved edits will be discarded before starting from {replaceSource==='blank'?'the current inventory':'the selected copy'}.</p><div><button class="secondary-button" onclick={()=>replaceSource=null}>Keep this draft</button><button class="primary-button" onclick={()=>replace(replaceSource!)}>Replace draft</button></div></dialog>{/if}
</section>

<style>
  .profile-row-actions{display:flex;justify-content:end;gap:14px;padding:0 20px 12px;background:var(--surface)}.profile-row-actions button,.library-editor-tools button{font-size:10px}.library-editor-tools{display:flex;align-items:center;gap:16px;flex-wrap:wrap;margin:16px 0}.library-editor-tools span{font-size:9px;color:var(--muted)}
  .summary-stop{margin-left:auto;display:inline-flex;align-items:center;gap:7px;font-size:10px;}.profile-session .summary-error{color:#d3a078;}.runtime-tools{margin:20px 0;border:1px solid var(--line);border-radius:7px;padding:15px;}.runtime-tools>summary{font-size:11px;color:var(--soft);cursor:pointer;}.runtime-tools[open]>summary{margin-bottom:18px;}.profile-session>span{min-width:0;overflow-wrap:anywhere;}
  .profile-session{display:flex;gap:20px;align-items:center;flex-wrap:wrap;padding:14px 17px;border:1px solid var(--line);border-radius:7px;background:var(--surface);font-size:10px;color:var(--soft);margin-bottom:24px;}.profile-session strong{font-size:9px;color:var(--muted);font-weight:400;margin-right:7px;}.profile-session p{flex-basis:100%;font-size:9px;color:var(--muted);margin:0;line-height:1.7;}.page-heading,.editor-title{display:flex;align-items:center;justify-content:space-between;gap:20px;margin-bottom:24px;}h1{font-size:26px;letter-spacing:-.5px;margin:8px 0 10px;}h2{font-size:14px;}.page-heading p,.editor-title p{font-size:11px;color:var(--muted);line-height:1.8;}.page-heading button,.editor-save-actions button{display:inline-flex;align-items:center;gap:8px;white-space:nowrap;font-size:11px;}.eyebrow{font-size:8px!important;letter-spacing:1.5px;text-transform:uppercase;color:var(--muted);}.list-tools{display:flex;align-items:center;justify-content:space-between;font-size:10px;color:var(--muted);margin-bottom:12px;}.list-tools button{font-size:10px;}.profile-list{list-style:none;padding:0;margin:0 0 22px;border:1px solid var(--line);border-radius:8px;overflow:hidden;}.profile-list li+li{border-top:1px solid var(--line);}.profile-row{width:100%;display:flex;align-items:center;gap:17px;background:var(--surface);color:var(--text);padding:21px;text-align:left;}.profile-row:hover{background:var(--surface-raised);}.profile-row:focus-visible{outline:2px solid var(--accent);outline-offset:-3px;}.profile-mark{display:grid;place-items:center;width:43px;height:43px;background:var(--accent-bg);border-radius:9px;color:var(--accent);}.profile-copy{display:grid;gap:8px;flex:1;min-width:0;}.profile-copy strong{font-size:13px;display:flex;align-items:center;gap:12px;overflow-wrap:anywhere;}.profile-copy small{font-size:8px;color:var(--muted);font-weight:400;}.profile-copy>span{font-size:9px;color:var(--muted);}.profile-badges{display:flex;gap:8px;flex-wrap:wrap;justify-content:end;}.profile-badges span{font-size:8px;color:var(--accent);border:1px solid var(--line);border-radius:4px;padding:5px 7px;}.empty-profiles{padding:30px;color:var(--muted);font-size:12px;}.workspace-error,.profile-diagnostics{border:1px solid #ad875440;color:#c9ab7b;background:#ad875408;border-radius:6px;padding:12px 15px;font-size:10px;line-height:1.8;margin-bottom:16px;}.profile-diagnostics p{margin:0;}.back-profiles{font-size:10px;padding-left:0;margin-bottom:20px;}.editor-save-actions{display:flex;align-items:center;gap:15px;}.editor-basics{padding:19px;margin-bottom:20px;}.editor-basics>label{max-width:440px;}.editor-basics label{display:grid;gap:8px;font-size:10px;color:var(--muted);}.editor-basics input{min-width:0;width:100%;}.editor-basics input[readonly]{opacity:.7;}.draft-source{border:0;border-top:1px solid var(--line);margin-top:20px;padding:17px 0 0;display:flex;align-items:end;gap:12px;flex-wrap:wrap;}.draft-source legend{font-size:10px;color:var(--muted);padding-right:10px;}.draft-source label{min-width:170px;}.draft-source button{font-size:10px;}.draft-source p{flex-basis:100%;font-size:9px;color:var(--muted);line-height:1.8;margin:0;}.inventory-heading{display:flex;align-items:center;justify-content:space-between;gap:15px;margin-bottom:15px;}.inventory-heading p{font-size:10px;line-height:1.8;color:var(--muted);}.inventory-heading button{font-size:10px;white-space:nowrap;display:flex;align-items:center;gap:7px;}.inventory-picker{padding:20px;margin-bottom:20px;}.inventory-picker>div:first-child{display:flex;align-items:center;justify-content:space-between;gap:20px;}.inventory-picker button{font-size:10px;}.inventory-help{font-size:10px;color:var(--muted);line-height:1.7;margin:12px 0;}.inventory-choices{max-height:300px;overflow:auto;margin:14px 0;display:grid;grid-template-columns:1fr 1fr;gap:12px;}.inventory-choices label{display:flex;align-items:start;gap:10px;border:1px solid var(--line);border-radius:6px;padding:13px;}.inventory-choices strong{display:block;font-size:11px;color:var(--soft);}.inventory-choices small{display:block;font-size:9px;color:var(--muted);line-height:1.7;margin-top:6px;}.inventory-choices p{font-size:11px;color:var(--muted);}.editor-confirmation{position:fixed;z-index:30;left:50%;top:50%;transform:translate(-50%,-50%);max-width:460px;width:calc(100% - 40px);padding:26px;background:var(--surface);border:1px solid var(--line);border-radius:10px;box-shadow:0 20px 80px #0005;color:var(--text);margin:0;}.editor-confirmation::backdrop{background:#0009;}.editor-confirmation p{font-size:11px;color:var(--muted);line-height:1.8;margin:15px 0 23px;}.editor-confirmation>div{display:flex;gap:10px;justify-content:end;flex-wrap:wrap;}.editor-confirmation button{font-size:10px;}@media(max-width:700px){.page-heading,.editor-title,.inventory-heading{align-items:start;flex-direction:column;}.editor-save-actions{align-self:end;}.profile-badges{max-width:120px;}.profile-row{gap:11px;padding:16px;}.profile-copy strong{display:block;}.profile-copy small{display:block;margin-top:5px;}.inventory-choices{grid-template-columns:1fr;}}
</style>
