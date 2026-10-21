<script lang="ts">
  import { onDestroy, onMount, tick, untrack } from 'svelte';
  import Icon from './Icon.svelte';
  import { notify } from './notifications';
  import ProfileRoutingEditor from './ProfileRoutingEditor.svelte';
  import ProfileComponentPalette from './ProfileComponentPalette.svelte';
  import ProfileVehicleConflicts from './ProfileVehicleConflicts.svelte';
  import ProfileRuntimeControls from './ProfileRuntimeControls.svelte';
  import ProfileAutomaticControls from './ProfileAutomaticControls.svelte';
  import ProfileLibraryTools from './ProfileLibraryTools.svelte';
  import type{ProfileSummary}from'./profiles';
  import type{ProfileLibraryClient}from'./profile-library';
  import { gatewayRequest } from './gateway';
  import { DEFAULT_PROFILE_ID, ProfilesClient, profileDocumentError, type ProfileDocument, type ProfileLocalAssignment } from './profiles';
  import { ProfileInventoryClient, type ResourceSummary } from './profile-inventory';
  import { ProfileEditorStore, ProfileNavigationGuard, addProfileResources, profileEditorDirty } from './profile-editor';
  import {ProfileEditorUndo}from'./profile-graph-history';
  import {profilePaletteCanAdd,profilePalettePanelReason,profilePlaceComponent,type ProfilePaletteComponent,type ProfilePaletteDrag}from'./profile-palette';
  import type {ProcessingPreset}from'./profile-processing';
  import {ProfileCompanionAuthoringClient,graphCaptureCompanionPanel,latestCompanionPanels,type ProfileCompanionCatalogue}from'./profile-companion-authoring';
  import type {CanvasPoint}from'./profile-canvas';
  import {profileGameContext,profileGameContextNeedsSync,profileGameName,profileSetGameContext,profileAddVehicleIntegration,profileChangeVehicleIntegration,profileGameTabInventory,profileReconcileDcsVehicleCatalogue}from'./profile-game-context';
  import {profileAddVehicleTab,profileRemoveVehicleTab,profileStampVehicleNodes,profileTabDocument,profileTabNodeIds}from'./profile-vehicle-tabs';
  import {ProfileCompilerClient}from'./profile-compilation';
  import {ProfileRuntimeClient,nativeProfileStatus}from'./profile-runtime';
  import {ProfileRuntimeController,type RuntimeControlState}from'./profile-runtime-controller';
  import {ProfileLearningClient}from'./profile-learning';
  import type {DeepReadonly} from './profiles';
  import { supportedGames, type Game, type GameId } from './games';
  let { editorStore: suppliedStore, resourceClient: suppliedResources, runtimeClient: suppliedRuntime, libraryClient:suppliedLibrary, companionClient:suppliedCompanion, openGameSettings, active=true, onNavigate }: { editorStore?: ProfileEditorStore; resourceClient?: ProfileInventoryClient; runtimeClient?: ProfileRuntimeClient;libraryClient?:ProfileLibraryClient;companionClient?:Pick<ProfileCompanionAuthoringClient,'catalogue'|'inspect'>; openGameSettings?: (gameId: GameId, returnState?: unknown) => void; active?:boolean; onNavigate?: (profileId:string|null)=>void } = $props();
  const supplied = untrack(() => suppliedStore);
  const resources = untrack(() => suppliedResources) ?? new ProfileInventoryClient(async request => { const result=(await gatewayRequest(request)).profileResources;if(!result)throw new Error('Resource inventory is unavailable. Reconnect the native service.');return result; });
  const profiles = new ProfilesClient(async request => { const result=(await gatewayRequest(request)).profiles;if(!result)throw new Error('Profiles are unavailable. Reconnect the native service.');return result; });
  const companionAuthoring=untrack(()=>suppliedCompanion)??new ProfileCompanionAuthoringClient(async request=>(await gatewayRequest(request)).profileCompanionAuthoring);
  const store = supplied ?? new ProfileEditorStore(profiles,resources);
  let libraryTools=$state.raw<{askDelete:(row:DeepReadonly<ProfileSummary>)=>void;askImport:()=>void;askExport:(row:DeepReadonly<ProfileSummary>)=>Promise<void>}>(),libraryBusy=$state(false);
  let libraryContext=0;
  const compiler=new ProfileCompilerClient(async request=>{const result=(await gatewayRequest(request)).profileCompilation;if(!result)throw new Error('Native compilation is unavailable. Reconnect the service.');return result;});
  const runtime=new ProfileRuntimeController(untrack(()=>suppliedRuntime)??new ProfileRuntimeClient(async request=>{const result=await gatewayRequest(request);return request.command==='profile_activation_status'||request.command==='profile_activation_cancel'?result.profileActivation:nativeProfileStatus(result.profiles?.profileSession);}));
  let runtimeState=$state.raw<DeepReadonly<RuntimeControlState>>(runtime.state);
  const learningClient=new ProfileLearningClient(async request=>{const result=(await gatewayRequest(request)).profileLearning;if(!result)throw new Error('Native input learning is unavailable. Reconnect the service.');return result;});
  let routingSelection=$state('');
  const navigationGuard = new ProfileNavigationGuard(store);
  let navigationCancelled:(()=>void)|undefined;
  let workspace = $state.raw(store.state);
  const history=new ProfileEditorUndo(store);
  let notice = $state(''), leaveDialog = $state(false), replaceSource = $state<string|null>(null), copyId = $state(DEFAULT_PROFILE_ID);
  let routingEditor=$state.raw<{addVirtual:(position?:CanvasPoint)=>void;addDesktop:(position?:CanvasPoint)=>void;addTransformation:(preset:ProcessingPreset,position?:CanvasPoint)=>void;addGroup:()=>string|undefined;focusGroup:(id:string)=>void;renameGroup:(id:string,name:string)=>void;deleteGroup:(id:string)=>void}>();
  let paletteDrag=$state.raw<ProfilePaletteDrag|null>(null);
  let companionCatalogue=$state.raw<DeepReadonly<ProfileCompanionCatalogue>|null>(null),companionPanelsBusy=$state(false),companionPanelsError=$state(''),companionDropBusy=$state(false),companionCatalogueEpoch=0,companionDropEpoch=0;
  let editingName = $state(false), nameDraft = $state('');
  let gameCatalogue=$state.raw<Game[]>(supportedGames),vehicleError=$state(''),activeVehicleTabId=$state(''),addingVehicleTab=$state(false),customVehicle=$state(false),vehicleIdDraft=$state(''),vehicleNameDraft=$state('');
  let vehicleTabsElement=$state.raw<HTMLDivElement|undefined>();
  let vehicleSelect=$state.raw<HTMLSelectElement|undefined>();
  let dcsCatalogues=$state.raw<Record<string,DeepReadonly<ResourceSummary>>>({}),dcsCatalogueBusy=$state(false),dcsCatalogueError=$state(''),dcsCatalogueRevision=$state(0);
  let requestedDcsCatalogue='',dcsCatalogueEpoch=0;
  const editor = $derived(workspace.editor);
  const editorInventory=$derived([...workspace.inventory,...Object.values(dcsCatalogues).filter(source=>!workspace.inventory.some(current=>current.id===source.id))]);
  const selectedGame=$derived(editor?profileGameContext(editor.document):'');
  const contextGame=$derived(gameCatalogue.find(game=>game.id===selectedGame));
  const vehicleTabs=$derived(editor?.document.vehicleTabs??[]);
  const vehicleTabsEnabled=$derived(!!selectedGame&&(contextGame?.vehicleSupport?.supported??true));
  const activeVehicleTab=$derived(vehicleTabs.find(tab=>tab.id===activeVehicleTabId));
  const dcsCatalogueKey=$derived(active&&selectedGame==='dcs'&&activeVehicleTab&&!addingVehicleTab?`${editor?.document.id}/${activeVehicleTab.id}/${activeVehicleTab.vehicleId}`:'');
  const routingInventory=$derived(editor?profileGameTabInventory(editor.document,editorInventory,activeVehicleTabId):editorInventory);
  const tabDcsModule=$derived(routingInventory.find(source=>source.attributes.gameId==='dcs')?.attributes.aircraftModule);
  const inventoryRevision=$derived.by(()=>{void workspace;return store.inventoryRevision+dcsCatalogueRevision;});
  const tabDocument=$derived(editor?profileTabDocument(editor.document,activeVehicleTabId):null);
  const tabNodeIds=$derived(editor?profileTabNodeIds(editor.document,activeVehicleTabId):new Set<string>());
  const vehicleChoices=$derived.by(()=>{const choices=new Map<string,{id:string;name:string}>();for(const vehicle of contextGame?.vehicleSupport?.vehicles??[])choices.set(vehicle.id,vehicle);for(const tab of vehicleTabs)choices.set(tab.vehicleId,{id:tab.vehicleId,name:tab.vehicleName});if(contextGame?.currentVehicle)choices.set(contextGame.currentVehicle.id,contextGame.currentVehicle);return [...choices.values()].sort((a,b)=>a.name.localeCompare(b.name));});
  const editorIdentity=$derived(editor?.document.id??'');
  const canRename = $derived(!!editor && editor.document.id!==DEFAULT_PROFILE_ID && !workspace.readOnly && !workspace.busy && !libraryBusy);
  const nameChanged = $derived(!!editor && editingName && nameDraft.trim()!==editor.document.name);
  const dirty = $derived(editor ? profileEditorDirty(editor)||nameChanged : false);
  const error = $derived(editor ? profileDocumentError(editingName?{...editor.document,name:nameDraft.trim()}:editor.document) : '');
  const activeProfileId=$derived(runtimeState.stale?undefined:runtimeState.status?.owner?.profileId);
  const rows = $derived([...workspace.profiles].sort((a,b)=>Number(b.id===activeProfileId)-Number(a.id===activeProfileId)||Number(b.isDefault)-Number(a.isDefault)||a.name.localeCompare(b.name)));
  const assignedIds = $derived(new Set(editor?.assignments.filter(assignment=>tabNodeIds.has(assignment.resourceId)).map(assignment=>assignment.sourceId)??[]));
  const capturedCompanionIds=$derived.by(()=>{const selected=new Set(editor?.document.companionAssignments.map(panel=>editor.assignments.find(assignment=>assignment.resourceId===panel.resourceId)?.sourceId).filter((id):id is string=>!!id)??[]),identities=new Set(workspace.inventory.filter(source=>selected.has(source.id)).map(source=>source.attributes.companionId).filter(Boolean));return new Set([...selected,...workspace.inventory.filter(source=>!!source.attributes.companionId&&identities.has(source.attributes.companionId)).map(source=>source.id)]);});
  const appliedSnapshot=$derived(runtimeState.status?.owner&&!runtimeState.stale?runtimeState.status.appliedSnapshot:null);
  const learningIdle=$derived(!!runtimeState.status&&!runtimeState.stale&&runtimeState.status.owner===null&&!['transitioning','recovery-needed'].includes(runtimeState.status.state));
  const editorContext=$derived.by(()=>{void workspace;return `${editorIdentity}/${store.editorEpoch}`;});
  const historyStatus=$derived.by(()=>{void workspace;return history.status;});
  onMount(() => { const stop=store.subscribe(next=>workspace=next),stopSaved=store.subscribeSaveNotifications(({message,kind})=>notify(message,kind));if(workspace.view==='list')void store.refresh();return()=>{stop();stopSaved();}; });
  onDestroy(() => { dcsCatalogueEpoch++;companionCatalogueEpoch++;companionDropEpoch++;navigationCancelled?.();runtime.dispose();if(!supplied)store.dispose(); });
  $effect(()=>{void editorIdentity;routingSelection='';});
  $effect(()=>{void editorIdentity;activeVehicleTabId='';addingVehicleTab=false;});
  $effect(()=>{if(activeVehicleTabId&&!vehicleTabs.some(tab=>tab.id===activeVehicleTabId))activeVehicleTabId='';});
  $effect(()=>{void selectedGame;addingVehicleTab=false;customVehicle=false;vehicleError='';});
  $effect(()=>{void editorContext;editingName=false;});
  $effect(()=>{void editorContext;paletteDrag=null;});
  $effect(()=>{void editorContext;void activeVehicleTabId;companionDropEpoch++;companionDropBusy=false;});
  $effect(()=>{if(editor&&!workspace.busy&&!workspace.readOnly&&!libraryBusy&&profileGameContextNeedsSync(editor,editorInventory))untrack(()=>setGameContext(selectedGame));});
  $effect(()=>{const key=dcsCatalogueKey;if(requestedDcsCatalogue===key)return;requestedDcsCatalogue=key;dcsCatalogueEpoch++;dcsCatalogueBusy=false;dcsCatalogueError='';if(key)untrack(()=>void loadDcsVehicleCatalogue());});
  $effect(()=>{if(paletteDrag&&(!active||!canDropComponent(paletteDrag.component)))paletteDrag=null;});
  $effect(()=>{
    if(!active)return;
    const refresh=()=>{if(!document.hidden){if(!store.state.inventoryBusy)void store.refreshInventory();if(!libraryBusy&&!editingName)void store.refreshSaved();void refreshVehicleCatalogue();}};
    refresh();const timer=setInterval(refresh,3000);
    document.addEventListener('visibilitychange',refresh);window.addEventListener('focus',refresh);
    return()=>{clearInterval(timer);document.removeEventListener('visibilitychange',refresh);window.removeEventListener('focus',refresh);};
  });
  function report(operation:()=>void) { notice='';const previous=getNavigation();try{operation();workspace=store.state;if(getNavigation()!==previous)onNavigate?.(getNavigation());}catch(error){notice=error instanceof Error?error.message:String(error);} }
  export function getNavigation():string|null { const current=store.state;return current.view==='editor'?current.editor?.document.id??null:null; }
  export async function restoreNavigation(profileId:string|null):Promise<void> {
    if(getNavigation()===profileId)return;
    if(profileId===null){store.back(true);workspace=store.state;return;}
    await store.open(profileId);
    if(getNavigation()!==profileId)store.back(true);
    workspace=store.state;
  }
  async function openProfile(profileId:string) { await store.open(profileId);workspace=store.state;if(getNavigation()===profileId)onNavigate?.(profileId); }
  function add() { notice='';report(()=>store.beginBlank([])); }
  function showConfirmation(node:HTMLDialogElement) { node.showModal();return()=>node.close(); }
  function change(document:ProfileDocument,assignments?:ProfileLocalAssignment[],label='Edit profile graph',scope=true) { if(scope&&editor)profileStampVehicleNodes(editor.document,document,activeVehicleTabId);report(()=>history.edit(document,assignments,label)); }
  let vehicleCatalogueBusy=false;
  async function refreshVehicleCatalogue(){if(vehicleCatalogueBusy)return;vehicleCatalogueBusy=true;try{const state=await gatewayRequest({command:'game_get_state'});if(state.games?.catalogue)gameCatalogue=state.games.catalogue;}catch{/* Keep saved vehicle choices available when disconnected. */}finally{vehicleCatalogueBusy=false;}}
  async function loadDcsVehicleCatalogue(){
    const tab=activeVehicleTab,profileId=editor?.document.id;if(!tab||selectedGame!=='dcs')return;
    const epoch=++dcsCatalogueEpoch;dcsCatalogueBusy=true;dcsCatalogueError='';
    try{
      const source=await resources.dcsVehicle(tab.vehicleId);if(epoch!==dcsCatalogueEpoch)return;
      dcsCatalogues={...dcsCatalogues,[source.id]:source};dcsCatalogueRevision++;
      const current=store.state.editor;if(!current||current.document.id!==profileId||!current.document.vehicleTabs?.some(current=>current.id===tab.id&&current.vehicleId===tab.vehicleId))return;
      if(!workspace.readOnly&&!workspace.busy&&!libraryBusy){
        const configured=current.document.resources.some(resource=>resource.kind==='game'&&tab.nodeIds.includes(resource.id)&&resource.ports.length&&current.assignments.find(assignment=>assignment.resourceId===resource.id)?.sourceId!==source.id);
        const ports=configured?(await resources.inspectAll(source.id)).ports:[];if(epoch!==dcsCatalogueEpoch)return;
        const latest=store.state.editor;if(latest&&latest.document.id===profileId&&latest.document.vehicleTabs?.some(current=>current.id===tab.id&&current.vehicleId===tab.vehicleId)){
          const next=profileReconcileDcsVehicleCatalogue(latest,tab.id,source,ports,editorInventory);
          if(JSON.stringify(next.assignments)!==JSON.stringify(latest.assignments))change(next.document,next.assignments,'Select aircraft catalogue',false);
        }
      }
      await store.refreshInventory();
    }catch(error){if(epoch===dcsCatalogueEpoch)dcsCatalogueError=error instanceof Error?error.message:String(error);}
    finally{if(epoch===dcsCatalogueEpoch)dcsCatalogueBusy=false;}
  }
  function selectTab(id:string){activeVehicleTabId=id;addingVehicleTab=false;customVehicle=false;vehicleError='';routingSelection='';}
  function beginVehicleTab(){addingVehicleTab=true;customVehicle=false;vehicleError='';routingSelection='';vehicleIdDraft='';vehicleNameDraft='';void tick().then(()=>vehicleSelect?.focus());}
  function chooseVehicle(id:string){if(id==='__custom'){customVehicle=true;vehicleIdDraft=activeVehicleTab?.vehicleId??'';vehicleNameDraft=activeVehicleTab?.vehicleName??'';return;}const vehicle=vehicleChoices.find(vehicle=>vehicle.id===id);if(vehicle)saveVehicleChoice(vehicle);}
  function saveVehicleChoice(vehicle:{id:string;name:string}){if(!editor||!selectedGame)return;vehicleError='';try{if(addingVehicleTab){const next=profileAddVehicleTab(editor.document,selectedGame,vehicle),integrated=profileAddVehicleIntegration({document:next.document,assignments:editor.assignments},next.tabId,editorInventory);change(integrated.document,integrated.assignments,'Add vehicle tab',false);activeVehicleTabId=next.tabId;addingVehicleTab=false;}else if(activeVehicleTab){const next=profileChangeVehicleIntegration(editor,activeVehicleTab.id,vehicle,editorInventory);change(next.document,next.assignments,'Change tab vehicle',false);}customVehicle=false;}catch(error){vehicleError=error instanceof Error?error.message:String(error);}}
  function saveCustomVehicle(event:SubmitEvent){event.preventDefault();saveVehicleChoice({id:vehicleIdDraft.trim(),name:vehicleNameDraft.trim()||vehicleIdDraft.trim()});}
  function removeVehicleTab(id:string){if(!editor)return;const next=profileRemoveVehicleTab(editor.document,editor.assignments,id);change(next.document,next.assignments,'Remove vehicle tab',false);if(activeVehicleTabId===id){selectTab('');void tick().then(()=>vehicleTabsElement?.querySelector<HTMLButtonElement>('[data-tab-index="0"]')?.focus());}}
  function name(value:string) { if(!editor)return;const next=JSON.parse(JSON.stringify(editor.document)) as ProfileDocument;next.name=value;change(next,undefined,'Rename profile'); }
  function editName() { if(!canRename||!editor)return;nameDraft=editor.document.name;editingName=true; }
  function focusName(node:HTMLInputElement) { node.focus();node.select(); }
  function finishName() { if(!editingName)return;editingName=false;const title=nameDraft.trim();if(canRename&&title!==editor?.document.name)name(title); }
  function historyStep(direction:'undo'|'redo'){report(()=>{history.step(direction);});}
  function setGameContext(gameId:GameId|'') { if(!editor||workspace.busy||workspace.readOnly||libraryBusy)return;report(()=>{const next=profileSetGameContext(editor,gameId,editorInventory);change(next.document,next.assignments,'Change game context',false);if(gameId!==selectedGame)selectTab('');}); }
  export function requestNavigation(continueNavigation:()=>void,cancelNavigation?:()=>void,preserveCleanEditor=false):void {
    finishName();
    notice='';
    navigationCancelled?.();navigationCancelled=undefined;navigationGuard.keepEditing();
    if(libraryBusy){notice='Wait for the current library operation before leaving.';cancelNavigation?.();return;}
    const current=store.state;
    if(preserveCleanEditor&&!current.busy&&current.editor&&!profileEditorDirty(current.editor)){leaveDialog=false;continueNavigation();return;}
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
    return !!editor&&!addingVehicleTab&&!workspace.busy&&!workspace.readOnly&&!libraryBusy&&!companionDropBusy&&profilePaletteCanAdd(component,editor.document,editor.assignments,workspace.inventory,activeVehicleTabId)&&(component.kind!=='companion-panel'||!!companionCatalogue?.panels.some(panel=>panel.publicationHash===component.publicationHash&&!profilePalettePanelReason(panel,workspace.inventory.find(source=>source.id===component.sourceId)!)));
  }
  async function refreshCompanionPanels(){
    if(!active||companionPanelsBusy)return;const epoch=++companionCatalogueEpoch;companionPanelsBusy=true;companionPanelsError='';
    try{const next=await companionAuthoring.catalogue();if(epoch===companionCatalogueEpoch)companionCatalogue=next;}
    catch(error){if(epoch===companionCatalogueEpoch)companionPanelsError=error instanceof Error?error.message:String(error);}
    finally{if(epoch===companionCatalogueEpoch)companionPanelsBusy=false;}
  }
  async function addCompanionPanel(sourceId:string,publicationHash:string,position?:CanvasPoint){
    const component:ProfilePaletteComponent={kind:'companion-panel',sourceId,publicationHash};if(!editor||!canDropComponent(component))return;
    const epoch=++companionDropEpoch,context=editorContext,editorEpoch=store.editorEpoch,profileId=editor.document.id,tabId=activeVehicleTabId,originalSource=workspace.inventory.find(source=>source.id===sourceId)!;companionDropBusy=true;notice='';
    try{
      const catalogue=await companionAuthoring.catalogue(),panel=latestCompanionPanels(catalogue.panels).find(panel=>panel.publicationHash===publicationHash);
      if(!panel)throw new Error('This saved panel changed while dragging. Refresh panels and drop its current version.');
      const reason=profilePalettePanelReason(panel,originalSource);if(reason)throw new Error(reason);
      const page=await companionAuthoring.inspect(sourceId,publicationHash,0,32);
      if(page.capabilityFingerprint!==panel.capabilityFingerprint)throw new Error('This panel’s declared ports changed. Refresh saved panels before dropping it.');
      if(epoch!==companionDropEpoch||context!==editorContext||editorEpoch!==store.editorEpoch||profileId!==store.state.editor?.document.id||tabId!==activeVehicleTabId||!active)return;
      const current=store.state.editor,source=store.state.inventory.find(source=>source.id===sourceId);
      if(!current||!source||source.generation!==originalSource.generation||source.capabilityRevision!==originalSource.capabilityRevision||JSON.stringify(source.attributes)!==JSON.stringify(originalSource.attributes)||workspace.readOnly||workspace.busy||libraryBusy||!profilePaletteCanAdd(component,current.document,current.assignments,store.state.inventory,tabId))throw new Error('The profile or paired companion changed. Drop the panel again after refreshing.');
      const previous=new Set(current.document.resources.map(resource=>resource.id)),added=addProfileResources(JSON.parse(JSON.stringify(current.document)),JSON.parse(JSON.stringify(current.assignments)),[source],tabId),node=added.document.resources.find(resource=>!previous.has(resource.id))!;
      node.name=[...panel.title].slice(0,80).join('');const captured=graphCaptureCompanionPanel(added,node.id,panel);
      captured.document=profilePlaceComponent(current.document,captured.document,node.id,position,tabId);companionCatalogue=catalogue;change(captured.document,captured.assignments,'Add companion panel');
    }catch(error){if(epoch===companionDropEpoch)notice=error instanceof Error?error.message:String(error);}
    finally{if(epoch===companionDropEpoch)companionDropBusy=false;}
  }
  function addResource(resource:DeepReadonly<ResourceSummary>,position?:CanvasPoint) {
    if(!editor||!canDropComponent({kind:'resource',sourceId:resource.id}))return;
    report(()=>{const before=editor!.document,previous=new Set(before.resources.map(resource=>resource.id));const next=addProfileResources(JSON.parse(JSON.stringify(before)),JSON.parse(JSON.stringify(editor!.assignments)),[resource],activeVehicleTabId);const nodeId=next.document.resources.find(resource=>!previous.has(resource.id))!.id;change(profilePlaceComponent(before,next.document,nodeId,position,activeVehicleTabId),next.assignments,'Add component');});
  }
  function dropComponent(component:ProfilePaletteComponent,position:CanvasPoint){
    if(!canDropComponent(component))return;
    if(component.kind==='virtual')routingEditor?.addVirtual(position);
    else if(component.kind==='desktop')routingEditor?.addDesktop(position);
    else if(component.kind==='transformation')routingEditor?.addTransformation(component.preset,position);
    else if(component.kind==='companion-panel')void addCompanionPanel(component.sourceId,component.publicationHash,position);
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
    const gameId=supportedGames.find(game=>game.id===profile.gameIds?.[0])?.id;
    await runtime.apply({id:profile.id,revision:profile.revision,baseRevision:profile.revision,dirty:false,readOnly:workspace.readOnly},gameId);
  }
