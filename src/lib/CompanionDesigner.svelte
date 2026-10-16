<script lang="ts">
  import { onMount, tick } from 'svelte';
  import CompanionConnections from './CompanionConnections.svelte';
  import {
    newDocument, createDesignerHistory, applyDesignerCommand, undoDesigner, redoDesigner,
    widgetPresets, createLibrary, saveDraft, duplicateDraft, openDraft, deleteDraft,
    importAsset, validateAssets, exportBundle, importBundle, publishDocument, compareDesignerPublications,
    flightTemplate, racingTemplate,
    type DesignerDocument, type DesignerHistory, type DesignerWidget, type AssetPayload,
    type Publication, type Bounds,
  } from '@opensimgear/companion-designer';
  import { PanelCanvas, type PreviewController, type PreviewSnapshot } from '@opensimgear/companion-ui';

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
  let axisMode = $state<'bipolar' | 'unipolar'>('bipolar');
  let store: import('./companion-library').CompanionLibraryStore | undefined;
  const document = $derived(history.present);
  const page = $derived(document.ui.pages.find(p => p.id === pageId) ?? document.ui.pages[0]);
  const widget = $derived(document.presentation.widgets.find(w => w.id === selected[0]));
  const viewport = $derived(document.presentation.viewports.find(v => v.pageId === page.id)!);
  const activeAssets = $derived(assets.filter(payload => document.ui.assets.some(a => a.id === payload.assetId)));
  const imageAssets = $derived(Object.fromEntries(activeAssets.map(payload => [payload.assetId, `data:${document.ui.assets.find(a => a.id === payload.assetId)?.mime};base64,${payload.base64}`])));
  const lastPublished = $derived(publications[draftId]?.at(-1));

  function stop() { controller?.disarm('Designer changed'); preview = false; }
  async function togglePreview() { if (preview) stop(); else { preview = true; await tick(); controller?.arm(); } }
  function select(ids: string[]) { selected = ids; const next = document.presentation.widgets.find(w => w.id === ids[0]); configText = next ? JSON.stringify(next.config, null, 2) : ''; }
  function command(command: Parameters<typeof applyDesignerCommand>[1]) {
    try { stop(); history = applyDesignerCommand(history, command); if (!document.ui.pages.some(p => p.id === pageId)) pageId = document.ui.pages[0].id; error = ''; }
    catch (e) { error = String(e); }
  }
  function patch(patch: Partial<DesignerWidget>) { if (widget) command({ type: 'update-widget', id: widget.id, patch }); }
  function changeBounds(key: keyof Bounds, value: number) {
    if (widget) command({ type: 'move-widget', id: widget.id, bounds: { ...widget.bounds, [key]: value } });
  }
  function create() {
    stop(); name = `Panel ${library.drafts.length + 1}`; draftId = crypto.randomUUID();
    history = createDesignerHistory(newDocument(name)); pageId = document.ui.pages[0].id; selected = []; assets = []; message = 'New unsaved draft';
  }
  function template(source: DesignerDocument) {
    const seed = { schemaVersion: 1 as const, drafts: [{ id: 'template', name: source.ui.title, document: source, assets: [] }] };
    const entry = duplicateDraft(seed, 'template', source.ui.title).drafts[1];
    stop(); draftId = entry.id; name = entry.name; history = createDesignerHistory(entry.document); assets = entry.assets;
    pageId = document.ui.pages[0].id; select([]); message = 'Template copied into a new unsaved panel';
  }
  function load(id: string) {
    try {
      const entry = openDraft(library, id); stop(); draftId = entry.id; name = entry.name;
      history = createDesignerHistory(entry.document); assets = entry.assets; pageId = document.ui.pages[0].id; select([]); error = ''; message = 'Saved draft reopened';
    } catch (e) { error = String(e); }
  }
  async function operation(action: () => Promise<void>) {
    if (busy) return; stop(); busy = true; error = ''; try { await action(); } catch (e) { error = String(e); } finally { busy = false; }
  }
  async function persist(next = library, revisions = publications) {
    if (!store || !storageReady) throw new Error('Storage has not loaded; existing drafts have been preserved.');
    const saved = await store.save({ schemaVersion: 1, library: next, publications: revisions }); library = saved.library; publications = saved.publications;
  }
  async function save() {
    await operation(async () => {
      const next = await saveDraft(library, { id: draftId, name: document.ui.title, document, assets: activeAssets });
      await persist(next); message = 'Draft saved locally';
    });
  }
  async function duplicate() {
    await operation(async () => { const next = duplicateDraft(library, draftId); await persist(next); load(next.drafts.at(-1)!.id); message = 'Duplicate created with new IDs'; });
  }
  async function removeDraft(id: string) {
    await operation(async () => { const nextPublications = { ...publications }; delete nextPublications[id]; await persist(deleteDraft(library, id), nextPublications); if (id === draftId) create(); message = 'Draft removed'; });
  }
  async function importFile(file: File | undefined) {
    if (!file) return;
    await operation(async () => {
      if (file.size > 13 * 1024 * 1024) throw new Error('Bundle exceeds 13 MiB');
      const result = await importBundle(await file.text()); stop();
      history = createDesignerHistory(result.document); assets = result.assets; draftId = crypto.randomUUID(); name = result.document.ui.title;
      pageId = document.ui.pages[0].id; select([]); message = 'Validated bundle imported as an unsaved draft';
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
  async function publish() {
    await operation(async () => {
      stop(); const revision = await publishDocument(document, activeAssets);
      const change = lastPublished ? compareDesignerPublications(lastPublished.document, revision.document) : 'capability';
      const next = await saveDraft(library, { id: draftId, name: document.ui.title, document, assets: activeAssets });
      await persist(next, { ...publications, [draftId]: [...(publications[draftId] ?? []), revision].slice(-20) });
      message = `Immutable revision published: ${change}. Capability changes require a new session generation and target reconciliation before arming.`;
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
      if (library.drafts[0]) load(library.drafts[0].id);
    }).catch(e => { if (alive) error = `Cannot open local library: ${String(e)}`; });
    const cancel = () => stop();
    window.addEventListener('blur', cancel); window.addEventListener('pagehide', cancel);
    return () => { alive = false; stop(); unsubscribe?.(); window.removeEventListener('blur', cancel); window.removeEventListener('pagehide', cancel); };
  });
</script>

<section class="designer" aria-label="Companion panel designer">
  <header><div><h2>Companion panels</h2><p>Design portable panels for phones, tablets and another PC.</p></div><span class="badge">Local designer · output disabled</span></header>
  {#if error}<p role="alert" class="error">{error}</p>{/if}
  {#if message}<p role="status" class="message">{message}</p>{/if}
  <CompanionConnections publications={Object.values(publications).flat()} />
  <div inert={busy}>
  <div class="toolbar">
    <button onclick={create}>New panel</button>
    <button onclick={() => template(flightTemplate)}>Flight template</button><button onclick={() => template(racingTemplate)}>Racing template</button>
    <label class="file">Import bundle<input type="file" accept=".json,.osg-panel" onchange={e => void importFile(e.currentTarget.files?.[0])} disabled={busy} /></label>
    <button onclick={download} disabled={busy}>Export bundle</button>
    <button onclick={save} disabled={busy || !storageReady}>Save draft</button>
    <button onclick={publish} disabled={busy || !storageReady}>Publish revision</button>
    <button onclick={togglePreview}>{preview ? 'Stop preview' : 'Preview locally'}</button>
  </div>
  <div class="workspace">
    <aside aria-label="Panel library">
      <h3>Library</h3>
      {#each library.drafts as entry (entry.id)}
        <div class="library-row"><button class:active={draftId === entry.id} onclick={() => load(entry.id)}>{entry.name}</button><button aria-label={`Delete ${entry.name}`} onclick={() => removeDraft(entry.id)} disabled={busy}>×</button></div>
      {/each}
      <button onclick={duplicate} disabled={busy || !library.drafts.some(d => d.id === draftId)}>Duplicate saved panel</button>
      <h3>Widgets</h3>
      <label>New axis range<select bind:value={axisMode}><option value="bipolar">Bipolar −1 to 1</option><option value="unipolar">Unipolar 0 to 1</option></select></label>
      <div class="palette">{#each widgetPresets as preset}<button disabled={preview} onclick={() => { command({ type: 'add-widget', kind: preset.kind, pageId: page.id, axisMode }); select([document.presentation.widgets.at(-1)!.id]); }}>{preset.title}</button>{/each}</div>
      <h3>Assets</h3><p class="hint">PNG, JPEG or WebP · 2 MiB each · 8 MiB total</p>
      <input aria-label="Add panel image" type="file" accept="image/png,image/jpeg,image/webp" disabled={busy} onchange={e => void addAsset(e.currentTarget.files?.[0])} />
      <label>Page background<select value={viewport.backgroundAssetId ?? ''} onchange={e => command({ type: 'set-viewport', ...viewport, backgroundAssetId: e.currentTarget.value || undefined })}><option value="">None</option>{#each document.ui.assets as a}<option value={a.id}>{a.id}</option>{/each}</select></label>
    </aside>
    <main>
      <div class="toolbar"><label>Panel name<input bind:value={name} maxlength="120" onchange={() => command({ type: 'replace', document: { ...document, ui: { ...document.ui, title: name } } })} /></label>
        <button disabled={!history.past.length} onclick={() => { stop(); history = undoDesigner(history); name = document.ui.title; select([]); }}>Undo</button><button disabled={!history.future.length} onclick={() => { stop(); history = redoDesigner(history); name = document.ui.title; select([]); }}>Redo</button></div>
      <div class="toolbar"><label>Page<select value={page.id} onchange={e => { stop(); pageId = e.currentTarget.value; select([]); }}>{#each document.ui.pages as p}<option value={p.id}>{p.title}</option>{/each}</select></label>
        <button onclick={() => { command({ type: 'add-page', title: `Page ${document.ui.pages.length + 1}` }); pageId = document.ui.pages.at(-1)!.id; select([]); }}>Add page</button>
        <button disabled={document.ui.pages.length <= 1} onclick={() => { command({ type: 'delete-page', id: page.id }); select([]); }}>Remove page</button>
        <label>Viewport<select value={`${viewport.width}x${viewport.height}`} onchange={e => { const [w,h] = e.currentTarget.value.split('x').map(Number); setViewport(w,h); }}><option value="390x844">Phone portrait</option><option value="844x390">Phone landscape</option><option value="768x1024">Tablet portrait</option><option value="1024x768">Tablet landscape</option><option value="1280x720">Desktop panel</option></select></label>
        <button onclick={() => setViewport(viewport.height, viewport.width)}>Rotate</button>
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
      {#if lastPublished}<p class="hint">Published hash: <code>{lastPublished.publicationHash}</code></p>{/if}
    </main>
    <aside aria-label="Widget properties"><h3>Properties</h3>
      {#if widget}
        <p class="hint">{widget.kind} · <code>{widget.id}</code></p>
        <label>Label<input value={widget.label} onchange={e => patch({ label: e.currentTarget.value })} /></label>
        <div class="bounds">{#each ['x','y','width','height'] as field}<label>{field}<input type="number" min="0" max="1" step="0.01" value={widget.bounds[field as keyof Bounds]} onchange={e => changeBounds(field as keyof Bounds, e.currentTarget.valueAsNumber)} /></label>{/each}</div>
        {#each ['foreground','background','accent'] as color}<label>{color}<input type="color" value={widget.style?.[color as keyof NonNullable<DesignerWidget['style']>] ?? (color === 'background' ? '#12232e' : '#55ddc7')} onchange={e => patch({ style: { ...widget.style, [color]: e.currentTarget.value } })} /></label>{/each}
        {#if widget.kind === 'button'}
          <label>Button behavior<select value={widget.config.buttonMode} onchange={e => configPatch('buttonMode', e.currentTarget.value)}>{#each ['momentary','toggle','pulse','repeat'] as mode}<option value={mode}>{mode}</option>{/each}</select></label>
        {/if}
        {#each Object.entries(widget.config) as [key,value]}
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
      <details><summary>Logical target layouts</summary><p class="hint">Portable descriptors and mappings. Machine slots and credentials are stored separately.</p><button onclick={() => { targetText = JSON.stringify(document.targets, null, 2); }}>Edit layouts</button><textarea aria-label="Logical target layouts JSON" rows="8" bind:value={targetText}></textarea><button onclick={() => { try { command({ type: 'replace', document: { ...document, targets: JSON.parse(targetText) } }); } catch (e) { error = String(e); } }}>Apply layouts</button></details>
    </aside>
  </div>
  </div>
</section>

<style>
  .designer{color:var(--text,#dce7ed);padding:22px;min-width:0}header{display:flex;justify-content:space-between;gap:16px;align-items:center}h2{font-size:22px;margin:0}h3{font-size:13px;margin:18px 0 10px}p{font-size:13px;color:var(--muted,#9aafbf)}.badge{font-size:11px;color:#70dbc8;white-space:nowrap}.toolbar{display:flex;gap:8px;flex-wrap:wrap;align-items:end;margin:12px 0}.workspace{display:grid;grid-template-columns:180px minmax(320px,1fr) 250px;gap:18px}aside{min-width:0}main{min-width:0}.palette{display:grid;grid-template-columns:1fr 1fr;gap:5px}button,.file,select,input,textarea{font:inherit;font-size:12px;border:1px solid #344452;border-radius:5px;background:#14212c;color:#e1eaf0;padding:8px}button,.file{cursor:pointer}button:hover{border-color:#64caba}button:disabled{opacity:.4;cursor:default}button.active{border-color:#64caba}label{display:flex;flex-direction:column;gap:5px;font-size:12px;margin:7px 0}input,select,textarea{max-width:100%;box-sizing:border-box}input[type=checkbox]{width:auto}.file{display:inline-block}.file input{display:none}.library-row{display:flex;gap:4px;margin:5px 0}.library-row button:first-child{flex:1;text-align:left}.bounds{display:grid;grid-template-columns:1fr 1fr;gap:6px}.hint{font-size:11px;line-height:1.5;overflow-wrap:anywhere}.error{background:#50252a;color:#ffb7bb;padding:10px}.message{background:#123b36;color:#93e3d6;padding:10px}code{font-size:10px;overflow-wrap:anywhere}details{margin-top:20px}textarea{width:100%;font-family:monospace}@media(max-width:1050px){.workspace{grid-template-columns:150px minmax(280px,1fr)}.workspace>aside:last-child{grid-column:1/-1}}@media(max-width:650px){.workspace{display:block}header{display:block}}
</style>
