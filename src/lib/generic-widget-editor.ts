/** Source edits are immutable. Compiled output is deliberately not part of undo history. */
export interface SourceHistory<T> { past: T[]; present: T; future: T[] }
const HISTORY_LIMIT = 80;

export type ResizeHandle = 'nw' | 'n' | 'ne' | 'e' | 'se' | 's' | 'sw' | 'w';
export interface SelectionBounds { x: number; y: number; width: number; height: number }
/** Opposite edges stay anchored; corner resizing intentionally leaves the axes independent. */
export function resizeSelection(bounds: SelectionBounds, handle: ResizeHandle, dx: number, dy: number): SelectionBounds {
  let { x, y, width, height } = bounds;
  if (handle.includes('w')) { x = Math.min(bounds.x + dx, bounds.x + bounds.width - 1); width = bounds.x + bounds.width - x; }
  if (handle.includes('e')) width = Math.max(1, bounds.width + dx);
  if (handle.includes('n')) { y = Math.min(bounds.y + dy, bounds.y + bounds.height - 1); height = bounds.y + bounds.height - y; }
  if (handle.includes('s')) height = Math.max(1, bounds.height + dy);
  return { x, y, width, height };
}

export type LayerPlacement = 'before' | 'after' | 'inside' | 'root';

/** Move complete subtrees, rejecting cycles and nesting beyond the source depth limit. */
export function moveLayer<T extends { id: string; children?: T[] }>(nodes: T[], id: string, targetId: string, placement: LayerPlacement = 'before', maxDepth = 8): boolean {
  function locate(list: T[], key: string, depth = 0): { list: T[]; node: T; index: number; depth: number } | undefined {
    const index = list.findIndex(node => node.id === key);
    if (index >= 0) return { list, node: list[index], index, depth };
    for (const node of list) {
      const found = node.children && locate(node.children, key, depth + 1);
      if (found) return found;
    }
  }
  const source = locate(nodes, id), target = locate(nodes, targetId);
  if (!source || placement !== 'root' && (!target || id === targetId || source.node.children && locate(source.node.children, targetId))) return false;
  if (placement === 'inside' && !target!.node.children) return false;
  const list = placement === 'root' ? nodes : placement === 'inside' ? target!.node.children! : target!.list;
  const depth = placement === 'root' ? 0 : target!.depth + Number(placement === 'inside');
  const subtreeDepth = (node: T): number => node.children ? 1 + Math.max(0, ...node.children.map(subtreeDepth)) : 0;
  if (depth + subtreeDepth(source.node) > maxDepth) return false;
  let index = placement === 'root' || placement === 'inside' ? list.length : target!.index + Number(placement === 'after');
  if (source.list === list && source.index < index) index--;
  if (source.list === list && source.index === index) return false;
  source.list.splice(source.index, 1);
  list.splice(index, 0, source.node);
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
