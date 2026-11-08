import type { ControllerControl, ControllerResult, HostController } from './controllers.ts';
import { SharedObservations, type ReadonlyObservationState } from './streams/shared.ts';
import { uint64, type WireFrame } from './streams/protocol.ts';
import { visibleSubscription } from './visible-observation.ts';
import type { DeepReadonly } from './profiles.ts';

export type ControllerInputDetail = DeepReadonly<ControllerResult>;
export interface ControllerInputState {
  readonly status: 'connecting' | 'ready' | 'lost' | 'error';
  readonly detail?: ControllerInputDetail;
  readonly error?: string;
}
type Listener = (state: ControllerInputState) => void;
type PreviewClock = {
  requestAnimationFrame(callback: FrameRequestCallback): number;
  cancelAnimationFrame(id: number): void;
};
type RecordValue = Record<string, unknown>;
type Snapshot = DeepReadonly<ControllerResult & { cursor: string; capturedUs: string }>;
type ControlView = DeepReadonly<ControllerControl>;
const descriptorIndexes = new WeakMap<Snapshot, ReadonlyMap<string, number>>();
interface Entry {
  listeners: Set<Listener>;
  state: ControllerInputState;
  baseline?: Snapshot;
  descriptorBaseline?: Snapshot;
  stop?: () => void;
}
const record = (value: unknown): value is RecordValue => typeof value === 'object' && value !== null && !Array.isArray(value);
const text = (value: unknown): value is string => typeof value === 'string';
const integer = (value: unknown, min: number, max: number): value is number => Number.isInteger(value) && (value as number) >= min && (value as number) <= max;
const native = (value: unknown): value is number => integer(value, -2147483648, 4294967295);
const uuid = /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/i;
function requireValid(condition: unknown): asserts condition {
  if (!condition) throw new Error('Invalid controller input observation. Waiting for a fresh baseline.');
}

function source(value: unknown, id: string): asserts value is HostController {
  requireValid(record(value) && value.id === id && text(value.name) && text(value.role)
    && value.origin === 'host-controller' && text(value.backend)
    && integer(value.vendorId, 0, 65535) && integer(value.productId, 0, 65535)
    && typeof value.osgHid === 'boolean' && typeof value.online === 'boolean'
    && uint64(value.generation) && uint64(value.sequence) && integer(value.controlCount, 0, 256)
    && typeof value.baselineReady === 'boolean' && (!value.baselineReady || value.online)
    && ['serial', 'local-path'].includes(value.identityConfidence as string));
}
function capability(value: unknown): asserts value is ControllerControl['capability'] {
  requireValid(record(value) && text(value.id) && value.id.length > 0 && value.id.length <= 128
    && ['axis', 'button', 'hat', 'diagnostic'].includes(value.kind as string)
    && integer(value.usagePage, 0, 65535) && integer(value.usage, 0, 65535)
    && integer(value.reportId, 0, 255) && integer(value.collection, 0, 65535)
    && integer(value.dataIndex, 0, 65535) && integer(value.bits, 0, 65535)
    && native(value.min) && native(value.max) && typeof value.hasNull === 'boolean');
  requireValid(value.kind === 'diagnostic' || (value.bits >= 1 && value.bits <= 32 && value.min <= value.max));
}
function sample(value: unknown, descriptor: ControllerControl['capability']): asserts value is NonNullable<ControllerControl['value']> {
  requireValid(record(value) && native(value.native)
    && (value.normalized === null || integer(value.normalized, -32768, 32767)));
  if (descriptor.kind === 'diagnostic') {
    requireValid(value.native >= 0 && value.normalized === null);
    return;
  }
  if (value.normalized === null) return;
  requireValid(value.native >= descriptor.min && value.native <= descriptor.max);
  if (descriptor.kind === 'button') requireValid(value.normalized === (value.native === 0 ? 0 : 1));
  else if (descriptor.kind === 'hat') {
    const count = descriptor.max - descriptor.min + 1;
    requireValid((count === 4 || count === 8) && value.normalized === (value.native - descriptor.min) * (8 / count));
  } else if (descriptor.kind === 'axis') {
    const low = descriptor.min < 0 ? -32768 : 0;
    const span = descriptor.min < 0 ? 65535 : 32767;
    requireValid(descriptor.max > descriptor.min
      && value.normalized === low + Math.trunc((value.native - descriptor.min) * span / (descriptor.max - descriptor.min)));
  } else requireValid(false);
}
function frozenCopy<T>(value: T): DeepReadonly<T> {
  const copy = structuredClone(value);
  function freeze(item: unknown): void {
    if (item && typeof item === 'object' && !Object.isFrozen(item)) {
      // Wire values are JSON records/arrays. Freezing Map/Date wrappers would
      // leave their internal mutable state exposed to consumers.
      requireValid(Array.isArray(item) || Object.getPrototypeOf(item) === Object.prototype);
      for (const child of Object.values(item)) freeze(child);
      Object.freeze(item);
    }
  }
  freeze(copy);
  return copy as DeepReadonly<T>;
}
function row(descriptor: ControlView['capability'], value: ControllerControl['value'] | ControlView['value'], previous?: ControlView): ControlView {
  if (previous && previous.capability === descriptor && previous.value?.native === value?.native
      && previous.value?.normalized === value?.normalized) return previous;
  return Object.freeze({ capability: descriptor, value: value === null ? null : Object.freeze({ native: value.native, normalized: value.normalized }) });
}
function clear(detail: ControllerInputDetail | undefined): ControllerInputDetail | undefined {
  if (!detail) return undefined;
  // Subscriber continuity is not native device presence. Only the owner can
  // declare a disconnect; transport loss withdraws usable samples instead.
  return Object.freeze({ ...detail, source: Object.freeze({ ...detail.source!, baselineReady: false }),
    controls: Object.freeze(detail.controls!.map(control => row(control.capability, null, control))) });
}

