import type { ActionValue, DeviceAction, DeviceActions } from './device-actions';
import type { DcsInputKind, DcsTarget, PanelInputState } from './dcs-profile';
import { panelInputMetadata } from './panel-input-metadata';

export type IntegrationProvider = 'dcs-bios' | 'assetto-corsa-rally' | 'x-plane';
export type IntegrationRole = { id: string; label: string };
export type IntegrationInput = {
  id: string; role: string; kind: DcsInputKind; control: number; command: string;
  interface: 'set_state' | 'fixed_step' | 'variable_step';
  pressedValue?: number; releasedValue?: number; step?: number; deadband?: number;
};
export type IntegrationFormat = {
  kind: 'boolean' | 'text' | 'integer' | 'fixed' | 'threshold' | 'rgb';
  scale?: number; decimals?: number; prefix?: string; suffix?: string;
  align?: 'left' | 'right'; overflow?: 'truncate' | 'reject'; unknownText?: string; staleText?: string;
  threshold?: number; comparison?: 'gte' | 'lte';
  bands?: { minimum: number; color: string }[]; unknownColor?: string; staleColor?: string;
};
export type IntegrationOutput = {
  id: string; role: string; endpoint: string; actionId: string; channel: string; output?: number; format: IntegrationFormat;
};
export type IntegrationFeedbackOutput = { id: string; channel: string; output?: number };
export type IntegrationDocument = {
  schemaVersion: 1 | 2; id: string; name: string; provider: IntegrationProvider;
  context: { aircraftModule?: string }; roles: IntegrationRole[]; inputs: IntegrationInput[]; outputs: IntegrationOutput[]; feedbackOutputs?: IntegrationFeedbackOutput[];
};
export type IntegrationConnection = {
  freshnessMs: number; cataloguePath?: string; commandHost?: string; commandPort?: number;
  listenAddress?: string; listenPort?: number; multicastGroup?: string; multicastInterface?: string;
  source?: 'shared-memory' | 'synthetic' | 'rref'; processId?: number | null;
  host?: string; port?: number; updateHz?: number; version?: 11 | 12;
  processIdentity?: { pid: number; path: string; epoch: number };
};
export type IntegrationAssignment = { role: string; gatewayId: string; moduleId: string };
export type IntegrationConfig = { document: IntegrationDocument; connection: IntegrationConnection; assignments: IntegrationAssignment[] };
export type IntegrationDiagnostic = { severity: 'error' | 'warning'; code: string; bindingId?: string | null; message: string };
export type IntegrationChannel = {
  id: string; label: string; type: 'boolean' | 'integer' | 'number' | 'string'; output?: number;
  unit?: string; minimum?: number; maximum?: number; maxLength?: number; derived?: boolean;
};
export type IntegrationCommand = { id: string; label: string; interfaces: { kind: string; maximum?: number }[] };
export type IntegrationCatalogue = {
  provider?: string; revision?: number | string; offset?: number; total?: number;
  cataloguePath?: string; aircraftModule?: string;
  channels?: IntegrationChannel[]; commands?: IntegrationCommand[];
  controls?: { id: string; label: string; inputs: { interface: string; maxValue?: number }[];
    outputs: { index: number; type: 'integer' | 'string'; maxValue?: number; maxLength?: number }[] }[];
};
export type IntegrationOutputStatus = {
  id: string; status?: string; desired?: ActionValue[] | ActionValue | null; applied?: ActionValue[] | ActionValue | null;
  error?: string | null; owned?: boolean; owner?: string | null; overrideUntilMs?: number | null; overrideRemainingMs?: number | null;
};
export type IntegrationState = {
  configured?: boolean; config?: IntegrationConfig | null; revision?: number; running?: boolean;
  outputs?: IntegrationOutputStatus[]; diagnostics?: IntegrationDiagnostic[];
  validation?: { valid: boolean; diagnostics: IntegrationDiagnostic[]; revision?: number; catalogueRevision?: string | number };
  catalogue?: IntegrationCatalogue; error?: string | null;
  telemetry?: { fresh?: boolean; status?: string; ageMs?: number | null; channels?: Record<string, { value: ActionValue | null; status: string; error?: string | null }> };
};
export type IntegrationCommandName = 'integration_get_state' | 'integration_save' | 'integration_validate' | 'integration_start' | 'integration_stop' | 'integration_override' | 'integration_catalogue' | 'integration_simulate';
export type IntegrationRequest = { command: IntegrationCommandName; integrationConfig?: IntegrationConfig; bindingId?: string;
  values?: ActionValue[]; durationMs?: number; resume?: boolean; query?: string; offset?: number; limit?: number;
  telemetryFrame?: TelemetryFrame; revision?: number };
