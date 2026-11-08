import assert from 'node:assert/strict';
import {execFile} from 'node:child_process';
import {copyFile, readdir, realpath, rename, rm, stat, truncate, utimes, writeFile} from 'node:fs/promises';
import {isAbsolute, join, relative, resolve} from 'node:path';
import {setTimeout as delay} from 'node:timers/promises';
import test from 'node:test';
import {promisify} from 'node:util';
import {browserPage, nativePage, until} from '../../scripts/reactive/cdp.mjs';

const browserUrl = process.env.OSG_DRIVER_OBSERVATION_BROWSER_URL;
const directory = process.env.OSG_TEST_OPTIONAL_DRIVER_DIRECTORY;
const gateway = browserUrl ? '/api/gateway/' : '/gateway_request';
const powershell = join(process.env.SystemRoot ?? 'C:/Windows', 'System32/WindowsPowerShell/v1.0/powershell.exe');

async function fileVersion(path) {
  const {stdout} = await promisify(execFile)(powershell, ['-NoProfile', '-NonInteractive', '-Command',
    '(Get-Item -LiteralPath $env:OSG_DRIVER_FIXTURE_PE).VersionInfo.FileVersion'], {
    env:{...process.env, OSG_DRIVER_FIXTURE_PE:path}, windowsHide:true,
  });
  return stdout.trim();
}

test('External driver file changes update Devices settings and its reopened snapshot', {
  skip: !directory || process.platform !== 'win32', timeout: 60_000,
}, async () => {
  const root = await realpath(directory);
  const within = relative(await realpath(resolve('.research')), root);
  assert.ok(within && !within.startsWith('..') && !isAbsolute(within), 'Use a private driver directory in .research');
  assert.deepEqual(await readdir(root), [], 'Use an empty driver directory');
  const vjoy = join(root, 'vjoy.sys'), hide = join(root, 'HidHide.sys');
  const staging = join(root, 'incoming.fixture'), unrelated = join(root, 'other.sys');
  const sources = [process.execPath, powershell];
  const versions = [];
  const length = Math.max(...(await Promise.all(sources.map(stat))).map(file => file.size));
  const stamp = new Date('2025-01-01T00:00:00Z');
  const install = async (source, destination) => {
    await copyFile(source, staging);
    await truncate(staging, length);
    await utimes(staging, stamp, stamp);
    await rename(staging, destination);
  };
  const page = browserUrl ? await browserPage(browserUrl) : await nativePage();
  const reads = [];
  const stop = page.listen(({method, params}) => {
    if (method === 'Network.loadingFinished') {
      const read = reads.find(row => row.id === params.requestId);
      if (read) void page.call('Network.getResponseBody', {requestId:read.id}).then(({body,base64Encoded}) => {
        const value = JSON.parse(base64Encoded ? Buffer.from(body, 'base64').toString('utf8') : body);
        const inventory = value.state?.systemInventory;
        read.result = {observedAt:inventory?.observedAt, nextOffset:inventory?.nextOffset,
          drivers:inventory?.drivers, slots:inventory?.virtualDevices?.map(slot => [slot.id,slot.status]), diagnostics:inventory?.diagnostics};
      }).catch(() => {});
    }
    if (method !== 'Network.requestWillBeSent' || !params.request?.postData
        || new URL(params.request.url).pathname !== gateway) return;
    const body = JSON.parse(params.request.postData);
    const request = body.request ?? body;
    if (request.command === 'system_inventory') reads.push({id:params.requestId, request});
  });
  const click = (selector, label) => until(() => page.evaluate(`(() => {
    const button = [...document.querySelectorAll(${JSON.stringify(selector)})]
      .find(item => ${label ? `(item.title || item.textContent.trim()) === ${JSON.stringify(label)}` : 'true'});
    if (!button || button.disabled || button.closest('[hidden]')) return false;
    button.click();
    return true;
  })()`));
  const settings = '[aria-label="Controller and desktop output settings"]';
  const rendered = (card, part, text) => until(() => page.evaluate(
    `document.querySelector(${JSON.stringify(`${settings} [aria-label="${card}"] ${part}`)})?.textContent.trim()`),
    value => value === text);
  const version = (card, value) => rendered(card, '.driver-version', `· ${value}`);
  const absent = card => rendered(card, '.setup-status', 'Not installed');
  const checkReads = async (before, expected) => {
    await delay(350);
    const requests = reads.slice(before);
    assert.equal(requests.length, expected, `Unexpected general inventory reads: ${JSON.stringify(requests)}`);
    assert.ok(requests.every(({request}) => !request.refresh), 'The view must never request a forced refresh');
  };
  try {
    await page.call('Network.enable', {maxPostDataSize:65536});
    await click('.sidebar-bottom button', 'Settings');
    await click('#settings-tab-outputs');
    await Promise.all([absent('Virtual controllers'), absent('Device hiding')]);
    if (browserUrl) {
      assert.equal(await page.evaluate(`document.querySelectorAll('${settings} .dependency-setup').length`), 0,
        'The browser observes driver state without offering native setup');
    }
    await delay(700);
    let before = reads.length;
    await writeFile(unrelated, 'other driver file');
    await delay(500);
    assert.equal(reads.length, before, 'An unrelated filename must not publish inventory');

    await install(sources[0], vjoy);
    versions.push(await fileVersion(vjoy));
    assert.ok(versions[0]);
    await version('Virtual controllers', versions[0]);
    await checkReads(before, 0);
    const old = await stat(vjoy);
    before = reads.length;
    await install(sources[1], vjoy);
    const replacement = await stat(vjoy);
    assert.equal(replacement.size, old.size);
    assert.equal(replacement.mtimeMs, old.mtimeMs);
    versions.push(await fileVersion(vjoy));
    assert.ok(versions[1] && versions[1] !== versions[0], 'Use two different PE file versions');
    await version('Virtual controllers', versions[1]);
    await checkReads(before, 0);

    await click('#settings-tab-general');
    await until(() => page.evaluate(`!document.querySelector(${JSON.stringify(settings)})`));
    before = reads.length;
    await install(sources[0], hide);
    await delay(1400);
    assert.equal(reads.length, before, 'Hidden Settings must not read the driver change');
    await click('#settings-tab-outputs');
    await version('Device hiding', versions[0]);
    await checkReads(before, 1);

    before = reads.length;
    await rm(vjoy);
    await absent('Virtual controllers');
    await checkReads(before, 0);
    console.log(JSON.stringify({transport:browserUrl ? 'browser-dev' : 'native', versions, sameSizeAndTime:true,
      unrelatedReads:0, hiddenReads:0, generalInventoryReadsPerDriverChange:0, reopenReads:1, removal:true}));
  } catch (error) {
    const displayed = await page.evaluate(`document.querySelector(${JSON.stringify(settings)})?.innerText`);
    console.error(JSON.stringify({displayed, expectedVersions:versions, installedVersion:await fileVersion(vjoy).catch(() => null)}));
    throw error;
  } finally {
    stop();
    try { await page.close(); }
    finally { for (const path of [vjoy, hide, staging, unrelated]) await rm(path, {force:true}); }
  }
});
