<script module lang="ts">
  export type {CompanionPage as CompanionNavigation} from './cockpit-history';
</script>
<script lang="ts">
  import PageHeader from './PageHeader.svelte';
  import { onMount, tick, untrack } from 'svelte';
  import type {CompanionPage as CompanionNavigation} from './cockpit-history';
  import Icon from './Icon.svelte';
  import Notification from './Notification.svelte';
  import CompanionConnections from './CompanionConnections.svelte';
  import GenericWidgetDesigner from './GenericWidgetDesigner.svelte';
  import SourcePresentation from './SourcePresentation.svelte';
  import CompiledPanel from '../../packages/companion-ui/src/CompiledPanel.svelte';
  import { gatewayRequest } from './gateway';
  import { companionObservations } from './companion-observations';
  import { panelAssignmentTargets, synchronizePanel, removePanelAssignments } from './panel-actions';
  import { restoreCompanionPanels } from './companion-startup';
  import { notify } from './notifications';
  import { panelDestinationWarnings } from './companion-panel-resolution';
  import { type CompanionCommand, type CompanionHostState } from './companions';
  import { compiledPanelResources, canonicalWidget, type CompiledPanelPublication, type PanelBounds, type CompiledWidgetBundle, type Scalar } from '../../packages/widget-runtime/src/index.ts';
  import { validateWidgetSource, type WidgetSource } from '../../packages/widget-compiler/src/index.ts';
  import { CompiledWorkspaceStore, emptyCompiledWorkspace, newCompiledDraft, blankWidgetSource, mergeWidgetSources, replaceWorkspaceSources, validateCompiledDraft, compilePanelDraft, designerId, observeBrowserDesignerCommits, type CompiledDraft, type CompiledWorkspace } from './compiled-companion-library';
  import { desktopDesignerStorage } from './persistence/designer-storage';

  let { openSettings, initialCompanionId = '', onNavigate, active = true }: { openSettings: () => void; initialCompanionId?: string; onNavigate?: (navigation: CompanionNavigation) => void; active?: boolean } = $props();
  let workspace = $state.raw(emptyCompiledWorkspace()), ready = $state(false), error = $state(''), message = $state('');
  let libraryError = $state(''), shown = $state(false);
  let draft = $state.raw<CompiledDraft>(), past = $state.raw<CompiledDraft[]>([]), future = $state.raw<CompiledDraft[]>([]), selected = $state(''), pageId = $state('');
  let panelBase = $state.raw<CompiledDraft>(), widgetBase = $state.raw<WidgetSource>();
  let dirty = $state(false), busy = $state(false), saving = $state(false), progress = $state(''), compiled = $state.raw<CompiledPanelPublication>(), trying = $state(false);
  let motion = $state(true), audio = $state(true), output = $state.raw<Record<string, boolean | number>>({});
  let assignmentEdits = $state.raw<ReadonlyMap<string, boolean>>(new Map());
  let details = $state(untrack(() => !!initialCompanionId)), host = $state.raw<CompanionHostState>({});
  let widgetEditor = $state.raw<WidgetSource>(), editingInstance = $state('');
  const same = (a: unknown, b: unknown) => a === b || canonicalWidget(a ?? null) === canonicalWidget(b ?? null);
  const panelConflict = $derived(!!draft && !same(panelBase, workspace.drafts.find(item => item.id === draft?.id)));
  const widgetConflict = $derived(!!widgetEditor && !same(widgetBase, workspace.sources.find(item => item.id === widgetEditor?.id)));
  const conflict = $derived(widgetConflict ? 'This widget was changed elsewhere. Reopen the design before saving.' : panelConflict ? 'This panel was changed elsewhere. Reopen the panel before saving.' : '');
  const libraryUnavailable = $derived(libraryError || (!ready ? 'Loading the saved designer workspace.' : ''));
  const saveDisabledReason=$derived(libraryUnavailable||(panelConflict?conflict:saving?'The panel is being saved. Please wait.':busy?'Wait for compilation to finish before saving.':!compiled?(error?'Fix the error and compile the panel before saving.':'Compile the panel before saving. Layout or target changes require a new compilation.'):''));
  let widgetDesigner=$state<GenericWidgetDesigner>();
  let connections = $state<CompanionConnections>(), player = $state<CompiledPanel>(), stage = $state<HTMLDivElement>();
  let widgetImport = $state<HTMLInputElement>(), panelImport = $state<HTMLInputElement>();
  let disposed = false, epoch = 0, hostRequest = 0, appliedHostRequest = 0;
  let build = $state<AbortController>();
  const store = new CompiledWorkspaceStore();
  function saveWorkspace(next: CompiledWorkspace) {
    if (libraryUnavailable) throw new Error(libraryUnavailable);
    return store.save(next, workspace);
  }
  const panelVisible = $derived(!!draft && !widgetEditor);
  const sources = $derived(workspace.sources);
  const publications = $derived(workspace.drafts.flatMap(d => workspace.publications[d.id]?.slice(-1) ?? []));
  const page = $derived(draft?.pages.find(page => page.id === pageId) ?? draft?.pages[0]);
  const widget = $derived(draft?.widgets.find(widget => widget.id === selected));
  const source = $derived(widget ? workspace.sources.find(source => source.id === widget.sourceId) : undefined);
  const resources = $derived(compiled ? compiledPanelResources(compiled.document) : undefined);
  const selectedCompanions = $derived(draft
    ? panelAssignmentTargets(host, draft.id, (workspace.publications[draft.id] ?? []).map(panel => panel.publicationHash), assignmentEdits)
    : []);
  const resolutionWarnings = $derived(panelDestinationWarnings(page,selectedCompanions,host));
  function sourceFor(item: CompiledDraft['widgets'][number]) { return workspace.sources.find(source => source.id === item.sourceId); }
  function invalidate() { epoch++; build?.abort(); build = undefined; busy = saving; compiled = undefined; trying = false; output = {}; progress = ''; player?.reset(); }
  function change(apply: (next: CompiledDraft) => void) {
    if (!draft || saving) return;
    const next = structuredClone(draft); apply(next);
    try { validateCompiledDraft(next, workspace.sources); past = [...past.slice(-49), structuredClone(draft)]; future = []; draft = next; dirty = true; error = ''; invalidate(); }
    catch (e) { error = String(e); }
  }
  function undo() { if (!draft || saving || !past.length) return; future = [draft, ...future]; draft = past.at(-1)!; past = past.slice(0, -1); dirty = true; invalidate(); }
  function redo() { if (!draft || saving || !future.length) return; past = [...past, draft]; draft = future[0]; future = future.slice(1); dirty = true; invalidate(); }
  function leave(): boolean { return (widgetDesigner?.canLeave()??true) && !busy && (!dirty || window.confirm('Discard unsaved panel changes?')); }
  export function getNavigation(): CompanionNavigation { return widgetEditor ? {kind:'widget',id:widgetEditor.id} : draft ? { kind: 'panel', id: draft.id } : connections?.getCompanionId() ? { kind: 'companion', id: connections.getCompanionId() } : { kind: 'main' }; }
  export function showMain(): boolean { if (!leave() || connections && !connections.showMain(false)) return false; widgetEditor=undefined;editingInstance='';draft = undefined; dirty = false; details = false; invalidate(); onNavigate?.({ kind: 'main' }); return true; }
  export function requestNavigation(proceed: () => void, cancel?: () => void) { if (!leave()) { cancel?.(); return; } const done = () => { trying = false; player?.reset(); proceed(); }; connections ? connections.requestNavigation(done, cancel) : done(); }
  export async function restoreNavigation(target: CompanionNavigation): Promise<void> {
    await tick(); await libraryLoad; if (disposed || !active) return;
    if(target.kind==='widget'){if(widgetEditor?.id!==target.id){const source=workspace.sources.find(s=>s.id===target.id);if(source)editWidget(source,'',false);}return;}
    widgetEditor=undefined;editingInstance='';
    if (target.kind === 'panel') { if (draft?.id !== target.id) openPanel(target.id, false); }
    else { draft = undefined; dirty = false; invalidate(); if (target.kind === 'companion') { details = true; connections?.showCompanion(target.id); } else { details = false; connections?.showMain(false); } }
  }
  let libraryLoad = Promise.resolve();
  async function send(command: CompanionCommand, companion: Record<string, unknown> = {}, current?: () => boolean) {
    const version = ++hostRequest;
    const result = (await gatewayRequest({ command, companion }, current)).companions;
    if (!result) throw new Error('This operation requires the native Cockpit.');
    if (!disposed && version >= appliedHostRequest && (!current || current())) {
      appliedHostRequest = version;
      host = { ...host, ...result };
    }
    return result;
  }
  $effect(() => {
    if (!active || !shown || !panelVisible) return;
    let stopped = false, reading = false, pending = false;
    const current = () => !stopped && active && panelVisible && !document.hidden;
    const refresh = async () => {
      if (!current()) return;
      if (reading) { pending = true; return; }
      reading = true;
      do {
        pending = false;
        try { await send('companion_status', {}, current); } catch { /* Offline source editing remains available. */ }
      } while (pending && current());
      reading = false;
    };
    const observations = companionObservations();
    const stops = ['companion.presence', 'companion.listener'].map(topic =>
      observations.subscribe(topic, state => {
        if (state.status === 'ready') untrack(() => { void refresh(); });
      }));
    return () => { stopped = true; stops.forEach(stop => stop()); };
  });
  onMount(() => {
    const visibility = () => { shown = !document.hidden; };
    visibility();
    document.addEventListener('visibilitychange', visibility);
    return () => { document.removeEventListener('visibilitychange', visibility); disposed = true; invalidate(); };
  });
  $effect(() => {
    if (!active || !shown) return;
    let stopped = false, reading = false, pending = false;
    const refreshLibrary = () => {
      if (stopped) return;
      if (reading) { pending = true; return; }
      reading = true;
      libraryLoad = (async () => {
        do {
          pending = false;
          try {
            const current = await store.load();
            if (!stopped && !disposed) {
              if (draft?.widgets.some(widget => !same(sourceFor(widget), current.sources.find(source => source.id === widget.sourceId)))) invalidate();
              workspace = current;
              ready = true;
              libraryError = '';
            }
          } catch (e) {
            if (!stopped && !disposed) libraryError = `Cannot load designer: ${String(e)}`;
          }
        } while (pending && !stopped);
        reading = false;
      })();
    };
    ready = false;
    const stop = desktopDesignerStorage()
      ? companionObservations().subscribe('designer.workspace', state => {
          if (state.status === 'ready') refreshLibrary();
        })
      : observeBrowserDesignerCommits(refreshLibrary);
    refreshLibrary();
    return () => { stopped = true; stop(); };
  });
  $effect(() => { if (!active) untrack(() => { trying = false; player?.reset(); }); });
  function openPanel(id: string, record = true) {
    if (!ready || !leave()) return;
    const saved = workspace.drafts.find(draft => draft.id === id); if (!saved) { error = 'This panel is no longer available.'; return; }
    invalidate(); panelBase = saved; draft = structuredClone(saved); past = []; future = []; pageId = draft.pages[0].id; selected = ''; dirty = false;
    compiled = workspace.publications[id]?.at(-1);
    assignmentEdits = new Map();
    details = false; error = ''; message = ''; if (record) onNavigate?.({ kind: 'panel', id });
  }
  function createPanel() { if (!leave()) return; invalidate(); panelBase = undefined; draft = newCompiledDraft(); past = []; future = []; pageId = draft.pages[0].id; selected = ''; dirty = true; assignmentEdits = new Map(); onNavigate?.({ kind: 'panel', id: draft.id }); }
  function add(item: WidgetSource) {
    if (!draft || !page) return;
    const width = Math.min(item.size.width, page.width), height = Math.min(item.size.height, page.height);
    const id = designerId('instance'); change(next => next.widgets.push({ id, pageId: page!.id, label: item.title, sourceId: item.id, parameters: {}, bounds: { x: 0, y: 0, width: width / page!.width, height: height / page!.height } })); selected = id;
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
  function editWidget(item?: WidgetSource, instance = '', record=true) { if(!ready)return;editingInstance = instance; trying=false;player?.reset();widgetBase = item; widgetEditor = item ? structuredClone(item) : blankWidgetSource();if(record)onNavigate?.({kind:'widget',id:widgetEditor.id}); }
  function closeWidget(){widgetEditor=undefined;editingInstance='';onNavigate?.(getNavigation());}
  async function saveWidget(next: WidgetSource, _bundle: CompiledWidgetBundle) {
    await libraryLoad;
    if (libraryError) throw new Error(libraryError);
    if (widgetConflict) throw new Error(conflict);
    const updated = replaceWorkspaceSources(workspace, [next]);
    workspace = await saveWorkspace(updated);
    if(draft?.widgets.some(w=>w.sourceId===next.id)){invalidate();dirty=true;}
    if (editingInstance && draft) change(draft => { const widget = draft.widgets.find(widget => widget.id === editingInstance); if (widget) { widget.sourceId = next.id;  } });
    closeWidget(); message = `${next.title} saved.`;

  }
  async function compile() {
    if (!draft) return; invalidate(); const revision = epoch, controller = new AbortController(); build = controller; busy = true; error = ''; progress = 'Compiling…';
    try { const result = await compilePanelDraft(draft, workspace.sources, { signal: controller.signal, motion, audio, onProgress: (done, total) => { if (revision === epoch) progress = `${done}/${total} widgets compiled`; } }); if (!disposed && revision === epoch) { compiled = result; progress = 'Compilation complete'; } }
    catch (e) { if (revision === epoch) error = String(e); }
    finally { if (revision === epoch) { busy = false; build = undefined; } }
  }
  async function save() {
    if (!draft || !compiled || busy || panelConflict) return;
    busy = saving = true; error = '';
    try {
      await restoreCompanionPanels();
      await libraryLoad;
      if (libraryError) throw new Error(libraryError);
      if (panelConflict) throw new Error(conflict);
      if (!draft || !compiled) throw new Error('The panel sources changed. Compile the panel before saving.');
      const captured = structuredClone(draft), publication = compiled;
      const drafts = [...workspace.drafts.filter(d => d.id !== captured.id), captured], previous = workspace.publications[captured.id] ?? [];
      workspace = await saveWorkspace({ ...workspace, drafts, publications: { ...workspace.publications, [captured.id]: [publication] } });
      panelBase = captured;
      await synchronizePanel(publication, previous.map(p => p.publicationHash), send, assignmentEdits.size ? assignmentEdits : undefined);
      assignmentEdits = new Map();
      dirty = false; message = 'Saved and assigned to the selected browser companions.';
    } catch (e) { error = String(e); } finally { busy = saving = false; }
  }
  async function removePanel(id: string) {
    if (!ready) return;
    if (!window.confirm('Delete this panel and its assignments?')) return;
    busy = true; try { await restoreCompanionPanels(); await removePanelAssignments((workspace.publications[id] ?? []).map(p => p.publicationHash), send); const publications = { ...workspace.publications }; delete publications[id]; workspace = await saveWorkspace({ ...workspace, drafts: workspace.drafts.filter(d => d.id !== id), publications }); if (draft?.id === id) { dirty = false; busy = false; showMain(); } } catch (e) { error = String(e); } finally { busy = false; }
  }
  function download(value: unknown, name: string) { const url = URL.createObjectURL(new Blob([JSON.stringify(value)], { type: 'application/json' })), anchor = document.createElement('a'); anchor.href = url; anchor.download = name; anchor.click(); setTimeout(() => URL.revokeObjectURL(url), 1000); }
  async function importWidgets(file?: File) { if (!file) return; try { if (file.size > 8 * 1024 * 1024) throw new Error('Widget source import exceeds 8 MiB.'); const input = JSON.parse(await file.text()); const definitions = Array.isArray(input.definitions) ? input.definitions : [input]; const updated = replaceWorkspaceSources(workspace, definitions); workspace = await saveWorkspace(updated); } catch (e) { error = String(e); } }

  async function importPanel(file?: File) {
    if (!file || !leave()) return;
    try {
      if (file.size > 13 * 1024 * 1024) throw new Error('Panel import exceeds 13 MiB.');
      const input = JSON.parse(await file.text());
      if (!input.draft || !Array.isArray(input.sources)) throw new Error('Choose a panel export.');
      const updated = replaceWorkspaceSources(workspace, input.sources);
      const sources = updated.sources;
      const panel = validateCompiledDraft(input.draft, sources);
      if (workspace.drafts.some(d => d.id === panel.id)) panel.id = designerId('panel');
      workspace = await saveWorkspace({ ...updated, drafts: [...workspace.drafts.filter(d => d.id !== panel.id), panel] });

      openPanel(panel.id); compiled = undefined; dirty = true;
      message = 'Imported source. Compile this panel for its current browser target.';
    } catch (e) { error = String(e); }
  }
</script>

<svelte:window onkeydown={keyboard}/>
<section class="compiled-designer" aria-label="Companion panel designer">
  <div hidden={!!draft||!!widgetEditor}><CompanionConnections bind:this={connections} {publications} {openSettings} {initialCompanionId} active={active&&!draft&&!widgetEditor} onViewChange={value => details = value} onNavigate={id => onNavigate?.(id ? { kind: 'companion', id } : { kind: 'main' })}/></div>
  {#if error || libraryError}<Notification message={error || libraryError} kind="error"/>{/if}
  {#if conflict}<p role="status">{conflict}</p>{/if}
  {#if message}<Notification {message}/>{/if}
  {#if widgetEditor}{#key widgetEditor.id}<GenericWidgetDesigner bind:this={widgetDesigner} source={widgetEditor} {active} saveConflict={libraryUnavailable || (widgetConflict ? conflict : '')} onsave={saveWidget} oncancel={closeWidget}/>{/key}
  {:else if !draft && !details}
    <section aria-label="Panels"><PageHeader title="Panels" subtitle="Arrange widgets into panels for your companion screens." level={2}>{#snippet actions()}<button class="secondary-button" disabled={!ready || busy} onclick={() => panelImport?.click()}><Icon name="upload" size={14}/>Import</button><button class="secondary-button" disabled={!ready || busy || workspace.drafts.length >= 20} onclick={createPanel}><Icon name="plus" size={14}/>New</button>{/snippet}</PageHeader>
      <div class="cards panel-cards">{#each workspace.drafts as panel (panel.id)}<article><button class="card" aria-label={`Edit ${panel.title}`} onclick={() => openPanel(panel.id)}><Icon name="grid" size={24}/><strong title={panel.title}>{panel.title}</strong><small>{panel.pages.length} {panel.pages.length===1?'page':'pages'} · {panel.widgets.length} {panel.widgets.length===1?'widget':'widgets'}</small></button><button class="icon-button card-delete" aria-label={`Delete ${panel.title}`} onclick={() => removePanel(panel.id)}><Icon name="trash" size={14}/></button></article>{:else}<p class="empty-library">No panels yet.</p>{/each}</div>
    </section>
    <section aria-label="Widgets"><PageHeader title="Widgets" subtitle="Design visuals, signals, states, motion and sound." level={2}>{#snippet actions()}<button class="secondary-button" disabled={!ready} onclick={() => widgetImport?.click()}><Icon name="upload" size={14}/>Import</button><button class="secondary-button" disabled={!ready} onclick={() => download({ definitions: workspace.sources }, 'widgets.json')}><Icon name="download" size={14}/>Export</button><button class="secondary-button" disabled={!ready} onclick={() => editWidget()}><Icon name="plus" size={14}/>New</button>{/snippet}</PageHeader>
      <div class="cards widget-cards">{#each sources as item (item.id)}<article><button class="card widget-card" aria-label={`Edit design ${item.title}`} title={item.title} onclick={() => editWidget(item)}><div class="thumbnail"><SourcePresentation source={item} fit="contain"/></div><strong>{item.title}</strong><small>{item.size.width} × {item.size.height}</small></button></article>{/each}</div>
    </section>
  {:else if draft && page}
    <PageHeader title={draft.title} subtitle={`${dirty ? 'Unsaved changes' : 'Saved'} · Browser target`} onback={showMain} backDisabled={busy}>{#snippet actions()}<button class="secondary-button" aria-label="Undo" disabled={!past.length || busy} onclick={undo}><Icon name="undo" size={14}/></button><button class="secondary-button" aria-label="Redo" disabled={!future.length || busy} onclick={redo}><Icon name="redo" size={14}/></button><button class="secondary-button" disabled={busy && !build} onclick={busy ? invalidate : compile}><Icon name={busy ? 'close' : 'refresh'} size={14}/>{busy ? 'Cancel' : 'Compile'}</button><button class="secondary-button" disabled={!compiled || busy} onclick={() => { trying = !trying; output = {}; player?.reset(); }}><Icon name={trying ? 'stop' : 'play'} size={14}/>{trying ? 'Stop' : 'Try'}</button><button class="secondary-button" disabled={!compiled || busy} onclick={() => download({ draft, sources: workspace.sources.filter(s => draft!.widgets.some(w => w.sourceId === s.id)), publication: compiled }, `${draft!.id}.osg-panel`)}><Icon name="download" size={14}/>Export</button><span class="save-action" title={saveDisabledReason||undefined}><button class="primary-button" disabled={!!saveDisabledReason} onclick={save} title={saveDisabledReason||undefined}><Icon name="save" size={14}/>Save</button></span>{/snippet}</PageHeader>
    <div class="workspace" inert={saving}>
      <aside aria-label="Panel configuration"><h3>Panel</h3><label>Name<input aria-label="Panel name" value={draft.title} onchange={e => change(d => d.title = e.currentTarget.value)}/></label><label>Page<select value={page.id} onchange={e => { pageId = e.currentTarget.value; trying = false; player?.reset(); }}>{#each draft.pages as p}<option value={p.id}>{p.title}</option>{/each}</select></label><div class="actions"><button class="secondary-button" onclick={() => change(d => d.pages.push({ id: designerId('page'), title: `Page ${d.pages.length + 1}`, width: page!.width, height: page!.height }))} disabled={draft.pages.length >= 16}><Icon name="plus" size={14}/>Add</button><button class="secondary-button" disabled={draft.pages.length < 2} onclick={() => { const id = page!.id; change(d => { d.pages = d.pages.filter(p => p.id !== id); d.widgets = d.widgets.filter(w => w.pageId !== id); }); pageId = draft!.pages[0].id; }}><Icon name="trash" size={14}/>Remove</button></div>
        <label>Width<input type="number" min="320" max="8192" step="1" value={page.width} onchange={e => change(d => d.pages.find(p => p.id === page!.id)!.width = e.currentTarget.valueAsNumber)}/></label><label>Height<input type="number" min="320" max="8192" step="1" value={page.height} onchange={e => change(d => d.pages.find(p => p.id === page!.id)!.height = e.currentTarget.valueAsNumber)}/></label>
        {#each resolutionWarnings as warning (warning.companionId)}<div class="panel-resolution-warning" role="status"><Icon name="alert" size={17}/><span>{warning.message}</span></div>{/each}
        <label>Background<input aria-label="Panel background" type="color" value={page.background??'#0d1820'} onchange={e=>change(d=>d.pages.find(p=>p.id===page!.id)!.background=e.currentTarget.value)}/></label>
        <h3>Widgets</h3>{#each sources as item}<button class="secondary-button palette" onclick={() => add(item)} disabled={trying || busy}><Icon name="plus" size={14}/>{item.title}</button>{/each}<button class="secondary-button" onclick={() => editWidget()}><Icon name="pencil" size={14}/>New</button>
        <h3>Target</h3><label class="check"><input type="checkbox" checked={motion} onchange={e => { motion = e.currentTarget.checked; invalidate(); }}/>Animations</label><label class="check"><input type="checkbox" checked={audio} onchange={e => { audio = e.currentTarget.checked; invalidate(); }}/>Audio</label><small>Static visuals and live feedback remain available with media disabled.</small>
        <h3>Assignments</h3>{#each host.host?.clients ?? [] as client}<label class="check"><input type="checkbox" value={String(client.companionId)} disabled={saving || client.kind !== 'browser'} checked={selectedCompanions.includes(String(client.companionId))} onchange={e => { assignmentEdits = new Map(assignmentEdits).set(String(client.companionId), e.currentTarget.checked); dirty = true; }}/>{String(client.name ?? client.companionId)}{client.kind !== 'browser' ? ' · unsupported target' : ''}</label>{:else}<small>Pair a browser companion to assign this panel.</small>{/each}
      </aside>
      <main><div class="stage-heading"><strong>{trying ? 'Compiled Try' : 'Source preview'}</strong><span>{page.width} × {page.height}</span></div><div class="stage" bind:this={stage} style:background={page.background??'#0d1820'} style:aspect-ratio={`${page.width}/${page.height}`}>
        {#if trying && compiled}<CompiledPanel bind:this={player} publication={compiled} pageId={page.id} onoutput={value => output = { ...output, [value.controlId]: value.value }} onerror={value => error = value}/>
        {:else}{#each draft.widgets.filter(w => w.pageId === page!.id) as item (item.id)}{@const placementSource = sourceFor(item)}<div class="placement" class:selected={item.id === selected} style:left={`${item.bounds.x * 100}%`} style:top={`${item.bounds.y * 100}%`} style:width={`${item.bounds.width * 100}%`} style:height={`${item.bounds.height * 100}%`}><button class="widget-drag" aria-label={`Select ${item.label}`} onpointerdown={e => startDrag(e, item.id)} onpointermove={moveDrag} onpointerup={endDrag} onpointercancel={endDrag} onclick={() => selected = item.id}>{#if placementSource}<SourcePresentation source={placementSource} parameters={item.parameters}/>{:else}<span>Widget source unavailable</span>{/if}</button>{#if selected === item.id}<button class="resize" aria-label={`Resize ${item.label}`} onpointerdown={e => startDrag(e, item.id, true)} onpointermove={moveDrag} onpointerup={endDrag} onpointercancel={endDrag}><Icon name="fit" size={14}/></button>{/if}</div>{/each}{/if}
      </div><p class="hint">{trying ? 'Inputs stay inside this simulator.' : 'Drag to move. Resize from the corner or enter exact pixel dimensions. Arrow keys move the selected widget.'}</p>{#if progress}<p role="status">{progress}</p>{/if}{#if resources}<div class="resources" aria-label="Compiled resources"><strong>Compiled</strong><span>{Math.ceil(resources.assetBytes / 1024)} KiB assets</span><span>{Math.ceil(resources.manifestBytes / 1024)} KiB manifest</span><span>{Math.ceil(resources.activeCandidateBytes / 1024)} KiB active + candidate</span>{#each compiled!.document.presentation.artifacts.flatMap(a => a.artifact.fallbacks) as fallback}<span>{fallback.feature}: {fallback.reason}</span>{/each}</div>{/if}{#if trying}<pre aria-label="Requested outputs">{JSON.stringify(output, null, 2)}</pre>{/if}</main>
      <aside aria-label="Widget properties">{#if widget && source}<h3>{widget.label}</h3><label>Label<input value={widget.label} onchange={e => change(d => d.widgets.find(w => w.id === selected)!.label = e.currentTarget.value)}/></label>{#each ['x', 'y', 'width', 'height'] as field}<label>{field}<input aria-label={`Widget ${field}`} type="number" step="1" min={field === 'width' || field === 'height' ? 1 : 0} value={Math.round(widget.bounds[field as keyof PanelBounds] * (field === 'x' || field === 'width' ? page.width : page.height))} onchange={e => bounds({ [field]: e.currentTarget.valueAsNumber / (field === 'x' || field === 'width' ? page!.width : page!.height) })}/></label>{/each}
        {#each source.parameters as p}<label>{p.label}{#if p.type === 'boolean'}<input type="checkbox" checked={Boolean(widget.parameters[p.id] ?? p.default)} onchange={e => parameter(p.id, e.currentTarget.checked)}/>{:else}<input type={p.type === 'number' ? 'number' : p.type === 'color' ? 'color' : 'text'} min={p.type === 'number' ? p.min : undefined} max={p.type === 'number' ? p.max : undefined} value={String(widget.parameters[p.id] ?? p.default)} onchange={e => parameter(p.id, p.type === 'number' ? e.currentTarget.valueAsNumber : e.currentTarget.value)}/>{/if}</label>{/each}
        <button class="secondary-button" onclick={() => editWidget(source, widget!.id)}><Icon name="pencil" size={14}/>Edit design</button><button class="secondary-button" onclick={removeSelected}><Icon name="trash" size={14}/>Remove</button><small>Resizing or editing invalidates the compiled result.</small>
      {:else if widget}<h3>{widget.label}</h3><p>Widget source unavailable.</p><button class="secondary-button" onclick={removeSelected}><Icon name="trash" size={14}/>Remove</button>{:else}<h3>Selection</h3><p>Select a widget to edit its placement and parameters.</p>{/if}</aside>
    </div>
  {/if}
  <input hidden type="file" bind:this={widgetImport} aria-label="Import widget sources" accept="application/json,.json" onchange={e => { void importWidgets(e.currentTarget.files?.[0]); e.currentTarget.value = ''; }}/>
  <input hidden type="file" bind:this={panelImport} aria-label="Import compiled panel" accept="application/json,.json,.osg-panel" onchange={e => { void importPanel(e.currentTarget.files?.[0]); e.currentTarget.value = ''; }}/>
</section>
<style>
  .compiled-designer {display:grid;gap:var(--ui-section-gap);min-width:0;font-size:var(--ui-text-size);line-height:var(--ui-line-height);}
  .compiled-designer > :global(.page-header) {margin-bottom:0;}
  .compiled-designer :global(.primary-button) {border:1px solid transparent;}
  .hint, small {color:var(--muted);font-size:var(--ui-text-size);line-height:var(--ui-line-height);}
  .actions {display:flex;gap:8px;flex-wrap:wrap;}
  .save-action {display:inline-flex;}
  .save-action button:disabled {pointer-events:none;}
  .cards {display:grid;grid-template-columns:repeat(auto-fill,minmax(min(260px,100%),1fr));gap:20px;}
  .cards article {position:relative;display:flex;align-items:flex-start;gap:12px;min-width:0;padding:var(--ui-panel-padding);border:1px solid var(--line);border-radius:9px;background:var(--surface);}
  .cards article:hover, .cards article:focus-within {border-color:var(--accent);}
  .card {flex:1;display:flex;flex-direction:column;align-items:flex-start;gap:8px;min-width:0;padding:0;border:0;background:transparent;text-align:left;color:var(--text);font-size:var(--ui-text-size);line-height:var(--ui-line-height);}
  .card strong {width:100%;min-width:0;overflow:hidden;text-overflow:ellipsis;white-space:nowrap;font-size:var(--ui-heading-size);font-weight:600;line-height:var(--ui-line-height);}
  .panel-cards .card {min-height:112px;}
  .panel-cards .card > :global(svg) {color:var(--accent);margin-bottom:8px;}
  .card-delete {flex-shrink:0;width:28px;height:28px;}
  .empty-library {grid-column:1/-1;padding:32px 20px;border:1px dashed var(--line);border-radius:9px;color:var(--muted);text-align:center;}
  .widget-cards article {height:240px;}
  .card.widget-card {display:grid;grid-template-rows:minmax(0,1fr) auto auto;align-items:stretch;width:100%;height:100%;}
  .widget-card .thumbnail {height:100%;min-height:0;min-width:0;}
  .thumbnail {width:100%;flex-shrink:0;background:transparent;}
  .workspace {display:grid;grid-template-columns:220px minmax(300px,1fr) 220px;gap:20px;align-items:start;}
  .workspace aside {display:grid;gap:12px;min-width:0;padding:var(--ui-panel-padding);background:var(--surface);border:1px solid var(--line);border-radius:9px;}
  .workspace aside h3 {margin:4px 0;font-size:var(--ui-heading-size);font-weight:600;line-height:var(--ui-line-height);}
  label {display:grid;gap:8px;min-width:0;font-size:var(--ui-text-size);line-height:var(--ui-line-height);color:var(--soft);}
  .check {display:flex;align-items:center;gap:8px;}
  .check input {width:auto;}
  input, select {box-sizing:border-box;width:100%;min-width:0;padding:10px 12px;background:var(--input-bg);border:1px solid var(--line);border-radius:5px;color:var(--text);font:inherit;}
  input[type=color] {height:36px;padding:3px;}
  .workspace main {min-width:0;}
  .stage {position:relative;width:100%;background:#0d1820;overflow:hidden;border:1px solid var(--line);box-sizing:border-box;}
  .stage-heading {display:flex;justify-content:space-between;gap:12px;padding:8px 0;font-size:var(--ui-text-size);}
  .placement {position:absolute;box-sizing:border-box;}
  .placement.selected {outline:1px solid var(--accent);outline-offset:1px;}
  .widget-drag {display:block;width:100%;height:100%;padding:0;border:0;border-radius:0;background:transparent;touch-action:none;}
  .resize {position:absolute;right:0;bottom:0;display:flex;padding:2px;border:1px solid var(--line);border-radius:0;background:var(--surface);color:var(--text);touch-action:none;z-index:2;}
  .palette {justify-content:flex-start;text-align:left;white-space:normal;}
  .resources {display:flex;gap:12px;flex-wrap:wrap;padding:12px;background:var(--surface);border:1px solid var(--line);border-radius:6px;font-size:var(--ui-text-size);}
  .panel-resolution-warning {display:flex;gap:8px;align-items:start;padding:12px;border:1px solid #ad875440;border-radius:6px;background:#ad875408;color:#c9ab7b;font-size:var(--ui-text-size);line-height:var(--ui-line-height);}
  .panel-resolution-warning :global(svg) {flex-shrink:0;}
  pre {overflow:auto;font-size:var(--ui-text-size);max-height:180px;}
  @media(max-width:1100px) {
    .workspace {grid-template-columns:180px minmax(260px,1fr);}
    .workspace > aside:last-child {grid-column:1/-1;grid-template-columns:repeat(auto-fit,minmax(min(130px,100%),1fr));}
  }
  @media(max-width:700px) {
    .workspace {display:flex;flex-direction:column;}
    .workspace > * {width:100%;box-sizing:border-box;}
    .workspace aside {padding:16px;}
  }
</style>