</script>

{#snippet runtimeTools()}
  <ProfileRuntimeControls controller={runtime} profiles={workspace.profiles} compact {active} onState={next=>runtimeState=next}/>
{/snippet}

<section class="profiles-workspace" aria-label="Profiles">
  <div class="profile-content">
  {#if workspace.view==='list'}
    <div class="page-heading"><div><div class="eyebrow">YOUR OPERATING SETUPS</div><h1>Profiles</h1><p>Edit Default or create an independent profile for your rig.</p></div><div class="page-actions"><ProfileAutomaticControls profiles={workspace.profiles} {active}/><button class="secondary-button" disabled={workspace.busy||libraryBusy||workspace.readOnly||rows.length>=32} onclick={()=>libraryTools?.askImport()}><Icon name="upload" size={14}/>Import profile</button><button class="primary-button" disabled={workspace.busy||libraryBusy||workspace.readOnly} onclick={add}><Icon name="plus" size={14}/> Add profile</button></div></div>
  {:else if editor}
    <button class="text-button back-profiles" disabled={workspace.busy} onclick={back}>← Back to Profiles</button>
    <div class="editor-title">
      <div class="profile-heading-copy"><p class="eyebrow">{editor.baseRevision===null?'NEW PROFILE DRAFT':'PROFILE EDITOR'}</p><h1 aria-label={editor.document.name||'Untitled profile'}>{#if editingName}<input class="profile-name-input" aria-label="Profile name" maxlength="80" bind:value={nameDraft} use:focusName onblur={finishName} onkeydown={event=>{if(event.key==='Enter'){event.preventDefault();finishName();}else if(event.key==='Escape'){event.preventDefault();editingName=false;}}}/>{:else if editor.document.id!==DEFAULT_PROFILE_ID}<button class="profile-title-button" aria-label="Edit profile name" title="Edit profile name" disabled={!canRename} onclick={editName}><span>{editor.document.name||'Untitled profile'}</span><Icon name="pencil" size={18}/></button>{:else}{editor.document.name}{/if}</h1><p>{dirty?'Unsaved changes. ':''}{appliedSnapshot?.profileId===editor.document.id?'Saving updates this active profile.':'Activate this profile when ready.'}</p></div>
      <div class="editor-save-actions">
        <div class="history-tools" role="group" aria-label="Edit history">
          <button class="icon-button" aria-label="Undo" title={historyStatus.undoLabel?`Undo ${historyStatus.undoLabel}`:'Undo'} disabled={workspace.busy||workspace.readOnly||libraryBusy||!historyStatus.canUndo} onclick={()=>historyStep('undo')}><Icon name="undo" size={18}/></button>
          <button class="icon-button" aria-label="Redo" title={historyStatus.redoLabel?`Redo ${historyStatus.redoLabel}`:'Redo'} disabled={workspace.busy||workspace.readOnly||libraryBusy||!historyStatus.canRedo} onclick={()=>historyStep('redo')}><Icon name="redo" size={18}/></button>
        </div>
        <label class="game-context" title="Saved with the profile. Selecting a context adds its integration and makes this profile available for that game.">Game context<select aria-label="Game context" value={selectedGame} disabled={workspace.busy||workspace.readOnly||libraryBusy} onchange={event=>setGameContext(event.currentTarget.value as GameId|'')}><option value="">None</option>{#each supportedGames as game (game.id)}<option value={game.id}>{game.name}</option>{/each}</select></label>
        <button class="secondary-button" aria-label="Export saved profile" title="Export saved profile" disabled={workspace.busy||libraryBusy||!savedEditor} onclick={()=>savedEditor&&libraryTools?.askExport(savedEditor)}><Icon name="download" size={14}/>Export</button>
        {#if editor.baseRevision===null}<button class="text-button" disabled={workspace.busy} onclick={cancel}>Cancel creation</button>{/if}
        <button class="primary-button" disabled={workspace.busy||workspace.readOnly||!dirty||!!error} onclick={()=>save()}><Icon name="save" size={14}/> {workspace.busy?'Saving…':'Save profile'}</button>
        {#if savedEditor&&!savedEditor.isDefault}<button class="secondary-button destructive-button" aria-label="Delete saved profile" disabled={workspace.busy||libraryBusy||workspace.readOnly} onclick={()=>libraryTools?.askDelete(savedEditor!)}><Icon name="trash" size={14}/>Delete</button>{/if}
      </div>
    </div>
  {/if}
  {@render runtimeTools()}
  {#if workspace.diagnostics.length}<div class="profile-diagnostics" role="status">{#each workspace.diagnostics as diagnostic,index (index)}<p>{diagnostic}</p>{/each}</div>{/if}
  {#if workspace.readOnly}<p class="workspace-error" role="status">This library is read-only. Resolve its diagnostics before making changes.</p>{/if}
  {#if notice||workspace.error}<p class="workspace-error" role="alert">{notice||workspace.error}</p>{/if}
  {#if workspace.view==='list'}
    <div class="list-tools"><span>{rows.length} {rows.length===1?'profile':'profiles'}</span></div>
    <ul class="profile-list" aria-label="Saved profiles">{#each rows as profile (profile.id)}<li class="profile-tile" class:profile-active={activeProfileId===profile.id} class:profile-muted={!!activeProfileId&&activeProfileId!==profile.id}><button class="profile-row" disabled={workspace.busy||libraryBusy} onclick={()=>openProfile(profile.id)} title={profile.name} aria-label={`Edit ${profile.name}`}><span class="profile-tile-heading"><span class="profile-mark"><Icon name="network" size={22}/></span><span class="profile-badges">{#each profile.gameIds??[] as gameId (gameId)}<span class="profile-game-badge" title={`Game context: ${profileGameName(gameId)}`}><Icon name="gamepad" size={12}/>{profileGameName(gameId)}</span>{/each}{#if activeProfileId===profile.id}<span>Active</span>{/if}</span></span><span class="profile-copy"><strong>{profile.name}</strong>{#if profile.isDefault}<span class="built-in-badge">Built-in</span>{/if}</span></button><div class="profile-row-actions"><button class="secondary-button activate-profile" title={runtimeState.status?.owner?.profileId===profile.id?'Deactivate profile':profile.gameIds?.length?'Activate saved profile for its game context':'Activate saved profile (hardware only)'} aria-label={runtimeState.status?.owner?.profileId===profile.id?`Deactivate ${profile.name}`:`Activate ${profile.name}`} disabled={runtimeState.status?.owner?.profileId===profile.id?runtimeState.pending==='stop':activationDisabled(profile)} onclick={()=>runtimeState.status?.owner?.profileId===profile.id?runtime.stop():activateProfile(profile)}><Icon name={runtimeState.status?.owner?.profileId===profile.id?'stop':'play'} size={14}/>{runtimeState.status?.owner?.profileId===profile.id?'Deactivate':'Activate'}</button><button class="icon-button" title="Export profile" aria-label={`Export ${profile.name}`} disabled={workspace.busy||libraryBusy} onclick={()=>libraryTools?.askExport(profile)}><Icon name="download" size={16}/></button>{#if !profile.isDefault}<button class="icon-button delete-profile" title="Delete profile" aria-label={`Delete ${profile.name}`} disabled={workspace.busy||libraryBusy||workspace.readOnly} onclick={()=>libraryTools?.askDelete(profile)}><Icon name="trash" size={16}/></button>{/if}</div></li>{:else}<li class="empty-profiles">{workspace.busy?'Loading profiles…':'No saved profiles are available.'}</li>{/each}</ul>
    {#if workspace.inventoryError}<p class="workspace-error" role="status">{workspace.inventoryError}</p><button class="secondary-button" disabled={workspace.inventoryBusy} onclick={()=>store.refreshInventory()}>Refresh resource inventory</button>{/if}
  {:else if editor}
    {#if editor.baseRevision===null}<div class="editor-basics panel"><fieldset class="draft-source"><legend>Start from</legend><button class="secondary-button" disabled={workspace.busy||workspace.readOnly} onclick={()=>source('blank')}>Blank profile</button><label>Copy saved profile<select aria-label="Copy source profile" bind:value={copyId} disabled={workspace.busy}>{#each rows as profile (profile.id)}<option value={profile.id}>{profile.name}</option>{/each}</select></label><button class="secondary-button" disabled={workspace.busy||workspace.readOnly||!copyId} onclick={()=>source(copyId)}>Use copy</button><p>Copies retain the graph and layout with a new identity. Select devices for the copy; machine assignments are excluded.</p></fieldset></div>{/if}
    {#if error}<p class="workspace-error" role="alert">{error}</p>{/if}
    <div class="profile-authoring">
      <ProfileComponentPalette inventory={workspace.inventory} {assignedIds} editorGroups={tabDocument?.editor.groups??[]} canAddGroup={!activeVehicleTabId} addGroup={()=>{if(!activeVehicleTabId)routingEditor?.addGroup();}} focusGroup={id=>routingEditor?.focusGroup(id)} renameGroup={(id,name)=>routingEditor?.renameGroup(id,name)} deleteGroup={id=>routingEditor?.deleteGroup(id)} resourceCount={editor.document.resources.length} virtualCount={editor.document.virtualOutputs.filter(node=>profileTabNodeIds(editor.document).has(node.id)||tabNodeIds.has(node.id)).length} desktopAdded={!!editor.document.desktopOutputs?.some(node=>profileTabNodeIds(editor.document).has(node.id)||tabNodeIds.has(node.id))}
        processingCount={editor.document.processingNodes.length+(editor.document.desktopOutputs??[]).reduce((count,node)=>count+node.actions.length,0)} addTransformation={preset=>{if(canDropComponent({kind:'transformation',preset}))routingEditor?.addTransformation(preset);}}
        {capturedCompanionIds} companionPanelCount={editor.document.companionAssignments.length} companionPanels={companionCatalogue?.panels??[]} companionPanelsLoaded={!!companionCatalogue} companionPanelsBusy={companionPanelsBusy||companionDropBusy} {companionPanelsError} refreshPanels={()=>void refreshCompanionPanels()} addCompanionPanel={(sourceId,publicationHash)=>void addCompanionPanel(sourceId,publicationHash)}
        disabled={workspace.readOnly||workspace.busy||libraryBusy||addingVehicleTab||companionDropBusy} refreshing={workspace.inventoryBusy} error={workspace.inventoryError} refresh={()=>{void store.refreshInventory();if(companionCatalogue)void refreshCompanionPanels();}} addResource={resource=>addResource(resource)} addVirtual={()=>{if(canDropComponent({kind:'virtual'}))routingEditor?.addVirtual();}} addDesktop={()=>{if(canDropComponent({kind:'desktop'}))routingEditor?.addDesktop();}} startDrag={startPaletteDrag} endDrag={endPaletteDrag}/>
      <div class="profile-canvas-workspace">
        {#if vehicleTabsEnabled&&!addingVehicleTab}<ProfileVehicleConflicts document={editor.document} assignments={editor.assignments} {activeVehicleTabId} {compiler} {inventoryRevision} {active}/>{/if}
        {#if vehicleTabsEnabled}
          <div class="vehicle-tab-bar">
            <div class="vehicle-tabs" role="group" aria-label="Profile vehicle tabs" bind:this={vehicleTabsElement}>
              <button type="button" class="vehicle-tab vehicle-base" class:active={!activeVehicleTabId&&!addingVehicleTab} data-tab-index="0" aria-pressed={!activeVehicleTabId&&!addingVehicleTab} aria-controls="profile-vehicle-canvas" onclick={()=>selectTab('')}>Base</button>
              {#each vehicleTabs as tab,index (tab.id)}
                <div class="vehicle-tab" class:active={activeVehicleTabId===tab.id&&!addingVehicleTab}>
                  <button type="button" class="vehicle-tab-label" aria-label={tab.vehicleName} aria-controls="profile-vehicle-canvas" data-vehicle-tab-id={tab.id} data-tab-index={index+1} aria-pressed={activeVehicleTabId===tab.id&&!addingVehicleTab} onclick={()=>selectTab(tab.id)}>{tab.vehicleName}</button>
                  <button type="button" class="remove-vehicle-tab" aria-label={`Remove ${tab.vehicleName} tab`} title="Remove tab and all its nodes" disabled={workspace.readOnly||workspace.busy||libraryBusy} onclick={()=>removeVehicleTab(tab.id)}><Icon name="close" size={12}/></button>
                </div>
              {/each}
              {#if addingVehicleTab}
                <div class="vehicle-tab active">
                  <span class="vehicle-tab-label">New vehicle</span>
                  <button type="button" class="remove-vehicle-tab" aria-label="Cancel new vehicle tab" title="Cancel new vehicle tab" onclick={()=>selectTab(activeVehicleTabId)}><Icon name="close" size={12}/></button>
                </div>
              {/if}
            </div>
            <button type="button" class="add-vehicle-tab" aria-label="Add vehicle tab" title="Add vehicle tab" disabled={workspace.readOnly||workspace.busy||libraryBusy||vehicleTabs.length>=32} onclick={beginVehicleTab}><Icon name="plus" size={15}/></button>
          </div>
        {/if}
        <div id="profile-vehicle-canvas" class:vehicle-content={vehicleTabsEnabled} role={vehicleTabsEnabled?'region':undefined} aria-label={vehicleTabsEnabled?(addingVehicleTab?'New vehicle':activeVehicleTab?.vehicleName??'Base'):undefined}>
          {#if vehicleTabsEnabled}
            <div class="vehicle-settings">
              <div class="vehicle-context">
                {#if addingVehicleTab||activeVehicleTab}<label>Vehicle<select aria-label="Tab vehicle" data-vehicle-tab-id={addingVehicleTab?undefined:activeVehicleTab?.id} value={addingVehicleTab?'':activeVehicleTab?.vehicleId??''} disabled={workspace.readOnly||workspace.busy||libraryBusy} bind:this={vehicleSelect} onchange={event=>chooseVehicle(event.currentTarget.value)}>
                  {#if addingVehicleTab}<option value="" disabled>Choose a vehicle…</option>{/if}
                  {#each vehicleChoices.filter(vehicle=>!vehicleTabs.some(tab=>(addingVehicleTab||tab.id!==activeVehicleTabId)&&tab.vehicleId===vehicle.id)) as vehicle (vehicle.id)}<option value={vehicle.id}>{vehicle.name}</option>{/each}
                  <option value="__custom">Enter exact vehicle ID…</option>
                </select></label><p>Applies with Base while this vehicle is loaded. Conflicts are checked against Base.</p>{:else}<p>Base applies to every vehicle.</p>{/if}
                {#if selectedGame==='dcs'&&!addingVehicleTab}<span class="vehicle-hint">DCS-BIOS catalogue: {activeVehicleTab?tabDcsModule??'Waiting for aircraft catalogue':'CommonData (shared flight telemetry)'}</span>{/if}
                {#if contextGame?.currentVehicle}<span class="current-vehicle">Loaded: {contextGame.currentVehicle.name}</span>{/if}
              </div>
              {#if (addingVehicleTab||activeVehicleTab)&&customVehicle}<form class="vehicle-custom" onsubmit={saveCustomVehicle}><label>Exact vehicle ID<input aria-label="Exact vehicle ID" required maxlength="256" disabled={workspace.readOnly||workspace.busy||libraryBusy} bind:value={vehicleIdDraft}/></label><label>Vehicle name<input aria-label="Vehicle name" maxlength="80" disabled={workspace.readOnly||workspace.busy||libraryBusy} bind:value={vehicleNameDraft} placeholder="Same as ID"/></label><button class="secondary-button" type="submit" disabled={workspace.readOnly||workspace.busy||libraryBusy}>Use vehicle</button></form>{/if}
              {#if addingVehicleTab&&contextGame?.vehicleSupport?.message}<p class="vehicle-hint">{contextGame.vehicleSupport.message}</p>{/if}
              {#if vehicleError}<p class="workspace-error" role="alert">{vehicleError}</p>{/if}
              {#if dcsCatalogueBusy}<p class="vehicle-hint" role="status">Loading this aircraft’s controls and telemetry…</p>{/if}
              {#if dcsCatalogueError}<p class="workspace-error" role="alert">{dcsCatalogueError}</p><button class="secondary-button" disabled={dcsCatalogueBusy} onclick={()=>void loadDcsVehicleCatalogue()}>Retry aircraft catalogue</button>{/if}
            </div>
          {/if}
          {#if addingVehicleTab}<div class="vehicle-empty"><Icon name="plus" size={24}/><p>Choose a vehicle to start configuring its canvas.</p></div>{:else}{#key `${editor.document.id}/${activeVehicleTabId}`}<ProfileRoutingEditor bind:this={routingEditor} document={editor.document} {activeVehicleTabId} assignments={editor.assignments} inventory={routingInventory} inventoryClient={resources} companionClient={companionAuthoring} compilerClient={compiler} {learningClient} {editorContext} {paletteDrag} {canDropComponent} {dropComponent} {endPaletteDrag} {active} runtimeIdle={learningIdle} {inventoryRevision} readOnly={workspace.readOnly||workspace.busy||libraryBusy||companionDropBusy} bind:selectedId={routingSelection} change={(draft,label)=>change(draft.document,draft.assignments,label)} openGameSettings={openGameSettings?gameSettings:undefined} undo={()=>historyStep('undo')} redo={()=>historyStep('redo')}/>{/key}{/if}
        </div>
      </div>
    </div>
  {/if}
  </div>
  <ProfileLibraryTools bind:this={libraryTools} client={suppliedLibrary??profiles} profileCount={rows.length} readOnly={workspace.readOnly} busy={workspace.busy} prepare={prepareLibrary} resolveProfile={id=>workspace.profiles.find(row=>row.id===id)} onDeleted={libraryDeleted} onImported={libraryImported} onBusy={value=>libraryBusy=value}/>
  {#if runtimeState.activationConfirmation}<dialog class="editor-confirmation" aria-label="Switch active profile?" oncancel={event=>{event.preventDefault();runtime.cancelActivation();}} {@attach showConfirmation}><h2>Switch active profile?</h2><p>Deactivate {workspace.profiles.find(profile=>profile.id===runtimeState.activationConfirmation?.previousOwner.profileId)?.name??'the active profile'} and activate {workspace.profiles.find(profile=>profile.id===runtimeState.activationConfirmation?.target.id)?.name??'the new profile'}?</p><div><button class="secondary-button" onclick={()=>runtime.cancelActivation()}>Cancel</button><button class="primary-button" disabled={!!runtimeState.pending||runtimeState.stale} onclick={()=>runtime.confirmActivation()}>Switch profile</button></div></dialog>{/if}
  {#if leaveDialog}<dialog class="editor-confirmation" role="alertdialog" aria-modal="true" aria-labelledby="leave-profile-title" onclose={keepEditing} {@attach showConfirmation}><h2 id="leave-profile-title">Keep your unsaved changes?</h2><p>Save this profile before leaving, or discard your changes. Saving updates it immediately if it is active.</p>{#if workspace.error}<p class="workspace-error" role="alert">{workspace.error}</p>{/if}<div><button class="secondary-button" onclick={keepEditing}>Keep editing</button><button class="secondary-button" disabled={workspace.busy} onclick={cancel}>Discard and continue</button><button class="primary-button" disabled={!!error||workspace.readOnly||workspace.busy} onclick={()=>save(true)}>Save and continue</button></div></dialog>{/if}
  {#if replaceSource}<dialog class="editor-confirmation" role="alertdialog" aria-modal="true" aria-labelledby="replace-profile-title" onclose={()=>replaceSource=null} {@attach showConfirmation}><h2 id="replace-profile-title">Replace this new draft?</h2><p>Your unsaved edits will be discarded before starting from {replaceSource==='blank'?'an empty profile':'the selected copy'}.</p><div><button class="secondary-button" onclick={()=>replaceSource=null}>Keep this draft</button><button class="primary-button" onclick={()=>replace(replaceSource!)}>Replace draft</button></div></dialog>{/if}
</section>

<style>
  .history-tools{display:flex;align-items:center;gap:4px;padding-right:12px;border-right:1px solid var(--line)}.history-tools button{display:grid;place-items:center;width:34px;height:34px;padding:0;background:transparent;color:var(--muted);border:1px solid var(--line);border-radius:5px}.history-tools button:hover:enabled{color:var(--text);background:var(--surface-raised)}.history-tools button:focus-visible{outline:2px solid var(--accent);outline-offset:2px}
  .vehicle-tab-bar{display:flex;align-items:stretch;gap:5px;border-bottom:1px solid var(--line);margin-bottom:0}.vehicle-tabs{display:flex;align-items:stretch;gap:4px;overflow-x:auto;min-width:0}.vehicle-tab{display:flex;align-items:stretch;flex-shrink:0;overflow:hidden;border:1px solid var(--line);border-bottom:2px solid transparent;border-radius:7px 7px 0 0;background:var(--surface);color:var(--muted);font-size:11px}.vehicle-tab.active{border-bottom-color:var(--accent);color:var(--accent);background:var(--surface-raised)}.vehicle-base{padding:11px 16px;white-space:nowrap}.vehicle-tab-label{min-width:100px;max-width:280px;box-sizing:border-box;border:0;border-radius:0;padding:11px 13px;background:transparent;color:inherit;font:inherit;text-align:left;overflow:hidden;text-overflow:ellipsis;white-space:nowrap}button.vehicle-tab-label{cursor:pointer}.remove-vehicle-tab,.add-vehicle-tab{display:grid;place-items:center;background:transparent;color:var(--muted);padding:8px}.remove-vehicle-tab{width:30px;flex-shrink:0;border-left:1px solid var(--line)}.remove-vehicle-tab:hover:enabled{color:var(--danger,#e67979);background:var(--surface)}.add-vehicle-tab{flex-shrink:0;min-width:34px}.vehicle-settings{padding:14px;border:1px solid var(--line);border-top:0;border-bottom:0;background:var(--surface)}.vehicle-content :global(.profile-graph){border-top-left-radius:0;border-top-right-radius:0}.vehicle-context{display:flex;align-items:center;gap:12px;flex-wrap:wrap}.vehicle-context label{display:grid;gap:5px;font-size:10px;color:var(--muted)}.vehicle-context select{min-width:180px;max-width:100%;padding:8px 12px;background:var(--input-bg);color:var(--text);border:1px solid var(--line);border-radius:5px;font:inherit}.vehicle-context p{margin:0}.vehicle-custom label{display:grid;gap:5px;font-size:10px;color:var(--muted)}.vehicle-custom input{background:var(--input-bg);color:var(--text);border:1px solid var(--line);border-radius:5px;padding:8px;max-width:260px;font-size:11px}.vehicle-context p,.vehicle-hint{font-size:10px;color:var(--muted);line-height:1.6}.current-vehicle{margin-left:auto;font-size:10px;color:var(--accent)}.vehicle-custom{display:flex;align-items:end;gap:10px;flex-wrap:wrap;margin-top:14px}.vehicle-custom button{font-size:10px}.vehicle-empty{display:grid;justify-items:center;align-content:center;min-height:430px;border:1px solid var(--line);border-top:0;border-radius:0 0 7px 7px;color:var(--muted);font-size:12px}.vehicle-tabs button:focus-visible,.add-vehicle-tab:focus-visible{outline:2px solid var(--accent);outline-offset:-3px}
  .profile-game-badge{display:inline-flex;align-items:center;gap:5px;min-width:0;max-width:100%}
  .game-context{display:grid;gap:5px;font-size:9px;color:var(--muted)}.game-context select{min-width:180px;max-width:220px;padding:9px 12px;font-size:11px;background:var(--input-bg);color:var(--text);border:1px solid var(--line);border-radius:5px}
  .profile-authoring{display:grid;grid-template-columns:236px minmax(0,1fr);align-items:start;gap:18px}.profile-canvas-workspace{min-width:0}@media(max-width:1050px){.profile-authoring{grid-template-columns:minmax(0,1fr)}}
  .profile-heading-copy{min-width:0;flex:1}.profile-title-button{display:inline-flex;align-items:center;gap:10px;max-width:100%;padding:0;background:transparent;color:inherit;font:inherit;letter-spacing:inherit;text-align:left}.profile-title-button span{min-width:0;overflow-wrap:anywhere}.profile-title-button :global(svg){flex-shrink:0;color:var(--muted);opacity:0;transition:opacity .15s}.profile-title-button:hover:enabled :global(svg),.profile-title-button:focus-visible :global(svg){opacity:1}.profile-name-input{width:100%;min-width:0;margin:0;padding:0 6px;font:inherit;letter-spacing:inherit;line-height:inherit;background:var(--input-bg);border:1px solid var(--line);border-radius:4px}.editor-save-actions{flex-shrink:0}@media(hover:none){.profile-title-button:enabled :global(svg){opacity:1}}
  .page-actions{display:flex;align-items:center;justify-content:flex-end;gap:18px;flex-wrap:wrap}.profile-tile.profile-active{border-color:var(--accent);background:var(--surface-raised)}.profile-tile.profile-muted{filter:grayscale(1);opacity:.6}.profile-tile.profile-muted:hover,.profile-tile.profile-muted:focus-within{filter:none;opacity:1}
  .profile-row-actions{position:relative;z-index:1;display:flex;align-items:center;justify-content:end;gap:6px;padding:0 16px 12px;}.profile-row-actions button{width:34px;height:34px;padding:0;border:1px solid var(--line);border-radius:5px;}.profile-row-actions button:hover:enabled{border-color:var(--accent);background:var(--surface-raised);}.profile-row-actions .activate-profile{width:auto;min-height:34px;margin-right:auto;padding:0 10px;font-size:10px;color:var(--accent);}.profile-row-actions .delete-profile{color:var(--danger,#e67979);}.profile-row-actions button:focus-visible{outline:2px solid var(--accent);outline-offset:2px;}

  .page-heading,.editor-title{display:flex;align-items:center;justify-content:space-between;gap:20px;margin-bottom:24px;}h1{font-size:26px;letter-spacing:-.5px;margin:8px 0 10px;}h2{font-size:14px;}.page-heading p,.editor-title p{font-size:11px;color:var(--muted);line-height:1.8;}.page-heading button,.editor-save-actions button{display:inline-flex;align-items:center;gap:8px;white-space:nowrap;font-size:11px;}.eyebrow{font-size:8px!important;letter-spacing:1.5px;text-transform:uppercase;color:var(--muted);}.list-tools{display:flex;align-items:center;justify-content:space-between;font-size:10px;color:var(--muted);margin-bottom:12px;}.profile-list{display:grid;grid-template-columns:repeat(auto-fill,minmax(min(100%,260px),1fr));gap:18px;list-style:none;padding:0;margin:0 0 22px;}.profile-tile{position:relative;display:flex;flex-direction:column;min-width:0;min-height:224px;border:1px solid var(--line);border-radius:9px;background:var(--surface);transition:border-color .15s,background .15s;}.profile-tile:hover{border-color:var(--accent);background:var(--surface-raised);}.profile-row{width:100%;flex:1;display:flex;flex-direction:column;align-items:stretch;gap:18px;background:transparent;color:var(--text);padding:20px;text-align:left;border-radius:9px;}.profile-row::after{content:'';position:absolute;inset:0;border-radius:9px;}.profile-row:focus-visible{outline:none;}.profile-row:focus-visible::after{outline:2px solid var(--accent);outline-offset:2px;}.profile-tile-heading{display:flex;align-items:start;justify-content:space-between;gap:14px;}.profile-mark{display:grid;place-items:center;width:43px;height:43px;flex-shrink:0;background:var(--accent-bg);border-radius:9px;color:var(--accent);}.profile-copy{display:flex;align-items:center;gap:8px;min-width:0;}.profile-copy strong{min-width:0;font-size:16px;line-height:1.4;display:block;overflow:hidden;text-overflow:ellipsis;white-space:nowrap;}.built-in-badge{flex-shrink:0;font-size:9px;line-height:1.4;color:var(--muted);border:1px solid var(--line);border-radius:4px;padding:4px 6px;}.profile-badges{display:flex;gap:6px;flex-wrap:wrap;justify-content:end;min-width:0;}.profile-badges span{font-size:9px;line-height:1.4;color:var(--accent);border:1px solid var(--line);border-radius:4px;padding:5px 7px;}.empty-profiles{grid-column:1/-1;border:1px solid var(--line);border-radius:9px;background:var(--surface);padding:30px;color:var(--muted);font-size:12px;}.workspace-error,.profile-diagnostics{border:1px solid #ad875440;color:#c9ab7b;background:#ad875408;border-radius:6px;padding:12px 15px;font-size:10px;line-height:1.8;margin-bottom:16px;}.profile-diagnostics p{margin:0;}.back-profiles{font-size:10px;padding-left:0;margin-bottom:20px;}.editor-save-actions{display:flex;align-items:end;justify-content:flex-end;gap:10px;flex-wrap:wrap;max-width:100%;}.editor-save-actions .destructive-button{background:#b32b38;border-color:#cd4552;color:#fff}.editor-save-actions .destructive-button:hover:enabled{background:#98232e;border-color:#e35d69;color:#fff}.editor-basics{padding:19px;margin-bottom:20px;}.editor-basics label{display:grid;gap:8px;font-size:10px;color:var(--muted);}.draft-source{border:0;margin:0;padding:0;display:flex;align-items:end;gap:12px;flex-wrap:wrap;}.draft-source legend{font-size:10px;color:var(--muted);padding-right:10px;}.draft-source label{min-width:170px;}.draft-source button{font-size:10px;}.draft-source p{flex-basis:100%;font-size:9px;color:var(--muted);line-height:1.8;margin:0;}.editor-confirmation{position:fixed;z-index:30;left:50%;top:50%;transform:translate(-50%,-50%);max-width:460px;width:calc(100% - 40px);padding:26px;background:var(--surface);border:1px solid var(--line);border-radius:10px;box-shadow:0 20px 80px #0005;color:var(--text);margin:0;}.editor-confirmation::backdrop{background:#0009;}.editor-confirmation p{font-size:11px;color:var(--muted);line-height:1.8;margin:15px 0 23px;}.editor-confirmation>div{display:flex;gap:10px;justify-content:end;flex-wrap:wrap;}.editor-confirmation button{font-size:10px;}@media(max-width:700px){.page-heading,.editor-title{align-items:start;flex-direction:column;}.editor-save-actions{align-self:end;}.profile-row{gap:11px;padding:16px;}.profile-copy strong{display:block;}}
</style>
