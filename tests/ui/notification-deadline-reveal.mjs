import assert from 'node:assert/strict';
import {setTimeout as delay} from 'node:timers/promises';
import test from 'node:test';
import {browserPage, nativePage, until} from '../../scripts/reactive/cdp.mjs';

const browserUrl = process.env.OSG_NOTIFICATION_REVEAL_BROWSER_URL;
const transport = browserUrl ? 'browser-dev' : 'native';

test(`${transport} clears an elapsed save notification when the app becomes visible`, {
  skip:!browserUrl && process.env.OSG_NOTIFICATION_REVEAL_NATIVE !== '1', timeout:60_000,
}, async () => {
  assert.ok(process.env.OSG_SQLITE_DATABASE, 'Use a private Cockpit database');
  const page = browserUrl ? await browserPage(browserUrl) : await nativePage();
  const request = value => page.evaluate(`import('/src/lib/gateway.ts').then(({gatewayRequest}) =>
    gatewayRequest(${JSON.stringify(value)}))`);
  const click = selector => until(() => page.evaluate(`(() => {
    const node = document.querySelector(${JSON.stringify(selector)});
    if (!node?.checkVisibility() || node.disabled) return false;
    node.click();
    return true;
  })()`));
  const navigate = async title => {
    const selector = `nav[aria-label="Main navigation"] button[title="${title}"]`;
    await click(selector);
    await until(() => page.evaluate(`!!document.querySelector(${JSON.stringify(selector + '.current')})`));
  };
  const shown = () => page.evaluate(`[...document.querySelectorAll('[aria-label="Notifications"] .toast p')]
    .filter(node => node.textContent === 'Profile saved.').length`);
  let profile;
  try {
    await navigate('Profiles');
    const name = 'Notification deadline fixture';
    const savedName = name + ' saved';
    profile = (await request({command:'profile_create', name})).profiles.document;
    await click(`button[aria-label="Edit ${name}"]`);
    await click('button[aria-label="Edit profile name"]');
    await page.evaluate(`(() => {
      const input = document.querySelector('.profile-name-input');
      input.value = ${JSON.stringify(savedName)};
      input.dispatchEvent(new Event('input', {bubbles:true}));
      input.dispatchEvent(new KeyboardEvent('keydown', {key:'Enter', bubbles:true}));
    })()`);
    await page.evaluate(`(() => {
      const original = window.setTimeout;
      const probe = window.__notificationDeadline = {original, delayed:0};
      window.setTimeout = function(callback, timeout, ...args) {
        // Emulate a throttled renderer only for this save toast's timer.
        if (timeout === 3000 && String(callback).includes('dismissNotification')) {
          probe.delayed++;
          probe.scheduledAt = Date.now();
          const handle = original.call(window, callback, 60_000, ...args);
          probe.handle = handle;
          return handle;
        }
        return original.call(window, callback, timeout, ...args);
      };
      Object.defineProperty(document, 'hidden', {configurable:true, value:true});
      document.dispatchEvent(new Event('visibilitychange'));
    })()`);
    await until(() => page.evaluate(`(() => {
      const button = [...document.querySelectorAll('.editor-save-actions button')]
        .find(node => node.textContent.trim() === 'Save' && !node.disabled);
      if (!button) return false;
      button.click();
      return true;
    })()`));
    await until(shown, count => count === 1);
    assert.equal(await page.evaluate('window.__notificationDeadline.delayed'), 1);
    await navigate('Overview');
    assert.equal(await shown(), 1, 'Navigation keeps a current global notice');
    await delay(3100);
    assert.equal(await shown(), 1, 'The delayed timeout did not remove the notice');
    assert.equal(await page.evaluate('Date.now() - window.__notificationDeadline.scheduledAt >= 3000'), true);
    await page.evaluate(`(() => {
      delete document.hidden;
      document.dispatchEvent(new Event('visibilitychange'));
    })()`);
    await until(shown, count => count === 0, 1500);
    await navigate('Profiles');
    assert.equal(await shown(), 0, 'Reopening the editor does not replay the expired save');
    const current = (await request({command:'profile_get', id:profile.id})).profiles.document;
    assert.equal(current.name, savedName);
    assert.equal(current.revision, profile.revision + 1);
    console.log(JSON.stringify({transport, realSave:true, retainedAcrossNavigation:true,
      elapsedWhileHidden:true, expiredOnReveal:true, replayed:false}));
  } finally {
    try {
      await page.evaluate(`(() => {
        const probe = window.__notificationDeadline;
        if (probe) {
          window.setTimeout = probe.original;
          clearTimeout(probe.handle);
          delete window.__notificationDeadline;
          delete document.hidden;
          document.dispatchEvent(new Event('visibilitychange'));
        }
      })()`);
      if (profile) {
        const current = (await request({command:'profile_get', id:profile.id})).profiles.document;
        await request({command:'profile_delete', id:profile.id, expectedRevision:current.revision});
      }
    } finally { await page.close(); }
  }
});
