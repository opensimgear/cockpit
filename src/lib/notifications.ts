import { writable } from 'svelte/store';

export type Notification = { id: number; message: string; kind: 'info' | 'warning' | 'error' };
export const notifications = writable<Notification[]>([]);
let nextId = 0;
const timers = new Map<number, ReturnType<typeof setTimeout>>();
export function dismissNotification(id: number) {
  clearTimeout(timers.get(id));
  timers.delete(id);
  notifications.update(items => items.filter(item => item.id !== id));
}
export function notify(message: string, kind: Notification['kind'] = 'info') {
  if (!message.trim()) return;
  let id = 0;
  notifications.update(items => {
    const existing = items.find(item => item.message === message && item.kind === kind);
    if (existing) { id = existing.id; return items; }
    id = ++nextId;
    const remaining = items.slice(-4);
    for (const item of items.slice(0, -4)) { clearTimeout(timers.get(item.id)); timers.delete(item.id); }
    return [...remaining, { id, message, kind }];
  });
  clearTimeout(timers.get(id));
  timers.set(id, setTimeout(() => dismissNotification(id), kind !== 'info' ? 12000 : 6000));
}