/** Validate into a new result; an invalid frame can never partially update live values. */
function reduce(id: string, frame: Readonly<WireFrame>, previous?: Snapshot, descriptorBaseline?: Snapshot): Snapshot {
  const payload = frame.payload;
  requireValid(record(payload) && text(payload.session) && payload.session.length > 0);
  source(payload.source, id);
  requireValid(frame.source === 'controller-input' && frame.clockId === `controllers:${payload.session}`
    && BigInt(frame.generation) === BigInt(payload.source.generation) + 1n
    && uint64(payload.cursor) && uint64(payload.capturedUs) && payload.capturedUs === frame.capturedUs);
  const unavailable = !payload.source.online || !payload.source.baselineReady;
  if (frame.kind === 'snapshot') {
    requireValid(record(payload.identity) && text(payload.identity.path) && text(payload.identity.collection)
      && (payload.identity.serial === null || text(payload.identity.serial)) && text(payload.identity.fingerprint)
      && (payload.identity.backend === undefined || text(payload.identity.backend))
      && (payload.identity.vendorId === undefined || integer(payload.identity.vendorId, 0, 65535))
      && (payload.identity.productId === undefined || integer(payload.identity.productId, 0, 65535))
      && Array.isArray(payload.diagnostics) && payload.diagnostics.every(text)
      && payload.offset === 0 && payload.limit === 256 && integer(payload.total, 0, 256)
      && Array.isArray(payload.controls) && payload.controls.length === payload.total
      && payload.total === payload.source.controlCount);
    const ids = new Set<string>();
    const reusable = descriptorBaseline?.session === payload.session && descriptorBaseline.source!.generation === payload.source.generation
      ? descriptorBaseline : undefined;
    const oldIndexes = reusable ? descriptorIndexes.get(reusable) : undefined;
    requireValid(!reusable || reusable.total === payload.total);
    let sameOrder = !!oldIndexes;
    const controls = payload.controls.map((raw, index) => {
      requireValid(record(raw) && record(raw.capability) && text(raw.capability.id));
      const incoming = raw.capability;
      const id = incoming.id as string;
      requireValid(!ids.has(id));
      ids.add(id);
      const previousIndex = oldIndexes?.get(id);
      sameOrder &&= previousIndex === index;
      const previousRow = previousIndex === undefined ? undefined : reusable!.controls![previousIndex];
      let descriptor: ControlView['capability'];
      if (oldIndexes) {
        const existing = previousRow?.capability;
        requireValid(existing && Object.keys(existing).length === Object.keys(incoming).length
          && Object.entries(existing).every(([key, value]) => incoming[key] === value));
        descriptor = existing;
      } else {
        capability(incoming);
        descriptor = frozenCopy(incoming);
      }
      requireValid(raw.value === null || !unavailable);
      requireValid(unavailable || descriptor.kind === 'diagnostic' || raw.value !== null);
      if (raw.value !== null) sample(raw.value, descriptor);
      return row(descriptor, raw.value, previousRow);
    });
    requireValid(unavailable || controls.some(control => control.capability.kind !== 'diagnostic'));
    const result: Snapshot = Object.freeze({ session: payload.session, source: frozenCopy(payload.source), identity: frozenCopy(payload.identity),
      diagnostics: Object.freeze([...payload.diagnostics]), controls: reusable && controls.every((control, index) => control === reusable.controls![index])
        ? reusable.controls : Object.freeze(controls), offset: 0, total: payload.total, limit: 256,
      cursor: payload.cursor, capturedUs: payload.capturedUs }) as Snapshot;
    descriptorIndexes.set(result, sameOrder ? oldIndexes!
      : new Map(controls.map((control, index) => [control.capability.id, index])));
    return result;
  }
  requireValid(frame.kind === 'change' && previous && previous.session === payload.session
    && previous.source!.generation === payload.source.generation && previous.total === payload.source.controlCount
    && BigInt(payload.cursor) >= BigInt(previous.cursor) && BigInt(payload.capturedUs) >= BigInt(previous.capturedUs)
    && BigInt(payload.source.sequence) >= BigInt(previous.source!.sequence)
    && ['baseline', 'change', 'gap', 'disconnect', 'metadata'].includes(payload.reason as string)
    && record(payload.values));
  const values = payload.values;
  const entries = Object.entries(values);
  const ids = descriptorIndexes.get(previous)!;
  requireValid(!unavailable || entries.length === 0);
  // A complete baseline report must describe every control before values become available.
  if (payload.reason === 'baseline' && !unavailable) requireValid(previous.controls!.some(control => control.capability.kind !== 'diagnostic')
    && previous.controls!.every(control => control.capability.kind === 'diagnostic' || Object.hasOwn(values, control.capability.id)));
  requireValid(!payload.source.baselineReady || previous.source!.baselineReady || payload.reason === 'baseline');
  if (payload.reason === 'gap' || payload.reason === 'disconnect') requireValid(unavailable);
  const controls = previous.controls!;
  let updated: ControlView[] | undefined;
  if (unavailable && previous.source!.baselineReady) {
    updated = controls.map(control => row(control.capability, null, control));
  }
  for (const [controlId, value] of entries) {
    const index = ids.get(controlId);
    requireValid(index !== undefined);
    const control = controls[index];
    sample(value, control.capability);
    const next = row(control.capability, value, control);
    if (next !== control) (updated ??= [...controls])[index] = next;
  }
  const result: Snapshot = Object.freeze({ ...previous, source: frozenCopy(payload.source), cursor: payload.cursor, capturedUs: payload.capturedUs,
    controls: updated ? Object.freeze(updated) : controls });
  descriptorIndexes.set(result, ids);
  return result;
}

