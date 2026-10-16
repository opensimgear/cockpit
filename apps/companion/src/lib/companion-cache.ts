import { canonical, DESIGNER_LIMITS, publishDocument, type AssetPayload, type DesignerDocument, type Publication } from '@opensimgear/companion-designer';

export const CACHE_LIMITS = Object.freeze({ entries: 4, bytes: 24 * 1024 * 1024, entryBytes: DESIGNER_LIMITS.bundleBytes });
export interface CacheOwner { hostId: string; instanceId: string; name: string }
export interface CachedPanel extends CacheOwner { key: string; publication: Publication; savedAtMs: number; pageId: string }
export interface CacheSummary extends CacheOwner { key: string; title: string; publicationHash: string; savedAtMs: number; pageId: string }
export interface CacheStore { read(): Promise<unknown>; write(serialized: string): Promise<void> }
export interface PublishedPayload { canonicalDocument: string; document: unknown; assets: AssetPayload[]; publicationHash: string }
const clone = <T,>(value: T): T => structuredClone(value);
const size = (value: string) => new TextEncoder().encode(value).length;
function object(value: unknown): Record<string, unknown> { if (!value || typeof value !== 'object' || Array.isArray(value)) throw new Error('Invalid cached panel'); return value as Record<string, unknown>; }
function text(value: unknown, max: number): string { if (typeof value !== 'string' || !value || value.length > max || /[\u0000-\u001f]/.test(value)) throw new Error('Invalid cache identity'); return value; }
export function cacheKey(owner: Pick<CacheOwner, 'hostId' | 'instanceId'>): string { return JSON.stringify([text(owner.hostId, 64), text(owner.instanceId, 64)]); }
export async function verifyPublishedPayload(payload: PublishedPayload): Promise<Publication> {
  if (typeof payload.canonicalDocument !== 'string' || size(payload.canonicalDocument) > 1024 * 1024 || typeof payload.publicationHash !== 'string') throw new Error('Invalid published payload');
  const parsed: unknown = JSON.parse(payload.canonicalDocument);
  // Both representations must be exactly the same immutable document, including canonical bytes.
  if (canonical(parsed) !== payload.canonicalDocument || canonical(payload.document) !== payload.canonicalDocument) throw new Error('Publication document differs from its canonical bytes');
  const verified = await publishDocument(parsed as DesignerDocument, payload.assets);
  if (verified.publicationHash !== payload.publicationHash) throw new Error('Published layout hash differs from the verified transfer');
  return verified;
}
function summary(entry: CachedPanel): CacheSummary { return { hostId: entry.hostId, instanceId: entry.instanceId, name: entry.name, key: entry.key, title: entry.publication.document.ui.title, publicationHash: entry.publication.publicationHash, savedAtMs: entry.savedAtMs, pageId: entry.pageId }; }

