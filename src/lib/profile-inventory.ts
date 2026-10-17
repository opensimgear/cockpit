/** Native resource observations stay local; portable graph definitions live in profiles.ts. */
import type { DeepReadonly, ProfileValue } from './profiles';
export const RESOURCE_PAGE_SIZE = 32;
export const RESOURCE_PORT_PAGE_SIZE = 32;
export type ResourceType = 'host-controller' | 'osg-module' | 'companion' | 'game-integration';
export type ResourceAccess = 'read' | 'write' | 'invoke';
export type ResourceEvidence = { kind: 'local-path' | 'serial' | 'verified-physical' | 'companion-instance' | 'game-scope'; value: string };
export type ResourceScalar = { type: 'boolean' } | { type: 'rgb' }
  | { type: 'integer' | 'number'; min: number; max: number; unit: string | null }
  | { type: 'text'; maxBytes: number; encoding: 'utf8' | 'ascii' }
  | { type: 'enum'; values: string[] };
export type ResourcePortContract = { delivery: 'state'; schema: ResourceScalar; freshnessMs: number; fallback: ProfileValue }
  | { delivery: 'event'; schema: ResourceScalar }
  | { delivery: 'action'; parameters: { id: string; name: string; schema: ResourceScalar }[]; semantics: 'setter' | 'one-shot'; endpoint: string; readable: boolean;
      display: { columns: number; rows: number; encoding: 'utf8' | 'ascii' } | null;
      ownership: { scope: string; maxLeaseMs: number } | null };
export type ResourcePortDescriptor = { id: string; name: string; direction: 'input' | 'output'; contract: ResourcePortContract };
export type ResourceRoute = { id: string; transport: 'hid' | 'can' | 'usb' | 'network' | 'integration'; providerGeneration: string; attributes: Record<string, string>; baselineReady: boolean; writable: boolean; priority: number };
export type ResourceSummary = { id: string; name: string; resourceType: ResourceType; availability: 'online' | 'offline' | 'unavailable'; generation: string; capabilityRevision: string;
  portCount: number; cachedPortCount: number; connections: { scope: string; key: string; route: ResourceRoute }[]; attributes: Record<string, string>; diagnostics: string[] };
export type InventoryMetadata = { inventoryDiagnostics?: string[]; readOnly?: boolean };
export type ResourcePage = InventoryMetadata & { session: string; revision: string; offset: number; total: number; resources: ResourceSummary[] };
export type ObservedResourcePort = { descriptor: ResourcePortDescriptor; contractHash: string; routeIds: string[]; current: boolean };
export type ResourceDetails = InventoryMetadata & { session: string; revision: string; resource: ResourceSummary; evidence: ResourceEvidence[]; ports: ObservedResourcePort[]; offset: number; total: number };
export type ResourceToken = { inventorySession: string; resourceId: string; generation: string; capabilityRevision: string; routeId: string; providerSession: string; providerGeneration: string; portId: string; contractHash: string; access: ResourceAccess };
export type ProfileInventoryCommand = 'profile_resources_list' | 'profile_resources_inspect' | 'profile_resources_token' | 'profile_resources_validate';
export type ProfileInventoryRequest = { command: 'profile_resources_list'; offset: number; limit: number }
  | { command: 'profile_resources_inspect'; resourceId: string; offset: number; limit: number }
  | { command: 'profile_resources_token'; resourceId: string; portId: string; access: ResourceAccess }
  | { command: 'profile_resources_validate'; resourceToken: ResourceToken; portId: string; access: ResourceAccess };
export type ProfileResourcesResult = InventoryMetadata & (ResourcePage | ResourceDetails | { resourceToken: ResourceToken } | { valid: true });
export type ProfileInventorySend = (request: ProfileInventoryRequest) => Promise<ProfileResourcesResult>;

