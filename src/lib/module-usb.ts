/** Explicit standalone USB operations. Native code owns the wire handshake and writer claim. */
import { configurationError } from './runtime-config.ts';
import type { RuntimeConfig, RuntimeSnapshot } from './runtime-config';
import { actionOwnershipValid } from './device-actions.ts';
import type { DeviceAction, DeviceActions, ActionValue } from './device-actions';
import type { PanelInputState } from './panel-input-metadata';
import type { DeepReadonly } from './profiles';

export type UsbRouteToken = { moduleId: string; serial: string; connectionGeneration: number; boot: number; configurationGeneration: number };
export type UsbIdentity = { model: 'device-template'; board: string; profile: string; moduleId: string; serial: string; protocol: 3; buttonCount: number; axisCount: number; hatCount: number };
export type UsbPortInfo = { path: string; serialNumber?: string; vendorId?: string; productId?: string; manufacturer?: string; friendlyName?: string };
export type UsbUnsupported = { boot: number; route: UsbRouteToken; status: 'unsupported' };
export type UsbActions = DeviceActions & { route: UsbRouteToken };
export type UsbEvents = PanelInputState & { ok: true; schemaVersion: 1; route: UsbRouteToken; oldest: number; overflow: number;
  after?: number; next?: number; more?: boolean; events?: { sequence: number; control: number; pressed: boolean }[] };
export type UsbObservation = { connected: boolean; port: string | null; identity: UsbIdentity | null; route: UsbRouteToken | null;
  claimedBy: string | null; observedAtMs: number | null; error: string | null };
export type UsbSnapshot = UsbObservation & { config: RuntimeSnapshot | null; draft: RuntimeSnapshot | null; actions: UsbActions | UsbUnsupported | null; events: UsbEvents | UsbUnsupported | null };
export type UsbSlot<S = UsbSnapshot> = { key: string; port: UsbPortInfo; present: boolean; automatic: boolean; admissible: boolean; snapshot: S; diagnostics: string[] };
export type UsbModule = UsbSlot<UsbSnapshot>;
export type UsbModuleSummary = UsbSlot<UsbObservation>;
export type UsbPortSelection = { key: string; port: UsbPortInfo; present: boolean; automatic: boolean; admissible: boolean; connected: boolean; diagnostics: string[] };
export type UsbPortsResult = { ports: UsbPortSelection[] };
export type UsbListResult = { offset: number; total: number; modules: UsbModuleSummary[] };
export type ModuleUsbResult = UsbPortsResult | UsbListResult | { module: UsbModule };
type RouteCommand = 'module_usb_config_read' | 'module_usb_claim' | 'module_usb_release' | 'module_usb_config_save' | 'module_usb_config_discard' | 'module_usb_config_reset' | 'module_usb_actions_describe' | 'module_usb_actions_state' | 'module_usb_events_state';
export type ModuleUsbRequest = { command: 'module_usb_ports' }
  | { command: 'module_usb_list'; offset: number; limit: number }
  | { command: 'module_usb_connect'; key: string; port: string }
  | { command: 'module_usb_disconnect' | 'module_usb_status'; key: string }
  | { command: RouteCommand; key: string; route: UsbRouteToken }
  | { command: 'module_usb_config_apply'; key: string; route: UsbRouteToken; config: RuntimeConfig; save: boolean }
  | { command: 'module_usb_actions_invoke'; key: string; route: UsbRouteToken; catalogueRevision: number; actionId: string; values: ActionValue[] }
  | { command: 'module_usb_events_read'; key: string; route: UsbRouteToken; stream: number; after: number };
export type ModuleUsbCommand = ModuleUsbRequest['command'];
export type ModuleUsbSend = (request: ModuleUsbRequest) => Promise<ModuleUsbResult>;

