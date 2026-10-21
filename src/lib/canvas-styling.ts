export type CanvasArrowStyle = 'smooth' | 'orthogonal';

export type CanvasStyling = Readonly<{
  arrowStyle: CanvasArrowStyle;
  cornerRadius: number;
  storageError: string | null;
}>;

export type CanvasStylingStorage = Pick<Storage, 'getItem' | 'setItem'>;
export const CANVAS_STYLING_STORAGE_KEY = 'osg-canvas-styling';

const defaults = { arrowStyle: 'smooth' as CanvasArrowStyle, cornerRadius: 24 };
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
  let storageUnavailable = false;
  if (storage === undefined) {
    try { storage = typeof localStorage === 'undefined' ? null : localStorage; }
    catch { storage = null; storageError = loadError; storageUnavailable = true; }
  }
  let preferences = { ...defaults };
  if (storage) {
    try {
      const saved: unknown = JSON.parse(storage.getItem(CANVAS_STYLING_STORAGE_KEY) ?? 'null');
      if (saved && typeof saved === 'object' && !Array.isArray(saved)) {
        const value = saved as Record<string, unknown>;
        preferences = {
          arrowStyle: value.arrowStyle === 'orthogonal' ? 'orthogonal' : 'smooth',
          cornerRadius: cornerRadius(value.cornerRadius),
        };
      }
    } catch { storageError = loadError; }
  }
  let current: CanvasStyling = Object.freeze({ ...preferences, storageError });
  const subscribers = new Set<(value: CanvasStyling) => void>();

  function update(next: typeof preferences) {
    let error: string | null = storageUnavailable ? saveError : null;
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
      update({ arrowStyle: style, cornerRadius: current.cornerRadius });
    },
    setCornerRadius(value: number) {
      if (!Number.isFinite(value)) return;
      const radius = cornerRadius(value);
      if (radius === current.cornerRadius && !current.storageError) return;
      update({ arrowStyle: current.arrowStyle, cornerRadius: radius });
    },
  };
}

export const canvasStyling = createCanvasStylingStore();
export const setArrowStyle = canvasStyling.setArrowStyle;
export const setCornerRadius = canvasStyling.setCornerRadius;
