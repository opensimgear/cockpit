/** Desktop designer content has one native SQLite authority. Browser previews
 * choose their separate storage adapter explicitly; native errors never fall back. */
export type PersistenceInvoke = (command: string, arguments_: Record<string, unknown>) => Promise<unknown>;
export interface DesignerStorage<T> {
  read(): Promise<unknown>;
  write(value: T): Promise<T | void>;
}

function revision(value: unknown): number {
  if (!Number.isSafeInteger(value) || (value as number) < 0) throw new Error('Invalid designer storage revision. Reload the designer.');
  return value as number;
}
function response(value: unknown): { revision: number; libraryRevision:number; value: unknown } {
  if (!value || typeof value !== 'object' || Array.isArray(value)) throw new Error('Invalid designer storage response.');
  const record = value as Record<string, unknown>;
  return { revision: revision(record.revision), libraryRevision:revision(record.libraryRevision), value: record.value };
}

export function sqliteDesignerStorage<T>(invoke: PersistenceInvoke): DesignerStorage<T> {
  let expected = 0, expectedLibrary=0;
  return {
    async read() {
      const result = await invoke('persistence_workspace', { request: { operation: 'read' } });
      if (!result || typeof result !== 'object' || !Object.hasOwn(result, 'value')) throw new Error('Invalid designer storage response.');
      const record = response(result);
      expected = record.revision; expectedLibrary=record.libraryRevision;
      return record.value === null ? undefined : record.value;
    },
    async write(value) {
      const record = response(await invoke('persistence_workspace', {
        request: { operation: 'write', expectedRevision: expected, expectedLibraryRevision:expectedLibrary, value },
      }));
      if (record.revision !== expected + 1) throw new Error('Designer save acknowledgement changed. Reload before editing again.');
      expected = record.revision; expectedLibrary=record.libraryRevision;
      return record.value as T | undefined;
    },
  };
}

export function desktopDesignerStorage<T>(): DesignerStorage<T> | undefined {
  const native = (globalThis as typeof globalThis & {
    __TAURI_INTERNALS__?: { invoke: PersistenceInvoke };
  }).__TAURI_INTERNALS__;
  return native ? sqliteDesignerStorage<T>((command, arguments_) => native.invoke(command, arguments_)) : undefined;
}