/** Shares both the transport subscription and descriptor/value reduction for each exact controller. */
export class ControllerInputObservations {
  private readonly entries = new Map<string, Entry>();
  private readonly observations: SharedObservations;
  constructor(observations = new SharedObservations()) { this.observations = observations; }

  /** Presentation only. Transport receipts and other listeners remain immediate. */
  observePreview(sourceId: string, listener: Listener, clock: PreviewClock = globalThis): () => void {
    let rendered: ControllerInputState | undefined;
    let pending: ControllerInputState | undefined;
    let frame: number | undefined;
    const cancel = () => {
      if (frame !== undefined) clock.cancelAnimationFrame(frame);
      frame = undefined;
      pending = undefined;
    };
    const demand = visibleSubscription(() => {
      const stop = this.observe(sourceId, state => {
        const previous = rendered?.detail;
        const detail = state.detail;
        if (state.status !== 'ready' || !detail?.source?.baselineReady
          || rendered?.status !== 'ready' || !previous?.source?.baselineReady
          || previous.session !== detail.session || previous.source.generation !== detail.source.generation) {
          cancel();
          // Keep the controller card mounted during reconnect, with values withdrawn.
          rendered = detail || !previous ? state : { ...state, detail: previous };
          this.notify(listener, rendered);
          return;
        }
        pending = state;
        frame ??= clock.requestAnimationFrame(() => {
          frame = undefined;
          const next = pending;
          pending = undefined;
          if (next) {
            rendered = next;
            this.notify(listener, next);
          }
        });
      });
      return () => {
        stop();
        cancel();
        rendered = { status: 'connecting', detail: clear(rendered?.detail) };
      };
    });
    demand.setVisible(true);
    return demand.close;
  }

