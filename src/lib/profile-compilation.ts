/** Read-only compiler previews. Preview IDs never authorize Apply or device writes. */
import { profileDocumentError, profileContractError, profileAssignmentsError } from './profiles.ts';
import { resourceTokensEqual } from './profile-inventory.ts';
import type { DeepReadonly, ProfileDocument, ProfileEndpoint, ProfileLocalAssignment, ProfilePortContract } from './profiles';
import type { ResourceRoute, ResourceToken } from './profile-inventory';

export const COMPILATION_SCHEMA_VERSION = 1;
export const COMPILATION_SECTIONS = ['diagnostics', 'nodes', 'order', 'sources', 'sinks', 'branches', 'controllers', 'visibility'] as const;
export type CompilationSection = typeof COMPILATION_SECTIONS[number];
export type PlanReadiness = 'ready' | 'blocked' | 'dormant' | 'invalid';
export type CompileDiagnostic = { code: string; severity: 'error' | 'warning' | 'info'; message: string;
  location: { resourceId: string | null; portId: string | null; nodeId: string | null; edgeIds: string[] } };
export type PlanBudgets = { usedSources: number; virtualTargets: number; sourcePorts: number; processingNodes: number;
  sinkPorts: number; bindings: number; stateSlots: number; eventQueueCapacity: number; actionQueueCapacity: number; timerCapacity: number };
export type CompilationSummary = { schemaVersion: 1; compilationId: string; profileId: string; profileRevision: number;
  requirements?:{desktopInput:boolean;virtualDevices:boolean;visibility:boolean}|null;
  programHash: string | null; documentSemanticHash: string | null; captureFingerprint: string; readiness: PlanReadiness;
  diagnostics: CompileDiagnostic[]; sectionTotals: Record<CompilationSection, number>; budgets: PlanBudgets | null };
export type CompiledSource = { endpoint: ProfileEndpoint; inventoryResourceId: string | null; observedPortId: string | null;
  contract: ProfilePortContract; token: ResourceToken | null; route: ResourceRoute | null; readiness: PlanReadiness };
export type CompiledSink = CompiledSource & { adapter: 'desktop-input' | 'virtual-controller' | 'physical-state' | 'physical-action' | 'simulator-command' | 'companion-feedback' };
export type CompilationItems = {
  diagnostics: CompileDiagnostic;
  nodes: { nodeId: string; inputPorts: string[]; outputPorts: string[]; incomingEdgeIds: string[]; outgoingEdgeIds: string[] };
  order: string;
  sources: CompiledSource;
  sinks: CompiledSink;
  branches: { sink: ProfileEndpoint; sourcePorts: ProfileEndpoint[]; processingNodes: string[]; edgeIds: string[]; required: boolean; readiness: PlanReadiness };
  controllers: { id: string; name: string; buttons: number; axes: number; hats: number; connectedPorts: string[]; readiness: PlanReadiness };
  visibility: { resourceId: string; inventoryResourceId: string | null; mode: 'visible' | 'hidden-while-active'; readiness: PlanReadiness };
};
export type CompilationPage<S extends CompilationSection = CompilationSection> = {
  compilationId: string; profileId: string; profileRevision: number; programHash: string | null; captureFingerprint: string;
  section: S; offset: number; total: number; items: CompilationItems[S][];
};
export type ProfileCompilationCommand = 'profile_compile' | 'profile_compilation_inspect';
export type ProfileCompilationRequest = { command: 'profile_compile'; document: ProfileDocument; assignments?:ProfileLocalAssignment[] }
  | { command: 'profile_compilation_inspect'; compilationId: string; section: CompilationSection; offset: number; limit: number };
export type ProfileCompilationResult = CompilationSummary | CompilationPage;
export type ProfileCompilationSend = (request: ProfileCompilationRequest) => Promise<ProfileCompilationResult>;

