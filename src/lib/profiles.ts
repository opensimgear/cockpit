/** Portable unified profile contracts. Native validation remains authoritative. */
import { desktopActionValid, desktopActionContract, type ProfileDesktopOutput } from './profile-desktop.ts';
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
      ownership: { scope: 'endpoint'; maxLeaseMs: number; restorePolicy?: 'if-unchanged'; fallbackPolicy?: 'capture-at-claim' } | null };
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
  | {kind:'hat-direction';direction:'up'|'right'|'down'|'left';diagonals:boolean}
  | { kind: 'declare-quantity'; quantity: string; unit: string }
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
export type ProfileEditorGroup = { id: string; name: string; members: string[] };
export type ProfilePortGroup = 'outputs' | 'inputs' | 'actions';
export type ProfileDocument = {
  schemaVersion: 2; id: string; name: string; revision: number; eligibility: { gameIds: string[] };
  resources: ProfileResource[]; bindings: ProfileBinding[]; processingNodes: ProfileProcessingNode[]; desktopOutputs?: ProfileDesktopOutput[];
  virtualOutputs: ProfileVirtualOutput[]; visibility: { resourceId: string; mode: 'visible' | 'hidden-while-active' }[];
  overrides: { resourceId: string; enabled: boolean }[];
  companionAssignments: { resourceId: string; publicationHash: string }[];
  editor: { positions: { nodeId: string; x: number; y: number }[]; viewport: { x: number; y: number; zoom: number }; groups?: ProfileEditorGroup[]; collapsedPortGroups?: { nodeId: string; group: ProfilePortGroup }[] };
};
export type ProfilePortExpectation = { profilePortId: string; observedPortId: string; expectedContractHash: string };
export type ProfileLocalAssignment = { resourceId: string; sourceId: string; ports: ProfilePortExpectation[] };
export type ProfileSelectionMode = 'manual' | 'automatic';
export type AppliedProfileSnapshot = { profileId: string; revision: number; semanticHash: string };
export type ProfileSession = {
  selectedProfileId: string; selectionMode: ProfileSelectionMode; state: 'stopped' | 'running' | 'degraded' | 'paused' | 'faulted' | 'recovery-needed' | 'transitioning' | 'awaiting-game';
  appliedSnapshot: AppliedProfileSnapshot | null; fault: string | null;
};
export type ProfileSummary = { id: string; name: string; revision: number; isDefault: boolean; semanticHash: string };
export type ProfilePublicationReferences = { schemaVersion: 1; libraryFingerprint: string; hashes: string[] };
export type ProfileCommand = 'profile_companion_references' | 'profile_list' | 'profile_get' | 'profile_create' | 'profile_rename' | 'profile_duplicate' | 'profile_delete' | 'profile_save' | 'profile_import' | 'profile_export' | 'profile_status' | 'profile_select' | 'profile_apply' | 'profile_stop' | 'profile_recover' | 'profile_pause' | 'profile_selection_mode' | 'profile_reset' | 'profile_assignments_get' | 'profile_assignments_save' | 'profile_editor_save';
export type ProfileRequest =
  | { command: 'profile_list' | 'profile_status' | 'profile_companion_references' }
  | { command: 'profile_get' | 'profile_export' | 'profile_assignments_get'; id: string }
  | { command: 'profile_assignments_save'; id: string; expectedRevision: number; assignments: ProfileLocalAssignment[] }
  | { command: 'profile_create'; name: string }
  | { command: 'profile_rename'; id: string; name: string; expectedRevision: number }
  | { command: 'profile_duplicate'; id: string; name?: string }
  | { command: 'profile_delete'; id: string; expectedRevision: number }
  | { command: 'profile_save'; document: ProfileDocument; expectedRevision: number }
  | { command: 'profile_editor_save'; document: ProfileDocument; expectedRevision: number | null; assignments: ProfileLocalAssignment[] }
  | { command: 'profile_import'; document: ProfileDocument; name?: string }
  | { command: 'profile_select'; id: string; selectionSource: ProfileSelectionMode }
  | { command: 'profile_apply'; id: string; expectedRevision: number; gameId?: 'dcs' | 'x-plane-11' | 'x-plane-12' | 'assetto-corsa-rally' }
  | { command: 'profile_stop' | 'profile_recover' }
  | { command: 'profile_pause'; paused: boolean }
  | { command: 'profile_selection_mode'; mode: ProfileSelectionMode }
  | { command: 'profile_reset'; confirm: true };
