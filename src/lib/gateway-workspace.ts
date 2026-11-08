import { GatewayError, gatewayClientTag, gatewayRequest, type GatewayPort, type GatewayRequest, type GatewayState } from './gateway';
import { candidateBackend } from './gateway-automation';
import { observeWorkspaceState } from './workspace-read-models/shared';

export type GatewayEntry = {
  id: string;
  port: GatewayPort;
  state: GatewayState | null;
  error: string;
  busy: boolean;
  present: boolean;
  stale: boolean;
  loading?: boolean;
  autoConnectPaused?: boolean;
};
type Observation = Parameters<Parameters<typeof observeWorkspaceState>[0]>[0];
type Entry = GatewayEntry & { session: string; token: object; actions: number; nativeBusy: boolean;
  claimOwners: Record<string, string>; withdrawn: Set<string>; privateClaims: Set<string>; ownershipEpoch: number };
export type WorkspaceOptions = {
  request?: (request: GatewayRequest) => Promise<GatewayState>;
  observe?: typeof observeWorkspaceState;
  getClientTag?: () => Promise<string>;
  onChange?: (entries: GatewayEntry[], inventoryError: string) => void;
  setTimer?: (callback: () => void, milliseconds: number) => unknown;
  clearTimer?: (handle: unknown) => void;
};
const copy = <T>(value: T): T => JSON.parse(JSON.stringify(value)) as T;
const errorMessage = (error: unknown): string => error && typeof error === 'object' && 'message' in error && typeof error.message === 'string'
  ? error.message : String(error);
// A remounted workspace shares the same private client identity. An outstanding
// native renewal keeps its slot until settlement even after its owner disposes.
const clientRenewals = new Set<string>();

/** Gateway identity is independent of USB aliases and changing local endpoints. */
export function gatewayIdentity(port: GatewayPort): string | undefined {
  const backend = candidateBackend(port);
  if (!backend || !port.serialNumber) return;
  return `${backend}:${backend === 'stm32' ? port.serialNumber.toLowerCase() : port.serialNumber}`;
}

/** Passive native catalogue with explicitly requested, gateway-ordered operations. */
export class GatewayWorkspace {
  private readonly transport: (request: GatewayRequest) => Promise<GatewayState>;
  private readonly observe: typeof observeWorkspaceState;
  private readonly getClientTag: () => Promise<string>;
  private readonly onChange: (entries: GatewayEntry[], inventoryError: string) => void;
  private readonly setTimer: NonNullable<WorkspaceOptions['setTimer']>;
  private readonly clearTimer: NonNullable<WorkspaceOptions['clearTimer']>;
  private readonly gateways = new Map<string, Entry>();
  private readonly queues = new Map<string, Promise<void>>();
  private stop?: () => void;
  private started = false;
  private disposed = false;
  private ready = false;
  private clientTag?: string;
  private sourceKey?: string;
  private timer?: unknown;
  inventoryError = '';

  constructor(options: WorkspaceOptions = {}) {
    this.transport = options.request ?? gatewayRequest;
    this.observe = options.observe ?? observeWorkspaceState;
    this.getClientTag = options.getClientTag ?? gatewayClientTag;
    this.onChange = options.onChange ?? (() => {});
    this.setTimer = options.setTimer ?? ((callback, milliseconds) => setTimeout(callback, milliseconds));
    this.clearTimer = options.clearTimer ?? (handle => clearTimeout(handle as ReturnType<typeof setTimeout>));
  }

  get entries(): GatewayEntry[] {
    return [...this.gateways.values()].sort((a, b) => a.id.localeCompare(b.id)).map(entry => copy({
      id: entry.id, port: entry.port,
      state: !entry.present && entry.state ? { ...entry.state, connected: false, running: false } : entry.state,
      error: entry.error,
      busy: entry.busy, present: entry.present, stale: entry.stale, loading: entry.loading, autoConnectPaused: entry.autoConnectPaused,
    }));
  }

  start(): void {
    if (this.started || this.disposed) return;
    this.started = true;
    void this.getClientTag().then(tag => {
      if (this.disposed) return;
      this.clientTag = tag;
      for (const entry of this.gateways.values()) this.mapClaims(entry);
      this.changed(); this.manageLease();
    }, () => { /* Missing local identity cannot grant an ownership view. */ });
    try {
      const stop = this.observe(state => { if (!this.disposed) this.accept(state); });
      if (this.disposed) stop(); else this.stop = stop;
    } catch (error) { this.lost(errorMessage(error)); }
  }

