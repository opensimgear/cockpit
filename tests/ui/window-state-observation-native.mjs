import assert from 'node:assert/strict';
import { execFile } from 'node:child_process';
import { realpath } from 'node:fs/promises';
import { resolve } from 'node:path';
import { DatabaseSync } from 'node:sqlite';
import test from 'node:test';
import { promisify } from 'node:util';
import { nativePage, until } from '../../scripts/reactive/cdp.mjs';

const topic = 'settings.window-state';
const notice = 'Window geometry could not be saved or restored:';

test('General observes a real window save failure, reader retirement, and recovery', {
  skip: process.platform !== 'win32' || process.env.OSG_WINDOW_STATE_NATIVE !== '1',
  timeout: 45_000,
}, async () => {
  const page = await nativePage();
  const invoke = (command, args = {}) => page.evaluate(`window.__TAURI_INTERNALS__.invoke(${JSON.stringify(command)}, ${JSON.stringify(args)})`);
  const settings = `Array.from(document.querySelectorAll('.sidebar-bottom button')).find(button => button.textContent.trim() === 'Settings')`;
  const tab = id => page.evaluate(`document.querySelector('#settings-tab-${id}').click()`);
  const windowError = () => page.evaluate(`document.querySelector('#settings-panel-general:not([hidden]) [aria-label="Desktop behavior"]')?.textContent.includes(${JSON.stringify(notice)})`);
  const opening = new Set(), tokens = [], retired = new Set(), acknowledged = new Set(), errors = [];
  let reads = 0, lock;
  const stop = page.listen(({method, params}) => {
    if (method === 'Network.loadingFinished' && opening.delete(params.requestId)) {
      void page.call('Network.getResponseBody', {requestId:params.requestId})
        .then(({body}) => tokens.push(JSON.parse(body))).catch(error => errors.push(error));
    }
    if (method !== 'Network.requestWillBeSent' || params.request.method !== 'POST') return;
    const path = new URL(params.request.url).pathname;
    if (path === '/desktop_preferences') reads++;
    if (path === '/observation_subscribe' && JSON.parse(params.request.postData).topic === topic) opening.add(params.requestId);
    if (path === '/observation_unsubscribe') retired.add(JSON.parse(params.request.postData).subscriptionId);
    if (path === '/observation_acknowledge') {
      const {subscriptionId, delivery} = JSON.parse(params.request.postData);
      if (delivery === '1') acknowledged.add(subscriptionId);
    }
  });
  try {
    await until(() => page.evaluate(`!!(${settings})`));
    const status = await invoke('persistence_status');
    const fixtureRoot = await realpath(process.env.OSG_WINDOW_STATE_ROOT);
    const database = await realpath(status.path);
    assert.equal(database.toLowerCase(), resolve(fixtureRoot, 'cockpit.sqlite').toLowerCase());
    assert.match(fixtureRoot, /[\\/]window-state-qualification-\d+$/i);
    assert.equal(status.processId, Number(process.env.OSG_WINDOW_STATE_PID));
    const windowAction = async (action, options = []) => {
      const args = ['-NoProfile', '-NonInteractive', '-ExecutionPolicy', 'Bypass', '-File',
        resolve('tests/ui/ui-window-native.ps1'), '-CockpitPid', String(status.processId),
        '-Executable', process.env.OSG_WINDOW_STATE_BINARY, '-Action', action, ...options];
      const {stdout} = await promisify(execFile)('powershell.exe', args, {windowsHide:true,timeout:10_000});
      return JSON.parse(stdout.trim());
    };
    const initialWindow = await windowAction('inspect');
    await page.call('Network.enable', {maxPostDataSize:65536});
    await page.evaluate(`(${settings}).click()`);
    await until(() => tokens.length === 1 && acknowledged.has(tokens[0]));
    assert.equal(await windowError(), false);

    lock = new DatabaseSync(database);
    lock.exec('BEGIN IMMEDIATE');
    const width = initialWindow.width === 1300 ? 1200 : 1300;
    const resized = await windowAction('resize', ['-Width', String(width), '-Background', '-WindowHandle', initialWindow.handle]);
    assert.equal(resized.width, width);
    await until(windowError);
    assert.equal(reads, 1, 'The visible error must arrive without reading desktop preferences again');

    await tab('styling');
    await until(() => retired.has(tokens[0]));
    assert.equal(await windowError(), undefined);
    await tab('general');
    await until(() => tokens.length === 2 && acknowledged.has(tokens[1]));
    assert.notEqual(tokens[0], tokens[1]);
    await until(windowError);
    assert.equal(reads, 2, 'Visible reopen must read desktop preferences only once');

    lock.exec('ROLLBACK');
    lock.close();
    lock = undefined;
    await until(windowError, value => value === false);
    assert.equal(reads, 2, 'Successful retry must clear the visible error without another read');
    const persisted = await invoke('gateway_request', {request:{command:'ui_preferences_get', key:'windows', clientId:'window-state-fixture'}});
    assert.equal(persisted.ok, true, persisted.error);
    const geometry = persisted.state.uiPreferences.value;
    assert.deepEqual([geometry.x, geometry.y], [resized.x, resized.y]);
    assert.deepEqual(errors, []);
    console.log(JSON.stringify({visibleFailure:true,readerRetired:true,reopenBaseline:true,retryClearedError:true,statusReads:reads}));
  } finally {
    try { lock?.close(); }
    finally { stop(); await page.close(); }
  }
});