/** A bounded public-content cache. Every disk read is untrusted; it never contains authority. */
export function createPublicationCache(store: CacheStore, options: { clock?: () => number; maxBytes?: number; maxEntries?: number } = {}) {
  const clock = options.clock ?? Date.now, maxBytes = options.maxBytes ?? CACHE_LIMITS.bytes, maxEntries = options.maxEntries ?? CACHE_LIMITS.entries;
  let entries: CachedPanel[] | null = null, pending = 0, tail: Promise<unknown> = Promise.resolve();
  function serialized(value: CachedPanel[]) { return JSON.stringify({ version: 1, entries: value }); }
  async function load() {
    if (entries) return;
    const raw = await store.read();
    if (raw === null || raw === undefined) { entries = []; return; }
    if (typeof raw !== 'string' || raw.length > maxBytes || size(raw) > maxBytes) throw new Error('Offline cache exceeds its storage budget');
    const root = object(JSON.parse(raw));
    if (root.version !== 1 || Object.keys(root).some(key => !['version', 'entries'].includes(key)) || !Array.isArray(root.entries) || root.entries.length > maxEntries) throw new Error('Unsupported offline cache');
    const checked: CachedPanel[] = [], keys = new Set<string>();
    for (const value of root.entries) {
      const entry = object(value), owner = { hostId: text(entry.hostId, 64), instanceId: text(entry.instanceId, 64), name: text(entry.name, 80) }, key = cacheKey(owner);
      if (Object.keys(entry).some(field => !['hostId','instanceId','name','key','publication','savedAtMs','pageId'].includes(field)) || entry.key !== key || keys.has(key) || !Number.isSafeInteger(entry.savedAtMs) || Number(entry.savedAtMs) < 0 || size(JSON.stringify(entry)) > CACHE_LIMITS.entryBytes) throw new Error('Invalid cached publication metadata');
      const saved = object(entry.publication);
      const publication = await publishDocument(saved.document as DesignerDocument, saved.assets as AssetPayload[]);
      if (publication.publicationHash !== saved.publicationHash || publication.capabilityFingerprint !== saved.capabilityFingerprint) throw new Error('Cached publication digest changed');
      const pageId = text(entry.pageId, 64);
      if (!publication.document.ui.pages.some(page => page.id === pageId)) throw new Error('Cached page is absent from publication');
      checked.push({ ...owner, key, publication, savedAtMs: Number(entry.savedAtMs), pageId }); keys.add(key);
    }
    entries = checked;
  }
  function queued<T>(action: () => Promise<T>): Promise<T> { if (pending >= 16) return Promise.reject(new Error('Offline cache is busy; try again after pending saves finish')); pending++; const result = tail.then(action).finally(() => { pending--; }); tail = result.catch(() => {}); return result; }
  async function commit(next: CachedPanel[]) { const raw = serialized(next); if (size(raw) > maxBytes) throw new Error('Offline cache is full'); await store.write(raw); entries = next; }
  return {
    list: () => queued(async () => { await load(); return entries!.map(summary); }),
    open: (key: string) => queued(async () => { await load(); const entry = entries!.find(value => value.key === key); if (!entry) throw new Error('Cached panel is unavailable'); return clone(entry); }),
    save: (owner: CacheOwner, publication: Publication, pageId?: string) => queued(async () => {
      await load(); const key = cacheKey(owner), verified = await publishDocument(publication.document, publication.assets);
      if (verified.publicationHash !== publication.publicationHash || verified.capabilityFingerprint !== publication.capabilityFingerprint) throw new Error('Only verified publications can be cached');
      const old = entries!.find(entry => entry.key === key), nextPage = pageId ?? (old?.publication.publicationHash === verified.publicationHash ? old.pageId : verified.document.ui.pages[0].id);
      if (!verified.document.ui.pages.some(page => page.id === nextPage)) throw new Error('Unknown panel page');
      const entry: CachedPanel = { hostId: text(owner.hostId, 64), instanceId: text(owner.instanceId, 64), name: text(owner.name, 80), key, publication: verified, savedAtMs: clock(), pageId: nextPage };
      if (!Number.isSafeInteger(entry.savedAtMs) || entry.savedAtMs < 0 || size(JSON.stringify(entry)) > CACHE_LIMITS.entryBytes || size(serialized([entry])) > maxBytes) throw new Error('Panel exceeds offline cache budget');
      let next = [entry, ...entries!.filter(value => value.key !== key)].slice(0, maxEntries);
      while (size(serialized(next)) > maxBytes) next.pop();
      await commit(next); return summary(entry);
    }),
    rememberPage: (key: string, pageId: string) => queued(async () => { await load(); const next = clone(entries!), entry = next.find(value => value.key === key); if (!entry) return; if (!entry.publication.document.ui.pages.some(page => page.id === pageId)) throw new Error('Unknown panel page'); entry.pageId = pageId; await commit(next); }),
    remove: (key: string) => queued(async () => { await load(); await commit(entries!.filter(entry => entry.key !== key)); }),
    clear: () => queued(async () => { await commit([]); }),
  };
}

/** IndexedDB commits one bounded library atomically; private credentials never enter this store. */
export function browserCacheStore(factory: IDBFactory | undefined = globalThis.indexedDB): CacheStore {
  let database: Promise<IDBDatabase> | undefined;
  function open() { return database ??= new Promise<IDBDatabase>((resolve, reject) => {
    if (!factory) { reject(new Error('Durable offline storage is unavailable')); return; }
    const request = factory.open('opensimgear-companion-publications', 1);
    request.onupgradeneeded = () => { if (!request.result.objectStoreNames.contains('cache')) request.result.createObjectStore('cache'); };
    request.onerror = () => reject(new Error('Cannot open offline panel storage'));
    request.onblocked = () => reject(new Error('Offline storage is blocked by another app window'));
    request.onsuccess = () => { request.result.onversionchange = () => request.result.close(); resolve(request.result); };
  }); }
  async function transaction<T>(mode: IDBTransactionMode, action: (store: IDBObjectStore) => IDBRequest<T>): Promise<T> {
    const db = await open();
    return new Promise<T>((resolve, reject) => { const tx = db.transaction('cache', mode), request = action(tx.objectStore('cache')); let result: T;
      request.onsuccess = () => { result = request.result; }; tx.oncomplete = () => resolve(result);
      tx.onerror = tx.onabort = () => reject(new Error('Offline storage commit failed; available space may be exhausted'));
    });
  }
  return { read: () => transaction('readonly', store => store.get('library')), write: async value => { await transaction('readwrite', store => store.put(value, 'library')); } };
}
