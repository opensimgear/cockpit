<script lang="ts">
  import { onDestroy, onMount, untrack } from 'svelte';
  import Icon from './Icon.svelte';
  import Notification from './Notification.svelte';
  import ProfileRoutingEditor from './ProfileRoutingEditor.svelte';
  import ProfileComponentPalette from './ProfileComponentPalette.svelte';
  import ProfileRuntimeControls from './ProfileRuntimeControls.svelte';
  import ProfileAutomaticControls from './ProfileAutomaticControls.svelte';
  import ProfileDebugView from './ProfileDebugView.svelte';
  import ProfileLibraryTools from './ProfileLibraryTools.svelte';
  import type{ProfileSummary}from'./profiles';
  import type{ProfileLibraryClient}from'./profile-library';
  import { gatewayRequest } from './gateway';
  import { DEFAULT_PROFILE_ID, ProfilesClient, profileDocumentError, type ProfileDocument, type ProfileLocalAssignment } from './profiles';
  import { ProfileInventoryClient, type ResourceSummary } from './profile-inventory';
  import { ProfileEditorStore, ProfileNavigationGuard, addProfileResources, profileEditorDirty } from './profile-editor';
  import {ProfileEditorUndo}from'./profile-graph-history';
  import {profilePaletteCanAdd,profilePlaceComponent,type ProfilePaletteComponent,type ProfilePaletteDrag}from'./profile-palette';
  import type {CanvasPoint}from'./profile-canvas';
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
  let { editorStore: suppliedStore, resourceClient: suppliedResources, runtimeClient: suppliedRuntime, libraryClient:suppliedLibrary, openGameSettings, active=true, onNavigate }: { editorStore?: ProfileEditorStore; resourceClient?: ProfileInventoryClient; runtimeClient?: ProfileRuntimeClient;libraryClient?:ProfileLibraryClient; openGameSettings?: (gameId: GameId, returnState?: unknown) => void; active?:boolean; onNavigate?: (profileId:string|null)=>void } = $props();
  const supplied = untrack(() => suppliedStore);
  const resources = untrack(() => suppliedResources) ?? new ProfileInventoryClient(async request => { const result=(await gatewayRequest(request)).profileResources;if(!result)throw new Error('Resource inventory is unavailable. Reconnect the native service.');return result; });
  const profiles = new ProfilesClient(async request => { const result=(await gatewayRequest(request)).profiles;if(!result)throw new Error('Profiles are unavailable. Reconnect the native service.');return result; });
  const store = supplied ?? new ProfileEditorStore(profiles,resources);
  let libraryTools=$state.raw<{askDelete:(row:DeepReadonly<ProfileSummary>)=>void;askImport:()=>void;askExport:(row:DeepReadonly<ProfileSummary>)=>Promise<void>}>(),libraryBusy=$state(false);
  let libraryContext=0;
  const compiler=new ProfileCompilerClient(async request=>{const result=(await gatewayRequest(request)).profileCompilation;if(!result)throw new Error('Native compilation is unavailable. Reconnect the service.');return result;});
  const runtime=new ProfileRuntimeController(untrack(()=>suppliedRuntime)??new ProfileRuntimeClient(async request=>nativeProfileStatus((await gatewayRequest(request)).profiles?.profileSession)));
  let runtimeState=$state.raw<DeepReadonly<RuntimeControlState>>(runtime.state);
  const inspectionClient=new ProfileInspectorClient(async request=>{const result=(await gatewayRequest(request)).profileInspection;if(!result)throw new Error('Native applied port inspection is unavailable. Reconnect the service.');return result;});
  const inspection=new ProfileInspectionController(inspectionClient);
  const activity=new ProfileActivityController(new ProfileTopologyClient(async request=>{const result=(await gatewayRequest(request)).profileTopology;if(!result)throw new Error('Native applied topology is unavailable. Reconnect the service.');return result;}),inspectionClient);
  const learningClient=new ProfileLearningClient(async request=>{const result=(await gatewayRequest(request)).profileLearning;if(!result)throw new Error('Native input learning is unavailable. Reconnect the service.');return result;});
  let routingSelection=$state(''),debugOpen=$state(false);
  const navigationGuard = new ProfileNavigationGuard(store);
  let navigationCancelled:(()=>void)|undefined;
  let workspace = $state.raw(store.state);
  const history=new ProfileEditorUndo(store);
  let notice = $state(''), leaveDialog = $state(false), replaceSource = $state<string|null>(null), copyId = $state(DEFAULT_PROFILE_ID);
  let routingEditor=$state.raw<{addVirtual:(position?:CanvasPoint)=>void;addDesktop:(position?:CanvasPoint)=>void}>();
  let paletteDrag=$state.raw<ProfilePaletteDrag|null>(null);
  let editingName = $state(false), nameDraft = $state('');
  const editor = $derived(workspace.editor);
  const editorIdentity=$derived(editor?.document.id??'');
  const canRename = $derived(!!editor && editor.document.id!==DEFAULT_PROFILE_ID && !workspace.readOnly && !workspace.busy && !libraryBusy);
  const nameChanged = $derived(!!editor && editingName && nameDraft.trim()!==editor.document.name);
  const dirty = $derived(editor ? profileEditorDirty(editor)||nameChanged : false);
  const error = $derived(editor ? profileDocumentError(editingName?{...editor.document,name:nameDraft.trim()}:editor.document) : '');
  const activeProfileId=$derived(runtimeState.stale?undefined:runtimeState.status?.owner?.profileId);
  const rows = $derived([...workspace.profiles].sort((a,b)=>Number(b.id===activeProfileId)-Number(a.id===activeProfileId)||Number(b.isDefault)-Number(a.isDefault)||a.name.localeCompare(b.name)));
  const assignedIds = $derived(new Set(editor?.assignments.map(assignment=>assignment.sourceId)??[]));
  const componentCount=$derived(workspace.inventory.filter(resource=>resource.resourceType!=='companion'||resource.attributes.componentKind==='paired-companion').length);
  const appliedSnapshot=$derived(runtimeState.status?.owner&&!runtimeState.stale?runtimeState.status.appliedSnapshot:null);
  const appliedName = $derived(workspace.profiles.find(row=>row.id===appliedSnapshot?.profileId)?.name??appliedSnapshot?.profileId);
  const runtimeTarget=$derived(editor?{id:editor.document.id,revision:editor.document.revision,baseRevision:editor.baseRevision,dirty,readOnly:workspace.readOnly||workspace.busy||libraryBusy||!!error}:null);
  const learningIdle=$derived(!!runtimeState.status&&!runtimeState.stale&&runtimeState.status.owner===null&&!['transitioning','recovery-needed'].includes(runtimeState.status.state));
  const editorContext=$derived.by(()=>{void workspace;return `${editorIdentity}/${store.editorEpoch}`;});
  const historyStatus=$derived.by(()=>{void workspace;return history.status;});
  onMount(() => { const stop=store.subscribe(next=>workspace=next);if(workspace.view==='list')void store.refresh();return stop; });
  onDestroy(() => { navigationCancelled?.();runtime.dispose();inspection.dispose();activity.dispose();if(!supplied)store.dispose(); });
  $effect(()=>{void editorIdentity;routingSelection='';});
  $effect(()=>{void editorContext;editingName=false;});
  $effect(()=>{void editorContext;paletteDrag=null;});
  $effect(()=>{if(paletteDrag&&(!active||debugOpen||!canDropComponent(paletteDrag.component)))paletteDrag=null;});
  $effect(()=>{
    if(!active)return;
    const refresh=()=>{if(!document.hidden&&!store.state.inventoryBusy)void store.refreshInventory();};
    refresh();const timer=setInterval(refresh,3000);return()=>clearInterval(timer);
  });
  function report(operation:()=>void) { notice='';const previous=getNavigation();try{operation();workspace=store.state;if(getNavigation()!==previous)onNavigate?.(getNavigation());}catch(error){notice=error instanceof Error?error.message:String(error);} }
  export function getNavigation():string|null { const current=store.state;return current.view==='editor'?current.editor?.document.id??null:null; }
  export async function restoreNavigation(profileId:string|null):Promise<void> {
    debugOpen=false;
    if(getNavigation()===profileId)return;
    if(profileId===null){store.back(true);workspace=store.state;return;}
    await store.open(profileId);
    if(getNavigation()!==profileId)store.back(true);
    workspace=store.state;
  }
  async function openProfile(profileId:string) { await store.open(profileId);workspace=store.state;if(getNavigation()===profileId)onNavigate?.(profileId); }
  function add() { notice='';report(()=>store.beginBlank([])); }
  function showConfirmation(node:HTMLDialogElement) { node.showModal();return()=>node.close(); }
  function change(document:ProfileDocument,assignments?:ProfileLocalAssignment[],label='Edit profile graph') { report(()=>history.edit(document,assignments,label)); }
  function name(value:string) { if(!editor)return;const next=JSON.parse(JSON.stringify(editor.document)) as ProfileDocument;next.name=value;change(next,undefined,'Rename profile'); }
  function editName() { if(!canRename||!editor)return;nameDraft=editor.document.name;editingName=true; }
  function focusName(node:HTMLInputElement) { node.focus();node.select(); }
  function finishName() { if(!editingName)return;editingName=false;const title=nameDraft.trim();if(canRename&&title!==editor?.document.name)name(title); }
  function historyStep(direction:'undo'|'redo'){report(()=>{history.step(direction);});}
  export function requestNavigation(continueNavigation:()=>void,cancelNavigation?:()=>void):void {
    finishName();
    notice='';
    navigationCancelled?.();navigationCancelled=undefined;navigationGuard.keepEditing();
    if(libraryBusy){notice='Wait for the current library operation before leaving.';cancelNavigation?.();return;}
    navigationCancelled=cancelNavigation;
    const result=navigationGuard.request(()=>{navigationCancelled=undefined;continueNavigation();});
    leaveDialog=result==='confirm';
    if(result==='busy'){notice='Wait for the current profile operation to finish before leaving.';navigationCancelled=undefined;cancelNavigation?.();}
    workspace=store.state;
  }
  function back() { requestNavigation(()=>onNavigate?.(null)); }
  function keepEditing() { navigationGuard.keepEditing();leaveDialog=false;const cancelNavigation=navigationCancelled;navigationCancelled=undefined;cancelNavigation?.(); }
  async function save(backAfter=false) { finishName();notice='';if(backAfter){await navigationGuard.save();leaveDialog=navigationGuard.awaitingChoice;}else await store.save();workspace=store.state;await runtime.refresh(); }
  function cancel() { const pending=navigationGuard.awaitingChoice;if(!navigationGuard.discard())return;workspace=store.state;notice='';leaveDialog=false;if(!pending)onNavigate?.(null); }
  function source(id:string) { if(!editor)return;if(JSON.stringify(editor.document)!==JSON.stringify(editor.baseDocument)||JSON.stringify(editor.assignments)!==JSON.stringify(editor.baseAssignments)){replaceSource=id;return;}void replace(id); }
  async function replace(id:string) { replaceSource=null;notice='';if(id==='blank')report(()=>store.beginBlank([]));else{const previous=getNavigation();await store.beginCopy(id);workspace=store.state;if(getNavigation()!==previous)onNavigate?.(getNavigation());} }
  function canDropComponent(component:ProfilePaletteComponent):boolean {
    return !!editor&&!workspace.busy&&!workspace.readOnly&&!libraryBusy&&profilePaletteCanAdd(component,editor.document,editor.assignments,workspace.inventory);
  }
  function addResource(resource:DeepReadonly<ResourceSummary>,position?:CanvasPoint) {
    if(!editor||!canDropComponent({kind:'resource',sourceId:resource.id}))return;
    report(()=>{const before=editor!.document,previous=new Set(before.resources.map(resource=>resource.id));const next=addProfileResources(JSON.parse(JSON.stringify(before)),JSON.parse(JSON.stringify(editor!.assignments)),[resource]);const nodeId=next.document.resources.find(resource=>!previous.has(resource.id))!.id;change(profilePlaceComponent(before,next.document,nodeId,position),next.assignments,'Add component');});
  }
  function dropComponent(component:ProfilePaletteComponent,position:CanvasPoint){
    if(!canDropComponent(component))return;
    if(component.kind==='virtual')routingEditor?.addVirtual(position);
    else if(component.kind==='desktop')routingEditor?.addDesktop(position);
    else{const resource=workspace.inventory.find(resource=>resource.id===component.sourceId);if(resource)addResource(resource,position);}
  }
  function startPaletteDrag(drag:ProfilePaletteDrag){
    if(canDropComponent(drag.component))paletteDrag=drag;
  }
  function endPaletteDrag(){paletteDrag=null;}
  function gameSettings(gameId:GameId,returnState?:unknown) { openGameSettings?.(gameId,{editorStore:store,profileId:editor?.document.id,selection:returnState}); }
  function prepareLibrary(next:()=>void){requestNavigation(()=>{onNavigate?.(null);libraryContext=store.editorEpoch;next();});}
  async function libraryDeleted(){if(store.editorEpoch!==libraryContext||store.state.view!=='list')return;await store.refresh();workspace=store.state;}
  async function libraryImported(document:ProfileDocument){if(store.editorEpoch!==libraryContext||store.state.view!=='list')return;await store.refresh();workspace=store.state;await openProfile(document.id);}
  const savedEditor=$derived(editor?workspace.profiles.find(row=>row.id===editor.document.id):undefined);
  function activationDisabled(profile:DeepReadonly<ProfileSummary>):boolean {
    const status=runtimeState.status;
    return workspace.busy||workspace.readOnly||libraryBusy||!status||runtimeState.stale||!!runtimeState.pending||!!runtimeState.activationConfirmation||['recovery-needed','transitioning','awaiting-game'].includes(status.state)||status.owner?.profileId===profile.id;
  }
  async function activateProfile(profile:DeepReadonly<ProfileSummary>) {
    if(activationDisabled(profile))return;
    await runtime.apply({id:profile.id,revision:profile.revision,baseRevision:profile.revision,dirty:false,readOnly:workspace.readOnly});
  }
