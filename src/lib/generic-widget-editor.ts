/** Source edits are immutable. Compiled output is deliberately not part of undo history. */
export interface SourceHistory<T> { past: T[]; present: T; future: T[] }
const HISTORY_LIMIT = 80;

/** Reorder siblings without reparenting a layer or separating a group from its children. */
export function moveSiblingLayer<T extends { id: string; children?: T[] }>(nodes: T[], id: string, targetId: string, after = false): boolean {
  function siblings(list: T[], key: string): T[] | undefined {
    if (list.some(node => node.id === key)) return list;
    for (const node of list) {
      const found = node.children && siblings(node.children, key);
      if (found) return found;
    }
  }
  const list = siblings(nodes, id);
  if (!list || list !== siblings(nodes, targetId) || id === targetId) return false;
  const from = list.findIndex(node => node.id === id);
  let to = list.findIndex(node => node.id === targetId) + Number(after);
  if (from < to) to--;
  if (from === to) return false;
  const [node] = list.splice(from, 1);
  list.splice(to, 0, node);
  return true;
}

/** Stable identity includes all authoring values, independently of property insertion order. */
export function sourceIdentity(value: unknown): string {
  if (Array.isArray(value)) return '[' + value.map(sourceIdentity).join(',') + ']';
  if (value && typeof value === 'object') return '{' + Object.keys(value).sort().map(key => JSON.stringify(key) + ':' + sourceIdentity((value as Record<string, unknown>)[key])).join(',') + '}';
  const encoded = JSON.stringify(value);
  if (encoded === undefined || typeof value === 'number' && !Number.isFinite(value)) throw new Error('Source values must be finite and serializable.');
  return encoded;
}

export function createSourceHistory<T>(source: T): SourceHistory<T> {
  sourceIdentity(source);
  return { past: [], present: structuredClone(source), future: [] };
}
export function editSource<T>(history: SourceHistory<T>, next: T): SourceHistory<T> {
  if (sourceIdentity(history.present) === sourceIdentity(next)) return history;
  return { past: [...history.past, structuredClone(history.present)].slice(-HISTORY_LIMIT), present: structuredClone(next), future: [] };
}
export function undoSource<T>(history: SourceHistory<T>): SourceHistory<T> {
  if (!history.past.length) return history;
  return { past: history.past.slice(0, -1), present: structuredClone(history.past.at(-1)!), future: [structuredClone(history.present), ...history.future].slice(0, HISTORY_LIMIT) };
}
export function redoSource<T>(history: SourceHistory<T>): SourceHistory<T> {
  if (!history.future.length) return history;
  return { past: [...history.past, structuredClone(history.present)].slice(-HISTORY_LIMIT), present: structuredClone(history.future[0]), future: history.future.slice(1) };
}

/** Cancellation is cooperative; generation fencing also rejects workers that ignore AbortSignal. */
export class CompilationFence<T> {
  private revision = 0;
  private active?: AbortController;
  private current?: { identity: string; value: T };
  invalidate(): void { this.revision++; this.active?.abort(); this.active = undefined; this.current = undefined; }
  get(identity: string): T | undefined { return this.current?.identity === identity ? this.current.value : undefined; }
  async run(identity: string, build: (signal: AbortSignal) => Promise<T>): Promise<T | undefined> {
    this.invalidate();
    const revision = this.revision, controller = new AbortController();
    this.active = controller;
    try {
      const value = await build(controller.signal);
      if (revision !== this.revision || controller.signal.aborted) return undefined;
      this.current = { identity, value }; return value;
    } catch (error) {
      if (revision !== this.revision || controller.signal.aborted) return undefined;
      throw error;
    } finally { if (revision === this.revision) this.active = undefined; }
  }
}
