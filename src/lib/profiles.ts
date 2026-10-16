/** Portable unified profile contracts. Native validation remains authoritative. */
export const PROFILE_SCHEMA_VERSION = 1;
export const MAX_PROFILE_BYTES = 524_288;
export const MAX_PROFILES = 32;
export const DEFAULT_PROFILE_ID = '00000000-0000-4000-8000-000000000001';
export type ProfileValue = boolean | number | string | null;
export type ProfilePort = {
  id: string; name: string; direction: 'input' | 'output'; delivery: 'state' | 'event' | 'action';
  valueType: 'boolean' | 'integer' | 'number' | 'text' | 'enum'; unit: string | null;
  range: { min: number; max: number } | null; enumValues: string[];
  freshnessMs: number | null; fallback: ProfileValue;
};
export type ProfileResource = { id: string; name: string; kind: 'device' | 'companion' | 'game'; required: boolean; ports: ProfilePort[] };
export type ProfileEndpoint = { nodeId: string; portId: string };
export type ProfileBinding = { id: string; from: ProfileEndpoint; to: ProfileEndpoint };
export type ProfileOperation = { kind: 'identity' | 'not' | 'toggle' | 'trigger' } | { kind: 'scale'; factor: number; offset: number } | { kind: 'threshold'; threshold: number };
export type ProfileProcessingNode = { id: string; name: string; operation: ProfileOperation; inputs: ProfilePort[]; outputs: ProfilePort[] };
export type ProfileVirtualOutput = { id: string; name: string; required: boolean; ports: ProfilePort[] };
export type ProfileDocument = {
  schemaVersion: 1; id: string; name: string; revision: number; eligibility: { gameIds: string[] };
  resources: ProfileResource[]; bindings: ProfileBinding[]; processingNodes: ProfileProcessingNode[];
  virtualOutputs: ProfileVirtualOutput[]; visibility: { resourceId: string; mode: 'visible' | 'hidden-while-active' }[];
  overrides: { resourceId: string; enabled: boolean | null; debounceMs: number | null; invertPorts: string[] }[];
  companionAssignments: { resourceId: string; publicationHash: string }[];
  editor: { positions: { nodeId: string; x: number; y: number }[]; viewport: { x: number; y: number; zoom: number } };
};
export type ProfileSelectionMode = 'manual' | 'automatic';
export type AppliedProfileSnapshot = { profileId: string; revision: number; semanticHash: string };
export type ProfileSession = {
  selectedProfileId: string; selectionMode: ProfileSelectionMode; state: 'stopped' | 'paused' | 'faulted';
  appliedSnapshot: AppliedProfileSnapshot | null; fault: string | null;
};
export type ProfileSummary = { id: string; name: string; revision: number; isDefault: boolean; semanticHash: string };
export type ProfileCommand = 'profile_list' | 'profile_get' | 'profile_create' | 'profile_rename' | 'profile_duplicate' | 'profile_delete' | 'profile_save' | 'profile_import' | 'profile_export' | 'profile_status' | 'profile_select' | 'profile_pause' | 'profile_selection_mode' | 'profile_reset';
export type ProfileRequest =
  | { command: 'profile_list' | 'profile_status' }
  | { command: 'profile_get' | 'profile_export'; id: string }
  | { command: 'profile_create'; name: string }
  | { command: 'profile_rename'; id: string; name: string; expectedRevision: number }
  | { command: 'profile_duplicate'; id: string; name?: string }
  | { command: 'profile_delete'; id: string; expectedRevision: number }
  | { command: 'profile_save'; document: ProfileDocument; expectedRevision: number }
  | { command: 'profile_import'; document: ProfileDocument; name?: string }
  | { command: 'profile_select'; id: string; selectionSource: ProfileSelectionMode }
  | { command: 'profile_pause'; paused: boolean }
  | { command: 'profile_selection_mode'; mode: ProfileSelectionMode }
  | { command: 'profile_reset'; confirm: true };