export type IntegrationTarget = DcsTarget;

const bytes = (s: string) => new TextEncoder().encode(s).length;
const text = (v: unknown, max: number, empty = false): v is string => typeof v === 'string' && (empty || !!v) && bytes(v) <= max && !/[\u0000-\u001f\u007f-\u009f]/.test(v);
const identifier = (v: unknown, max = 31): v is string => typeof v === 'string' && new RegExp(`^[a-zA-Z0-9_.-]{1,${max}}$`).test(v);
const whole = (v: unknown, min: number, max: number): v is number => typeof v === 'number' && Number.isInteger(v) && v >= min && v <= max;
const finite = (v: unknown, max: number): v is number => typeof v === 'number' && Number.isFinite(v) && Math.abs(v) <= max;
const object = (v: unknown): v is Record<string, unknown> => !!v && typeof v === 'object' && !Array.isArray(v);
const known = (v: Record<string,unknown>, keys: string[]) => Object.keys(v).every(key => keys.includes(key));
const rgb = (v: unknown) => typeof v === 'string' && /^[a-fA-F0-9]{6}$/.test(v);

export function newIntegrationConfig(provider: IntegrationProvider = 'dcs-bios'): IntegrationConfig {
  return {
    document: { schemaVersion: 1, id: 'panel-profile', name: 'Panel profile', provider,
      context: provider === 'dcs-bios' ? { aircraftModule: 'A-10C' } : {}, roles: [{ id: 'panel', label: 'Panel' }], inputs: [], outputs: [] },
    connection: provider === 'dcs-bios' ? { freshnessMs: 1500, cataloguePath: '', commandHost: '127.0.0.1', commandPort: 7778,
      listenAddress: '0.0.0.0', listenPort: 5010, multicastGroup: '239.255.50.10', multicastInterface: '127.0.0.1' } :
      provider === 'x-plane' ? { freshnessMs: 1500, source: 'rref', host: '127.0.0.1', port: 49000, listenPort: 0, updateHz: 10, version: 11 } :
      { freshnessMs: 1500, source: 'shared-memory' }, assignments: [],
  };
}

export function integrationFormatError(f: unknown): string {
  if (!object(f) || !['boolean', 'text', 'integer', 'fixed', 'threshold', 'rgb'].includes(String(f.kind))) return 'Choose a supported output format.';
  if (!known(f,['kind','scale','decimals','prefix','suffix','align','overflow','unknownText','staleText','threshold','comparison','bands','unknownColor','staleColor'])) return 'Remove unsupported output format fields before exporting.';
  if (!finite(f.scale ?? 1, 1e6) || !whole(f.decimals ?? 0, 0, 6)) return 'Use a finite scale within ±1000000 and zero to six decimal places.';
  for (const [key, max] of [['prefix',16],['suffix',16],['unknownText',63],['staleText',63]] as const) if (f[key] !== undefined && !text(f[key], max, true)) return 'Keep output text within its UTF-8 byte limit and remove control characters.';
  if (f.align !== undefined && !['left','right'].includes(String(f.align)) || f.overflow !== undefined && !['truncate','reject'].includes(String(f.overflow))) return 'Choose left/right alignment and truncate/reject overflow.';
  if ((f.kind === 'threshold' || f.threshold !== undefined || f.comparison !== undefined) && !finite(f.threshold, 1e15) || f.comparison !== undefined && !['gte','lte'].includes(String(f.comparison))) return 'Set a finite threshold and supported comparison.';
  if (f.kind === 'rgb' || f.bands !== undefined) {
    if (!Array.isArray(f.bands) || !f.bands.length || f.bands.length > 8) return 'Add one to eight RGB bands.';
    let previous = -Infinity;
    for (const band of f.bands) {
      if (!object(band) || !known(band,['minimum','color']) || !finite(band.minimum, 1e15) || band.minimum <= previous || !rgb(band.color)) return 'RGB bands need increasing finite minimum values and six-digit hex colors.';
      previous = band.minimum;
    }
  }
  if (f.unknownColor !== undefined && !rgb(f.unknownColor) || f.staleColor !== undefined && !rgb(f.staleColor)) return 'Fallback colors use six hex digits.';
  return '';
}

