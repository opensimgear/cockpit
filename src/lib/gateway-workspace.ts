import { GatewayError, gatewayRequest, type GatewayPort, type GatewayRequest, type GatewayState } from './gateway';
import { candidateBackend, gatewayCandidates, nextConfigurationRead } from './gateway-automation';
import { supportsDeviceContracts } from './gateway-capabilities';
import { nextActionRead } from './device-actions';

export type GatewayEntry = {
  id: string;
  port: GatewayPort;
  state: GatewayState | null;
  error: string;
  busy: boolean;
  present: boolean;
};
type Entry = GatewayEntry & {
  operation: Promise<void> | null;
  actions: number;
  failures: number;
  retryAt: number;
  autoStartFailed: boolean;
};
type WorkspaceOptions = {
  request?: (request: GatewayRequest) => Promise<GatewayState>;
  onChange?: (entries: GatewayEntry[], inventoryError: string) => void;
  now?: () => number;
};

/** Gateway identity is independent of USB aliases and changing local endpoints. */
export function gatewayIdentity(port: GatewayPort): string | undefined {
  const backend = candidateBackend(port);
  if (!backend || !port.serialNumber) return;
  return `${backend}:${backend === 'stm32' ? port.serialNumber.toLowerCase() : port.serialNumber}`;
}

/** Own independent connections; selecting a device never changes transport routing. */
export class GatewayWorkspace {
  private readonly transport: (request: GatewayRequest) => Promise<GatewayState>;
  private readonly onChange: (entries: GatewayEntry[], inventoryError: string) => void;
  private readonly now: () => number;
  private readonly gateways = new Map<string, Entry>();
  private discovery: Promise<void> | null = null;
  private disposed = false;
  inventoryError = '';

  constructor(options: WorkspaceOptions = {}) {
    this.transport = options.request ?? gatewayRequest;
    this.onChange = options.onChange ?? (() => {});
    this.now = options.now ?? Date.now;
  }

  get entries(): GatewayEntry[] {
    return [...this.gateways.values()].sort((a, b) => a.id.localeCompare(b.id)).map(entry => ({
      id: entry.id, port: { ...entry.port }, state: entry.state ? { ...entry.state } : null,
      error: entry.error, busy: entry.busy, present: entry.present,
    }));
  }

  private changed() {
    if (!this.disposed) this.onChange(this.entries, this.inventoryError);
  }

  private async discover() {
    try {
      const inventory = await this.transport({ command: 'ports' });
      if (this.disposed) return;
      this.inventoryError = '';
      const previouslyPresent = new Set([...this.gateways.values()].filter(entry => entry.present).map(entry => entry.id));
      for (const entry of this.gateways.values()) entry.present = false;
      for (const port of gatewayCandidates(inventory.ports)) {
        const id = gatewayIdentity(port)!;
        const existing = this.gateways.get(id);
        if (existing) {
          if (!previouslyPresent.has(id) || existing.port.path !== port.path) existing.retryAt = 0;
          existing.present = true;
          existing.port = { ...port, gatewayId: id };
        } else {
          this.gateways.set(id, { id, port: { ...port, gatewayId: id }, state: null,
            error: '', busy: false, present: true, operation: null, actions: 0,
            failures: 0, retryAt: 0, autoStartFailed: false });
        }
      }
      // Previously connected gateways retain their saved registration and last-known
      // devices. A candidate which vanished before connecting has nothing to retain.
      for (const [id, entry] of this.gateways) if (!entry.present && !entry.state && !entry.operation) this.gateways.delete(id);
    } catch (error) {
      if (!this.disposed) this.inventoryError = error instanceof Error ? error.message : String(error);
    }
    this.changed();
  }

  /** Safe to call every poll interval, even while a different gateway is slow. */
  async refresh(): Promise<void> {
    if (this.disposed) return;
    if (!this.discovery) {
      const discovery = this.discover();
      this.discovery = discovery;
      void discovery.finally(() => { if (this.discovery === discovery) this.discovery = null; });
    }
    await this.discovery;
    if (this.disposed) return;
    await Promise.allSettled([...this.gateways.values()].map(entry => {
      if (entry.operation) return Promise.resolve();
      return this.schedule(entry, () => this.poll(entry), false);
    }));
  }

