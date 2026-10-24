import type { CacheStore } from '../companion-cache.ts';
type Invoke = <T>(command: string, args: Record<string, unknown>) => Promise<T>;
interface Snapshot { revision: number; payload: string | null; error: string | null }
const revisionOf = (snapshot: Snapshot) => {
  if (!Number.isSafeInteger(snapshot?.revision) || snapshot.revision < 0) throw new Error('Invalid native cache revision');
  return snapshot.revision;
};
/** An acknowledged native snapshot owns its revision; errors never select IDB. */
export function nativeCacheStore(invoke: Invoke): CacheStore {
  let revision: number | undefined;
  return {
    async read() {
      const snapshot = await invoke<Snapshot>('companion_cache_request', { request: { op: 'read' } });
      revision = revisionOf(snapshot);
      if (snapshot.error) throw new Error(snapshot.error);
      if (snapshot.payload !== null && typeof snapshot.payload !== 'string') throw new Error('Invalid native cache payload');
      return snapshot.payload;
    },
    async write(payload) {
      if (revision === undefined) throw new Error('Read the native cache before saving');
      const snapshot = await invoke<Snapshot>('companion_cache_request', { request: { op: 'write', expectedRevision: revision, payload } });
      const next = revisionOf(snapshot);
      if (snapshot.error) throw new Error(snapshot.error);
      if (next <= revision) throw new Error('Native cache save was not acknowledged');
      revision = next;
    },
  };
}
export function publicationCacheStore(options: {
  native?: boolean;
  invoke: Invoke;
  browser: () => CacheStore;
}): CacheStore {
  const native = options.native ?? ('__TAURI_INTERNALS__' in globalThis);
  return native ? nativeCacheStore(options.invoke) : options.browser();
}
