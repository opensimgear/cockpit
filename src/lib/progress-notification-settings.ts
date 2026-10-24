export const PROGRESS_NOTIFICATION_POSITIONS = [
  ['bottom-right', 'Bottom right'],
  ['bottom-left', 'Bottom left'],
  ['top-right', 'Top right'],
  ['top-left', 'Top left'],
] as const;

export type ProgressNotificationPosition = typeof PROGRESS_NOTIFICATION_POSITIONS[number][0];
export type ProgressNotificationDelivery = 'system' | 'window';
export type ProgressNotificationSettings = { delivery: ProgressNotificationDelivery; position: ProgressNotificationPosition };
export function progressNotificationSettings(value: unknown): ProgressNotificationSettings {
  const settings = value && typeof value === 'object' ? value as Record<string, unknown> : {};
  if (!PROGRESS_NOTIFICATION_POSITIONS.some(([id]) => id === settings.position) || !['system', 'window'].includes(String(settings.delivery))) {
    throw new Error('The notification settings are unavailable.');
  }
  return { delivery: settings.delivery as ProgressNotificationDelivery, position: settings.position as ProgressNotificationPosition };
}
