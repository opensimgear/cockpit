<script lang="ts">
  import { firmwareWidgetPresets, simplifyFirmwareWidgets } from './firmware-widgets';
  import { restoreCompanionPanels } from './companion-startup';
  import Notification from './Notification.svelte';
  import { onMount, tick, untrack } from 'svelte';
  import Icon from './Icon.svelte';
  import CompanionConnections from './CompanionConnections.svelte';
  import { gatewayRequest } from './gateway';
  import { ProfilesClient } from './profiles';
  import { sendPublication, type CompanionCommand, type CompanionHostState } from './companions';
  import { synchronizePanel, removePanelAssignments } from './panel-actions';
  import {
    newDocument, createDesignerHistory, applyDesignerCommand, undoDesigner, redoDesigner,
    createLibrary, saveDraft, duplicateDraft, openDraft,
    importAsset, validateAssets, exportBundle, importBundle, publishDocument,
    viewportRegions, widgetViewport, boundsContain,
    type DesignerHistory, type DesignerWidget, type AssetPayload,
    type Publication, type Bounds,
  } from '@opensimgear/companion-designer';
  import { PanelCanvas, type PreviewController, type PreviewSnapshot } from '@opensimgear/companion-ui';

  let { openSettings, initialCompanionId = '', onNavigate, active = true }: { openSettings: () => void; initialCompanionId?: string; onNavigate?: (page: CompanionNavigation) => void; active?: boolean } = $props();

  let importInput = $state<HTMLInputElement>();
  let assetInput = $state<HTMLInputElement>();
  let connectionsPanel = $state.raw<{showMain:(recordNavigation?:boolean)=>boolean;getCompanionId:()=>string;showCompanion:(id:string)=>void;requestNavigation:(continueNavigation:()=>void,cancelNavigation?:()=>void)=>void}>();
  let editorOpen = $state(false);
  let companionDetailsOpen = $state(untrack(() => !!initialCompanionId));
  let dirty = $state(false);
  let hostState = $state.raw<CompanionHostState>({});
  let library = $state.raw(createLibrary());
  let history = $state.raw<DesignerHistory>(createDesignerHistory(newDocument('New panel')));
  let draftId = $state<string>(crypto.randomUUID());
  let name = $state('New panel');
  let editingName = $state(false), nameDraft = $state('');
  let assets = $state.raw<AssetPayload[]>([]);
  let publications = $state.raw<Record<string, Publication[]>>({});
  let pageId = $state('');
  let selected = $state<string[]>([]);
  let selectedViewportId = $state('');
  let preview = $state(false);
  let controller: PreviewController | undefined;
  let unsubscribe: (() => void) | undefined;
  let previewSnapshot = $state.raw<PreviewSnapshot>();
  let busy = $state(false);
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
  const page = $derived(document.ui.pages.find(p => p.id === pageId) ?? document.ui.pages[0]);
  const widget = $derived(document.presentation.widgets.find(w => w.id === selected[0]));
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
  const currentPublications = $derived(library.drafts.flatMap(d => publications[d.id]?.slice(-1) ?? []));
  function panelHashes(id: string) { return (publications[id] ?? []).map(p => p.publicationHash); }
  function panelAssignments(id: string) {
    const hashes = new Set(panelHashes(id));
    return (hostState.host?.instances ?? []).filter(i => typeof i.publicationHash==='string' && hashes.has(i.publicationHash));
  }
  function leaveEditor() { return !dirty || window.confirm('Discard unsaved panel changes?'); }
  function backToPanels() { if(leaveEditor()){stop();editorOpen=false;dirty=false;discardPanelOnRestore=false;onNavigate?.({kind:'main'});} }
  export function showMain() {
    if (editorOpen && !leaveEditor()) return false;
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
    if (busy || editorOpen && !leaveEditor()) { cancelNavigation?.(); return; }
    const approve=() => { discardPanelOnRestore=editorOpen && dirty; stop(); continueNavigation(); };
    if (connectionsPanel) connectionsPanel.requestNavigation(approve,cancelNavigation);
    else approve();
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
  $effect(() => { if (!active) untrack(stop); else if (mounted) untrack(() => { void refreshCompanions(); }); });


  function stop() { controller?.disarm('Designer changed'); preview = false; }
  async function togglePreview() { if (preview) stop(); else { preview = true; await tick(); controller?.arm(); } }
  function select(ids: string[]) {
    selected = ids;
    const next = document.presentation.widgets.find(w => w.id === ids[0]);
    configText = next ? JSON.stringify(next.config, null, 2) : '';
    if (next) {
      const region = widgetViewport(document, next);
      if (region) selectedViewportId = region.id;
    }
  }
  function selectViewport(id: string) { selectedViewportId = id; select([]); }
  function command(command: Parameters<typeof applyDesignerCommand>[1]) {
    try { stop(); history = applyDesignerCommand(history, command); dirty=true; if (!document.ui.pages.some(p => p.id === pageId)) pageId = document.ui.pages[0].id; error = ''; return true; }
    catch (e) { error = e instanceof Error ? e.message : String(e); return false; }
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
  function changeViewportBounds(key: keyof Bounds, input: HTMLInputElement) {
    const scale = key === 'x' || key === 'width' ? usableWidth : usableHeight;
    command({ type: 'update-viewport', pageId: page.id, id: activeViewport.id, patch: { bounds: { ...activeViewport.bounds, [key]: input.valueAsNumber / scale } } });
    input.value = String(Math.round(activeViewport.bounds[key] * scale * 100) / 100);
  }
  function patch(patch: Partial<DesignerWidget>) { if (widget) command({ type: 'update-widget', id: widget.id, patch }); }
  function changeBounds(key: keyof Bounds, input: HTMLInputElement) {
    if (widget) {
      command({ type: 'move-widget', id: widget.id, bounds: { ...widget.bounds, [key]: input.valueAsNumber } });
      input.value = String(widget.bounds[key]);
    }
  }
  function create() {
    if(!leaveEditor())return;editorOpen=true;dirty=true;
    stop(); name = `Panel ${library.drafts.length + 1}`; draftId = crypto.randomUUID();
    history = createDesignerHistory(newDocument(name)); pageId = document.ui.pages[0].id; selected = []; assets = []; message = '';error='';
    onNavigate?.({kind:'panel',id:draftId});
  }
  function load(id: string, recordNavigation = true, checkLeave = true) {
    try {
      if(checkLeave && !leaveEditor())return;
      const entry = openDraft(library, id); stop();editorOpen=true;dirty=false; draftId = entry.id; name = entry.name;
      const simplified=simplifyFirmwareWidgets(entry.document);
      history = createDesignerHistory(simplified.document); assets = entry.assets; pageId = document.ui.pages[0].id; select([]); error = '';
      dirty=simplified.removed>0;
      message=simplified.removed?'Removed '+simplified.removed+' unsupported widget(s). Save to update the panel and its companions.':'';
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
    await operation(async () => {
      const previousHashes=panelHashes(draftId);
      const simplified=simplifyFirmwareWidgets(document);
      if(simplified.removed)history=createDesignerHistory(simplified.document);
      const panel=await publishDocument(document,activeAssets);
      const next=await saveDraft(library,{id:draftId,name:document.ui.title,document,assets:activeAssets});
      await persist(next,{...publications,[draftId]:[panel]});dirty=false;
      message='Panel saved';
      try {
        await compactSnapshots(await send('companion_status'),true);
        const state=await synchronizePanel(panel,previousHashes,send);
        await compactSnapshots(state);
        message='Panel saved and assigned companions updated';
      } catch(e) { error='Panel saved locally. Companion update failed: '+String(e); }
    });
  }
  async function removeDraft(id: string) {
    if(!window.confirm('Delete this panel and disconnect it from its companions?'))return;
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
      const seed={schemaVersion:1 as const,drafts:[{id:'import',name:result.document.ui.title,document:simplifyFirmwareWidgets(result.document).document,assets:result.assets}]};
      const entry=duplicateDraft(seed,'import',result.document.ui.title).drafts[1];
      editorOpen=true;dirty=true;history = createDesignerHistory(entry.document); assets = entry.assets; draftId = entry.id; name = entry.name;
      pageId = document.ui.pages[0].id; select([]); message = 'Panel imported. Save to add it to Panels.';
      onNavigate?.({kind:'panel',id:draftId});
    });
  }
  async function download() {
    await operation(async () => {
      const text = await exportBundle(document, activeAssets); const url = URL.createObjectURL(new Blob([text], { type: 'application/json' }));
      const link = window.document.createElement('a'); link.href = url; link.download = `${name.replace(/[^a-z0-9_-]/gi, '-')}.osg-panel.json`; link.click(); URL.revokeObjectURL(url); message = 'Portable bundle exported';
    });
  }
  async function addAsset(file: File | undefined) {
    if (!file) return;
    await operation(async () => {
      if (file.size > 2 * 1024 * 1024) throw new Error('Each image must be at most 2 MiB');
      const result = await importAsset(document, new Uint8Array(await file.arrayBuffer()), file.type as 'image/png' | 'image/jpeg' | 'image/webp');
      const nextAssets = [...assets.filter(a => a.assetId !== result.asset.assetId), result.asset];
      await validateAssets(result.document, nextAssets.filter(payload => result.document.ui.assets.some(asset => asset.id === payload.assetId)));
      command({ type: 'replace', document: result.document }); assets = nextAssets; message = `Image added (${result.width} × ${result.height})`;
    });
  }
  function removeAsset(id: string) {
    const next = structuredClone(document);
    next.ui.assets = next.ui.assets.filter(asset => asset.id !== id);
    for (const viewport of next.presentation.viewports) if (viewport.backgroundAssetId === id) delete viewport.backgroundAssetId;
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
      if (!alive) return; library = snapshot.library; publications = snapshot.publications; storageReady = true; resolveStorageLoad();
      await refreshCompanions();
      await restoreCompanionPanels();
      await refreshCompanions();
    }).catch(e => { if (alive) error = `Cannot open local library: ${String(e)}`; }).finally(resolveStorageLoad);
    const poll=setInterval(() => { if(active && !busy) void refreshCompanions(); },2500);
    const cancel = () => stop();
    window.addEventListener('blur', cancel); window.addEventListener('pagehide', cancel);
    return () => { alive = false; disposed=true; resolveStorageLoad(); clearInterval(poll); stop(); unsubscribe?.(); window.removeEventListener('blur', cancel); window.removeEventListener('pagehide', cancel); };
  });
