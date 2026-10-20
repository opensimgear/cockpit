import {
  publishDocument, validateLibrary, deleteDraft,
  type DesignerLibrary, type Publication,
} from '../../packages/companion-designer/src/index.ts';

/** Local designer data only. Pairings and native controller assignments never enter this store. */
export interface CompanionLibrarySnapshot {
  schemaVersion: 1;
  library: DesignerLibrary;
  publications: Record<string, Publication[]>;
}

/** A failed write must leave the previously committed value intact. */
export interface CompanionLibraryStorage {
  read(): Promise<unknown>;
  write(snapshot: CompanionLibrarySnapshot): Promise<void>;
}

export const COMPANION_LIBRARY_LIMITS = Object.freeze({ bytes: 64 * 1024 * 1024, revisions: 100 });
const encoder = new TextEncoder();
const idPattern = /^[A-Za-z0-9][A-Za-z0-9_-]{0,63}$/;
const emptySnapshot = (): CompanionLibrarySnapshot => ({ schemaVersion: 1, library: { schemaVersion: 1, drafts: [] }, publications: {} });

/** One saved state per library panel; retained transport snapshots stay internal. */
export function latestSavedPanels(library: DesignerLibrary, publications: Record<string, Publication[]>): Publication[] {
  return library.drafts.flatMap(draft => publications[draft.id]?.slice(-1) ?? []);
}

function capture(input: unknown): unknown {
  const json = JSON.stringify(input);
  if (!json || encoder.encode(json).length > COMPANION_LIBRARY_LIMITS.bytes) throw new Error('Companion library exceeds its 64 MiB storage budget.');
  return JSON.parse(json) as unknown;
}

function exact(input: unknown, fields: string[], description: string): Record<string, unknown> {
  if (!input || typeof input !== 'object' || Array.isArray(input)) throw new Error(`Invalid ${description}.`);
  const value = input as Record<string, unknown>;
  if (Object.keys(value).length !== fields.length || fields.some(field => !Object.hasOwn(value, field))) throw new Error(`Unexpected or missing ${description} field.`);
  return value;
}

async function validateSnapshot(input: unknown): Promise<CompanionLibrarySnapshot> {
  const value = exact(input, ['schemaVersion', 'library', 'publications'], 'companion library snapshot');
  if (value.schemaVersion !== 1) throw new Error('Unsupported companion library storage version.');
  const library = await validateLibrary(value.library);
  if (!library.ok) throw new Error(`Invalid saved companion drafts: ${library.error}`);
  if (!value.publications || typeof value.publications !== 'object' || Array.isArray(value.publications)) throw new Error('Invalid companion publication archive.');
  const groups = Object.entries(value.publications);
  if (groups.length > COMPANION_LIBRARY_LIMITS.revisions) throw new Error('Companion publication archive is full.');
  let count = 0;
  for (const [draftId, revisions] of groups) {
    if (!idPattern.test(draftId) || !Array.isArray(revisions) || (count += revisions.length) > COMPANION_LIBRARY_LIMITS.revisions) throw new Error('Invalid or oversized companion publication archive.');
  }
  const publications: Record<string, Publication[]> = {};
  for (const [draftId, revisions] of groups) {
    const validated: Publication[] = [];
    const seen = new Set<string>();
    for (const input of revisions as unknown[]) {
      const revision = exact(input, ['document', 'assets', 'publicationHash', 'capabilityFingerprint'], 'published revision');
      const recomputed = await publishDocument(revision.document as Publication['document'], revision.assets as Publication['assets'], { optimizeAssets: false });
      if (revision.publicationHash !== recomputed.publicationHash || revision.capabilityFingerprint !== recomputed.capabilityFingerprint) throw new Error('Published companion revision failed its content or capability hash check.');
      if (!seen.has(recomputed.publicationHash)) { seen.add(recomputed.publicationHash); validated.push(recomputed); }
    }
    publications[draftId] = validated;
  }
  return { schemaVersion: 1, library: library.value, publications };
}

/**
 * Panel documents are replaceable; transport snapshots remain immutable while retained.
 * Operations are serialized so a slow earlier validation cannot overwrite a later save.
 */
export class CompanionLibraryStore {
  private pending: Promise<unknown> = Promise.resolve();
  private readonly storage: CompanionLibraryStorage;
  constructor(storage: CompanionLibraryStorage = indexedDbStorage()) { this.storage = storage; }

  private ordered<T>(operation: () => Promise<T>): Promise<T> {
    const result = this.pending.then(operation);
    this.pending = result.catch(() => undefined);
    return result;
  }

