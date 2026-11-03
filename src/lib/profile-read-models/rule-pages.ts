import type { DeepReadonly } from '../profiles.ts';

export type RulesManifest = Readonly<{ id: string; bytes: number; count: number }>;
export type ReadModelPage = { id: string; offset: number; nextOffset: number; total: number; chunk: string };
export type ReadModelPageGetter = (id: string, offset: number, signal: AbortSignal) => Promise<unknown>;
export const MAX_RULE_BYTES = 3 * 1024 * 1024;
const encoder = new TextEncoder();
const record = (value: unknown): value is Record<string, unknown> => !!value && typeof value === 'object' && !Array.isArray(value);
const integer = (value: unknown, min: number, max: number): value is number => Number.isSafeInteger(value) && Number(value) >= min && Number(value) <= max;
function valid(condition: unknown): asserts condition { if (!condition) throw new Error('Invalid immutable profile-rule page. Waiting for a current baseline.'); }
const cancelled = () => new Error('Profile-rule resolution was replaced or disposed.');
export function captureRulesManifest(value: unknown): RulesManifest {
  valid(record(value) && Object.keys(value).length === 1 && record(value.pages));
  const pages = value.pages;
  valid(Object.keys(pages).length === 3 && typeof pages.id === 'string' && /^sha256:[a-f0-9]{64}$/.test(pages.id)
    && integer(pages.bytes, 2, MAX_RULE_BYTES) && integer(pages.count, 0, 512));
  return Object.freeze({id:pages.id,bytes:pages.bytes,count:pages.count});
}
interface Inflight { manifest: RulesManifest; controller: AbortController; users: number; promise: Promise<readonly unknown[]>; }
interface Cached { manifest: RulesManifest; rules: readonly unknown[]; }
/** Hash-addressed immutable data only. Two loads and two cached blobs bound memory and work. */
export class ProfileRulePages {
  private cache = new Map<string, Cached>();
  private inflight = new Map<string, Inflight>();
  private active = 0;
  private waiters = new Set<() => void>();
  private getPage: ReadModelPageGetter;
  constructor(getPage: ReadModelPageGetter) { this.getPage=getPage; }
  cached(manifest: RulesManifest): readonly unknown[] | undefined {
    captureRulesManifest({pages:manifest});
    const cached = this.cache.get(manifest.id);
    if (!cached) return undefined;
    this.same(cached.manifest,manifest);
    this.cache.delete(manifest.id); this.cache.set(manifest.id,cached);
    return cached.rules;
  }
  resolve(manifest: RulesManifest, signal: AbortSignal): Promise<readonly unknown[]> {
    if (signal.aborted) return Promise.reject(cancelled());
    try { const cached = this.cached(manifest); if (cached) return Promise.resolve(cached); }
    catch (error) { return Promise.reject(error); }
    let job = this.inflight.get(manifest.id);
    try { if (job) this.same(job.manifest,manifest); } catch (error) { return Promise.reject(error); }
    if (!job) {
      const controller = new AbortController();
      job = {manifest,controller,users:0,promise:Promise.resolve([])};
      const current = job;
      this.inflight.set(manifest.id,current);
      current.promise = this.load(manifest,controller.signal).then(rules => {
        if (controller.signal.aborted) throw cancelled();
        this.cache.set(manifest.id,{manifest,rules});
        while (this.cache.size > 2) this.cache.delete(this.cache.keys().next().value!);
        return rules;
      }).finally(() => { if (this.inflight.get(manifest.id) === current) this.inflight.delete(manifest.id); });
    }
    const current = job; current.users++;
    return new Promise((resolve,reject) => {
      let done = false;
      const finish = (error?: unknown, rules?: readonly unknown[]) => {
        if (done) return; done=true; signal.removeEventListener('abort',abort); current.users--;
        if (current.users === 0) {
          current.controller.abort();
          if (this.inflight.get(manifest.id) === current) this.inflight.delete(manifest.id);
        }
        if (error) reject(error); else resolve(rules!);
      };
      const abort = () => finish(cancelled());
      signal.addEventListener('abort',abort,{once:true});
      current.promise.then(rules => finish(undefined,rules),error => finish(error));
      if (signal.aborted) abort();
    });
  }
  private same(a: RulesManifest, b: RulesManifest): void { valid(a.id === b.id && a.bytes === b.bytes && a.count === b.count); }
  private async acquire(signal: AbortSignal): Promise<() => void> {
    while (this.active >= 2) {
      if (signal.aborted) throw cancelled();
      valid(this.waiters.size < 2);
      await new Promise<void>((resolve,reject) => {
        const wake = () => { signal.removeEventListener('abort',abort);this.waiters.delete(wake);resolve(); };
        const abort = () => { this.waiters.delete(wake);reject(cancelled()); };
        this.waiters.add(wake);signal.addEventListener('abort',abort,{once:true});
        if (signal.aborted) abort();
      });
    }
    if (signal.aborted) throw cancelled();
    this.active++;
    return () => {this.active--;for (const wake of [...this.waiters]) wake();};
  }
  private async load(manifest: RulesManifest, signal: AbortSignal): Promise<readonly unknown[]> {
    const release = await this.acquire(signal);
    try {
      const bytes = new Uint8Array(manifest.bytes);let offset=0;
      while (offset < manifest.bytes) {
        if (signal.aborted) throw cancelled();
        const page = await this.getPage(manifest.id,offset,signal);
        if (signal.aborted) throw cancelled();
        valid(record(page) && Object.keys(page).length === 5 && page.id === manifest.id && page.offset === offset
          && page.total === manifest.bytes && integer(page.nextOffset,offset+1,manifest.bytes) && typeof page.chunk === 'string');
        const chunk = encoder.encode(page.chunk);
        valid(chunk.length > 0 && chunk.length <= 8192 && page.nextOffset === offset + chunk.length
          && new TextDecoder('utf-8',{fatal:true}).decode(chunk) === page.chunk);
        bytes.set(chunk,offset);offset=page.nextOffset;
      }
      const digest = new Uint8Array(await crypto.subtle.digest('SHA-256',bytes));
      if (signal.aborted) throw cancelled();
      valid(`sha256:${[...digest].map(byte=>byte.toString(16).padStart(2,'0')).join('')}` === manifest.id);
      const rules: unknown = JSON.parse(new TextDecoder('utf-8',{fatal:true}).decode(bytes));
      valid(Array.isArray(rules) && rules.length === manifest.count);
      const freeze = (value: unknown) => { if (value && typeof value === 'object') { Object.values(value).forEach(freeze);Object.freeze(value); } };
      freeze(rules);return rules as DeepReadonly<unknown[]>;
    } finally { release(); }
  }
}
