import type { ControllerControl } from './controllers';
export type MappingCommand = 'mapping_learn_start' | 'mapping_profiles' | 'mapping_export' | 'mapping_import_begin' | 'mapping_import_chunk' | 'mapping_import_commit' | 'mapping_import_abort' | 'mapping_validate_commit' | 'mapping_status' | 'mapping_preview_start' | 'mapping_stop' | 'mapping_recording' | 'mapping_simulate' | 'mapping_assignments_save' | 'mapping_assignments_get' | 'mapping_virtual_inspect' | 'mapping_virtual_start' | 'mapping_virtual_create' | 'mapping_virtual_recover' | 'mapping_virtual_acknowledge' | 'mapping_provision_cancel' | 'mapping_provision_recover' | 'mapping_virtual_enable' | 'mapping_targets_save' | 'mapping_targets_get';
export type MappingKind = { kind: 'button' } | { kind: 'hat' } | { kind: 'axis'; range: 'bipolar' | 'unipolar' };
export type MappingValue = { kind: 'button'; value: boolean } | { kind: 'hat'; value: number | null } | { kind: 'axis'; value: number };
export type MappingSource = { origin: 'host-controller' | 'synthetic' | 'osg-panel-events'; id: string };
export type MappingAssignment = { role: string; source: MappingSource; controls: { control: string; sourceControl: string }[] };
export type MappingToken = { session: string; generation: string };
export type MappingProfile = { schema: 1; id: string; name: string; roles: { id: string; name: string; origin: MappingSource['origin']; controls: { id: string; kind: MappingKind }[] }[]; targets: { id: string; name: string; layout: { buttons: number; axes: { range: 'bipolar' | 'unipolar'; fallback: number }[]; hats: number } }[]; nodes: unknown[]; arbitration: unknown[]; requirements: { virtualDevices: boolean; deviceHiding: boolean; visibility: unknown[] } };
export type VirtualBinding = { target: string; slot: number; axes: number[]; layout: MappingProfile['targets'][number]['layout'] };
export type RecordedMapping = { cursor: string; report: { target: string; sequence: string; atUs: string; reason: string; state: { buttons: boolean[]; axes: number[]; hats: (number | null)[] } } };
export type VirtualSlot = { id: number; status: string; ownerPid: number | null; buttons: number; axes: { usage: number; min: number; max: number }[]; continuousHats: number; discreteHats: number; forceFeedback: boolean };
export type ProvisioningStatus = { jobId: string; operation: string; active: boolean; state: string; message: string };
export type MappingResult = { learning?: {sourceId: string; phase: string; reason: string; candidates: string[]} | null; inspector?: {nodes: {id: string; inputs: {input: unknown; value: MappingValue | null; raw?: {native: number; normalized: number | null}}[]; operation: unknown; output: unknown; value: MappingValue | null; contribution: MappingValue | null; nextUs: string | null; writers: string[] | null}[]; offset: number; total: number; atUs: string; outputs: Record<string, RecordedMapping["report"]["state"]>} | null; backend?: string; limits?: {axes: number; buttons: number; hats: number; targets: number}; provisioningQualified?: boolean; provisioning?: ProvisioningStatus | null; provisioningRecovery?: { pending: { operation: string; target: number; refreshUncertain: boolean; written: number[] }[]; error?: string }; recoveryRequired?: boolean; outputRecovery?: { required: boolean; phase?: 'intent' | 'confirmed' | 'releasing' | 'clean'; token?: MappingToken; slots?: number[]; error?: string }; bindings?: VirtualBinding[] | null; storageError?: string; document?: MappingProfile; ids?: string[]; next?: number | string | null; uploadId?: string; offset?: number; total?: number; controls?: MappingAssignment['controls']; source?: MappingSource; roles?: { role: string; source: MappingSource; controlCount: number }[]; running?: boolean; ready?: boolean; token?: MappingToken | null; profileId?: string; fault?: string | null; recordingCursor?: string; records?: RecordedMapping[]; gap?: boolean; dropped?: string; slots?: VirtualSlot[]; reason?: string; state?: string; virtualDevices?: { backend?: string; enabled: boolean; qualified: boolean; state: string; reason: string } };
export type MappingRequest = {sourceId?: string; inspect?: boolean; layout?: { axes: number; buttons: number; hats: number }; slot?: number; jobId?: string; operation?: string; enabled?: boolean; bindings?: VirtualBinding[]; command: MappingCommand; profileId?: string; offset?: number; role?: string; after?: string; uploadId?: string; size?: number; bytes?: number[]; assignments?: MappingAssignment[]; source?: MappingSource; token?: MappingToken; changes?: { control: string; value: MappingValue }[] };
export type MappingSend = (request: MappingRequest) => Promise<MappingResult>;
export function capturedKind(control: ControllerControl): MappingKind | null {
  const c = control.capability;
  if (c.kind === 'button' || c.kind === 'hat') return { kind: c.kind };
  return c.kind === 'axis' ? { kind: 'axis', range: c.min < 0 ? 'bipolar' : 'unipolar' } : null;
}
export function compatibleControl(kind: MappingKind, control: ControllerControl): boolean {
  const native = capturedKind(control);
  return native?.kind === kind.kind && (native.kind !== 'axis' || (kind.kind === 'axis' && native.range === kind.range));
}
export function directProfile(name: string, kind: MappingKind, source: MappingSource, sourceControl: string): { profile: MappingProfile; assignments: MappingAssignment[] } {
  if (!name.trim() || name.trim().length > 80 || !source.id || !sourceControl || source.origin === 'osg-panel-events') throw new Error('Choose a name, source and supported control.');
  const profile: MappingProfile = { schema: 1, id: crypto.randomUUID(), name: name.trim(), roles: [{ id: 'input', name: 'Input controller', origin: source.origin, controls: [{ id: 'control', kind }] }], targets: [{ id: 'output', name: 'Mapped controller', layout: { buttons: kind.kind === 'button' ? 1 : 0, axes: kind.kind === 'axis' ? [{ range: kind.range, fallback: 0 }] : [], hats: kind.kind === 'hat' ? 1 : 0 } }], nodes: [{ id: 'direct', inputs: [{ kind: 'source', role: 'input', control: 'control' }], output: { target: 'output', control: { kind: kind.kind, index: 0 } }, operation: { kind: 'direct', activation: kind.kind === 'button' ? 'momentary' : 'level' } }], arbitration: [], requirements: { virtualDevices: false, deviceHiding: false, visibility: [] } };
  return { profile, assignments: [{ role: 'input', source, controls: [{ control: 'control', sourceControl }] }] };
}
export async function importMapping(text: string, send: MappingSend, validateOnly = false): Promise<MappingProfile> {
  const bytes = new TextEncoder().encode(text);
  if (!bytes.length || bytes.length > 65536) throw new Error('Mapping files must contain 1–65536 UTF-8 bytes.');
  const start = await send({ command: 'mapping_import_begin', size: bytes.length });
  if (!start.uploadId) throw new Error('The mapping upload did not start.');
  try {
    for (let offset = 0; offset < bytes.length; offset += 8192) await send({ command: 'mapping_import_chunk', uploadId: start.uploadId, offset, bytes: Array.from(bytes.subarray(offset, offset + 8192)) });
    const result = await send({ command: validateOnly ? 'mapping_validate_commit' : 'mapping_import_commit', uploadId: start.uploadId });
    if (!result.document) throw new Error('The imported mapping document is missing.');
    return result.document;
  } catch (error) {
    await send({ command: 'mapping_import_abort', uploadId: start.uploadId }).catch(() => {});
    throw error;
  }
}
export async function savedAssignments(profileId: string, send: MappingSend): Promise<MappingAssignment[]> {
  const result = await send({ command: 'mapping_assignments_get', profileId });
  const assignments: MappingAssignment[] = [];
  if (!result.roles || result.roles.length > 16) throw new Error('Invalid saved mapping assignments.');
  for (const role of result.roles) {
    if (role.controlCount < 1 || role.controlCount > 256) throw new Error('Invalid assigned control count.');
    const controls: MappingAssignment['controls'] = [];
    for (let offset = 0; offset < role.controlCount; offset += 32) {
      const page = await send({ command: 'mapping_assignments_get', profileId, role: role.role, offset });
      if (!page.controls || page.controls.length !== Math.min(32, role.controlCount - offset)) throw new Error('Incomplete saved control assignments.');
      controls.push(...page.controls);
    }
    assignments.push({ role: role.role, source: role.source, controls });
  }
  return assignments;
}