  private async read(): Promise<CompanionLibrarySnapshot> {
    const stored = await this.storage.read();
    return stored === undefined ? emptySnapshot() : validateSnapshot(capture(stored));
  }

  deletePanel(id: string, pinnedHashes: string[]): Promise<CompanionLibrarySnapshot> {
    const pinned = new Set(pinnedHashes);
    return this.ordered(async () => {
      const snapshot = await this.read();
      snapshot.library = deleteDraft(snapshot.library, id);
      const retained = (snapshot.publications[id] ?? []).filter(revision => pinned.has(revision.publicationHash));
      if (retained.length) snapshot.publications[id] = retained; else delete snapshot.publications[id];
      await this.storage.write(structuredClone(snapshot));
      return snapshot;
    });
  }

  compact(pinnedHashes: string[]): Promise<CompanionLibrarySnapshot> {
    const pinned = new Set(pinnedHashes);
    return this.ordered(async () => {
      const snapshot = await this.read();
      const active = new Set(snapshot.library.drafts.map(d => d.id));
      for (const [id, revisions] of Object.entries(snapshot.publications)) {
        const current = active.has(id) ? revisions.at(-1)?.publicationHash : undefined;
        const retained = revisions.filter(p => p.publicationHash===current || pinned.has(p.publicationHash));
        if (retained.length) snapshot.publications[id]=retained; else delete snapshot.publications[id];
      }
      await this.storage.write(structuredClone(snapshot));
      return snapshot;
    });
  }

  load(): Promise<CompanionLibrarySnapshot> { return this.ordered(() => this.read()); }

  async save(input: CompanionLibrarySnapshot): Promise<CompanionLibrarySnapshot> {
    // Capture before queuing or awaiting: caller edits cannot change an in-flight save.
    const captured = capture(input);
    return this.ordered(async () => {
      const incoming = await validateSnapshot(captured);
      const previous = await this.read();
      const publications: Record<string, Publication[]> = { ...previous.publications };
      for (const [draftId, revisions] of Object.entries(incoming.publications)) {
        const existing = Object.hasOwn(publications, draftId) ? publications[draftId] : [];
        const hashes = new Set(revisions.map(revision => revision.publicationHash));
        // Saving a previous document state must make it current again, even
        // when its immutable payload is already retained for a device.
        publications[draftId] = [...existing.filter(revision => !hashes.has(revision.publicationHash)), ...revisions];
      }
      const merged = { schemaVersion: 1 as const, library: incoming.library, publications };
      // Enforce the aggregate archive and byte limits after preserving old publications.
      const checked = await validateSnapshot(capture(merged));
      await this.storage.write(structuredClone(checked));
      return checked;
    });
  }
}

/** One IndexedDB record, committed atomically; no partially saved draft/publication pair. */
export function indexedDbStorage(factory: IDBFactory | undefined = globalThis.indexedDB): CompanionLibraryStorage {
  let database: Promise<IDBDatabase> | undefined;
  function open(): Promise<IDBDatabase> {
    if (!factory) return Promise.reject(new Error('IndexedDB is unavailable; companion drafts have not been changed.'));
    if (!database) {
      database = new Promise<IDBDatabase>((resolve, reject) => {
        let rejected = false;
        const fail = (error: unknown) => { rejected = true; reject(error); };
        const request = factory.open('opensimgear-cockpit-companion-designer', 1);
        request.onupgradeneeded = () => { if (!request.result.objectStoreNames.contains('workspace')) request.result.createObjectStore('workspace'); };
        request.onsuccess = () => {
          const db = request.result;
          if (rejected) { db.close(); return; }
          db.onversionchange = () => { db.close(); database = undefined; };
          resolve(db);
        };
        request.onerror = () => fail(request.error ?? new Error('Could not open companion draft storage.'));
        request.onblocked = () => fail(new Error('Companion storage is blocked by another open app window.'));
      }).catch(error => { database = undefined; throw error; });
    }
    return database;
  }
  async function transaction(mode: IDBTransactionMode, snapshot?: CompanionLibrarySnapshot): Promise<unknown> {
    const db = await open();
    return new Promise((resolve, reject) => {
      const tx = db.transaction('workspace', mode);
      const request = snapshot === undefined ? tx.objectStore('workspace').get('snapshot') : tx.objectStore('workspace').put(snapshot, 'snapshot');
      tx.oncomplete = () => resolve(snapshot === undefined ? request.result : undefined);
      tx.onerror = () => reject(tx.error ?? new Error('Companion storage transaction failed.'));
      tx.onabort = () => reject(tx.error ?? new Error('Companion storage transaction was cancelled.'));
    });
  }
  return { read: () => transaction('readonly'), write: async snapshot => { await transaction('readwrite', snapshot); } };
}
