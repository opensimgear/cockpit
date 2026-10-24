import { uiPreferences } from './persistence/ui-preferences';
export type CanvasArrowStyle = 'smooth' | 'orthogonal';

export type CanvasStyling = Readonly<{
  arrowStyle: CanvasArrowStyle;
  cornerRadius: number;
  collapseParallelWires: boolean;
  storageError: string | null;
  saving?: boolean;
  ready?: boolean;
}>;

export type CanvasStylingStorage = Pick<Storage, 'getItem' | 'setItem'>;
export const CANVAS_STYLING_STORAGE_KEY = 'osg-canvas-styling';

const defaults = { arrowStyle: 'smooth' as CanvasArrowStyle, cornerRadius: 24, collapseParallelWires: true };
const loadError = 'Canvas styling could not be loaded locally. Default styling is active for this session.';
const saveError = 'Changes apply to this session. Canvas styling could not be saved locally.';

function cornerRadius(value: unknown): number {
  return typeof value === 'number' && Number.isFinite(value)
    ? Math.min(100, Math.max(0, value))
    : defaults.cornerRadius;
}

/** A plain readable store so canvas preferences also work without a Svelte runtime. */
export function createCanvasStylingStore(storage?: CanvasStylingStorage | null) {
  let storageError: string | null = null;
  let preferences = { ...defaults };
  if (storage) {
    try {
      const saved: unknown = JSON.parse(storage.getItem(CANVAS_STYLING_STORAGE_KEY) ?? 'null');
      if (saved && typeof saved === 'object' && !Array.isArray(saved)) {
        const value = saved as Record<string, unknown>;
        preferences = {
          arrowStyle: value.arrowStyle === 'orthogonal' ? 'orthogonal' : 'smooth',
          cornerRadius: cornerRadius(value.cornerRadius),
          collapseParallelWires: typeof value.collapseParallelWires === 'boolean' ? value.collapseParallelWires : defaults.collapseParallelWires,
        };
      }
    } catch { storageError = loadError; }
  }
  let current: CanvasStyling = Object.freeze({ ...preferences, storageError });
  const subscribers = new Set<(value: CanvasStyling) => void>();

  function update(next: typeof preferences) {
    let error: string | null = null;
    try { storage?.setItem(CANVAS_STYLING_STORAGE_KEY, JSON.stringify(next)); }
    catch { error = saveError; }
    current = Object.freeze({ ...next, storageError: error });
    for (const subscriber of subscribers) subscriber(current);
  }

  return {
    subscribe(subscriber: (value: CanvasStyling) => void) {
      subscribers.add(subscriber);
      subscriber(current);
      return () => { subscribers.delete(subscriber); };
    },
    setArrowStyle(style: CanvasArrowStyle) {
      if (style !== 'smooth' && style !== 'orthogonal') return;
      if (style === current.arrowStyle && !current.storageError) return;
      update({ arrowStyle: style, cornerRadius: current.cornerRadius, collapseParallelWires: current.collapseParallelWires });
    },
    setCornerRadius(value: number) {
      if (!Number.isFinite(value)) return;
      const radius = cornerRadius(value);
      if (radius === current.cornerRadius && !current.storageError) return;
      update({ arrowStyle: current.arrowStyle, cornerRadius: radius, collapseParallelWires: current.collapseParallelWires });
    },
    setCollapseParallelWires(value: boolean) {
      if (typeof value !== 'boolean') return;
      if (value === current.collapseParallelWires && !current.storageError) return;
      update({ arrowStyle: current.arrowStyle, cornerRadius: current.cornerRadius, collapseParallelWires: value });
    },
  };
}

export const canvasStyling = {
  subscribe(listener: (value: CanvasStyling) => void) {
    return uiPreferences.subscribe('canvas-styling', entry => listener(Object.freeze({ ...entry.value, storageError: entry.error || null, saving: entry.busy, ready: entry.ready })));
  },
  setArrowStyle(style: CanvasArrowStyle) {
    if (style !== 'smooth' && style !== 'orthogonal') return;
    const current = uiPreferences.get('canvas-styling');
    if (current.value.arrowStyle === style && !current.error) return;
    void uiPreferences.save('canvas-styling', { ...current.value, arrowStyle: style }).catch(() => {});
  },
  setCornerRadius(value: number) {
    if (!Number.isFinite(value)) return;
    const current = uiPreferences.get('canvas-styling'), radius = cornerRadius(value);
    if (current.value.cornerRadius === radius && !current.error) return;
    void uiPreferences.save('canvas-styling', { ...current.value, cornerRadius: radius }).catch(() => {});
  },
  setCollapseParallelWires(value: boolean) {
    if (typeof value !== 'boolean') return;
    const current = uiPreferences.get('canvas-styling');
    if (current.value.collapseParallelWires === value && !current.error) return;
    void uiPreferences.save('canvas-styling', { ...current.value, collapseParallelWires: value }).catch(() => {});
  },
};
export const setArrowStyle = canvasStyling.setArrowStyle;
export const setCornerRadius = canvasStyling.setCornerRadius;
export const setCollapseParallelWires = canvasStyling.setCollapseParallelWires;