export type RuleInput = {kind: 'source'; role: string; control: string} | {kind: 'node'; node: string};
export type RuleNode = {id: string; inputs: RuleInput[]; output: {target: string; control: {kind: 'button' | 'axis' | 'hat'; index: number}} | null; operation: {kind: 'direct'; activation: 'momentary' | 'level'} | {kind: 'transform'; rule: Record<string, unknown> & {kind: string}}};
export const builtins = ['axis','merge','split','button-axis','threshold','chord','gate','toggle','tap','hold','double-tap','pulse','macro'] as const;
export function operationPreset(kind: string, axisRange: 'bipolar' | 'unipolar' = 'bipolar'): RuleNode['operation'] {
  const defaults: Record<string, Record<string, unknown>> = {
    axis: {deadZone: 0, invert: false, scale: 1000, curve: []}, merge: {}, split: {positive: true},
    'button-axis': {range: axisRange, pressed: 32767, released: 0}, threshold: {press: 16000, release: 14000}, chord: {}, gate: {}, toggle: {},
    tap: {kind: 'gesture', gesture: 'tap', holdUs: 300000, doubleUs: 250000, pulseUs: 50000},
    hold: {kind: 'gesture', gesture: 'hold', holdUs: 300000, doubleUs: 250000, pulseUs: 50000},
    'double-tap': {kind: 'gesture', gesture: 'double-tap', holdUs: 300000, doubleUs: 250000, pulseUs: 50000},
    pulse: {durationUs: 50000, repeatUs: 0}, macro: {steps: [{afterUs: 0, pressed: true},{afterUs: 100000, pressed: false}]}
  };
  if (kind === 'direct') return {kind: 'direct', activation: 'momentary'};
  if (!defaults[kind]) throw new Error('Unknown transformation.');
  return {kind: 'transform', rule: {kind, ...structuredClone(defaults[kind])}};
}
export function oneToOneProfile(name: string, source: MappingSource, controls: ControllerControl[]) {
  const usable = controls.filter(c => capturedKind(c));
  if (!usable.length || usable.length > 256) throw new Error('Choose a controller with 1–256 supported controls.');
  const first = directProfile(name, capturedKind(usable[0])!, source, usable[0].capability.id);
  const layout: MappingProfile['targets'][number]['layout'] = {buttons: 0, axes: [], hats: 0};
  first.profile.roles[0].controls = [];
  first.profile.nodes = [];
  first.assignments[0].controls = [];
  usable.forEach((control, i) => {
    const kind = capturedKind(control)!, id = 'control-' + i;
    const index = kind.kind === 'button' ? layout.buttons++ : kind.kind === 'hat' ? layout.hats++ : layout.axes.push({range: kind.range, fallback: 0}) - 1;
    first.profile.roles[0].controls.push({id,kind});
    first.assignments[0].controls.push({control:id,sourceControl:control.capability.id});
    first.profile.nodes.push({id:'direct-'+i,inputs:[{kind:'source',role:'input',control:id}],output:{target:'output',control:{kind:kind.kind,index}},operation:{kind:'direct',activation:kind.kind==='button'?'momentary':'level'}});
  });
  if (layout.buttons > 128 || layout.axes.length > 16 || layout.hats > 4) throw new Error('This controller exceeds one logical target. Split it into explicit layouts.');
  first.profile.targets[0].layout = layout;
  return first;
}
export function draftDocument(text: string, expectedId: string): MappingProfile {
  if (new TextEncoder().encode(text).length > 65536) throw new Error('Draft exceeds 65536 UTF-8 bytes.');
  const draft = JSON.parse(text) as MappingProfile;
  if (draft.id !== expectedId || draft.schema !== 1 || !Array.isArray(draft.nodes) || draft.nodes.length > 256) throw new Error('Keep the selected draft identity and supported schema.');
  return draft;
}