const object = (value: unknown): value is Record<string, unknown> => value !== null && typeof value === 'object' && !Array.isArray(value);
const text = (value: unknown, max = 4096): value is string => typeof value === 'string' && value.length > 0 && new TextEncoder().encode(value).length <= max && !/[\u0000-\u001f\u007f-\u009f]/.test(value);
const counter = (value: unknown): value is string => typeof value === 'string' && /^(0|[1-9][0-9]*)$/.test(value);
const whole = (value: unknown, max: number): value is number => typeof value === 'number' && Number.isSafeInteger(value) && value >= 0 && value <= max;
const array = (value: unknown, max: number): value is unknown[] => Array.isArray(value) && value.length <= max;
const strings = (value: unknown, max: number) => array(value, max) && value.every(item => text(item));
const attributes = (value: unknown, maximum = 32) => object(value) && Object.keys(value).length <= maximum && Object.entries(value).every(([key, entry]) => text(key, 80) && text(entry));
function freezeClone<T>(value: T): DeepReadonly<T> {
  const cloned: T = JSON.parse(JSON.stringify(value));
  const freeze = (value: unknown) => { if (value && typeof value === 'object') { Object.values(value).forEach(freeze); Object.freeze(value); } };
  freeze(cloned);
  return cloned as DeepReadonly<T>;
}
function scalar(value: unknown): value is ResourceScalar {
  if (!object(value)) return false;
  if (value.type === 'boolean' || value.type === 'rgb') return true;
  if (value.type === 'text') return whole(value.maxBytes, 4096) && value.maxBytes > 0 && ['utf8', 'ascii'].includes(String(value.encoding));
  if (value.type === 'enum') return array(value.values, 64) && value.values.length > 0 && value.values.every(entry => text(entry, 256)) && new Set(value.values).size === value.values.length;
  return ['integer', 'number'].includes(String(value.type)) && typeof value.min === 'number' && Number.isFinite(value.min) && typeof value.max === 'number' && Number.isFinite(value.max) && value.min <= value.max && Math.abs(value.min) <= Number.MAX_SAFE_INTEGER && Math.abs(value.max) <= Number.MAX_SAFE_INTEGER && (value.type !== 'integer' || Number.isSafeInteger(value.min) && Number.isSafeInteger(value.max)) && (value.unit === null || text(value.unit, 128));
}
function accepts(schema: ResourceScalar, value: unknown): boolean {
  if (schema.type === 'boolean') return typeof value === 'boolean';
  if (schema.type === 'rgb') return typeof value === 'string' && /^[0-9a-f]{6}$/i.test(value);
  if (schema.type === 'enum') return typeof value === 'string' && schema.values.includes(value);
  if (schema.type === 'text') return typeof value === 'string' && (value === '' || text(value, schema.maxBytes)) && (schema.encoding === 'utf8' || /^[\x00-\x7f]*$/.test(value));
  return typeof value === 'number' && Number.isFinite(value) && value >= schema.min && value <= schema.max && (schema.type !== 'integer' || Number.isSafeInteger(value));
}
export function resourcePortError(value: unknown): string {
  if (!object(value) || !text(value.id, 256) || !text(value.name, 320) || !['input', 'output'].includes(String(value.direction)) || !object(value.contract)) return 'Malformed resource port descriptor.';
  const contract = value.contract;
  if (contract.delivery === 'state') return scalar(contract.schema) && whole(contract.freshnessMs, 600000) && contract.freshnessMs > 0 && Object.hasOwn(contract, 'fallback') && (contract.fallback === null || accepts(contract.schema, contract.fallback)) ? '' : 'Malformed resource state contract.';
  if (contract.delivery === 'event') return scalar(contract.schema) ? '' : 'Malformed resource event contract.';
  if (contract.delivery !== 'action' || value.direction !== 'input' || !['setter', 'one-shot'].includes(String(contract.semantics)) || !text(contract.endpoint, 256) || typeof contract.readable !== 'boolean' || !array(contract.parameters, 4) || !contract.parameters.every(parameter => object(parameter) && text(parameter.id, 80) && text(parameter.name, 320) && scalar(parameter.schema))) return 'Malformed resource action contract.';
  if (new Set(contract.parameters.map(parameter => (parameter as { id: string }).id)).size !== contract.parameters.length) return 'Duplicate resource action argument.';
  if (!Object.hasOwn(contract, 'display') || contract.display !== null && (!object(contract.display) || !whole(contract.display.columns, 63) || contract.display.columns === 0 || !whole(contract.display.rows, 4) || contract.display.rows === 0 || contract.display.columns * contract.display.rows > 63 || !['utf8', 'ascii'].includes(String(contract.display.encoding)))) return 'Malformed resource display constraints.';
  if (contract.display !== null) {
    const display = contract.display as { columns: number; rows: number; encoding: string };
    const parameters = contract.parameters as { schema: ResourceScalar }[];
    const schema = parameters[0]?.schema;
    if (contract.semantics !== 'setter' || !contract.readable || parameters.length !== 1 || schema?.type !== 'text' || schema.maxBytes < display.columns * display.rows || schema.encoding !== display.encoding) return 'Resource display does not match its action argument.';
  }
  if (!Object.hasOwn(contract, 'ownership') || contract.ownership !== null && (!object(contract.ownership) || contract.ownership.scope !== 'endpoint' || !whole(contract.ownership.maxLeaseMs, 60000) || contract.ownership.maxLeaseMs === 0)) return 'Malformed resource ownership constraints.';
  return '';
}
function route(value: unknown): boolean {
  return object(value) && text(value.id) && ['hid', 'can', 'usb', 'network', 'integration'].includes(String(value.transport)) && text(value.providerGeneration) && attributes(value.attributes) && typeof value.baselineReady === 'boolean' && typeof value.writable === 'boolean' && typeof value.priority === 'number' && Number.isInteger(value.priority) && value.priority >= -2147483648 && value.priority <= 2147483647;
}
function summary(value: unknown): value is ResourceSummary {
  return object(value) && text(value.id) && text(value.name) && ['host-controller', 'osg-module', 'companion', 'game-integration'].includes(String(value.resourceType)) && ['online', 'offline', 'unavailable'].includes(String(value.availability)) && counter(value.generation) && text(value.capabilityRevision) && whole(value.portCount, 40000) && whole(value.cachedPortCount, 40000) && attributes(value.attributes, 2048) && strings(value.diagnostics, 1152) && array(value.connections, 64) && value.connections.every(connection => object(connection) && text(connection.scope) && text(connection.key) && route(connection.route));
}
function page(value: unknown, offset: number, limit: number): value is ResourcePage {
  return object(value) && text(value.session) && counter(value.revision) && value.offset === offset && whole(value.total, 2048) && array(value.resources, limit) && value.resources.every(summary) && offset + value.resources.length <= value.total && (offset >= value.total || value.resources.length > 0);
}
function details(value: unknown, resourceId: string, offset: number, limit: number): value is ResourceDetails {
  return object(value) && text(value.session) && counter(value.revision) && summary(value.resource) && value.resource.id === resourceId && value.offset === offset && whole(value.total, 40000) && array(value.evidence, 1024) && value.evidence.every(evidence => object(evidence) && ['local-path', 'serial', 'verified-physical', 'companion-instance', 'game-scope'].includes(String(evidence.kind)) && text(evidence.value)) && array(value.ports, limit) && value.ports.every(port => object(port) && resourcePortError(port.descriptor) === '' && text(port.contractHash) && strings(port.routeIds, 64) && typeof port.current === 'boolean') && offset + value.ports.length <= value.total && (offset >= value.total || value.ports.length > 0);
}
const tokenFields = ['inventorySession', 'resourceId', 'generation', 'capabilityRevision', 'routeId', 'providerSession', 'providerGeneration', 'portId', 'contractHash', 'access'] as const;
function token(value: unknown): value is ResourceToken {
  return object(value) && tokenFields.every(field => text(value[field])) && counter(value.generation) && ['read', 'write', 'invoke'].includes(String(value.access));
}
export function resourceTokensEqual(left: ResourceToken, right: ResourceToken): boolean {
  return token(left) && token(right) && tokenFields.every(field => left[field] === right[field]);
}
function requestPage(offset: number, limit: number, maximum: number): void {
  if (!whole(offset, maximum) || !whole(limit, 128) || limit === 0) throw new Error('Choose a bounded inventory page.');
}

