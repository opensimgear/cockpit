/** Portable unified profile contracts. Native validation remains authoritative. */
export const PROFILE_SCHEMA_VERSION = 2;
export const MAX_PROFILE_BYTES = 524_288;
export const MAX_PROFILES = 32;
export const DEFAULT_PROFILE_ID = '00000000-0000-4000-8000-000000000001';
export type ProfileValue = boolean | number | string | null;
export type ProfileScalarSchema = { type: 'boolean' } | { type: 'rgb' }
  | { type: 'integer' | 'number'; min: number; max: number; unit: string | null }
  | { type: 'text'; maxBytes: number; encoding: 'utf8' | 'ascii' }
  | { type: 'enum'; values: string[] };
export type ProfileActionParameter = { id: string; name: string; schema: ProfileScalarSchema };
export type ProfileActionSemantics = 'setter' | 'one-shot';
export type ProfilePortContract = { delivery: 'state'; schema: ProfileScalarSchema; freshnessMs: number; fallback: ProfileValue }
  | { delivery: 'event'; schema: ProfileScalarSchema }
  | { delivery: 'invocation'; parameters: ProfileActionParameter[]; semantics: ProfileActionSemantics }
  | { delivery: 'action'; parameters: ProfileActionParameter[]; semantics: ProfileActionSemantics; endpoint: string; readable: boolean;
      display: { columns: number; rows: number; encoding: 'utf8' | 'ascii' } | null;
      ownership: { scope: 'endpoint'; maxLeaseMs: number } | null };
export type ProfilePort = { id: string; name: string; direction: 'input' | 'output'; contract: ProfilePortContract };
export type ProfileControllerLayout = { buttons: string[]; axes: { portId: string; range: 'bipolar' | 'unipolar'; fallback: number }[]; hats: string[] };
export type ProfileRounding = 'none' | 'nearest' | 'floor' | 'ceil' | 'truncate';
export type ProfileComparison = 'greater' | 'greater-equal' | 'less' | 'less-equal' | 'equal' | 'not-equal';
export type ProfileBuiltin =
  | { kind: 'axis'; deadZone: number; invert: boolean; scale: number; curve: { input: number; output: number }[] }
  | { kind: 'merge' | 'chord' | 'gate' | 'toggle' }
  | { kind: 'split'; positive: boolean }
  | { kind: 'button-axis'; range: 'bipolar' | 'unipolar'; pressed: number; released: number }
  | { kind: 'threshold'; press: number; release: number }
  | { kind: 'gesture'; gesture: 'tap' | 'hold' | 'double-tap'; holdUs: number; doubleUs: number; pulseUs: number }
  | { kind: 'pulse'; durationUs: number; repeatUs: number }
  | { kind: 'macro'; steps: { afterUs: number; pressed: boolean }[] };
export type ProfileResource = { id: string; name: string; kind: 'device' | 'companion' | 'game'; required: boolean; ports: ProfilePort[] };
export type ProfileEndpoint = { nodeId: string; portId: string };
export type ProfileBinding = { id: string; from: ProfileEndpoint; to: ProfileEndpoint };
export type ProfileOperation = { kind: 'identity' | 'not' | 'invert' | 'gate' | 'any' | 'all' | 'event-to-state' }
  | { kind: 'constant'; value: Exclude<ProfileValue, null> }
  | { kind: 'scale'; factor: number; offset: number }
  | { kind: 'range-map'; inputMin: number; inputMax: number; outputMin: number; outputMax: number; rounding: ProfileRounding }
  | { kind: 'unit-convert'; from: string; to: string; rounding: ProfileRounding }
  | { kind: 'deadzone'; center: number; radius: number; rounding: ProfileRounding }
  | { kind: 'curve'; points: { input: number; output: number }[]; rounding: ProfileRounding }
  | { kind: 'compare'; comparison: ProfileComparison; threshold: number }
  | { kind: 'hysteresis'; press: number; release: number; comparison: ProfileComparison }
  | { kind: 'priority'; order: string[] }
  | { kind: 'event-edge'; edge: 'rising' | 'falling' | 'change' }
  | { kind: 'select'; whenTrue: Exclude<ProfileValue, null>; whenFalse: Exclude<ProfileValue, null> }
  | { kind: 'format'; decimals: number; prefix: string; suffix: string; rounding: ProfileRounding; overflow: 'reject' | 'truncate' }
  | { kind: 'fallback'; unknown: Exclude<ProfileValue, null>; stale: Exclude<ProfileValue, null>; unavailable: Exclude<ProfileValue, null> }
  | { kind: 'action-pack' | 'set-on-change'; arguments: string[] }
  | { kind: 'builtin'; rule: ProfileBuiltin; inputs: string[] }
  | { kind: 'script'; source: string; parameters: Record<string, Exclude<ProfileValue, null>> };
