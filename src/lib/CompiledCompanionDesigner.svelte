<script module lang="ts">
  export type CompanionNavigation = { kind: 'main' } | { kind: 'companion'; id: string } | { kind: 'panel'; id: string };
</script>
<script lang="ts">
  import { onMount, tick, untrack } from 'svelte';
  import Icon from './Icon.svelte';
  import Notification from './Notification.svelte';
  import CompanionConnections from './CompanionConnections.svelte';
  import GenericWidgetDesigner from './GenericWidgetDesigner.svelte';
  import SourcePresentation from './SourcePresentation.svelte';
  import CompiledPanel from '../../packages/companion-ui/src/CompiledPanel.svelte';
  import { gatewayRequest } from './gateway';
  import { synchronizePanel, removePanelAssignments } from './panel-actions';
  import { restoreCompanionPanels } from './companion-startup';
  import { notify } from './notifications';
  import { type CompanionCommand, type CompanionHostState } from './companions';
  import { compiledPanelResources, canonicalWidget, type CompiledPanelPublication, type PanelBounds, type CompiledWidgetBundle, type Scalar } from '../../packages/widget-runtime/src/index.ts';
  import { validateWidgetSource, type WidgetSource } from '../../packages/widget-compiler/src/index.ts';
  import { CompiledWorkspaceStore, emptyCompiledWorkspace, newCompiledDraft, blankWidgetSource, latestWidgetSources, mergeWidgetSources, validateCompiledDraft, compilePanelDraft, designerId, type CompiledDraft, type WidgetSourceBundle } from './compiled-companion-library';
  let { openSettings, initialCompanionId = '', onNavigate, active = true }: { openSettings: () => void; initialCompanionId?: string; onNavigate?: (navigation: CompanionNavigation) => void; active?: boolean } = $props();
  let workspace = $state.raw(emptyCompiledWorkspace()), ready = $state(false), error = $state(''), message = $state('');
  let draft = $state.raw<CompiledDraft>(), past = $state.raw<CompiledDraft[]>([]), future = $state.raw<CompiledDraft[]>([]), selected = $state(''), pageId = $state('');
  let dirty = $state(false), busy = $state(false), saving = $state(false), progress = $state(''), compiled = $state.raw<CompiledPanelPublication>(), trying = $state(false);
  let motion = $state(true), audio = $state(true), selectedCompanions = $state<string[]>([]), output = $state.raw<Record<string, boolean | number>>({});
  let details = $state(untrack(() => !!initialCompanionId)), host = $state.raw<CompanionHostState>({});
  let widgetEditor = $state.raw<WidgetSource>(), editingInstance = $state('');
  let connections = $state<CompanionConnections>(), player = $state<CompiledPanel>(), stage = $state<HTMLDivElement>();
  let widgetImport = $state<HTMLInputElement>(), panelImport = $state<HTMLInputElement>();
  let disposed = false, epoch = 0, polling = false;
  let build = $state<AbortController>();
  const store = new CompiledWorkspaceStore();
  const sources = $derived(latestWidgetSources(workspace.sources));
  const publications = $derived(workspace.drafts.flatMap(d => workspace.publications[d.id]?.slice(-1) ?? []));
  const page = $derived(draft?.pages.find(page => page.id === pageId) ?? draft?.pages[0]);
  const widget = $derived(draft?.widgets.find(widget => widget.id === selected));
  const source = $derived(widget ? workspace.sources.find(source => source.id === widget.sourceId && source.revision === widget.revision) : undefined);
  const resources = $derived(compiled ? compiledPanelResources(compiled.document) : undefined);
  function sourceFor(item: CompiledDraft['widgets'][number]) { return workspace.sources.find(source => source.id === item.sourceId && source.revision === item.revision)!; }
  function invalidate() { epoch++; build?.abort(); build = undefined; busy = false; compiled = undefined; trying = false; output = {}; progress = ''; player?.reset(); }
  function change(apply: (next: CompiledDraft) => void) {
    if (!draft || saving) return;
    const next = structuredClone(draft); apply(next);
    try { validateCompiledDraft(next, workspace.sources); past = [...past.slice(-49), structuredClone(draft)]; future = []; draft = next; dirty = true; error = ''; invalidate(); }
    catch (e) { error = String(e); }
  }
  function undo() { if (!draft || saving || !past.length) return; future = [draft, ...future]; draft = past.at(-1)!; past = past.slice(0, -1); dirty = true; invalidate(); }
  function redo() { if (!draft || saving || !future.length) return; past = [...past, draft]; draft = future[0]; future = future.slice(1); dirty = true; invalidate(); }
  function leave(): boolean { return !busy && (!dirty || window.confirm('Discard unsaved panel changes?')); }
  export function getNavigation(): CompanionNavigation { return draft ? { kind: 'panel', id: draft.id } : connections?.getCompanionId() ? { kind: 'companion', id: connections.getCompanionId() } : { kind: 'main' }; }
  export function showMain(): boolean { if (!leave() || connections && !connections.showMain(false)) return false; draft = undefined; dirty = false; details = false; invalidate(); onNavigate?.({ kind: 'main' }); return true; }
  export function requestNavigation(proceed: () => void, cancel?: () => void) { if (!leave()) { cancel?.(); return; } const done = () => { trying = false; player?.reset(); proceed(); }; connections ? connections.requestNavigation(done, cancel) : done(); }
  export async function restoreNavigation(target: CompanionNavigation): Promise<void> {
    await loaded; await tick(); if (disposed) return;
    if (target.kind === 'panel') { if (draft?.id !== target.id) openPanel(target.id, false); }
    else { draft = undefined; dirty = false; invalidate(); if (target.kind === 'companion') { details = true; connections?.showCompanion(target.id); } else { details = false; connections?.showMain(false); } }
  }
  let resolveLoaded: () => void = () => {};
  const loaded = new Promise<void>(resolve => { resolveLoaded = resolve; });
  async function send(command: CompanionCommand, companion: Record<string, unknown> = {}) { const result = (await gatewayRequest({ command, companion })).companions; if (!result) throw new Error('This operation requires the native Cockpit.'); host = { ...host, ...result }; return result; }
  async function refresh() { if (disposed || !active || polling) return; polling = true; try { await send('companion_status'); } catch { /* Offline source editing remains available. */ } finally { polling = false; } }
  onMount(() => {
    void (async () => { try {
      workspace = await store.load();
      const library = (await gatewayRequest({ command: 'widget_library_get' })).widgetLibrary;
      if (library?.bundleVersion === 2) workspace = await store.save({ ...workspace, sources: mergeWidgetSources(workspace.sources, library.library.definitions) });
      ready = true;
    } catch (e) { error = `Cannot load designer: ${String(e)}`; } finally { resolveLoaded(); } })();
    void refresh(); const timer = setInterval(refresh, 2000);
    return () => { disposed = true; clearInterval(timer); invalidate(); resolveLoaded(); };
  });
  $effect(() => { if (!active) untrack(() => { trying = false; player?.reset(); }); });
  function openPanel(id: string, record = true) {
    if (!leave()) return;
    const saved = workspace.drafts.find(draft => draft.id === id); if (!saved) { error = 'This panel is no longer available.'; return; }
    invalidate(); draft = structuredClone(saved); past = []; future = []; pageId = draft.pages[0].id; selected = ''; dirty = false;
    compiled = workspace.publications[id]?.at(-1);
    const hashes = new Set((workspace.publications[id] ?? []).map(p => p.publicationHash));
    selectedCompanions = (host.host?.instances ?? []).filter(i => hashes.has(String(i.publicationHash))).map(i => String(i.companionId));
    details = false; error = ''; message = ''; if (record) onNavigate?.({ kind: 'panel', id });
  }
  function createPanel() { if (!leave()) return; invalidate(); draft = newCompiledDraft(); past = []; future = []; pageId = draft.pages[0].id; selected = ''; dirty = true; selectedCompanions = []; onNavigate?.({ kind: 'panel', id: draft.id }); }
  function add(item: WidgetSource) {
    if (!draft || !page) return;
    const width = Math.min(item.size.width, page.width), height = Math.min(item.size.height, page.height);
    const id = designerId('instance'); change(next => next.widgets.push({ id, pageId: page!.id, label: item.title, sourceId: item.id, revision: item.revision, parameters: {}, bounds: { x: 0, y: 0, width: width / page!.width, height: height / page!.height } })); selected = id;
  }
  function bounds(patch: Partial<PanelBounds>) { if (widget) change(next => Object.assign(next.widgets.find(w => w.id === selected)!.bounds, patch)); }
  let drag: { id: number; startX: number; startY: number; bounds: PanelBounds; history: CompiledDraft; past: CompiledDraft[]; resize: boolean } | undefined;
  function startDrag(event: PointerEvent, id: string, resize = false) { if (event.button !== 0 || trying || saving || !draft) return; selected = id; const item = draft.widgets.find(w => w.id === id)!; event.preventDefault(); event.stopPropagation(); drag = { id: event.pointerId, startX: event.clientX, startY: event.clientY, bounds: { ...item.bounds }, history: structuredClone(draft), past, resize }; (event.currentTarget as Element).setPointerCapture(event.pointerId); }
  function moveDrag(event: PointerEvent) {
    if (!drag || saving || event.pointerId !== drag.id || !stage || !page) return;
    const rect = stage.getBoundingClientRect(), dx = (event.clientX - drag.startX) / rect.width, dy = (event.clientY - drag.startY) / rect.height;
    const b = drag.bounds, snap = (n: number, size: number) => Math.round(n * size) / size;
    const patch = drag.resize ? { width: Math.max(1 / page.width, Math.min(1 - b.x, snap(b.width + dx, page.width))), height: Math.max(1 / page.height, Math.min(1 - b.y, snap(b.height + dy, page.height))) } : { x: Math.max(0, Math.min(1 - b.width, snap(b.x + dx, page.width))), y: Math.max(0, Math.min(1 - b.height, snap(b.y + dy, page.height))) };
    draft = structuredClone(drag.history); past = drag.past; change(next => Object.assign(next.widgets.find(w => w.id === selected)!.bounds, patch));
  }
  function endDrag() { drag = undefined; }
  function keyboard(event: KeyboardEvent) {
    if (!active || !draft || saving || widgetEditor || event.target instanceof HTMLInputElement || event.target instanceof HTMLTextAreaElement || event.target instanceof HTMLSelectElement) return;
    if ((event.ctrlKey || event.metaKey) && event.key.toLowerCase() === 'z') { event.preventDefault(); event.shiftKey ? redo() : undo(); }
    else if (event.key === 'Delete' && selected) { event.preventDefault(); removeSelected(); }
    else if (widget && ['ArrowLeft', 'ArrowRight', 'ArrowUp', 'ArrowDown'].includes(event.key)) { event.preventDefault(); const delta = event.shiftKey ? 10 : 1; bounds({ x: Math.max(0, Math.min(1 - widget.bounds.width, widget.bounds.x + (event.key === 'ArrowLeft' ? -delta : event.key === 'ArrowRight' ? delta : 0) / page!.width)), y: Math.max(0, Math.min(1 - widget.bounds.height, widget.bounds.y + (event.key === 'ArrowUp' ? -delta : event.key === 'ArrowDown' ? delta : 0) / page!.height)) }); }
  }
  function removeSelected() { change(next => { next.widgets = next.widgets.filter(w => w.id !== selected); }); selected = ''; }
  function parameter(id: string, value: Scalar) { change(next => { next.widgets.find(w => w.id === selected)!.parameters[id] = value; }); }
  function editWidget(item?: WidgetSource, instance = '') { editingInstance = instance; widgetEditor = item ? { ...structuredClone(item), revision: Math.max(...workspace.sources.filter(s => s.id === item.id).map(s => s.revision)) + 1 } : blankWidgetSource(); }
  async function saveWidget(next: WidgetSource, _bundle: CompiledWidgetBundle) {
    const sources = mergeWidgetSources(workspace.sources, [next]);
    const native = await gatewayRequest({ command: 'widget_library_merge', bundle: { bundleVersion: 2, library: { formatVersion: 2, definitions: [next] } } });
    if (native.widgetLibrary?.bundleVersion !== 2) throw new Error('Restart Cockpit to use the generic widget source library.');
    workspace = await store.save({ ...workspace, sources });
    if (editingInstance && draft) change(draft => { const widget = draft.widgets.find(widget => widget.id === editingInstance); if (widget) { widget.sourceId = next.id; widget.revision = next.revision; widget.parameters = {}; } });
    widgetEditor = undefined; editingInstance = ''; message = `${next.title} saved.`;
  }
  async function compile() {
    if (!draft) return; invalidate(); const revision = epoch, controller = new AbortController(); build = controller; busy = true; error = ''; progress = 'Compiling…';
    try { const result = await compilePanelDraft(draft, workspace.sources, { signal: controller.signal, motion, audio, onProgress: (done, total) => { if (revision === epoch) progress = `${done}/${total} widgets compiled`; } }); if (!disposed && revision === epoch) { compiled = result; progress = 'Compilation complete'; } }
    catch (e) { if (revision === epoch) error = String(e); }
    finally { if (revision === epoch) { busy = false; build = undefined; } }
  }
  async function save() {
    if (!draft || !compiled || busy) return; const captured = structuredClone(draft), publication = compiled; busy = saving = true; error = '';
    try {
      await restoreCompanionPanels();
      const drafts = [...workspace.drafts.filter(d => d.id !== captured.id), captured], previous = workspace.publications[captured.id] ?? [];
      const revisions = [...previous.filter(p => p.publicationHash !== publication.publicationHash), publication];
      workspace = await store.save({ ...workspace, drafts, publications: { ...workspace.publications, [captured.id]: revisions } });
      await synchronizePanel(publication, previous.map(p => p.publicationHash), send, selectedCompanions);
      dirty = false; message = 'Saved and assigned to the selected browser companions.';
    } catch (e) { error = String(e); } finally { busy = saving = false; }
  }
  async function removePanel(id: string) {
    if (!window.confirm('Delete this panel and its assignments?')) return;
    busy = true; try { await restoreCompanionPanels(); await removePanelAssignments((workspace.publications[id] ?? []).map(p => p.publicationHash), send); const publications = { ...workspace.publications }; delete publications[id]; workspace = await store.save({ ...workspace, drafts: workspace.drafts.filter(d => d.id !== id), publications }); if (draft?.id === id) { dirty = false; busy = false; showMain(); } } catch (e) { error = String(e); } finally { busy = false; }
  }
  function download(value: unknown, name: string) { const url = URL.createObjectURL(new Blob([JSON.stringify(value)], { type: 'application/json' })), anchor = document.createElement('a'); anchor.href = url; anchor.download = name; anchor.click(); setTimeout(() => URL.revokeObjectURL(url), 1000); }
  async function importWidgets(file?: File) { if (!file) return; try { if (file.size > 8 * 1024 * 1024) throw new Error('Widget source import exceeds 8 MiB.'); const input = JSON.parse(await file.text()); const definitions = input.bundleVersion === 2 ? input.library.definitions : [input]; const sources = mergeWidgetSources(workspace.sources, definitions); const bundle: WidgetSourceBundle = { bundleVersion: 2, library: { formatVersion: 2, definitions } }; await gatewayRequest({ command: 'widget_library_merge', bundle }); workspace = await store.save({ ...workspace, sources }); } catch (e) { error = String(e); } }
  async function importPanel(file?: File) {
    if (!file || !leave()) return;
    try {
      if (file.size > 13 * 1024 * 1024) throw new Error('Panel import exceeds 13 MiB.');
      const input = JSON.parse(await file.text());
      if (input.formatVersion !== 2) throw new Error('Choose a generic panel export.');
      const sources = mergeWidgetSources(workspace.sources, input.sources);
      const panel = validateCompiledDraft(input.draft, sources);
      if (workspace.drafts.some(d => d.id === panel.id)) panel.id = designerId('panel');
      const native = await gatewayRequest({ command: 'widget_library_merge', bundle: { bundleVersion: 2, library: { formatVersion: 2, definitions: input.sources } } });
      if (native.widgetLibrary?.bundleVersion !== 2) throw new Error('Restart Cockpit to use the generic widget source library.');
      workspace = await store.save({ ...workspace, sources, drafts: [...workspace.drafts.filter(d => d.id !== panel.id), panel] });
      openPanel(panel.id); compiled = undefined; dirty = true;
      message = 'Imported source. Compile this panel for its current browser target.';
    } catch (e) { error = String(e); }
  }
