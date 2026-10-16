<script lang="ts">
  import { firmwareWidgetPresets, simplifyFirmwareWidgets } from './firmware-widgets';
  import { restoreCompanionPanels } from './companion-startup';
  import Notification from './Notification.svelte';
  import { onMount, tick } from 'svelte';
  import Icon from './Icon.svelte';
  import CompanionConnections from './CompanionConnections.svelte';
  import { gatewayRequest } from './gateway';
  import { sendPublication, type CompanionCommand, type CompanionHostState } from './companions';
  import { synchronizePanel, removePanelAssignments } from './panel-actions';
  import {
    newDocument, createDesignerHistory, applyDesignerCommand, undoDesigner, redoDesigner,
    createLibrary, saveDraft, duplicateDraft, openDraft, deleteDraft,
    importAsset, validateAssets, exportBundle, importBundle, publishDocument,
    flightTemplate, racingTemplate,
    type DesignerDocument, type DesignerHistory, type DesignerWidget, type AssetPayload,
    type Publication, type Bounds,
  } from '@opensimgear/companion-designer';
  import { PanelCanvas, type PreviewController, type PreviewSnapshot } from '@opensimgear/companion-ui';

  let { openSettings }: { openSettings: () => void } = $props();

  let importInput = $state<HTMLInputElement>();
  let editorOpen = $state(false);
  let dirty = $state(false);
  let assignmentSelection = $state<string[] | null>(null);
  let hostState = $state.raw<CompanionHostState>({});
  let library = $state.raw(createLibrary());
  let history = $state.raw<DesignerHistory>(createDesignerHistory(newDocument('New panel')));
  let draftId = $state<string>(crypto.randomUUID());
  let name = $state('New panel');
  let assets = $state.raw<AssetPayload[]>([]);
  let publications = $state.raw<Record<string, Publication[]>>({});
  let pageId = $state('');
  let selected = $state<string[]>([]);
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
  let store: import('./companion-library').CompanionLibraryStore | undefined;
  const document = $derived(history.present);
  const page = $derived(document.ui.pages.find(p => p.id === pageId) ?? document.ui.pages[0]);
  const widget = $derived(document.presentation.widgets.find(w => w.id === selected[0]));
  const viewport = $derived(document.presentation.viewports.find(v => v.pageId === page.id)!);
  const activeAssets = $derived(assets.filter(payload => document.ui.assets.some(a => a.id === payload.assetId)));
  const imageAssets = $derived(Object.fromEntries(activeAssets.map(payload => [payload.assetId, `data:${document.ui.assets.find(a => a.id === payload.assetId)?.mime};base64,${payload.base64}`])));
  const currentPublications = $derived(library.drafts.flatMap(d => publications[d.id]?.slice(-1) ?? []));
  function panelHashes(id: string) { return (publications[id] ?? []).map(p => p.publicationHash); }
  function panelAssignments(id: string) {
    const hashes = new Set(panelHashes(id));
    return (hostState.host?.instances ?? []).filter(i => typeof i.publicationHash==='string' && hashes.has(i.publicationHash));
  }
  const assignedCompanions = $derived(assignmentSelection ?? panelAssignments(draftId).map(i => String(i.companionId)));
  function toggleAssignment(id: string, checked: boolean) {
    assignmentSelection = checked ? [...new Set([...assignedCompanions,id])] : assignedCompanions.filter(i => i!==id); dirty=true;
  }
  function leaveEditor() { return !dirty || window.confirm('Discard unsaved panel changes?'); }
  function backToPanels() { if(leaveEditor()){stop();editorOpen=false;dirty=false;} }
  async function send(command: CompanionCommand, companion: Record<string,unknown> = {}) {
    const result=(await gatewayRequest({command,companion})).companions;
    if(!result) throw new Error('Companion updates require the native Cockpit app.');
    hostState={...hostState,...result};return result;
  }
  async function refreshCompanions() { try { await send('companion_status'); } catch { /* Local panels remain editable offline. */ } }


  function stop() { controller?.disarm('Designer changed'); preview = false; }
  async function togglePreview() { if (preview) stop(); else { preview = true; await tick(); controller?.arm(); } }
  function select(ids: string[]) { selected = ids; const next = document.presentation.widgets.find(w => w.id === ids[0]); configText = next ? JSON.stringify(next.config, null, 2) : ''; }
  function command(command: Parameters<typeof applyDesignerCommand>[1]) {
    try { stop(); dirty=true; history = applyDesignerCommand(history, command); if (!document.ui.pages.some(p => p.id === pageId)) pageId = document.ui.pages[0].id; error = ''; }
    catch (e) { error = String(e); }
  }
  function patch(patch: Partial<DesignerWidget>) { if (widget) command({ type: 'update-widget', id: widget.id, patch }); }
  function changeBounds(key: keyof Bounds, value: number) {
    if (widget) command({ type: 'move-widget', id: widget.id, bounds: { ...widget.bounds, [key]: value } });
  }
  function create() {
    if(!leaveEditor())return;editorOpen=true;dirty=true;assignmentSelection=null;
    stop(); name = `Panel ${library.drafts.length + 1}`; draftId = crypto.randomUUID();
    history = createDesignerHistory(newDocument(name)); pageId = document.ui.pages[0].id; selected = []; assets = []; message = '';error='';
  }
  function template(source: DesignerDocument) {
    if(!leaveEditor())return;editorOpen=true;dirty=true;assignmentSelection=null;
    const seed = { schemaVersion: 1 as const, drafts: [{ id: 'template', name: source.ui.title, document: simplifyFirmwareWidgets(source).document, assets: [] }] };
    const entry = duplicateDraft(seed, 'template', source.ui.title).drafts[1];
    stop(); draftId = entry.id; name = entry.name; history = createDesignerHistory(entry.document); assets = entry.assets;
    pageId = document.ui.pages[0].id; select([]); message = 'Template copied into a new unsaved panel';
  }
  function load(id: string) {
    try {
      if(!leaveEditor())return;
      const entry = openDraft(library, id); stop();editorOpen=true;dirty=false;assignmentSelection=null; draftId = entry.id; name = entry.name;
      const simplified=simplifyFirmwareWidgets(entry.document);
      history = createDesignerHistory(simplified.document); assets = entry.assets; pageId = document.ui.pages[0].id; select([]); error = '';
      dirty=simplified.removed>0;
      message=simplified.removed?'Removed '+simplified.removed+' unsupported widget(s). Save to update the panel and its companions.':'';
    } catch (e) { error = String(e); }
  }
  async function operation(action: () => Promise<void>) {
    if (busy) return; stop(); busy = true; error = ''; try { await action(); } catch (e) { error = String(e); } finally { busy = false; }
  }
  async function persist(next = library, revisions = publications) {
    if (!store || !storageReady) throw new Error('Storage has not loaded; existing drafts have been preserved.');
    const saved = await store.save({ schemaVersion: 1, library: next, publications: revisions }); library = saved.library; publications = saved.publications;
  }
  async function compactSnapshots(state: CompanionHostState, beforeUpload = false) {
    const games=(await gatewayRequest({command:'game_get_state'})).games;
    if(!games) return; // Preserve captured game layouts when their references cannot be checked.
    const pins=(state.host?.instances ?? []).flatMap(i => typeof i.publicationHash==='string'?[i.publicationHash]:[]);
    pins.push(...games.profiles.flatMap(p => p.companions?.assignments.map(a => a.publicationHash) ?? []));
    const managedIds=[...new Set([...library.drafts.map(d => d.document.ui.id),...Object.values(publications).flat().map(p => p.document.ui.id)])];
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
        const state=await synchronizePanel(panel,previousHashes,send,assignmentSelection ?? undefined);
        await compactSnapshots(state);
        assignmentSelection=null;message='Panel saved and assigned companions updated';
      } catch(e) { error='Panel saved locally. Companion update failed: '+String(e); }
    });
  }
  async function duplicate() {
    await operation(async () => {
      const seed={schemaVersion:1 as const,drafts:[{id:'copy',name:document.ui.title,document,assets:activeAssets}]};
      const entry=duplicateDraft(seed,'copy').drafts[1];
      const copy=structuredClone(entry.document);copy.ui.title=entry.name;
      const panel=await publishDocument(copy,entry.assets);
      const next=await saveDraft(library,{...entry,document:copy});
      await persist(next,{...publications,[entry.id]:[panel]});dirty=false;load(entry.id);message='Panel duplicated';
    });
  }
  async function removeDraft(id: string) {
    if(!window.confirm('Delete this panel and disconnect it from its companions?'))return;
    await operation(async () => {
      await removePanelAssignments(panelHashes(id),send);
      const nextPublications={...publications};delete nextPublications[id];
      await persist(deleteDraft(library,id),nextPublications);
      editorOpen=false;dirty=false;message='Panel deleted';
      await compactSnapshots(await send('companion_status'));
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
      editorOpen=true;dirty=true;assignmentSelection=null;history = createDesignerHistory(entry.document); assets = entry.assets; draftId = entry.id; name = entry.name;
      pageId = document.ui.pages[0].id; select([]); message = 'Panel imported. Save to add it to Panels.';
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
    command({ type: 'set-viewport', pageId: page.id, width, height, safeInsets: viewport.safeInsets ?? { top: .04, right: .02, bottom: .04, left: .02 }, ...(viewport.backgroundAssetId ? { backgroundAssetId: viewport.backgroundAssetId } : {}) });
  }
  function bindControl(controlId: string, value: string) {
    if (!value) { command({ type: 'unbind', controlId }); return; }
    const [targetId, kind, index] = value.split(':'); command({ type: 'bind', controlId, targetId, kind: kind as 'axis' | 'button' | 'hat', index: Number(index) });
  }
  onMount(() => {
    let alive = true;
    void import('./companion-library').then(async ({ CompanionLibraryStore, indexedDbStorage }) => {
      store = new CompanionLibraryStore(indexedDbStorage()); const snapshot = await store.load();
      if (!alive) return; library = snapshot.library; publications = snapshot.publications; storageReady = true;
      await refreshCompanions();
      await restoreCompanionPanels();
      await refreshCompanions();
    }).catch(e => { if (alive) error = `Cannot open local library: ${String(e)}`; });
    const poll=setInterval(() => { if(!busy) void refreshCompanions(); },2500);
    const cancel = () => stop();
    window.addEventListener('blur', cancel); window.addEventListener('pagehide', cancel);
    return () => { alive = false; clearInterval(poll); stop(); unsubscribe?.(); window.removeEventListener('blur', cancel); window.removeEventListener('pagehide', cancel); };
  });
</script>

<section class="designer" aria-label="Companion panel designer">
  <CompanionConnections publications={currentPublications} {openSettings} />
  <section class="panels-section" aria-label="Panels">
    <h2>Panels</h2>
    {#if error}<Notification message={String(error)} kind="error" />{/if}
    {#if message}<Notification message={String(message)} />{/if}
  {#if !editorOpen}
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

  {:else}
  <div class="editor-heading"><button onclick={backToPanels} disabled={busy}><Icon name="back" size={16} />Panels</button><h3>{name}</h3>{#if dirty}<span>Unsaved changes</span>{/if}</div>
  <div inert={busy}>
  <div class="toolbar">
    <button onclick={save} disabled={busy || !storageReady}><Icon name="save" size={16} />Save panel</button>
    <button onclick={download} disabled={busy}><Icon name="download" size={16} />Export panel</button>
    <button onclick={togglePreview}><Icon name={preview ? 'stop' : 'play'} size={16} />{preview ? 'Stop preview' : 'Preview locally'}</button>
    <button onclick={() => template(flightTemplate)}><Icon name="monitor" size={16} />Flight template</button><button onclick={() => template(racingTemplate)}><Icon name="wheel" size={16} />Racing template</button>
    {#if library.drafts.some(d => d.id===draftId)}<button onclick={duplicate}><Icon name="copy" size={16} />Duplicate panel</button><button onclick={() => removeDraft(draftId)}><Icon name="trash" size={16} />Delete panel</button>{/if}
  </div>
  <details class="panel-assignments" open><summary>Assigned companions</summary>
    {#if !hostState.host?.clients?.length}<p>No paired companions yet. Add one in Companions above.</p>{/if}
    {#each hostState.host?.clients ?? [] as client}
      <label class="companion-choice"><input type="checkbox" checked={assignedCompanions.includes(String(client.companionId))} onchange={e => toggleAssignment(String(client.companionId),e.currentTarget.checked)} />{String(client.name || 'Companion')} <small>{client.connected?'Connected':'Offline'} · {String(client.companionId).slice(-6)}</small></label>
    {/each}
    <p class="hint">Select one or more companions. Save panel applies the layout and assignments.</p>
  </details>
  <div class="workspace">
    <aside aria-label="Panel library">
      <h3>Widgets</h3>
      <div class="palette">{#each firmwareWidgetPresets as preset}<button disabled={preview} onclick={() => { command({ type: 'add-widget', kind: preset.kind, pageId: page.id }); select([document.presentation.widgets.at(-1)!.id]); }}>{preset.title}</button>{/each}</div>
      <h3>Assets</h3><p class="hint">PNG, JPEG or WebP · 2 MiB each · 8 MiB total</p>
      <input aria-label="Add panel image" type="file" accept="image/png,image/jpeg,image/webp" disabled={busy} onchange={e => void addAsset(e.currentTarget.files?.[0])} />
      <label>Page background<select value={viewport.backgroundAssetId ?? ''} onchange={e => command({ type: 'set-viewport', ...viewport, backgroundAssetId: e.currentTarget.value || undefined })}><option value="">None</option>{#each document.ui.assets as a}<option value={a.id}>{a.id}</option>{/each}</select></label>
    </aside>
    <main>
      <div class="toolbar"><label>Panel name<input bind:value={name} maxlength="120" onchange={() => command({ type: 'replace', document: { ...document, ui: { ...document.ui, title: name } } })} /></label>
        <button disabled={!history.past.length} onclick={() => { stop(); dirty=true;history = undoDesigner(history); name = document.ui.title; select([]); }}><Icon name="undo" size={16} />Undo</button><button disabled={!history.future.length} onclick={() => { stop(); dirty=true;history = redoDesigner(history); name = document.ui.title; select([]); }}><Icon name="redo" size={16} />Redo</button></div>
      <div class="toolbar"><label>Page<select value={page.id} onchange={e => { stop(); pageId = e.currentTarget.value; select([]); }}>{#each document.ui.pages as p}<option value={p.id}>{p.title}</option>{/each}</select></label>
        <button onclick={() => { command({ type: 'add-page', title: `Page ${document.ui.pages.length + 1}` }); pageId = document.ui.pages.at(-1)!.id; select([]); }}><Icon name="plus" size={16} />Add page</button>
        <button disabled={document.ui.pages.length <= 1} onclick={() => { command({ type: 'delete-page', id: page.id }); select([]); }}><Icon name="trash" size={16} />Remove page</button>
        <label>Viewport<select value={`${viewport.width}x${viewport.height}`} onchange={e => { const [w,h] = e.currentTarget.value.split('x').map(Number); setViewport(w,h); }}><option value="390x844">Phone portrait</option><option value="844x390">Phone landscape</option><option value="768x1024">Tablet portrait</option><option value="1024x768">Tablet landscape</option><option value="800x480">ESP32 screen · 800 × 480</option><option value="1280x800">Large panel · 1280 × 800</option><option value="1280x720">Desktop panel</option></select></label>
        <button onclick={() => setViewport(viewport.height, viewport.width)}><Icon name="refresh" size={16} />Rotate</button>
        <label>Safe area<select value={viewport.safeInsets?.top ? 'inset' : 'none'} onchange={e => command({ type: 'set-viewport', ...viewport, safeInsets: e.currentTarget.value === 'inset' ? { top: .04, right: .02, bottom: .04, left: .02 } : { top: 0, right: 0, bottom: 0, left: 0 } })}><option value="none">Full screen</option><option value="inset">Phone inset</option></select></label>
      </div>
      <div class="toolbar"><label><input type="checkbox" checked={document.presentation.grid.snap} onchange={e => command({ type: 'set-grid', ...document.presentation.grid, snap: e.currentTarget.checked })} />Snap to grid</label>
        <button disabled={!selected.length} onclick={() => command({ type: 'snap', ids: selected })}>Snap selection</button>
        <button disabled={selected.length < 2} onclick={() => command({ type: 'align', ids: selected, edge: 'left' })}>Align left</button>
        <button disabled={selected.length < 2} onclick={() => command({ type: 'align', ids: selected, edge: 'center-y' })}>Align centres</button>
        <button disabled={selected.length < 2} onclick={() => command({ type: 'add-group', pageId: page.id, label: 'Group', widgetIds: selected })}>Group</button>
        <button disabled={!widget?.groupId} onclick={() => widget?.groupId && command({ type: 'ungroup', id: widget.groupId })}>Ungroup</button>
      </div>
      <p class="hint">{preview ? 'Local requested values only. Press controls to preview; no network or virtual device is started.' : 'Drag to move. Arrow keys nudge. Shift-click selects several widgets. Resize using the bounds inspector.'}</p>
      <PanelCanvas {document} pageId={page.id} assets={imageAssets} mode={preview ? 'preview' : 'edit'} selectedIds={selected} onselect={select} oncontroller={value => { unsubscribe?.(); controller = value; unsubscribe = value.subscribe(snapshot => previewSnapshot = snapshot); }} onlayout={(id,bounds) => command({ type: 'move-widget', id, bounds })} />
      {#if preview && previewSnapshot}<p role="status" class="hint">Preview {previewSnapshot.armed ? 'armed' : 'disarmed'} · {previewSnapshot.activePointers} active pointers · {previewSnapshot.events.length} recorded transitions · {previewSnapshot.reason}</p><details><summary>Local requested values</summary>{#each document.ui.controls as control}<p class="hint">{control.label}: {String(previewSnapshot.values[control.id])}</p>{/each}</details>{/if}
    </main>
    <aside aria-label="Widget properties"><h3>Properties</h3>
      {#if widget}
        <p class="hint">{widget.kind} · <code>{widget.id}</code></p>
        <label>Label<input value={widget.label} onchange={e => patch({ label: e.currentTarget.value })} /></label>
        <div class="bounds">{#each ['x','y','width','height'] as field}<label>{field}<input type="number" min="0" max="1" step="0.01" value={widget.bounds[field as keyof Bounds]} onchange={e => changeBounds(field as keyof Bounds, e.currentTarget.valueAsNumber)} /></label>{/each}</div>
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
      {:else}<p>Select a widget to edit. Shift-click to select a group.</p>{/if}
      <details><summary>Logical target layouts</summary><p class="hint">Portable descriptors and mappings. Machine slots and credentials are stored separately.</p><button onclick={() => { targetText = JSON.stringify(document.targets, null, 2); }}><Icon name="sliders" size={16} />Edit layouts</button><textarea aria-label="Logical target layouts JSON" rows="8" bind:value={targetText}></textarea><button onclick={() => { try { command({ type: 'replace', document: { ...document, targets: JSON.parse(targetText) } }); } catch (e) { error = String(e); } }}><Icon name="check" size={16} />Apply layouts</button></details>
    </aside>
  </div>
  </div>
  {/if}
  </section>
</section>

<style>
  .panel-list-actions{display:flex;gap:8px;align-items:center;flex-shrink:0}.panel-import{display:none}.action,.toolbar button,.editor-heading button{display:inline-flex;align-items:center;justify-content:center;gap:6px}.panel-list-heading{flex-wrap:wrap}.panels-section{margin-top:28px;padding-top:24px;border-top:1px solid var(--border,#344452)}.panel-list-heading,.editor-heading{display:flex;gap:16px;align-items:center;justify-content:space-between;margin:20px 0}.editor-heading h3{font-size:20px;margin:0}.editor-heading{justify-content:flex-start}.editor-heading span{font-size:12px;color:var(--muted,#9aafbf)}.panel-tiles{display:grid;grid-template-columns:repeat(auto-fill,minmax(240px,1fr));gap:18px;margin:20px 0}.panel-tile{padding:0;text-align:left;overflow:hidden;border-radius:10px;background:var(--panel,#14212c)}.panel-thumbnail{display:block;width:100%;aspect-ratio:16/9}.tile-copy{padding:16px}.tile-copy h3{font-size:17px;margin:0 0 8px}.tile-copy p{margin:0 0 10px}.tile-copy span{font-size:12px;color:#70dbc8}.empty-panels{padding:60px 24px;text-align:center;border:1px dashed #344452;border-radius:10px;margin:20px 0}.panel-assignments{padding:12px;border:1px solid #344452;border-radius:7px;margin-bottom:18px}.companion-choice{display:inline-flex;flex-direction:row;align-items:center;margin-right:20px}.designer{color:var(--text,#dce7ed);padding:22px;min-width:0}h2{font-size:22px;margin:0}h3{font-size:13px;margin:18px 0 10px}p{font-size:13px;color:var(--muted,#9aafbf)}.toolbar{display:flex;gap:8px;flex-wrap:wrap;align-items:end;margin:12px 0}.workspace{display:grid;grid-template-columns:180px minmax(320px,1fr) 250px;gap:18px}aside{min-width:0}main{min-width:0}.palette{display:grid;grid-template-columns:1fr 1fr;gap:5px}button,select,input,textarea{font:inherit;font-size:12px;border:1px solid #344452;border-radius:5px;background:#14212c;color:#e1eaf0;padding:8px}button{cursor:pointer}button:hover{border-color:#64caba}button:disabled{opacity:.4;cursor:default}label{display:flex;flex-direction:column;gap:5px;font-size:12px;margin:7px 0}input,select,textarea{max-width:100%;box-sizing:border-box}input[type=checkbox]{width:auto}.bounds{display:grid;grid-template-columns:1fr 1fr;gap:6px}.hint{font-size:11px;line-height:1.5;overflow-wrap:anywhere}code{font-size:10px;overflow-wrap:anywhere}details{margin-top:20px}textarea{width:100%;font-family:monospace}@media(max-width:1050px){.workspace{grid-template-columns:150px minmax(280px,1fr)}.workspace>aside:last-child{grid-column:1/-1}}@media(max-width:650px){.workspace{display:block}}
</style>
