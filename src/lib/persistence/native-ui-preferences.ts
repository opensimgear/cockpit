import { invoke, isTauri } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import { gatewayRequest } from '../gateway';
import { uiPreferences, UI_KEYS, type UiKey, type UiSnapshot } from './ui-preferences';

/** Bootstrap finishes before any desktop view can render or save defaults. */
export async function bootstrapUiPreferences(): Promise<void> {
  if (!isTauri()) return;
  uiPreferences.connect({
    async read() {
      return invoke<UiSnapshot>('persistence_ui_read');
    },
    async write(key, expectedRevision, value) {
      const state = await gatewayRequest({ command: 'ui_preferences_save', key, expectedRevision, value });
      return state.uiPreferences as { revision: number; value: unknown };
    },
  });
  try {
    await listen<{ key: UiKey; revision: number }>('ui-preferences-changed', event => {
      if (UI_KEYS.includes(event.payload.key)) void uiPreferences.changed(event.payload.key, event.payload.revision);
    });
  } catch { /* Focus still reconciles durable state if native event setup fails. */ }
  window.addEventListener('focus', () => { void uiPreferences.hydrate(); });
  await uiPreferences.hydrate();
}
