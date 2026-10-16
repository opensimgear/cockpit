import { test } from 'node:test';
import assert from 'node:assert/strict';
import { get } from 'svelte/store';
import { notify, notifications, dismissNotification } from './notifications.ts';

test('notifications deduplicate, dismiss and expire with a longer error timeout', t => {
  t.mock.timers.enable({ apis: ['setTimeout'] });
  notify('Saved');
  notify('Saved');
  notify('Connection failed', 'error');
  assert.equal(get(notifications).length, 2);
  t.mock.timers.tick(6000);
  assert.deepEqual(get(notifications).map(n => n.message), ['Connection failed']);
  t.mock.timers.tick(6000);
  assert.equal(get(notifications).length, 0);
  notify('Dismiss me');
  dismissNotification(get(notifications)[0].id);
  assert.equal(get(notifications).length, 0);
  t.mock.timers.reset();
});
test('notification stack is bounded and ignores empty messages', t => {
  t.mock.timers.enable({ apis: ['setTimeout'] });
  notify('  ');
  assert.equal(get(notifications).length, 0);
  for (let i = 0; i < 8; i++) notify('Message ' + i);
  assert.deepEqual(get(notifications).map(n => n.message), ['Message 3', 'Message 4', 'Message 5', 'Message 6', 'Message 7']);
  for (const n of get(notifications)) dismissNotification(n.id);
  t.mock.timers.tick(12000);
  assert.equal(get(notifications).length, 0);
  t.mock.timers.reset();
});
