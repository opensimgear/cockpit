import assert from 'node:assert/strict';
import test from 'node:test';
import { createProgressNotificationSettings, progressNotificationSettings, PROGRESS_NOTIFICATION_POSITIONS } from './progress-notification-settings.ts';
const settings = (position, delivery = 'system') => ({ delivery, position });

test('opening settings reads the native position without writing or replacing it with the default', async () => {
  const calls = [], updates = [];
  const client = createProgressNotificationSettings(async (...args) => { calls.push(args); return settings('top-left', 'window'); }, true, state => updates.push(state));
  assert.equal(client.state.position, 'bottom-right');
  assert.equal(client.state.delivery, 'system');
  assert.equal(client.state.loaded, false);
  assert.deepEqual(calls, []);
  await client.load();
  assert.deepEqual(calls, [['profile_progress_settings_get']]);
  assert.equal(client.state.position, 'top-left');
  assert.equal(client.state.delivery, 'window');
  assert.equal(client.state.loaded, true);
  assert.equal(client.state.busy, '');
  assert.equal(updates[0].busy, 'load');
});

test('every selectable corner saves its exact native setting and confirms success', async () => {
  const calls = [];
  const client = createProgressNotificationSettings(async (command, args) => {
    calls.push({ command, args });
    return args?.settings ?? settings('bottom-right');
  }, true, () => {});
  await client.load();
  for (const [position] of PROGRESS_NOTIFICATION_POSITIONS) {
    await client.save(position);
    assert.deepEqual(calls.at(-1), { command: 'profile_progress_settings_save', args: { settings: settings(position) } });
    assert.equal(client.state.position, position);
    assert.equal(client.state.error, '');
    assert.equal(client.state.message, 'Notification settings saved.');
  }
});

test('a failed read blocks writes and retries loading rather than silently persisting the default', async () => {
  const calls = [];
  let unavailable = true;
  const client = createProgressNotificationSettings(async command => {
    calls.push(command);
    if (unavailable) throw new Error('Settings file cannot be read.');
    return settings('bottom-left');
  }, true, () => {});
  await client.load();
  assert.match(client.state.error, /Could not load.*Settings file cannot be read/);
  assert.equal(client.state.retry, 'load');
  await client.save('top-right');
  assert.deepEqual(calls, ['profile_progress_settings_get']);
  unavailable = false;
  await client.retry();
  assert.deepEqual(calls, ['profile_progress_settings_get', 'profile_progress_settings_get']);
  assert.equal(client.state.position, 'bottom-left');
  assert.equal(client.state.loaded, true);
  assert.equal(client.state.error, '');
});

test('failed saves preserve the requested corner and retry that setting', async () => {
  const writes = [];
  let rejectSave = true;
  const client = createProgressNotificationSettings(async (command, args) => {
    if (command === 'profile_progress_settings_get') return settings('bottom-right');
    writes.push(args);
    if (rejectSave) throw new Error('Disk full.');
    return args.settings;
  }, true, () => {});
  await client.load();
  await client.save('top-right');
  assert.match(client.state.error, /Could not save.*Disk full/);
  assert.equal(client.state.position, 'top-right');
  assert.equal(client.state.retry, 'save');
  assert.equal(client.state.message, '');
  rejectSave = false;
  await client.retry();
  assert.deepEqual(writes, [{ settings: settings('top-right') }, { settings: settings('top-right') }]);
  assert.equal(client.state.error, '');
  assert.equal(client.state.retry, '');
});

test('unexpected native positions and unconfirmed saves surface errors instead of claiming success', async () => {
  for (const value of [null, {}, settings('center'), settings(2), settings('top-left', 'unknown'), { position: 'top-left' }]) assert.throws(() => progressNotificationSettings(value), /unavailable/);
  const client = createProgressNotificationSettings(async () => settings('bottom-right'), true, () => {});
  await client.load();
  await assert.rejects(client.save('center'), /unavailable/);
  await client.save('top-left');
  assert.match(client.state.error, /selected notification settings were not saved/);
  assert.equal(client.state.message, '');
  assert.equal(client.state.position, 'top-left');
});

test('delivery changes retain the selected corner and retry the requested delivery after a failed save', async () => {
  const calls = [];
  let fail = true;
  const client = createProgressNotificationSettings(async (command, args) => {
    calls.push({ command, args });
    if (command === 'profile_progress_settings_get') return settings('bottom-left');
    if (fail) throw new Error('Disk full.');
    return args.settings;
  }, true, () => {});
  await client.load();
  await client.setDelivery('window');
  assert.match(client.state.error, /Could not save.*Disk full/);
  assert.equal(client.state.position, 'bottom-left');
  assert.equal(client.state.delivery, 'window');
  assert.equal(client.state.retry, 'save');
  fail = false;
  await client.retry();
  assert.deepEqual(calls.slice(1), Array.from({ length: 2 }, () => ({ command: 'profile_progress_settings_save', args: { settings: settings('bottom-left', 'window') } })));
  assert.equal(client.state.message, 'Notification settings saved.');
  assert.equal(client.state.error, '');
  await client.setDelivery('system');
  assert.deepEqual(calls.at(-1), { command: 'profile_progress_settings_save', args: { settings: settings('bottom-left') } });
  assert.equal(client.state.delivery, 'system');
});

test('browser mode, pending operations and disposed views do not issue extra native requests', async () => {
  const calls = [];
  const browser = createProgressNotificationSettings(async command => { calls.push(command); }, false, () => {});
  await browser.load();
  await browser.save('top-left');
  await browser.setDelivery('window');
  await browser.retry();
  assert.deepEqual(calls, []);
  let finish;
  const updates = [];
  const client = createProgressNotificationSettings(command => { calls.push(command); return new Promise(resolve => { finish = resolve; }); }, true, state => updates.push(state));
  const loading = client.load();
  await client.load();
  await client.save('top-left');
  assert.deepEqual(calls, ['profile_progress_settings_get']);
  const beforeDispose = updates.length;
  client.dispose();
  finish(settings('top-left'));
  await loading;
  await client.retry();
  assert.equal(updates.length, beforeDispose);
  assert.deepEqual(calls, ['profile_progress_settings_get']);
});
