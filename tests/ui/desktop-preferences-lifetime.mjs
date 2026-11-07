import assert from 'node:assert/strict';
import test from 'node:test';
import { browserPage, nativePage, until } from '../../scripts/reactive/cdp.mjs';

const browserUrl = process.env.OSG_DESKTOP_PREFERENCES_BROWSER_URL;
const transport = browserUrl ? 'browser-dev' : 'native';

test(`${transport} General settings reconcile saved desktop preferences on reopen`, {
  skip: !browserUrl && process.env.OSG_DESKTOP_PREFERENCES_NATIVE !== '1',
  timeout: 60_000,
}, async () => {
  const page = browserUrl ? await browserPage(browserUrl) : await nativePage();
  const request = value => page.evaluate(`import('/src/lib/gateway.ts')
    .then(({gatewayRequest}) => gatewayRequest(${JSON.stringify(value)}))`);
  const read = async key => (await request({command:'ui_preferences_get', key})).uiPreferences;
  const write = (key, expectedRevision, value) => request({command:'ui_preferences_save', key, expectedRevision, value});
  const tab = id => page.evaluate(`document.querySelector('#settings-tab-${id}').click()`);
  const visibility = hidden => page.evaluate(`(() => {
    if (${hidden}) Object.defineProperty(document, 'hidden', {configurable:true, value:true});
    else delete document.hidden;
    document.dispatchEvent(new Event('visibilitychange'));
  })()`);
  const desktopPane = () => page.evaluate(`!!document.querySelector('#settings-panel-general:not([hidden]) [aria-label="Desktop behavior"]')`);
  const settingsButton = `Array.from(document.querySelectorAll('.sidebar-bottom button'))
    .find(button => button.textContent.trim() === 'Settings')`;
  const trayValue = () => page.evaluate(`document.querySelector('[aria-label="Close to tray"]')?.getAttribute('aria-checked')`);
  const progressValue = () => page.evaluate(`document.querySelector('[aria-label="Progress notification position"]')?.value`);
  let reads = 0;
  let held;
  let completed = false;
  const stopTrace = page.listen(({method, params}) => {
    if (method === 'Network.loadingFinished' && params.requestId === held?.networkId) completed = true;
    if (!params.request || new URL(params.request.url).pathname !== '/desktop_preferences') return;
    if (method === 'Network.requestWillBeSent' && params.request.method === 'POST') reads++;
    if (method === 'Fetch.requestPaused') {
      if (params.request.method === 'POST') held = params;
      else void page.call('Fetch.continueRequest', {requestId:params.requestId});
    }
  });
  const restore = new Map();
  async function commit(key, value) {
    const current = await read(key);
    if (!restore.has(key)) restore.set(key, current.value);
    return write(key, current.revision, value);
  }
  try {
    await until(() => page.evaluate(`!!(${settingsButton})`));
    if (!browserUrl) {
      await page.call('Network.enable');
      await page.call('Fetch.enable', {patterns:[{urlPattern:'*/desktop_preferences'}]});
    }
    await page.evaluate(`(${settingsButton}).click()`);
    await until(desktopPane);
    const progress = (await read('progress')).value;
    const progressPosition = progress.position === 'top-left' ? 'bottom-right' : 'top-left';
    if (browserUrl) {
      assert.equal(await page.evaluate(`document.querySelector('[aria-label="Progress notification position"]').disabled`), true);
      assert.match(await page.evaluate(`document.querySelector('[aria-label="Desktop behavior"]').textContent`), /available in the Cockpit desktop app/);
      await tab('styling');
      assert.equal(await desktopPane(), false);
      await commit('progress', {...progress, position:progressPosition});
      await tab('general');
      await until(progressValue, position => position === progressPosition);
      assert.equal(await page.evaluate(`document.querySelector('[aria-label="Progress notification position"]').disabled`), true);
      return;
    }

    const tray = (await read('tray')).value;
    await until(() => held);
    await visibility(true);
    await page.call('Fetch.continueRequest', {requestId:held.requestId});
    await until(() => completed);
    await page.call('Fetch.disable');
    await page.evaluate('new Promise(resolve => requestAnimationFrame(resolve))');
    assert.equal(await trayValue(), undefined, 'A retired read must not restore the hidden snapshot');
    await tab('styling');
    assert.equal(await desktopPane(), false, 'Other tabs must hide desktop status');
    await commit('tray', {...tray, closeToTray:!tray.closeToTray});
    await commit('progress', {...progress, position:progressPosition});
    await tab('general');
    await until(desktopPane);
    assert.equal(reads, 1, 'Hidden General must make no desktop status reads');
    assert.equal(await trayValue(), undefined, 'Hidden General must withdraw the editable desktop snapshot');

    await visibility(false);
    await until(trayValue, value => value === String(!tray.closeToTray));
    await until(progressValue, position => position === progressPosition);
    assert.equal(reads, 2, 'Visible reopen must request desktop status exactly once');
    assert.equal(await trayValue(), String(!tray.closeToTray));

    await commit('tray', tray);
    await commit('progress', progress);
    await until(trayValue, value => value === String(tray.closeToTray));
    await until(progressValue, position => position === progress.position);
    assert.equal(reads, 2, 'Visible preference commits must not reread desktop status');
    await tab('styling');
    assert.equal(await trayValue(), undefined, 'Leaving General must retire its desktop snapshot');
    assert.equal(reads, 2);
    await tab('general');
    await until(trayValue, value => value === String(tray.closeToTray));
    assert.equal(reads, 3, 'Returning to General must read the current desktop status once');
    console.log(JSON.stringify({transport,desktopReads:3,hiddenReads:0,visibleCommitReads:0}));
  } finally {
    try {
      if (!browserUrl) await page.call('Fetch.disable');
      await visibility(false);
      for (const [key, value] of restore) {
        const current = await read(key);
        if (JSON.stringify(current.value) !== JSON.stringify(value)) await write(key, current.revision, value);
      }
    } finally { stopTrace(); await page.close(); }
  }
});