export type ProfileResult = { publicationReferences?: ProfilePublicationReferences; document?: ProfileDocument; profiles?: ProfileSummary[]; session?: ProfileSession; profileSession?: import('./profile-runtime').NativeProfileStatus; automatic?: import('./profile-automatic').ProfileAutomaticStatus; deletedId?: string; diagnostics?: string[]; readOnly?: boolean; assignments?: ProfileLocalAssignment[] };
export type ProfileEditorSaveResult = { document: ProfileDocument; assignments: ProfileLocalAssignment[]; profiles: ProfileSummary[]; session: ProfileSession };
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
  if (value.ownership !== null && (!object(value.ownership) || !exact(value.ownership, ['scope','maxLeaseMs',...(Object.hasOwn(value.ownership,'restorePolicy')?['restorePolicy']:[]),...(Object.hasOwn(value.ownership,'fallbackPolicy')?['fallbackPolicy']:[])]) || value.ownership.scope !== 'endpoint' || !whole(value.ownership.maxLeaseMs, 1, 60000) || Object.hasOwn(value.ownership,'restorePolicy') && value.ownership.restorePolicy !== 'if-unchanged' || Object.hasOwn(value.ownership,'fallbackPolicy') && (value.ownership.fallbackPolicy !== 'capture-at-claim' || value.ownership.restorePolicy !== 'if-unchanged'))) return 'Malformed ownership capability.';
  return '';
}
function port(value: unknown): boolean {
  return exact(value, ['id', 'name', 'direction', 'contract']) && profilePortIdValid(value.id) && text(value.name) && (value.direction === 'input' || value.direction === 'output') && profileContractError(value.contract, value.direction) === '';
}
function ports(value: unknown): value is ProfilePort[] {
  return list(value, 128) && value.every(port) && new Set(value.map(item => (item as ProfilePort).id)).size === value.length;
}
function desktopOutput(value:unknown):value is ProfileDesktopOutput {
  if(!exact(value,[...labelKeys,'required','ports','actions'])||!nodeLabel(value)||typeof value.required!=='boolean'||!ports(value.ports)||value.ports.length===0||!list(value.actions,128)||value.actions.length!==value.ports.length)return false;
  const declared=value.ports;
  for(const definition of value.actions){
    if(!exact(definition,['portId','action'])||!profilePortIdValid(definition.portId)||!desktopActionValid(definition.action))return false;
    const port=declared.find(port=>port.id===definition.portId),expected=desktopActionContract(definition.action);
    if(!port||port.direction!=='input'||port.contract.delivery!==expected.delivery||!('schema'in port.contract)||!('schema'in expected))return false;
    const a=port.contract.schema,b=expected.schema;
    if(a.type!==b.type||b.type==='number'&&(a.type!=='number'||a.min!==b.min||a.max!==b.max||a.unit!==b.unit))return false;
    if(expected.delivery==='state'&&(port.contract.delivery!=='state'||port.contract.freshnessMs!==expected.freshnessMs||port.contract.fallback!==expected.fallback))return false;
  }
  return new Set(value.actions.map(definition=>(definition as {portId:string}).portId)).size===value.actions.length;
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
  if (kind === 'declare-quantity') return exact(value, ['kind', 'quantity', 'unit']) && boundedText(value.quantity, 128) && value.quantity.trim() === value.quantity && boundedText(value.unit, 128) && value.unit.trim() === value.unit;
  if (kind === 'constant') return exact(value, ['kind', 'value']) && scalarValue(value.value);
  if (kind === 'scale') return exact(value, ['kind', 'factor', 'offset']) && coefficient(value.factor) && coefficient(value.offset);
  if (kind === 'range-map') return exact(value, ['kind', 'inputMin', 'inputMax', 'outputMin', 'outputMax', 'rounding']) && [value.inputMin, value.inputMax, value.outputMin, value.outputMax].every(coefficient) && rounding(value.rounding);
  if (kind === 'unit-convert') return exact(value, ['kind', 'from', 'to', 'rounding']) && boundedText(value.from, 128) && boundedText(value.to, 128) && rounding(value.rounding);
  if (kind === 'deadzone') return exact(value, ['kind', 'center', 'radius', 'rounding']) && coefficient(value.center) && coefficient(value.radius) && (value.radius as number) >= 0 && rounding(value.rounding);
  if (kind === 'curve') return exact(value, ['kind', 'points', 'rounding']) && list(value.points, 32) && value.points.length >= 2 && value.points.every(p => exact(p, ['input', 'output']) && coefficient(p.input) && coefficient(p.output)) && rounding(value.rounding);
  if (kind === 'compare') return exact(value, ['kind', 'comparison', 'threshold']) && comparison(value.comparison) && coefficient(value.threshold);
  if (kind === 'hysteresis') return exact(value, ['kind', 'comparison', 'press', 'release']) && comparison(value.comparison) && coefficient(value.press) && coefficient(value.release);
  if (kind === 'priority') return exact(value, ['kind', 'order']) && names(value.order, 8);
  if (kind === 'hat-direction') return exact(value,['kind','direction','diagonals'])&&['up','right','down','left'].includes(String(value.direction))&&typeof value.diagonals==='boolean';
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
  if (!exact(value, ['schemaVersion', 'id', 'name', 'revision', 'eligibility', 'resources', 'bindings', 'processingNodes', 'virtualOutputs', 'visibility', 'overrides', 'companionAssignments', 'editor',...(object(value)&&Object.hasOwn(value,'desktopOutputs')?['desktopOutputs']:[])])) return 'Use the unified portable profile document without local assignments or legacy fields.';
  if (value.schemaVersion !== PROFILE_SCHEMA_VERSION || !uuid(value.id) || !text(value.name) || !whole(value.revision, 1, Number.MAX_SAFE_INTEGER)) return 'Choose a supported profile schema, canonical UUID, name and revision.';
  if (!exact(value.eligibility, ['gameIds']) || !list(value.eligibility.gameIds, 32) || !value.eligibility.gameIds.every(identifier) || new Set(value.eligibility.gameIds).size !== value.eligibility.gameIds.length) return 'Choose unique logical game identifiers.';
  if (!list(value.resources, 64) || !value.resources.every(row => exact(row, [...labelKeys, 'kind', 'required', 'ports']) && nodeLabel(row) && ['device', 'companion', 'game'].includes(String(row.kind)) && typeof row.required === 'boolean' && ports(row.ports))) return 'Use valid logical resources and typed ports.';
  if (!list(value.processingNodes, 256) || !value.processingNodes.every(row => exact(row, [...labelKeys, 'operation', 'inputs', 'outputs']) && nodeLabel(row) && operation(row.operation) && ports(row.inputs) && ports(row.outputs) && ((row.operation as ProfileOperation).kind !== 'script' || row.inputs.length >= 1 && row.inputs.length <= 32 && row.outputs.length >= 1 && row.outputs.length <= 16))) return 'Use supported processing nodes and typed ports.';
  if (!list(value.virtualOutputs, 16) || !value.virtualOutputs.every(row => exact(row, [...labelKeys, 'required', 'ports', 'layout']) && nodeLabel(row) && typeof row.required === 'boolean' && ports(row.ports) && layout(row.layout))) return 'Use valid logical virtual outputs.';
  const desktops=Object.hasOwn(value,'desktopOutputs')?value.desktopOutputs:[];
  if(!list(desktops,1)||!desktops.every(desktopOutput))return 'Use one desktop endpoint with one exact typed action per input port.';
  if(value.processingNodes.length+desktops.reduce<number>((sum,row)=>sum+(row as ProfileDesktopOutput).actions.length,0)>256)return 'Desktop actions and processing nodes together support at most 256 definitions.';
  const nodes = [...value.resources, ...value.processingNodes, ...value.virtualOutputs,...desktops] as (ProfileResource | ProfileProcessingNode | ProfileVirtualOutput | ProfileDesktopOutput)[];
  if (new Set(nodes.map(row => row.id)).size !== nodes.length) return 'Graph node identifiers must be unique.';
  if (nodes.reduce((sum, row) => sum + ('ports' in row ? row.ports.length : row.inputs.length + row.outputs.length), 0) > 1024) return 'Use at most 1024 graph ports.';
  if (!list(value.bindings, 512) || !value.bindings.every(row => exact(row, ['id', 'from', 'to']) && identifier(row.id) && endpoint(row.from) && endpoint(row.to)) || new Set(value.bindings.map(row => (row as ProfileBinding).id)).size !== value.bindings.length) return 'Use unique bindings with logical graph endpoints.';
  if (!list(value.visibility, 64) || !value.visibility.every(row => exact(row, ['resourceId', 'mode']) && identifier(row.resourceId) && ['visible', 'hidden-while-active'].includes(String(row.mode)))) return 'Use logical visibility policies.';
  if (!list(value.overrides, 64) || !value.overrides.every(row => exact(row, ['resourceId', 'enabled']) && identifier(row.resourceId) && typeof row.enabled === 'boolean')) return 'Use bounded logical runtime overrides.';
  if (!list(value.companionAssignments, 64) || !value.companionAssignments.every(row => exact(row, ['resourceId', 'publicationHash']) && identifier(row.resourceId) && typeof row.publicationHash === 'string' && /^sha256:[0-9a-f]{64}$/.test(row.publicationHash))) return 'Use logical companion publication assignments.';
  const nodeIds = new Set(nodes.map(row => row.id));
  if (!object(value.editor) || !Object.hasOwn(value.editor, 'positions') || !Object.hasOwn(value.editor, 'viewport') || Object.keys(value.editor).some(key => !['positions', 'viewport', 'groups', 'collapsedPortGroups'].includes(key)) || !list(value.editor.positions, 336) || !value.editor.positions.every(row => exact(row, ['nodeId', 'x', 'y']) && identifier(row.nodeId) && nodeIds.has(row.nodeId) && coordinate(row.x) && coordinate(row.y)) || new Set(value.editor.positions.map(row => (row as {nodeId:string}).nodeId)).size !== value.editor.positions.length || !exact(value.editor.viewport, ['x', 'y', 'zoom']) || !coordinate(value.editor.viewport.x) || !coordinate(value.editor.viewport.y) || !finite(value.editor.viewport.zoom) || value.editor.viewport.zoom < 0.1 || value.editor.viewport.zoom > 4) return 'Use bounded editor positions and viewport.';
  const groups = Object.hasOwn(value.editor, 'groups') ? value.editor.groups : [];
  if (!list(groups, 32) || !Array.from(groups).every(row => exact(row, ['id', 'name', 'members']) && identifier(row.id) && text(row.name) && list(row.members, 336) && row.members.length > 0 && Array.from(row.members).every(member => identifier(member) && nodeIds.has(member))) || new Set(groups.map(row => (row as ProfileEditorGroup).id)).size !== groups.length) return 'Use at most 32 named editor groups with existing block members.';
  const members = groups.flatMap(row => (row as ProfileEditorGroup).members);
  if (new Set(members).size !== members.length) return 'Each block can belong to only one editor group.';
  const collapsed = Object.hasOwn(value.editor, 'collapsedPortGroups') ? value.editor.collapsedPortGroups : [];
  if (!list(collapsed, 1008) || !Array.from(collapsed).every(row => exact(row, ['nodeId', 'group']) && identifier(row.nodeId) && nodeIds.has(row.nodeId) && typeof row.group === 'string' && ['outputs', 'inputs', 'actions'].includes(row.group)) || new Set(collapsed.map(row => { const pair=row as {nodeId:string;group:string}; return `${pair.nodeId}/${pair.group}`; })).size !== collapsed.length) return 'Use unique collapsed port categories for existing blocks.';
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
function validProfileSummaries(value: unknown): value is ProfileSummary[] {
  return list(value, MAX_PROFILES) && value.length > 0 && value.every(summary => exact(summary, ['id', 'name', 'revision', 'isDefault', 'semanticHash']) && uuid(summary.id) && text(summary.name) && whole(summary.revision, 1, Number.MAX_SAFE_INTEGER) && summary.isDefault === (summary.id === DEFAULT_PROFILE_ID) && typeof summary.semanticHash === 'string' && /^sha256:[0-9a-f]{64}$/.test(summary.semanticHash)) && new Set(value.map(summary => (summary as ProfileSummary).id)).size === value.length && value.some(summary => (summary as ProfileSummary).id === DEFAULT_PROFILE_ID);
}
/** Saved display metadata only; it cannot substitute for an applied snapshot. */
export function captureProfileSummaries(value: unknown): DeepReadonly<ProfileSummary[]> {
  if (!validProfileSummaries(value)) throw new Error('Native saved profile summaries are malformed. Refresh the profile list.');
  return freeze(clone(value));
}
/** This immutable library projection is metadata, never permission to prune. */
export function captureProfilePublicationReferences(value: unknown): DeepReadonly<ProfilePublicationReferences> {
  const digest = (value: unknown): value is string => typeof value === 'string' && /^sha256:[0-9a-f]{64}$/.test(value);
  if (!exact(value, ['schemaVersion', 'libraryFingerprint', 'hashes']) || value.schemaVersion !== 1 || !digest(value.libraryFingerprint)
    || !list(value.hashes, MAX_PROFILES * 64) || !value.hashes.every(digest) || value.hashes.some((hash, index, hashes) => index > 0 && String(hashes[index - 1]) >= String(hash)))
    throw new Error('Native profile publication references are unavailable or malformed. Preserve retained snapshots.');
  return freeze(clone(value as ProfilePublicationReferences));
}
/** Local pins are separate from portable JSON and complete for every assigned resource. */
export function profileDocumentAssignmentsError(document: ProfileDocument, assignments: unknown): string {
  const error = profileAssignmentsError(assignments);
  if (error) return error;
  for (const assignment of assignments as ProfileLocalAssignment[]) {
    const resource = document.resources.find(resource => resource.id === assignment.resourceId);
    if (!resource || resource.ports.length !== assignment.ports.length || !resource.ports.every(port => assignment.ports.some(expected => expected.profilePortId === port.id))) return 'Local assignments must pin every declared port of an existing logical resource exactly.';
  }
  return '';
}
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
  async companionReferences(): Promise<DeepReadonly<ProfilePublicationReferences>> { return captureProfilePublicationReferences((await this.request({ command: 'profile_companion_references' })).publicationReferences); }
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
  /** Commit one editor graph and local pin set without changing runtime selection. */
  async editorSave(document: ProfileDocument, expectedRevision: number | null, assignments: ProfileLocalAssignment[]): Promise<ProfileEditorSaveResult> {
    const error = profileDocumentError(document) || profileDocumentAssignmentsError(document, assignments);
    if (error) throw new Error(error);
    if (expectedRevision === null ? document.revision !== 1 || document.id === DEFAULT_PROFILE_ID : !whole(expectedRevision, 1, Number.MAX_SAFE_INTEGER) || document.revision !== expectedRevision) throw new Error('Keep the saved revision, or use a fresh profile identity at revision 1.');
    if (document.id === DEFAULT_PROFILE_ID && document.name !== 'Default') throw new Error('The built-in Default profile keeps its protected name.');
    const checked = parseProfileDocument(JSON.stringify(document));
    const result = clone(await this.send({ command: 'profile_editor_save', document: checked, expectedRevision, assignments: clone(assignments) }));
    const saved = documentResult(result);
    if (saved.id !== document.id || saved.revision !== (expectedRevision === null ? 1 : expectedRevision + 1) || profileDocumentAssignmentsError(saved, result.assignments) || !result.session || !validProfileSummaries(result.profiles)) throw new Error('Malformed atomic editor save response.');
    const summary = result.profiles.find(summary => summary.id === saved.id);
    if (!summary || summary.name !== saved.name || summary.revision !== saved.revision || summary.isDefault !== (saved.id === DEFAULT_PROFILE_ID)) throw new Error('Atomic editor save summary does not match its saved document.');
    this.currentSession = captureProfileSession(result.session);
    return { document: saved, assignments: clone(result.assignments!), profiles: clone(result.profiles), session: clone(result.session) };
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
