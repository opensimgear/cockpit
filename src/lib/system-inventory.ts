/** Shared, read-only system inventory. Provider identities and routes remain authoritative. */
export type SystemType = 'host' | 'controller' | 'osg' | 'gateway' | 'virtual' | 'integration' | 'companion';
export type SystemPresence = 'connected' | 'offline' | 'unknown' | 'unavailable';
export type SystemConnection = { id: string; from: string; to: string; transport: 'usb' | 'can' | 'network' | 'bluetooth' | 'software' | 'unknown'; current: boolean; observed?: boolean; endpoint?: string };
export type SystemReference = { kind: string; id: string; name: string };
export type SystemTarget = { kind: 'module' | 'gateway' | 'controller' | 'usb' | 'integration' | 'companion' | 'virtual' | 'visibility'; id: string; gatewayId?: string };
export type SystemComponent = {
  id: string; name: string; type: SystemType; role: string; presence: SystemPresence;
  hidden: boolean | null; enabled: boolean | null; owner: string | null;
  inputOwner?: string | null; configurationOwner?: string | null; configurationRoute?: string | null;
  capture: 'ready' | 'waiting' | 'unavailable' | 'unknown'; status: string;
  diagnostics: string[]; connections: SystemConnection[]; targets: SystemTarget[];
  evidence: string[]; references: SystemReference[]; firstSeen: number; lastSeen: number;
  observedAt: number; provider: string; stale: boolean;
  forget: { allowed: boolean; reason: string; effect: string };
};
export type SystemSnapshot = { components: SystemComponent[]; diagnostics: string[]; observedAt: number };
export type SystemObservation = Omit<SystemComponent, 'firstSeen' | 'lastSeen'>;
type InventoryTombstone = { signature: string; sawAbsent: boolean; type?: SystemType; provider?: string };
export type InventoryMemory = { version: 1; components: SystemComponent[]; forgotten: Record<string, InventoryTombstone> };
export type InventoryStorage = { getItem(key: string): string | null; setItem(key: string, value: string): void };
export const INVENTORY_STORAGE_KEY = 'osg-system-inventory-v1';
export const systemTypeLabels: Record<SystemType, string> = { host: 'Computer', controller: 'Physical · Third-party', osg: 'Physical · OSG', gateway: 'Gateway · OSG', virtual: 'Virtual', integration: 'Integration', companion: 'Companion' };
export const presenceLabels: Record<SystemPresence, string> = { connected: 'Connected', offline: 'Offline', unknown: 'Unknown', unavailable: 'Unavailable' };
export function systemStateLabel(component: SystemComponent): string {
  return [presenceLabels[component.presence], component.hidden === true ? 'Hidden via HidHide' : '', component.enabled === false ? 'Disabled' : '', component.stale ? 'Stale observation' : '', component.status].filter(Boolean).join(' · ');
}
export function filterSystem(components: readonly SystemComponent[], search: string, filter: string, transport = 'all'): SystemComponent[] {
  const query = search.trim().toLocaleLowerCase();
  return components.filter(c => isVisibleSystemComponent(c) && `${c.name} ${c.role} ${c.id} ${systemTypeLabels[c.type]} ${c.status} ${c.connections.map(x => `${x.transport} ${x.endpoint ?? ''}`).join(' ')}`.toLocaleLowerCase().includes(query)
    && (filter === 'all' || filter === c.type || filter === c.presence || filter === 'hidden' && c.hidden === true)
    && (transport === 'all' || c.connections.some(connection => connection.transport === transport) || transport === 'unknown' && !c.connections.length));
}
/** Panels belong to their companion; integrations appear when the applied profile uses them. */
export function isVisibleSystemComponent(component: SystemComponent): boolean {
  return component.type !== 'host' && !component.id.startsWith('companion-instance:')
    && (component.type !== 'integration' || component.evidence.includes('active-bindings') && !component.stale);
}
function signature(c: SystemComponent | SystemObservation): string {
  // Report counters, timestamps and polling generations cannot resurrect a forgotten record.
  return JSON.stringify([c.presence, c.evidence.slice().sort(), c.connections.map(x => [x.from, x.to, x.transport, x.current, x.endpoint]).sort()]);
}
function isComponent(value: unknown): value is SystemComponent {
  const c = value as SystemComponent;
  return !!c && typeof c.id === 'string' && typeof c.name === 'string' && Object.hasOwn(systemTypeLabels, c.type) && Object.hasOwn(presenceLabels, c.presence)
    && Array.isArray(c.connections) && Array.isArray(c.targets) && Array.isArray(c.evidence) && Array.isArray(c.references) && Array.isArray(c.diagnostics)
    && typeof c.firstSeen === 'number' && typeof c.lastSeen === 'number' && typeof c.observedAt === 'number' && typeof c.provider === 'string' && !!c.forget;
}
const cloneMemory = (memory: InventoryMemory): InventoryMemory => JSON.parse(JSON.stringify(memory));
const tombstoneFor = (component: SystemComponent): InventoryTombstone => ({ signature: signature(component), sawAbsent: false, type: component.type, provider: component.provider });
const hasTombstone = (memory: InventoryMemory, id: string) => Object.hasOwn(memory.forgotten, id);
function rememberTombstone(memory: InventoryMemory, id: string, component: SystemComponent) {
  Object.defineProperty(memory.forgotten, id, { value: tombstoneFor(component), enumerable: true, writable: true, configurable: true });
}
function validTombstones(value: unknown): value is InventoryMemory['forgotten'] {
  return !!value && typeof value === 'object' && !Array.isArray(value) && Object.values(value).every(tombstone => tombstone && typeof tombstone === 'object'
    && typeof tombstone.signature === 'string' && typeof tombstone.sawAbsent === 'boolean'
    && (tombstone.type === undefined || Object.hasOwn(systemTypeLabels, tombstone.type)) && (tombstone.provider === undefined || typeof tombstone.provider === 'string'));
}
function forgottenGateways(memory: InventoryMemory): Set<string> {
  return new Set(Object.entries(memory.forgotten).filter(([id, tombstone]) => tombstone.type === 'gateway' || id.startsWith('gateway:')).map(([id]) => id));
}
/** Remove only routes owned by forgotten gateways. A correlated USB/HID sighting
 * keeps the physical component and all of its independent routes and references. */
