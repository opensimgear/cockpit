<script lang="ts">
  import { restoreCompanionPanels } from './companion-startup';
  import Notification from './Notification.svelte';
  import WidgetDesigner from './WidgetDesigner.svelte';
  import { onMount, onDestroy, tick, untrack } from 'svelte';
  import Icon from './Icon.svelte';
  import CompanionConnections from './CompanionConnections.svelte';
  import { gatewayRequest } from './gateway';
  import { ProfilesClient } from './profiles';
  import { latestSavedPanels } from './companion-library';
  import { sendPublication, type CompanionCommand, type CompanionHostState } from './companions';
  import { companionPanelReason, knownPanelScreenChoices, panelScreenSizeReason, PANEL_SCREEN_MIN, PANEL_SCREEN_MAX } from './companion-screen';
  import { synchronizePanel, removePanelAssignments } from './panel-actions';
  import {
    newDocument, createDesignerHistory, applyDesignerCommand, applyPixelDesignerCommand, undoDesigner, redoDesigner, DESIGNER_LIMITS,
    createLibrary, saveDraft, duplicateDraft, openDraft,
    importAsset, validateAssets, exportBundle, importBundle, publishDocument,
    viewportRegions, widgetViewport, boundsContain, widgetPresets, createWidgetCatalog, includeBuiltinWidgets, newId, canonical,
    mergeWidgetBundles, importWidgetBundle, exportWidgetBundle, widgetAssetIds,
    type DesignerHistory, type DesignerWidget, type AssetPayload,
    type Publication, type Bounds, type WidgetDefinition, type WidgetLibraryBundle, type WidgetPropertyValue,
  } from '@opensimgear/companion-designer';
  import { PanelCanvas, componentDefinition, type PreviewController, type PreviewSnapshot } from '@opensimgear/companion-ui';

  let { openSettings, initialCompanionId = '', onNavigate, active = true }: { openSettings: () => void; initialCompanionId?: string; onNavigate?: (page: CompanionNavigation) => void; active?: boolean } = $props();

  let importInput = $state<HTMLInputElement>();
  let assetInput = $state<HTMLInputElement>();
  let widgetImportInput = $state<HTMLInputElement>();
  let widgetBundle = $state.raw<WidgetLibraryBundle>({ bundleVersion: 1, library: { formatVersion: 1, definitions: createWidgetCatalog() }, manifest: [], assets: [] });
  let widgetEditorDefinition = $state.raw<WidgetDefinition | null>(null);
  let widgetEditInstanceId = $state('');
  const availableWidgets = $derived.by(() => {
    const latest = new Map<string, WidgetDefinition>();
    for (const d of widgetBundle.library.definitions) if (!latest.has(d.id) || latest.get(d.id)!.version < d.version) latest.set(d.id, d);
    return [...latest.values()];
  });
  let connectionsPanel = $state.raw<{showMain:(recordNavigation?:boolean)=>boolean;getCompanionId:()=>string;showCompanion:(id:string)=>void;requestNavigation:(continueNavigation:()=>void,cancelNavigation?:()=>void)=>void}>();
  let editorOpen = $state(false);
  let newPanelOpen=$state(false),newPanelName=$state(''),newPanelSource=$state('custom'),newPanelWidth=$state(1280),newPanelHeight=$state(800),newPanelError=$state('');
  let companionDetailsOpen = $state(untrack(() => !!initialCompanionId));
  let dirty = $state(false);
  let hostState = $state.raw<CompanionHostState>({});
  let library = $state.raw(createLibrary());
  let history = $state.raw<DesignerHistory>(createDesignerHistory(newDocument('New panel')));
  let draftId = $state<string>(crypto.randomUUID());
  let name = $state('New panel');
  let editingName = $state(false), nameDraft = $state('');
  let editingViewportId = $state(''), viewportNameDraft = $state('');
  let assets = $state.raw<AssetPayload[]>([]);
  let publications = $state.raw<Record<string, Publication[]>>({});
  let pageId = $state('');
  let selected = $state<string[]>([]);
  let propertiesOpen = $state(false);
  let selectedViewportId = $state('');
  let preview = $state(false);
  let paletteDragging = $state(false);
  let draggedWidget = $state('');
  let controller: PreviewController | undefined;
  let unsubscribe: (() => void) | undefined;
  let previewSnapshot = $state.raw<PreviewSnapshot>();
  let busy = $state(false);
  let pendingLeave = $state.raw<{ proceed:()=>void; cancel?:()=>void } | null>(null);
  let pendingDelete = $state.raw<{ message:string; resolve:(confirmed:boolean)=>void } | null>(null);
  onDestroy(()=>{pendingDelete?.resolve(false);});
  function confirmDelete(message:string):Promise<boolean> {
    pendingDelete?.resolve(false);
    return new Promise(resolve=>pendingDelete={message,resolve});
  }
  function finishDelete(confirmed:boolean) { const pending=pendingDelete;pendingDelete=null;pending?.resolve(confirmed); }
  function showDeleteDialog(node:HTMLDialogElement) { node.showModal();node.querySelector<HTMLButtonElement>('[data-cancel]')?.focus();return {destroy:()=>node.close()}; }
  let message = $state('');
  let error = $state('');
  let configText = $state('');
  let targetText = $state('');
  let storageReady = $state(false);
  let resolveStorageLoad: () => void = () => {};
  const storageLoaded = new Promise<void>(resolve => { resolveStorageLoad = resolve; });
  let navigationRevision = 0;
  let disposed = false;
  let mounted = $state(false);
  let discardPanelOnRestore = false;
  let store: import('./companion-library').CompanionLibraryStore | undefined;
  const document = $derived(history.present);
  const panelScreenChoices=$derived(knownPanelScreenChoices(hostState));
  const chosenPanelScreen=$derived(newPanelSource==='custom'?{width:newPanelWidth,height:newPanelHeight}:panelScreenChoices.find(choice=>choice.id===newPanelSource));
  const newPanelReason=$derived(!newPanelName.trim()?'Enter a panel name.':!chosenPanelScreen?'Choose a companion with a known screen size, or select Custom.':panelScreenSizeReason(chosenPanelScreen.width,chosenPanelScreen.height));
  const page = $derived(document.ui.pages.find(p => p.id === pageId) ?? document.ui.pages[0]);
  const emptyPageNumbers = $derived(document.ui.pages.flatMap((page,index)=>document.presentation.widgets.some(widget=>widget.pageId===page.id)?[]:[index]));
  const assignedTargetError=$derived((hostState.host?.clients??[]).filter(c=>panelAssignments(draftId).some(i=>i.companionId===c.companionId)).map(c=>companionPanelReason({document} as Publication,hostState,String(c.companionId))).find(Boolean)??'');
  const panelInvalidReason = $derived(emptyPageNumbers.length ? `Add at least one widget to ${emptyPageNumbers.length===1?'page':'pages'} ${emptyPageNumbers.join(', ')} before previewing, saving or exporting.` : '');
  const widget = $derived(document.presentation.widgets.find(w => w.id === selected[0]));
  const reusableDefinition = $derived(widget ? componentDefinition(document, widget) : undefined);
  const viewport = $derived(document.presentation.viewports.find(v => v.pageId === page.id)!);
  const regions = $derived(viewportRegions(document, page.id));
  const activeViewport = $derived(regions.find(region => region.id === selectedViewportId) ?? regions[0]);
  const activeViewportOccupied = $derived(document.presentation.widgets.some(w => w.pageId === page.id && boundsContain(activeViewport.bounds, w.bounds)));
  const usableWidth = $derived(viewport.width * (1 - (viewport.safeInsets?.left ?? 0) - (viewport.safeInsets?.right ?? 0)));
  const usableHeight = $derived(viewport.height * (1 - (viewport.safeInsets?.top ?? 0) - (viewport.safeInsets?.bottom ?? 0)));
  const margin = $derived(Math.round((viewport.safeInsets?.left ?? 0) * viewport.width));
  const maxMargin = $derived(Math.ceil(Math.min(viewport.width, viewport.height) / 4) - 1);
  const activeAssets = $derived(assets.filter(payload => document.ui.assets.some(a => a.id === payload.assetId)));
  const imageAssets = $derived(Object.fromEntries(activeAssets.map(payload => [payload.assetId, `data:${document.ui.assets.find(a => a.id === payload.assetId)?.mime};base64,${payload.base64}`])));
  const widgetEditorAssets = $derived({ ...Object.fromEntries(widgetBundle.assets.map(payload => [payload.assetId, `data:${widgetBundle.manifest.find(a => a.id === payload.assetId)?.mime};base64,${payload.base64}`])), ...imageAssets });
  const widgetAssetOptions = $derived([...new Map([...widgetBundle.manifest, ...document.ui.assets].map(a => [a.id, a])).values()].map((a, i) => ({ id: a.id, label: `Image ${i + 1}` })));
  const currentPublications = $derived(latestSavedPanels(library,publications));
  function panelHashes(id: string) { return (publications[id] ?? []).map(p => p.publicationHash); }
  function panelAssignments(id: string) {
    const hashes = new Set(panelHashes(id));
    const uiId=library.drafts.find(d=>d.id===id)?.document.ui.id;
    for(const panel of hostState.host?.publications??[])if(uiId&&panel.uiId===uiId&&typeof panel.publicationHash==='string')hashes.add(panel.publicationHash);
    return (hostState.host?.instances ?? []).filter(i => typeof i.publicationHash==='string' && hashes.has(i.publicationHash));
  }
  function leaveEditor() { return !dirty || window.confirm('Discard unsaved panel changes?'); }
  function askLeave(proceed:()=>void,cancel?:()=>void) {
    if (busy) { cancel?.();return; }
    if (!editorOpen || !dirty) { proceed();return; }
    pendingLeave?.cancel?.();pendingLeave={proceed,cancel};
  }
  function keepEditing() { if(busy)return;const pending=pendingLeave;pendingLeave=null;pending?.cancel?.(); }
  function continueLeaving(discard=false) {
    const pending=pendingLeave;pendingLeave=null;
    if(discard){discardPanelOnRestore=editorOpen;dirty=false;}
    pending?.proceed();
  }
  async function saveAndContinue() { await save();if(!dirty&&!busy)continueLeaving(); }
  function showLeaveDialog(node:HTMLDialogElement) { node.showModal();return {destroy:()=>node.close()}; }
  function backToPanels() { askLeave(()=>{stop();editorOpen=false;dirty=false;discardPanelOnRestore=false;onNavigate?.({kind:'main'});}); }
  export function showMain() {
    if (editorOpen && dirty) { askLeave(()=>{dirty=false;showMain();});return false; }
    if (connectionsPanel && !connectionsPanel.showMain(false)) return false;
    stop(); editorOpen=false; companionDetailsOpen=false; dirty=false; discardPanelOnRestore=false;
    onNavigate?.({kind:'main'});
    return true;
  }
  export function getNavigation(): CompanionNavigation {
    if (editorOpen) return {kind:'panel',id:draftId};
    const companionId=connectionsPanel?.getCompanionId() ?? initialCompanionId;
    return companionId ? {kind:'companion',id:companionId} : {kind:'main'};
  }
  export function requestNavigation(continueNavigation: () => void, cancelNavigation?: () => void): void {
    askLeave(()=>{
      const approve=() => { discardPanelOnRestore=discardPanelOnRestore || editorOpen && dirty; dirty=false;stop();continueNavigation(); };
      if (connectionsPanel) connectionsPanel.requestNavigation(approve,cancelNavigation);
      else approve();
    },cancelNavigation);
  }
  export async function restoreNavigation(target: CompanionNavigation): Promise<void> {
    const revision=++navigationRevision;
    await tick();
    if (target.kind==='panel') await storageLoaded;
    if (disposed || revision!==navigationRevision) return;
    if (target.kind==='panel' && editorOpen && draftId===target.id && !discardPanelOnRestore) return;
    stop();
    if (target.kind==='companion') {
      editorOpen=false; dirty=false; discardPanelOnRestore=false; companionDetailsOpen=true;
      connectionsPanel?.showCompanion(target.id);
      return;
    }
    connectionsPanel?.showMain(false);
    editorOpen=false; companionDetailsOpen=false; dirty=false; discardPanelOnRestore=false;
    if (target.kind==='panel') {
      if (storageReady && library.drafts.some(entry=>entry.id===target.id)) load(target.id,false,false);
      else error='This panel is no longer available. Choose a saved panel from Companions.';
    }
  }
  function focusEditor(node: HTMLButtonElement) { node.focus(); }
  async function send(command: CompanionCommand, companion: Record<string,unknown> = {}) {
    const result=(await gatewayRequest({command,companion})).companions;
    if(!result) throw new Error('Companion updates require the native Cockpit app.');
    hostState={...hostState,...result};return result;
  }
  async function refreshCompanions() { if (!active || disposed) return; try { await send('companion_status'); } catch { /* Local panels remain editable offline. */ } }
  let refreshingWidgets=false;
  async function refreshWidgets() {
    if (!active || disposed || !storageReady || refreshingWidgets || widgetEditorDefinition) return;
    refreshingWidgets=true;
    const before=widgetBundle;
    try { const saved=(await gatewayRequest({command:'widget_library_get'})).widgetLibrary;
      if (saved) {const merged=await mergeWidgetBundles(before,saved);if(!disposed && widgetBundle===before && !widgetEditorDefinition)widgetBundle=merged;}
    } catch(e) { if(!disposed) error=`Cannot refresh widget library: ${String(e)}`; }
    finally { refreshingWidgets=false; }
  }
  $effect(() => { if (!active) untrack(stop); else if (mounted) untrack(() => { void refreshCompanions(); }); });


  function stop() { controller?.disarm('Designer changed'); preview = false; }
  async function togglePreview() { if (preview) stop(); else if (!panelInvalidReason) { ensurePixelGeometry();select([]); preview = true; await tick(); controller?.arm(); } }
  function select(ids: string[]) {
    selected = ids;
    propertiesOpen = ids.length > 0;
    const next = document.presentation.widgets.find(w => w.id === ids[0]);
    configText = next ? JSON.stringify(next.config, null, 2) : '';
    if (next) {
      const region = widgetViewport(document, next);
      if (region) selectedViewportId = region.id;
    }
  }
  function selectViewport(id: string) {
    const deselect = propertiesOpen && !selected.length && selectedViewportId === id;
    selectedViewportId = id;
    select([]);
    propertiesOpen = !deselect;
  }
  function slideProperties(_node: HTMLElement) {
    return { duration: window.matchMedia('(prefers-reduced-motion: reduce)').matches ? 0 : 180,
      css: (t: number) => `transform:translateX(${(1-t)*100}%)` };
  }
  function command(command: Parameters<typeof applyDesignerCommand>[1]) {
    try { stop(); history = applyPixelDesignerCommand(history, command); dirty=true; if (!document.ui.pages.some(p => p.id === pageId)) pageId = document.ui.pages[0].id; error = ''; return true; }
    catch (e) { error = e instanceof Error ? e.message : String(e); return false; }
  }
  function ensurePixelGeometry() {
    const next=applyPixelDesignerCommand(history,{type:'replace',document});
    if(next.present.presentation.widgets.some((widget,index)=>JSON.stringify(widget.bounds)!==JSON.stringify(document.presentation.widgets[index].bounds))){history=next;dirty=true;}
  }
  function editName() { nameDraft = name; editingName = true; }
  function focusName(node: HTMLInputElement) { node.focus(); node.select(); }
  function finishName() {
    if (!editingName) return;
    editingName = false;
    const title = nameDraft.trim();
    if (title && title !== name && command({ type: 'replace', document: { ...document, ui: { ...document.ui, title } } })) name = title;
  }
  function addViewport() {
    if (command({ type: 'add-viewport', pageId: page.id, sourceId: activeViewport.id })) selectViewport(viewportRegions(document, page.id).at(-1)!.id);
  }
  async function removeViewport(id: string) {
    const region=regions.find(region=>region.id===id);
    if(!region || !await confirmDelete(`Delete viewport "${region.label}"?`))return;
    if (command({ type:'remove-viewport',pageId:page.id,id })) { selectedViewportId='';select([]); }
  }
  async function removePage(id:string,index:number) {
    if(await confirmDelete(`Delete page ${index} and all its widgets?`)){command({type:'delete-page',id});select([]);}
  }
  function removeSelectedWidget() {
    const widgets=document.presentation.widgets.filter(item=>selected.includes(item.id));
    for(const item of widgets)command({type:'delete-widget',id:item.id});
    select([]);
  }
  function widgetDeleteKey(event:KeyboardEvent) {
    if(event.key!=='Delete'||event.defaultPrevented||event.repeat||event.ctrlKey||event.metaKey||event.altKey||!active||!editorOpen||preview||busy||pendingDelete||pendingLeave||widgetEditorDefinition||!widget)return;
    const target=event.target;
    if(target instanceof HTMLElement && (target.closest('input,textarea,select,[contenteditable]:not([contenteditable="false"])')||target.closest('dialog')))return;
    event.preventDefault();
    void removeSelectedWidget();
  }
  function paletteBounds(viewportId:string,point:{x:number;y:number}):Bounds|undefined {
    const region=regions.find(region=>region.id===viewportId);if(!region)return;
    const grid=document.presentation.grid,width=region.bounds.width*.3,height=region.bounds.height*.25;
    let x=point.x-width/2,y=point.y-height/2;
    if(grid.snap){x=Math.round(x*grid.columns)/grid.columns;y=Math.round(y*grid.rows)/grid.rows;}
    const align=(position:number,size:number,start:number,extent:number,scale:number)=>{
      const min=Math.ceil(start*scale-1e-8),max=Math.floor((start+extent)*scale+1e-8);
      const pixels=Math.max(1,Math.min(max-min,Math.round(size*scale)));
      return [Math.max(min,Math.min(max-pixels,Math.round(position*scale)))/scale,pixels/scale];
    };
    const [px,w]=align(x,width,region.bounds.x,region.bounds.width,usableWidth);
    const [py,h]=align(y,height,region.bounds.y,region.bounds.height,usableHeight);
    return {x:px,y:py,width:w,height:h};
  }
  function addPaletteWidget(kind:string,viewportId=activeViewport.id,point?:{x:number;y:number}) {
    const preset=widgetPresets.find(preset=>preset.kind===kind),region=regions.find(region=>region.id===viewportId);
    if(!preset||!region||preview)return;
    const bounds=point?paletteBounds(viewportId,point):undefined;
    if(command({type:'add-widget',kind:preset.kind,pageId:page.id,viewportId,...(bounds?{bounds}:{})}))select(point?[]:[document.presentation.widgets.at(-1)!.id]);
  }
  function widgetResources(definition: WidgetDefinition, properties: Record<string, WidgetPropertyValue> = {}) {
    const manifest = [], payloads: AssetPayload[] = [];
    for (const id of widgetAssetIds(definition, properties)) {
      const local = document.ui.assets.find(a => a.id === id), shared = widgetBundle.manifest.find(a => a.id === id);
      if (local && shared && canonical(local) !== canonical(shared)) throw new Error('Widget artwork has a conflicting asset identity.');
      const asset = local ?? shared, payload = assets.find(a => a.assetId === id) ?? widgetBundle.assets.find(a => a.assetId === id);
      if (!asset || !payload) throw new Error('Widget artwork is missing. Import its asset bundle before using this widget.');
      manifest.push(structuredClone(asset)); payloads.push(structuredClone(payload));
    }
    return { manifest, payloads };
  }
  function applyLibraryWidget(definition: WidgetDefinition, instanceId?: string) {
    const instance = instanceId ? document.presentation.widgets.find(w => w.id === instanceId) : undefined;
    if (instanceId && instance?.kind !== 'component') throw new Error('The reusable widget instance is no longer available.');
    const properties = instance?.kind === 'component' ? instance.config.properties : definition.interaction.kind === 'navigate' ? { [definition.interaction.pageProperty]: page.id } : {};
    const resources = widgetResources(definition, properties), doc = structuredClone(document);
    for (const a of resources.manifest) if (!doc.ui.assets.some(existing => existing.id === a.id)) doc.ui.assets.push(a);
    const candidate = applyDesignerCommand(createDesignerHistory(doc), instanceId ? { type: 'update-component', id: instanceId, definition } : { type: 'add-component', definition, properties, pageId: page.id, viewportId: activeViewport.id });
    const nextAssets = [...assets, ...resources.payloads.filter(a => !assets.some(existing => existing.assetId === a.assetId))];
    if (!command({ type: 'replace', document: candidate.present })) throw new Error(error || 'The widget could not be added to this panel.');
    assets = nextAssets; select([instanceId ?? document.presentation.widgets.at(-1)!.id]);
  }
  function addLibraryWidget(definition: WidgetDefinition) { try { applyLibraryWidget(definition); } catch (e) { error = e instanceof Error ? e.message : String(e); } }
  function editWidgetDefinition(definition?: WidgetDefinition, instanceId = '') {
    stop(); widgetEditInstanceId = instanceId;
    const next = structuredClone(definition ?? createWidgetCatalog()[0]);
    if (!definition) { next.id = newId('widgetdef'); next.version = 1; next.title = 'New widget'; }
    widgetEditorDefinition = next;
  }
  async function saveWidgetDesign(definition: WidgetDefinition) {
    if (!store || !storageReady) throw new Error('Widget storage is not ready.');
    const existing = widgetBundle.library.definitions.some(d => d.id === definition.id);
    const expectedVersion = existing ? widgetEditorDefinition!.version : null;
    const next = { ...definition, version: (expectedVersion ?? 0) + 1 };
    const resources = widgetResources(next);
    const incoming:WidgetLibraryBundle={ bundleVersion: 1, library: { formatVersion: 1, definitions: [next] }, manifest: resources.manifest, assets: resources.payloads };
    await gatewayRequest({command:'widget_save',bundle:incoming,expectedVersion});
    const bundle = await mergeWidgetBundles(widgetBundle, incoming);
    const saved = await store.saveWidgets(bundle); widgetBundle = saved.widgets!;
    const instanceId = widgetEditInstanceId; widgetEditorDefinition = null; widgetEditInstanceId = '';
    message = `Saved ${next.title} · version ${next.version}. Existing panels retain their pinned revisions.`;
    if (instanceId) { try { applyLibraryWidget(next, instanceId); message = `Saved ${next.title} · version ${next.version} and updated the selected draft instance. Save the panel to publish it.`; } catch (e) { error = `Widget revision saved. The existing instance was preserved: ${e instanceof Error ? e.message : String(e)}`; } }
  }
  function componentProperty(name: string, value: WidgetPropertyValue) {
    if (widget?.kind === 'component') command({ type: 'update-component', id: widget.id, properties: { ...widget.config.properties, [name]: value } });
  }
  async function importWidgetFile(file?: File) {
    if (!file) return;
    await operation(async () => { if (!store || file.size > DESIGNER_LIMITS.bundleBytes) throw new Error('Widget bundle exceeds its import budget or storage is not ready.'); const incoming = await importWidgetBundle(await file.text()); const merged = (await gatewayRequest({command:'widget_library_merge',bundle:incoming})).widgetLibrary!; const saved = await store.saveWidgets(merged); widgetBundle = saved.widgets!; message = 'Widget library imported. Existing panels keep their pinned versions.'; });
  }
  async function exportWidgets() {
    await operation(async () => { const data = await exportWidgetBundle(widgetBundle), url = URL.createObjectURL(new Blob([data], { type: 'application/json' })); const link = globalThis.document.createElement('a'); link.href = url; link.download = 'opensimgear-widgets.json'; link.click(); setTimeout(() => URL.revokeObjectURL(url), 1000); });
  }
  function editViewportName(id: string, label: string) {
    selectedViewportId=id;select([]);propertiesOpen=true;
    viewportNameDraft=label;editingViewportId=id;
  }
  function finishViewportName() {
    const id=editingViewportId,label=viewportNameDraft.trim();editingViewportId='';
    if (id && label && regions.some(region=>region.id===id && region.label!==label)) command({type:'update-viewport',pageId:page.id,id,patch:{label}});
  }
  function changeViewportBounds(key: keyof Bounds, input: HTMLInputElement) {
    const scale = key === 'x' || key === 'width' ? usableWidth : usableHeight;
    command({ type: 'update-viewport', pageId: page.id, id: activeViewport.id, patch: { bounds: { ...activeViewport.bounds, [key]: input.valueAsNumber / scale } } });
    input.value = String(Math.round(activeViewport.bounds[key] * scale * 100) / 100);
  }
  function patch(patch: Partial<DesignerWidget>) { if (widget) command({ type: 'update-widget', id: widget.id, patch }); }
  function changeGrid(axis: 'columns' | 'rows', input: HTMLInputElement) {
    const value = input.valueAsNumber;
    if (Number.isInteger(value) && value >= 1 && value <= 128) {
      command({ type: 'set-grid', ...document.presentation.grid, [axis]: value });
    }
    input.value = String(document.presentation.grid[axis]);
  }
  function changeBounds(key: keyof Bounds, input: HTMLInputElement) {
    if (widget) {
      const scale = key === 'x' || key === 'width' ? usableWidth : usableHeight;
      command({ type: 'move-widget', id: widget.id, bounds: { ...widget.bounds, [key]: Math.round(input.valueAsNumber) / scale } });
      input.value = String(Math.round(widget.bounds[key] * scale));
    }
  }
  function beginCreate() {
    if(!storageReady||busy||library.drafts.length>=DESIGNER_LIMITS.drafts)return;
    newPanelName=`Panel ${library.drafts.length+1}`;newPanelSource='custom';newPanelWidth=1280;newPanelHeight=800;newPanelError='';newPanelOpen=true;
    void refreshCompanions();
  }
  function choosePanelScreen(id:string){newPanelSource=id;newPanelError='';const screen=panelScreenChoices.find(choice=>choice.id===id);if(screen){newPanelWidth=screen.width;newPanelHeight=screen.height;}}
  function create() {
    if(!newPanelOpen||!storageReady||busy||newPanelReason||!chosenPanelScreen)return;
    try{
      const size=chosenPanelScreen;
      const next=newDocument(newPanelName.trim());
      next.presentation.viewports=next.presentation.viewports.map(screen=>({...screen,width:size.width,height:size.height}));
      const nextHistory=createDesignerHistory(next);
      if(!leaveEditor())return;stop();name=next.ui.title;draftId=crypto.randomUUID();history=nextHistory;pageId=document.ui.pages[0].id;selected=[];assets=[];propertiesOpen=false;selectedViewportId='';message='';error='';editorOpen=true;dirty=true;newPanelOpen=false;
      onNavigate?.({kind:'panel',id:draftId});
    }catch(reason){newPanelError=reason instanceof Error?reason.message:String(reason);}
  }
  async function duplicatePanel(id:string){
    if(!storageReady||busy)return;
    await operation(async()=>{
      const source=library.drafts.find(entry=>entry.id===id);if(!source)throw Error('This panel is no longer available.');
      let copyName='';for(let number=1;!copyName;number++){const suffix=number===1?' copy':` copy ${number}`;const candidate=source.name.slice(0,120-suffix.length).trimEnd()+suffix;if(!library.drafts.some(entry=>entry.name===candidate))copyName=candidate;}
      const next=duplicateDraft(library,id,copyName),copy=next.drafts.at(-1)!;copy.document.ui.title=copyName;
      const panel=await publishDocument(copy.document,copy.assets);
      await persist(next,{...publications,[copy.id]:[panel]});message=`Panel duplicated as “${copyName}”.`;
    });
  }
  function load(id: string, recordNavigation = true, checkLeave = true) {
    try {
      if(checkLeave && !leaveEditor())return;
      const entry = openDraft(library, id); stop();editorOpen=true;dirty=false; draftId = entry.id; name = entry.name;
      history = createDesignerHistory(entry.document); dirty=entry.document.presentation.viewports.some(v=>v.backgroundAssetId); assets = entry.assets; pageId = document.ui.pages[0].id; select([]); error = ''; message='';
      if (recordNavigation) onNavigate?.({kind:'panel',id:draftId});
    } catch (e) { error = String(e); }
  }
  async function operation(action: () => Promise<void>) {
    if (busy) return; stop(); busy = true; error = ''; try { await action(); } catch (e) { error = String(e); } finally { busy = false; }
  }
  async function persist(next = library, revisions = publications) {
    if (!store || !storageReady) throw new Error('Storage has not loaded; existing drafts have been preserved.');
    const saved = await store.save({ schemaVersion: 1, library: next, publications: revisions }); library = saved.library; publications = saved.publications;
  }
  async function compactSnapshots(state: CompanionHostState, beforeUpload = false, removedPanelIds: string[] = []) {
    let references;
    try { references = await new ProfilesClient(async request => (await gatewayRequest(request)).profiles ?? {}).companionReferences(); }
    catch { return; } // Preserve every snapshot if the native saved library cannot prove its references.
    const pins=(state.host?.instances ?? []).flatMap(i => typeof i.publicationHash==='string'?[i.publicationHash]:[]);
    pins.push(...references.hashes);
    const managedIds=[...new Set([...removedPanelIds,...library.drafts.map(d => d.document.ui.id),...Object.values(publications).flat().map(p => p.document.ui.id)])];
    if(!beforeUpload){const compacted=await store!.compact(pins);library=compacted.library;publications=compacted.publications;}
    const current=currentPublications.map(p => p.publicationHash);
    await send('companion_prune',{hashes:[...new Set([...pins,...current])],panelIds:managedIds});
  }
  async function save() {
    if (panelInvalidReason) { error=panelInvalidReason;return; }
    await operation(async () => {
      message='';
      const previousHashes=panelHashes(draftId);
      ensurePixelGeometry();
      const panel=await publishDocument(document,activeAssets);
      const next=await saveDraft(library,{id:draftId,name:document.ui.title,document,assets:activeAssets});
      await persist(next,{...publications,[draftId]:[panel]});dirty=false;
      try {
        await compactSnapshots(await send('companion_status'),true);
        const state=await synchronizePanel(panel,previousHashes,send);
        await compactSnapshots(state);
        message='Panel saved and assigned companions updated';
      } catch(e) { error='Panel saved locally. Companion update failed: '+(e instanceof Error?e.message:String(e)); }
    });
  }
  async function removeDraft(id: string) {
    const panelName = library.drafts.find(d => d.id === id)?.name ?? name;
    if(!await confirmDelete(`Delete "${panelName}" and its assets? This also disconnects it from its companions.`))return;
    await operation(async () => {
      const removedPanelId = library.drafts.find(d => d.id === id)?.document.ui.id;
      const references = await new ProfilesClient(async request => (await gatewayRequest(request)).profiles ?? {}).companionReferences();
      await removePanelAssignments(panelHashes(id),send);
      const state = await send('companion_status');
      const pins = [...references.hashes,...(state.host?.instances ?? []).flatMap(instance=>typeof instance.publicationHash==='string'?[instance.publicationHash]:[])];
      if (!store || !storageReady) throw new Error('Panel storage is not ready.');
      const saved = await store.deletePanel(id,pins);library=saved.library;publications=saved.publications;
      assets=[];history=createDesignerHistory(newDocument('New panel'));pageId=document.ui.pages[0].id;select([]);
      editorOpen=false;dirty=false;message='Panel deleted';onNavigate?.({kind:'main'});
      await compactSnapshots(await send('companion_status'),false,removedPanelId?[removedPanelId]:[]);
    });
  }
  async function importFile(file: File | undefined) {
    if (!file) return;
    await operation(async () => {
      if(!leaveEditor())return;
      if (file.size > 13 * 1024 * 1024) throw new Error('Bundle exceeds 13 MiB');
      const result = await importBundle(await file.text()); stop();
      const seed={schemaVersion:1 as const,drafts:[{id:'import',name:result.document.ui.title,document:result.document,assets:result.assets}]};
      const entry=duplicateDraft(seed,'import',result.document.ui.title).drafts[1];
      editorOpen=true;dirty=true;history = createDesignerHistory(entry.document); assets = entry.assets; draftId = entry.id; name = entry.name;
      pageId = document.ui.pages[0].id; select([]); message = 'Panel imported. Save to add it to Panels.';
      onNavigate?.({kind:'panel',id:draftId});
    });
  }
  async function download() {
    if (panelInvalidReason) { error=panelInvalidReason;return; }
    await operation(async () => {
      ensurePixelGeometry();
      const text = await exportBundle(document, activeAssets); const url = URL.createObjectURL(new Blob([text], { type: 'application/json' }));
      const link = window.document.createElement('a'); link.href = url; link.download = `${name.replace(/[^a-z0-9_-]/gi, '-')}.osg-panel.json`; link.click(); URL.revokeObjectURL(url); message = 'Portable bundle exported';
    });
  }
  async function addAsset(file: File | undefined) {
    if (!file) return;
    await operation(() => importWidgetArtwork(file));
  }
  async function importWidgetArtwork(file: File) {
      if (file.size > 2 * 1024 * 1024) throw new Error('Each image must be at most 2 MiB');
      const result = await importAsset(document, new Uint8Array(await file.arrayBuffer()), file.type as 'image/png' | 'image/jpeg' | 'image/webp');
      const nextAssets = [...assets.filter(a => a.assetId !== result.asset.assetId), result.asset];
      await validateAssets(result.document, nextAssets.filter(payload => result.document.ui.assets.some(asset => asset.id === payload.assetId)));
      if (!command({ type: 'replace', document: result.document })) throw new Error(error || 'The image could not be added to this panel.');
      assets = nextAssets; message = `Image added (${result.width} × ${result.height})`;
  }
  async function removeAsset(id: string) {
    const index=document.ui.assets.findIndex(asset=>asset.id===id);
    if(index<0 || !await confirmDelete(`Delete image ${index+1}? It will also be removed from image widgets.`))return;
    const next = structuredClone(document);
    next.ui.assets = next.ui.assets.filter(asset => asset.id !== id);
    for (const viewport of next.presentation.viewports) if (viewport.backgroundAssetId === id) delete viewport.backgroundAssetId;
    for(const widget of next.presentation.widgets) if(widget.kind==='image' && widget.config.assetId===id) delete widget.config.assetId;
    for (const widget of next.presentation.widgets) if ((widget.kind === 'button' || widget.kind === 'icon') && widget.config.iconAssetId === id) delete widget.config.iconAssetId;
    if (command({ type: 'replace', document: next })) { select(selected); message='Image removed'; }
  }
  function updateControl(controlId: string, key: string, value: unknown) {
    const next = structuredClone(document);
    const control = next.ui.controls.find(c => c.id === controlId)!;
    Object.assign(control, { [key]: value });
    command({ type: 'replace', document: next });
  }
  function updateFeedback(id: string, channelId: string) {
    const next = structuredClone(document); next.ui.feedbackWidgets.find(f => f.id === id)!.channelId = channelId;
    command({ type: 'replace', document: next });
  }
  function configPatch(key: string, value: unknown) {
    if (!widget) return;
    let config = { ...widget.config, [key]: value };
    if (key === 'buttonMode') {
      config = { ...config, pulseMs: value === 'pulse' || value === 'repeat' ? 50 : 0, repeatMs: value === 'repeat' ? 200 : 0 };
    }
    patch({ config } as Partial<DesignerWidget>); select(selected);
  }
  function setViewport(width: number, height: number) {
    const insets = viewport.safeInsets;
    const safeInsets = insets ? { top: insets.top * viewport.height / height, bottom: insets.bottom * viewport.height / height,
      left: insets.left * viewport.width / width, right: insets.right * viewport.width / width } : undefined;
    command({ type: 'set-viewport', ...viewport, width, height, ...(safeInsets ? { safeInsets } : {}) });
  }
  function setMargin(value: number) {
    if (!Number.isInteger(value) || value < 0 || value > maxMargin) { error = `Margin must be between 0 and ${maxMargin} pixels.`; return; }
    command({ type: 'set-viewport', ...viewport, safeInsets: { top: value / viewport.height, bottom: value / viewport.height,
      left: value / viewport.width, right: value / viewport.width } });
  }
  function bindControl(controlId: string, value: string) {
    if (!value) { command({ type: 'unbind', controlId }); return; }
    const [targetId, kind, index] = value.split(':'); command({ type: 'bind', controlId, targetId, kind: kind as 'axis' | 'button' | 'hat', index: Number(index) });
  }
  onMount(() => {
    mounted=true;
    let alive = true;
    void import('./companion-library').then(async ({ CompanionLibraryStore, indexedDbStorage }) => {
      store = new CompanionLibraryStore(indexedDbStorage()); const snapshot = await store.load();
      if (!alive) return; library = snapshot.library; publications = snapshot.publications;
      const localWidgets=snapshot.widgets ? {...snapshot.widgets,library:{...snapshot.widgets.library,definitions:includeBuiltinWidgets(snapshot.widgets.library.definitions)}} : widgetBundle;
      widgetBundle=(await gatewayRequest({command:'widget_library_merge',bundle:localWidgets})).widgetLibrary!;
      if(!alive)return;storageReady = true; resolveStorageLoad();
      await refreshCompanions();
      await restoreCompanionPanels();
      await refreshCompanions();
    }).catch(e => { if (alive) error = `Cannot open local library: ${String(e)}`; }).finally(resolveStorageLoad);
    const poll=setInterval(() => { if(active && !busy) {void refreshCompanions();void refreshWidgets();} },2500);
    const cancel = () => stop();
    window.addEventListener('blur', cancel); window.addEventListener('pagehide', cancel);
    return () => { alive = false; disposed=true; resolveStorageLoad(); clearInterval(poll); stop(); unsubscribe?.(); window.removeEventListener('blur', cancel); window.removeEventListener('pagehide', cancel); };
  });