export type ProfileResult = { document?: ProfileDocument; profiles?: ProfileSummary[]; session?: ProfileSession; deletedId?: string; diagnostics?: string[]; readOnly?: boolean };
export type ProfileSend = (request: ProfileRequest) => Promise<ProfileResult>;
export type DeepReadonly<T> = T extends object ? { readonly [K in keyof T]: DeepReadonly<T[K]> } : T;
export type ProfileEditorState = { document: ProfileDocument; baseDocument: ProfileDocument; baseRevision: number };

const object = (value: unknown): value is Record<string, unknown> => value !== null && typeof value === 'object' && !Array.isArray(value);
const exact = (value: unknown, keys: string[]): value is Record<string, unknown> => object(value) && Object.keys(value).length === keys.length && keys.every(key => Object.hasOwn(value, key));
const finite = (value: unknown): value is number => typeof value === 'number' && Number.isFinite(value);
const whole = (value: unknown, min: number, max: number): value is number => finite(value) && Number.isSafeInteger(value) && value >= min && value <= max;
const text = (value: unknown, max = 80): value is string => typeof value === 'string' && value.length > 0 && value.trim() === value && [...value].length <= max && !/[\u0000-\u001f\u007f-\u009f]/.test(value);
const identifier = (value: unknown): value is string => typeof value === 'string' && /^[A-Za-z][A-Za-z0-9._-]{0,79}$/.test(value);
const uuid = (value: unknown): value is string => typeof value === 'string' && /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/.test(value);
const list = (value: unknown, max: number): value is unknown[] => Array.isArray(value) && value.length <= max;
const coordinate = (value: unknown) => finite(value) && Math.abs(value) <= 1_000_000;
const clone = <T>(value: T): T => JSON.parse(JSON.stringify(value));
const labelKeys = ['id', 'name'];
const nodeLabel = (value: Record<string, unknown>) => identifier(value.id) && text(value.name);
const endpoint = (value: unknown) => exact(value, ['nodeId', 'portId']) && identifier(value.nodeId) && identifier(value.portId);
function port(value: unknown): boolean {
  if (!exact(value, ['id', 'name', 'direction', 'delivery', 'valueType', 'unit', 'range', 'enumValues', 'freshnessMs', 'fallback']) || !nodeLabel(value)) return false;
  if (!['input', 'output'].includes(String(value.direction)) || !['state', 'event', 'action'].includes(String(value.delivery)) || !['boolean', 'integer', 'number', 'text', 'enum'].includes(String(value.valueType))) return false;
  const numeric = value.valueType === 'integer' || value.valueType === 'number';
  if (value.unit !== null && (!numeric || !text(value.unit, 32))) return false;
  if (numeric) {
    if (!exact(value.range, ['min', 'max']) || !finite(value.range.min) || !finite(value.range.max) || value.range.min > value.range.max || Math.abs(value.range.min) > Number.MAX_SAFE_INTEGER || Math.abs(value.range.max) > Number.MAX_SAFE_INTEGER) return false;
    if (value.valueType === 'integer' && (!Number.isSafeInteger(value.range.min) || !Number.isSafeInteger(value.range.max))) return false;
  } else if (value.range !== null) return false;
  if (!list(value.enumValues, 64) || !value.enumValues.every(item => text(item)) || new Set(value.enumValues).size !== value.enumValues.length) return false;
  if (value.valueType === 'enum' ? value.enumValues.length === 0 : value.enumValues.length !== 0) return false;
  const fallback = value.fallback;
  if (value.delivery !== 'state') return value.freshnessMs === null && fallback === null;
  if (!whole(value.freshnessMs, 1, 600_000) || fallback === null) return false;
  if (value.valueType === 'boolean') return typeof fallback === 'boolean';
  if (value.valueType === 'text') return typeof fallback === 'string' && [...fallback].length <= 256 && !/[\u0000-\u001f\u007f-\u009f]/.test(fallback);
  if (value.valueType === 'enum') return typeof fallback === 'string' && value.enumValues.includes(fallback);
  return finite(fallback) && (value.valueType !== 'integer' || Number.isSafeInteger(fallback)) && (value.range === null || fallback >= (value.range as { min: number }).min && fallback <= (value.range as { max: number }).max);
}
function ports(value: unknown): value is ProfilePort[] {
  return list(value, 128) && value.every(port) && new Set(value.map(item => (item as ProfilePort).id)).size === value.length;
}
function operation(value: unknown): boolean {
  if (!object(value)) return false;
  if (['identity', 'not', 'toggle', 'trigger'].includes(String(value.kind))) return exact(value, ['kind']);
  if (value.kind === 'scale') return exact(value, ['kind', 'factor', 'offset']) && finite(value.factor) && finite(value.offset);
  return value.kind === 'threshold' && exact(value, ['kind', 'threshold']) && finite(value.threshold);
}
/** Structural checks reject local assignments and unknown fields; native code validates graph semantics. */
export function profileDocumentError(value: unknown): string {
  let encoded: string;
  try { encoded = JSON.stringify(value); } catch { return 'Profile must be JSON data.'; }
  if (!encoded || new TextEncoder().encode(encoded).length > MAX_PROFILE_BYTES) return 'Use a profile no larger than 512 KiB.';
  if (!exact(value, ['schemaVersion', 'id', 'name', 'revision', 'eligibility', 'resources', 'bindings', 'processingNodes', 'virtualOutputs', 'visibility', 'overrides', 'companionAssignments', 'editor'])) return 'Use the unified portable profile document without local assignments or legacy fields.';
  if (value.schemaVersion !== PROFILE_SCHEMA_VERSION || !uuid(value.id) || !text(value.name) || !whole(value.revision, 1, Number.MAX_SAFE_INTEGER)) return 'Choose a supported profile schema, canonical UUID, name and revision.';
  if (!exact(value.eligibility, ['gameIds']) || !list(value.eligibility.gameIds, 32) || !value.eligibility.gameIds.every(identifier) || new Set(value.eligibility.gameIds).size !== value.eligibility.gameIds.length) return 'Choose unique logical game identifiers.';
  if (!list(value.resources, 64) || !value.resources.every(row => exact(row, [...labelKeys, 'kind', 'required', 'ports']) && nodeLabel(row) && ['device', 'companion', 'game'].includes(String(row.kind)) && typeof row.required === 'boolean' && ports(row.ports))) return 'Use valid logical resources and typed ports.';
  if (!list(value.processingNodes, 256) || !value.processingNodes.every(row => exact(row, [...labelKeys, 'operation', 'inputs', 'outputs']) && nodeLabel(row) && operation(row.operation) && ports(row.inputs) && ports(row.outputs))) return 'Use supported processing nodes and typed ports.';
  if (!list(value.virtualOutputs, 16) || !value.virtualOutputs.every(row => exact(row, [...labelKeys, 'required', 'ports']) && nodeLabel(row) && typeof row.required === 'boolean' && ports(row.ports))) return 'Use valid logical virtual outputs.';
  const nodes = [...value.resources, ...value.processingNodes, ...value.virtualOutputs] as (ProfileResource | ProfileProcessingNode | ProfileVirtualOutput)[];
  if (new Set(nodes.map(row => row.id)).size !== nodes.length) return 'Graph node identifiers must be unique.';
  if (nodes.reduce((sum, row) => sum + ('ports' in row ? row.ports.length : row.inputs.length + row.outputs.length), 0) > 1024) return 'Use at most 1024 graph ports.';
  if (!list(value.bindings, 512) || !value.bindings.every(row => exact(row, ['id', 'from', 'to']) && identifier(row.id) && endpoint(row.from) && endpoint(row.to)) || new Set(value.bindings.map(row => (row as ProfileBinding).id)).size !== value.bindings.length) return 'Use unique bindings with logical graph endpoints.';
  if (!list(value.visibility, 64) || !value.visibility.every(row => exact(row, ['resourceId', 'mode']) && identifier(row.resourceId) && ['visible', 'hidden-while-active'].includes(String(row.mode)))) return 'Use logical visibility policies.';
  if (!list(value.overrides, 64) || !value.overrides.every(row => exact(row, ['resourceId', 'enabled', 'debounceMs', 'invertPorts']) && identifier(row.resourceId) && (row.enabled === null || typeof row.enabled === 'boolean') && (row.debounceMs === null || whole(row.debounceMs, 0, 60_000)) && list(row.invertPorts, 128) && row.invertPorts.every(identifier))) return 'Use bounded logical runtime overrides.';
  if (!list(value.companionAssignments, 64) || !value.companionAssignments.every(row => exact(row, ['resourceId', 'publicationHash']) && identifier(row.resourceId) && typeof row.publicationHash === 'string' && /^sha256:[0-9a-f]{64}$/.test(row.publicationHash))) return 'Use logical companion publication assignments.';
  if (!exact(value.editor, ['positions', 'viewport']) || !list(value.editor.positions, 336) || !value.editor.positions.every(row => exact(row, ['nodeId', 'x', 'y']) && identifier(row.nodeId) && coordinate(row.x) && coordinate(row.y)) || !exact(value.editor.viewport, ['x', 'y', 'zoom']) || !coordinate(value.editor.viewport.x) || !coordinate(value.editor.viewport.y) || !finite(value.editor.viewport.zoom) || value.editor.viewport.zoom < 0.1 || value.editor.viewport.zoom > 4) return 'Use bounded editor positions and viewport.';
  return '';
}
export function parseProfileDocument(source: string): ProfileDocument {
  if (new TextEncoder().encode(source).length > MAX_PROFILE_BYTES) throw new Error('Use a profile no larger than 512 KiB.');
  const value: unknown = JSON.parse(source);
  const error = profileDocumentError(value);
  if (error) throw new Error(error);
  return value as ProfileDocument;
}
export function exportProfileDocument(document: ProfileDocument): string {
  const error = profileDocumentError(document);
  if (error) throw new Error(error);
  return JSON.stringify(document);
}
export function profileDraft(document: ProfileDocument): ProfileEditorState {
  const checked = parseProfileDocument(JSON.stringify(document));
  return { document: clone(checked), baseDocument: checked, baseRevision: checked.revision };
}
export function profileDraftDirty(editor: ProfileEditorState): boolean {
  return JSON.stringify(editor.document) !== JSON.stringify(editor.baseDocument);
}
function freeze<T>(value: T): DeepReadonly<T> {
  if (value && typeof value === 'object') { for (const child of Object.values(value)) freeze(child); Object.freeze(value); }
  return value as DeepReadonly<T>;
}
/** Capture native session state without sharing mutable document or transport objects. */
export function captureProfileSession(session: ProfileSession): DeepReadonly<ProfileSession> {
  return freeze(clone(session));
}
function documentResult(result: ProfileResult): ProfileDocument {
  if (!result.document) throw new Error('Profile response is missing its document.');
  return parseProfileDocument(JSON.stringify(result.document));
}