</script>

{#snippet runtimeTools()}
  <ProfileRuntimeControls controller={runtime} editor={runtimeTarget} profiles={workspace.profiles} gameIds={editor?.document.eligibility.gameIds??[]} compact={workspace.view==='list'} active={active&&!debugOpen} onState={next=>runtimeState=next}/>
{/snippet}

<section class="profiles-workspace" aria-label="Profiles">
  {#if debugOpen}<ProfileDebugView {editor} profiles={workspace.profiles} inventory={workspace.inventory} inventoryRevision={store.inventoryRevision} {compiler} {runtimeState} {runtime} onRuntimeState={next=>runtimeState=next} {inspection} {activity} selectedId={routingSelection} profileName={appliedName} {active} loadProfile={id=>profiles.getAssignments(id)} close={()=>debugOpen=false} locate={id=>{routingSelection=id;debugOpen=false;}}/>{/if}
  <div class="profile-content" hidden={debugOpen}>
  {#if workspace.view==='list'}
    <div class="page-heading"><div><div class="eyebrow">YOUR OPERATING SETUPS</div><h1>Profiles</h1><p>Edit Default or create an independent profile for your rig.</p></div><div class="page-actions"><ProfileAutomaticControls profiles={workspace.profiles} active={active&&!debugOpen}/><button class="secondary-button" onclick={()=>debugOpen=true}><Icon name="bug" size={14}/>Debug</button><button class="primary-button" disabled={workspace.busy||libraryBusy||workspace.readOnly} onclick={add}><Icon name="plus" size={14}/> Add profile</button></div></div>
  {:else if editor}
    <button class="text-button back-profiles" disabled={workspace.busy} onclick={back}>← Back to Profiles</button>
    <div class="editor-title">
      <div class="profile-heading-copy"><p class="eyebrow">{editor.baseRevision===null?'NEW PROFILE DRAFT':'PROFILE EDITOR'}</p><h1 aria-label={editor.document.name||'Untitled profile'}>{#if editingName}<input class="profile-name-input" aria-label="Profile name" maxlength="80" bind:value={nameDraft} use:focusName onblur={finishName} onkeydown={event=>{if(event.key==='Enter'){event.preventDefault();finishName();}else if(event.key==='Escape'){event.preventDefault();editingName=false;}}}/>{:else if editor.document.id!==DEFAULT_PROFILE_ID}<button class="profile-title-button" aria-label="Edit profile name" title="Edit profile name" disabled={!canRename} onclick={editName}><span>{editor.document.name||'Untitled profile'}</span><Icon name="pencil" size={18}/></button>{:else}{editor.document.name}{/if}</h1><p>{dirty?'Unsaved changes. ':''}{appliedSnapshot?.profileId===editor.document.id?'Saving updates this active profile.':'Activate this profile when ready.'}</p></div>
      <div class="editor-save-actions">
        <button class="secondary-button" onclick={()=>debugOpen=true}><Icon name="bug" size={14}/>Debug</button>
        <button class="secondary-button" aria-label="Export saved profile" title="Export saved profile" disabled={workspace.busy||libraryBusy||!savedEditor} onclick={()=>savedEditor&&libraryTools?.askExport(savedEditor)}><Icon name="download" size={14}/>Export</button>
        {#if editor.baseRevision===null}<button class="text-button" disabled={workspace.busy} onclick={cancel}>Cancel creation</button>{/if}
        <button class="primary-button" disabled={workspace.busy||workspace.readOnly||!dirty||!!error} onclick={()=>save()}><Icon name="save" size={14}/> {workspace.busy?'Saving…':'Save profile'}</button>
        {#if savedEditor&&!savedEditor.isDefault}<button class="secondary-button destructive-button" aria-label="Delete saved profile" disabled={workspace.busy||libraryBusy||workspace.readOnly} onclick={()=>libraryTools?.askDelete(savedEditor!)}><Icon name="trash" size={14}/>Delete</button>{/if}
      </div>
    </div>
  {/if}
  {@render runtimeTools()}
  {#if workspace.saveMessage}<Notification message={workspace.saveMessage} kind={workspace.saveNotificationKind}/>{/if}
  {#if workspace.diagnostics.length}<div class="profile-diagnostics" role="status">{#each workspace.diagnostics as diagnostic,index (index)}<p>{diagnostic}</p>{/each}</div>{/if}
  {#if workspace.readOnly}<p class="workspace-error" role="status">This library is read-only. Resolve its diagnostics before making changes.</p>{/if}
  {#if notice||workspace.error}<p class="workspace-error" role="alert">{notice||workspace.error}</p>{/if}
  {#if workspace.view==='list'}
    <div class="list-tools"><span>{rows.length} {rows.length===1?'profile':'profiles'} · {componentCount} components</span><div><button class="text-button" disabled={workspace.busy||libraryBusy||workspace.readOnly||rows.length>=32} onclick={()=>libraryTools?.askImport()}>Import profile</button><button class="text-button" disabled={workspace.busy||libraryBusy} onclick={()=>{void store.refresh();void store.refreshInventory();}}>Refresh profiles</button></div></div>
    <ul class="profile-list" aria-label="Saved profiles">{#each rows as profile (profile.id)}<li class="profile-tile" class:profile-active={activeProfileId===profile.id} class:profile-muted={!!activeProfileId&&activeProfileId!==profile.id}><button class="profile-row" disabled={workspace.busy||libraryBusy} onclick={()=>openProfile(profile.id)} title={profile.name} aria-label={`Edit ${profile.name}`}><span class="profile-tile-heading"><span class="profile-mark"><Icon name="network" size={22}/></span><span class="profile-badges">{#if activeProfileId===profile.id}<span>Active</span>{/if}</span></span><span class="profile-copy"><strong>{profile.name}</strong>{#if profile.isDefault}<span class="built-in-badge">Built-in</span>{/if}</span></button><div class="profile-row-actions"><button class="secondary-button activate-profile" title={runtimeState.status?.owner?.profileId===profile.id?'Deactivate profile':'Activate saved profile (hardware only)'} aria-label={runtimeState.status?.owner?.profileId===profile.id?`Deactivate ${profile.name}`:`Activate ${profile.name}`} disabled={runtimeState.status?.owner?.profileId===profile.id?runtimeState.pending==='stop':activationDisabled(profile)} onclick={()=>runtimeState.status?.owner?.profileId===profile.id?runtime.stop():activateProfile(profile)}><Icon name={runtimeState.status?.owner?.profileId===profile.id?'stop':'play'} size={14}/>{runtimeState.status?.owner?.profileId===profile.id?'Deactivate':'Activate'}</button><button class="icon-button" title="Export profile" aria-label={`Export ${profile.name}`} disabled={workspace.busy||libraryBusy} onclick={()=>libraryTools?.askExport(profile)}><Icon name="download" size={16}/></button>{#if !profile.isDefault}<button class="icon-button delete-profile" title="Delete profile" aria-label={`Delete ${profile.name}`} disabled={workspace.busy||libraryBusy||workspace.readOnly} onclick={()=>libraryTools?.askDelete(profile)}><Icon name="trash" size={16}/></button>{/if}</div></li>{:else}<li class="empty-profiles">{workspace.busy?'Loading profiles…':'No profile list is available. Refresh profiles to load Default.'}</li>{/each}</ul>
    {#if workspace.inventoryError}<p class="workspace-error" role="status">{workspace.inventoryError}</p><button class="secondary-button" disabled={workspace.inventoryBusy} onclick={()=>store.refreshInventory()}>Refresh resource inventory</button>{/if}
  {:else if editor}
    {#if editor.baseRevision===null}<div class="editor-basics panel"><fieldset class="draft-source"><legend>Start from</legend><button class="secondary-button" disabled={workspace.busy||workspace.readOnly} onclick={()=>source('blank')}>Blank profile</button><label>Copy saved profile<select aria-label="Copy source profile" bind:value={copyId} disabled={workspace.busy}>{#each rows as profile (profile.id)}<option value={profile.id}>{profile.name}</option>{/each}</select></label><button class="secondary-button" disabled={workspace.busy||workspace.readOnly||!copyId} onclick={()=>source(copyId)}>Use copy</button><p>Copies retain the graph and layout with a new identity. Select devices for the copy; machine assignments are excluded.</p></fieldset></div>{/if}
    {#if error}<p class="workspace-error" role="alert">{error}</p>{/if}
    <div class="profile-authoring">
      <ProfileComponentPalette inventory={workspace.inventory} {assignedIds} resourceCount={editor.document.resources.length} virtualCount={editor.document.virtualOutputs.length} desktopAdded={!!editor.document.desktopOutputs?.length} disabled={workspace.readOnly||workspace.busy||libraryBusy} refreshing={workspace.inventoryBusy} error={workspace.inventoryError} refresh={()=>void store.refreshInventory()} addResource={resource=>addResource(resource)} addVirtual={()=>{if(canDropComponent({kind:'virtual'}))routingEditor?.addVirtual();}} addDesktop={()=>{if(canDropComponent({kind:'desktop'}))routingEditor?.addDesktop();}} startDrag={startPaletteDrag} endDrag={endPaletteDrag}/>
      <div class="profile-canvas-workspace">{#key editor.document.id}<ProfileRoutingEditor bind:this={routingEditor} document={editor.document} assignments={editor.assignments} inventory={workspace.inventory} inventoryClient={resources} compilerClient={compiler} {learningClient} {editorContext} {paletteDrag} {canDropComponent} {dropComponent} {endPaletteDrag} active={active&&!debugOpen} runtimeIdle={learningIdle} inventoryRevision={store.inventoryRevision} readOnly={workspace.readOnly||workspace.busy||libraryBusy} bind:selectedId={routingSelection} change={(draft,label)=>change(draft.document,draft.assignments,label)} openGameSettings={openGameSettings?gameSettings:undefined} undo={()=>historyStep('undo')} redo={()=>historyStep('redo')} canUndo={historyStatus.canUndo} canRedo={historyStatus.canRedo} undoLabel={historyStatus.undoLabel} redoLabel={historyStatus.redoLabel}/>{/key}</div>
    </div>
  {/if}
  </div>
  <ProfileLibraryTools bind:this={libraryTools} client={suppliedLibrary??profiles} profileCount={rows.length} readOnly={workspace.readOnly} busy={workspace.busy} prepare={prepareLibrary} resolveProfile={id=>workspace.profiles.find(row=>row.id===id)} onDeleted={libraryDeleted} onImported={libraryImported} onBusy={value=>libraryBusy=value}/>
  {#if runtimeState.activationConfirmation}<dialog class="editor-confirmation" aria-label="Switch active profile?" oncancel={event=>{event.preventDefault();runtime.cancelActivation();}} {@attach showConfirmation}><h2>Switch active profile?</h2><p>Deactivate {workspace.profiles.find(profile=>profile.id===runtimeState.activationConfirmation?.previousOwner.profileId)?.name??'the active profile'} and activate {workspace.profiles.find(profile=>profile.id===runtimeState.activationConfirmation?.target.id)?.name??'the new profile'}?</p><div><button class="secondary-button" onclick={()=>runtime.cancelActivation()}>Cancel</button><button class="primary-button" disabled={!!runtimeState.pending||runtimeState.stale} onclick={()=>runtime.confirmActivation()}>Switch profile</button></div></dialog>{/if}
  {#if leaveDialog}<dialog class="editor-confirmation" role="alertdialog" aria-modal="true" aria-labelledby="leave-profile-title" onclose={keepEditing} {@attach showConfirmation}><h2 id="leave-profile-title">Keep your unsaved changes?</h2><p>Save this profile before leaving, or discard your changes. Saving updates it immediately if it is active.</p>{#if workspace.error}<p class="workspace-error" role="alert">{workspace.error}</p>{/if}<div><button class="secondary-button" onclick={keepEditing}>Keep editing</button><button class="secondary-button" disabled={workspace.busy} onclick={cancel}>Discard and continue</button><button class="primary-button" disabled={!!error||workspace.readOnly||workspace.busy} onclick={()=>save(true)}>Save and continue</button></div></dialog>{/if}
  {#if replaceSource}<dialog class="editor-confirmation" role="alertdialog" aria-modal="true" aria-labelledby="replace-profile-title" onclose={()=>replaceSource=null} {@attach showConfirmation}><h2 id="replace-profile-title">Replace this new draft?</h2><p>Your unsaved edits will be discarded before starting from {replaceSource==='blank'?'an empty profile':'the selected copy'}.</p><div><button class="secondary-button" onclick={()=>replaceSource=null}>Keep this draft</button><button class="primary-button" onclick={()=>replace(replaceSource!)}>Replace draft</button></div></dialog>{/if}
</section>

<style>
  .profile-authoring{display:grid;grid-template-columns:236px minmax(0,1fr);align-items:start;gap:18px}.profile-canvas-workspace{min-width:0}@media(max-width:1050px){.profile-authoring{grid-template-columns:minmax(0,1fr)}}
  .profile-heading-copy{min-width:0;flex:1}.profile-title-button{display:inline-flex;align-items:center;gap:10px;max-width:100%;padding:0;background:transparent;color:inherit;font:inherit;letter-spacing:inherit;text-align:left}.profile-title-button span{min-width:0;overflow-wrap:anywhere}.profile-title-button :global(svg){flex-shrink:0;color:var(--muted);opacity:0;transition:opacity .15s}.profile-title-button:hover:enabled :global(svg),.profile-title-button:focus-visible :global(svg){opacity:1}.profile-name-input{width:100%;min-width:0;margin:0;padding:0 6px;font:inherit;letter-spacing:inherit;line-height:inherit;background:var(--input-bg);border:1px solid var(--line);border-radius:4px}.editor-save-actions{flex-shrink:0}@media(hover:none){.profile-title-button:enabled :global(svg){opacity:1}}
  .profile-content[hidden]{display:none}.page-actions{display:flex;align-items:center;justify-content:flex-end;gap:18px;flex-wrap:wrap}.profile-tile.profile-active{border-color:var(--accent);background:var(--surface-raised)}.profile-tile.profile-muted{filter:grayscale(1);opacity:.6}.profile-tile.profile-muted:hover,.profile-tile.profile-muted:focus-within{filter:none;opacity:1}
  .profile-row-actions{position:relative;z-index:1;display:flex;align-items:center;justify-content:end;gap:6px;padding:0 16px 12px;}.profile-row-actions button{width:34px;height:34px;padding:0;border:1px solid var(--line);border-radius:5px;}.profile-row-actions button:hover:enabled{border-color:var(--accent);background:var(--surface-raised);}.profile-row-actions .activate-profile{width:auto;min-height:34px;margin-right:auto;padding:0 10px;font-size:10px;color:var(--accent);}.profile-row-actions .delete-profile{color:var(--danger,#e67979);}.profile-row-actions button:focus-visible{outline:2px solid var(--accent);outline-offset:2px;}

  .page-heading,.editor-title{display:flex;align-items:center;justify-content:space-between;gap:20px;margin-bottom:24px;}h1{font-size:26px;letter-spacing:-.5px;margin:8px 0 10px;}h2{font-size:14px;}.page-heading p,.editor-title p{font-size:11px;color:var(--muted);line-height:1.8;}.page-heading button,.editor-save-actions button{display:inline-flex;align-items:center;gap:8px;white-space:nowrap;font-size:11px;}.eyebrow{font-size:8px!important;letter-spacing:1.5px;text-transform:uppercase;color:var(--muted);}.list-tools{display:flex;align-items:center;justify-content:space-between;font-size:10px;color:var(--muted);margin-bottom:12px;}.list-tools button{font-size:10px;}.list-tools>div{display:flex;gap:12px;flex-wrap:wrap;justify-content:end;}.profile-list{display:grid;grid-template-columns:repeat(auto-fill,minmax(min(100%,260px),1fr));gap:18px;list-style:none;padding:0;margin:0 0 22px;}.profile-tile{position:relative;display:flex;flex-direction:column;min-width:0;min-height:224px;border:1px solid var(--line);border-radius:9px;background:var(--surface);transition:border-color .15s,background .15s;}.profile-tile:hover{border-color:var(--accent);background:var(--surface-raised);}.profile-row{width:100%;flex:1;display:flex;flex-direction:column;align-items:stretch;gap:18px;background:transparent;color:var(--text);padding:20px;text-align:left;border-radius:9px;}.profile-row::after{content:'';position:absolute;inset:0;border-radius:9px;}.profile-row:focus-visible{outline:none;}.profile-row:focus-visible::after{outline:2px solid var(--accent);outline-offset:2px;}.profile-tile-heading{display:flex;align-items:start;justify-content:space-between;gap:14px;}.profile-mark{display:grid;place-items:center;width:43px;height:43px;flex-shrink:0;background:var(--accent-bg);border-radius:9px;color:var(--accent);}.profile-copy{display:flex;align-items:center;gap:8px;min-width:0;}.profile-copy strong{min-width:0;font-size:16px;line-height:1.4;display:block;overflow:hidden;text-overflow:ellipsis;white-space:nowrap;}.built-in-badge{flex-shrink:0;font-size:9px;line-height:1.4;color:var(--muted);border:1px solid var(--line);border-radius:4px;padding:4px 6px;}.profile-badges{display:flex;gap:6px;flex-wrap:wrap;justify-content:end;min-width:0;}.profile-badges span{font-size:9px;line-height:1.4;color:var(--accent);border:1px solid var(--line);border-radius:4px;padding:5px 7px;}.empty-profiles{grid-column:1/-1;border:1px solid var(--line);border-radius:9px;background:var(--surface);padding:30px;color:var(--muted);font-size:12px;}.workspace-error,.profile-diagnostics{border:1px solid #ad875440;color:#c9ab7b;background:#ad875408;border-radius:6px;padding:12px 15px;font-size:10px;line-height:1.8;margin-bottom:16px;}.profile-diagnostics p{margin:0;}.back-profiles{font-size:10px;padding-left:0;margin-bottom:20px;}.editor-save-actions{display:flex;align-items:center;justify-content:flex-end;gap:10px;flex-wrap:wrap;max-width:100%;}.editor-save-actions .destructive-button{background:#b32b38;border-color:#cd4552;color:#fff}.editor-save-actions .destructive-button:hover:enabled{background:#98232e;border-color:#e35d69;color:#fff}.editor-basics{padding:19px;margin-bottom:20px;}.editor-basics label{display:grid;gap:8px;font-size:10px;color:var(--muted);}.draft-source{border:0;margin:0;padding:0;display:flex;align-items:end;gap:12px;flex-wrap:wrap;}.draft-source legend{font-size:10px;color:var(--muted);padding-right:10px;}.draft-source label{min-width:170px;}.draft-source button{font-size:10px;}.draft-source p{flex-basis:100%;font-size:9px;color:var(--muted);line-height:1.8;margin:0;}.editor-confirmation{position:fixed;z-index:30;left:50%;top:50%;transform:translate(-50%,-50%);max-width:460px;width:calc(100% - 40px);padding:26px;background:var(--surface);border:1px solid var(--line);border-radius:10px;box-shadow:0 20px 80px #0005;color:var(--text);margin:0;}.editor-confirmation::backdrop{background:#0009;}.editor-confirmation p{font-size:11px;color:var(--muted);line-height:1.8;margin:15px 0 23px;}.editor-confirmation>div{display:flex;gap:10px;justify-content:end;flex-wrap:wrap;}.editor-confirmation button{font-size:10px;}@media(max-width:700px){.page-heading,.editor-title{align-items:start;flex-direction:column;}.editor-save-actions{align-self:end;}.profile-row{gap:11px;padding:16px;}.profile-copy strong{display:block;}}
</style>