  private changed(): void {
    if (!this.disposed) this.onChange(this.entries, this.inventoryError);
  }

  private mapClaims(entry: Entry): void {
    if (entry.state) entry.state.claimedModules = this.clientTag ? [...new Set([
      ...Object.keys(entry.claimOwners).filter(id => entry.claimOwners[id] === this.clientTag), ...entry.privateClaims,
    ])].filter(id => !entry.withdrawn.has(id)) : [];
  }

  private lost(error: string, loading = false): void {
    this.ready = false;
    this.inventoryError = error;
    for (const entry of this.gateways.values()) {
      entry.stale = true; entry.loading = loading; entry.error = error;
      // Resolving a paged replacement gates dispatch but does not invalidate
      // operations already sent for this native connection identity.
      if (!loading) { entry.token = {}; entry.actions = 0; entry.busy = false; }
    }
    this.manageLease(); this.changed();
  }

  private accept(source: Observation): void {
    if (source.status !== 'ready' || !source.value) {
      this.lost(source.error || (source.status === 'connecting' ? 'Connecting to the gateway workspace.' : 'Gateway workspace observations are unavailable.'), source.loading === true);
      return;
    }
    const key = JSON.stringify([source.clockId, source.session, source.generation]);
    const replaced = this.sourceKey !== key;
    this.sourceKey = key; this.ready = true;
    this.inventoryError = source.value.inventoryError;
    const retained = new Set<string>();
    for (const row of source.value.entries) {
      retained.add(row.id);
      let entry = this.gateways.get(row.id);
      const same = entry && !replaced && entry.session === row.session;
      if (!same) {
        entry = { ...copy(row), state: null, stale: false, busy: false, nativeBusy: false, actions: 0,
          token: {}, claimOwners: {}, withdrawn: new Set(), privateClaims: new Set(), ownershipEpoch: 0, session: row.session } as Entry;
        this.gateways.set(row.id, entry);
      }
      entry = entry!;
      entry.port = copy(row.port) as GatewayPort;
      entry.state = row.state ? copy(row.state) as GatewayState : null;
      entry.present = row.present; entry.error = row.error; entry.stale = false; entry.loading = false;
      entry.nativeBusy = row.busy; entry.busy = entry.nativeBusy || entry.actions > 0;
      entry.autoConnectPaused = row.autoConnectPaused;
      const owners = { ...row.claimOwners };
      if (JSON.stringify(Object.entries(entry.claimOwners).sort()) !== JSON.stringify(Object.entries(owners).sort())) entry.ownershipEpoch++;
      entry.claimOwners = owners;
      // A new public revision can carry the same expired owner hash. Only an
      // observed owner absence (then presence) or private claim acknowledgment
      // establishes another claim epoch; revision churn cannot restore a lease.
      for (const id of entry.withdrawn) if (entry.claimOwners[id] !== this.clientTag) entry.withdrawn.delete(id);
      for (const id of entry.privateClaims) {
        const owner = entry.claimOwners[id];
        if (owner) entry.privateClaims.delete(id); // Confirmed locally, or superseded by another owner.
      }
      this.mapClaims(entry);
    }
    for (const id of this.gateways.keys()) if (!retained.has(id)) this.gateways.delete(id);
    this.manageLease(); this.changed();
  }

  private current(entry: Entry, token: object): boolean {
    return !this.disposed && this.gateways.get(entry.id) === entry && entry.token === token;
  }

