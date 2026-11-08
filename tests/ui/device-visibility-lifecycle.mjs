// Exercises the controller detail card against the running native or browser-dev Cockpit.
import assert from 'node:assert/strict';
import test from 'node:test';
import {setTimeout as delay} from 'node:timers/promises';
import {browserPage, nativePage, until} from '../../scripts/reactive/cdp.mjs';

const browserUrl = process.env.OSG_DEVICE_VISIBILITY_LIFECYCLE_BROWSER_URL;
const privateDatabase = process.env.OSG_DEVICE_VISIBILITY_LIFECYCLE_PRIVATE_DATABASE === '1';
const transport = browserUrl ? 'browser-dev' : 'native';
const topics = ['controllers.visibility', 'system.inventory'];
const endpoints = new Map([
  ['/observation_subscribe', 'subscribe'], ['/observation_unsubscribe', 'unsubscribe'],
  ['/observation_acknowledge', 'ack'], ['/api/observe/subscribe', 'subscribe'],
  ['/api/observe/unsubscribe', 'unsubscribe'], ['/api/observe/ack', 'ack'],
]);
const openingId = ({connectionId, requestId}) => JSON.stringify([connectionId, requestId]);

test(transport + ' controller detail releases hidden visibility readers and refreshes on reveal', {
  skip: !browserUrl && process.env.OSG_DEVICE_VISIBILITY_LIFECYCLE_NATIVE !== '1',
  timeout: 70_000,
}, async () => {
  const page = browserUrl ? await browserPage(browserUrl) : await nativePage();
  const pending = new Map(), active = new Map(), baselines = new Map(), changes = new Map();
  const retired = new Set(), errors = [], statusReads = [];
  let sourceId, originalIntent, restoreIntent = false, openedDetail = false;
  const evaluate = source => page.evaluate(source);
  const request = body => evaluate("import('/src/lib/gateway.ts').then(({gatewayRequest}) => gatewayRequest(" +
    JSON.stringify(body) + '))');
  const button = (selector, label) => until(() => evaluate('(() => { const button = [...document.querySelectorAll(' +
    JSON.stringify(selector) + ')].find(value => !value.disabled && (value.title || value.textContent.trim()) === ' +
    JSON.stringify(label) + '); if (!button) return false; button.click(); return true; })()'));
  const navigate = label => button('nav[aria-label="Main navigation"] button', label);
  const setHidden = hidden => evaluate("(() => {Object.defineProperty(document, 'hidden', {configurable:true, value:" +
    hidden + "}); document.dispatchEvent(new Event('visibilitychange'));})()");
  const card = 'section[aria-label="Device visibility"]';
  const displayedMode = () => evaluate('document.querySelector(' + JSON.stringify(card + ' select') + ')?.value');
  const scopedReads = () => statusReads.filter(id => id === sourceId).length;
  const save = intent => request({command: 'controller_visibility_save', sourceId, intent});

  const stopTrace = page.listen(({method, params}) => {
    if (method === 'Network.requestWillBeSent' && params.request.postData) {
      const path = new URL(params.request.url).pathname;
      const operation = endpoints.get(path);
      if (path === '/gateway_request' || path === '/api/gateway/') {
        const body = JSON.parse(params.request.postData);
        const value = body.request ?? body;
        if (value.command === 'controller_visibility_status') statusReads.push(value.sourceId);
      }
      if (operation) pending.set(params.requestId, {operation, body: JSON.parse(params.request.postData)});
      return;
    }
    const entry = pending.get(params.requestId);
    if (!entry) return;
    if (method === 'Network.responseReceived') entry.status = params.response.status;
    if (method !== 'Network.loadingFinished' && method !== 'Network.loadingFailed') return;
    const {operation, body, status} = entry;
    if (operation === 'subscribe' && topics.includes(body.topic) && status === 200 && method === 'Network.loadingFinished') {
      void page.call('Network.getResponseBody', {requestId: params.requestId}).then(({body: response}) => {
        const reply = JSON.parse(response);
        const token = browserUrl ? reply.subscriptionId : reply;
        assert.equal(typeof token, 'string');
        const id = browserUrl ? openingId(body) : token;
        assert.ok(!active.has(id), 'One transport identity cannot own two readers');
        if (!retired.has(id)) active.set(id, {topic: body.topic, token});
      }).catch(error => errors.push(error.message)).finally(() => pending.delete(params.requestId));
      return;
    }
    pending.delete(params.requestId);
    if (status !== 200 || method !== 'Network.loadingFinished') {
      if (operation === 'subscribe' && topics.includes(body.topic) && !params.canceled) errors.push(body.topic + ' subscription failed');
      return;
    }
    if (operation === 'unsubscribe') {
      const id = browserUrl ? openingId(body) : body.subscriptionId;
      retired.add(id);
      active.delete(id);
    } else if (operation === 'ack' && body.delivery === '1') {
      baselines.set(body.subscriptionId, (baselines.get(body.subscriptionId) ?? 0) + 1);
    } else if (operation === 'ack') {
      changes.set(body.subscriptionId, (changes.get(body.subscriptionId) ?? 0) + 1);
    }
  });
  const committedChanges = readers => readers.reduce((total, [, row]) => total + (changes.get(row.token) ?? 0), 0);

  const settled = expected => until(() => {
    assert.deepEqual(errors, []);
    const rows = [...active].filter(([id]) => !expected.persistent.has(id));
    return rows.length === expected.count && rows.every(([, reader]) => baselines.get(reader.token) === 1) && rows;
  }).catch(cause => {
    const readers = [...active.values()].map(row => [row.topic, baselines.get(row.token) ?? 0]);
    throw new Error(`Expected ${expected.count} detail readers. Observed ${JSON.stringify(readers)}.`, {cause});
  });
  let persistent = new Set();
  try {
    await navigate('Overview');
    await page.call('Network.enable', {maxPostDataSize: 65_536});
    // SystemDevices stays mounted outside the Devices view. Trace its initial
    // registration so its reader can be distinguished from the nested card.
    await page.call('Page.reload');
    await until(async () => {
      try { return await evaluate('document.readyState === "complete" && !!document.querySelector("nav")'); }
      catch { return false; }
    });
    await setHidden(false);
    await navigate('Devices');
    const inventory = await settled({count: 1, persistent});
    assert.equal(inventory[0][1].topic, 'system.inventory');
    persistent = new Set(inventory.map(([id]) => id));
    await button('[aria-label="Devices presentation"] button', 'List');
    const tile = await until(() => evaluate("document.querySelector('.component-tile[data-component-id^=\"controller:\"]')?.dataset.componentId"));
    sourceId = tile.slice('controller:'.length);
    if (privateDatabase) {
      const initial = await request({command: 'controller_visibility_status', sourceId});
      originalIntent = initial.controllers.visibility.intent;
    }
    const beforeOpen = scopedReads();
    await evaluate('document.querySelector(' + JSON.stringify('[data-component-id="' + tile + '"]') + ').click()');
    openedDetail = true;
    const first = await settled({count: 2, persistent});
    assert.deepEqual(first.map(([, row]) => row.topic).sort(), topics);
    const initialMode = await until(displayedMode);
    await delay(650);
    const initialReads = scopedReads() - beforeOpen;
    assert.ok(initialReads >= 1 && initialReads <= 1 + committedChanges(first),
      'Visible reads require an initial baseline or a committed change');

    await setHidden(true);
    await until(() => first.every(([id]) => retired.has(id) && !active.has(id)), Boolean, 5_000).catch(cause => {
      throw new Error('Hidden controller detail retained its exact visibility or inventory reader', {cause});
    });
    assert.ok([...persistent].every(id => active.has(id)), 'The separate Devices inventory reader stays connected');

    let expectedMode = initialMode;
    const hiddenReads = scopedReads();
    if (privateDatabase) {
      expectedMode = originalIntent.mode === 'visible' ? 'hide-all' : 'visible';
      const reply = await save({mode: expectedMode});
      restoreIntent = true;
      assert.equal(reply.controllers.visibility.intent.mode, expectedMode);
    }
    await delay(400);
    assert.equal(scopedReads(), hiddenReads, 'A hidden preference commit must not read controller visibility');
    assert.equal(await displayedMode(), initialMode);

    await setHidden(false);
    const reopened = await settled({count: 2, persistent});
    assert.deepEqual(reopened.map(([, row]) => row.topic).sort(), topics);
    assert.ok(reopened.every(([id]) => !first.some(([old]) => old === id)));
    await until(() => displayedMode(), mode => mode === expectedMode);
    await delay(650);
    assert.equal(scopedReads(), hiddenReads + 1, 'Reveal reads the current status once');

    await button('.component-details .header-back', 'Back to Devices');
    openedDetail = false;
    await settled({count: 0, persistent});
    assert.ok(reopened.every(([id]) => retired.has(id)));
    assert.ok([...persistent].every(id => active.has(id)));
    if (restoreIntent) {
      await save(originalIntent);
      restoreIntent = false;
    }
    console.log(JSON.stringify({transport, sourceId, initialReads, initialChanges: committedChanges(first), hiddenReads: 0, revealReads: 1,
      ownedReadersRetiredOnHideAndExit: 2, backgroundPreferenceObserved: privateDatabase}));
  } finally {
    try {
      await setHidden(false);
      if (openedDetail) await navigate('Overview');
      if (restoreIntent) await save(originalIntent);
    } finally {
      await evaluate('delete document.hidden').catch(() => {});
      stopTrace();
      await page.close();
    }
  }
});
