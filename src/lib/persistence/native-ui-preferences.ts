import { invoke, isTauri } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import { gatewayRequest } from '../gateway';
import { SharedObservations } from '../streams/index.ts';
import { uiPreferences, UI_KEYS, OBSERVED_UI_KEYS, type UiKey, type UiSnapshot } from './ui-preferences';

/** Connect before render; subscription delivery is never a persistence query. */
export async function bootstrapUiPreferences(): Promise<() => void> {
  const native = isTauri();
  uiPreferences.connect({
    async read() {
      if (native) return invoke<UiSnapshot>('persistence_ui_read');
      const state = await gatewayRequest({ command: 'ui_preferences_get' });
      return state.uiPreferences as UiSnapshot;
    },
    async write(key, expectedRevision, value) {
      const state = await gatewayRequest({ command: 'ui_preferences_save', key, expectedRevision, value });
      return state.uiPreferences as { revision: number; value: unknown };
    },
  });
  let disposed = false;
  let observations: SharedObservations | undefined;
  let unlisten: (() => void) | undefined;
  const focus = () => {
    if (disposed) return;
    const hydrating = uiPreferences.hydrate();
    subscribe();
    void hydrating;
  };
  let observed: readonly UiKey[] = OBSERVED_UI_KEYS;
  const cleanup = () => {
    if (disposed) return;
    disposed = true;
    observations?.close();
    unlisten?.();
    window.removeEventListener('focus', focus);
    window.removeEventListener('pagehide', cleanup);
    // Fence a bootstrap/focus read still resolving after window disposal.
    for (const key of UI_KEYS) uiPreferences.observationError(key, 'UI preference observation closed.');
  };
  window.addEventListener('pagehide', cleanup);
  if (native) {
    try {
      const { getCurrentWindow } = await import('@tauri-apps/api/window');
      if (getCurrentWindow().label === 'profile-activation') observed = ['theme', 'progress'];
    } catch {
      // An unidentified secondary window receives only the common display topics.
      observed = ['theme', 'progress'];
    }
  }
  if (disposed) return cleanup;
  function subscribe() {
    observations?.close();
    observations = new SharedObservations();
    for (const key of observed) {
      observations.subscribe(`preferences.${key}`, state => {
        if (disposed) return;
        if (state.status === 'ready' && state.frame) uiPreferences.applyObservation(key, state.frame.payload);
        else if (state.status === 'lost' || state.status === 'error') {
          uiPreferences.observationError(key, state.error ?? 'UI preference continuity lost.');
        }
      });
    }
  }
  // Start the single explicit read before subscription callbacks can overtake it.
  const hydrating = uiPreferences.hydrate();
  subscribe();
  if (native) {
    try {
      const stop = await listen<{ key: UiKey; revision: number }>('ui-preferences-changed', event => {
        if (!disposed && event.payload.key === 'remembered') void uiPreferences.changed('remembered', event.payload.revision);
      });
      if (disposed) stop(); else unlisten = stop;
    } catch { /* Explicit focus reconciliation also covers the remembered inventory. */ }
  }
  if (!disposed) {
    window.addEventListener('focus', focus);
    await hydrating;
  }
  return cleanup;
}