  request(gatewayId: string, request: GatewayRequest, background = false): Promise<GatewayState> {
    const entry = this.gateways.get(gatewayId);
    if (!entry || this.disposed || !this.ready || entry.stale) return Promise.reject(new GatewayError('Wait for a current gateway workspace observation.'));
    if (request.gatewayId && request.gatewayId !== gatewayId) return Promise.reject(new GatewayError('The request targets a different gateway.'));
    const token = entry.token;
    if (!background) { entry.actions++; entry.busy = true; this.changed(); }
    const previous = this.queues.get(gatewayId) ?? Promise.resolve();
    const pending = previous.then(async () => {
      if (!this.current(entry, token) || !this.ready || entry.stale) throw new GatewayError('Wait for a current gateway workspace observation before dispatch.');
      try {
        const receipt = await this.transport({ ...request, gatewayId });
        if (!this.current(entry, token)) throw new GatewayError('The gateway workspace session changed during the request.');
        if (receipt.gatewayId === entry.id && request.moduleId && Array.isArray(receipt.claimedModules)) {
          const moduleId = request.moduleId;
          if (request.command === 'claim' && receipt.claimedModules.includes(moduleId) && this.clientTag) {
            entry.withdrawn.delete(moduleId); entry.privateClaims.add(moduleId); entry.ownershipEpoch++;
          } else if (request.command === 'release' && !receipt.claimedModules.includes(moduleId)) {
            entry.privateClaims.delete(moduleId); entry.withdrawn.add(moduleId); entry.ownershipEpoch++;
          }
          this.mapClaims(entry); this.manageLease(); this.changed();
        }
        // Receipts belong only to their caller (for example a configuration dialog).
        // Native observations are the sole source of catalogue and pause state.
        return receipt;
      } catch (error) {
        if (this.current(entry, token) && this.ready) { entry.error = errorMessage(error); this.changed(); }
        throw error;
      }
    });
    const settled = pending.then(() => {}, () => {});
    this.queues.set(gatewayId, settled);
    void settled.then(() => {
      if (this.queues.get(gatewayId) === settled) this.queues.delete(gatewayId);
      if (this.current(entry, token) && !background) { entry.actions--; entry.busy = entry.nativeBusy || entry.actions > 0; this.changed(); }
    });
    return pending;
  }

  async retry(gatewayId: string): Promise<void> {
    const entry = this.gateways.get(gatewayId);
    if (!entry) throw new GatewayError('This gateway is no longer available.');
    await this.request(gatewayId, { command: 'connect', backend: entry.port.backend, port: entry.port.path });
  }

  async disconnect(gatewayId: string): Promise<void> {
    await this.request(gatewayId, { command: 'disconnect' });
  }

  private ownClaims(): Entry[] {
    return !this.disposed ? [...this.gateways.values()].filter(entry => entry.state?.claimedModules?.length) : [];
  }

  private manageLease(): void {
    if (!this.ownClaims().length) {
      if (this.timer !== undefined) this.clearTimer(this.timer);
      this.timer = undefined;
    } else if (this.timer === undefined) {
      this.timer = this.setTimer(() => {
        this.timer = undefined;
        for (const entry of this.ownClaims()) if (!clientRenewals.has(this.renewalKey(entry))) void this.renewLease(entry);
        this.manageLease();
      }, 1000);
    }
  }

  private renewalKey(entry: Entry): string { return JSON.stringify([this.clientTag, entry.id]); }

  private async renewLease(entry: Entry): Promise<void> {
    const token = entry.token, ownershipEpoch = entry.ownershipEpoch, key = this.renewalKey(entry);
    const withdraw = (retained: Set<string>): void => {
      if (!this.current(entry, token) || ownershipEpoch !== entry.ownershipEpoch) return;
      for (const id of entry.state?.claimedModules ?? []) if (!retained.has(id)) {
        entry.withdrawn.add(id); entry.privateClaims.delete(id);
      }
      this.mapClaims(entry); this.changed();
    };
    // Native workspace bounds the catalogue to 64 gateways. Each exact gateway
    // has at most one renewal in flight, so a slow gateway cannot starve others.
    clientRenewals.add(key);
    try {
      // A live configuring client must retain its already-owned native lease while
      // idle. This timer cannot acquire claims, connect, start, or activate output.
      const result = await this.transport({ command: 'workspace_client_renew', gatewayId: entry.id });
      const renewal = result.workspaceRenewal;
      withdraw(new Set(renewal?.gatewayId === entry.id && Array.isArray(renewal.claimedModules) ? renewal.claimedModules : []));
    } catch { withdraw(new Set()); }
    finally { clientRenewals.delete(key); this.manageLease(); }
  }

  dispose(): void {
    if (this.disposed) return;
    this.disposed = true; this.ready = false;
    this.stop?.(); this.stop = undefined;
    if (this.timer !== undefined) this.clearTimer(this.timer);
    this.timer = undefined;
  }
}
