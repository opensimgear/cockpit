import { test } from 'node:test';
import assert from 'node:assert/strict';
import { get } from 'svelte/store';
import { notify, notifications, dismissNotification, pauseNotification, resumeNotification } from './notifications.ts';

function clock(t) {
  t.mock.timers.enable({ apis: ['setTimeout', 'Date'], now: 10000 });
  t.after(() => {
    for (const notification of get(notifications)) dismissNotification(notification.id);
    t.mock.timers.reset();
  });
  return ms => t.mock.timers.tick(ms);
}
const current = () => get(notifications)[0];

test('notifications expose countdown metadata and expire at six or twelve seconds by severity', t => {
  const tick = clock(t);
  notify('Saved');
  notify('Connection warning', 'warning');
  notify('Connection failed', 'error');
  assert.deepEqual(get(notifications).map(({ message, durationMs, remainingMs, paused, timerVersion }) => ({ message, durationMs, remainingMs, paused, timerVersion })), [
    { message: 'Saved', durationMs: 6000, remainingMs: 6000, paused: false, timerVersion: 1 },
    { message: 'Connection warning', durationMs: 12000, remainingMs: 12000, paused: false, timerVersion: 1 },
    { message: 'Connection failed', durationMs: 12000, remainingMs: 12000, paused: false, timerVersion: 1 },
  ]);
  tick(5999);
  assert.equal(get(notifications).length, 3);
  tick(1);
  assert.deepEqual(get(notifications).map(n => n.message), ['Connection warning', 'Connection failed']);
  tick(5999);
  assert.equal(get(notifications).length, 2);
  tick(1);
  assert.deepEqual(get(notifications), []);
});

test('duplicates reset their deadline and visual timer without adding or moving a notification', t => {
  const tick = clock(t);
  notify('Saved');
  const first = current();
  notify('Other');
  tick(4500);
  notify('Saved');
  assert.equal(get(notifications).length, 2);
  assert.deepEqual(current(), { ...first, timerVersion: 2 });
  assert.deepEqual(get(notifications).map(n => n.message), ['Saved', 'Other']);
  tick(1500);
  assert.deepEqual(get(notifications).map(n => n.message), ['Saved']);
  tick(4499);
  assert.equal(current().id, first.id);
  tick(1);
  assert.deepEqual(get(notifications), []);
});

test('hover pauses the precise remaining deadline and resumes that duration', t => {
  const tick = clock(t);
  notify('Saved');
  const { id } = current();
  tick(1750);
  pauseNotification(id);
  assert.deepEqual(current(), { id, message: 'Saved', kind: 'info', durationMs: 6000, remainingMs: 4250, paused: true, timerVersion: 2 });
  tick(60000);
  pauseNotification(id);
  assert.equal(current().remainingMs, 4250);
  assert.equal(current().timerVersion, 2);
  resumeNotification(id);
  assert.equal(current().paused, false);
  assert.equal(current().timerVersion, 3);
  tick(4249);
  assert.equal(current().id, id);
  tick(1);
  assert.deepEqual(get(notifications), []);
});

test('repeated hover cycles use the resumed deadline and inactive releases cannot extend it', t => {
  const tick = clock(t);
  notify('Saved');
  const { id } = current();
  tick(1000);
  pauseNotification(id);
  tick(10000);
  resumeNotification(id);
  tick(1500);
  pauseNotification(id);
  assert.equal(current().remainingMs, 3500);
  assert.equal(current().timerVersion, 4);
  tick(10000);
  resumeNotification(id);
  tick(1000);
  resumeNotification(id);
  resumeNotification(id, 'focus');
  assert.equal(current().timerVersion, 5);
  tick(2499);
  assert.equal(current().id, id);
  tick(1);
  assert.deepEqual(get(notifications), []);
});

test('hover and focus independently hold expiry until both reasons are released', t => {
  const tick = clock(t);
  notify('Warning', 'warning');
  const { id } = current();
  tick(2000);
  pauseNotification(id, 'hover');
  pauseNotification(id, 'focus');
  pauseNotification(id, 'focus');
  tick(20000);
  resumeNotification(id, 'hover');
  assert.equal(current().paused, true);
  assert.equal(current().remainingMs, 10000);
  assert.equal(current().timerVersion, 2);
  tick(20000);
  resumeNotification(id, 'focus');
  assert.equal(current().paused, false);
  assert.equal(current().timerVersion, 3);
  tick(9999);
  assert.equal(current().id, id);
  tick(1);
  assert.deepEqual(get(notifications), []);
});

test('a duplicate while paused resets the duration and keeps every interaction hold', t => {
  const tick = clock(t);
  notify('Failed', 'error');
  const { id } = current();
  tick(3000);
  pauseNotification(id, 'focus');
  pauseNotification(id, 'hover');
  tick(20000);
  notify('Failed', 'error');
  assert.deepEqual(current(), { id, message: 'Failed', kind: 'error', durationMs: 12000, remainingMs: 12000, paused: true, timerVersion: 3 });
  resumeNotification(id, 'focus');
  tick(20000);
  assert.equal(current().paused, true);
  assert.equal(current().timerVersion, 3);
  resumeNotification(id, 'hover');
  assert.equal(current().timerVersion, 4);
  tick(11999);
  assert.equal(current().id, id);
  tick(1);
  assert.deepEqual(get(notifications), []);
});

test('eviction, dismissal and expiry clear old holds without affecting reused messages or other timers', t => {
  const tick = clock(t);
  notify('Evicted');
  const evictedId = current().id;
  pauseNotification(evictedId, 'hover');
  pauseNotification(evictedId, 'focus');
  for (let i = 0; i < 5; i++) notify('Message ' + i);
  assert.deepEqual(get(notifications).map(n => n.message), ['Message 0', 'Message 1', 'Message 2', 'Message 3', 'Message 4']);
  resumeNotification(evictedId, 'hover');
  resumeNotification(evictedId, 'focus');
  notify('Evicted');
  const recreated = get(notifications).at(-1);
  assert.notEqual(recreated.id, evictedId);
  assert.equal(recreated.paused, false);
  pauseNotification(recreated.id, 'focus');
  dismissNotification(recreated.id);
  resumeNotification(recreated.id, 'focus');
  notify('Evicted');
  const fresh = get(notifications).at(-1);
  assert.equal(fresh.paused, false);
  tick(6000);
  assert.deepEqual(get(notifications), []);
  pauseNotification(fresh.id);
  resumeNotification(fresh.id);
  dismissNotification(evictedId);
  notify('Evicted');
  assert.equal(current().paused, false);
  assert.equal(current().timerVersion, 1);
  tick(6000);
  assert.deepEqual(get(notifications), []);
});

test('empty messages are ignored and countdowns do not emit per-frame store updates', t => {
  const tick = clock(t);
  let updates = 0;
  const unsubscribe = notifications.subscribe(() => updates++);
  t.after(unsubscribe);
  notify('  ');
  assert.deepEqual(get(notifications), []);
  notify('Saved');
  const afterNotify = updates;
  tick(1000);
  assert.equal(updates, afterNotify);
  pauseNotification(current().id);
  const afterPause = updates;
  pauseNotification(current().id);
  pauseNotification(current().id, 'focus');
  resumeNotification(current().id, 'focus');
  assert.equal(updates, afterPause);
  tick(10000);
  assert.equal(updates, afterPause);
  resumeNotification(current().id);
  const afterResume = updates;
  resumeNotification(current().id);
  assert.equal(updates, afterResume);
  tick(4999);
  assert.equal(updates, afterResume);
  tick(1);
  assert.equal(updates, afterResume + 1);
});