/** Cheap document checks also protect file imports; native validation is authoritative. */
export function integrationError(value: unknown, portableOnly = false): string {
  if (!object(value) || bytes(JSON.stringify(value)) > 65536 || !object(value.document)) return 'Use a JSON profile no larger than 64 KiB.';
  const d = value.document;
  if ((d.schemaVersion !== 1 && d.schemaVersion !== 2) || !identifier(d.id) || !text(d.name,63) || !['dcs-bios','assetto-corsa-rally','x-plane'].includes(String(d.provider))) return 'Choose a supported provider, profile identifier, name and schema version.';
  if (Object.keys(d).some(key => !['schemaVersion','id','name','provider','context','roles','inputs','outputs',...(d.schemaVersion===2?['feedbackOutputs']:[])].includes(key))) return 'Portable profiles must not contain local paths, network settings or device assignments.';
  if (!object(d.context) || !known(d.context,['aircraftModule']) || d.provider === 'dcs-bios' && !text(d.context.aircraftModule,64) || d.context.aircraftModule !== undefined && !text(d.context.aircraftModule,64)) return 'Choose the installed aircraft module by name.';
  const feedback = d.schemaVersion === 2 ? d.feedbackOutputs ?? [] : [];
  if (!Array.isArray(feedback) || feedback.length > 16) return 'Use up to 16 logical feedback outputs.';
  const feedbackOnly = feedback.length > 0 && Array.isArray(d.inputs) && !d.inputs.length && Array.isArray(d.outputs) && !d.outputs.length;
  if (!Array.isArray(d.roles) || !d.roles.length && !feedbackOnly || d.roles.length > 8 || !Array.isArray(d.inputs) || d.inputs.length > 16 || !Array.isArray(d.outputs) || d.outputs.length + feedback.length > 16 || !d.inputs.length && !d.outputs.length && !feedback.length) return 'Add one to eight roles and up to 16 inputs and 16 outputs.';

  if (d.provider !== 'dcs-bios' && d.inputs.length) return 'This provider currently provides telemetry outputs only.';
  const roles = new Set<string>(), ids = new Set<string>(), physical = new Set<string>(), commands = new Set<string>(), endpoints = new Set<string>();
  for (const role of d.roles) {
    if (!object(role) || !known(role,['id','label']) || !identifier(role.id) || !text(role.label,63) || roles.has(role.id)) return 'Each device role needs a unique identifier and label.';
    roles.add(role.id);
  }
  for (const row of [...d.inputs, ...d.outputs]) {
    if (!object(row) || !identifier(row.id) || ids.has(row.id) || !identifier(row.role) || !roles.has(row.role)) return 'Each binding needs a unique identifier and an existing device role.';
    ids.add(row.id);
  }
  for (const row of d.inputs) {
    if (!known(row,['id','role','kind','control','command','interface','pressedValue','releasedValue','step','deadband'])) return 'Remove local or unsupported fields from portable input bindings.';
    if (!object(row) || !['button','switch','encoder','axis'].includes(String(row.kind)) || !whole(row.control,0,row.kind === 'axis' || row.kind === 'encoder' ? 7 : 127) || !identifier(row.command,64)) return 'Choose a named command and advertised physical input.';
    if (row.kind === 'encoder' ? !['fixed_step','variable_step'].includes(String(row.interface)) : row.interface !== 'set_state') return 'Choose a catalogue interface supported by this input.';
    const key = `${row.role}/${row.kind}/${row.control}`;
    if (physical.has(key) || commands.has(row.command)) return 'Each physical input and named command can be bound once.';
    physical.add(key); commands.add(row.command);
    if (![row.pressedValue ?? 1,row.releasedValue ?? 0,row.deadband ?? 128].every(v => whole(v,0,65535)) || !whole(row.step ?? 1,1,65535)) return 'Use bounded input values, axis deadband and positive encoder step.';
  }
  for (const row of d.outputs) {
    if (!known(row,['id','role','endpoint','actionId','channel','output','format'])) return 'Remove local or unsupported fields from portable output bindings.';
    if (!object(row) || !identifier(row.channel,64) || !identifier(row.actionId) || !identifier(row.endpoint) || !whole(row.output ?? 0,0,63)) return 'Choose a named telemetry channel and advertised endpoint/action.';
    const key = `${row.role}/${row.endpoint}`;
    if (endpoints.has(key)) return 'Each output endpoint can be bound once per role.';
    endpoints.add(key);
    const error = integrationFormatError(row.format); if (error) return error;
  }
  for (const row of feedback) {
    if (!object(row) || !known(row,['id','channel','output']) || !identifier(row.id) || ids.has(row.id) || !identifier(row.channel,64) || !whole(row.output ?? 0,0,15)) return 'Logical feedback outputs need unique IDs, named telemetry channels and bounded output indices.';
    ids.add(row.id);
  }
  if (portableOnly) return '';
  if (!object(value.connection) || !whole(value.connection.freshnessMs,100,10000)) return 'Feedback freshness must be from 100 to 10000 ms.';
  const c = value.connection;
  if (d.provider === 'dcs-bios') {
    if (!text(c.cataloguePath,4096)) return 'Choose the installed DCS-BIOS doc/json directory.';
    if (![c.commandHost,c.listenAddress,c.multicastInterface].every(v => text(v,253) && !/\s/.test(v)) || !text(c.multicastGroup,253,true) || /\s/.test(c.multicastGroup)) return 'Enter UDP addresses without spaces; multicast may be blank.';
    if (!whole(c.commandPort,1,65535) || !whole(c.listenPort,1,65535)) return 'UDP ports must be from 1 to 65535.';
  } else if (d.provider === 'x-plane') {
    if (c.source !== 'rref' || ![11,12].includes(Number(c.version)) || !/^127\.(?:\d{1,3}\.){2}\d{1,3}$/.test(String(c.host)) || String(c.host).split('.').some(n => Number(n)>255)) return 'Choose X-Plane 11/12 RREF and a local IPv4 loopback host.';
    if (!whole(c.port,1,65535) || !whole(c.listenPort,0,65535) || !whole(c.updateHz,1,20) || Number(c.freshnessMs)<2000/Number(c.updateHz)) return 'Choose valid UDP ports, 1–20 updates per second and freshness allowing two updates.';
  } else if (c.source !== 'synthetic' && (c.source !== 'shared-memory' || c.processId != null && !whole(c.processId,1,4294967295))) return 'Choose synthetic telemetry or the Windows game process ID.';
  if (!Array.isArray(value.assignments) || value.assignments.length > 8) return 'Assign up to eight logical roles.';
  const assigned = new Map<string,string>(), gateways = new Set<string>();
  for (const a of value.assignments) {
    if (!object(a) || !identifier(a.role) || !roles.has(a.role) || assigned.has(a.role) || !text(a.gatewayId,78) || !/^[a-zA-Z0-9_.-]+:[a-zA-Z0-9_.-]+$/.test(a.gatewayId) || typeof a.moduleId !== 'string' || !/^[0-9a-f]{32}$/i.test(a.moduleId) || /^0{32}$|^f{32}$/i.test(a.moduleId)) return 'Choose an exact gateway/module target once for each role.';
    assigned.set(a.role, `${a.gatewayId}/${a.moduleId.toLowerCase()}`); gateways.add(a.gatewayId);
  }
  if (gateways.size > 4) return 'One active profile supports up to four gateways.';
  physical.clear(); endpoints.clear();
  for (const row of [...d.inputs, ...d.outputs]) {
    const target = assigned.get(row.role); if (!target) continue;
    const isInput = 'command' in row, key = `${target}/${isInput ? `${row.kind}/${row.control}` : row.endpoint}`;
    const seen = isInput ? physical : endpoints;
    if (seen.has(key)) return 'Different roles cannot bind the same physical input or output endpoint twice.';
    seen.add(key);
  }
  return '';
}

