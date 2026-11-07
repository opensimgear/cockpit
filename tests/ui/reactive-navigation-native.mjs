// Running Cockpit lifetime check. Observe native IPC without replacing it.
import assert from 'node:assert/strict';
import {randomUUID} from 'node:crypto';
import test from 'node:test';
import {dirname, resolve, toNamespacedPath} from 'node:path';
import {browserPage, nativePage, until} from '../../scripts/reactive/cdp.mjs';
import {checkActivitySettings} from './reactive-navigation/activity-settings.mjs';
const browserUrl = process.env.OSG_NAVIGATION_BROWSER_URL;
const transport = browserUrl ? 'browser development' : 'native';
const browserOpenId = ({ connectionId, requestId }) => JSON.stringify([connectionId, requestId]);
const endpoints = new Map([
  ['/observation_subscribe','subscribe'],
  ['/observation_unsubscribe','unsubscribe'],
  ['/observation_acknowledge','ack'],
  ['/api/observe/subscribe','subscribe'],
  ['/api/observe/unsubscribe','unsubscribe'],
  ['/api/observe/ack','ack'],
]);

test(`${transport} main view navigation retires observation readers without duplicate baselines`, {timeout:240000}, async () => {
  const page = browserUrl ? await browserPage(browserUrl) : await nativePage();
  const attemptedGames = new Map();
  const discardDraft = () => page.evaluate(`Array.from(document.querySelectorAll('dialog[open] button'))
    .find(button => button.textContent.trim() === 'Discard and continue')?.click()`);
  const request = (value, timeout) => page.evaluate(`import('/src/lib/gateway.ts').then(({gatewayRequest}) => gatewayRequest(${JSON.stringify(value)}))`, timeout);
  const pending = new Map();
  const active = new Map();
  const opened = new Map();
  const baselines = new Map();
  const deliveries = new Map();
  const errors = [];
  const retired = new Set();
  const abortedOpens = new Map();
  let companionReads = 0;
  let systemInventoryReads = 0;
  const navigate = async (view, discard = false) => {
    const label = JSON.stringify(view);
    const selector = view === 'Settings' ? '.sidebar-bottom button' : 'nav[aria-label="Main navigation"] button';
    await until(() => page.evaluate(`(() => {
      const target = Array.from(document.querySelectorAll(${JSON.stringify(selector)}))
        .find(button => (button.title || button.textContent.trim()) === ${label});
      if (!target) return false;
      target.click();
      return true;
    })()`));
    await until(async () => {
      if (discard) await discardDraft();
      return page.evaluate(`document.querySelector('[aria-label="Breadcrumb"] [aria-current="page"]')?.textContent.trim() === ${label}`);
    });
  };
  const stopTrace = page.listen(({method, params}) => {
    if (method === 'Network.requestWillBeSent' && params.request.postData) {
      const path = new URL(params.request.url).pathname;
      if (path === '/gateway_request' || path === '/api/gateway/') {
        const body = JSON.parse(params.request.postData);
        if ((body.request ?? body).command === 'companion_status') companionReads++;
        if ((body.request ?? body).command === 'system_inventory') systemInventoryReads++;
      }
      const operation = endpoints.get(path);
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
          // Browser disposal aborts the open HTTP request and sends its exact
          // cancellation separately. Require that cancellation below.
          if (browserUrl && entry.operation === 'subscribe' && params.canceled) {
            abortedOpens.set(browserOpenId(entry.body), detail);
          } else {
            errors.push(detail);
          }
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
          const id = browserUrl ? browserOpenId(entry.body) : token;
          assert.ok(!opened.has(token), 'An observation token must identify one reader lifetime');
          opened.set(token, entry.body.topic);
          if (!retired.has(id)) active.set(id, {topic:entry.body.topic, token});
        }).catch(error => errors.push(error.message)).finally(() => pending.delete(params.requestId));
      } else {
        pending.delete(params.requestId);
        const {subscriptionId, delivery} = entry.body;
        if (entry.operation === 'unsubscribe') {
          const id = browserUrl ? browserOpenId(entry.body) : subscriptionId;
          retired.add(id);
          active.delete(id);
        } else if (delivery === '1') {
          baselines.set(subscriptionId, (baselines.get(subscriptionId) ?? 0) + 1);
        } else {
          deliveries.set(subscriptionId, delivery);
        }
      }
    }
  });
  const settled = async (expected, exact = false) => {
    await until(async () => {
      assert.deepEqual(errors, []);
      return pending.size === 0 && (!exact || active.size === expected.length)
        && [...active.values()].every(reader => baselines.get(reader.token) === 1)
        && (!expected || [...expected].every(topic => [...active.values()].some(reader => reader.topic === topic)));
    }).catch(error => {
      if (errors.length) throw error;
      const actual = [...active.values()].map(reader => [reader.topic, baselines.get(reader.token) ?? 0]);
      throw new Error(`Expected ${expected.join(', ')}; observed ${JSON.stringify(actual)} with ${pending.size} pending requests.`, { cause:error });
    });
    return [...active.values()].map(reader => reader.topic).sort();
  };
  try {
    await navigate('Overview');
    const companionLibraryTopics = browserUrl
      ? ['companion.firmware-releases']
      : ['companion.firmware-releases', 'designer.workspace'];
    const views = [
      ['Devices', ['system.inventory.vjoy']],
      ['Profiles', []],
      ['Games', ['games.discovery', 'games.jobs']],
      ['Companions', companionLibraryTopics],
      ['Debug', ['games.telemetry']],
      ['Activity', ['activity.log']],
    ];
    // Some models are shared with the navigation shell and intentionally
    // survive view changes. Settle their first use before tracing view readers.
    for (const [view] of views) await navigate(view);
    await navigate('Overview');
    await page.call('Network.enable', {maxPostDataSize:65536});
    if (browserUrl) {
      let paused;
      const stopPaused = page.listen(event => {
        if (event.method === 'Fetch.requestPaused') paused = event.params;
      });
      await page.call('Fetch.enable', { patterns: [{urlPattern:'*/api/observe/subscribe'}] });
      try {
        await page.evaluate(`import('/src/lib/companion-observations.ts').then(({companionObservations}) => {
          window.stopNavigationProbe = companionObservations().subscribe('companion.firmware', () => {});
        })`);
        await until(() => paused);
        const body = JSON.parse(paused.request.postData);
        assert.equal(body.topic, 'companion.firmware');
        const id = browserOpenId(body);
        await page.evaluate('window.stopNavigationProbe(); delete window.stopNavigationProbe;');
        await until(() => abortedOpens.has(id) && retired.has(id));
      } finally {
        await page.call('Fetch.disable');
        stopPaused();
      }
    }
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
    await navigate('Debug');
    const clickDebugTab = label => page.evaluate(`Array.from(document.querySelectorAll('[aria-label="Debug sections"] [role="tab"]'))
      .find(button => button.textContent.trim() === ${JSON.stringify(label)}).click()`);
    const debugTopics = ['profiles.retired-recovery'];
    const firmwareTopics = ['companion.firmware', 'companion.firmware-releases'];
    const detailsTopics = [...debugTopics, ...firmwareTopics];
    const firmwareReaders = () => [...active.entries()]
      .filter(([, reader]) => firmwareTopics.includes(reader.topic)).map(([id]) => id);
    // The Devices sidebar count retains the shared companion readers.
    // Opening Debug or the designer must not create a second pair.
    const companionDetails = () => page.evaluate(`[...document.querySelectorAll('.profile-debug details')]
      .find(item => item.querySelector('summary')?.textContent.trim() === 'Companion loading and image cache details')
      ?.querySelector('pre')?.textContent.trim()`);
    const setDebugDetails = (label, open) => page.evaluate(`(() => {
      const summary = [...document.querySelectorAll('.profile-debug details > summary')]
        .find(row => row.textContent.trim() === ${JSON.stringify(label)});
      if (summary.parentElement.open !== ${open}) summary.click();
    })()`);
    const companionsOpen = open => setDebugDetails('Companion loading and image cache details', open);
    const desktopOpen = open => setDebugDetails('Native runtime and desktop output details', open);
    await settled(['games.telemetry']);
    for (let cycle = 0; cycle < 2; cycle++) {
      await clickDebugTab('Runtime & recovery');
      await settled(debugTopics, true);
      await desktopOpen(true);
      await settled([...debugTopics, 'profiles.desktop'], true);
      const desktopReader = [...active.entries()].find(([, reader]) => reader.topic === 'profiles.desktop')[0];
      await desktopOpen(false);
      await until(() => retired.has(desktopReader));
      await settled(debugTopics, true);
      const beforeOpen = companionReads;
      await companionsOpen(true);
      await until(companionDetails, value => !!value && value !== 'null');
      await settled(detailsTopics, true);
      assert.equal(companionReads - beforeOpen, 1, 'The four companion baselines must share one status read');
      if (cycle === 0) {
        const visibility = hidden => page.evaluate(`(() => {
          if (${hidden}) Object.defineProperty(document, 'hidden', {configurable:true, value:true});
          else delete document.hidden;
          document.dispatchEvent(new Event('visibilitychange'));
        })()`);
        const hiddenReaders = firmwareReaders();
        await visibility(true);
        const beforeReopen = companionReads;
        try {
          await until(companionDetails, value => value === 'null');
          await until(() => hiddenReaders.every(id => retired.has(id)));
        } finally {
          await visibility(false);
        }
        await until(companionDetails, value => !!value && value !== 'null');
        await settled(detailsTopics, true);
        assert.equal(companionReads - beforeReopen, 1, 'Reopen must load a single current companion snapshot');
      }
      const closingReaders = firmwareReaders();
      await companionsOpen(false);
      await until(() => closingReaders.every(id => retired.has(id)));
      await settled(debugTopics, true);
      await clickDebugTab('Incoming telemetry');
      await settled(['games.telemetry'], true);
    }
    await navigate('Overview');
    await until(() => pending.size === 0 && active.size === 0);
    for (let cycle = 0; cycle < 2; cycle++) {
      const before = companionReads;
      await navigate('Companions');
      await settled(companionLibraryTopics, true);
      await until(() => companionReads > before);
      await new Promise(resolve => setTimeout(resolve, 200));
      assert.ok(companionReads - before <= 2, 'The visible list alone reads companion status at open');
      await until(() => page.evaluate(`(() => {
        const button = [...document.querySelectorAll('section[aria-label="Panels"] button')]
          .find(button => button.textContent.trim() === 'New');
        if (!button || button.disabled) return false;
        button.click();
        return true;
      })()`));
      await until(() => page.evaluate(`!!document.querySelector('[aria-label="Panel name"]')`));
      await settled(browserUrl ? [] : ['designer.workspace'], true);
      await page.evaluate(`(() => {
        const confirm = window.confirm;
        try {
          window.confirm = message => message === 'Discard unsaved panel changes?';
          document.querySelector('section.compiled-designer > .page-header .header-back').click();
        } finally {
          window.confirm = confirm;
        }
      })()`);
      await until(() => page.evaluate(`!!document.querySelector('section[aria-label="Panels"]')`));
      await settled(companionLibraryTopics, true);
      await navigate('Overview');
      await settled([], true);
    }
    await navigate('Settings');
    const clickTab = label => page.evaluate(`Array.from(document.querySelectorAll('[aria-label="Settings sections"] [role="tab"]'))
      .find(button => button.textContent.trim() === ${JSON.stringify(label)}).click()`);
    const deviceTopics = ['settings.desktop', 'system.inventory', 'system.inventory.drivers', 'system.inventory.vjoy'];
    const settingsTopics = () => [...active.values()].map(reader => reader.topic)
      .filter(topic => deviceTopics.includes(topic) || topic === 'companion.listener' || topic === 'settings.mcp').sort();
    await new Promise(resolve => setTimeout(resolve, 200));
    await settled(browserUrl ? ['settings.activity'] : ['settings.activity', 'storage.backups']);
    assert.deepEqual(settingsTopics(), [], 'Hidden settings tabs must not open readers');
    const activityReader = () => [...active.values()].find(reader => reader.topic === 'settings.activity')?.token;
    await checkActivitySettings({
      page, request, clickTab, activityReader, deliveries,
      waitForReader: () => settled(['settings.activity']),
      waitForRetirement: () => until(() => pending.size === 0 && !activityReader()),
    });
    const backupReader = () => [...active.values()].find(reader => reader.topic === 'storage.backups')?.token;
    const backupRow = id => page.evaluate(`Array.from(document.querySelectorAll('[data-backup-id]'))
      .some(row => row.dataset.backupId === ${JSON.stringify(id)})`);
    if (browserUrl) {
      assert.equal(backupReader(), undefined);
      assert.ok(await page.evaluate(`document.querySelector('[aria-label="Backups"]')
        ?.textContent.includes('Open the desktop Cockpit to manage backups.')`));
    } else {
      const previousReader = backupReader();
      const {storage: before} = await request({command:'storage_status'});
      assert.ok(before.backups.length < before.maximumBackups, 'The dev Cockpit needs one free backup slot');
      await clickTab('Devices');
      await until(() => pending.size === 0 && !backupReader());
      let backup;
      try {
        const {storage: created} = await request({command:'storage_create', expectedRevision:before.revision}, 45_000);
        backup = created.backup;
        assert.ok(backup?.id);
        assert.equal(backupReader(), undefined, 'A hidden backup view must have no active reader');
        assert.equal(await backupRow(backup.id), false);
        await clickTab('General');
        await settled(['storage.backups']);
        assert.notEqual(backupReader(), previousReader);
        await until(() => backupRow(backup.id));
      } finally {
        if (backup?.id) {
          const {storage: current} = await request({command:'storage_status'});
          const {storage: removed} = await request({command:'storage_remove', id:backup.id, expectedRevision:current.revision}, 45_000);
          assert.equal(removed.filesRemoved, true);
        }
      }
      await until(() => backupRow(backup.id), visible => !visible);
      await until(() => pending.size === 0 && settingsTopics().length === 0);
    }
    const {desktop} = await request({command:'profile_desktop_status'});
    const desktopReader = () => [...active.values()].find(reader => reader.topic === 'settings.desktop')?.token;
    const readDesktopToggle = () => page.evaluate(`(() => {
      const card = document.querySelector('section[aria-label="Keyboard and mouse"]');
      const toggle = card?.querySelector('[role="switch"]');
      return toggle && {checked:toggle.checked, disabled:toggle.disabled,
        note:card.querySelector('.status-note')?.textContent.trim()};
    })()`);
    if (desktop.qualified && !desktop.error) {
      const saveDesktop = async enabled => {
        const {localSettings} = await request({command:'profile_local_settings_get', scope:'desktop'});
        assert.equal(localSettings.profileSession.owner, null, 'Desktop settings fixture requires a stopped profile');
        await request({command:'profile_local_settings_save', scope:'desktop',
          expectedSettingsRevision:localSettings.revision, expectedActiveOwner:null,
          settings:{enabled, scope:localSettings.settings.scope}});
      };
      await clickTab('Devices');
      await settled(deviceTopics);
      await until(readDesktopToggle, state => state?.checked === desktop.enabled && !state.disabled);
      const firstReader = desktopReader();
      assert.ok(firstReader);
      try {
        const beforeSave = deliveries.get(firstReader);
        await saveDesktop(!desktop.enabled);
        await until(readDesktopToggle, state => state?.checked === !desktop.enabled);
        await until(() => deliveries.get(firstReader) !== beforeSave);
        await clickTab('General');
        await until(() => pending.size === 0 && !desktopReader());
        await saveDesktop(desktop.enabled);
        assert.equal(desktopReader(), undefined);
        await clickTab('Devices');
        await settled(deviceTopics);
        assert.notEqual(desktopReader(), firstReader);
        await until(readDesktopToggle, state => state?.checked === desktop.enabled);
      } finally {
        const {desktop: current} = await request({command:'profile_desktop_status'});
        if (current.enabled !== desktop.enabled) await saveDesktop(desktop.enabled);
      }
      await clickTab('General');
      await until(() => pending.size === 0 && !desktopReader());
      console.log(JSON.stringify({transport, desktopSettingsBackgroundAndReopen: 'passed'}));
    }
    for (let cycle = 0; cycle < 2; cycle++) {
      for (const [tab, topics] of [['Devices', deviceTopics], ['Companions', ['companion.listener']],
        ['AI integration', browserUrl ? [] : ['settings.mcp']]]) {
        const beforeInventory = systemInventoryReads;
        await clickTab(tab);
        if (tab === 'AI integration') {
          await until(() => page.evaluate(`!!document.querySelector('section[aria-label="AI integration settings"]')`));
        }
        await settled(topics);
        assert.deepEqual(settingsTopics(), topics);
        if (tab === 'Devices') {
          await until(() => systemInventoryReads > beforeInventory);
          assert.equal(systemInventoryReads - beforeInventory, 1, 'The inventory baseline owns the initial read');
          await until(readDesktopToggle, state => state?.checked === desktop.enabled
            && state.disabled === (!desktop.qualified || !!desktop.error)
            && (desktop.qualified || state.note === desktop.reason));
        }
        await clickTab('General');
        await settled(['settings.activity']);
        await until(async () => pending.size === 0 && settingsTopics().length === 0);
      }
    }
    await navigate('Overview');
    await until(async () => pending.size === 0 && active.size === 0);
    // Reopening Profiles must reuse the shell's committed library projection.
    const name = `Profile navigation ${randomUUID()}`;
    const hasProfile = label => page.evaluate(`[...document.querySelectorAll('[aria-label="Saved profiles"] .profile-row')]
      .some(row => row.title === ${JSON.stringify(label)})`);
    let profile;
    try {
      ({profiles:{document:profile}} = await request({command:'profile_create', name}));
      await navigate('Profiles');
      await until(() => hasProfile(name));
      await settled([], true);
      const visibleName = `${name} visible`;
      ({profiles:{document:profile}} = await request({
        command:'profile_rename', id:profile.id, expectedRevision:profile.revision, name:visibleName,
      }));
      await until(() => hasProfile(visibleName));
      assert.equal(await hasProfile(name), false);
      await navigate('Overview');
      const hiddenName = `${name} hidden`;
      ({profiles:{document:profile}} = await request({
        command:'profile_rename', id:profile.id, expectedRevision:profile.revision, name:hiddenName,
      }));
      await navigate('Profiles');
      await until(() => hasProfile(hiddenName));
      assert.equal(await hasProfile(visibleName), false);
      await settled([], true);
    } finally {
      if (profile) await request({command:'profile_delete', id:profile.id, expectedRevision:profile.revision});
      await navigate('Overview');
    }
    const fixtures = [
      ['x-plane-11', 'X-Plane 11', process.env.OSG_NAVIGATION_XPLANE11_EXE],
      ['x-plane-12', 'X-Plane 12', process.env.OSG_NAVIGATION_XPLANE12_EXE],
    ];
    if (fixtures.every(([, , executable]) => executable)) {
      const {games} = await request({command:'game_get_state'});
      for (const [id, name, path] of fixtures) {
        if (games.catalogue.some(game => game.id === id && game.installed)) continue;
        const executable = resolve(path);
        attemptedGames.set(id, toNamespacedPath(executable));
        const registered = await request({command:'game_add', name, executable, installPath:dirname(executable)}, 45_000);
        assert.ok(registered.games.catalogue.some(game => game.id === id && game.installed));
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
    assert.deepEqual([...abortedOpens].filter(([id]) => !retired.has(id)), [],
      'Every aborted open must have an accepted cancellation for the same request');
    if (browserUrl) console.log(JSON.stringify({ transport, canceledOpens: abortedOpens.size }));
    assert.ok([...baselines.values()].every(count => count === 1));
  } finally {
    try {
      await navigate('Overview', true);
    }
    finally {
      stopTrace();
      try {
        if (attemptedGames.size) {
          // A native registration can commit even if its DevTools reply times out.
          const {games} = await request({command:'game_get_state'});
          for (const game of games.catalogue) {
            const executable = attemptedGames.get(game.id);
            if (executable && game.installations.length === 1
                && toNamespacedPath(game.installations[0].executable ?? '') === executable) {
              const removed = await request({command:'game_remove', gameId:game.id}, 45_000);
              assert.ok(!removed.games.catalogue.some(entry => entry.id === game.id && entry.installed));
            }
          }
        }
      }
      finally { await page.close(); }
    }
  }
});
