// Running Cockpit lifetime check. Observe native IPC without replacing it.
import assert from 'node:assert/strict';
import test from 'node:test';
import {createRequire} from 'node:module';
import {dirname, resolve} from 'node:path';
import {nativePage, until} from '../../scripts/reactive/cdp.mjs';

const browserUrl = process.env.OSG_NAVIGATION_BROWSER_URL;
const transport = browserUrl ? 'browser development' : 'native';
const endpoints = new Map([
  ['/observation_subscribe','subscribe'],
  ['/observation_unsubscribe','unsubscribe'],
  ['/observation_acknowledge','ack'],
  ['/api/observe/subscribe','subscribe'],
  ['/api/observe/unsubscribe','unsubscribe'],
  ['/api/observe/ack','ack'],
]);

async function browserPage(url) {
  const require = createRequire(import.meta.url);
  const {chromium} = require(process.env.OSG_PLAYWRIGHT_MODULE ?? 'playwright');
  const browser = await chromium.launch({headless:true, channel:process.env.OSG_BROWSER_CHANNEL});
  try {
    const page = await browser.newPage();
    await page.goto(url);
    await page.getByRole('navigation', {name:'Main navigation'}).waitFor();
    const session = await page.context().newCDPSession(page);
    return {
      call: (method, args) => session.send(method, args),
      evaluate: expression => page.evaluate(expression),
      listen(listener) {
        const methods = ['Network.requestWillBeSent','Network.responseReceived','Network.loadingFinished','Network.loadingFailed'];
        const handlers = methods.map(method => [method, params => listener({method,params})]);
        for (const [method, handler] of handlers) session.on(method, handler);
        return () => handlers.forEach(([method, handler]) => session.off(method, handler));
      },
      close: () => browser.close(),
    };
  } catch (error) {
    await browser.close();
    throw error;
  }
}

