import { SharedObservations, type ObservationState } from '../streams/shared.ts';
import { uint64, type WireFrame } from '../streams/protocol.ts';
import type { ControllerResult, HostController } from '../controllers.ts';
import type { DeepReadonly } from '../profiles.ts';
import type { HidHideSelfAccess } from '../visibility.ts';

export type ControllerInventory = ControllerResult & {
  session: string; backendStatus: string; diagnostic: string; sources: HostController[]; selfAccess: HidHideSelfAccess;
};
export type ControllerInventoryState = Readonly<{
  status: 'connecting' | 'ready' | 'lost' | 'error'; value?: DeepReadonly<ControllerInventory>;
  error?: string; generation?: string; revision?: string; clockId?: string; session?: string;
}>;
export type ControllerInventoryListener = (state: ControllerInventoryState) => void;
const uuid = (value: unknown): value is string => typeof value === 'string' && /^[a-f0-9]{8}-[a-f0-9]{4}-[a-f0-9]{4}-[a-f0-9]{4}-[a-f0-9]{12}$/.test(value);
const record = (value: unknown): value is Record<string, unknown> => !!value && typeof value === 'object' && !Array.isArray(value);
const text = (value: unknown, max: number): value is string => typeof value === 'string' && value.length <= max;
const integer = (value: unknown, max: number): value is number => Number.isInteger(value) && Number(value) >= 0 && Number(value) <= max;
function valid(condition: unknown): asserts condition { if (!condition) throw new Error('Invalid controller inventory observation. Waiting for a fresh baseline.'); }
function same(a: object | undefined, b: Record<string, unknown>): boolean {
  return !!a && Object.keys(a).length === Object.keys(b).length && Object.entries(a).every(([key, value]) => b[key] === value);
}
function source(value: unknown): asserts value is HostController {
  valid(record(value) && Object.keys(value).length === 14 && uuid(value.id) && text(value.name, 512) && text(value.role, 64)
    && value.origin === 'host-controller' && text(value.backend, 128) && integer(value.vendorId, 65535) && integer(value.productId, 65535)
    && typeof value.osgHid === 'boolean' && typeof value.online === 'boolean' && uint64(value.generation) && value.sequence === '0'
    && integer(value.controlCount, 256) && typeof value.baselineReady === 'boolean' && (!value.baselineReady || value.online)
    && ['serial', 'local-path'].includes(value.identityConfidence as string));
}
function selfAccess(value: unknown): asserts value is HidHideSelfAccess {
  valid(record(value) && Object.keys(value).every(key => ['status', 'ready', 'changed', 'message', 'executable'].includes(key))
    && ['configured', 'unchanged', 'absent', 'unsupported', 'blocked', 'error', 'not-requested'].includes(value.status as string)
    && typeof value.ready === 'boolean' && (value.changed === null || typeof value.changed === 'boolean')
    && text(value.message, 65536) && (value.executable === undefined || text(value.executable, 32768)));
}
/** Complete metadata only: report values and descriptor ranges belong to selected input observations. */
export function captureControllerInventory(frame: WireFrame, previous?: DeepReadonly<ControllerInventory>): DeepReadonly<ControllerInventory> {
  const payload = frame.payload;
  valid(frame.topic === 'controllers.inventory' && frame.source === 'controller-input' && frame.kind !== 'loss' && record(payload)
    && Object.keys(payload).length === 8 && uuid(payload.session) && frame.clockId === `controllers:${payload.session}`
    && text(payload.backendStatus, 128) && text(payload.diagnostic, 65536) && Array.isArray(payload.sources) && payload.sources.length <= 256
    && payload.offset === 0 && payload.limit === 256 && payload.total === payload.sources.length);
  selfAccess(payload.selfAccess);
  const existing = new Map(previous?.sources.map(row => [row.id, row])), ids = new Set<string>();
  const sources = payload.sources.map(row => {
    source(row); valid(!ids.has(row.id)); ids.add(row.id);
    const old = existing.get(row.id);
    return same(old, row) ? old! : Object.freeze({ ...row });
  });
  const access = same(previous?.selfAccess, payload.selfAccess) ? previous!.selfAccess : Object.freeze({ ...payload.selfAccess });
  const unchanged = previous && previous.sources.length === sources.length && sources.every((row, index) => previous.sources[index] === row);
  return Object.freeze({ session: payload.session, backendStatus: payload.backendStatus, diagnostic: payload.diagnostic, selfAccess: access,
    sources: unchanged ? previous.sources : Object.freeze(sources), offset: 0, total: sources.length, limit: 256 });
}
interface Entry {
  state: ControllerInventoryState; listeners: Set<ControllerInventoryListener>; stop?: () => void;
  latest?: WireFrame; continuous: boolean;
}
/** One full inventory reduction and exact-topic transport registration for all consumers. */
export class ControllerReadModels {
  private inventory?: Entry;
  constructor(private observations: Pick<SharedObservations, 'subscribe'> = new SharedObservations()) {}
  observeInventory(listener: ControllerInventoryListener): () => void {
    const fresh = !this.inventory;
    const entry: Entry = this.inventory ??= { state: Object.freeze({ status: 'connecting' }), listeners: new Set(), continuous: false };
    const active = () => this.inventory === entry && entry.listeners.size > 0;
    const registration: ControllerInventoryListener = state => listener(state);
    entry.listeners.add(registration); this.notify(registration, entry.state);
    if (fresh && active()) {
      try {
        const stop = this.observations.subscribe('controllers.inventory', state => { if (active()) this.accept(entry, state, active); });
        if (active()) entry.stop = stop; else stop();
      } catch (error) { this.fail(entry, error); this.publish(entry, active); }
    }
    let subscribed = true;
    return () => {
      if (!subscribed) return; subscribed = false; entry.listeners.delete(registration);
      if (!entry.listeners.size && this.inventory === entry) { this.inventory = undefined; entry.stop?.(); }
    };
  }
  private notify(listener: ControllerInventoryListener, state: ControllerInventoryState): void { try { listener(state); } catch { /* A consumer cannot stall metadata delivery or acknowledgment. */ } }
  private publish(entry: Entry, active: () => boolean): void {
    for (const listener of [...entry.listeners]) if (active() && entry.listeners.has(listener)) this.notify(listener, entry.state);
  }
  private fail(entry: Entry, error: unknown): void {
    entry.continuous = false;
    entry.state = Object.freeze({ ...entry.state, status: 'error', error: error instanceof Error ? error.message : String(error) });
  }
  private accept(entry: Entry, state: ObservationState, active: () => boolean): void {
    if (state.status !== 'ready') {
      entry.continuous = false;
      entry.state = Object.freeze({ ...entry.state, status: state.status, error: state.error });
    } else {
      try {
        const frame = state.frame!, previous = entry.latest;
        valid(!previous || (frame.clockId === previous.clockId && frame.session === previous.session));
        if (previous && (BigInt(frame.generation) < BigInt(previous.generation)
          || (frame.generation === previous.generation && BigInt(frame.revision) < BigInt(previous.revision)))) return;
        if ((!entry.continuous || (previous && frame.generation !== previous.generation)) && frame.kind !== 'snapshot') {
          entry.continuous = false;
          entry.state = Object.freeze({ ...entry.state, status: 'lost', error: 'Waiting for a fresh controller inventory baseline.' });
        } else {
          const value = captureControllerInventory(frame, entry.state.value);
          entry.latest = frame; entry.continuous = true;
          entry.state = Object.freeze({ status: 'ready', value, generation: frame.generation, revision: frame.revision, clockId: frame.clockId, session: frame.session });
        }
      } catch (error) { this.fail(entry, error); }
    }
    this.publish(entry, active);
  }
}
let shared: ControllerReadModels | undefined;
export function observeControllerInventory(listener: ControllerInventoryListener): () => void { return (shared ??= new ControllerReadModels()).observeInventory(listener); }
