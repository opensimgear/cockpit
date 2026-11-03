import { SharedObservations, type ObservationState } from '../streams/shared.ts';
import type { WireFrame } from '../streams/protocol.ts';
import type { GatewayEntry } from '../gateway-workspace.ts';
import type { GatewayPort } from '../gateway.ts';
import type { DeepReadonly } from '../profiles.ts';
import { GamePages, digestId, record, valid, type GamePageGetter } from '../game-read-models/pages.ts';

export type WorkspaceEntry = GatewayEntry & { session: string; claimOwners: Record<string, string> };
export type WorkspaceModel = { ports: GatewayPort[]; inventoryError: string; entries: WorkspaceEntry[] };
export type WorkspaceModelState = Readonly<{
  status: 'connecting' | 'ready' | 'lost' | 'error'; value?: DeepReadonly<WorkspaceModel>; error?: string; loading?: boolean;
  generation?: string; revision?: string; clockId?: string; session?: string;
}>;
export type WorkspaceListener = (state: WorkspaceModelState) => void;
const text = (value: unknown, max = 256): value is string => typeof value === 'string' && value.length <= max;
const identity = (value: unknown): value is string => text(value) && /^(?:pico|stm32):[A-Za-z0-9_-]{1,128}$/.test(value);
const normalized = new WeakMap<object, DeepReadonly<WorkspaceEntry>>();
function port(value: unknown): asserts value is GatewayPort {
  valid(record(value) && text(value.path, 32768) && value.path.length > 0
    && (value.backend === undefined || value.backend === 'pico' || value.backend === 'stm32')
    && (value.gatewayId === undefined || identity(value.gatewayId)));
  for (const key of ['manufacturer', 'friendlyName', 'serialNumber', 'vendorId', 'productId']) valid(value[key] === undefined || text(value[key], 1024));
}
/** The page engine has already validated, bounded and frozen the complete JSON tree. */
export function captureWorkspaceModel(value: unknown): DeepReadonly<WorkspaceModel> {
  valid(record(value) && Object.keys(value).length === 3 && Array.isArray(value.ports) && value.ports.length <= 64
    && text(value.inventoryError, 16384) && Array.isArray(value.entries) && value.entries.length <= 64);
  value.ports.forEach(port);
  const ids = new Set<string>();
  const entries = value.entries.map(row => {
    valid(record(row) && identity(row.id) && !ids.has(row.id) && text(row.session) && row.session.length > 0 && record(row.claimOwners)
      && Object.keys(row.claimOwners).length <= 4096 && Object.entries(row.claimOwners).every(([id, owner]) => id.length > 0 && id.length <= 256 && digestId(owner))
      && text(row.error, 16384) && typeof row.busy === 'boolean' && typeof row.present === 'boolean'
      && (row.autoConnectPaused === undefined || typeof row.autoConnectPaused === 'boolean')
      && (row.stale === undefined || typeof row.stale === 'boolean'));
    ids.add(row.id); port(row.port); valid(row.port.gatewayId === undefined || row.port.gatewayId === row.id);
    if (row.state !== null) {
      valid(record(row.state) && row.state.gatewayId === row.id && typeof row.state.running === 'boolean' && typeof row.state.needsReconnect === 'boolean'
        && (row.state.connected === undefined || typeof row.state.connected === 'boolean') && Array.isArray(row.state.modules) && row.state.modules.length <= 4096);
      const moduleIds = new Set<string>();
      for (const module of row.state.modules) {
        valid(record(module) && text(module.id) && module.id.length > 0 && !moduleIds.has(module.id) && typeof module.online === 'boolean'
          && typeof module.registered === 'boolean' && typeof module.active === 'boolean' && record(module.settings)); moduleIds.add(module.id);
      }
    }
    let captured = normalized.get(row);
    if (!captured) { captured = Object.freeze({ ...row, stale: false }) as DeepReadonly<WorkspaceEntry>; normalized.set(row, captured); }
    return captured;
  });
  return Object.freeze({ ports: value.ports, inventoryError: value.inventoryError, entries: Object.freeze(entries) }) as DeepReadonly<WorkspaceModel>;
}
async function nativePage(id: string, offset: number, signal: AbortSignal): Promise<unknown> {
  if (signal.aborted) throw new Error('Workspace page request cancelled.');
  const { gatewayRequest } = await import('../gateway.ts');
  if (signal.aborted) throw new Error('Workspace page request cancelled.');
  return (await gatewayRequest({ command: 'workspace_read_model_page', id, offset })).workspaceReadModelPage;
}
interface Resolution { key: string; frame: WireFrame; controller: AbortController; }
interface Entry { state: WorkspaceModelState; listeners: Set<WorkspaceListener>; stop?: () => void; latest?: WireFrame; continuous: boolean; resolution?: Resolution; }
/** Shared immutable public workspace. The generic page engine also serves games. */
export class WorkspaceReadModels {
  private entry?: Entry;
  private pages: GamePages;
  constructor(private observations: Pick<SharedObservations, 'subscribe'> = new SharedObservations(), getPage: GamePageGetter = nativePage) { this.pages = new GamePages(getPage); }
  observe(listener: WorkspaceListener): () => void {
    const fresh = !this.entry;
    const entry: Entry = this.entry ??= { state: Object.freeze({ status: 'connecting' }), listeners: new Set(), continuous: false };
    const active = () => this.entry === entry && entry.listeners.size > 0;
    const registration: WorkspaceListener = state => listener(state);
    entry.listeners.add(registration); this.notify(registration, entry.state);
    if (fresh && active()) {
      try { const stop = this.observations.subscribe('workspace.state', state => { if (active()) this.accept(entry, state, active); }); if (active()) entry.stop = stop; else stop(); }
      catch (error) { this.fail(entry, error); this.publish(entry, active); }
    }
    let subscribed = true;
    return () => { if (!subscribed) return; subscribed = false; entry.listeners.delete(registration); if (!entry.listeners.size && this.entry === entry) { this.entry = undefined; this.cancel(entry); entry.stop?.(); } };
  }
  private notify(listener: WorkspaceListener, state: WorkspaceModelState): void { try { listener(state); } catch { /* Consumer exceptions cannot stall public metadata acknowledgment. */ } }
  private publish(entry: Entry, active: () => boolean): void { for (const listener of [...entry.listeners]) if (active() && entry.listeners.has(listener)) this.notify(listener, entry.state); }
  private cancel(entry: Entry): void { const resolution = entry.resolution; entry.resolution = undefined; resolution?.controller.abort(); }
  private fail(entry: Entry, error: unknown, continuous = false): void {
    this.cancel(entry); entry.continuous = continuous;
    entry.state = Object.freeze({ ...entry.state, status: 'error', loading: false, error: error instanceof Error ? error.message : String(error) });
  }
  private accept(entry: Entry, state: ObservationState, active: () => boolean): void {
    if (state.status !== 'ready') { this.cancel(entry); entry.continuous = false; entry.state = Object.freeze({ ...entry.state, status: state.status, loading: false, error: state.error }); this.publish(entry, active); return; }
    try {
      const frame = state.frame!, payload = frame.payload, previous = entry.latest;
      valid(frame.topic === 'workspace.state' && frame.source === 'workspace' && /^workspace:[a-f0-9]{8}-[a-f0-9]{4}-[a-f0-9]{4}-[a-f0-9]{4}-[a-f0-9]{12}$/.test(frame.clockId)
        && frame.kind !== 'loss' && record(payload) && Object.keys(payload).length === 1 && Object.hasOwn(payload, 'value'));
      valid(!previous || (previous.clockId === frame.clockId && previous.session === frame.session));
      if (previous && (BigInt(frame.generation) < BigInt(previous.generation) || (frame.generation === previous.generation && BigInt(frame.revision) < BigInt(previous.revision)))) return;
      if ((!entry.continuous || (previous && previous.generation !== frame.generation)) && frame.kind !== 'snapshot') {
        this.cancel(entry); entry.continuous = false; entry.state = Object.freeze({ ...entry.state, status: 'lost', loading: false, error: 'Waiting for a fresh workspace baseline.' }); this.publish(entry, active); return;
      }
      entry.latest = frame; entry.continuous = true;
      const key = JSON.stringify([frame.clockId, frame.session, frame.generation, payload]);
      if (entry.resolution?.key === key) { entry.resolution.frame = frame; return; }
      this.cancel(entry); const resolution: Resolution = { key, frame, controller: new AbortController() }; entry.resolution = resolution;
      entry.state = Object.freeze({ ...entry.state, status: 'lost', loading: true, error: 'Loading current gateway workspace.' }); this.publish(entry, active);
      if (!active() || resolution.controller.signal.aborted) return;
      void this.pages.resolve(payload.value, resolution.controller.signal).then(value => {
        if (!active() || entry.resolution !== resolution) return;
        try { const model = captureWorkspaceModel(value), latest = resolution.frame; this.cancel(entry); entry.state = Object.freeze({ status: 'ready', value: model, generation: latest.generation, revision: latest.revision, clockId: latest.clockId, session: latest.session }); }
        catch (error) { this.fail(entry, error); } this.publish(entry, active);
      }, error => { if (active() && entry.resolution === resolution) { this.fail(entry, error, true); this.publish(entry, active); } });
    } catch (error) { this.fail(entry, error); this.publish(entry, active); }
  }
}
let shared: WorkspaceReadModels | undefined;
export function observeWorkspaceState(listener: WorkspaceListener): () => void { return (shared ??= new WorkspaceReadModels()).observe(listener); }