const object = (value: unknown): value is Record<string, unknown> => value !== null && typeof value === 'object' && !Array.isArray(value);
const text = (value: unknown, maximum = 2048): value is string => typeof value === 'string' && value.length > 0 && new TextEncoder().encode(value).length <= maximum && !/[\u0000-\u001f\u007f-\u009f]/.test(value);
const uint = (value: unknown, maximum = 0xffffffff, minimum = 0): value is number => typeof value === 'number' && Number.isSafeInteger(value) && value >= minimum && value <= maximum;
const rows = (value: unknown, maximum: number): value is unknown[] => Array.isArray(value) && value.length <= maximum;
const strings = (value: unknown) => rows(value, 256) && value.every(item => text(item, 4096));
const clone = <T>(value: T): T => JSON.parse(JSON.stringify(value));
function capture<T>(value: T): DeepReadonly<T> {
  const copied = clone(value);
  const freeze = (item: unknown) => { if (item && typeof item === 'object') { Object.values(item).forEach(freeze); Object.freeze(item); } };
  freeze(copied);
  return copied as DeepReadonly<T>;
}
function route(value: unknown): value is UsbRouteToken {
  return object(value) && typeof value.serial === 'string' && /^[0-9a-f]{24}$/.test(value.serial) && value.moduleId === `4f534744${value.serial}`
    && uint(value.connectionGeneration, Number.MAX_SAFE_INTEGER, 1) && uint(value.boot, 0xffffffff, 1) && uint(value.configurationGeneration, 0xffffffff, 1);
}
export function usbRoutesEqual(left: UsbRouteToken, right: UsbRouteToken): boolean {
  return route(left) && route(right) && left.moduleId === right.moduleId && left.serial === right.serial && left.connectionGeneration === right.connectionGeneration && left.boot === right.boot && left.configurationGeneration === right.configurationGeneration;
}
function identity(value: unknown): value is UsbIdentity {
  return object(value) && value.model === 'device-template' && value.protocol === 3 && text(value.board, 31) && text(value.profile, 63)
    && typeof value.serial === 'string' && /^[0-9a-f]{24}$/.test(value.serial) && value.moduleId === `4f534744${value.serial}`
    && uint(value.buttonCount, 128) && uint(value.axisCount, 8) && uint(value.hatCount, 4);
}
function port(value: unknown): value is UsbPortInfo {
  return object(value) && text(value.path) && ['serialNumber', 'vendorId', 'productId', 'manufacturer', 'friendlyName'].every(field => value[field] === undefined || text(value[field], 256));
}
function observation(value: unknown): value is UsbObservation {
  if (!object(value) || typeof value.connected !== 'boolean' || !(value.port === null || text(value.port)) || !(value.identity === null || identity(value.identity)) || !(value.route === null || route(value.route)) || !(value.claimedBy === null || text(value.claimedBy, 80)) || !(value.observedAtMs === null || uint(value.observedAtMs, Number.MAX_SAFE_INTEGER)) || !(value.error === null || text(value.error, 4096))) return false;
  return !value.connected || value.route !== null && value.identity !== null && (value.route as UsbRouteToken).moduleId === (value.identity as UsbIdentity).moduleId;
}
function config(value: unknown): value is RuntimeConfig {
  return object(value) && text(value.name, 63) && uint(value.expanderMask, 255) && ['buttons', 'axes', 'hats', 'encoders', 'selectors'].every(key => rows(value[key], 128) && value[key].every(row => rows(row, 7) && row.every(item => uint(item, 65535))));
}
function runtime(value: unknown): value is RuntimeSnapshot {
  return object(value) && uint(value.generation, 0xffffffff, 1) && uint(value.savedGeneration) && uint(value.owner) && uint(value.claimExpiresMs) && typeof value.dirty === 'boolean' && typeof value.pendingApply === 'boolean' && typeof value.valid === 'boolean' && ['active', 'draft'].includes(String(value.view)) && config(value.config) && typeof value.descriptorHex === 'string' && /^(?:[0-9a-f]{2}){0,256}$/i.test(value.descriptorHex)
    && (value.layout === null && !value.valid || object(value.layout) && uint(value.layout.buttons, 128) && uint(value.layout.axes, 8) && uint(value.layout.hats, 4) && uint(value.layout.reportBytes, 35));
}
function action(value: unknown): value is DeviceAction {
  if (!object(value) || !text(value.id, 31) || !text(value.label, 63) || !text(value.endpoint, 31) || !['setter', 'oneShot'].includes(String(value.semantics)) || typeof value.readable !== 'boolean' || !rows(value.parameters, 4)) return false;
  if (!value.parameters.every(parameter => object(parameter) && text(parameter.id, 31) && text(parameter.label, 63) && (
    parameter.type === 'boolean' || parameter.type === 'rgb' || parameter.type === 'integer' && typeof parameter.minimum === 'number' && Number.isSafeInteger(parameter.minimum) && parameter.minimum >= -2147483648 && typeof parameter.maximum === 'number' && Number.isSafeInteger(parameter.maximum) && parameter.maximum <= 2147483647 && parameter.minimum <= parameter.maximum
    || parameter.type === 'enum' && rows(parameter.options, 16) && parameter.options.length > 0 && parameter.options.every(option => text(option, 31)) && new Set(parameter.options).size === parameter.options.length
    || parameter.type === 'string' && uint(parameter.maxBytes, 63, 1)))) return false;
  if (new Set(value.parameters.map(parameter => (parameter as { id: string }).id)).size !== value.parameters.length) return false;
  return value.display === undefined || object(value.display) && uint(value.display.columns, 63, 1) && uint(value.display.rows, 4, 1) && value.display.columns * value.display.rows <= 63 && ['ascii', 'utf8'].includes(String(value.display.encoding)) && value.semantics === 'setter' && value.readable && value.parameters.length === 1 && (value.parameters[0] as Record<string, unknown>).type === 'string' && ((value.parameters[0] as Record<string, unknown>).maxBytes as number) >= value.display.columns * value.display.rows;
}
function actionValues(descriptor: DeviceAction, values: readonly ActionValue[]): boolean {
  return values.length === descriptor.parameters.length && descriptor.parameters.every((parameter, index) => {
    const value = values[index];
    return parameter.type === 'boolean' ? typeof value === 'boolean' : parameter.type === 'integer' ? typeof value === 'number' && Number.isSafeInteger(value) && value >= parameter.minimum! && value <= parameter.maximum! : parameter.type === 'enum' ? typeof value === 'string' && parameter.options!.includes(value) : parameter.type === 'rgb' ? typeof value === 'string' && /^[0-9a-f]{6}$/i.test(value) : typeof value === 'string' && (value === '' || text(value, parameter.maxBytes)) && (!descriptor.display || (descriptor.display.encoding !== 'ascii' || /^[\x00-\x7f]*$/.test(value)) && [...value].length <= descriptor.display.columns * descriptor.display.rows);
  });
}
function capability(value: unknown, kind: 'actions' | 'events'): boolean {
  if (!object(value) || !uint(value.boot, 0xffffffff, 1) || !route(value.route) || value.route.boot !== value.boot) return false;
  if (value.status === 'unsupported') return true;
  if (kind === 'actions') return (value.ownership === undefined || value.ownership === null || actionOwnershipValid(value.ownership)) && ['ready', 'unknown', 'error', 'uncertain'].includes(String(value.status)) && uint(value.catalogueRevision, 0xffffffff, 1) && rows(value.actions, 32) && value.actions.every(action) && rows(value.states, 32) && value.states.every(state => object(state) && text(state.id) && rows(state.values, 4) && state.values.every(entry => typeof entry === 'boolean' || typeof entry === 'string' || typeof entry === 'number' && Number.isFinite(entry)));
  return value.ok === true && value.schemaVersion === 1 && uint(value.stream, 0xffffffff, 1) && uint(value.head) && uint(value.oldest, 0xffffffff, 1) && uint(value.overflow) && uint(value.buttonCount, 128) && rows(value.held, 128) && value.held.length === value.buttonCount && value.held.every(entry => typeof entry === 'boolean') && rows(value.encoderButtons, 64) && value.encoderButtons.every(entry => uint(entry, 127))
    && (value.axisCount === undefined || uint(value.axisCount, 8) && ['axes', 'axisNeutral', 'axisUsage', 'axisAvailable'].every(key => rows(value[key], 8) && value[key].length === value.axisCount && value[key].every(entry => key === 'axisAvailable' ? typeof entry === 'boolean' : uint(entry, 65535))))
    && (value.events === undefined || uint(value.after) && uint(value.next) && typeof value.more === 'boolean' && rows(value.events, 16) && value.events.every(event => object(event) && uint(event.sequence, 0xffffffff, 1) && uint(event.control, value.buttonCount as number - 1) && typeof event.pressed === 'boolean'));
}
function slot(value: unknown, full: boolean): value is UsbModule {
  if (!object(value) || !text(value.key) || !port(value.port) || typeof value.present !== 'boolean' || typeof value.automatic !== 'boolean' || typeof value.admissible !== 'boolean' || !strings(value.diagnostics) || !observation(value.snapshot)) return false;
  if (!full) return true;
  const snapshot = value.snapshot as Record<string, unknown>;
  return ['config', 'draft'].every(key => snapshot[key] === null || runtime(snapshot[key])) && ['actions', 'events'].every(key => snapshot[key] === null || capability(snapshot[key], key as 'actions' | 'events'));
}
/** Capture a current native route. Never reconstruct it from a name, USB path, or module ID. */
export function usbRoute(module: DeepReadonly<UsbModule>): UsbRouteToken {
  if (!slot(module, true) || !module.present || !module.admissible || !module.snapshot.connected || !route(module.snapshot.route) || !module.snapshot.config?.valid || module.snapshot.config.view !== 'active' || module.snapshot.config.generation !== module.snapshot.route.configurationGeneration) throw new Error('Refresh and connect this unambiguous USB endpoint before continuing.');
  return clone(module.snapshot.route);
}

