/** Immutable, hash-addressed JSON trees. Assets stay opaque until explicitly observed. */
export type GamePageKind = 'asset' | 'text' | 'array' | 'object';
export type GamePageReference = Readonly<{ id: string; bytes: number; kind: GamePageKind }>;
export type GamePageGetter = (id: string, offset: number, signal: AbortSignal) => Promise<unknown>;
export const MAX_GAME_BLOB_BYTES = 16 * 1024 * 1024;
export const MAX_GAME_CACHE_BYTES = 128 * 1024 * 1024;
export const MAX_GAME_CACHE_NODES = 4096;
export const MAX_GAME_LOGICAL_NODES = 1000000;
const encoder = new TextEncoder();
const decoder = new TextDecoder('utf-8', { fatal: true });
export const record = (value: unknown): value is Record<string, unknown> => !!value && typeof value === 'object' && !Array.isArray(value);
export function valid(condition: unknown): asserts condition { if (!condition) throw new Error('Invalid immutable game read model. Waiting for a current baseline.'); }
const integer = (value: unknown, min: number, max: number): value is number => Number.isSafeInteger(value) && Number(value) >= min && Number(value) <= max;
export const digestId = (value: unknown): value is string => typeof value === 'string' && /^sha256:[a-f0-9]{64}$/.test(value);
const cancelled = () => new Error('Game read-model resolution was replaced or disposed.');
export function capturePageReference(value: unknown): GamePageReference {
  valid(record(value) && Object.keys(value).length === 1 && record(value.$gamePage));
  const page = value.$gamePage;
  valid(Object.keys(page).length === 3 && digestId(page.id) && integer(page.bytes, 0, MAX_GAME_BLOB_BYTES)
    && ['asset', 'text', 'array', 'object'].includes(page.kind as string));
  return Object.freeze({ id: page.id, bytes: page.bytes, kind: page.kind as GamePageKind });
}
export function assetReference(value: string): GamePageReference {
  const match = /^game-asset:([a-f0-9]{64}):([0-9]+)$/.exec(value);
  valid(match && String(Number(match[2])) === match[2]);
  return capturePageReference({ $gamePage: { id: `sha256:${match[1]}`, bytes: Number(match[2]), kind: 'asset' } });
}
interface Result { value: unknown; count: number; depth: number; bytes: number; }
interface Cached { reference: GamePageReference; result: Result; }
interface Inflight { reference: GamePageReference; controller: AbortController; users: number; promise: Promise<Result>; }
function same(a: GamePageReference, b: GamePageReference): void { valid(a.id === b.id && a.bytes === b.bytes && a.kind === b.kind); }
function bounded(result: Result): Result {
  valid(result.count <= MAX_GAME_LOGICAL_NODES && result.depth <= 64 && result.bytes <= MAX_GAME_CACHE_BYTES);
  return result;
}
/** At most eight native loads; duplicate consumers share work and cancel independently. */
export class GamePages {
  private cache = new Map<string, Cached>();
  private inflight = new Map<string, Inflight>();
  private cacheBytes = 0;
  private active = 0;
  private waiters = new Set<() => void>();
  constructor(private getPage: GamePageGetter) {}

