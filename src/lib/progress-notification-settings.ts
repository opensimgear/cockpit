export const PROGRESS_NOTIFICATION_POSITIONS = [
  ['bottom-right', 'Bottom right'],
  ['bottom-left', 'Bottom left'],
  ['top-right', 'Top right'],
  ['top-left', 'Top left'],
] as const;

export type ProgressNotificationPosition = typeof PROGRESS_NOTIFICATION_POSITIONS[number][0];
export type ProgressNotificationDelivery = 'system' | 'window';
export type ProgressNotificationSettings = { delivery: ProgressNotificationDelivery; position: ProgressNotificationPosition };
export type ProgressNotificationSettingsState = ProgressNotificationSettings & {
  native: boolean;
  loaded: boolean;
  busy: '' | 'load' | 'save';
  error: string;
  retry: '' | 'load' | 'save';
  message: string;
};
type Invoke = (command: string, args?: Record<string, unknown>) => Promise<unknown>;

export function progressNotificationSettings(value: unknown): ProgressNotificationSettings {
  const settings = value && typeof value === 'object' ? value as Record<string, unknown> : {};
  if (!PROGRESS_NOTIFICATION_POSITIONS.some(([id]) => id === settings.position) || !['system', 'window'].includes(String(settings.delivery))) {
    throw new Error('The notification settings are unavailable.');
  }
  return { delivery: settings.delivery as ProgressNotificationDelivery, position: settings.position as ProgressNotificationPosition };
}

export function createProgressNotificationSettings(
  invoke: Invoke,
  native: boolean,
  changed: (state: ProgressNotificationSettingsState) => void,
) {
  let state: ProgressNotificationSettingsState = {
    delivery: 'system', position: 'bottom-right', native, loaded: false, busy: '', error: '', retry: '', message: '',
  };
  let disposed = false;
  function update(patch: Partial<ProgressNotificationSettingsState>) {
    state = { ...state, ...patch };
    if (!disposed) changed({ ...state });
  }
  async function load() {
    if (!native || disposed || state.busy) return;
    update({ busy: 'load', error: '', retry: '', message: '' });
    try {
      const settings = progressNotificationSettings(await invoke('profile_progress_settings_get'));
      if (!disposed) update({ ...settings, loaded: true });
    } catch (reason) {
      update({ error: `Could not load progress notification settings: ${reason instanceof Error ? reason.message : String(reason)}`, retry: 'load' });
    } finally { update({ busy: '' }); }
  }
  async function saveSettings(value: { delivery: string; position: string }) {
    if (!native || disposed || !state.loaded || state.busy) return;
    const settings = progressNotificationSettings(value);
    update({ ...settings, busy: 'save', error: '', retry: '', message: '' });
    try {
      const saved = progressNotificationSettings(await invoke('profile_progress_settings_save', { settings }));
      if (saved.position !== settings.position || saved.delivery !== settings.delivery) throw new Error('The selected notification settings were not saved.');
      if (!disposed) update({ ...saved, message: 'Notification settings saved.' });
    } catch (reason) {
      update({ error: `Could not save notification settings: ${reason instanceof Error ? reason.message : String(reason)}`, retry: 'save' });
    } finally { update({ busy: '' }); }
  }
  return {
    get state() { return { ...state }; },
    load,
    save(position: string) { return saveSettings({ delivery: state.delivery, position }); },
    setDelivery(delivery: string) { return saveSettings({ delivery, position: state.position }); },
    retry() { return state.retry === 'save' ? saveSettings(state) : load(); },
    dispose() { disposed = true; },
  };
}