const object = (value: unknown): value is Record<string, unknown> => value !== null && typeof value === 'object' && !Array.isArray(value);
const text = (value: unknown, max = 4096): value is string => typeof value === 'string' && value.length > 0 && value.length <= max && !/[\u0000-\u001f\u007f-\u009f]/.test(value);
const whole = (value: unknown, max: number): value is number => typeof value === 'number' && Number.isSafeInteger(value) && value >= 0 && value <= max;
const hash = (value: unknown): value is string => typeof value === 'string' && /^sha256:[a-f0-9]{64}$/.test(value);
const optionalHash = (value: unknown) => value === null || hash(value);
const readiness = (value: unknown): value is PlanReadiness => ['ready', 'blocked', 'dormant', 'invalid'].includes(String(value));
const uuid = (value: unknown): value is string => typeof value === 'string' && /^[a-f0-9]{8}-[a-f0-9]{4}-[a-f0-9]{4}-[a-f0-9]{4}-[a-f0-9]{12}$/.test(value);
const names = (value: unknown, limit: number) => Array.isArray(value) && value.length <= limit && value.every(item => text(item, 256));
const nullableText = (value: unknown) => value === null || text(value, 256);
function diagnostic(value: unknown): value is CompileDiagnostic {
  return object(value) && text(value.code, 128) && ['error', 'warning', 'info'].includes(String(value.severity)) && text(value.message, 16384)
    && object(value.location) && nullableText(value.location.resourceId) && nullableText(value.location.portId) && nullableText(value.location.nodeId) && names(value.location.edgeIds, 512);
}
const budgetFields = ['usedSources', 'virtualTargets', 'sourcePorts', 'processingNodes', 'sinkPorts', 'bindings', 'stateSlots', 'eventQueueCapacity', 'actionQueueCapacity', 'timerCapacity'] as const;
function summary(value: unknown): value is CompilationSummary {
  return object(value) && value.schemaVersion === COMPILATION_SCHEMA_VERSION && uuid(value.compilationId) && uuid(value.profileId)
    && whole(value.profileRevision, Number.MAX_SAFE_INTEGER) && value.profileRevision > 0 && optionalHash(value.programHash)
    && optionalHash(value.documentSemanticHash) && hash(value.captureFingerprint) && readiness(value.readiness)
    && Array.isArray(value.diagnostics) && value.diagnostics.length <= 32 && value.diagnostics.every(diagnostic)
    && object(value.sectionTotals) && COMPILATION_SECTIONS.every(section => whole((value.sectionTotals as Record<string, unknown>)[section], 16384))
    && value.diagnostics.length <= (value.sectionTotals.diagnostics as number)
    && (value.budgets === null || object(value.budgets) && budgetFields.every(field => whole((value.budgets as Record<string, unknown>)[field], 65536)))
    && (value.requirements===undefined||value.requirements===null||object(value.requirements)&&['desktopInput','virtualDevices','visibility'].every(key=>typeof(value.requirements as Record<string,unknown>)[key]==='boolean'));
}
function freezeClone<T>(value: T): DeepReadonly<T> {
  const copy: T = JSON.parse(JSON.stringify(value));
  const freeze = (entry: unknown) => { if (entry && typeof entry === 'object') { Object.values(entry).forEach(freeze); Object.freeze(entry); } };
  freeze(copy);
  return copy as DeepReadonly<T>;
}
function endpoint(value: unknown): boolean { return object(value) && text(value.nodeId, 80) && text(value.portId, 256); }
function item(section: CompilationSection, value: unknown): boolean {
  if (section === 'diagnostics') return diagnostic(value);
  if (section === 'order') return text(value, 80);
  if (!object(value)) return false;
  if (section === 'nodes') return text(value.nodeId, 80) && names(value.inputPorts, 128) && names(value.outputPorts, 128) && names(value.incomingEdgeIds, 512) && names(value.outgoingEdgeIds, 512);
  if (section === 'branches') return endpoint(value.sink) && Array.isArray(value.sourcePorts) && value.sourcePorts.length <= 1024 && value.sourcePorts.every(endpoint) && names(value.processingNodes, 256) && names(value.edgeIds, 512) && typeof value.required === 'boolean' && readiness(value.readiness);
  if (section === 'controllers') return text(value.id, 80) && text(value.name, 320) && whole(value.buttons, 128) && whole(value.axes, 16) && whole(value.hats, 4) && names(value.connectedPorts, 148) && readiness(value.readiness);
  if (section === 'visibility') return text(value.resourceId, 80) && nullableText(value.inventoryResourceId) && ['visible', 'hidden-while-active'].includes(String(value.mode)) && readiness(value.readiness);
  return endpoint(value.endpoint) && nullableText(value.inventoryResourceId) && nullableText(value.observedPortId)
    && profileContractError(value.contract, section === 'sources' ? 'output' : 'input') === '' && readiness(value.readiness)
    && (value.token === null || object(value.token) && resourceTokensEqual(value.token as ResourceToken, value.token as ResourceToken)
      && value.token.resourceId === value.inventoryResourceId && value.token.portId === value.observedPortId
      && value.token.access === (section === 'sources' ? 'read' : (value.contract as ProfilePortContract).delivery === 'action' ? 'invoke' : 'write'))
    && (value.route === null || object(value.route) && text(value.route.id, 4096) && text(value.route.providerGeneration, 4096)
      && ['hid', 'can', 'usb', 'network', 'integration'].includes(String(value.route.transport)) && object(value.route.attributes)
      && Object.keys(value.route.attributes).length <= 32 && Object.entries(value.route.attributes).every(([key, entry]) => text(key, 80) && text(entry, 4096))
      && typeof value.route.baselineReady === 'boolean' && typeof value.route.writable === 'boolean'
      && typeof value.route.priority === 'number' && Number.isInteger(value.route.priority) && value.route.priority >= -2147483648 && value.route.priority <= 2147483647)
    && (value.token === null || object(value.route) && value.token.routeId === value.route.id)
    && (section !== 'sinks' || ['desktop-input', 'virtual-controller', 'physical-state', 'physical-action', 'simulator-command', 'companion-feedback'].includes(String(value.adapter)));
}