  async resolve(value: unknown, signal: AbortSignal): Promise<unknown> { return (await this.node(value, signal, new Set(), 0)).value; }
  async resolveAsset(reference: GamePageReference, signal: AbortSignal): Promise<string> {
    valid(reference.kind === 'asset');
    const source = (await this.reference(reference, signal, new Set())).value;
    valid(typeof source === 'string' && source.startsWith('data:image/jpeg;base64,'));
    const data = source.slice('data:image/jpeg;base64,'.length);
    valid(data.length > 0 && data.length % 4 === 0 && /^[A-Za-z0-9+/]*={0,2}$/.test(data));
    return source;
  }
  private cached(reference: GamePageReference): Result | undefined {
    const found = this.cache.get(reference.id);
    if (!found) return;
    same(found.reference, reference);
    this.cache.delete(reference.id); this.cache.set(reference.id, found);
    return found.result;
  }
  private async node(value: unknown, signal: AbortSignal, path: Set<string>, depth: number): Promise<Result> {
    if (signal.aborted) throw cancelled();
    valid(depth <= 64);
    if (value === null || typeof value === 'boolean' || typeof value === 'string' || (typeof value === 'number' && Number.isFinite(value))) {
      if (typeof value === 'string') valid(decoder.decode(encoder.encode(value)) === value);
      return { value, count: 1, depth: 0, bytes: typeof value === 'string' ? encoder.encode(value).length : 8 };
    }
    valid(Array.isArray(value) || record(value));
    if (!Array.isArray(value) && Object.hasOwn(value, '$gamePage')) {
      const reference = capturePageReference(value);
      if (reference.kind === 'asset') return { value: `game-asset:${reference.id.slice(7)}:${reference.bytes}`, count: 1, depth: 0, bytes: 96 };
      const result = await this.reference(reference, signal, path);
      valid(depth + result.depth <= 64);
      return result;
    }
    const entries = Object.entries(value);
    valid(entries.length <= MAX_GAME_LOGICAL_NODES);
    const output: unknown[] | Record<string, unknown> = Array.isArray(value) ? [] : {};
    const result: Result = { value: output, count: 1, depth: 0, bytes: 0 };
    for (const [key, child] of entries) {
      valid(decoder.decode(encoder.encode(key)) === key);
      const decoded = await this.node(child, signal, path, depth + 1);
      result.count += decoded.count; result.depth = Math.max(result.depth, 1 + decoded.depth); result.bytes += decoded.bytes + encoder.encode(key).length;
      bounded(result);
      Object.defineProperty(output, key, { value: decoded.value, enumerable: true });
    }
    Object.freeze(output);
    return result;
  }
  private reference(reference: GamePageReference, signal: AbortSignal, path: Set<string>): Promise<Result> {
    if (signal.aborted) return Promise.reject(cancelled());
    let job: Inflight | undefined;
    try {
      valid(!path.has(reference.id) && path.size < 64);
      const cached = this.cached(reference); if (cached) return Promise.resolve(cached);
      job = this.inflight.get(reference.id); if (job) same(job.reference, reference);
      valid(job || this.inflight.size < MAX_GAME_CACHE_NODES);
    } catch (error) { return Promise.reject(error); }
    if (!job) {
      const controller = new AbortController(), nextPath = new Set(path).add(reference.id);
      job = { reference, controller, users: 0, promise: Promise.resolve({ value: null, count: 0, depth: 0, bytes: 0 }) };
      const current = job;
      this.inflight.set(reference.id, current);
      current.promise = this.load(reference, controller.signal, nextPath).then(result => {
        if (controller.signal.aborted) throw cancelled();
        this.cache.set(reference.id, { reference, result }); this.cacheBytes += result.bytes;
        while (this.cache.size > MAX_GAME_CACHE_NODES || this.cacheBytes > MAX_GAME_CACHE_BYTES) {
          const first = this.cache.keys().next().value!;
          this.cacheBytes -= this.cache.get(first)!.result.bytes; this.cache.delete(first);
        }
        return result;
      }).finally(() => { if (this.inflight.get(reference.id) === current) this.inflight.delete(reference.id); });
    }
    const current = job; current.users++;
    return new Promise((resolve, reject) => {
      let done = false;
      const finish = (error?: unknown, result?: Result) => {
        if (done) return; done = true; signal.removeEventListener('abort', abort); current.users--;
        if (!current.users) { current.controller.abort(); if (this.inflight.get(reference.id) === current) this.inflight.delete(reference.id); }
        if (error) reject(error); else resolve(result!);
      };
      const abort = () => finish(cancelled());
      signal.addEventListener('abort', abort, { once: true });
      current.promise.then(result => finish(undefined, result), error => finish(error));
      if (signal.aborted) abort();
    });
  }
  private async acquire(signal: AbortSignal): Promise<() => void> {
    while (this.active >= 8) {
      if (signal.aborted) throw cancelled();
      await new Promise<void>((resolve, reject) => {
        const wake = () => { signal.removeEventListener('abort', abort); this.waiters.delete(wake); resolve(); };
        const abort = () => { this.waiters.delete(wake); reject(cancelled()); };
        this.waiters.add(wake); signal.addEventListener('abort', abort, { once: true });
        if (signal.aborted) abort();
      });
    }
    if (signal.aborted) throw cancelled();
    this.active++;
    return () => { this.active--; for (const wake of [...this.waiters]) wake(); };
  }
  private async bytes(reference: GamePageReference, signal: AbortSignal): Promise<string> {
    const release = await this.acquire(signal);
    try {
      const bytes = new Uint8Array(reference.bytes); let offset = 0;
      while (offset < reference.bytes) {
        if (signal.aborted) throw cancelled();
        const page = await this.getPage(reference.id, offset, signal);
        if (signal.aborted) throw cancelled();
        valid(record(page) && Object.keys(page).length === 5 && page.id === reference.id && page.offset === offset && page.total === reference.bytes
          && (page.nextOffset === null || integer(page.nextOffset, offset + 1, reference.bytes)) && typeof page.chunk === 'string');
        const chunk = encoder.encode(page.chunk);
        const next = offset + chunk.length;
        valid(chunk.length > 0 && chunk.length <= 8192 && next <= reference.bytes && decoder.decode(chunk) === page.chunk
          && (next === reference.bytes ? page.nextOffset === null : page.nextOffset === next));
        bytes.set(chunk, offset); offset = next;
      }
      const digest = new Uint8Array(await crypto.subtle.digest('SHA-256', bytes));
      if (signal.aborted) throw cancelled();
      valid(`sha256:${[...digest].map(byte => byte.toString(16).padStart(2, '0')).join('')}` === reference.id);
      return decoder.decode(bytes);
    } finally { release(); }
  }
  private async load(reference: GamePageReference, signal: AbortSignal, path: Set<string>): Promise<Result> {
    const source = await this.bytes(reference, signal);
    if (reference.kind === 'asset' || reference.kind === 'text') return { value: source, count: 1, depth: 0, bytes: reference.bytes };
    const blob: unknown = JSON.parse(source);
    valid(record(blob) && Object.keys(blob).length === 2 && (blob.type === 'leaf' || blob.type === 'branch'));
    if (blob.type === 'leaf') {
      let value: unknown;
      if (reference.kind === 'array') { valid(Array.isArray(blob.items)); value = blob.items; }
      else {
        valid(Array.isArray(blob.entries) && blob.entries.length <= MAX_GAME_LOGICAL_NODES);
        const object: Record<string, unknown> = {};
        for (const entry of blob.entries) {
          valid(Array.isArray(entry) && entry.length === 2 && typeof entry[0] === 'string' && !Object.hasOwn(object, entry[0]));
          Object.defineProperty(object, entry[0], { value: entry[1], enumerable: true });
        }
        value = object;
      }
      const result = await this.node(value, signal, path, 0);
      return bounded({ ...result, bytes: reference.bytes + result.bytes });
    }
    valid(Array.isArray(blob.chunks) && blob.chunks.length > 0 && blob.chunks.length <= MAX_GAME_CACHE_NODES);
    const output: unknown[] | Record<string, unknown> = reference.kind === 'array' ? [] : {};
    const result: Result = { value: output, count: 1, depth: 0, bytes: reference.bytes };
    for (const chunk of blob.chunks) {
      const child = capturePageReference(chunk); valid(child.kind === reference.kind);
      const decoded = await this.reference(child, signal, path);
      result.count += decoded.count - 1; result.depth = Math.max(result.depth, decoded.depth); result.bytes += decoded.bytes;
      bounded(result);
      if (Array.isArray(output)) { valid(Array.isArray(decoded.value)); for (const item of decoded.value) output.push(item); }
      else {
        valid(record(decoded.value));
        for (const [key, value] of Object.entries(decoded.value)) {
          valid(!Object.hasOwn(output, key)); Object.defineProperty(output, key, { value, enumerable: true });
        }
      }
    }
    Object.freeze(output); return result;
  }
}