</script>
{#if widgetEditorDefinition}<WidgetDesigner definition={widgetEditorDefinition} assets={widgetEditorAssets} assetOptions={widgetAssetOptions} onsave={saveWidgetDesign} onasset={importWidgetArtwork} oncancel={() => { widgetEditorDefinition = null; widgetEditInstanceId = ''; }} />{/if}
<svelte:window onkeydown={widgetDeleteKey}/>

<script module lang="ts">
  export type CompanionNavigation = { kind: 'main' } | { kind: 'companion'; id: string } | { kind: 'panel'; id: string };
</script>

<section class="designer" aria-label="Companion panel designer">
  <div hidden={editorOpen}><CompanionConnections bind:this={connectionsPanel} publications={currentPublications} {openSettings} {initialCompanionId} {active} onViewChange={open => companionDetailsOpen=open} onNavigate={id => onNavigate?.(id ? {kind:'companion',id} : {kind:'main'})} /></div>
  {#if !editorOpen && !companionDetailsOpen}
  <section class="panels-section" aria-label="Panels">
    <h2>Panels</h2>
    {#if error}<Notification message={String(error)} kind="error" />{/if}
    {#if message}<Notification message={String(message)} />{/if}
    <div class="panel-list-heading"><p>Create a panel, edit its layout and assign it to your companions.</p><div class="panel-list-actions"><button class="action" disabled={!storageReady || busy} onclick={() => importInput?.click()}><Icon name="upload" size={16} />Import panel</button><input bind:this={importInput} class="panel-import" type="file" accept=".json,.osg-panel" onchange={e => { void importFile(e.currentTarget.files?.[0]); e.currentTarget.value = ''; }} disabled={!storageReady || busy} /><button class="action" disabled={!storageReady || busy || library.drafts.length>=DESIGNER_LIMITS.drafts} onclick={beginCreate}><Icon name="plus" size={16} />New panel</button></div></div>
    {#if !storageReady}<p>Loading panels…</p>
    {:else if !library.drafts.length}<div class="empty-panels"><h3>No panels yet</h3><p>Create your first panel to get started.</p></div>
    {:else}<div class="panel-tiles">
      {#each library.drafts as entry (entry.id)}
        <div class="panel-tile-container">
        <button class="panel-tile" aria-label={`Edit ${entry.name}`} disabled={busy} onclick={() => load(entry.id)}>
          <h3 class="panel-tile-title" title={entry.name}>{entry.name}</h3>
          <svg class="panel-thumbnail" viewBox="0 0 320 180" aria-hidden="true">
            <rect width="320" height="180" fill="#101b25" />
            {#each entry.document.presentation.widgets.filter(w => w.pageId===entry.document.ui.pages[0].id) as item}
              <rect x={item.bounds.x*320} y={item.bounds.y*180} width={item.bounds.width*320} height={item.bounds.height*180} rx="4" fill={item.style?.background ?? '#233d49'} stroke={item.style?.accent ?? '#59bda8'} stroke-width="1" />
              <text x={(item.bounds.x+item.bounds.width/2)*320} y={(item.bounds.y+item.bounds.height/2)*180} text-anchor="middle" dominant-baseline="middle" fill={item.style?.foreground ?? '#e1eaf0'} font-size="10">{item.label.slice(0,25)}</text>
            {/each}
          </svg>
          <div class="tile-copy"><p>{entry.document.ui.pages.length} {entry.document.ui.pages.length===1?'page':'pages'} · {entry.document.presentation.widgets.length} {entry.document.presentation.widgets.length===1?'widget':'widgets'}</p><span>{new Set(panelAssignments(entry.id).map(i => i.companionId)).size} {new Set(panelAssignments(entry.id).map(i => i.companionId)).size===1?'companion':'companions'}</span></div>
        </button>
        <div class="panel-tile-actions"><button aria-label={`Duplicate ${entry.name}`} title="Duplicate panel" disabled={busy||library.drafts.length>=DESIGNER_LIMITS.drafts} onclick={()=>duplicatePanel(entry.id)}><Icon name="copy" size={16}/></button><button class="panel-tile-delete" aria-label={`Delete ${entry.name}`} title="Delete panel" disabled={busy} onclick={() => removeDraft(entry.id)}><Icon name="trash" size={16}/></button></div>
        </div>
      {/each}
    </div>{/if}

  </section>
  <section class="widgets-section" aria-label="Widgets">
    <h2>Widgets</h2>
    <div class="panel-list-heading"><p>Edit reusable widget designs for your panels.</p><div class="panel-list-actions"><button class="action" disabled={!storageReady || busy} onclick={() => widgetImportInput?.click()}><Icon name="upload" size={16} />Import widgets</button><button class="action" disabled={!storageReady || busy} onclick={() => editWidgetDefinition()}><Icon name="plus" size={16} />New widget</button></div></div>
    <input hidden type="file" accept="application/json,.json" bind:this={widgetImportInput} aria-label="Import widget library" onchange={e => { void importWidgetFile(e.currentTarget.files?.[0]); e.currentTarget.value = ''; }} />
    {#if !storageReady}<p>Loading widgets…</p>
    {:else}<div class="widget-design-list">{#each availableWidgets as definition (definition.id)}<button class="widget-design-row" aria-label={`Edit design ${definition.title}`} disabled={busy} onclick={() => editWidgetDefinition(definition)}><span><strong>{definition.title}</strong><small>Version {definition.version} · {definition.size.width} × {definition.size.height}</small></span><Icon name="pencil" size={18} /></button>{/each}</div>{/if}
  </section>
  {:else if editorOpen}
  <section aria-label="Panel editor">
  <button class="text-button back-panels" use:focusEditor onclick={backToPanels} disabled={busy}><Icon name="back" size={16} />Back to Panels</button>
  <div class="page-heading editor-heading">
    <div class="editor-title"><div class="eyebrow">PANEL EDITOR</div>
      <h1 aria-label={name || 'Untitled panel'}>{#if editingName}<input class="panel-name-input" aria-label="Panel name" maxlength="120" bind:value={nameDraft} use:focusName onblur={finishName} onkeydown={event=>{if(event.key==='Enter'){event.preventDefault();finishName();}else if(event.key==='Escape'){event.preventDefault();editingName=false;}}}/>{:else}<button class="panel-title-button" aria-label="Edit panel name" title="Edit panel name" disabled={busy} onclick={editName}><span>{name || 'Untitled panel'}</span><Icon name="pencil" size={20}/></button>{/if}</h1>
      <p>{dirty ? 'Unsaved changes' : 'Edit the panel layout.'}</p>
    </div>
    <div class="editor-title-actions">
      <span class="action-tooltip" title={!preview && panelInvalidReason || undefined}><button class="action" title={!preview && panelInvalidReason || undefined} onclick={togglePreview} disabled={busy || !preview && !!panelInvalidReason}><Icon name={preview ? 'stop' : 'play'} size={16} />{preview ? 'Stop preview' : 'Preview'}</button></span>
      <span class="action-tooltip" title={panelInvalidReason || undefined}><button class="action" title={panelInvalidReason || undefined} onclick={download} disabled={busy || !!panelInvalidReason}><Icon name="download" size={16} />Export</button></span>
      <span class="action-tooltip" title={panelInvalidReason || undefined}><button class="action" title={panelInvalidReason || undefined} onclick={save} disabled={busy || !storageReady || !!panelInvalidReason}><Icon name="save" size={16} />{busy ? 'Saving…' : 'Save'}</button></span>
      {#if library.drafts.some(d => d.id===draftId)}<button class="action delete-panel" disabled={busy} onclick={() => removeDraft(draftId)}><Icon name="trash" size={16} />Delete</button>{/if}
    </div>
  </div>
  {#if error}<Notification message={String(error)} kind="error" />{/if}
  {#if message}<Notification message={String(message)} />{/if}
  <div inert={busy}>
  <div class="workspace">
    <aside class="panel-library" aria-label="Panel library">
      <details class="editor-accordion"><summary>Pages<button class="accordion-info" aria-label="About pages" title="Adding pages is a work in progress and is currently unavailable." onclick={e=>e.stopPropagation()}><Icon name="info" size={16}/></button></summary><div class="accordion-content">
        <ul class="page-list" aria-label="Panel pages">{#each document.ui.pages as p,index (p.id)}<li>
          <button class="page-select" class:active={p.id === page.id} aria-pressed={p.id === page.id} onclick={() => { stop(); pageId=p.id;select([]); }}>Page {index}{#if !/^Page \d+$/.test(p.title)}<small>{p.title}</small>{/if}</button>
          {#if document.ui.pages.length > 1}<button class="page-delete" aria-label={`Delete page ${index}`} title="Delete page" onclick={() => removePage(p.id,index)}><Icon name="trash" size={15}/></button>{/if}
        </li>{/each}</ul>
        <div class="accordion-add" title="Adding pages is a work in progress and is currently unavailable."><button class="action" disabled><Icon name="plus" size={16} />Add page</button></div>
      </div></details>
      <details class="editor-accordion"><summary>Viewports<button class="accordion-info" aria-label="About viewports" title="About viewports" popovertarget="viewport-help" onclick={e => e.stopPropagation()}><Icon name="info" size={16}/></button></summary><div class="accordion-content">
      <div id="viewport-help" class="viewport-help" popover role="dialog" aria-label="Viewport help">
        <button class="help-close" aria-label="Close viewport help" popovertarget="viewport-help" popovertargetaction="hide"><Icon name="close" size={16}/></button>
        <h3>Viewports</h3>
        <p>Divide a companion screen into separate areas for widgets. Add viewport uses free space, or splits the current viewport when the screen is full.</p>
        <p>Click a viewport to edit its properties; click it again to deselect. Drag its label to move it, or its corner to resize it. Hold Ctrl while dragging to bypass grid snapping.</p>
        <p>Widgets must stay inside a viewport, and viewports cannot overlap. Move or delete a viewport’s widgets before deleting it. Every page must keep at least one viewport.</p>
      </div>
      <div class="viewport-list">{#each regions as region}<div class="viewport-row" class:active={propertiesOpen && !selected.length && region.id === activeViewport.id}>
        {#if editingViewportId===region.id}<input class="viewport-name-input" aria-label="Viewport name" maxlength="120" bind:value={viewportNameDraft} use:focusName onblur={finishViewportName} onkeydown={event=>{if(event.key==='Enter'){event.preventDefault();finishViewportName();}else if(event.key==='Escape'){event.preventDefault();editingViewportId='';}}}/>
        {:else}<button class="viewport-select" aria-label={`Select viewport ${region.label}`} class:active={propertiesOpen && !selected.length && region.id === activeViewport.id} aria-pressed={propertiesOpen && !selected.length && region.id === activeViewport.id} disabled={preview} onclick={() => selectViewport(region.id)}><span class="viewport-name-space" aria-hidden="true">{region.label}</span></button><button class="viewport-name-button" aria-label={`Edit viewport name ${region.label}`} title="Edit viewport name" disabled={preview} onclick={() => editViewportName(region.id,region.label)}><span>{region.label}</span><span class="viewport-pencil"><Icon name="pencil" size={14}/></span></button>{/if}
        {#if regions.length > 1}<button class="viewport-delete" aria-label={`Delete viewport ${region.label}`} title={document.presentation.widgets.some(w => w.pageId === page.id && boundsContain(region.bounds,w.bounds)) ? 'Move or delete its widgets before removing this viewport' : 'Delete viewport'} disabled={preview || document.presentation.widgets.some(w => w.pageId === page.id && boundsContain(region.bounds,w.bounds))} onclick={() => removeViewport(region.id)}><Icon name="trash" size={15}/></button>{/if}
      </div>{/each}</div>
      <div class="accordion-add"><button class="action" disabled={preview || regions.length >= 32} onclick={addViewport}><Icon name="plus" size={16} />Add viewport</button></div>
      </div></details>
      <details class="editor-accordion" open><summary>Widgets</summary><div class="accordion-content">
      <p class="hint">Add to {activeViewport.label}</p>{#if assignedTargetError}<p class="hint">Companion update: {assignedTargetError} You can keep designing, previewing and exporting this panel.</p>{/if}
      <details class="shared-widget-library" open><summary>Reusable widget library</summary><div class="shared-widget-list">{#each availableWidgets as definition}<div><button disabled={preview || busy} title={`Add ${definition.title}, version ${definition.version}`} onclick={() => addLibraryWidget(definition)}>{definition.title}</button><button class="edit-library-widget" aria-label={`Edit design ${definition.title}`} title="Edit widget design" disabled={preview || busy} onclick={() => editWidgetDefinition(definition)}>✎</button></div>{/each}</div><div class="widget-library-actions"><button disabled={preview || busy} onclick={() => editWidgetDefinition()}>Create widget</button><button disabled={busy} onclick={() => widgetImportInput?.click()}>Import widgets</button><button disabled={busy} onclick={() => void exportWidgets()}>Export widgets</button></div><input hidden type="file" accept="application/json,.json" bind:this={widgetImportInput} aria-label="Import widget library" onchange={e => { void importWidgetFile(e.currentTarget.files?.[0]); e.currentTarget.value = ''; }} /></details>
      <details><summary>Basic controls</summary><div class="palette">{#each widgetPresets as preset}<button disabled={preview} draggable={!preview} title="Click to add to the selected viewport, or drag into a viewport" ondragstart={event=>{if(!event.dataTransfer)return;event.dataTransfer.setData('application/x-osg-widget',preset.kind);event.dataTransfer.effectAllowed='copy';paletteDragging=true;draggedWidget=preset.title;select([]);}} ondragend={()=>{paletteDragging=false;draggedWidget='';}} onclick={() => addPaletteWidget(preset.kind)}>{preset.title}</button>{/each}</div></details>
      </div></details>
      <details class="editor-accordion"><summary>Assets</summary><div class="accordion-content">

      <input class="asset-import" bind:this={assetInput} aria-label="Add panel image" type="file" accept="image/png,image/jpeg,image/webp" disabled={busy} onchange={e => {void addAsset(e.currentTarget.files?.[0]);e.currentTarget.value='';}} />
      <ul class="asset-list" aria-label="Panel assets">{#each document.ui.assets as asset,index (asset.id)}<li data-asset-id={asset.id}>
        <img src={imageAssets[asset.id]} alt=""/><div><strong>Image {index+1}</strong><small>{asset.mime.split('/')[1].toUpperCase()} · {Math.max(1,Math.ceil(asset.sizeBytes/1024))} KB</small></div>
        <button class="asset-delete" aria-label={`Delete image ${index+1}`} title="Delete asset" disabled={busy} onclick={()=>removeAsset(asset.id)}><Icon name="trash" size={15}/></button>
      </li>{:else}<li class="assets-empty">No assets yet.</li>{/each}</ul>
      <div class="accordion-add"><button class="action" title="PNG, JPEG or WebP · 2 MiB each · 8 MiB total" disabled={busy} onclick={()=>assetInput?.click()}><Icon name="plus" size={16}/>Add asset</button></div>
      </div></details>
      <details class="editor-accordion"><summary>Grid</summary><div class="accordion-content">
        <label>Columns<input type="number" min="1" max="128" step="1" disabled={preview} value={document.presentation.grid.columns} onchange={e => changeGrid('columns', e.currentTarget)} /></label>
        <label>Rows<input type="number" min="1" max="128" step="1" disabled={preview} value={document.presentation.grid.rows} onchange={e => changeGrid('rows', e.currentTarget)} /></label>
        <p class="hint">Spacing: {Math.round(usableWidth / document.presentation.grid.columns * 100) / 100} × {Math.round(usableHeight / document.presentation.grid.rows * 100) / 100} px. More columns or rows make a finer grid.</p>
      </div></details>
      <details class="editor-accordion"><summary>Screen size</summary><div class="accordion-content">
        <label>Companion screen<select value={`${viewport.width}x${viewport.height}`} onchange={e => { const [w,h] = e.currentTarget.value.split('x').map(Number); setViewport(w,h); }}><option value="390x844">Phone portrait</option><option value="844x390">Phone landscape</option><option value="768x1024">Tablet portrait</option><option value="1024x768">Tablet landscape</option><option value="800x480">ESP32 screen · 800 × 480</option><option value="1280x800">Large panel · 1280 × 800</option><option value="1280x720">Desktop · 1280 × 720</option><option value="1920x1080">Full HD · 1920 × 1080</option><option value="2560x1440">QHD · 2560 × 1440</option><option value="3840x2160">4K · 3840 × 2160</option>{#if !['390x844','844x390','768x1024','1024x768','800x480','1280x800','1280x720','1920x1080','2560x1440','3840x2160'].includes(`${viewport.width}x${viewport.height}`)}<option value={`${viewport.width}x${viewport.height}`}>Custom · {viewport.width} × {viewport.height}</option>{/if}</select></label>
        <label>Screen width<input class="screen-dimension" type="number" min="320" max="8192" step="1" value={viewport.width} onchange={e => { setViewport(e.currentTarget.valueAsNumber, viewport.height); e.currentTarget.value=String(viewport.width); }} /></label>
        <label>Screen height<input class="screen-dimension" type="number" min="320" max="8192" step="1" value={viewport.height} onchange={e => { setViewport(viewport.width, e.currentTarget.valueAsNumber); e.currentTarget.value=String(viewport.height); }} /></label>
        <button onclick={() => setViewport(viewport.height, viewport.width)}><Icon name="refresh" size={16} />Rotate</button>
        <label>Margin (px)<input type="number" min="0" max={maxMargin} step="1" value={margin} onchange={e => { setMargin(e.currentTarget.valueAsNumber); e.currentTarget.value=String(margin); }}/></label>
        <p class="hint">Applies equally to all four sides. Use 0 for the full screen.</p>
      </div></details>
    </aside>
    <div class="panel-canvas-workspace">
      <div class="toolbar layout-tools" role="group" aria-label="Widget layout tools">
        <div class="layout-toolset history-tools" role="group" aria-label="Edit history">
        <button aria-label="Undo" title="Undo" disabled={!history.past.length} onclick={() => { stop(); dirty=true;history = undoDesigner(history); name = document.ui.title; select([]); }}><Icon name="undo" size={18} /></button>
        <button aria-label="Redo" title="Redo" disabled={!history.future.length} onclick={() => { stop(); dirty=true;history = redoDesigner(history); name = document.ui.title; select([]); }}><Icon name="redo" size={18} /></button>
        </div><div class="layout-toolset">
        <button class:active={document.presentation.grid.snap} aria-label="Snap to grid" title="Snap to grid" aria-pressed={document.presentation.grid.snap} onclick={() => command({ type: 'set-grid', ...document.presentation.grid, snap: !document.presentation.grid.snap })}><Icon name="grid" size={18}/></button>
        </div><div class="layout-toolset" role="group" aria-label="Vertical alignment">
        <button disabled={selected.length < 2} aria-label="Align left" title="Align left" onclick={() => command({ type: 'align', ids: selected, edge: 'left' })}><Icon name="align-left" size={18}/></button>
        <button disabled={selected.length < 2} aria-label="Align vertical" title="Align vertical" onclick={() => command({ type: 'align', ids: selected, edge: 'center-x' })}><Icon name="align-center" size={18}/></button>
        <button disabled={selected.length < 2} aria-label="Align right" title="Align right" onclick={() => command({ type: 'align', ids: selected, edge: 'right' })}><Icon name="align-right" size={18}/></button>
        </div><div class="layout-toolset" role="group" aria-label="Horizontal alignment">
        <button disabled={selected.length < 2} aria-label="Align top" title="Align top" onclick={() => command({ type: 'align', ids: selected, edge: 'top' })}><Icon name="align-top" size={18}/></button>
        <button disabled={selected.length < 2} aria-label="Align horizontal" title="Align horizontal" onclick={() => command({ type: 'align', ids: selected, edge: 'center-y' })}><Icon name="align-horizontal" size={18}/></button>
        <button disabled={selected.length < 2} aria-label="Align bottom" title="Align bottom" onclick={() => command({ type: 'align', ids: selected, edge: 'bottom' })}><Icon name="align-bottom" size={18}/></button>
        </div><div class="layout-toolset" role="group" aria-label="Widget grouping">
        <button disabled={selected.length < 2} aria-label="Group" title="Group" onclick={() => command({ type: 'add-group', pageId: page.id, label: 'Group', widgetIds: selected })}><Icon name="group" size={18}/></button>
        <button disabled={!widget?.groupId} aria-label="Ungroup" title="Ungroup" onclick={() => widget?.groupId && command({ type: 'ungroup', id: widget.groupId })}><Icon name="ungroup" size={18}/></button>
        </div>
      </div>
      {#if error}<p class="layout-error" role="alert">{error}</p>{/if}
      <div class="panel-properties-layout" class:palette-dragging={paletteDragging}>
      <PanelCanvas {document} pageId={page.id} assets={imageAssets} mode={preview ? 'preview' : 'edit'} selectedIds={selected} selectedViewportId={propertiesOpen && !selected.length ? activeViewport.id : ''} onselectviewport={selectViewport} onwidgetdrop={addPaletteWidget} {draggedWidget} widgetDropBounds={paletteBounds} onviewportbounds={(id,bounds) => command({ type: 'update-viewport', pageId: page.id, id, patch: { bounds } })} onerror={value => error=value} onselect={select} oncontroller={value => { unsubscribe?.(); controller = value; unsubscribe = value.subscribe(snapshot => previewSnapshot = snapshot); }} onlayout={(id,bounds) => command({ type: 'move-widget', id, bounds })} />
      {#if propertiesOpen}
    <aside class="properties-overlay" aria-label="Widget properties" transition:slideProperties><button class="properties-close" aria-label="Close properties" title="Close properties" onclick={()=>select([])}><Icon name="close" size={18}/></button><h3>Properties</h3>
      {#if widget}
        <p class="hint">{widget.kind} · <code>{widget.id}</code></p>
        <label>Label<input value={widget.label} onchange={e => patch({ label: e.currentTarget.value })} /></label>
        <div class="bounds">{#each ['x','y','width','height'] as field}<label>{field === 'x' ? 'X' : field === 'y' ? 'Y' : field === 'width' ? 'Width' : 'Height'} (px)<input type="number" min={field === 'width' || field === 'height' ? 1 : 0} max={field === 'x' || field === 'width' ? usableWidth : usableHeight} step="1" value={Math.round(widget.bounds[field as keyof Bounds] * (field === 'x' || field === 'width' ? usableWidth : usableHeight))} onchange={e => changeBounds(field as keyof Bounds, e.currentTarget)} /></label>{/each}</div>
        {#if widget.kind === 'component' && reusableDefinition}
          <p class="hint">{reusableDefinition.title} · pinned version {widget.config.definitionVersion}</p>
          <button class="action" onclick={() => editWidgetDefinition(reusableDefinition, widget!.id)}>Edit widget design</button>
          <label>Library version<select aria-label="Widget library version" value={widget.config.definitionVersion} onchange={e => { const definition = widgetBundle.library.definitions.find(d => d.id === reusableDefinition!.id && d.version === Number(e.currentTarget.value)); if (definition) { try { applyLibraryWidget(definition, widget!.id); } catch (e) { error = String(e); } } }}>
            {#each [...new Map([...(document.presentation.definitions ?? []), ...widgetBundle.library.definitions].filter(d => d.id === reusableDefinition.id).map(d => [d.version, d])).values()].sort((a, b) => a.version - b.version) as revision}<option value={revision.version}>Version {revision.version}</option>{/each}
          </select></label>
          {#each reusableDefinition.properties as property}
            {@const value = widget.config.properties[property.name] ?? property.default}
            {#if property.type === 'boolean'}<label><input type="checkbox" checked={Boolean(value)} onchange={e => componentProperty(property.name, e.currentTarget.checked)} />{property.label}</label>
            {:else if property.type === 'number'}<label>{property.label}<input type="number" min={property.min} max={property.max} value={Number(value)} onchange={e => componentProperty(property.name, e.currentTarget.valueAsNumber)} /></label>
            {:else if property.type === 'color'}<label>{property.label}<input type="color" value={String(value)} onchange={e => componentProperty(property.name, e.currentTarget.value)} /></label>
            {:else if property.type === 'asset'}<label>{property.label}<select value={String(value)} onchange={e => componentProperty(property.name, e.currentTarget.value)}><option value="">None</option>{#each document.ui.assets as asset, index}<option value={asset.id}>Image {index + 1}</option>{/each}</select></label>
            {:else if reusableDefinition.interaction.kind === 'navigate' && reusableDefinition.interaction.pageProperty === property.name}<label>{property.label}<select value={String(value)} onchange={e => componentProperty(property.name, e.currentTarget.value)}>{#each document.ui.pages as destination}<option value={destination.id}>{destination.title}</option>{/each}</select></label>
            {:else}<label>{property.label}<input value={String(value)} onchange={e => componentProperty(property.name, e.currentTarget.value)} /></label>{/if}
          {/each}
        {:else}
        {#each ['foreground','background','accent'] as color}<label>{color}<input type="color" value={widget.style?.[color as keyof NonNullable<DesignerWidget['style']>] ?? (color === 'background' ? '#12232e' : '#55ddc7')} onchange={e => patch({ style: { ...widget.style, [color]: e.currentTarget.value } })} /></label>{/each}
        {#each Object.entries(widget.config).filter(([key])=>!['buttonMode','pulseMs','repeatMs'].includes(key)) as [key,value]}
          {#if typeof value === 'boolean'}<label><input type="checkbox" checked={value} onchange={e => configPatch(key,e.currentTarget.checked)} />{key}</label>
          {:else if typeof value === 'number'}<label>{key}<input type="number" value={value} step={key === 'deadzone' ? .01 : 1} onchange={e => configPatch(key,e.currentTarget.valueAsNumber)} /></label>
          {:else if key === 'centering'}<label>Return behavior<select value={String(value)} onchange={e => configPatch(key,e.currentTarget.value)}><option value="spring">Spring to fallback</option><option value="latched">Keep last position</option></select></label>
          {:else if key === 'orientation'}<label>Orientation<select value={String(value)} onchange={e => configPatch(key,e.currentTarget.value)}><option value="horizontal">Horizontal</option><option value="vertical">Vertical</option></select></label>
          {:else if typeof value === 'string' && key !== 'buttonMode' && key !== 'iconAssetId' && key !== 'assetId'}<label>{key}<input value={value} onchange={e => configPatch(key,e.currentTarget.value)} /></label>{/if}
        {/each}
        {#if widget.kind === 'image'}<label>Image asset<select aria-label="Image asset" value={widget.config.assetId ?? ''} onchange={e=>configPatch('assetId',e.currentTarget.value || undefined)}><option value="">None</option>{#each document.ui.assets as asset,index}<option value={asset.id}>Image {index+1}</option>{/each}</select></label><button class="action" onclick={()=>patch({bounds:{...activeViewport.bounds}})}>Fill viewport</button>{/if}
        {#if widget.kind === 'button' || widget.kind === 'icon'}<label>Icon image<select value={widget.config.iconAssetId ?? ''} onchange={e => { const config = { ...widget!.config } as Record<string,unknown>; if(e.currentTarget.value) config.iconAssetId = e.currentTarget.value; else delete config.iconAssetId; patch({ config } as Partial<DesignerWidget>); }}><option value="">None</option>{#each document.ui.assets as asset}<option value={asset.id}>{asset.id}</option>{/each}</select></label>{/if}
        <details><summary>Advanced behavior</summary><label>Validated configuration<textarea rows="8" bind:value={configText}></textarea></label><button onclick={() => { try { patch({ config: JSON.parse(configText) }); } catch (e) { error = String(e); } }}>Apply behavior</button></details>
        {/if}
        <button onclick={() => { command({ type: 'duplicate-widget', id: widget!.id }); select([document.presentation.widgets.at(-1)!.id]); }}>Duplicate widget</button>
        <button onclick={removeSelectedWidget}>Delete widget</button>
        {#each widget.controlIds as controlId}
          {@const control = document.ui.controls.find(c => c.id === controlId)!}
          {@const binding = document.bindings.find(b => b.controlId === controlId)}
          {#if control.kind === 'axis'}<label>{control.label} fallback ({control.axisMode})<input type="number" min={control.axisMode === 'bipolar' ? -1 : 0} max="1" step="0.01" value={control.fallback} onchange={e => updateControl(control.id,'fallback',e.currentTarget.valueAsNumber)} /></label>{/if}
          <label>Logical target for {control.label}<select value={binding ? `${binding.targetId}:${binding.kind}:${binding.index}` : ''} onchange={e => bindControl(controlId, e.currentTarget.value)}><option value="">Unassigned</option>
            {#each document.targets as target}{#each Array.from({length: control.kind === 'axis' ? target.axes.length : control.kind === 'button' ? target.buttons : target.hats}) as _, index}<option value={`${target.id}:${control.kind}:${index}`}>{target.label} · {control.kind} {index + 1}</option>{/each}{/each}
          </select></label>
        {/each}
        {#each widget.feedbackIds as feedbackId}{@const output = document.ui.feedbackWidgets.find(f => f.id === feedbackId)!}<label>Typed feedback channel<input value={output.channelId} onchange={e => updateFeedback(feedbackId,e.currentTarget.value)} /></label>{/each}
        {#if widget.feedbackIds.length}<p class="hint">Output widgets show typed host/simulator feedback with source and freshness when connected.</p>{/if}
      {:else}
        <div class="viewport-properties"><p class="eyebrow">VIEWPORT</p>
          <label>Viewport name<input value={activeViewport.label} disabled={preview} maxlength="120" onchange={e => { command({ type: 'update-viewport', pageId: page.id, id: activeViewport.id, patch: { label: e.currentTarget.value } }); e.currentTarget.value=activeViewport.label; }} /></label>
          <div class="bounds">{#each ['x','y','width','height'] as field}<label>{field === 'x' ? 'Left' : field === 'y' ? 'Top' : field === 'width' ? 'Width' : 'Height'} (px)<input aria-label={`Viewport ${field}`} type="number" min={field === 'width' || field === 'height' ? 1 : 0} max={field === 'x' || field === 'width' ? usableWidth : usableHeight} step="1" disabled={preview} value={Math.round(activeViewport.bounds[field as keyof Bounds] * (field === 'x' || field === 'width' ? usableWidth : usableHeight) * 100) / 100} onchange={e => changeViewportBounds(field as keyof Bounds, e.currentTarget)} /></label>{/each}</div>
          <p class="hint">Widgets move and scale with this viewport. Viewports may touch but cannot overlap.</p>
          <button disabled={preview || regions.length <= 1 || activeViewportOccupied} onclick={() => removeViewport(activeViewport.id)}>Remove viewport</button>
          <p class="hint">Move or delete its widgets before removing a viewport.</p>
        </div>
      {/if}
      <details><summary>Logical target layouts</summary><p class="hint">Portable descriptors and mappings. Machine slots and credentials are stored separately.</p><button onclick={() => { targetText = JSON.stringify(document.targets, null, 2); }}><Icon name="sliders" size={16} />Edit layouts</button><textarea aria-label="Logical target layouts JSON" rows="8" bind:value={targetText}></textarea><button onclick={() => { try { command({ type: 'replace', document: { ...document, targets: JSON.parse(targetText) } }); } catch (e) { error = String(e); } }}><Icon name="check" size={16} />Apply layouts</button></details>
    </aside>
      {/if}
      </div>
      {#if preview && previewSnapshot}<p role="status" class="hint">Preview {previewSnapshot.armed ? 'armed' : 'disarmed'} · {previewSnapshot.activePointers} active pointers · {previewSnapshot.events.length} recorded transitions · {previewSnapshot.reason}</p><details><summary>Local requested values</summary>{#each document.ui.controls as control}<p class="hint">{control.label}: {String(previewSnapshot.values[control.id])}</p>{/each}</details>{/if}
    </div>

  </div>
  </div>
  </section>
  {/if}
  {#if newPanelOpen}<dialog class="leave-panel-dialog new-panel-dialog" aria-label="New panel" use:showLeaveDialog oncancel={event=>{event.preventDefault();newPanelOpen=false;}}>
    <form onsubmit={event=>{event.preventDefault();create();}}><h2>New panel</h2>
      <label>Panel name<input aria-label="Panel name" bind:value={newPanelName} maxlength="120" required use:focusName/></label>
      <label>Initial screen size<select aria-label="Initial screen size" value={newPanelSource} onchange={event=>choosePanelScreen(event.currentTarget.value)}><option value="custom">Custom</option>{#each panelScreenChoices as choice (choice.id)}<option value={choice.id}>{choice.name} · {choice.width} × {choice.height} px</option>{/each}</select></label>
      <div class="panel-size-fields"><label>Width (px)<input aria-label="Panel width" type="number" min={PANEL_SCREEN_MIN} max={PANEL_SCREEN_MAX} step="1" value={chosenPanelScreen?.width??newPanelWidth} readonly={newPanelSource!=='custom'} oninput={event=>newPanelWidth=event.currentTarget.valueAsNumber} required/></label><label>Height (px)<input aria-label="Panel height" type="number" min={PANEL_SCREEN_MIN} max={PANEL_SCREEN_MAX} step="1" value={chosenPanelScreen?.height??newPanelHeight} readonly={newPanelSource!=='custom'} oninput={event=>newPanelHeight=event.currentTarget.valueAsNumber} required/></label></div>
      <p class="hint">Choose a companion to use its screen dimensions, or enter a custom size. Companions with a known screen size are listed. You can change the size in the editor.</p>
      {#if newPanelError||newPanelReason}<p class="layout-error" role="alert">{newPanelError||newPanelReason}</p>{/if}
      <div class="new-panel-actions"><button type="button" class="secondary-button" onclick={()=>newPanelOpen=false}>Cancel</button><button type="submit" class="primary-button" disabled={busy||!storageReady||!!newPanelReason}>Create panel</button></div>
    </form>
  </dialog>{/if}
  {#if pendingLeave}<dialog class="leave-panel-dialog" role="alertdialog" aria-modal="true" aria-labelledby="leave-panel-title" use:showLeaveDialog oncancel={event=>{event.preventDefault();keepEditing();}}>
    <h2 id="leave-panel-title">Keep your unsaved changes?</h2><p>Save this panel before leaving, or discard this draft.</p>
    {#if error}<p class="layout-error" role="alert">{error}</p>{/if}
    <div><button class="secondary-button" disabled={busy} onclick={keepEditing}>Keep editing</button><button class="secondary-button" disabled={busy} onclick={()=>continueLeaving(true)}>Discard and continue</button><span class="action-tooltip" title={panelInvalidReason || undefined}><button class="primary-button" title={panelInvalidReason || undefined} disabled={busy || !storageReady || !!panelInvalidReason} onclick={saveAndContinue}>{busy?'Saving…':'Save and continue'}</button></span></div>
  </dialog>{/if}
  {#if pendingDelete}<dialog class="leave-panel-dialog delete-confirmation" role="alertdialog" aria-modal="true" aria-labelledby="delete-panel-item-title" use:showDeleteDialog oncancel={event=>{event.preventDefault();finishDelete(false);}}>
    <h2 id="delete-panel-item-title">Confirm deletion</h2><p>{pendingDelete.message}</p>
    <div><button class="destructive-button" onclick={()=>finishDelete(true)}>Delete</button><button class="secondary-button" data-cancel onclick={()=>finishDelete(false)}>Cancel</button></div>
  </dialog>{/if}
</section>

<style>
  .widgets-section{margin-top:28px;padding-top:24px;border-top:1px solid var(--line)}
  .widget-design-list{display:grid;grid-template-columns:repeat(auto-fill,minmax(min(100%,280px),1fr));gap:10px;margin:20px 0}
  .widget-design-row{display:flex;align-items:center;justify-content:space-between;gap:16px;padding:16px;text-align:left;background:var(--surface);border-radius:8px;min-width:0}
  .widget-design-row span{display:flex;flex-direction:column;gap:6px;min-width:0}.widget-design-row strong{overflow:hidden;text-overflow:ellipsis;white-space:nowrap}.widget-design-row small{color:var(--muted)}
  .new-panel-dialog form{display:grid;gap:9px}.new-panel-dialog h2{margin-bottom:5px}.new-panel-dialog label{margin:0}.panel-size-fields{display:grid;grid-template-columns:1fr 1fr;gap:12px}.new-panel-actions{display:flex;justify-content:flex-end;gap:10px;margin-top:9px}.new-panel-dialog input[readonly]{color:var(--muted)}

  .toolbar.layout-tools{justify-content:flex-end;align-items:center;gap:12px;margin-top:0}.layout-toolset{display:flex;align-items:center;gap:6px}.layout-toolset + .layout-toolset{border-left:1px solid var(--line);padding-left:12px}
  .history-tools{margin-right:auto}.history-tools + .layout-toolset{border-left:0;padding-left:0}
  .action-tooltip{display:inline-flex}
  .delete-confirmation .destructive-button{background:var(--danger,#c94f5c);border-color:var(--danger,#c94f5c);color:#fff}
  .palette-dragging .properties-overlay{pointer-events:none}.palette button[draggable=true]{cursor:grab}.palette button[draggable=true]:active{cursor:grabbing}
  .leave-panel-dialog{position:fixed;left:50%;top:50%;transform:translate(-50%,-50%);max-width:460px;width:calc(100% - 40px);box-sizing:border-box;padding:26px;background:var(--surface);border:1px solid var(--line);border-radius:10px;box-shadow:0 20px 80px #0005;color:var(--text);margin:0}.leave-panel-dialog::backdrop{background:#0009}.leave-panel-dialog h2{font-size:14px}.leave-panel-dialog p{font-size:11px;color:var(--muted);line-height:1.8;margin:15px 0 23px}.leave-panel-dialog>div{display:flex;gap:10px;justify-content:flex-end;flex-wrap:wrap}.leave-panel-dialog button{font-size:10px}
  .viewport-name-space{visibility:hidden}.viewport-row .viewport-name-button{position:absolute;left:9px;top:50%;transform:translateY(-50%);display:inline-flex;align-items:center;gap:5px;max-width:calc(100% - 48px);padding:0;border:0;background:transparent}.viewport-name-button > span:first-child{min-width:0;overflow:hidden;text-overflow:ellipsis;white-space:nowrap}.viewport-row.active .viewport-name-button{color:var(--accent)}.viewport-pencil{display:flex;opacity:0;color:var(--muted)}.viewport-row:hover .viewport-pencil,.viewport-row:focus-within .viewport-pencil{opacity:1}.viewport-name-input{width:100%;padding-right:36px}
  @media(hover:none){.viewport-pencil{opacity:1}}
  .accordion-info{float:right;display:inline-flex;align-items:center;justify-content:center;padding:3px;margin:-3px 0;background:transparent;border:0;color:var(--muted)}.viewport-help{width:min(360px,calc(100vw - 32px));box-sizing:border-box;max-height:80vh;overflow:auto;padding:20px;background:var(--surface);color:var(--text);border:1px solid var(--line);border-radius:10px;box-shadow:0 12px 40px #0005}.viewport-help h3{margin:0 28px 12px 0;font-size:16px}.viewport-help p:last-child{margin-bottom:0}.help-close{float:right;display:grid;place-items:center;padding:3px;background:transparent;border:0;color:var(--muted)}
  .viewport-row{position:relative;min-width:0}.viewport-select{width:100%;padding-right:36px}.viewport-row .viewport-delete{position:absolute;right:4px;top:50%;transform:translateY(-50%);display:grid;place-items:center;padding:5px;color:#e39b9b;opacity:0;pointer-events:none}.viewport-row:hover .viewport-delete,.viewport-row:focus-within .viewport-delete{opacity:1;pointer-events:auto}
  @media(hover:none){.viewport-row .viewport-delete{opacity:1;pointer-events:auto}}
  .page-list{list-style:none;padding:0;margin:0;display:grid;gap:6px}.page-list li{position:relative;min-width:0}.page-select{width:100%;text-align:left;padding-right:36px}.page-select.active{border-color:var(--accent);background:var(--accent-bg);color:var(--accent)}.page-select small{display:block;color:var(--muted);overflow-wrap:anywhere}.page-delete{position:absolute;right:4px;top:50%;transform:translateY(-50%);display:grid;place-items:center;padding:5px;color:#e39b9b;opacity:0;pointer-events:none}.page-list li:hover .page-delete,.page-list li:focus-within .page-delete{opacity:1;pointer-events:auto}.accordion-add{border-top:1px solid var(--line);margin-top:4px;padding-top:12px}.accordion-add button{width:100%}
  @media(hover:none){.page-delete{opacity:1;pointer-events:auto}}
  .panel-properties-layout{position:relative;overflow:hidden;min-width:0}.properties-overlay{position:absolute;inset:0 0 0 auto;width:50%;box-sizing:border-box;padding:20px;overflow:auto;background:var(--surface);border-left:1px solid var(--line);box-shadow:-12px 0 32px #0003;z-index:20}.properties-overlay > h3{margin-top:0}.properties-close{float:right;display:grid;place-items:center;padding:6px;margin:-6px -6px 8px 12px;background:transparent;border:0;color:var(--muted)}
  .asset-import{display:none}.asset-list{list-style:none;padding:0;margin:0;display:grid;gap:6px}.asset-list li{display:flex;align-items:center;gap:8px;min-width:0;padding:7px;border:1px solid var(--line);border-radius:5px}.asset-list img{width:30px;height:30px;object-fit:contain;flex-shrink:0;background:var(--bg);border-radius:3px}.asset-list li > div{flex:1;min-width:0}.asset-list strong,.asset-list small{display:block;font-size:10px;overflow-wrap:anywhere}.asset-list small{font-size:9px;color:var(--muted);margin-top:4px}.asset-list .assets-empty{font-size:11px;color:var(--muted);border-style:dashed}.asset-delete{display:grid;place-items:center;flex-shrink:0;padding:5px;opacity:0;pointer-events:none;color:#e39b9b}.asset-list li:hover .asset-delete,.asset-list li:focus-within .asset-delete{opacity:1;pointer-events:auto}@media(hover:none){.asset-delete{opacity:1;pointer-events:auto}}
  .layout-tools button{display:grid;place-items:center;width:36px;height:36px;padding:0}.layout-tools button.active{color:var(--accent);background:var(--accent-bg);border-color:var(--accent)}
  .workspace > aside.panel-library{padding:0}.editor-accordion{margin:0;border-bottom:1px solid var(--line);interpolate-size:allow-keywords}.editor-accordion:last-child{border-bottom:0}.editor-accordion > summary{padding:14px;font-size:13px;font-weight:600;color:var(--text);cursor:pointer}.accordion-content{padding:0 14px 14px;display:grid;gap:8px;min-width:0}.accordion-content .hint,.accordion-content label{margin:0}.accordion-content input,.accordion-content select{width:100%}.accordion-content button{max-width:100%}
  .editor-accordion::details-content{height:0;overflow:clip;transition:height 180ms ease,content-visibility 180ms allow-discrete}.editor-accordion[open]::details-content{height:auto}
  @media(prefers-reduced-motion:reduce){.editor-accordion::details-content{transition:none}}
  .editor-title{min-width:min(100%,240px);flex:1}.editor-title-actions{display:flex;align-items:center;justify-content:flex-end;gap:8px;flex-wrap:wrap}.editor-title .panel-title-button{display:inline-flex;align-items:center;gap:12px;max-width:100%;text-align:left;font:inherit;letter-spacing:inherit;background:transparent;border:0;padding:0;color:inherit}.panel-title-button span{overflow-wrap:anywhere}.panel-title-button :global(svg){flex-shrink:0;color:var(--muted)}.editor-title .panel-name-input{font:inherit;letter-spacing:inherit;width:100%;padding:0 6px;margin:0;line-height:inherit}.editor-title-actions .delete-panel{color:#e39b9b}
  .designer{color:var(--text);min-width:0}.designer>[hidden]{display:none}
  .viewport-list{display:grid;gap:6px}.viewport-list button{text-align:left;overflow-wrap:anywhere}.viewport-list button.active{border-color:var(--accent);background:var(--accent-bg,var(--surface-raised));color:var(--accent)}.screen-dimension{width:92px}.layout-error{border:1px solid #d9666c;background:#52262c44;color:#ffb9bd;border-radius:5px;padding:10px;font-size:12px}.viewport-properties .eyebrow{margin-top:0}
  .back-panels{display:inline-flex;align-items:center;gap:6px;background:transparent;border:0;color:var(--accent);font-size:10px;padding:6px 0;margin-bottom:18px}.back-panels:hover{background:transparent;text-decoration:underline}
  .editor-heading{flex-wrap:wrap}.editor-heading h1{overflow-wrap:anywhere}
  .panel-list-actions{display:flex;gap:8px;align-items:center;flex-shrink:0}.panel-import{display:none}.action,.toolbar button{display:inline-flex;align-items:center;justify-content:center;gap:6px}.panels-section{margin-top:28px;padding-top:24px;border-top:1px solid var(--line)}.panel-list-heading{display:flex;gap:16px;align-items:center;justify-content:space-between;flex-wrap:wrap;margin:20px 0}
  .panel-tiles{display:grid;grid-template-columns:repeat(auto-fill,minmax(min(100%,240px),1fr));gap:18px;margin:20px 0}.panel-tile-container{position:relative;min-width:0}.panel-tile{padding:0;width:100%;height:100%;display:flex;flex-direction:column;text-align:left;overflow:hidden;border-radius:10px;background:var(--surface)}.panel-thumbnail{display:block;width:100%;aspect-ratio:16/9}.panel-tile-title{width:100%;box-sizing:border-box;font-size:17px;margin:0;padding:16px 96px 16px 16px;min-height:64px;overflow:hidden;white-space:nowrap;text-overflow:ellipsis}.panel-tile-actions{position:absolute;top:12px;right:10px;display:flex;gap:6px}.panel-tile-actions button{display:grid;place-items:center;width:32px;height:32px;padding:0}.panel-tile-delete{color:var(--danger,#e67979)}.tile-copy{padding:16px;flex:1}.tile-copy p{margin:0 0 10px}.tile-copy span{font-size:12px;color:var(--accent)}.empty-panels{padding:60px 24px;text-align:center;border:1px dashed var(--line);border-radius:10px;margin:20px 0}

  h2{font-size:22px;margin:0}h3{font-size:13px;margin:18px 0 10px}p{font-size:13px;color:var(--muted);line-height:1.6}.toolbar{display:flex;gap:8px;flex-wrap:wrap;align-items:end;margin:12px 0}.workspace{display:grid;grid-template-columns:220px minmax(0,1fr);gap:18px}.workspace>aside,.panel-canvas-workspace{min-width:0}.workspace>aside{padding:16px;background:var(--surface);border:1px solid var(--line);border-radius:7px;align-self:start}.palette{display:grid;grid-template-columns:1fr 1fr;gap:5px}
  button,select,input,textarea{font:inherit;font-size:12px;border:1px solid var(--line);border-radius:5px;background:var(--surface-raised);color:var(--text);padding:8px}button{cursor:pointer}button:hover{border-color:var(--accent)}button:disabled{opacity:.4;cursor:default}label{display:flex;flex-direction:column;gap:5px;font-size:12px;margin:7px 0}input,select,textarea{max-width:100%;min-width:0;box-sizing:border-box}input[type=checkbox]{width:auto;accent-color:var(--accent)}.bounds{display:grid;grid-template-columns:1fr 1fr;gap:6px}.hint{font-size:11px;line-height:1.5;overflow-wrap:anywhere}code{font-size:10px;overflow-wrap:anywhere}details{margin-top:20px}textarea{width:100%;font-family:monospace}
  @media(max-width:1050px){.workspace{grid-template-columns:220px minmax(0,1fr)}}@media(max-width:650px){.workspace{grid-template-columns:1fr}.panel-list-actions{flex-wrap:wrap}.editor-heading{align-items:flex-start}}
  .shared-widget-library{margin:12px 0}.shared-widget-library>summary{font-size:12px;color:#bed7cf}.shared-widget-list{display:flex;flex-direction:column;gap:4px;margin:10px 0}.shared-widget-list>div{display:flex;gap:3px}.shared-widget-list button{flex:1;min-width:0;text-align:left;overflow:hidden;text-overflow:ellipsis;white-space:nowrap;font-size:11px;padding:7px 9px;background:#19332f;border:1px solid #36574b;border-radius:4px;color:#c1ded2;cursor:pointer}.shared-widget-list button.edit-library-widget{flex:0 0 27px;padding:4px;text-align:center;background:#1c2c35;border-color:#344a58}.widget-library-actions{display:flex;gap:5px;flex-wrap:wrap}.widget-library-actions button{padding:6px 8px;font:10px inherit;border:1px solid #3f5d58;border-radius:4px;background:#203b32;color:#d5e6de;cursor:pointer}
</style>