</script>

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
    <div class="panel-list-heading"><p>Create a panel, edit its layout and assign it to your companions.</p><div class="panel-list-actions"><button class="action" disabled={!storageReady || busy} onclick={() => importInput?.click()}><Icon name="upload" size={16} />Import panel</button><input bind:this={importInput} class="panel-import" type="file" accept=".json,.osg-panel" onchange={e => { void importFile(e.currentTarget.files?.[0]); e.currentTarget.value = ''; }} disabled={!storageReady || busy} /><button class="action" disabled={!storageReady || busy} onclick={create}><Icon name="plus" size={16} />New panel</button></div></div>
    {#if !storageReady}<p>Loading panels…</p>
    {:else if !library.drafts.length}<div class="empty-panels"><h3>No panels yet</h3><p>Create your first panel to get started.</p></div>
    {:else}<div class="panel-tiles">
      {#each library.drafts as entry (entry.id)}
        <button class="panel-tile" onclick={() => load(entry.id)}>
          <svg class="panel-thumbnail" viewBox="0 0 320 180" aria-hidden="true">
            <rect width="320" height="180" fill="#101b25" />
            {#each entry.document.presentation.widgets.filter(w => w.pageId===entry.document.ui.pages[0].id) as item}
              <rect x={item.bounds.x*320} y={item.bounds.y*180} width={item.bounds.width*320} height={item.bounds.height*180} rx="4" fill={item.style?.background ?? '#233d49'} stroke={item.style?.accent ?? '#59bda8'} stroke-width="1" />
              <text x={(item.bounds.x+item.bounds.width/2)*320} y={(item.bounds.y+item.bounds.height/2)*180} text-anchor="middle" dominant-baseline="middle" fill={item.style?.foreground ?? '#e1eaf0'} font-size="10">{item.label.slice(0,25)}</text>
            {/each}
          </svg>
          <div class="tile-copy"><h3>{entry.name}</h3><p>{entry.document.ui.pages.length} {entry.document.ui.pages.length===1?'page':'pages'} · {entry.document.presentation.widgets.length} {entry.document.presentation.widgets.length===1?'widget':'widgets'}</p><span>{new Set(panelAssignments(entry.id).map(i => i.companionId)).size} {new Set(panelAssignments(entry.id).map(i => i.companionId)).size===1?'companion':'companions'}</span></div>
        </button>
      {/each}
    </div>{/if}

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
      <button class="action" onclick={download} disabled={busy}><Icon name="download" size={16} />Export panel</button>
      {#if library.drafts.some(d => d.id===draftId)}<button class="action delete-panel" disabled={busy} onclick={() => removeDraft(draftId)}><Icon name="trash" size={16} />Delete panel</button>{/if}
      <button class="primary-button" onclick={save} disabled={busy || !storageReady}><Icon name="save" size={16} />{busy ? 'Saving…' : 'Save panel'}</button>
    </div>
  </div>
  {#if error}<Notification message={String(error)} kind="error" />{/if}
  {#if message}<Notification message={String(message)} />{/if}
  <div inert={busy}>
  <div class="toolbar">
    <button onclick={togglePreview}><Icon name={preview ? 'stop' : 'play'} size={16} />{preview ? 'Stop preview' : 'Preview locally'}</button>
  </div>
  <div class="workspace">
    <aside class="panel-library" aria-label="Panel library">
      <details class="editor-accordion" name="panel-editor-tools"><summary>Pages</summary><div class="accordion-content"><label>Page<select value={page.id} onchange={e => { stop(); pageId = e.currentTarget.value; select([]); }}>{#each document.ui.pages as p}<option value={p.id}>{p.title}</option>{/each}</select></label>
        <button onclick={() => { command({ type: 'add-page', title: `Page ${document.ui.pages.length + 1}` }); pageId = document.ui.pages.at(-1)!.id; select([]); }}><Icon name="plus" size={16} />Add page</button>
        <button disabled={document.ui.pages.length <= 1} onclick={() => { command({ type: 'delete-page', id: page.id }); select([]); }}><Icon name="trash" size={16} />Remove page</button>
      </div></details>
      <details class="editor-accordion" name="panel-editor-tools" open><summary>Viewports</summary><div class="accordion-content">
      <p class="hint">Divide one companion screen into areas for your widgets.</p>
      <div class="viewport-list">{#each regions as region}<button class:active={region.id === activeViewport.id} aria-pressed={region.id === activeViewport.id} disabled={preview} onclick={() => selectViewport(region.id)}>{region.label}<small>{Math.round(region.bounds.width * usableWidth)} × {Math.round(region.bounds.height * usableHeight)}</small></button>{/each}</div>
      <button class="add-viewport" disabled={preview || regions.length >= 32} onclick={addViewport}><Icon name="plus" size={14} />Add viewport</button>
      <p class="hint">Uses free space, or splits the selected viewport.</p>
      </div></details>
      <details class="editor-accordion" name="panel-editor-tools"><summary>Widgets</summary><div class="accordion-content">
      <p class="hint">Add to {activeViewport.label}</p>
      <div class="palette">{#each firmwareWidgetPresets as preset}<button disabled={preview} onclick={() => { if (command({ type: 'add-widget', kind: preset.kind, pageId: page.id, viewportId: activeViewport.id })) select([document.presentation.widgets.at(-1)!.id]); }}>{preset.title}</button>{/each}</div>
      </div></details>
      <details class="editor-accordion" name="panel-editor-tools"><summary>Assets</summary><div class="accordion-content"><p class="hint">PNG, JPEG or WebP · 2 MiB each · 8 MiB total</p>
      <button class="action" disabled={busy} onclick={()=>assetInput?.click()}><Icon name="plus" size={14}/>Add asset</button>
      <input class="asset-import" bind:this={assetInput} aria-label="Add panel image" type="file" accept="image/png,image/jpeg,image/webp" disabled={busy} onchange={e => {void addAsset(e.currentTarget.files?.[0]);e.currentTarget.value='';}} />
      <ul class="asset-list" aria-label="Panel assets">{#each document.ui.assets as asset,index (asset.id)}<li data-asset-id={asset.id}>
        <img src={imageAssets[asset.id]} alt=""/><div><strong>Image {index+1}</strong><small>{asset.mime.split('/')[1].toUpperCase()} · {Math.max(1,Math.ceil(asset.sizeBytes/1024))} KB</small></div>
        <button class="asset-delete" aria-label={`Delete image ${index+1}`} title="Delete asset" disabled={busy} onclick={()=>removeAsset(asset.id)}><Icon name="trash" size={15}/></button>
      </li>{:else}<li class="assets-empty">No assets yet.</li>{/each}</ul>
      <label>Page background<select value={viewport.backgroundAssetId ?? ''} onchange={e => command({ type: 'set-viewport', ...viewport, backgroundAssetId: e.currentTarget.value || undefined })}><option value="">None</option>{#each document.ui.assets as a}<option value={a.id}>{a.id}</option>{/each}</select></label>
      </div></details>
      <details class="editor-accordion" name="panel-editor-tools"><summary>Screen size</summary><div class="accordion-content">
        <label>Companion screen<select value={`${viewport.width}x${viewport.height}`} onchange={e => { const [w,h] = e.currentTarget.value.split('x').map(Number); setViewport(w,h); }}><option value="390x844">Phone portrait</option><option value="844x390">Phone landscape</option><option value="768x1024">Tablet portrait</option><option value="1024x768">Tablet landscape</option><option value="800x480">ESP32 screen · 800 × 480</option><option value="1280x800">Large panel · 1280 × 800</option><option value="1280x720">Desktop · 1280 × 720</option><option value="1920x1080">Full HD · 1920 × 1080</option><option value="2560x1440">QHD · 2560 × 1440</option><option value="3840x2160">4K · 3840 × 2160</option>{#if !['390x844','844x390','768x1024','1024x768','800x480','1280x800','1280x720','1920x1080','2560x1440','3840x2160'].includes(`${viewport.width}x${viewport.height}`)}<option value={`${viewport.width}x${viewport.height}`}>Custom · {viewport.width} × {viewport.height}</option>{/if}</select></label>
        <label>Screen width<input class="screen-dimension" type="number" min="320" max="8192" step="1" value={viewport.width} onchange={e => { setViewport(e.currentTarget.valueAsNumber, viewport.height); e.currentTarget.value=String(viewport.width); }} /></label>
        <label>Screen height<input class="screen-dimension" type="number" min="320" max="8192" step="1" value={viewport.height} onchange={e => { setViewport(viewport.width, e.currentTarget.valueAsNumber); e.currentTarget.value=String(viewport.height); }} /></label>
        <button onclick={() => setViewport(viewport.height, viewport.width)}><Icon name="refresh" size={16} />Rotate</button>
        <label>Margin (px)<input type="number" min="0" max={maxMargin} step="1" value={margin} onchange={e => { setMargin(e.currentTarget.valueAsNumber); e.currentTarget.value=String(margin); }}/></label>
        <p class="hint">Applies equally to all four sides. Use 0 for the full screen.</p>
      </div></details>
    </aside>
    <div class="panel-canvas-workspace">
      <div class="toolbar">
        <button disabled={!history.past.length} onclick={() => { stop(); dirty=true;history = undoDesigner(history); name = document.ui.title; select([]); }}><Icon name="undo" size={16} />Undo</button><button disabled={!history.future.length} onclick={() => { stop(); dirty=true;history = redoDesigner(history); name = document.ui.title; select([]); }}><Icon name="redo" size={16} />Redo</button></div>

      <div class="toolbar layout-tools" role="group" aria-label="Widget layout tools">
        <button class:active={document.presentation.grid.snap} aria-label="Snap to grid" title="Snap to grid" aria-pressed={document.presentation.grid.snap} onclick={() => command({ type: 'set-grid', ...document.presentation.grid, snap: !document.presentation.grid.snap })}><Icon name="grid" size={18}/></button>
        <button disabled={selected.length < 2} aria-label="Align left" title="Align left" onclick={() => command({ type: 'align', ids: selected, edge: 'left' })}><Icon name="align-left" size={18}/></button>
        <button disabled={selected.length < 2} aria-label="Align center" title="Align center" onclick={() => command({ type: 'align', ids: selected, edge: 'center-x' })}><Icon name="align-center" size={18}/></button>
        <button disabled={selected.length < 2} aria-label="Align right" title="Align right" onclick={() => command({ type: 'align', ids: selected, edge: 'right' })}><Icon name="align-right" size={18}/></button>
        <button disabled={selected.length < 2} aria-label="Group" title="Group" onclick={() => command({ type: 'add-group', pageId: page.id, label: 'Group', widgetIds: selected })}><Icon name="group" size={18}/></button>
        <button disabled={!widget?.groupId} aria-label="Ungroup" title="Ungroup" onclick={() => widget?.groupId && command({ type: 'ungroup', id: widget.groupId })}><Icon name="ungroup" size={18}/></button>
      </div>
      <div class="canvas-caption"><strong>{viewport.width} × {viewport.height} px</strong><span>{regions.length} {regions.length === 1 ? 'viewport' : 'viewports'}</span></div>
      {#if error}<p class="layout-error" role="alert">{error}</p>{/if}
      <p class="hint">{preview ? 'Local requested values only. Press controls to preview; no network or virtual device is started.' : 'Drag viewport labels to move areas; drag a corner to resize. Widgets must stay inside a viewport. Shift-click selects several widgets.'}</p>
      <PanelCanvas {document} pageId={page.id} assets={imageAssets} mode={preview ? 'preview' : 'edit'} selectedIds={selected} selectedViewportId={activeViewport.id} onselectviewport={selectViewport} onviewportbounds={(id,bounds) => command({ type: 'update-viewport', pageId: page.id, id, patch: { bounds } })} onerror={value => error=value} onselect={select} oncontroller={value => { unsubscribe?.(); controller = value; unsubscribe = value.subscribe(snapshot => previewSnapshot = snapshot); }} onlayout={(id,bounds) => command({ type: 'move-widget', id, bounds })} />
      {#if preview && previewSnapshot}<p role="status" class="hint">Preview {previewSnapshot.armed ? 'armed' : 'disarmed'} · {previewSnapshot.activePointers} active pointers · {previewSnapshot.events.length} recorded transitions · {previewSnapshot.reason}</p><details><summary>Local requested values</summary>{#each document.ui.controls as control}<p class="hint">{control.label}: {String(previewSnapshot.values[control.id])}</p>{/each}</details>{/if}
    </div>
    <aside aria-label="Widget properties"><h3>Properties</h3>
      {#if widget}
        <p class="hint">{widget.kind} · <code>{widget.id}</code></p>
        <label>Label<input value={widget.label} onchange={e => patch({ label: e.currentTarget.value })} /></label>
        <div class="bounds">{#each ['x','y','width','height'] as field}<label>{field}<input type="number" min="0" max="1" step="0.01" value={widget.bounds[field as keyof Bounds]} onchange={e => changeBounds(field as keyof Bounds, e.currentTarget)} /></label>{/each}</div>
        {#each ['foreground','background','accent'] as color}<label>{color}<input type="color" value={widget.style?.[color as keyof NonNullable<DesignerWidget['style']>] ?? (color === 'background' ? '#12232e' : '#55ddc7')} onchange={e => patch({ style: { ...widget.style, [color]: e.currentTarget.value } })} /></label>{/each}
        {#each Object.entries(widget.config).filter(([key])=>!['buttonMode','pulseMs','repeatMs'].includes(key)) as [key,value]}
          {#if typeof value === 'boolean'}<label><input type="checkbox" checked={value} onchange={e => configPatch(key,e.currentTarget.checked)} />{key}</label>
          {:else if typeof value === 'number'}<label>{key}<input type="number" value={value} step={key === 'deadzone' ? .01 : 1} onchange={e => configPatch(key,e.currentTarget.valueAsNumber)} /></label>
          {:else if key === 'centering'}<label>Return behavior<select value={String(value)} onchange={e => configPatch(key,e.currentTarget.value)}><option value="spring">Spring to fallback</option><option value="latched">Keep last position</option></select></label>
          {:else if key === 'orientation'}<label>Orientation<select value={String(value)} onchange={e => configPatch(key,e.currentTarget.value)}><option value="horizontal">Horizontal</option><option value="vertical">Vertical</option></select></label>
          {:else if typeof value === 'string' && key !== 'buttonMode' && key !== 'iconAssetId'}<label>{key}<input value={value} onchange={e => configPatch(key,e.currentTarget.value)} /></label>{/if}
        {/each}
        {#if widget.kind === 'button' || widget.kind === 'icon'}<label>Icon image<select value={widget.config.iconAssetId ?? ''} onchange={e => { const config = { ...widget!.config } as Record<string,unknown>; if(e.currentTarget.value) config.iconAssetId = e.currentTarget.value; else delete config.iconAssetId; patch({ config } as Partial<DesignerWidget>); }}><option value="">None</option>{#each document.ui.assets as asset}<option value={asset.id}>{asset.id}</option>{/each}</select></label>{/if}
        <details><summary>Advanced behavior</summary><label>Validated configuration<textarea rows="8" bind:value={configText}></textarea></label><button onclick={() => { try { patch({ config: JSON.parse(configText) }); } catch (e) { error = String(e); } }}>Apply behavior</button></details>
        <button onclick={() => { command({ type: 'duplicate-widget', id: widget!.id }); select([document.presentation.widgets.at(-1)!.id]); }}>Duplicate widget</button>
        <button onclick={() => { command({ type: 'delete-widget', id: widget!.id }); select([]); }}>Delete widget</button>
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
          <button disabled={preview || regions.length <= 1 || activeViewportOccupied} onclick={() => { if (command({ type: 'remove-viewport', pageId: page.id, id: activeViewport.id })) selectedViewportId=''; }}>Remove viewport</button>
          <p class="hint">Move or delete its widgets before removing a viewport.</p>
        </div>
      {/if}
      <details><summary>Logical target layouts</summary><p class="hint">Portable descriptors and mappings. Machine slots and credentials are stored separately.</p><button onclick={() => { targetText = JSON.stringify(document.targets, null, 2); }}><Icon name="sliders" size={16} />Edit layouts</button><textarea aria-label="Logical target layouts JSON" rows="8" bind:value={targetText}></textarea><button onclick={() => { try { command({ type: 'replace', document: { ...document, targets: JSON.parse(targetText) } }); } catch (e) { error = String(e); } }}><Icon name="check" size={16} />Apply layouts</button></details>
    </aside>
  </div>
  </div>
  </section>
  {/if}
</section>

<style>
  .asset-import{display:none}.asset-list{list-style:none;padding:0;margin:0;display:grid;gap:6px}.asset-list li{display:flex;align-items:center;gap:8px;min-width:0;padding:7px;border:1px solid var(--line);border-radius:5px}.asset-list img{width:30px;height:30px;object-fit:contain;flex-shrink:0;background:var(--bg);border-radius:3px}.asset-list li > div{flex:1;min-width:0}.asset-list strong,.asset-list small{display:block;font-size:10px;overflow-wrap:anywhere}.asset-list small{font-size:9px;color:var(--muted);margin-top:4px}.asset-list .assets-empty{font-size:11px;color:var(--muted);border-style:dashed}.asset-delete{display:grid;place-items:center;flex-shrink:0;padding:5px;opacity:0;pointer-events:none;color:#e39b9b}.asset-list li:hover .asset-delete,.asset-list li:focus-within .asset-delete{opacity:1;pointer-events:auto}@media(hover:none){.asset-delete{opacity:1;pointer-events:auto}}
  .layout-tools button{display:grid;place-items:center;width:36px;height:36px;padding:0}.layout-tools button.active{color:var(--accent);background:var(--accent-bg);border-color:var(--accent)}
  .workspace > aside.panel-library{padding:0}.editor-accordion{margin:0;border-bottom:1px solid var(--line)}.editor-accordion:last-child{border-bottom:0}.editor-accordion > summary{padding:14px;font-size:13px;font-weight:600;color:var(--text);cursor:pointer}.accordion-content{padding:0 14px 14px;display:grid;gap:8px;min-width:0}.accordion-content .hint,.accordion-content label{margin:0}.accordion-content input,.accordion-content select{width:100%}.accordion-content button{max-width:100%}
  .editor-title{min-width:min(100%,240px);flex:1}.editor-title-actions{display:flex;align-items:center;justify-content:flex-end;gap:8px;flex-wrap:wrap}.editor-title .panel-title-button{display:inline-flex;align-items:center;gap:12px;max-width:100%;text-align:left;font:inherit;letter-spacing:inherit;background:transparent;border:0;padding:0;color:inherit}.panel-title-button span{overflow-wrap:anywhere}.panel-title-button :global(svg){flex-shrink:0;color:var(--muted)}.editor-title .panel-name-input{font:inherit;letter-spacing:inherit;width:100%;padding:0 6px;margin:0;line-height:inherit}.editor-title-actions .delete-panel{color:#e39b9b}
  .designer{color:var(--text);min-width:0}.designer>[hidden]{display:none}
  .viewport-list{display:grid;gap:6px}.viewport-list button{text-align:left;overflow-wrap:anywhere}.viewport-list button.active{border-color:var(--accent);background:var(--accent-bg,var(--surface-raised))}.viewport-list small{display:block;color:var(--muted);font-size:10px;margin-top:4px}.add-viewport{display:flex;align-items:center;gap:5px;width:100%;margin-top:8px}.screen-dimension{width:92px}.canvas-caption{display:flex;align-items:center;justify-content:space-between;gap:12px;font-size:11px;color:var(--muted);margin-top:18px}.canvas-caption strong{color:var(--text);font-weight:500}.layout-error{border:1px solid #d9666c;background:#52262c44;color:#ffb9bd;border-radius:5px;padding:10px;font-size:12px}.viewport-properties .eyebrow{margin-top:0}
  .back-panels{display:inline-flex;align-items:center;gap:6px;background:transparent;border:0;color:var(--accent);font-size:10px;padding:6px 0;margin-bottom:18px}.back-panels:hover{background:transparent;text-decoration:underline}
  .editor-heading{flex-wrap:wrap}.editor-heading h1{overflow-wrap:anywhere}.editor-heading .primary-button{display:inline-flex;align-items:center;gap:8px;background:var(--accent);color:var(--accent-text);border-color:transparent;padding:11px 14px;font-size:11px;font-weight:600}
  .panel-list-actions{display:flex;gap:8px;align-items:center;flex-shrink:0}.panel-import{display:none}.action,.toolbar button{display:inline-flex;align-items:center;justify-content:center;gap:6px}.panels-section{margin-top:28px;padding-top:24px;border-top:1px solid var(--line)}.panel-list-heading{display:flex;gap:16px;align-items:center;justify-content:space-between;flex-wrap:wrap;margin:20px 0}
  .panel-tiles{display:grid;grid-template-columns:repeat(auto-fill,minmax(min(100%,240px),1fr));gap:18px;margin:20px 0}.panel-tile{padding:0;text-align:left;overflow:hidden;border-radius:10px;background:var(--surface)}.panel-thumbnail{display:block;width:100%;aspect-ratio:16/9}.tile-copy{padding:16px}.tile-copy h3{font-size:17px;margin:0 0 8px}.tile-copy p{margin:0 0 10px}.tile-copy span{font-size:12px;color:var(--accent)}.empty-panels{padding:60px 24px;text-align:center;border:1px dashed var(--line);border-radius:10px;margin:20px 0}
  h2{font-size:22px;margin:0}h3{font-size:13px;margin:18px 0 10px}p{font-size:13px;color:var(--muted);line-height:1.6}.toolbar{display:flex;gap:8px;flex-wrap:wrap;align-items:end;margin:12px 0}.workspace{display:grid;grid-template-columns:180px minmax(0,1fr) 250px;gap:18px}.workspace>aside,.panel-canvas-workspace{min-width:0}.workspace>aside{padding:16px;background:var(--surface);border:1px solid var(--line);border-radius:7px;align-self:start}.workspace>aside>h3:first-child{margin-top:0}.palette{display:grid;grid-template-columns:1fr 1fr;gap:5px}
  button,select,input,textarea{font:inherit;font-size:12px;border:1px solid var(--line);border-radius:5px;background:var(--surface-raised);color:var(--text);padding:8px}button{cursor:pointer}button:hover{border-color:var(--accent)}button:disabled{opacity:.4;cursor:default}label{display:flex;flex-direction:column;gap:5px;font-size:12px;margin:7px 0}input,select,textarea{max-width:100%;min-width:0;box-sizing:border-box}input[type=checkbox]{width:auto;accent-color:var(--accent)}.bounds{display:grid;grid-template-columns:1fr 1fr;gap:6px}.hint{font-size:11px;line-height:1.5;overflow-wrap:anywhere}code{font-size:10px;overflow-wrap:anywhere}details{margin-top:20px}textarea{width:100%;font-family:monospace}
  @media(max-width:1050px){.workspace{grid-template-columns:180px minmax(0,1fr)}.workspace>aside:last-child{grid-column:1/-1}}@media(max-width:650px){.workspace{grid-template-columns:1fr}.workspace>aside:last-child{grid-column:auto}.panel-list-actions{flex-wrap:wrap}.editor-heading{align-items:flex-start}}
</style>