function sorted(value: unknown): unknown {
  if (Array.isArray(value)) return value.map(sorted);
  if (object(value)) return Object.fromEntries(Object.entries(value).filter(([,v]) => v !== undefined).sort(([a],[b]) => a.localeCompare(b)).map(([k,v]) => [k,sorted(v)]));
  return value;
}
export const integrationFingerprint = (config: IntegrationConfig) => JSON.stringify(sorted(config));
export const integrationEqual = (a: IntegrationConfig, b?: IntegrationConfig | null) => !!b && integrationFingerprint(a) === integrationFingerprint(b);
export function exportIntegrationDocument(config: IntegrationConfig): string {
  const error = integrationError(config,true); if (error) throw new Error(error);
  return `${JSON.stringify(sorted(config.document),null,2)}\n`;
}
export function importIntegrationDocument(source: string, current?: IntegrationConfig | null): { config?: IntegrationConfig; error?: string } {
  if (bytes(source) > 65536) return { error: 'Use a profile file no larger than 64 KiB.' };
  try {
    const document = JSON.parse(source);
    const draft = newIntegrationConfig(document?.provider);
    draft.document = document;
    const error = integrationError(draft,true); if (error) return { error };
    if (current && current.document.provider === document.provider) draft.connection = { ...current.connection };
    // Import never copies a machine assignment or binds a similarly named device.
    return { config: draft };
  } catch { return { error: 'The file must contain a valid portable JSON profile.' }; }
}
export function normalizeIntegrationCatalogue(catalogue?: IntegrationCatalogue): { channels: IntegrationChannel[]; commands: IntegrationCommand[] } {
  if (!catalogue) return { channels: [], commands: [] };
  if (catalogue.channels || catalogue.commands) return { channels: catalogue.channels ?? [], commands: catalogue.commands ?? [] };
  return { channels: (catalogue.controls ?? []).flatMap(c => c.outputs.map(o => ({ id:c.id,label:c.label,type:o.type,output:o.index,maximum:o.maxValue,maxLength:o.maxLength }))),
    commands: (catalogue.controls ?? []).filter(c => c.inputs.length).map(c => ({ id:c.id,label:c.label,interfaces:c.inputs.map(i => ({kind:i.interface,maximum:i.maxValue})) })) };
}
export function integrationOutputActions(kind: IntegrationFormat['kind'], snapshot?: DeviceActions): DeviceAction[] {
  if (snapshot?.status !== 'ready') return [];
  return snapshot.actions.filter(a => a.readable && a.semantics === 'setter' && a.parameters.length === 1 &&
    (kind === 'rgb' ? a.parameters[0].type === 'rgb' : kind === 'boolean' || kind === 'threshold' ?
      a.parameters[0].type === 'boolean' || a.parameters[0].type === 'enum' && !!a.parameters[0].options?.includes('on') && !!a.parameters[0].options?.includes('off') : a.parameters[0].type === 'string'));
}
export function integrationChannels(kind: IntegrationFormat['kind'], catalogue?: IntegrationCatalogue): IntegrationChannel[] {
  return normalizeIntegrationCatalogue(catalogue).channels.filter(c => kind === 'text' ? c.type === 'string' : kind === 'boolean' ? c.type === 'boolean' || c.type === 'integer' && c.maximum === 1 : c.type === 'integer' || c.type === 'number');
}
export const integrationTargetKey = (target: { gatewayId: string; moduleId: string }) => `${target.gatewayId}/${target.moduleId}`;
export function roleTarget(config: IntegrationConfig, role: string, targets: IntegrationTarget[]): IntegrationTarget | undefined {
  const a = config.assignments.find(a => a.role === role);
  return a && targets.find(t => t.gatewayId === a.gatewayId && t.module.id === a.moduleId);
}
export function assignIntegrationRole(config: IntegrationConfig, role: string, target?: IntegrationTarget): IntegrationConfig {
  return { ...config, assignments: [...config.assignments.filter(a => a.role !== role), ...(target ? [{ role,gatewayId:target.gatewayId,moduleId:target.module.id }] : [])] };
}
export function renameIntegrationRole(config: IntegrationConfig, previous: string, next: string): IntegrationConfig {
  if (!identifier(next) || config.document.roles.some(r => r.id === next && r.id !== previous)) throw new Error('Choose a unique role identifier of up to 31 letters, digits, dots, underscores or hyphens.');
  return { ...config, document: { ...config.document,
    roles:config.document.roles.map(r => r.id === previous ? {...r,id:next} : r),
    inputs:config.document.inputs.map(r => r.role === previous ? {...r,role:next} : r),
    outputs:config.document.outputs.map(r => r.role === previous ? {...r,role:next} : r) },
    assignments:config.assignments.map(a => a.role === previous ? {...a,role:next} : a) };
}
export function integrationPhysicalControls(kind: DcsInputKind, events?: PanelInputState): { value: number; label: string }[] {
  const metadata = panelInputMetadata(events);
  if (!metadata) return [];
  if (kind === 'axis') return Array.from({ length: metadata.axisCount ?? 0 },(_,value) => ({ value,label:`Axis ${value}` }));
  if (kind === 'encoder') return Array.from({ length: Math.floor(metadata.encoderButtons.length/2) },(_,value) => ({ value,label:`Encoder ${value}` }));
  return Array.from({length:metadata.contactButtonCount ?? 0},(_,value) => value).filter(value => kind === 'switch' ? metadata.maintainedButtons?.includes(value) : !metadata.maintainedButtons?.includes(value)).map(value => ({ value,label:`${kind === 'switch' ? 'Switch' : 'Button'} ${value}` }));
}
export function integrationStartError(config: IntegrationConfig, targets: IntegrationTarget[]): string {
  const error = integrationError(config); if (error) return error;
  if (config.document.provider === 'x-plane') return 'Launch or activate the matching X-Plane game profile from Games to select its monitored process.';
  if (config.document.provider === 'assetto-corsa-rally' && config.connection.source === 'shared-memory' && config.connection.processId == null) return 'Launch this integration from Games, or select a running ACR process for a manual session.';
  for (const role of config.document.roles) {
    const rows = [...config.document.inputs, ...config.document.outputs].filter(row => row.role === role.id); if (!rows.length) continue;
    const target = roleTarget(config,role.id,targets);
    if (!target?.state?.connected || !target.module.online || !target.module.boot) return `Assign and reconnect ${role.label}.`;
    if (target.busy || target.state.reconnecting || target.state.layoutPending) return 'Wait for device updates to finish.';
    if (config.document.inputs.some(row => row.role === role.id) && (target.module.directUsb || target.module.enabled !== false || target.module.published !== false)) return `Disable ${role.label} HID and disconnect direct USB before starting.`;
    for (const row of config.document.outputs.filter(row => row.role === role.id)) {
      const actions=target.state.moduleActions?.[target.module.id];
      if (actions?.boot !== target.module.boot || !integrationOutputActions(row.format.kind,actions).some(a => a.id === row.actionId && a.endpoint === row.endpoint)) return `Refresh ${role.label} and choose a current compatible output endpoint.`;
    }
    for (const row of config.document.inputs.filter(row => row.role === role.id)) {
      const events=target.state.moduleEvents?.[target.module.id];
      if (events?.boot !== target.module.boot || !integrationPhysicalControls(row.kind,events).some(c => c.value === row.control)) return `Refresh ${role.label} and choose an advertised ${row.kind}.`;
    }
  }
  return '';
}
/** Explicit schema upgrade for authored logical feedback; legacy saves remain schema 1. */
export function withIntegrationFeedback(config: IntegrationConfig): IntegrationConfig {
  return { ...config, document: { ...config.document, schemaVersion: 2, feedbackOutputs: (config.document.feedbackOutputs ?? []).map(row => ({ ...row })) } };
}
export function nextIntegrationId(config: IntegrationConfig, prefix: string): string {
  const ids = new Set([...config.document.inputs,...config.document.outputs,...(config.document.feedbackOutputs??[])].map(row => row.id));
  let n=1; while(ids.has(`${prefix}-${n}`)) n++; return `${prefix}-${n}`;
}
export function revLightOutput(config: IntegrationConfig, role: string, target: IntegrationTarget, catalogue?: IntegrationCatalogue): IntegrationOutput | undefined {
  if (!normalizeIntegrationCatalogue(catalogue).channels.some(c => c.id === 'rev_ratio' && c.type === 'number')) return undefined;
  const snapshot=target.state?.moduleActions?.[target.module.id];
  const rgbAction=integrationOutputActions('rgb',snapshot)[0], boolAction=integrationOutputActions('threshold',snapshot)[0];
  const action=rgbAction ?? boolAction; if (!action) return undefined;
  return { id:nextIntegrationId(config,'rev-light'),role,endpoint:action.endpoint,actionId:action.id,channel:'rev_ratio',output:0,
    format:rgbAction ? {kind:'rgb',bands:[{minimum:0.7,color:'00FF00'},{minimum:0.85,color:'FFFF00'},{minimum:0.95,color:'FF0000'}],unknownColor:'000000',staleColor:'000000'} : {kind:'threshold',threshold:0.9,comparison:'gte'} };
}

