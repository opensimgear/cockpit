import { SharedObservations, type ObservationState } from './streams/shared.ts';
import { uint64, type WireFrame } from './streams/protocol.ts';
import { usbMetadataConfigValid, usbRoutesEqual, type UsbIdentity, type UsbMetadataConfig, type UsbRouteToken } from './module-usb.ts';
import type { DeepReadonly } from './profiles.ts';

export type UsbMetadataPayload = { key: string; connected: boolean; port: string | null; identity: UsbIdentity | null; route: UsbRouteToken | null;
  config: UsbMetadataConfig | null; observedAtMs: number | null; error: string | null };
export type UsbMetadataObservationState = Readonly<{ status: 'connecting' | 'ready' | 'lost' | 'error'; value?: DeepReadonly<UsbMetadataPayload>; error?: string }>;
export type UsbMetadataListener = (state: UsbMetadataObservationState) => void;
type Digest = (bytes: Uint8Array<ArrayBuffer>) => Promise<ArrayBuffer>;
const encoder = new TextEncoder();
const record = (value: unknown): value is Record<string, unknown> => !!value && typeof value === 'object' && !Array.isArray(value);
const exact = (value: unknown, keys: string[]): value is Record<string, unknown> => record(value) && Object.keys(value).length === keys.length && keys.every(key => Object.hasOwn(value, key));
const uint = (value: unknown, max = 0xffffffff, min = 0): value is number => typeof value === 'number' && Number.isSafeInteger(value) && value >= min && value <= max;
const text = (value: unknown, maximum = 2048): value is string => typeof value === 'string' && value.length > 0 && encoder.encode(value).length <= maximum && !/[\u0000-\u001f\u007f-\u009f]/.test(value);
function valid(condition: unknown): asserts condition { if (!condition) throw new Error('Invalid USB metadata observation; waiting for a fresh baseline.'); }
function keyValid(value: unknown): value is string {
  return typeof value === 'string' && value.length > 0 && encoder.encode(value).length <= 2048
    && !/[\u0000-\u001f\u007f-\u009f]|[\uD800-\uDBFF](?![\uDC00-\uDFFF])|(?<![\uD800-\uDBFF])[\uDC00-\uDFFF]/u.test(value);
}
function route(value: unknown): asserts value is UsbRouteToken {
  valid(exact(value, ['moduleId', 'serial', 'connectionGeneration', 'boot', 'configurationGeneration'])
    && typeof value.serial === 'string' && /^[0-9a-f]{24}$/.test(value.serial) && value.moduleId === `4f534744${value.serial}`
    && uint(value.connectionGeneration, Number.MAX_SAFE_INTEGER, 1) && uint(value.boot, 0xffffffff, 1) && uint(value.configurationGeneration, 0xffffffff, 1));
}
function identity(value: unknown): asserts value is UsbIdentity {
  valid(exact(value, ['model', 'board', 'profile', 'moduleId', 'serial', 'protocol', 'buttonCount', 'axisCount', 'hatCount'])
    && value.model === 'device-template' && value.protocol === 3 && text(value.board, 31)
    && text(value.profile, 63)
    && typeof value.serial === 'string' && /^[0-9a-f]{24}$/.test(value.serial) && value.moduleId === `4f534744${value.serial}`
    && uint(value.buttonCount, 128) && uint(value.axisCount, 8) && uint(value.hatCount, 4));
}
function payload(value: unknown, key: string): asserts value is UsbMetadataPayload {
  valid(exact(value, ['key', 'connected', 'port', 'identity', 'route', 'config', 'observedAtMs', 'error']) && value.key === key
    && typeof value.connected === 'boolean' && (value.port === null || text(value.port))
    && (value.identity === null || record(value.identity)) && (value.route === null || record(value.route))
    && (value.config === null || record(value.config)) && (value.observedAtMs === null || uint(value.observedAtMs, Number.MAX_SAFE_INTEGER))
    && (value.error === null || text(value.error, 4096)));
  if (value.identity !== null) identity(value.identity);
  if (value.route !== null) route(value.route);
  if (value.config !== null) valid(usbMetadataConfigValid(value.config)
    && (value.route === null || (value.config as UsbMetadataConfig).generation === (value.route as UsbRouteToken).configurationGeneration));
  if (value.connected) valid(value.route !== null && value.identity !== null && value.config !== null
    && (value.route as UsbRouteToken).moduleId === (value.identity as UsbIdentity).moduleId);
}
function frozen<T>(value: T): DeepReadonly<T> {
  const copy = structuredClone(value);
  const freeze = (item: unknown) => { if (item && typeof item === 'object') { Object.values(item).forEach(freeze); Object.freeze(item); } };
  freeze(copy); return copy as DeepReadonly<T>;
}
/** Hash the exact endpoint key, with no case folding, path parsing or normalization. */
export async function usbMetadataTopic(key: string, digest: Digest = bytes => crypto.subtle.digest('SHA-256', bytes)): Promise<string> {
  valid(keyValid(key));
  const hash = new Uint8Array(await digest(encoder.encode(key))); valid(hash.length === 32);
  return `usb.metadata.${Array.from(hash, byte => byte.toString(16).padStart(2, '0')).join('')}`;
}
export function captureUsbMetadata(frame: WireFrame, topic: string, key: string): DeepReadonly<UsbMetadataPayload> {
  valid(keyValid(key) && frame.topic === topic && /^usb\.metadata\.[0-9a-f]{64}$/.test(topic) && frame.source === 'usb-metadata'
    && frame.kind !== 'loss');
  payload(frame.payload, key); return frozen(frame.payload as UsbMetadataPayload);
}
interface Entry {
  key: string; state: UsbMetadataObservationState; listeners: Set<UsbMetadataListener>; opening: boolean;
  stop?: () => void; latest?: WireFrame; boundary?: WireFrame; needsBaseline: boolean;
}
/** Passive exact-key subscriptions. Loss withdraws metadata pending a current baseline. */
export class UsbMetadataObservations {
  private entries = new Map<string, Entry>();
  constructor(private observations: Pick<SharedObservations, 'subscribe'> = new SharedObservations(), private digest?: Digest) {}
  observe(key: string, listener: UsbMetadataListener): () => void {
    if (!keyValid(key) || !this.entries.has(key) && this.entries.size >= 128) {
      this.notify(listener, Object.freeze({ status: 'error', error: 'Choose one bounded exact USB endpoint.' })); return () => {};
    }
    const entry: Entry = this.entries.get(key) ?? { key, state: Object.freeze({ status: 'connecting' }), listeners: new Set(), opening: false, needsBaseline: true };
    this.entries.set(key, entry);
    const registration: UsbMetadataListener = state => listener(state);
    entry.listeners.add(registration); this.notify(registration, entry.state);
    if (this.active(entry) && !entry.stop && !entry.opening) this.open(entry);
    let subscribed = true;
    return () => {
      if (!subscribed) return; subscribed = false; entry.listeners.delete(registration);
      if (!entry.listeners.size && this.entries.get(key) === entry) { this.entries.delete(key); entry.stop?.(); }
    };
  }
  private active(entry: Entry): boolean { return this.entries.get(entry.key) === entry && entry.listeners.size > 0; }
  private notify(listener: UsbMetadataListener, state: UsbMetadataObservationState): void { try { listener(state); } catch { /* Capture and ACK cannot depend on a view. */ } }
  private publish(entry: Entry, state: UsbMetadataObservationState): void {
    entry.state = Object.freeze(state);
    for (const listener of [...entry.listeners]) if (this.active(entry) && entry.listeners.has(listener)) this.notify(listener, entry.state);
  }
  private open(entry: Entry): void {
    entry.opening = true;
    void usbMetadataTopic(entry.key, this.digest).then(topic => {
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
      if (boundary?.topic === topic && boundary.source === 'usb-metadata' && uint64(boundary.generation) && uint64(boundary.revision)) entry.boundary = boundary;
      this.publish(entry, { status: observation.status, error: observation.error }); return;
    }
    try {
      const frame = observation.frame!, old = entry.latest, boundary = entry.boundary ?? old;
      valid(frame && uint64(frame.generation) && uint64(frame.revision));
      if (boundary) {
        valid(frame.session === boundary.session);
        if (BigInt(frame.generation) < BigInt(boundary.generation) || frame.generation === boundary.generation && BigInt(frame.revision) < BigInt(boundary.revision)) return;
      }
      if (old) valid(frame.session === old.session && (frame.clockId === old.clockId || frame.kind === 'snapshot' && BigInt(frame.generation) > BigInt(old.generation)));
      if ((entry.needsBaseline || old && old.generation !== frame.generation) && frame.kind !== 'snapshot') {
        entry.needsBaseline = true; this.publish(entry, { status: 'lost', error: 'Waiting for a fresh USB metadata baseline.' }); return;
      }
      const value = captureUsbMetadata(frame, topic, entry.key), previousRoute = entry.state.value?.route;
      if (previousRoute && value.route && !usbRoutesEqual(previousRoute, value.route) && frame.kind !== 'snapshot') {
        entry.needsBaseline = true; this.publish(entry, { status: 'lost', error: 'USB metadata route changed; waiting for a fresh baseline.' }); return;
      }
      entry.latest = frame; entry.boundary = undefined; entry.needsBaseline = false;
      this.publish(entry, { status: 'ready', value });
    } catch (error) { entry.needsBaseline = true; this.publish(entry, { status: 'error', error: String(error) }); }
  }
}
let shared: UsbMetadataObservations | undefined;
export function observeUsbMetadata(key: string, listener: UsbMetadataListener): () => void { return (shared ??= new UsbMetadataObservations()).observe(key, listener); }