test(`${transport} main view navigation retires observation readers without duplicate baselines`, {timeout:60000}, async () => {
  const page = browserUrl ? await browserPage(browserUrl) : await nativePage();
  const addedGames = [];
  const discardDraft = () => page.evaluate(`Array.from(document.querySelectorAll('dialog[open] button'))
    .find(button => button.textContent.trim() === 'Discard and continue')?.click()`);
  const request = value => page.evaluate(`import('/src/lib/gateway.ts').then(({gatewayRequest}) => gatewayRequest(${JSON.stringify(value)}))`);
  const pending = new Map();
  const active = new Map();
  const baselines = new Map();
  const errors = [];
  const retired = new Set();
  const navigate = async view => {
    const label = JSON.stringify(view);
    const selector = view === 'Settings' ? '.sidebar-bottom button' : 'nav[aria-label="Main navigation"] button';
    await page.evaluate(`Array.from(document.querySelectorAll(${JSON.stringify(selector)}))
      .find(button => (button.title || button.textContent.trim()) === ${label}).click()`);
    await until(() => page.evaluate(`document.querySelector('[aria-label="Breadcrumb"] [aria-current="page"]')?.textContent.trim() === ${label}`));
  };
  const stopTrace = page.listen(({method, params}) => {
    if (method === 'Network.requestWillBeSent' && params.request.postData) {
      const operation = endpoints.get(new URL(params.request.url).pathname);
      if (!operation) return;
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
      } else if (entry.operation === 'subscribe') {
        void page.call('Network.getResponseBody', {requestId:params.requestId}).then(({body}) => {
          const response = JSON.parse(body);
          const token = browserUrl ? response.subscriptionId : response;
          assert.equal(typeof token, 'string');
          // Bridge cancellation identifies the opening request. Baselines
          // still acknowledge the resulting native subscription token.
          const id = browserUrl ? entry.body.requestId : token;
          if (!retired.has(id)) active.set(id, {topic:entry.body.topic, token});
        }).catch(error => errors.push(error.message)).finally(() => pending.delete(params.requestId));
      } else {
        pending.delete(params.requestId);
        const {subscriptionId, delivery} = entry.body;
        if (entry.operation === 'unsubscribe') {
          const id = browserUrl ? entry.body.requestId : subscriptionId;
          retired.add(id);
          active.delete(id);
        } else if (delivery === '1') {
          baselines.set(subscriptionId, (baselines.get(subscriptionId) ?? 0) + 1);
        }
      }
    }
  });
  const settled = async expected => {
    await until(async () => {
      assert.deepEqual(errors, []);
      return pending.size === 0 && [...active.values()].every(reader => baselines.get(reader.token) === 1)
        && (!expected || [...expected].every(topic => [...active.values()].some(reader => reader.topic === topic)));
    });
    return [...active.values()].map(reader => reader.topic).sort();
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
        console.log(JSON.stringify({transport,view,cycle,topics}));
        assert.deepEqual(topics, [...required].sort(), view + ' reader set');
        await navigate('Overview');
        await until(async () => pending.size === 0 && active.size === 0);
      }
    }
    await navigate('Settings');
    const clickTab = label => page.evaluate(`Array.from(document.querySelectorAll('[aria-label="Settings sections"] [role="tab"]'))
      .find(button => button.textContent.trim() === ${JSON.stringify(label)}).click()`);
    const settingsTopics = () => [...active.values()].map(reader => reader.topic)
      .filter(topic => topic === 'system.inventory' || topic === 'companion.listener').sort();
    await new Promise(resolve => setTimeout(resolve, 200));
    await settled(browserUrl ? [] : ['storage.backups']);
    assert.deepEqual(settingsTopics(), [], 'Hidden settings tabs must not open readers');
    for (const [tab, topic] of [['Devices','system.inventory'], ['Companions','companion.listener']]) {
      await clickTab(tab);
      await settled([topic]);
      assert.deepEqual(settingsTopics(), [topic]);
      await clickTab('General');
      await until(async () => pending.size === 0 && settingsTopics().length === 0);
    }
    await navigate('Overview');
    await until(async () => pending.size === 0 && active.size === 0);
    const fixtures = [
      ['x-plane-11', 'X-Plane 11', process.env.OSG_NAVIGATION_XPLANE11_EXE],
      ['x-plane-12', 'X-Plane 12', process.env.OSG_NAVIGATION_XPLANE12_EXE],
    ];
    if (fixtures.every(([, , executable]) => executable)) {
      const {games} = await request({command:'game_get_state'});
      for (const [id, name, path] of fixtures) {
        if (games.catalogue.some(game => game.id === id && game.installed)) continue;
        const executable = resolve(path);
        const registered = await request({command:'game_add', name, executable, installPath:dirname(executable)});
        assert.ok(registered.games.catalogue.some(game => game.id === id && game.installed));
        addedGames.push(id);
      }
      await navigate('Profiles');
      const button = label => page.evaluate(`Array.from(document.querySelectorAll('main button'))
        .find(button => button.checkVisibility() && button.textContent.trim() === ${JSON.stringify(label)} && !button.disabled)?.click()`);
      await button('Add');
      const select = '[aria-label="Game context"]';
      await until(() => page.evaluate(`[...document.querySelector(${JSON.stringify(select)})?.options ?? []].some(option => option.value === 'x-plane-11')`));
      await page.evaluate(`(() => {
        const context = document.querySelector(${JSON.stringify(select)});
        context.value = 'x-plane-11';
        context.dispatchEvent(new Event('change', {bubbles:true}));
      })()`);
      await settled(['games.detail.x-plane-11']);
      await page.evaluate("window.dispatchEvent(new CustomEvent('osg:open-game-settings', {detail:{gameId:'x-plane-12'}}))");
      await settled(['games.detail.x-plane-12']);
      await until(async () => ![...active.values()].some(reader => reader.topic === 'games.detail.x-plane-11'));
      await navigate('Profiles');
      await settled(['games.detail.x-plane-11']);
      assert.equal(await page.evaluate(`document.querySelector(${JSON.stringify(select)}).value`), 'x-plane-11');
      await button('Cancel');
      await navigate('Overview');
      await until(async () => pending.size === 0 && active.size === 0);
    }
    assert.deepEqual(errors, []);
    assert.ok([...baselines.values()].every(count => count === 1));
  } finally {
    try {
      await discardDraft();
      const navigation = navigate('Overview');
      await page.evaluate('new Promise(resolve => setTimeout(resolve, 150))');
      await discardDraft();
      await navigation;
    }
    finally {
      stopTrace();
      try { for (const gameId of addedGames.reverse()) await request({command:'game_remove', gameId}); }
      finally { await page.close(); }
    }
  }
});
