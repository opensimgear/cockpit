import assert from 'node:assert/strict';
import {copyFile, readdir, realpath, rename, rm} from 'node:fs/promises';
import {isAbsolute, join, relative, resolve} from 'node:path';
import test from 'node:test';
import {nativePage, until} from '../../scripts/reactive/cdp.mjs';

const directory = process.env.OSG_TEST_OPTIONAL_DRIVER_DIRECTORY;

test('Native driver setup keeps its pending and terminal state across Devices settings reopen', {
  skip: !directory || process.env.OSG_DRIVER_SETUP_REOPEN !== '1' || process.platform !== 'win32',
  timeout: 60_000,
}, async () => {
  const root = await realpath(directory);
  const within = relative(await realpath(resolve('.research')), root);
  assert.ok(within && !within.startsWith('..') && !isAbsolute(within), 'Use a private driver directory in .research');
  assert.deepEqual(await readdir(root), [], 'Use an empty driver directory');

  const vjoyFile = join(root, 'vjoy.sys');
  const staging = join(root, 'incoming.fixture');
  const page = await nativePage();
  const pending = [];
  let manifestCalls = 0;
  let traceError;
  const stopTrace = page.listen(({method, params}) => {
    if (method !== 'Fetch.requestPaused') return;
    const command = new URL(params.request.url).pathname;
    if (params.request.method === 'POST' && command === '/optional_dependency_install') {
      pending.push(params);
      return;
    }
    if (params.request.method === 'POST' && command === '/optional_dependencies') manifestCalls++;
    void page.call('Fetch.continueRequest', {requestId:params.requestId}).catch(error => { traceError = error; });
  });
  const settle = async (call, outcome, message) => {
    await page.call('Fetch.fulfillRequest', {
      requestId:call.requestId, responseCode:200, responseHeaders:[
        {name:'Content-Type', value:'application/json'},
        {name:'Access-Control-Allow-Origin', value:'*'},
        {name:'Access-Control-Expose-Headers', value:'Tauri-Response'},
        {name:'Tauri-Response', value:outcome},
      ],
      body:Buffer.from(JSON.stringify(message)).toString('base64'),
    });
    call.completed = true;
  };
  const operation = async index => {
    const call = await until(() => {
      if (traceError) throw traceError;
      return pending[index];
    });
    assert.deepEqual(JSON.parse(call.request.postData), {id:'vjoy'});
    return call;
  };
  const vjoy = '[aria-label="Virtual controllers"]';
  const hiding = '[aria-label="Device hiding"]';
  const notifications = '[aria-label="Notifications"]';
  const output = '[aria-label="Controller and desktop output settings"]';
  const click = (selector, label) => until(() => page.evaluate(`(() => {
    const target = [...document.querySelectorAll(${JSON.stringify(selector)})]
      .find(item => ${label ? `item.textContent.trim() === ${JSON.stringify(label)}` : 'true'});
    if (!target || target.disabled || target.closest('[hidden]')) return false;
    target.click();
    return true;
  })()`));
  const setupState = card => page.evaluate(`(() => {
    const button = document.querySelector(${JSON.stringify(card + ' .dependency-setup button')});
    return button && {label:button.textContent.trim(), disabled:button.disabled};
  })()`);
  const waiting = () => until(() => setupState(vjoy), state => state?.label === 'Waiting for setup…' && state.disabled);
  const toast = message => until(() => page.evaluate(`[...document.querySelectorAll('${notifications} .toast p')]
    .filter(node => node.textContent === ${JSON.stringify(message)}).length`), count => count === 1);
  const reopen = () => click('#settings-tab-outputs');
  const leave = async () => {
    await click('#settings-tab-general');
    assert.equal(await page.evaluate(`!!document.querySelector('${output}')`), false);
  };
  try {
    await page.call('Fetch.enable', {
      patterns:[{urlPattern:'*/optional_dependency_install'}, {urlPattern:'*/optional_dependencies'}],
    });
    await click('.sidebar-bottom button', 'Settings');
    await reopen();
    await until(() => {
      if (traceError) throw traceError;
      return manifestCalls > 0;
    });
    await click(vjoy + ' .dependency-setup button', 'Set up');
    const first = await operation(0);
    await waiting();
    await leave();
    await reopen();
    assert.deepEqual(await until(() => setupState(vjoy)), {label:'Waiting for setup…', disabled:true},
      'The reopened Settings pane must display the existing operation');
    await until(() => setupState(hiding), state => state?.label === 'Set up' && state.disabled);
    assert.equal(pending.length, 1);

    await leave();
    const failure = 'Driver setup fixture cancelled.';
    await settle(first, 'error', failure);
    await toast(failure);
    await reopen();
    await until(() => page.evaluate(`document.querySelector('${vjoy} .dependency-setup [role="alert"]')?.textContent`),
      value => value === failure);
    await click(vjoy + ' .dependency-setup button', 'Set up');
    const second = await operation(1);

    await leave();
    await copyFile(process.execPath, staging);
    await rename(staging, vjoyFile);
    await reopen();
    await until(() => page.evaluate(`!!document.querySelector('${vjoy} .driver-version')`));
    await waiting();
    await leave();
    const success = 'Driver setup fixture completed.';
    await settle(second, 'ok', success);
    await toast(success);
    await reopen();
    await until(() => page.evaluate(`[...document.querySelectorAll('${vjoy} button')]
      .some(button => button.textContent.trim() === 'Manage' && !button.disabled)`));
    assert.equal(pending.length, 2, 'One invocation per explicit start, including the retry');
  } finally {
    try {
      for (const call of pending.filter(call => !call.completed)) {
        await settle(call, 'error', 'Driver setup fixture ended.');
      }
      await page.call('Fetch.disable');
    } finally {
      await page.close();
      await rm(vjoyFile, {force:true});
      await rm(staging, {force:true});
    }
  }
});
