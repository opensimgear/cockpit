/** Shared, read-only system inventory. Provider identities and routes remain authoritative. */
export type SystemType = 'host' | 'controller' | 'osg' | 'gateway' | 'virtual' | 'integration' | 'companion';
export type SystemPresence = 'connected' | 'offline' | 'unknown' | 'unavailable';
export type SystemConnection = { id: string; from: string; to: string; transport: 'usb' | 'can' | 'network' | 'software' | 'unknown'; current: boolean; endpoint?: string };
export type SystemReference = { kind: string; id: string; name: string };
export type SystemTarget = { kind: 'module' | 'gateway' | 'controller' | 'usb' | 'integration' | 'companion' | 'virtual' | 'visibility'; id: string; gatewayId?: string };
export type SystemComponent = {
  id: string; name: string; type: SystemType; role: string; presence: SystemPresence;
  hidden: boolean | null; enabled: boolean | null; owner: string | null;
  capture: 'ready' | 'waiting' | 'unavailable' | 'unknown'; status: string;
  diagnostics: string[]; connections: SystemConnection[]; targets: SystemTarget[];
  evidence: string[]; references: SystemReference[]; firstSeen: number; lastSeen: number;
  observedAt: number; provider: string; stale: boolean;
  forget: { allowed: boolean; reason: string; effect: string };
};
export type SystemSnapshot = { components: SystemComponent[]; diagnostics: string[]; observedAt: number };
export type SystemObservation = Omit<SystemComponent, 'firstSeen' | 'lastSeen'>;
export type InventoryMemory = { version: 1; components: SystemComponent[]; forgotten: Record<string, { signature: string; sawAbsent: boolean }> };
export type InventoryStorage = { getItem(key: string): string | null; setItem(key: string, value: string): void };
export const INVENTORY_STORAGE_KEY = 'osg-system-inventory-v1';
export const systemTypeLabels: Record<SystemType, string> = { host: 'Computer', controller: 'Physical · Third-party', osg: 'Physical · OSG', gateway: 'Gateway · OSG', virtual: 'Virtual', integration: 'Integration', companion: 'Companion' };
export const presenceLabels: Record<SystemPresence, string> = { connected: 'Connected', offline: 'Offline', unknown: 'Unknown', unavailable: 'Unavailable' };
export function systemStateLabel(component: SystemComponent): string {
  return [presenceLabels[component.presence], component.hidden === true ? 'Hidden via HidHide' : '', component.enabled === false ? 'Disabled' : '', component.stale ? 'Stale observation' : '', component.status].filter(Boolean).join(' · ');
}
export function filterSystem(components: readonly SystemComponent[], search: string, filter: string): SystemComponent[] {
  const query = search.trim().toLocaleLowerCase();
  return components.filter(c => c.type !== 'host' && `${c.name} ${c.role} ${c.id} ${systemTypeLabels[c.type]} ${c.status} ${c.connections.map(x => `${x.transport} ${x.endpoint ?? ''}`).join(' ')}`.toLocaleLowerCase().includes(query)
    && (filter === 'all' || filter === c.type || filter === c.presence || filter === 'hidden' && c.hidden === true));
}
function signature(c: SystemComponent | SystemObservation): string {
  // Report counters, timestamps and polling generations cannot resurrect a forgotten record.
  return JSON.stringify([c.presence, c.evidence.slice().sort(), c.connections.map(x => [x.from, x.to, x.transport, x.current, x.endpoint]).sort()]);
}
function isComponent(value: unknown): value is SystemComponent {
  const c = value as SystemComponent;
  return !!c && typeof c.id === 'string' && typeof c.name === 'string' && c.type in systemTypeLabels && c.presence in presenceLabels
    && Array.isArray(c.connections) && Array.isArray(c.targets) && Array.isArray(c.evidence) && Array.isArray(c.references) && Array.isArray(c.diagnostics)
    && typeof c.firstSeen === 'number' && typeof c.lastSeen === 'number' && typeof c.observedAt === 'number' && typeof c.provider === 'string' && !!c.forget;
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
      if (saved?.version === 1 && Array.isArray(saved.components) && saved.components.every(isComponent) && saved.forgotten && typeof saved.forgotten === 'object') this.memory = saved;
    } catch { this.storageError = 'Remembered inventory could not be loaded.'; }
    this.snapshot = { components: this.memory.components.map(c => this.remembered(c)), diagnostics: this.storageError ? [this.storageError] : [], observedAt: 0 };
  }
  private remembered(c: SystemComponent): SystemComponent {
    return { ...c, presence: 'unknown', stale: true, owner: null, capture: 'unknown', connections: c.connections.map(x => ({ ...x, current: false })), forget: { ...c.forget, allowed: false, reason: 'Refresh this provider before forgetting a remembered component.' } };
  }
  update(observations: SystemObservation[], completeProviders: string[], diagnostics: string[] = [], now = Date.now()): SystemSnapshot {
    const previous = new Map(this.memory.components.map(c => [c.id, c]));
    const seen = new Set<string>();
    const next: SystemComponent[] = [];
    for (const observation of observations) {
      if (seen.has(observation.id)) throw new Error('System inventory contains duplicate logical identities.');
      seen.add(observation.id);
      const tombstone = this.memory.forgotten[observation.id];
      if (tombstone) {
        if (signature(observation) === tombstone.signature && !tombstone.sawAbsent || observation.presence !== 'connected') continue;
        delete this.memory.forgotten[observation.id];
      }
      const old = previous.get(observation.id);
      next.push({ ...observation, firstSeen: old?.firstSeen ?? now, lastSeen: observation.presence === 'connected' ? now : old?.lastSeen ?? now });
    }
    for (const c of previous.values()) if (!seen.has(c.id) && !this.memory.forgotten[c.id]) {
      const complete = completeProviders.includes(c.provider);
      const remembered = this.remembered(c);
      if (complete) {
        remembered.presence = 'offline'; remembered.stale = false; remembered.observedAt = now;
        remembered.forget = { allowed: true, reason: '', effect: 'Forget this local remembered record. Remote storage and user bindings are retained. It can reappear when observed again.' };
      }
      next.push(remembered);
    }
    for (const [id, t] of Object.entries(this.memory.forgotten)) if (!seen.has(id)) t.sawAbsent = true;
    this.memory.components = next;
    this.persist();
    this.snapshot = { components: next, diagnostics: [...diagnostics, ...(this.storageError ? [this.storageError] : [])], observedAt: now };
    return this.snapshot;
  }
  forget(id: string): void {
    const c = this.snapshot.components.find(x => x.id === id);
    if (!c?.forget.allowed || c.presence === 'connected' || c.owner || c.hidden === true) throw new Error(c?.forget.reason || 'This component is currently present or owned. Refresh before forgetting it.');
    this.memory.forgotten[id] = { signature: signature(c), sawAbsent: false };
    this.memory.components = this.memory.components.filter(x => x.id !== id);
    this.persist();
    if (this.storageError) throw new Error(this.storageError);
    this.snapshot = { ...this.snapshot, components: this.memory.components };
  }
  private persist() {
    try { this.storage?.setItem(INVENTORY_STORAGE_KEY, JSON.stringify(this.memory)); this.storageError = ''; }
    catch { this.storageError = 'Inventory changes could not be saved. Durable cleanup is unavailable.'; }
  }
}