export type TelemetryFrame = { sourceId: string; sourceEpoch: number; physics: number[]; graphics: number[]; staticPage: number[] };
/** Authored ACR prefix bytes go through the same native parser as Windows reads. */
export function syntheticTelemetryFrame(sample: { packetId: number; epoch: number; rpm: number; gear: number; speed: number; maxRpm: number; status: 'live' | 'paused' | 'replay' | 'off' }): TelemetryFrame {
  if (!whole(sample.packetId,0,4294967295) || !whole(sample.epoch,1,Number.MAX_SAFE_INTEGER) || !whole(sample.rpm,0,100000) || !whole(sample.gear,-1,31) || !finite(sample.speed,2000) || sample.speed < 0 || !whole(sample.maxRpm,0,100000) || !['live','paused','replay','off'].includes(sample.status)) throw new Error('Check the bounded synthetic telemetry values.');
  const physics=new Uint8Array(32), graphics=new Uint8Array(8), staticPage=new Uint8Array(416);
  const p=new DataView(physics.buffer), g=new DataView(graphics.buffer), s=new DataView(staticPage.buffer);
  p.setUint32(0,sample.packetId,true);p.setInt32(16,sample.gear+1,true);p.setInt32(20,sample.rpm,true);p.setFloat32(28,sample.speed,true);
  g.setUint32(0,sample.packetId,true);g.setInt32(4,{off:0,replay:1,live:2,paused:3}[sample.status],true);s.setInt32(412,sample.maxRpm,true);
  for (const [offset,value,max] of [[0,'synthetic',15],[30,'fixture-1',15],[68,'Cockpit test car',33],[134,'Synthetic track',33]] as const) for(let i=0;i<Math.min(value.length,max-1);i++) s.setUint16(offset+i*2,value.charCodeAt(i),true);
  return {sourceId:'cockpit-synthetic',sourceEpoch:sample.epoch,physics:Array.from(physics),graphics:Array.from(graphics),staticPage:Array.from(staticPage)};
}