  private schedule<T>(entry: Entry, operation: () => Promise<T>, action: boolean): Promise<T> {
    if (action) { entry.actions++; entry.busy = true; this.changed(); }
    const previous = entry.operation ?? Promise.resolve();
    const pending = previous.then(() => {
      if (this.disposed) throw new GatewayError('The gateway workspace is closed.');
      return operation();
    });
    const settled = pending.then(() => {}, () => {});
    entry.operation = settled;
    void settled.then(() => {
      if (entry.operation === settled) entry.operation = null;
      if (action) { entry.actions--; entry.busy = entry.actions > 0; }
      this.changed();
    });
    return pending;
  }

  private accept(entry: Entry, state: GatewayState) {
    if (state.gatewayId !== entry.id) throw new GatewayError('The gateway response belongs to a different gateway.');
    entry.state = { ...state };
    entry.error = '';
    if (state.connected) { entry.failures = 0; entry.retryAt = 0; }
    this.changed();
  }

  private failed(entry: Entry, error: unknown) {
    const returned = error instanceof GatewayError ? error.state : undefined;
    if (returned?.gatewayId === entry.id) entry.state = { ...returned };
    else if (entry.state) entry.state = { ...entry.state, connected: false, running: false,
      claimedModules: [], modules: entry.state.modules.map(module => ({ ...module, online: false, active: false,
        buttons: [], axes: [], hats: [] })) };
    entry.error = error instanceof Error ? error.message : String(error);
    if (!entry.state?.connected) {
      entry.failures++;
      entry.retryAt = this.now() + Math.min(30_000, 1000 * 2 ** Math.min(5, entry.failures - 1));
    }
    this.changed();
  }

  private async send(entry: Entry, request: GatewayRequest) {
    try {
      const state = await this.transport({ ...request, gatewayId: entry.id });
      if (!this.disposed) this.accept(entry, state);
      return state;
    } catch (error) {
      if (!this.disposed) this.failed(entry, error);
      throw error;
    }
  }

  private async poll(entry: Entry): Promise<void> {
    if (!entry.state?.connected) {
      if (!entry.present || this.now() < entry.retryAt) return;
      entry.busy = true; this.changed();
      try { await this.send(entry, { command: 'connect', backend: entry.port.backend, port: entry.port.path }); }
      finally { entry.busy = entry.actions > 0; this.changed(); }
    } else await this.send(entry, { command: 'status' });
    const state = entry.state;
    if (!state?.connected || state.reconnecting) return;
    // Claims on one gateway never suspend polling or configuration on another.
    const claimed = !!state.claimedModules?.length;
    if (state.backend === 'pico' && !supportsDeviceContracts(state) && !claimed && !state.running && !state.needsReconnect &&
        !entry.autoStartFailed && state.modules.some(module => module.online && module.registered)) {
      entry.busy = true; this.changed();
      try { await this.send(entry, { command: 'start' }); }
      catch (error) { entry.autoStartFailed = true; throw error; }
      finally { entry.busy = entry.actions > 0; this.changed(); }
    }
    const unread = nextConfigurationRead(entry.state, claimed);
    if (unread) await this.send(entry, { command: 'config_read', moduleId: unread });
    const actionUnread = nextActionRead(entry.state);
    if (actionUnread) await this.send(entry, { command: 'action_describe', moduleId: actionUnread });
  }

  request(gatewayId: string, request: GatewayRequest, background = false): Promise<GatewayState> {
    const entry = this.gateways.get(gatewayId);
    if (!entry) return Promise.reject(new GatewayError('This gateway is no longer available.'));
    if (request.gatewayId && request.gatewayId !== gatewayId) return Promise.reject(new GatewayError('The request targets a different gateway.'));
    return this.schedule(entry, () => this.send(entry, request), !background);
  }

  async retry(gatewayId: string): Promise<void> {
    const entry = this.gateways.get(gatewayId);
    if (!entry) return;
    entry.retryAt = 0; entry.failures = 0; entry.autoStartFailed = false;
    await this.schedule(entry, () => this.poll(entry), true);
  }

  dispose() { this.disposed = true; }
}
