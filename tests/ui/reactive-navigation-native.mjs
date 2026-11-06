// Running Cockpit lifetime check. Observe native IPC without replacing it.
import assert from 'node:assert/strict';
import test from 'node:test';
import {dirname, resolve} from 'node:path';
import {browserPage, nativePage, until} from '../../scripts/reactive/cdp.mjs';

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

test(`${transport} main view navigation retires observation readers without duplicate baselines`, {timeout:60000}, async () => {
  const page = browserUrl ? await browserPage(browserUrl) : await nativePage();
  const addedGames = [];
  const discardDraft = () => page.evaluate(`Array.from(document.querySelectorAll('dialog[open] button'))
    .find(button => button.textContent.trim() === 'Discard and continue')?.click()`);
  const request = value => page.evaluate(`import('/src/lib/gateway.ts').then(({gatewayRequest}) => gatewayRequest(${JSON.stringify(value)}))`);
  const pending = new Map();
  const active = new Map();
  const opened = new Map();
  const baselines = new Map();
  const errors = [];
  const retired = new Set();
  const navigate = async (view, discard = false) => {
    const label = JSON.stringify(view);
    const selector = view === 'Settings' ? '.sidebar-bottom button' : 'nav[aria-label="Main navigation"] button';
    await page.evaluate(`Array.from(document.querySelectorAll(${JSON.stringify(selector)}))
      .find(button => (button.title || button.textContent.trim()) === ${label}).click()`);
    await until(async () => {
      if (discard) await discardDraft();
      return page.evaluate(`document.querySelector('[aria-label="Breadcrumb"] [aria-current="page"]')?.textContent.trim() === ${label}`);
    });
  };
  const stopTrace = page.listen(({method, params}) => {
    if (method === 'Network.requestWillBeSent' && params.request.postData) {
      const operation = endpoints.get(new URL(params.request.url).pathname);
      if (!operation) return;
      pending.set(params.requestId, {operation, body:JSON.parse(params.request.postData)});
    } else if (method === 'Network.responseReceived') {
      const entry = pending.get(params.requestId);
      if (entry) entry.status = params.response.status;
    } else if (method === 'Network.loadingFinished' || method === 'Network.loadingFailed') {
      const entry = pending.get(params.requestId);
      if (!entry) return;
      if (entry.status !== 200 || method === 'Network.loadingFailed') {
        const detail = [entry.operation, entry.body.topic, entry.status, params.errorText].filter(Boolean).join(' ');
        if (method === 'Network.loadingFailed') {
          errors.push(detail);
          pending.delete(params.requestId);
        } else {
          void page.call('Network.getResponseBody', {requestId:params.requestId})
            .then(({body}) => errors.push(detail + ' ' + body), () => errors.push(detail))
            .finally(() => pending.delete(params.requestId));
        }
      } else if (entry.operation === 'subscribe') {
        void page.call('Network.getResponseBody', {requestId:params.requestId}).then(({body}) => {
          const response = JSON.parse(body);
          const token = browserUrl ? response.subscriptionId : response;
          assert.equal(typeof token, 'string');
          // Bridge cancellation identifies the opening request. Baselines
          // still acknowledge the resulting native subscription token.
          const id = browserUrl ? entry.body.requestId : token;
          assert.ok(!opened.has(token), 'An observation token must identify one reader lifetime');
          opened.set(token, entry.body.topic);
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
    const deviceTopics = ['settings.desktop', 'system.inventory'];
    const settingsTopics = () => [...active.values()].map(reader => reader.topic)
      .filter(topic => deviceTopics.includes(topic) || topic === 'companion.listener').sort();
    await new Promise(resolve => setTimeout(resolve, 200));
    await settled(browserUrl ? [] : ['storage.backups']);
    assert.deepEqual(settingsTopics(), [], 'Hidden settings tabs must not open readers');
    const {desktop} = await request({command:'profile_desktop_status'});
    for (let cycle = 0; cycle < 2; cycle++) {
      for (const [tab, topics] of [['Devices',deviceTopics], ['Companions',['companion.listener']]]) {
        await clickTab(tab);
        await settled(topics);
        assert.deepEqual(settingsTopics(), topics);
        if (tab === 'Devices') {
          const keyboard = 'section[aria-label="Keyboard and mouse"]';
          const read = () => page.evaluate(`(() => {
            const card = document.querySelector('${keyboard}');
            const toggle = card?.querySelector('[role="switch"]');
            return toggle && {checked:toggle.checked, disabled:toggle.disabled,
              note:card.querySelector('.status-note')?.textContent.trim()};
          })()`);
          await until(read, state => state?.checked === desktop.enabled
            && state.disabled === (!desktop.qualified || !!desktop.error)
            && (desktop.qualified || state.note === desktop.reason));
        }
        await clickTab('General');
        await until(async () => pending.size === 0 && settingsTopics().length === 0);
      }
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
      const profileTopic = 'games.detail.x-plane-11';
      const settingsTopic = 'games.detail.x-plane-12';
      const exactQuery = async topic => {
        await settled([topic]);
        await until(async () => {
          const details = [...active.values()].filter(reader => reader.topic.startsWith('games.detail.'));
          return details.length === 1 && details[0].topic === topic;
        });
      };
      await exactQuery(profileTopic);
      for (let cycle = 0; cycle < 3; cycle++) {
        await page.evaluate("window.dispatchEvent(new CustomEvent('osg:open-game-settings', {detail:{gameId:'x-plane-12'}}))");
        await exactQuery(settingsTopic);
        await navigate('Profiles');
        await exactQuery(profileTopic);
        assert.equal(await page.evaluate(`document.querySelector(${JSON.stringify(select)}).value`), 'x-plane-11');
      }
      assert.equal([...opened.values()].filter(topic => topic === profileTopic).length, 4);
      assert.equal([...opened.values()].filter(topic => topic === settingsTopic).length, 3);
      await button('Cancel');
      await navigate('Overview', true);
      await until(async () => pending.size === 0 && active.size === 0);
    }
    assert.deepEqual(errors, []);
    assert.ok([...baselines.values()].every(count => count === 1));
  } finally {
    try {
      await navigate('Overview', true);
    }
    finally {
      stopTrace();
      try { for (const gameId of addedGames.reverse()) await request({command:'game_remove', gameId}); }
      finally { await page.close(); }
    }
  }
});