function pruneGatewayRoutes<T extends SystemObservation>(components: readonly T[], gateways: Set<string>): { components: T[]; removed: T[] } {
  const suppressed = new Set(gateways), removed = new Map<string, T>();
  let result = [...components];
  for (let changed = true; changed;) {
    changed = false;
    result = result.flatMap(component => {
      if (suppressed.has(component.id)) { removed.set(component.id, component); return []; }
      const connections = component.connections.filter(connection => !suppressed.has(connection.from) && !suppressed.has(connection.to));
      const targets = component.targets.filter(target => !target.gatewayId || !gateways.has(`gateway:${target.gatewayId}`) && !gateways.has(target.gatewayId));
      if (connections.length === component.connections.length && targets.length === component.targets.length) return [component];
      if (!connections.length && !targets.length) {
        suppressed.add(component.id); removed.set(component.id, component); changed = true; return [];
      }
      const evidence = component.evidence.filter(evidence => ![...gateways].some(id => evidence.startsWith(`route:${id.startsWith('gateway:') ? id.slice(8) : id}/`)));
      const provider = component.provider === 'gateways' && !targets.some(target => target.kind === 'module')
        ? targets.some(target => target.kind === 'usb') ? 'usb' : targets.some(target => target.kind === 'controller') ? 'controllers' : component.provider
        : component.provider;
      return [{ ...component, connections, targets, evidence, provider }];
    });
  }
  return { components: result, removed: [...removed.values()] };
}
/** Durable remembered records and tombstones; providers still perform their own guarded mutations. */
export class SystemInventoryRegistry {
  private memory: InventoryMemory = { version: 1, components: [], forgotten: {} };
  private storage?: InventoryStorage;
  private storageError = '';
  snapshot: SystemSnapshot = { components: [], diagnostics: [], observedAt: 0 };
  constructor(storage?: InventoryStorage) {
    this.storage = storage;
    try {
      const saved = JSON.parse(storage?.getItem(INVENTORY_STORAGE_KEY) ?? 'null');
      if (saved?.version === 1 && Array.isArray(saved.components) && saved.components.every(isComponent) && validTombstones(saved.forgotten)) this.memory = saved;
    } catch { this.storageError = 'Remembered inventory could not be loaded.'; }
    this.snapshot = { components: this.memory.components.map(c => this.remembered(c)), diagnostics: this.storageError ? [this.storageError] : [], observedAt: 0 };
  }
  private remembered(c: SystemComponent): SystemComponent {
    return { ...c, presence: 'unknown', stale: true, owner: null, inputOwner: null, configurationOwner: null, configurationRoute: null, capture: 'unknown', connections: c.connections.map(x => ({ ...x, current: false })), forget: { ...c.forget, allowed: false, reason: 'Refresh this provider before forgetting a remembered component.' } };
  }
  update(observations: SystemObservation[], completeProviders: string[], diagnostics: string[] = [], now = Date.now(), aliases: Record<string, string> = {}): SystemSnapshot {
    if (new Set(observations.map(c => c.id)).size !== observations.length) throw new Error('System inventory contains duplicate logical identities.');
    const candidate = cloneMemory(this.memory);
    // Positive provider identity evidence supersedes remembered sightings. This
    // preserves observation history without retaining a second stale physical node.
    for (const [from, to] of Object.entries(aliases)) {
      if (from === to) continue;
      const old = candidate.components.find(c => c.id === from), destination = candidate.components.find(c => c.id === to);
      if (old && destination) { destination.firstSeen = Math.min(old.firstSeen, destination.firstSeen); destination.lastSeen = Math.max(old.lastSeen, destination.lastSeen); destination.references = [...destination.references, ...old.references]; }
      else if (old) candidate.components.push({ ...old, id: to });
      candidate.components = candidate.components.filter(c => c.id !== from);
      if (candidate.forgotten[from] && !candidate.forgotten[to]) candidate.forgotten[to] = candidate.forgotten[from];
      delete candidate.forgotten[from];
    }
    const previous = new Map(candidate.components.map(c => [c.id, c]));
    const seen = new Set(observations.map(c => c.id));
    // Positive current observations can restore forgotten records. Timestamp or
    // polling changes, failures and repeated offline data never count as reconnects.
    for (const observation of observations) {
      if (!hasTombstone(candidate, observation.id)) continue;
      const tombstone = candidate.forgotten[observation.id];
      if (observation.presence === 'connected' && (signature(observation) !== tombstone.signature || tombstone.sawAbsent)) delete candidate.forgotten[observation.id];
    }
    const pruned = pruneGatewayRoutes(observations, forgottenGateways(candidate));
    const next: SystemComponent[] = [];
    for (const observation of pruned.components) {
      if (hasTombstone(candidate, observation.id)) continue;
      const old = previous.get(observation.id);
      next.push({ ...observation, evidence: [...new Set([...(old?.evidence.filter(e => e.startsWith('binding-source:')) ?? []), ...observation.evidence])], firstSeen: old?.firstSeen ?? now, lastSeen: observation.presence === 'connected' ? now : old?.lastSeen ?? 0 });
    }
    for (const c of previous.values()) if (!seen.has(c.id) && !hasTombstone(candidate, c.id)) {
      const complete = completeProviders.includes(c.provider);
      const remembered = this.remembered(c);
      if (complete) {
        remembered.presence = 'offline'; remembered.stale = false; remembered.observedAt = now;
        remembered.forget = { allowed: true, reason: '', effect: 'Forget this local remembered record. Remote storage and user bindings are retained. It can reappear when observed again.' };
      }
      next.push(remembered);
    }
    for (const [id, tombstone] of Object.entries(candidate.forgotten)) if (!seen.has(id) && (!tombstone.provider || completeProviders.includes(tombstone.provider))) tombstone.sawAbsent = true;
    candidate.components = pruneGatewayRoutes(next, forgottenGateways(candidate)).components;
    this.storageError = this.persist(candidate);
    // Live inspection remains usable if remembering it fails. Destructive
    // cleanup below requires persistence to succeed before publishing anything.
    this.memory = candidate;
    this.snapshot = { components: candidate.components, diagnostics: [...diagnostics, ...(this.storageError ? [this.storageError] : [])], observedAt: now };
    return this.snapshot;
  }
  forget(id: string): void {
    const c = this.snapshot.components.find(x => x.id === id);
    if (!c?.forget.allowed || c.presence === 'connected' || c.owner || c.hidden === true) throw new Error(c?.forget.reason || 'This component is currently present or owned. Refresh before forgetting it.');
    const candidate = cloneMemory(this.memory);
    rememberTombstone(candidate, id, c);
    const original = new Map(candidate.components.map(component => [component.id, component]));
    const pruned = c.type === 'gateway' ? pruneGatewayRoutes(candidate.components, forgottenGateways(candidate)) : { components: candidate.components.filter(component => component.id !== id), removed: [] };
    for (const child of pruned.removed) if (child.id !== id) {
      if (child.presence === 'connected' || child.owner || child.hidden === true) throw new Error('A connected, hidden or owned child still depends on this gateway. Refresh and release it before forgetting the gateway.');
      rememberTombstone(candidate, child.id, original.get(child.id)!);
    }
    candidate.components = pruned.components;
    const error = this.persist(candidate);
    if (error) throw new Error(error);
    this.memory = candidate;
    this.snapshot = { ...this.snapshot, components: candidate.components, diagnostics: this.snapshot.diagnostics.filter(diagnostic => !this.storageError || diagnostic !== this.storageError) };
    this.storageError = '';
  }
  private persist(memory: InventoryMemory): string {
    try { this.storage?.setItem(INVENTORY_STORAGE_KEY, JSON.stringify(memory)); return ''; }
    catch { return 'Inventory changes could not be saved. Durable cleanup is unavailable.'; }
  }
}