export class ProfileCompilerClient {
  private readonly send: ProfileCompilationSend;
  constructor(send: ProfileCompilationSend) { this.send = send; }
  async compile(document: ProfileDocument, assignments?:ProfileLocalAssignment[]): Promise<DeepReadonly<CompilationSummary>> {
    const error = profileDocumentError(document)||(assignments===undefined?'':profileAssignmentsError(assignments));
    if (error) throw new Error(error);
    if(assignments?.some(assignment=>{const resource=document.resources.find(resource=>resource.id===assignment.resourceId);return !resource||resource.ports.length!==assignment.ports.length||resource.ports.some(port=>!assignment.ports.some(pin=>pin.profilePortId===port.id));}))throw new Error('Pin every selected logical port explicitly for this draft preview.');
    // Validate before JSON cloning; NaN must never become a nullable policy.
    const captured: ProfileDocument = JSON.parse(JSON.stringify(document));
    const result = await this.send({ command: 'profile_compile', document: captured,...(assignments===undefined?{}:{assignments:JSON.parse(JSON.stringify(assignments)) as ProfileLocalAssignment[]}) });
    if (!summary(result) || result.profileId !== captured.id || result.profileRevision !== captured.revision) throw new Error('The compilation preview does not match the captured draft.');
    return freezeClone(result);
  }
  async inspect<S extends CompilationSection>(preview: DeepReadonly<CompilationSummary>, section: S, offset = 0, limit = 32): Promise<DeepReadonly<CompilationPage<S>>> {
    if (!summary(preview) || !COMPILATION_SECTIONS.includes(section) || !whole(offset, 16384) || !whole(limit, 64) || limit === 0) throw new Error('Choose a bounded compilation preview page.');
    const raw: unknown = await this.send({ command: 'profile_compilation_inspect', compilationId: preview.compilationId, section, offset, limit });
    if (!object(raw)) throw new Error('The compiler page does not belong to this immutable preview.');
    const result: Record<string, unknown> = raw;
    if (!object(result) || result.compilationId !== preview.compilationId || result.profileId !== preview.profileId || result.profileRevision !== preview.profileRevision
      || result.programHash !== preview.programHash || result.captureFingerprint !== preview.captureFingerprint || result.section !== section || result.offset !== offset
      || !whole(result.total, 16384) || result.total !== preview.sectionTotals[section] || !Array.isArray(result.items) || result.items.length > limit || result.items.some((value: unknown) => !item(section, value))
      || offset + result.items.length > result.total || offset < result.total && result.items.length === 0) throw new Error('The compiler page does not belong to this immutable preview.');
    return freezeClone(result as unknown as CompilationPage<S>);
  }
  async inspectAll<S extends CompilationSection>(preview: DeepReadonly<CompilationSummary>, section: S): Promise<DeepReadonly<CompilationPage<S>>> {
    const first = await this.inspect(preview, section);
    const items = [...first.items];
    while (items.length < first.total) { const next = await this.inspect(preview, section, items.length); items.push(...next.items); }
    return freezeClone({ ...first, items }) as DeepReadonly<CompilationPage<S>>;
  }
}