export type ProfileProcessingNode = { id: string; name: string; operation: ProfileOperation; inputs: ProfilePort[]; outputs: ProfilePort[] };
export type ProfileVirtualOutput = { id: string; name: string; required: boolean; ports: ProfilePort[]; layout: ProfileControllerLayout };
export type ProfileDocument = {
  schemaVersion: 2; id: string; name: string; revision: number; eligibility: { gameIds: string[] };
  resources: ProfileResource[]; bindings: ProfileBinding[]; processingNodes: ProfileProcessingNode[];
  virtualOutputs: ProfileVirtualOutput[]; visibility: { resourceId: string; mode: 'visible' | 'hidden-while-active' }[];
  overrides: { resourceId: string; enabled: boolean }[];
  companionAssignments: { resourceId: string; publicationHash: string }[];
  editor: { positions: { nodeId: string; x: number; y: number }[]; viewport: { x: number; y: number; zoom: number } };
};
export type ProfilePortExpectation = { profilePortId: string; observedPortId: string; expectedContractHash: string };
export type ProfileLocalAssignment = { resourceId: string; sourceId: string; ports: ProfilePortExpectation[] };
export type ProfileSelectionMode = 'manual' | 'automatic';
export type AppliedProfileSnapshot = { profileId: string; revision: number; semanticHash: string };
export type ProfileSession = {
  selectedProfileId: string; selectionMode: ProfileSelectionMode; state: 'stopped' | 'paused' | 'faulted';
  appliedSnapshot: AppliedProfileSnapshot | null; fault: string | null;
};
export type ProfileSummary = { id: string; name: string; revision: number; isDefault: boolean; semanticHash: string };
export type ProfileCommand = 'profile_list' | 'profile_get' | 'profile_create' | 'profile_rename' | 'profile_duplicate' | 'profile_delete' | 'profile_save' | 'profile_import' | 'profile_export' | 'profile_status' | 'profile_select' | 'profile_pause' | 'profile_selection_mode' | 'profile_reset' | 'profile_assignments_get' | 'profile_assignments_save';
export type ProfileRequest =
  | { command: 'profile_list' | 'profile_status' }
  | { command: 'profile_get' | 'profile_export' | 'profile_assignments_get'; id: string }
  | { command: 'profile_assignments_save'; id: string; expectedRevision: number; assignments: ProfileLocalAssignment[] }
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
export type ProfileResult = { document?: ProfileDocument; profiles?: ProfileSummary[]; session?: ProfileSession; deletedId?: string; diagnostics?: string[]; readOnly?: boolean; assignments?: ProfileLocalAssignment[] };
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
const endpoint = (value: unknown) => exact(value, ['nodeId', 'portId']) && identifier(value.nodeId) && profilePortIdValid(value.portId);
const bytes = (value: string) => new TextEncoder().encode(value).length;
const boundedText = (value: unknown, max: number): value is string => typeof value === 'string' && value.length > 0 && bytes(value) <= max && !/[\u0000-\u001f\u007f-\u009f]/.test(value);
export const profilePortIdValid = (value: unknown): value is string => boundedText(value, 80);
const scalarValue = (value: unknown): value is Exclude<ProfileValue, null> => typeof value === 'boolean' || finite(value) || typeof value === 'string';
export function profileScalarValid(value: unknown): value is ProfileScalarSchema {
  if (!object(value)) return false;
  if (value.type === 'boolean' || value.type === 'rgb') return exact(value, ['type']);
  if (value.type === 'text') return exact(value, ['type', 'maxBytes', 'encoding']) && whole(value.maxBytes, 1, 4096) && ['utf8', 'ascii'].includes(String(value.encoding));
  if (value.type === 'enum') return exact(value, ['type', 'values']) && list(value.values, 64) && value.values.length > 0 && value.values.every(item => boundedText(item, 256)) && new Set(value.values).size === value.values.length;
  return ['integer', 'number'].includes(String(value.type)) && exact(value, ['type', 'min', 'max', 'unit']) && finite(value.min) && finite(value.max) && value.min <= value.max && Math.abs(value.min) <= Number.MAX_SAFE_INTEGER && Math.abs(value.max) <= Number.MAX_SAFE_INTEGER && (value.type !== 'integer' || Number.isSafeInteger(value.min) && Number.isSafeInteger(value.max)) && (value.unit === null || boundedText(value.unit, 128));
}
export function profileScalarAccepts(schema: ProfileScalarSchema, value: unknown): boolean {
  if (schema.type === 'boolean') return typeof value === 'boolean';
  if (schema.type === 'rgb') return typeof value === 'string' && /^[0-9a-f]{6}$/i.test(value);
  if (schema.type === 'enum') return typeof value === 'string' && schema.values.includes(value);
  if (schema.type === 'text') return typeof value === 'string' && bytes(value) <= schema.maxBytes && !/[\u0000-\u001f\u007f-\u009f]/.test(value) && (schema.encoding === 'utf8' || /^[\x00-\x7f]*$/.test(value));
  return finite(value) && value >= schema.min && value <= schema.max && (schema.type !== 'integer' || Number.isSafeInteger(value));
}
export function profileContractError(value: unknown, direction: 'input' | 'output', provider = false): string {
  if (!object(value)) return 'Malformed signal contract.';
  if (value.delivery === 'state') return exact(value, ['delivery', 'schema', 'freshnessMs', 'fallback']) && profileScalarValid(value.schema) && whole(value.freshnessMs, 1, 600000) && (value.fallback === null || profileScalarAccepts(value.schema, value.fallback)) ? '' : 'Malformed state contract.';
  if (value.delivery === 'event') return exact(value, ['delivery', 'schema']) && profileScalarValid(value.schema) ? '' : 'Malformed event contract.';
  const invocation = value.delivery === 'invocation';
  if (invocation ? provider || direction !== 'output' || !exact(value, ['delivery', 'parameters', 'semantics']) : value.delivery !== 'action' || direction !== 'input' || !exact(value, ['delivery', 'parameters', 'semantics', 'endpoint', 'readable', 'display', 'ownership'])) return 'Malformed action or invocation contract.';
  if (!['setter', 'one-shot'].includes(String(value.semantics)) || !list(value.parameters, 4) || !value.parameters.every(p => exact(p, ['id', 'name', 'schema']) && boundedText(p.id, 80) && boundedText(p.name, 320) && profileScalarValid(p.schema)) || new Set(value.parameters.map(p => (p as ProfileActionParameter).id)).size !== value.parameters.length) return 'Malformed ordered action arguments.';
  if (invocation) return '';
  if (!boundedText(value.endpoint, 256) || typeof value.readable !== 'boolean') return 'Malformed action endpoint.';
  if (value.display !== null) {
    if (!exact(value.display, ['columns', 'rows', 'encoding']) || !whole(value.display.columns, 1, 63) || !whole(value.display.rows, 1, 4) || value.display.columns * value.display.rows > 63 || !['utf8', 'ascii'].includes(String(value.display.encoding))) return 'Malformed display constraints.';
    const schema = (value.parameters as ProfileActionParameter[])[0]?.schema;
    if (value.semantics !== 'setter' || !value.readable || value.parameters.length !== 1 || schema?.type !== 'text' || schema.maxBytes < value.display.columns * value.display.rows || schema.encoding !== value.display.encoding) return 'Display does not match its action argument.';
  }
  if (value.ownership !== null && (!exact(value.ownership, ['scope', 'maxLeaseMs']) || value.ownership.scope !== 'endpoint' || !whole(value.ownership.maxLeaseMs, 1, 60000))) return 'Malformed ownership capability.';
  return '';
}
function port(value: unknown): boolean {
  return exact(value, ['id', 'name', 'direction', 'contract']) && profilePortIdValid(value.id) && text(value.name) && (value.direction === 'input' || value.direction === 'output') && profileContractError(value.contract, value.direction) === '';
}
function ports(value: unknown): value is ProfilePort[] {
  return list(value, 128) && value.every(port) && new Set(value.map(item => (item as ProfilePort).id)).size === value.length;
}
const names = (value: unknown, max = 128): value is string[] => list(value, max) && value.every(profilePortIdValid) && new Set(value).size === value.length;
const rounding = (value: unknown) => ['none', 'nearest', 'floor', 'ceil', 'truncate'].includes(String(value));
const comparison = (value: unknown) => ['greater', 'greater-equal', 'less', 'less-equal', 'equal', 'not-equal'].includes(String(value));
const coefficient = (value: unknown) => finite(value) && Math.abs(value) <= Number.MAX_SAFE_INTEGER;
function builtin(value: unknown): boolean {
  if (!object(value)) return false;
  if (['merge', 'chord', 'gate', 'toggle'].includes(String(value.kind))) return exact(value, ['kind']);
  if (value.kind === 'axis') return exact(value, ['kind', 'deadZone', 'invert', 'scale', 'curve']) && whole(value.deadZone, 0, 32766) && typeof value.invert === 'boolean' && whole(value.scale, 0, 4000) && list(value.curve, 32) && value.curve.every(p => exact(p, ['input', 'output']) && whole(p.input, -32768, 32767) && whole(p.output, -32768, 32767));
  if (value.kind === 'split') return exact(value, ['kind', 'positive']) && typeof value.positive === 'boolean';
  if (value.kind === 'button-axis') return exact(value, ['kind', 'range', 'pressed', 'released']) && ['bipolar', 'unipolar'].includes(String(value.range)) && whole(value.pressed, -32768, 32767) && whole(value.released, -32768, 32767);
  if (value.kind === 'threshold') return exact(value, ['kind', 'press', 'release']) && whole(value.press, -32768, 32767) && whole(value.release, -32768, 32767);
  if (value.kind === 'gesture') return exact(value, ['kind', 'gesture', 'holdUs', 'doubleUs', 'pulseUs']) && ['tap', 'hold', 'double-tap'].includes(String(value.gesture)) && whole(value.holdUs, 1000, 60000000) && whole(value.doubleUs, 1000, 60000000) && whole(value.pulseUs, 1000, 60000000);
  if (value.kind === 'pulse') return exact(value, ['kind', 'durationUs', 'repeatUs']) && whole(value.durationUs, 1000, 60000000) && whole(value.repeatUs, 0, 60000000);
  return value.kind === 'macro' && exact(value, ['kind', 'steps']) && list(value.steps, 16) && value.steps.every(p => exact(p, ['afterUs', 'pressed']) && whole(p.afterUs, 0, 60000000) && typeof p.pressed === 'boolean');
}
function operation(value: unknown): boolean {
  if (!object(value)) return false;
  const kind = value.kind;
  if (['identity', 'not', 'invert', 'gate', 'any', 'all', 'event-to-state'].includes(String(kind))) return exact(value, ['kind']);
  if (kind === 'constant') return exact(value, ['kind', 'value']) && scalarValue(value.value);
  if (kind === 'scale') return exact(value, ['kind', 'factor', 'offset']) && coefficient(value.factor) && coefficient(value.offset);
  if (kind === 'range-map') return exact(value, ['kind', 'inputMin', 'inputMax', 'outputMin', 'outputMax', 'rounding']) && [value.inputMin, value.inputMax, value.outputMin, value.outputMax].every(coefficient) && rounding(value.rounding);
  if (kind === 'unit-convert') return exact(value, ['kind', 'from', 'to', 'rounding']) && boundedText(value.from, 128) && boundedText(value.to, 128) && rounding(value.rounding);
  if (kind === 'deadzone') return exact(value, ['kind', 'center', 'radius', 'rounding']) && coefficient(value.center) && coefficient(value.radius) && (value.radius as number) >= 0 && rounding(value.rounding);
  if (kind === 'curve') return exact(value, ['kind', 'points', 'rounding']) && list(value.points, 32) && value.points.length >= 2 && value.points.every(p => exact(p, ['input', 'output']) && coefficient(p.input) && coefficient(p.output)) && rounding(value.rounding);
  if (kind === 'compare') return exact(value, ['kind', 'comparison', 'threshold']) && comparison(value.comparison) && coefficient(value.threshold);
  if (kind === 'hysteresis') return exact(value, ['kind', 'comparison', 'press', 'release']) && comparison(value.comparison) && coefficient(value.press) && coefficient(value.release);
  if (kind === 'priority') return exact(value, ['kind', 'order']) && names(value.order, 8);
  if (kind === 'event-edge') return exact(value, ['kind', 'edge']) && ['rising', 'falling', 'change'].includes(String(value.edge));
  if (kind === 'select') return exact(value, ['kind', 'whenTrue', 'whenFalse']) && scalarValue(value.whenTrue) && scalarValue(value.whenFalse);
  if (kind === 'format') return exact(value, ['kind', 'decimals', 'prefix', 'suffix', 'rounding', 'overflow']) && whole(value.decimals, 0, 6) && typeof value.prefix === 'string' && typeof value.suffix === 'string' && bytes(value.prefix) <= 64 && bytes(value.suffix) <= 64 && !/[\u0000-\u001f\u007f-\u009f]/.test(value.prefix + value.suffix) && rounding(value.rounding) && ['reject', 'truncate'].includes(String(value.overflow));
  if (kind === 'fallback') return exact(value, ['kind', 'unknown', 'stale', 'unavailable']) && [value.unknown, value.stale, value.unavailable].every(scalarValue);
  if (kind === 'action-pack' || kind === 'set-on-change') return exact(value, ['kind', 'arguments']) && names(value.arguments, 4);
  if (kind === 'builtin') return exact(value, ['kind', 'rule', 'inputs']) && names(value.inputs, 128) && builtin(value.rule);
  return kind === 'script' && exact(value, ['kind', 'source', 'parameters']) && typeof value.source === 'string' && bytes(value.source) > 0 && bytes(value.source) <= 16384 && object(value.parameters) && Object.keys(value.parameters).length <= 64 && bytes(JSON.stringify(value.parameters)) <= 4096 && Object.entries(value.parameters).every(([key, value]) => boundedText(key, 80) && scalarValue(value) && (typeof value !== 'number' || coefficient(value)));
}
function layout(value: unknown): boolean {
  return exact(value, ['buttons', 'axes', 'hats']) && names(value.buttons, 128) && names(value.hats, 4) && list(value.axes, 16) && value.axes.every(axis => exact(axis, ['portId', 'range', 'fallback']) && profilePortIdValid(axis.portId) && ['bipolar', 'unipolar'].includes(String(axis.range)) && whole(axis.fallback, axis.range === 'bipolar' ? -32768 : 0, 32767))
    && new Set([...(value.buttons as string[]), ...value.axes.map(axis => (axis as {portId: string}).portId), ...(value.hats as string[])]).size === value.buttons.length + value.axes.length + value.hats.length && value.buttons.length + value.axes.length + value.hats.length > 0;
}
export function profileAssignmentsError(value: unknown): string {
  if (!list(value, 64) || !value.every(a => exact(a, ['resourceId', 'sourceId', 'ports']) && identifier(a.resourceId) && boundedText(a.sourceId, 1024) && list(a.ports, 128) && a.ports.every(p => exact(p, ['profilePortId', 'observedPortId', 'expectedContractHash']) && profilePortIdValid(p.profilePortId) && boundedText(p.observedPortId, 256) && typeof p.expectedContractHash === 'string' && /^sha256:[0-9a-f]{64}$/.test(p.expectedContractHash)) && new Set(a.ports.map(p => (p as ProfilePortExpectation).profilePortId)).size === a.ports.length && new Set(a.ports.map(p => (p as ProfilePortExpectation).observedPortId)).size === a.ports.length) || new Set(value.map(a => (a as ProfileLocalAssignment).resourceId)).size !== value.length) return 'Use exact bounded local resource and port assignments.';
  return '';
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
  if (!list(value.processingNodes, 256) || !value.processingNodes.every(row => exact(row, [...labelKeys, 'operation', 'inputs', 'outputs']) && nodeLabel(row) && operation(row.operation) && ports(row.inputs) && ports(row.outputs) && ((row.operation as ProfileOperation).kind !== 'script' || row.inputs.length >= 1 && row.inputs.length <= 32 && row.outputs.length >= 1 && row.outputs.length <= 16))) return 'Use supported processing nodes and typed ports.';
  if (!list(value.virtualOutputs, 16) || !value.virtualOutputs.every(row => exact(row, [...labelKeys, 'required', 'ports', 'layout']) && nodeLabel(row) && typeof row.required === 'boolean' && ports(row.ports) && layout(row.layout))) return 'Use valid logical virtual outputs.';
  const nodes = [...value.resources, ...value.processingNodes, ...value.virtualOutputs] as (ProfileResource | ProfileProcessingNode | ProfileVirtualOutput)[];
  if (new Set(nodes.map(row => row.id)).size !== nodes.length) return 'Graph node identifiers must be unique.';
  if (nodes.reduce((sum, row) => sum + ('ports' in row ? row.ports.length : row.inputs.length + row.outputs.length), 0) > 1024) return 'Use at most 1024 graph ports.';
  if (!list(value.bindings, 512) || !value.bindings.every(row => exact(row, ['id', 'from', 'to']) && identifier(row.id) && endpoint(row.from) && endpoint(row.to)) || new Set(value.bindings.map(row => (row as ProfileBinding).id)).size !== value.bindings.length) return 'Use unique bindings with logical graph endpoints.';
  if (!list(value.visibility, 64) || !value.visibility.every(row => exact(row, ['resourceId', 'mode']) && identifier(row.resourceId) && ['visible', 'hidden-while-active'].includes(String(row.mode)))) return 'Use logical visibility policies.';
  if (!list(value.overrides, 64) || !value.overrides.every(row => exact(row, ['resourceId', 'enabled']) && identifier(row.resourceId) && typeof row.enabled === 'boolean')) return 'Use bounded logical runtime overrides.';
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
  const error = profileDocumentError(document);
  if (error) throw new Error(error);
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
    const error = profileDocumentError(document);
    if (error) throw new Error(error);
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
  async getAssignments(id: string): Promise<ProfileResult> {
    const result = await this.request({ command: 'profile_assignments_get', id });
    if (profileAssignmentsError(result.assignments)) throw new Error('Malformed local assignment response.');
    documentResult(result);
    return result;
  }
  async saveAssignments(id: string, expectedRevision: number, assignments: ProfileLocalAssignment[]): Promise<ProfileResult> {
    const error = profileAssignmentsError(assignments);
    if (error || !whole(expectedRevision, 1, Number.MAX_SAFE_INTEGER)) throw new Error(error || 'Choose a safe expected revision.');
    const result = await this.request({ command: 'profile_assignments_save', id, expectedRevision, assignments });
    if (profileAssignmentsError(result.assignments)) throw new Error('Malformed local assignment response.');
    documentResult(result);
    return result;
  }
  async select(id: string, selectionSource: ProfileSelectionMode = 'manual'): Promise<ProfileResult> { return this.request({ command: 'profile_select', id, selectionSource }); }
  async pause(paused = true): Promise<ProfileResult> { return this.request({ command: 'profile_pause', paused }); }
  async selectionMode(mode: ProfileSelectionMode): Promise<ProfileResult> { return this.request({ command: 'profile_selection_mode', mode }); }
  async reset(): Promise<ProfileResult> {
    const result = await this.request({ command: 'profile_reset', confirm: true });
    this.close();
    return result;
  }
}