/** Storage and selection helpers only. No method applies a profile or activates hardware. */
export class ProfilesClient {
  private readonly send: ProfileSend;
  private currentEditor: ProfileEditorState | null = null;
  private currentSession: DeepReadonly<ProfileSession> | null = null;
  private editorEpoch = 0;
  constructor(send: ProfileSend) { this.send = send; }
  get editor(): ProfileEditorState | null { return this.currentEditor ? clone(this.currentEditor) : null; }
  get session(): DeepReadonly<ProfileSession> | null { return this.currentSession; }
  private async request(request: ProfileRequest): Promise<ProfileResult> {
    const result = clone(await this.send(clone(request)));
    if (result.session) this.currentSession = captureProfileSession(result.session);
    return result;
  }
  async list(): Promise<ProfileResult> { return this.request({ command: 'profile_list' }); }
  async status(): Promise<ProfileResult> { return this.request({ command: 'profile_status' }); }
  async get(id: string): Promise<ProfileDocument> { return documentResult(await this.request({ command: 'profile_get', id })); }
  async open(id: string): Promise<ProfileEditorState> {
    const epoch = ++this.editorEpoch;
    const editor = profileDraft(await this.get(id));
    if (epoch === this.editorEpoch) this.currentEditor = editor;
    return clone(editor);
  }
  close(): void { this.editorEpoch++; this.currentEditor = null; }
  updateDraft(document: ProfileDocument): void {
    if (!this.currentEditor || document.id !== this.currentEditor.document.id || document.revision !== this.currentEditor.baseRevision) throw new Error('Open this profile before changing its draft; keep its saved identity and revision.');
    this.currentEditor = { ...this.currentEditor, document: clone(document) };
  }
  async create(name: string): Promise<ProfileDocument> { return documentResult(await this.request({ command: 'profile_create', name })); }
  async rename(id: string, name: string, expectedRevision: number): Promise<ProfileDocument> { return documentResult(await this.request({ command: 'profile_rename', id, name, expectedRevision })); }
  async duplicate(id: string, name?: string): Promise<ProfileDocument> { return documentResult(await this.request({ command: 'profile_duplicate', id, ...(name === undefined ? {} : { name }) })); }
  async delete(id: string, expectedRevision: number): Promise<ProfileResult> {
    const result = await this.request({ command: 'profile_delete', id, expectedRevision });
    if (this.currentEditor?.document.id === id) this.close();
    return result;
  }
  async save(document: ProfileDocument, expectedRevision = document.revision): Promise<ProfileDocument> {
    const error = profileDocumentError(document);
    if (error) throw new Error(error);
    const checked = parseProfileDocument(JSON.stringify(document));
    return documentResult(await this.request({ command: 'profile_save', document: checked, expectedRevision }));
  }
  async saveDraft(): Promise<ProfileDocument> {
    const editor = this.currentEditor;
    if (!editor) throw new Error('Open a profile before saving its draft.');
    const epoch = this.editorEpoch;
    const saved = await this.save(editor.document, editor.baseRevision);
    const current = this.currentEditor;
    if (epoch === this.editorEpoch && current?.document.id === editor.document.id && current.baseRevision === editor.baseRevision) {
      // Edits made while this save was pending remain dirty against the newly
      // committed baseline and can be saved with its optimistic revision.
      this.currentEditor = current === editor ? profileDraft(saved) : {
        document: { ...current.document, revision: saved.revision },
        baseDocument: clone(saved), baseRevision: saved.revision,
      };
    }
    return saved;
  }
  async import(source: string, name?: string): Promise<ProfileDocument> {
    return documentResult(await this.request({ command: 'profile_import', document: parseProfileDocument(source), ...(name === undefined ? {} : { name }) }));
  }
  async export(id: string): Promise<string> { return exportProfileDocument(documentResult(await this.request({ command: 'profile_export', id }))); }
  async select(id: string, selectionSource: ProfileSelectionMode = 'manual'): Promise<ProfileResult> { return this.request({ command: 'profile_select', id, selectionSource }); }
  async pause(paused = true): Promise<ProfileResult> { return this.request({ command: 'profile_pause', paused }); }
  async selectionMode(mode: ProfileSelectionMode): Promise<ProfileResult> { return this.request({ command: 'profile_selection_mode', mode }); }
  async reset(): Promise<ProfileResult> {
    const result = await this.request({ command: 'profile_reset', confirm: true });
    this.close();
    return result;
  }
}
