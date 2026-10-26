import { writable } from 'svelte/store';

export type Notification = {
  id: number;
  message: string;
  kind: 'info' | 'warning' | 'error';
  durationMs: number;
  remainingMs: number;
  paused: boolean;
  timerVersion: number;
};
export type NotificationPauseReason = 'hover' | 'focus';
export const notifications = writable<Notification[]>([]);
let nextId = 0;
type Timer = { timeout?: ReturnType<typeof setTimeout>; deadline: number; reasons: Set<NotificationPauseReason> };
const timers = new Map<number, Timer>();

function clearTimer(timer: Timer) {
  clearTimeout(timer.timeout);
  delete timer.timeout;
}
function forgetTimer(id: number) {
  const timer = timers.get(id);
  if (timer) clearTimer(timer);
  timers.delete(id);
}
function schedule(id: number, timer: Timer, remainingMs: number) {
  timer.deadline = Date.now() + remainingMs;
  timer.timeout = setTimeout(() => dismissNotification(id), remainingMs);
}
export function dismissNotification(id: number) {
  forgetTimer(id);
  notifications.update(items => items.filter(item => item.id !== id));
}
export function notify(message: string, kind: Notification['kind'] = 'info') {
  if (!message.trim()) return;
  notifications.update(items => {
    const existing = items.find(item => item.message === message && item.kind === kind);
    const id = existing?.id ?? ++nextId;
    const timer = timers.get(id) ?? { deadline: 0, reasons: new Set<NotificationPauseReason>() };
    clearTimer(timer);
    timers.set(id, timer);
    const durationMs = 3000;
    const paused = timer.reasons.size > 0;
    const notification: Notification = { id, message, kind, durationMs, remainingMs: durationMs, paused, timerVersion: (existing?.timerVersion ?? 0) + 1 };
    if (!paused) schedule(id, timer, durationMs);
    if (existing) return items.map(item => item.id === id ? notification : item);
    const remaining = items.slice(-4);
    for (const item of items.slice(0, -4)) forgetTimer(item.id);
    return [...remaining, notification];
  });
}

export function pauseNotification(id: number, reason: NotificationPauseReason = 'hover') {
  const timer = timers.get(id);
  if (!timer || timer.reasons.has(reason)) return;
  timer.reasons.add(reason);
  if (timer.reasons.size > 1) return;
  notifications.update(items => {
    const notification = items.find(item => item.id === id);
    if (!notification) { forgetTimer(id); return items; }
    clearTimer(timer);
    const remainingMs = Math.max(0, Math.min(notification.durationMs, timer.deadline - Date.now()));
    return items.map(item => item.id === id ? { ...item, remainingMs, paused: true, timerVersion: item.timerVersion + 1 } : item);
  });
}

export function resumeNotification(id: number, reason: NotificationPauseReason = 'hover') {
  const timer = timers.get(id);
  if (!timer || !timer.reasons.delete(reason) || timer.reasons.size > 0) return;
  notifications.update(items => {
    const notification = items.find(item => item.id === id);
    if (!notification) { forgetTimer(id); return items; }
    schedule(id, timer, notification.remainingMs);
    return items.map(item => item.id === id ? { ...item, paused: false, timerVersion: item.timerVersion + 1 } : item);
  });
}
