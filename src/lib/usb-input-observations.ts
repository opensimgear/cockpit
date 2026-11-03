import { SharedObservations, type ObservationState } from './streams/shared.ts';
import { uint64, type WireFrame } from './streams/protocol.ts';
import { usbRoutesEqual, type UsbRouteToken } from './module-usb.ts';
import type { DeepReadonly } from './profiles.ts';

export type UsbInputMetadata = {
  ok: true; schemaVersion: 1; stream: number; head: number; oldest: number; overflow: number;
  buttonCount: number; held: boolean[]; contactButtonCount: number; maintainedButtons: number[];
  encoderButtons: number[]; selectorButtons: number[][]; axisCount: number; axes: number[];
  axisNeutral: number[]; axisUsage: number[]; axisAvailable: boolean[]; sampledAtMs: number;
};
export type UsbInputPayload = { key: string; route: UsbRouteToken | null; ready: boolean; state: UsbInputMetadata | null; error: string | null };
export type UsbInputObservationState = Readonly<{ status: 'connecting' | 'ready' | 'lost' | 'error'; value?: DeepReadonly<UsbInputPayload>; error?: string }>;
export type UsbInputListener = (state: UsbInputObservationState) => void;
type Digest = (bytes: Uint8Array<ArrayBuffer>) => Promise<ArrayBuffer>;
const encoder = new TextEncoder();
const record = (value: unknown): value is Record<string, unknown> => !!value && typeof value === 'object' && !Array.isArray(value);
const exact = (value: unknown, keys: string[]): value is Record<string, unknown> => record(value) && Object.keys(value).length === keys.length && keys.every(key => Object.hasOwn(value, key));
const uint = (value: unknown, max = 0xffffffff, min = 0): value is number => typeof value === 'number' && Number.isSafeInteger(value) && value >= min && value <= max;
const rows = (value: unknown, max: number): value is unknown[] => Array.isArray(value) && value.length <= max;
function valid(condition: unknown): asserts condition { if (!condition) throw new Error('Invalid USB input observation; waiting for a fresh baseline.'); }
function keyValid(value: unknown): value is string {
  return typeof value === 'string' && value.length > 0 && encoder.encode(value).length <= 2048
    && !/[\u0000-\u001f\u007f-\u009f]|[\uD800-\uDBFF](?![\uDC00-\uDFFF])|(?<![\uD800-\uDBFF])[\uDC00-\uDFFF]/u.test(value);
}
function route(value: unknown): asserts value is UsbRouteToken {
  valid(exact(value, ['moduleId', 'serial', 'connectionGeneration', 'boot', 'configurationGeneration'])
    && typeof value.serial === 'string' && /^[0-9a-f]{24}$/.test(value.serial) && value.moduleId === `4f534744${value.serial}`
    && uint(value.connectionGeneration, Number.MAX_SAFE_INTEGER, 1) && uint(value.boot, 0xffffffff, 1) && uint(value.configurationGeneration, 0xffffffff, 1));
}
function metadata(value: unknown, expected: UsbRouteToken): asserts value is UsbInputMetadata {
  valid(exact(value, ['ok', 'schemaVersion', 'stream', 'head', 'oldest', 'overflow', 'buttonCount', 'held', 'contactButtonCount',
    'maintainedButtons', 'encoderButtons', 'selectorButtons', 'axisCount', 'axes', 'axisNeutral', 'axisUsage', 'axisAvailable', 'sampledAtMs'])
    && value.ok === true && value.schemaVersion === 1 && value.stream === expected.configurationGeneration
    && uint(value.head) && uint(value.oldest, Math.min(value.head + 1, 0xffffffff), 1) && uint(value.overflow)
    && uint(value.buttonCount, 128) && rows(value.held, 128) && value.held.length === value.buttonCount && value.held.every(item => typeof item === 'boolean')
    && uint(value.contactButtonCount, value.buttonCount) && rows(value.maintainedButtons, value.contactButtonCount)
    && value.maintainedButtons.every(item => uint(item, Number(value.contactButtonCount) - 1)) && new Set(value.maintainedButtons).size === value.maintainedButtons.length
    && rows(value.encoderButtons, 16) && value.encoderButtons.length % 2 === 0
    && value.encoderButtons.every((item, index) => item === Number(value.contactButtonCount) + index)
    && rows(value.selectorButtons, 8) && value.contactButtonCount + value.encoderButtons.length + value.selectorButtons.length * 3 === value.buttonCount
    && value.selectorButtons.every((group, index) => rows(group, 3) && group.length === 3
      && group.every((item, offset) => item === Number(value.contactButtonCount) + (value.encoderButtons as unknown[]).length + index * 3 + offset))
    && uint(value.axisCount, 8) && uint(value.sampledAtMs));
  for (const field of ['axes', 'axisNeutral', 'axisUsage', 'axisAvailable']) {
    const values = value[field as keyof UsbInputMetadata];
    valid(rows(values, 8) && values.length === value.axisCount && values.every(item => field === 'axisAvailable'
      ? typeof item === 'boolean' : field === 'axisUsage' ? uint(item, 56, 48) : uint(item, 65535)));
  }
}
function frozen<T>(value: T): DeepReadonly<T> {
  const copy = structuredClone(value);
  const freeze = (item: unknown) => { if (item && typeof item === 'object') { Object.values(item).forEach(freeze); Object.freeze(item); } };
  freeze(copy); return copy as DeepReadonly<T>;
}
/** Hash the exact endpoint key, with no case folding, path parsing or normalization. */
export async function usbInputsTopic(key: string, digest: Digest = bytes => crypto.subtle.digest('SHA-256', bytes)): Promise<string> {
  valid(keyValid(key));
  const hash = new Uint8Array(await digest(encoder.encode(key))); valid(hash.length === 32);
  return `usb.inputs.${Array.from(hash, byte => byte.toString(16).padStart(2, '0')).join('')}`;
}
export function captureUsbInputs(frame: WireFrame, topic: string, key: string): DeepReadonly<UsbInputPayload> {
  const value = frame.payload;
  valid(keyValid(key) && frame.topic === topic && /^usb\.inputs\.[0-9a-f]{64}$/.test(topic) && frame.source === 'usb-inputs'
    && frame.kind !== 'loss' && exact(value, ['key', 'route', 'ready', 'state', 'error']) && value.key === key
    && typeof value.ready === 'boolean' && (value.error === null || typeof value.error === 'string' && encoder.encode(value.error).length <= 4096));
  if (value.route !== null) route(value.route);
  if (value.ready) { valid(value.route !== null && value.error === null); metadata(value.state, value.route as UsbRouteToken); }
  else valid(value.state === null);
  return frozen(value as UsbInputPayload);
}
interface Entry {
  key: string; state: UsbInputObservationState; listeners: Set<UsbInputListener>; opening: boolean;
  stop?: () => void; latest?: WireFrame; boundary?: WireFrame; needsBaseline: boolean;
}
/** Passive, exact-key subscriptions. Lost input is withdrawn, never shown as a current sample. */
export class UsbInputObservations {
  private entries = new Map<string, Entry>();
  constructor(private observations: Pick<SharedObservations, 'subscribe'> = new SharedObservations(), private digest?: Digest) {}
  observe(key: string, listener: UsbInputListener): () => void {
    if (!keyValid(key) || !this.entries.has(key) && this.entries.size >= 128) {
      this.notify(listener, Object.freeze({ status: 'error', error: 'Choose one bounded exact USB endpoint.' })); return () => {};
    }
    const entry: Entry = this.entries.get(key) ?? { key, state: Object.freeze({ status: 'connecting' }), listeners: new Set(), opening: false, needsBaseline: true };
    this.entries.set(key, entry);
    const registration: UsbInputListener = state => listener(state);
    entry.listeners.add(registration); this.notify(registration, entry.state);
    if (this.active(entry) && !entry.stop && !entry.opening) this.open(entry);
    let subscribed = true;
    return () => {
      if (!subscribed) return; subscribed = false; entry.listeners.delete(registration);
      if (!entry.listeners.size && this.entries.get(key) === entry) { this.entries.delete(key); entry.stop?.(); }
    };
  }
  private active(entry: Entry): boolean { return this.entries.get(entry.key) === entry && entry.listeners.size > 0; }
  private notify(listener: UsbInputListener, state: UsbInputObservationState): void { try { listener(state); } catch { /* Capture and ACK cannot depend on a view. */ } }
  private publish(entry: Entry, state: UsbInputObservationState): void {
    entry.state = Object.freeze(state);
    for (const listener of [...entry.listeners]) if (this.active(entry) && entry.listeners.has(listener)) this.notify(listener, entry.state);
  }
  private open(entry: Entry): void {
    entry.opening = true;
    void usbInputsTopic(entry.key, this.digest).then(topic => {
      if (!this.active(entry)) return;
      try {
        const stop = this.observations.subscribe(topic, state => this.accept(entry, topic, state));
        if (this.active(entry)) entry.stop = stop; else stop();
      } catch (error) { if (this.active(entry)) this.publish(entry, { status: 'error', error: String(error) }); }
      finally { entry.opening = false; }
    }, error => { entry.opening = false; if (this.active(entry)) this.publish(entry, { status: 'error', error: String(error) }); });
  }
  private accept(entry: Entry, topic: string, observation: ObservationState): void {
    if (!this.active(entry)) return;
    if (observation.status !== 'ready') {
      entry.needsBaseline = true;
      const boundary = observation.frame;
      if (boundary?.topic === topic && boundary.source === 'usb-inputs' && uint64(boundary.generation) && uint64(boundary.revision)) entry.boundary = boundary;
      this.publish(entry, { status: observation.status, error: observation.error }); return;
    }
    try {
      const frame = observation.frame!, old = entry.latest, boundary = entry.boundary ?? old;
      valid(frame && uint64(frame.generation) && uint64(frame.revision));
      if (boundary) {
        valid(frame.session === boundary.session);
        if (BigInt(frame.generation) < BigInt(boundary.generation) || frame.generation === boundary.generation && BigInt(frame.revision) < BigInt(boundary.revision)) return;
      }
      if (old) {
        valid(frame.session === old.session && (frame.clockId === old.clockId || frame.kind === 'snapshot' && BigInt(frame.generation) > BigInt(old.generation)));
      }
      if ((entry.needsBaseline || old && old.generation !== frame.generation) && frame.kind !== 'snapshot') {
        entry.needsBaseline = true; this.publish(entry, { status: 'lost', error: 'Waiting for a fresh USB input baseline.' }); return;
      }
      const value = captureUsbInputs(frame, topic, entry.key), previousRoute = entry.state.value?.route;
      if (previousRoute && value.route && !usbRoutesEqual(previousRoute, value.route) && frame.kind !== 'snapshot') {
        entry.needsBaseline = true; this.publish(entry, { status: 'lost', error: 'USB input route changed; waiting for a fresh baseline.' }); return;
      }
      entry.latest = frame; entry.boundary = undefined; entry.needsBaseline = false;
      this.publish(entry, { status: 'ready', value });
    } catch (error) { entry.needsBaseline = true; this.publish(entry, { status: 'error', error: String(error) }); }
  }
}
let shared: UsbInputObservations | undefined;
export function observeUsbInputs(key: string, listener: UsbInputListener): () => void { return (shared ??= new UsbInputObservations()).observe(key, listener); }