/** Inspection does not claim routes, dispatch actions, or change profile/session state. */
export class ProfileInventoryClient {
  private readonly send: ProfileInventorySend;
  constructor(send: ProfileInventorySend) { this.send = send; }
  async list(offset = 0, limit = RESOURCE_PAGE_SIZE): Promise<DeepReadonly<ResourcePage>> {
    requestPage(offset, limit, 2048);
    const result = await this.send({ command: 'profile_resources_list', offset, limit });
    if (!page(result, offset, limit)) throw new Error('Malformed resource inventory page.');
    return freezeClone(result);
  }
  async listAll(): Promise<DeepReadonly<ResourcePage>> {
    const first = await this.list();
    const resources: DeepReadonly<ResourceSummary>[] = [...first.resources];
    while (resources.length < first.total) {
      const next = await this.list(resources.length);
      if (next.session !== first.session || next.revision !== first.revision || next.total !== first.total) throw new Error('Resource inventory changed while loading; refresh the inventory.');
      resources.push(...next.resources);
    }
    if (new Set(resources.map(resource => resource.id)).size !== resources.length) throw new Error('Resource inventory contains duplicate identities.');
    return freezeClone({ ...first, resources });
  }
  async inspect(resourceId: string, offset = 0, limit = RESOURCE_PORT_PAGE_SIZE): Promise<DeepReadonly<ResourceDetails>> {
    requestPage(offset, limit, 40000);
    const result = await this.send({ command: 'profile_resources_inspect', resourceId, offset, limit });
    if (!details(result, resourceId, offset, limit)) throw new Error('Malformed resource detail page.');
    return freezeClone(result);
  }
  async inspectAll(resourceId: string): Promise<DeepReadonly<ResourceDetails>> {
    const first = await this.inspect(resourceId);
    const ports: DeepReadonly<ObservedResourcePort>[] = [...first.ports];
    while (ports.length < first.total) {
      const next = await this.inspect(resourceId, ports.length);
      if (next.session !== first.session || next.revision !== first.revision || next.total !== first.total || next.resource.generation !== first.resource.generation || next.resource.capabilityRevision !== first.resource.capabilityRevision) throw new Error('Resource capabilities or route changed while loading; inspect the resource again.');
      ports.push(...next.ports);
    }
    if (new Set(ports.map(port => JSON.stringify([port.descriptor.id, port.contractHash]))).size !== ports.length) throw new Error('Resource details contain duplicate port variants.');
    return freezeClone({ ...first, ports });
  }
  async token(resourceId: string, portId: string, access: ResourceAccess = 'read'): Promise<DeepReadonly<ResourceToken>> {
    const result = await this.send({ command: 'profile_resources_token', resourceId, portId, access });
    if (!object(result) || !('resourceToken' in result) || !token(result.resourceToken) || result.resourceToken.resourceId !== resourceId || result.resourceToken.portId !== portId || result.resourceToken.access !== access) throw new Error('Resource token does not match the requested port and access.');
    return freezeClone(result.resourceToken);
  }
  async validate(resourceToken: ResourceToken, portId = resourceToken.portId, access: ResourceAccess = resourceToken.access): Promise<void> {
    if (!token(resourceToken) || resourceToken.portId !== portId || resourceToken.access !== access) throw new Error('Resource token does not match the requested port and access.');
    const result = await this.send({ command: 'profile_resources_validate', resourceToken: JSON.parse(JSON.stringify(resourceToken)), portId, access });
    if (!object(result) || !('valid' in result) || result.valid !== true) throw new Error('Resource token is stale; inspect the resource again.');
  }
}