  observe(sourceId: string, listener: Listener): () => void {
    if (!uuid.test(sourceId)) {
      this.notify(listener, { status: 'error', error: 'Choose an exact physical controller.' });
      return () => {};
    }
    let entry = this.entries.get(sourceId);
    const fresh = !entry;
    if (!entry) {
      entry = { listeners: new Set(), state: { status: 'connecting' } };
      this.entries.set(sourceId, entry);
    }
    const current = entry;
    const registration: Listener = state => listener(state);
    current.listeners.add(registration);
    this.notify(registration, current.state);
    if (fresh && this.entries.get(sourceId) === current && current.listeners.size > 0) {
      const stop = this.observations.subscribeReadonly(`controllers.input.${sourceId}`, state => {
        if (this.entries.get(sourceId) !== current) return;
        this.accept(sourceId, current, state);
      });
      if (this.entries.get(sourceId) === current && current.listeners.size > 0) current.stop = stop;
      else stop();
    }
    let active = true;
    return () => {
      if (!active) return;
      active = false;
      current.listeners.delete(registration);
      if (current.listeners.size === 0 && this.entries.get(sourceId) === current) {
        this.entries.delete(sourceId);
        current.stop?.();
      }
    };
  }

  private notify(listener: Listener, state: ControllerInputState): void {
    try { listener(Object.freeze(state)); } catch { /* Frozen views isolate synchronous and delayed consumers without cloning descriptors. */ }
  }
  private accept(id: string, entry: Entry, state: ReadonlyObservationState): void {
    if (state.status !== 'ready') {
      entry.state = { status: state.status, detail: clear(entry.baseline ?? entry.state.detail), error: state.error };
      entry.baseline = undefined;
    } else {
      try {
        const detail = reduce(id, state.frame!, entry.baseline, entry.descriptorBaseline);
        entry.baseline = detail;
        entry.descriptorBaseline = detail;
        entry.state = { status: 'ready', detail };
      } catch (error) {
        entry.state = { status: 'error', detail: clear(entry.baseline ?? entry.state.detail), error: (error as Error).message };
        entry.baseline = undefined;
      }
    }
    for (const listener of [...entry.listeners]) {
      if (this.entries.get(id) === entry && entry.listeners.has(listener)) this.notify(listener, entry.state);
    }
  }
}

let shared: ControllerInputObservations | undefined;
export function observeControllerInputs(sourceId: string, listener: Listener): () => void {
  shared ??= new ControllerInputObservations();
  return shared.observe(sourceId, listener);
}

export function observeControllerPreview(sourceId: string, listener: Listener): () => void {
  shared ??= new ControllerInputObservations();
  return shared.observePreview(sourceId, listener);
}
