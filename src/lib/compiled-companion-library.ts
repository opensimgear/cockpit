import { desktopDesignerStorage } from './persistence/designer-storage.ts';
import { compileWidget, createToggleSource, validateWidgetSource, resolvedParameters, emptyDesign, type WidgetSource } from '../../packages/widget-compiler/src/index.ts';

import { createCompiledPanel, verifyCompiledPanelPublication, canonicalWidget, widgetDigest, type CompiledPanelPublication, type PanelBounds, type WidgetCapabilities, type Scalar } from '../../packages/widget-runtime/src/index.ts';

export interface WidgetSourceBundle { definitions: WidgetSource[] }
export interface CompiledDraft {
  id: string; title: string;
  pages: { id: string; title: string; width: number; height: number; background?: string }[];
  widgets: { id: string; pageId: string; label: string; sourceId: string; parameters: Record<string, Scalar>; bounds: PanelBounds }[];
}
export interface CompiledWorkspace {
  sources: WidgetSource[]; drafts: CompiledDraft[];
  publications: Record<string, CompiledPanelPublication[]>;
}
export interface CompiledWorkspaceStorage { read(): Promise<unknown>; write(value: CompiledWorkspace): Promise<CompiledWorkspace | void> }
const MAX_BYTES = 64 * 1024 * 1024;
const encoder = new TextEncoder();
function ensure(value: unknown, message: string): asserts value { if (!value) throw new Error(message); }
function stable(value: unknown): asserts value is string { ensure(typeof value === 'string' && /^[A-Za-z0-9][A-Za-z0-9_-]{0,63}$/.test(value), 'Invalid designer ID'); }
function title(value: unknown): asserts value is string { ensure(typeof value === 'string' && value.trim() && value.length <= 120 && !/[\u0000-\u001f]/.test(value), 'Enter a name of 1 to 120 characters.'); }
function exact(value: unknown, fields: string[]): Record<string, any> { ensure(value && typeof value === 'object' && !Array.isArray(value) && Object.keys(value).length === fields.length && fields.every(field => Object.hasOwn(value, field)), 'Unexpected or missing workspace field'); return value as Record<string, any>; }
export function designerId(prefix: string): string { return `${prefix}-${crypto.randomUUID().slice(0, 12)}`; }
export function emptyCompiledWorkspace(): CompiledWorkspace { return { sources: [createToggleSource()], drafts: [], publications: {} }; }
export function newCompiledDraft(name = 'New panel', width = 1280, height = 800): CompiledDraft { return { id: designerId('panel'), title: name, pages: [{ id: designerId('page'), title: 'Main', width, height }], widgets: [] }; }
export function blankWidgetSource(): WidgetSource {
  return { design:emptyDesign(), id: designerId('widget'), title: 'New widget', size: { width: 320, height: 160 }, parameters: [],
    behavior: { version: 1, signals: [], events: [], regions: [] }, visualDomains: [], inputs: [], outputs: [], hitRegions: [],
    scene: [{ id: 'surface', kind: 'rectangle', x: 0, y: 0, width: 320, height: 160, radius: 16, fill: 'none' }] };
}
export function mergeWidgetSources(previous: WidgetSource[], incoming: WidgetSource[]): WidgetSource[] {
  const next = structuredClone(previous);
  for (const source of incoming) {
    const checked = validateWidgetSource(source); ensure(checked.ok, checked.ok ? '' : checked.error);
    const old = next.find(item => item.id === source.id);
    if(old)next[next.indexOf(old)]=checked.value;else next.push(checked.value);
  }
  ensure(next.length <= 256 && encoder.encode(canonicalWidget(next)).length <= 8 * 1024 * 1024, 'Widget source library exceeds its storage budget.');
  return next;
}
/** Updating a source makes its saved panel assets stale until the next compilation. */
export function replaceWorkspaceSources(workspace: CompiledWorkspace, incoming: WidgetSource[]): CompiledWorkspace {
  const sources = mergeWidgetSources(workspace.sources, incoming);
  const changed = new Set(sources.filter(source => canonicalWidget(source) !== canonicalWidget(workspace.sources.find(old => old.id === source.id) ?? null)).map(source => source.id));
  const publications = { ...workspace.publications };
  for (const draft of workspace.drafts) if (draft.widgets.some(widget => changed.has(widget.sourceId))) delete publications[draft.id];
  return { ...workspace, sources, publications };
}
export function validateCompiledDraft(input: unknown, sources: WidgetSource[]): CompiledDraft {
  const draft = exact(input, ['id', 'title', 'pages', 'widgets']); stable(draft.id); title(draft.title);
  ensure(Array.isArray(draft.pages) && draft.pages.length >= 1 && draft.pages.length <= 16 && Array.isArray(draft.widgets) && draft.widgets.length <= 192, 'Panel exceeds page or widget limits.');
  const ids = new Set<string>();
  const unique = (id: unknown) => { stable(id); ensure(!ids.has(id), 'Duplicate designer ID'); ids.add(id); };
  for (const page of draft.pages) { exact(page, ['id', 'title', 'width', 'height', ...(Object.hasOwn(page,'background')?['background']:[])]);ensure(page.background===undefined||typeof page.background==='string'&&/^#[0-9a-fA-F]{6}$/.test(page.background),'Use a six-digit hex background color.'); unique(page.id); title(page.title); ensure([page.width, page.height].every(n => Number.isInteger(n) && n >= 320 && n <= 8192), 'Panel dimensions must be 320 to 8192 whole pixels.'); }
  for (const widget of draft.widgets) {
    exact(widget, ['id', 'pageId', 'label', 'sourceId', 'parameters', 'bounds']); unique(widget.id); title(widget.label);
    ensure(draft.pages.some((page: any) => page.id === widget.pageId), 'Unknown widget page');
    const source = sources.find(source => source.id === widget.sourceId); ensure(source, 'A panel references an unavailable widget.');
    resolvedParameters(source, widget.parameters);
    const b = exact(widget.bounds, ['x', 'y', 'width', 'height']);
    ensure(Object.values(b).every(v => typeof v === 'number' && Number.isFinite(v) && v >= 0 && v <= 1) && b.width > 0 && b.height > 0 && b.x + b.width <= 1 + 1e-10 && b.y + b.height <= 1 + 1e-10, 'Widget lies outside its page');
  }
  return structuredClone(draft) as CompiledDraft;
}
export async function compilePanelDraft(draft: CompiledDraft, sources: WidgetSource[], options: { signal?: AbortSignal; capabilities?: WidgetCapabilities; motion?: boolean; audio?: boolean; onProgress?: (completed: number, total: number) => void } = {}): Promise<CompiledPanelPublication> {
  const captured = validateCompiledDraft(draft, sources), bank = structuredClone(sources);
  ensure(captured.pages.every(page => captured.widgets.some(widget => widget.pageId === page.id)), 'Add a widget to every page before compiling.');
  const placements = [];
  for (let index = 0; index < captured.widgets.length; index++) {
    if (options.signal?.aborted) throw new DOMException('Panel compilation cancelled', 'AbortError');
    const widget = captured.widgets[index], page = captured.pages.find(page => page.id === widget.pageId)!;
    const source = bank.find(source => source.id === widget.sourceId)!;
    const bundle = await compileWidget(source, { parameters: widget.parameters, width: Math.round(widget.bounds.width * page.width), height: Math.round(widget.bounds.height * page.height), capabilities: options.capabilities, motion: options.motion, audio: options.audio, signal: options.signal });
    placements.push({ id: widget.id, pageId: widget.pageId, label: widget.label, bounds: widget.bounds, bundle }); options.onProgress?.(index + 1, captured.widgets.length);
  }
  if (options.signal?.aborted) throw new DOMException('Panel compilation cancelled', 'AbortError');
  const publication = await createCompiledPanel({ id: captured.id, title: captured.title, viewports: captured.pages.map(({ id: pageId, ...page }) => ({ pageId, ...page })) }, placements, options.capabilities);
  if (options.signal?.aborted) throw new DOMException('Panel compilation cancelled', 'AbortError');
  return publication;
}
/** A valid artifact can still belong to an earlier source or parameter set. */
export async function publicationMatchesDraft(draft: CompiledDraft, sources: WidgetSource[], publication: CompiledPanelPublication): Promise<boolean> {
  const d = publication.document;
  if (d.ui.id !== draft.id || d.ui.title !== draft.title || canonicalWidget(d.ui.pages) !== canonicalWidget(draft.pages.map(({ id, title }) => ({ id, title }))) || canonicalWidget(d.presentation.viewports) !== canonicalWidget(draft.pages.map(({ id: pageId, title: _, ...viewport }) => ({ pageId, ...viewport }))) || d.presentation.widgets.length !== draft.widgets.length) return false;
  for (const widget of draft.widgets) {
    const placed = d.presentation.widgets.find(w => w.id === widget.id), source = sources.find(source => source.id === widget.sourceId);
    if (!placed || !source || placed.pageId !== widget.pageId || placed.label !== widget.label || canonicalWidget(placed.bounds) !== canonicalWidget(widget.bounds)) return false;
    const artifact = d.presentation.artifacts.find(a => a.hash === placed.artifactHash)!.artifact;
    if (artifact.sourceHash !== await widgetDigest(encoder.encode(canonicalWidget(source))) || artifact.parameterHash !== await widgetDigest(encoder.encode(canonicalWidget(resolvedParameters(source, widget.parameters))))) return false;
  }
  return true;
}
async function validateWorkspace(input: unknown): Promise<CompiledWorkspace> {
  const serialized = JSON.stringify(input); ensure(serialized && encoder.encode(serialized).length <= MAX_BYTES, 'Designer storage exceeds 64 MiB.');
  const workspace = exact(JSON.parse(serialized), ['sources', 'drafts', 'publications']);
  ensure(Array.isArray(workspace.sources), 'Invalid widget source library'); const sources = mergeWidgetSources([], workspace.sources);
  ensure(Array.isArray(workspace.drafts) && workspace.drafts.length <= 20, 'Panel library exceeds 20 panels.');
  const drafts = workspace.drafts.map((draft: unknown) => validateCompiledDraft(draft, sources)); ensure(new Set(drafts.map((draft: CompiledDraft) => draft.id)).size === drafts.length, 'Duplicate saved panel');
  ensure(workspace.publications && typeof workspace.publications === 'object' && !Array.isArray(workspace.publications), 'Invalid compiled presentations');
  const publications: Record<string, CompiledPanelPublication[]> = {}; let count = 0;
  for (const [id, values] of Object.entries(workspace.publications)) { stable(id); ensure(Array.isArray(values) && values.length <= 1 && (count += values.length) <= 20, 'Keep one current compiled presentation per panel, up to 20 panels.'); publications[id] = []; for (const value of values) { const checked = await verifyCompiledPanelPublication(value); ensure(checked.document.ui.id === id, 'Publication belongs to a different panel'); publications[id].push(checked); } }
  for (const draft of drafts) { const current = publications[draft.id]?.at(-1); if (current) ensure(await publicationMatchesDraft(draft, sources, current), 'Saved compiled presentation belongs to an earlier panel or widget source. Recompile before saving.'); }
  return { sources, drafts, publications };
}
/** Source and compiled publication are committed together. Invalid saved data
 * remains an error; no compatibility reset can overwrite it. */

export class CompiledWorkspaceStore {
  private pending: Promise<unknown> = Promise.resolve();
  private storage: CompiledWorkspaceStorage;
  constructor(storage: CompiledWorkspaceStorage = desktopDesignerStorage<CompiledWorkspace>() ?? compiledIndexedDbStorage()) { this.storage = storage; }
  private ordered<T>(work: () => Promise<T>): Promise<T> { const result = this.pending.then(work); this.pending = result.catch(() => undefined); return result; }
  load(): Promise<CompiledWorkspace> { return this.ordered(async () => {
    const input = await this.storage.read();
    if (input === undefined) { const empty = emptyCompiledWorkspace(); const saved = await this.storage.write(empty); return saved ? validateWorkspace(saved) : empty; }

    return validateWorkspace(input);
  }); }
  save(input: CompiledWorkspace): Promise<CompiledWorkspace> { const capture = structuredClone(input); return this.ordered(async () => { const checked = await validateWorkspace(capture); const saved = await this.storage.write(checked); return saved ? validateWorkspace(saved) : structuredClone(checked); }); }
}
export function compiledIndexedDbStorage(factory = globalThis.indexedDB): CompiledWorkspaceStorage {
  let opening: Promise<IDBDatabase> | undefined;
  async function open(): Promise<IDBDatabase> {
    ensure(factory, 'IndexedDB is unavailable; changes have not been saved.');
    return opening ??= new Promise((resolve, reject) => {
      const request = factory.open('opensimgear-cockpit-companion-designer', 1);
      request.onupgradeneeded = () => request.result.createObjectStore('workspace');
      request.onsuccess = () => { const db = request.result; db.onversionchange = () => { db.close(); opening = undefined; }; resolve(db); };
      request.onerror = () => { opening = undefined; reject(request.error); };
      request.onblocked = () => { opening = undefined; reject(new Error('Designer storage is blocked by another window.')); };
    });
  }
  async function transact(value?: CompiledWorkspace): Promise<unknown> {
    const db = await open();
    return new Promise((resolve, reject) => {
      const tx = db.transaction('workspace', value ? 'readwrite' : 'readonly'), store = tx.objectStore('workspace');
      const request = value ? store.put(value, 'snapshot') : store.get('snapshot');
      tx.oncomplete = () => resolve(value ? undefined : request.result);
      tx.onerror = () => reject(tx.error); tx.onabort = () => reject(tx.error ?? new Error('Designer save cancelled.'));
    });
  }
  return { read: () => transact(), write: async value => { await transact(value); } };
}