</script>

<svelte:window onkeydown={keyboard}/>
<section class="compiled-designer" aria-label="Companion panel designer">
  <div hidden={!!draft}><CompanionConnections bind:this={connections} {publications} {openSettings} {initialCompanionId} {active} onViewChange={value => details = value} onNavigate={id => onNavigate?.(id ? { kind: 'companion', id } : { kind: 'main' })}/></div>
  {#if error}<Notification message={error} kind="error"/>{/if}
  {#if message}<Notification {message}/>{/if}
  {#if !draft && !details}
    <section aria-label="Panels"><div class="section-heading"><div><h2>Panels</h2><p>Arrange compiled widgets and assign a panel to a browser companion.</p></div><div class="actions"><button disabled={!ready || busy} onclick={() => panelImport?.click()}><Icon name="upload" size={16}/>Import</button><button disabled={!ready || busy || workspace.drafts.length >= 20} onclick={createPanel}><Icon name="plus" size={16}/>New</button></div></div>
      <div class="cards">{#each workspace.drafts as panel (panel.id)}<article><button class="card" aria-label={`Edit ${panel.title}`} onclick={() => openPanel(panel.id)}><Icon name="grid" size={24}/><strong>{panel.title}</strong><small>{panel.pages.length} pages · {panel.widgets.length} widgets</small></button><button aria-label={`Delete ${panel.title}`} onclick={() => removePanel(panel.id)}><Icon name="trash" size={16}/></button></article>{:else}<p>No panels yet.</p>{/each}</div>
    </section>
    <section aria-label="Widgets"><div class="section-heading"><div><h2>Widgets</h2><p>Design visuals, signals, states, motion and sound.</p></div><div class="actions"><button disabled={!ready} onclick={() => widgetImport?.click()}><Icon name="upload" size={16}/>Import</button><button disabled={!ready} onclick={() => download({ bundleVersion: 2, library: { formatVersion: 2, definitions: workspace.sources } }, 'widgets.json')}><Icon name="download" size={16}/>Export</button><button disabled={!ready} onclick={() => editWidget()}><Icon name="plus" size={16}/>New</button></div></div>
      <div class="cards">{#each sources as item (item.id)}<article><button class="card" aria-label={`Edit design ${item.title}`} onclick={() => editWidget(item)}><div class="thumbnail"><SourcePresentation source={item} fit="contain"/></div><strong>{item.title}</strong><small>Revision {item.revision} · {item.size.width} × {item.size.height}</small><Icon name="pencil" size={16}/></button></article>{/each}</div>
    </section>
  {:else if draft && page}
    <div class="section-heading"><div><button onclick={showMain} disabled={busy}><Icon name="back" size={16}/>Back</button><h1>{draft.title}</h1><p>{dirty ? 'Unsaved changes' : 'Saved'} · Browser target</p></div><div class="actions"><button aria-label="Undo" disabled={!past.length || busy} onclick={undo}><Icon name="undo" size={16}/></button><button aria-label="Redo" disabled={!future.length || busy} onclick={redo}><Icon name="redo" size={16}/></button><button disabled={busy && !build} onclick={busy ? invalidate : compile}><Icon name={busy ? 'close' : 'refresh'} size={16}/>{busy ? 'Cancel' : 'Compile'}</button><button disabled={!compiled || busy} onclick={() => { trying = !trying; output = {}; player?.reset(); }}><Icon name={trying ? 'stop' : 'play'} size={16}/>{trying ? 'Stop' : 'Try'}</button><button disabled={!compiled || busy} onclick={() => download({ formatVersion: 2, draft, sources: workspace.sources.filter(s => draft!.widgets.some(w => w.sourceId === s.id && w.revision === s.revision)), publication: compiled }, `${draft!.id}.osg-panel`)}><Icon name="download" size={16}/>Export</button><button disabled={!compiled || busy} onclick={save}><Icon name="save" size={16}/>Save</button></div></div>
    <div class="workspace" inert={saving}>
      <aside aria-label="Panel configuration"><h3>Panel</h3><label>Name<input aria-label="Panel name" value={draft.title} onchange={e => change(d => d.title = e.currentTarget.value)}/></label><label>Page<select value={page.id} onchange={e => { pageId = e.currentTarget.value; trying = false; player?.reset(); }}>{#each draft.pages as p}<option value={p.id}>{p.title}</option>{/each}</select></label><div class="actions"><button onclick={() => change(d => d.pages.push({ id: designerId('page'), title: `Page ${d.pages.length + 1}`, width: page!.width, height: page!.height }))} disabled={draft.pages.length >= 16}><Icon name="plus" size={16}/>Add</button><button disabled={draft.pages.length < 2} onclick={() => { const id = page!.id; change(d => { d.pages = d.pages.filter(p => p.id !== id); d.widgets = d.widgets.filter(w => w.pageId !== id); }); pageId = draft!.pages[0].id; }}><Icon name="trash" size={16}/>Remove</button></div>
        <label>Width<input type="number" min="320" max="8192" step="1" value={page.width} onchange={e => change(d => d.pages.find(p => p.id === page!.id)!.width = e.currentTarget.valueAsNumber)}/></label><label>Height<input type="number" min="320" max="8192" step="1" value={page.height} onchange={e => change(d => d.pages.find(p => p.id === page!.id)!.height = e.currentTarget.valueAsNumber)}/></label>
        <h3>Widgets</h3>{#each sources as item}<button class="palette" onclick={() => add(item)} disabled={trying || busy}><Icon name="plus" size={16}/>{item.title}</button>{/each}<button onclick={() => editWidget()}><Icon name="pencil" size={16}/>New</button>
        <h3>Target</h3><label class="check"><input type="checkbox" checked={motion} onchange={e => { motion = e.currentTarget.checked; invalidate(); }}/>Animations</label><label class="check"><input type="checkbox" checked={audio} onchange={e => { audio = e.currentTarget.checked; invalidate(); }}/>Audio</label><small>Static visuals and live feedback remain available with media disabled.</small>
        <h3>Assignments</h3>{#each host.host?.clients ?? [] as client}<label class="check"><input type="checkbox" value={String(client.companionId)} disabled={client.kind !== 'browser'} checked={selectedCompanions.includes(String(client.companionId))} onchange={e => { selectedCompanions = e.currentTarget.checked ? [...selectedCompanions, String(client.companionId)] : selectedCompanions.filter(id => id !== client.companionId); dirty = true; }}/>{String(client.name ?? client.companionId)}{client.kind !== 'browser' ? ' · unsupported target' : ''}</label>{:else}<small>Pair a browser companion to assign this panel.</small>{/each}
      </aside>
      <main><div class="stage-heading"><strong>{trying ? 'Compiled Try' : 'Source preview'}</strong><span>{page.width} × {page.height}</span></div><div class="stage" bind:this={stage} style:aspect-ratio={`${page.width}/${page.height}`}>
        {#if trying && compiled}<CompiledPanel bind:this={player} publication={compiled} pageId={page.id} onoutput={value => output = { ...output, [value.controlId]: value.value }} onerror={value => error = value}/>
        {:else}{#each draft.widgets.filter(w => w.pageId === page!.id) as item (item.id)}<div class="placement" class:selected={item.id === selected} style:left={`${item.bounds.x * 100}%`} style:top={`${item.bounds.y * 100}%`} style:width={`${item.bounds.width * 100}%`} style:height={`${item.bounds.height * 100}%`}><button class="widget-drag" aria-label={`Select ${item.label}`} onpointerdown={e => startDrag(e, item.id)} onpointermove={moveDrag} onpointerup={endDrag} onpointercancel={endDrag} onclick={() => selected = item.id}><SourcePresentation source={sourceFor(item)} parameters={item.parameters}/></button>{#if selected === item.id}<button class="resize" aria-label={`Resize ${item.label}`} onpointerdown={e => startDrag(e, item.id, true)} onpointermove={moveDrag} onpointerup={endDrag} onpointercancel={endDrag}><Icon name="fit" size={14}/></button>{/if}</div>{/each}{/if}
      </div><p class="hint">{trying ? 'Inputs stay inside this simulator.' : 'Drag to move. Resize from the corner or enter exact pixel dimensions. Arrow keys move the selected widget.'}</p>{#if progress}<p role="status">{progress}</p>{/if}{#if resources}<div class="resources" aria-label="Compiled resources"><strong>Compiled</strong><span>{Math.ceil(resources.assetBytes / 1024)} KiB assets</span><span>{Math.ceil(resources.manifestBytes / 1024)} KiB manifest</span><span>{Math.ceil(resources.activeCandidateBytes / 1024)} KiB active + candidate</span>{#each compiled!.document.presentation.artifacts.flatMap(a => a.artifact.fallbacks) as fallback}<span>{fallback.feature}: {fallback.reason}</span>{/each}</div>{/if}{#if trying}<pre aria-label="Requested outputs">{JSON.stringify(output, null, 2)}</pre>{/if}</main>
      <aside aria-label="Widget properties">{#if widget && source}<h3>{widget.label}</h3><label>Label<input value={widget.label} onchange={e => change(d => d.widgets.find(w => w.id === selected)!.label = e.currentTarget.value)}/></label>{#each ['x', 'y', 'width', 'height'] as field}<label>{field}<input aria-label={`Widget ${field}`} type="number" step="1" min={field === 'width' || field === 'height' ? 1 : 0} value={Math.round(widget.bounds[field as keyof PanelBounds] * (field === 'x' || field === 'width' ? page.width : page.height))} onchange={e => bounds({ [field]: e.currentTarget.valueAsNumber / (field === 'x' || field === 'width' ? page!.width : page!.height) })}/></label>{/each}
        {#each source.parameters as p}<label>{p.label}{#if p.type === 'boolean'}<input type="checkbox" checked={Boolean(widget.parameters[p.id] ?? p.default)} onchange={e => parameter(p.id, e.currentTarget.checked)}/>{:else}<input type={p.type === 'number' ? 'number' : p.type === 'color' ? 'color' : 'text'} min={p.type === 'number' ? p.min : undefined} max={p.type === 'number' ? p.max : undefined} value={String(widget.parameters[p.id] ?? p.default)} onchange={e => parameter(p.id, p.type === 'number' ? e.currentTarget.valueAsNumber : e.currentTarget.value)}/>{/if}</label>{/each}
        <button onclick={() => editWidget(source, widget!.id)}><Icon name="pencil" size={16}/>Edit design</button><button onclick={removeSelected}><Icon name="trash" size={16}/>Remove</button><small>Revision {source.revision}. Resizing or editing invalidates the compiled result.</small>
      {:else}<h3>Selection</h3><p>Select a widget to edit its placement and parameters.</p>{/if}</aside>
    </div>
  {/if}
  <input hidden type="file" bind:this={widgetImport} aria-label="Import widget sources" accept="application/json,.json" onchange={e => { void importWidgets(e.currentTarget.files?.[0]); e.currentTarget.value = ''; }}/>
  <input hidden type="file" bind:this={panelImport} aria-label="Import compiled panel" accept="application/json,.json,.osg-panel" onchange={e => { void importPanel(e.currentTarget.files?.[0]); e.currentTarget.value = ''; }}/>
  {#if widgetEditor}{#key `${widgetEditor.id}:${widgetEditor.revision}`}<GenericWidgetDesigner source={widgetEditor} onsave={saveWidget} oncancel={() => { widgetEditor = undefined; editingInstance = ''; }}/>{/key}{/if}
</section>
<style>
  .compiled-designer{display:grid;gap:24px}.section-heading{display:flex;justify-content:space-between;align-items:center;gap:16px;flex-wrap:wrap}.section-heading h1,.section-heading h2{margin:8px 0}.section-heading p,.hint,small{color:var(--muted,#91a5b0);font-size:13px}.actions{display:flex;gap:8px;flex-wrap:wrap}button{display:inline-flex;align-items:center;justify-content:center;gap:8px;padding:8px 12px;border:1px solid var(--border,#30434e);border-radius:6px;background:var(--surface,#17252d);color:inherit;cursor:pointer}button:disabled{opacity:.45;cursor:default}.cards{display:grid;grid-template-columns:repeat(auto-fill,minmax(220px,1fr));gap:12px}.cards article{position:relative;display:flex;gap:4px;border:1px solid var(--border,#30434e);border-radius:8px;padding:8px}.card{flex:1;display:flex;flex-direction:column;border:0;background:transparent;min-width:0;align-items:start}.thumbnail{width:100%;height:100px;background:#0d1820}.workspace{display:grid;grid-template-columns:220px minmax(300px,1fr) 220px;gap:16px;align-items:start}.workspace aside{padding:16px;background:var(--surface,#17252d);border:1px solid var(--border,#30434e);border-radius:8px;display:grid;gap:12px}.workspace aside h3{margin:6px 0}label{display:grid;gap:5px;font-size:13px}.check{display:flex;align-items:center;gap:8px}.check input{width:auto}input,select{box-sizing:border-box;width:100%;min-width:0;background:var(--bg,#0d1820);border:1px solid var(--border,#30434e);border-radius:4px;color:inherit;padding:7px}.workspace main{min-width:0}.stage{position:relative;width:100%;background:#0d1820;overflow:hidden;border:1px solid #40515b;box-sizing:border-box}.stage-heading{display:flex;justify-content:space-between;padding:8px 0;font-size:13px}.placement{position:absolute;box-sizing:border-box}.placement.selected{outline:1px solid #93d8cf;outline-offset:1px}.widget-drag{display:block;width:100%;height:100%;padding:0;border:0;border-radius:0;background:transparent;touch-action:none}.resize{position:absolute;right:0;bottom:0;padding:2px;border-radius:0;touch-action:none;z-index:2}.palette{justify-content:start}.resources{display:flex;gap:12px;flex-wrap:wrap;font-size:12px;padding:12px;background:var(--surface,#17252d);border-radius:6px}pre{overflow:auto;font-size:12px;max-height:180px}@media(max-width:1100px){.workspace{grid-template-columns:180px minmax(260px,1fr)}.workspace>aside:last-child{grid-column:1/-1;grid-template-columns:repeat(auto-fit,minmax(130px,1fr))}}@media(max-width:700px){.workspace{display:flex;flex-direction:column}.workspace>*{width:100%;box-sizing:border-box}}
</style>