export class ModuleUsbClient {
  private readonly send: ModuleUsbSend;
  constructor(send: ModuleUsbSend) { this.send = send; }
  async ports(): Promise<DeepReadonly<UsbPortsResult>> {
    const result = await this.send({ command: 'module_usb_ports' });
    if (!object(result) || !('ports' in result) || !rows(result.ports, 32) || !result.ports.every(item => object(item) && text(item.key) && port(item.port) && item.present === true && typeof item.automatic === 'boolean' && typeof item.admissible === 'boolean' && typeof item.connected === 'boolean' && strings(item.diagnostics)) || new Set(result.ports.map(item => (item as UsbPortSelection).key)).size !== result.ports.length) throw new Error('Malformed observed USB ports.');
    return capture(result as UsbPortsResult);
  }
  async list(offset = 0, limit = 16): Promise<DeepReadonly<UsbListResult>> {
    if (!uint(offset, 128) || !uint(limit, 32, 1)) throw new Error('Choose a bounded USB inventory page.');
    const result = await this.send({ command: 'module_usb_list', offset, limit });
    if (!object(result) || !('modules' in result) || result.offset !== offset || !uint(result.total, 128) || !rows(result.modules, limit) || !result.modules.every(module => slot(module, false)) || offset + result.modules.length > result.total || offset < result.total && result.modules.length === 0 || new Set(result.modules.map(module => (module as UsbModuleSummary).key)).size !== result.modules.length) throw new Error('Malformed USB inventory page.');
    return capture(result as UsbListResult);
  }
  private async module(request: ModuleUsbRequest, key: string): Promise<DeepReadonly<UsbModule>> {
    const result = await this.send(clone(request));
    if (!object(result) || !('module' in result) || !slot(result.module, true) || result.module.key !== key) throw new Error('Malformed or mismatched USB module response; the operation result is unknown. Refresh status before another explicit change.');
    return capture(result.module);
  }
  async connect(selection: DeepReadonly<UsbPortSelection>): Promise<DeepReadonly<UsbModule>> {
    if (!selection.present || !selection.admissible || !text(selection.key) || !port(selection.port)) throw new Error('Select one currently observed unambiguous USB port.');
    const module = await this.module({ command: 'module_usb_connect', key: selection.key, port: selection.port.path }, selection.key);
    usbRoute(module);
    return module;
  }
  status(key: string): Promise<DeepReadonly<UsbModule>> { return this.module({ command: 'module_usb_status', key }, key); }
  disconnect(key: string): Promise<DeepReadonly<UsbModule>> { return this.module({ command: 'module_usb_disconnect', key }, key); }
  private async routed(command: RouteCommand, module: DeepReadonly<UsbModule>): Promise<DeepReadonly<UsbModule>> {
    return this.module({ command, key: module.key, route: usbRoute(module) }, module.key);
  }
  readConfig(module: DeepReadonly<UsbModule>) { return this.routed('module_usb_config_read', module); }
  claim(module: DeepReadonly<UsbModule>) { return this.routed('module_usb_claim', module); }
  release(module: DeepReadonly<UsbModule>) { return this.routed('module_usb_release', module); }
  save(module: DeepReadonly<UsbModule>) { return this.routed('module_usb_config_save', module); }
  discard(module: DeepReadonly<UsbModule>) { return this.routed('module_usb_config_discard', module); }
  reset(module: DeepReadonly<UsbModule>) { return this.routed('module_usb_config_reset', module); }
  describeActions(module: DeepReadonly<UsbModule>) { return this.routed('module_usb_actions_describe', module); }
  readActions(module: DeepReadonly<UsbModule>) { return this.routed('module_usb_actions_state', module); }
  eventsState(module: DeepReadonly<UsbModule>) { return this.routed('module_usb_events_state', module); }
  async apply(module: DeepReadonly<UsbModule>, configuration: RuntimeConfig, save = false): Promise<DeepReadonly<UsbModule>> {
    if (!config(configuration)) throw new Error('Provide a valid bounded USB configuration.');
    const error = configurationError(configuration);
    if (error) throw new Error(error);
    return this.module({ command: 'module_usb_config_apply', key: module.key, route: usbRoute(module), config: configuration, save }, module.key);
  }
  async invoke(module: DeepReadonly<UsbModule>, actionId: string, values: readonly ActionValue[]): Promise<DeepReadonly<UsbModule>> {
    const route = usbRoute(module), cache = module.snapshot.actions;
    if (!cache || cache.status !== 'ready' || !usbRoutesEqual(cache.route, route) || !cache.catalogueRevision) throw new Error('Read the current USB action catalogue and state before invoking an action.');
    const descriptor = cache.actions.find(action => action.id === actionId);
    if (!descriptor || !actionValues(clone(descriptor) as DeviceAction, values)) throw new Error('Provide the exact values allowed by this USB action.');
    return this.module({ command: 'module_usb_actions_invoke', key: module.key, route, catalogueRevision: cache.catalogueRevision, actionId, values: [...values] }, module.key);
  }
  async eventsRead(module: DeepReadonly<UsbModule>, stream: number, after: number): Promise<DeepReadonly<UsbModule>> {
    const route = usbRoute(module), events = module.snapshot.events;
    if (!uint(stream, 0xffffffff, 1) || !uint(after) || !events || 'status' in events || !usbRoutesEqual(events.route, route) || stream !== events.stream || after !== (events.next ?? events.head)) throw new Error('Establish a fresh USB input baseline and keep its event cursor.');
    return this.module({ command: 'module_usb_events_read', key: module.key, route, stream, after }, module.key);
  }
}
