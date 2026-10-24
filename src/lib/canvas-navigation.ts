import { uiPreferences } from './persistence/ui-preferences';
export type CanvasWheelAction = 'pan' | 'zoom';
export type CanvasNavigation = Readonly<{
  wheelAction: CanvasWheelAction;
  storageError: string | null;
  saving?: boolean;
  ready?: boolean;
}>;
export type CanvasNavigationStorage = Pick<Storage, 'getItem' | 'setItem'>;
export const CANVAS_NAVIGATION_STORAGE_KEY = 'osg-canvas-navigation';

const loadError = 'Canvas navigation could not be loaded locally. Default navigation is active for this session.';
const saveError = 'Changes apply to this session. Canvas navigation could not be saved locally.';

/** Local canvas controls are app preferences, independent of profile history. */
export function createCanvasNavigationStore(storage?: CanvasNavigationStorage | null) {
  let storageError: string | null = null;
  let wheelAction: CanvasWheelAction = 'pan';
  if (storage) {
    try {
      const saved: unknown = JSON.parse(storage.getItem(CANVAS_NAVIGATION_STORAGE_KEY) ?? 'null');
      if (saved && typeof saved === 'object' && !Array.isArray(saved) && (saved as Record<string, unknown>).wheelAction === 'zoom') wheelAction = 'zoom';
    } catch { storageError = loadError; }
  }
  let current: CanvasNavigation = Object.freeze({ wheelAction, storageError });
  const subscribers = new Set<(value: CanvasNavigation) => void>();
  return {
    subscribe(subscriber: (value: CanvasNavigation) => void) {
      subscribers.add(subscriber);
      subscriber(current);
      return () => { subscribers.delete(subscriber); };
    },
    setWheelAction(action: CanvasWheelAction) {
      if (action !== 'pan' && action !== 'zoom' || action === current.wheelAction && !current.storageError) return;
      let error: string | null = null;
      try { storage?.setItem(CANVAS_NAVIGATION_STORAGE_KEY, JSON.stringify({ wheelAction: action })); }
      catch { error = saveError; }
      current = Object.freeze({ wheelAction: action, storageError: error });
      for (const subscriber of subscribers) subscriber(current);
    },
  };
}

export const canvasNavigation = {
  subscribe(listener: (value: CanvasNavigation) => void) {
    return uiPreferences.subscribe('canvas-navigation', entry => listener(Object.freeze({ ...entry.value, storageError: entry.error || null, saving: entry.busy, ready: entry.ready })));
  },
  setWheelAction(action: CanvasWheelAction) {
    if (action !== 'pan' && action !== 'zoom') return;
    const current = uiPreferences.get('canvas-navigation');
    if (current.value.wheelAction === action && !current.error) return;
    void uiPreferences.save('canvas-navigation', { wheelAction: action }).catch(() => {});
  },
};
export const setWheelAction = canvasNavigation.setWheelAction;

/** Shared wheel intent for interactive canvases; modifiers reverse the preference. */
export function canvasWheelNavigation(action: CanvasWheelAction, event: Pick<WheelEvent, 'ctrlKey'|'metaKey'|'deltaMode'|'deltaX'|'deltaY'>, pageHeight: number) {
  const unit = event.deltaMode === 1 ? 16 : event.deltaMode === 2 ? pageHeight : 1;
  return {
    zoom: (action === 'zoom') !== !!(event.ctrlKey || event.metaKey),
    deltaX: event.deltaX * unit,
    deltaY: event.deltaY * unit,
    multiplier: event.deltaY === 0 ? 1 : event.deltaY < 0 ? 1.1 : 1 / 1.1,
  };
}
