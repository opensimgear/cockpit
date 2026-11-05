// Running Cockpit lifetime check. Observe native IPC without replacing it.
import assert from 'node:assert/strict';
import test from 'node:test';
import {nativePage, until} from '../../scripts/reactive/cdp.mjs';

test('main view navigation retires observation readers without duplicate baselines', {timeout:60000}, async () => {
  const page = await nativePage();
  const pending = new Map();
  const active = new Map();
  const baselines = new Map();
  const errors = [];
  const retired = new Set();
  const navigate = async view => {
    const label = JSON.stringify(view);
    await page.evaluate(`Array.from(document.querySelectorAll('nav[aria-label="Main navigation"] button'))
      .find(button => button.title === ${label}).click()`);
    await until(() => page.evaluate(`document.querySelector('[aria-label="Breadcrumb"] [aria-current="page"]')?.textContent.trim() === ${label}`));
  };
  const stopTrace = page.listen(({method, params}) => {
    if (method === 'Network.requestWillBeSent' && params.request.postData) {
      const operation = new URL(params.request.url).pathname.split('/').at(-1);
      if (!['observation_subscribe','observation_unsubscribe','observation_acknowledge'].includes(operation)) return;
      pending.set(params.requestId, {operation, body:JSON.parse(params.request.postData)});
    } else if (method === 'Network.responseReceived') {
      const entry = pending.get(params.requestId);
      if (entry) entry.accepted = params.response.status === 200;
    } else if (method === 'Network.loadingFinished' || method === 'Network.loadingFailed') {
      const entry = pending.get(params.requestId);
      if (!entry) return;
      if (!entry.accepted || method === 'Network.loadingFailed') {
        pending.delete(params.requestId);
        errors.push(entry.operation);
      } else if (entry.operation === 'observation_subscribe') {
        void page.call('Network.getResponseBody', {requestId:params.requestId}).then(({body}) => {
          const id = JSON.parse(body);
          if (!retired.has(id)) active.set(id, entry.body.topic);
        }).catch(error => errors.push(error.message)).finally(() => pending.delete(params.requestId));
      } else {
        pending.delete(params.requestId);
        const {subscriptionId, delivery} = entry.body;
        if (entry.operation === 'observation_unsubscribe') {
          retired.add(subscriptionId);
          active.delete(subscriptionId);
        } else if (delivery === '1') {
          baselines.set(subscriptionId, (baselines.get(subscriptionId) ?? 0) + 1);
        }
      }
    }
  });
  const settled = async expected => {
    await until(async () => {
      assert.deepEqual(errors, []);
      return pending.size === 0 && [...active.keys()].every(id => baselines.get(id) === 1)
        && (!expected || [...expected].every(topic => [...active.values()].includes(topic)));
    });
    return [...active.values()].sort();
  };
  try {
    await navigate('Overview');
    const views = [
      ['Devices', []],
      ['Profiles', []],
      ['Games', ['games.jobs']],
      ['Companions', ['companion.listener','companion.presence']],
      ['Debug', ['games.telemetry']],
      ['Activity', ['activity.log']],
    ];
    // Some models are shared with the navigation shell and intentionally
    // survive view changes. Settle their first use before tracing view readers.
    for (const [view] of views) await navigate(view);
    await navigate('Overview');
    await page.call('Network.enable', {maxPostDataSize:65536});
    for (let cycle = 0; cycle < 2; cycle++) {
      for (const [view, required] of views) {
        await navigate(view);
        await new Promise(resolve => setTimeout(resolve, 200));
        const topics = await settled(required);
        console.log(JSON.stringify({view,cycle,topics}));
        assert.deepEqual(topics, [...required].sort(), view + ' reader set');
        await navigate('Overview');
        await until(async () => pending.size === 0 && active.size === 0);
      }
    }
    assert.deepEqual(errors, []);
    assert.ok([...baselines.values()].every(count => count === 1));
  } finally {
    try { await navigate('Overview'); }
    finally { stopTrace(); page.close(); }
  }
});
