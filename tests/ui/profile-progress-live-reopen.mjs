import assert from 'node:assert/strict';
import {execFile} from 'node:child_process';
import {randomUUID} from 'node:crypto';
import {readFile, realpath} from 'node:fs/promises';
import {isAbsolute, resolve} from 'node:path';
import test from 'node:test';
import {fileURLToPath} from 'node:url';
import {promisify} from 'node:util';
import {browserPage, nativePage, until} from '../../scripts/reactive/cdp.mjs';
import {progressSettingsInvoke} from './native-progress-settings.mjs';

const browserUrl = process.env.OSG_PROFILE_PROGRESS_LIVE_BROWSER_URL;
const run = process.env.OSG_PROFILE_PROGRESS_LIVE_RUN;
const appId = 426196196;
const gameName = 'Progress fixture';
const profileName = 'Progress reopen fixture';
const waitMessage = `Waiting for ${gameName} to start…`;
const transport = browserUrl ? 'browser-dev' : 'native';
const execute = promisify(execFile);

test(`${transport} shows real pending launch progress and retires it after hidden cancellation`, {
  skip:process.platform !== 'win32' || !run, timeout:125_000,
}, async () => {
  assert.ok(isAbsolute(run));
  const directory = await realpath(run);
  assert.equal(resolve(process.env.OSG_SQLITE_DATABASE ?? ''), resolve(directory, 'cockpit.sqlite'));
  const root = resolve(directory, 'programs/Steam');
  const executable = await realpath(resolve(root, 'steamapps/common/ProgressFixture/ProgressRuntime.exe'));
  const launcher = await realpath(resolve(root, 'steam.exe'));
  const marker = resolve(root, 'launch.started');
  await assert.rejects(readFile(marker), {code:'ENOENT'});
  const endpoint = new URL(browserUrl ?? process.env.OSG_NATIVE_CDP_URL ?? 'http://127.0.0.1:9222');
  assert.ok(endpoint.protocol === 'http:' && endpoint.hostname === '127.0.0.1'
    && !endpoint.username && !endpoint.password);
  const pid = process.env.OSG_PROFILE_PROGRESS_LIVE_APP_PID;
  if (!browserUrl) assert.match(pid ?? '', /^[1-9][0-9]*$/);

  const page = browserUrl ? await browserPage(endpoint.href) : await nativePage(endpoint.origin);
  const invoke = (command, args = {}, timeout) => page.evaluate(
    `window.__TAURI_INTERNALS__.invoke(${JSON.stringify(command)},${JSON.stringify(args)})`, timeout);
  const settings = progressSettingsInvoke(invoke);
  const request = value => page.evaluate(`import('/src/lib/gateway.ts').then(({gatewayRequest}) =>
    gatewayRequest(${JSON.stringify(value)}))`);
  const externalId = randomUUID();
  const external = value => {
    const payload = {...value, clientId:externalId};
    return browserUrl ? page.evaluate(`fetch('/api/gateway/', {method:'POST',
      headers:{'Content-Type':'application/json', 'X-OSG-Client':'1'},
      body:${JSON.stringify(JSON.stringify(payload))}}).then(response => response.json())`)
      : invoke('gateway_request', {request:payload}, 65_000);
  };
  const popupTarget = async () => (await (await fetch(new URL('/json/list', endpoint))).json())
    .find(target => target.type === 'page' && new URL(target.url).searchParams.has('profile-activation-window'));
  const popupVisibility = async action => {
    const script = fileURLToPath(new URL('./native-progress-visibility.ps1', import.meta.url));
    const {stdout} = await execute('powershell.exe', ['-NoProfile', '-NonInteractive', '-File',
      script, '-ProcessId', pid, '-Action', action], {windowsHide:true, timeout:10_000});
    const handle = stdout.trim();
    assert.match(handle, /^[1-9][0-9]*$/);
    return handle;
  };
  const documentHidden = hidden => page.evaluate(`(() => {
    if (${hidden}) Object.defineProperty(document, 'hidden', {configurable:true, value:true});
    else delete document.hidden;
    document.dispatchEvent(new Event('visibilitychange'));
  })()`);
  const modalMessage = () => page.evaluate(`document.querySelector(
    'dialog[aria-label="Profile operation progress"][open] [role="status"]')?.textContent.trim()`);
  const progress = match => until(() => page.evaluate(
    `window.__liveProgress?.events.find(entry => ${match})`));
  const status = async () => (await request({command:'profile_status'})).profiles.profileSession;
  const normalized = path => path.replace(/^\\\\\?\\/, '').replaceAll('/', '\\').toLowerCase();

  const refreshes = [];
  let gameId, profile, operationId, launch, child, savedSettings;
  let stopTrace = () => {};
  try {
    await until(() => page.evaluate(`document.querySelector(
      'nav[aria-label="Main navigation"] button[title="Overview"]')?.classList.contains('current')`));
    assert.equal((await status()).state, 'stopped');
    if (!browserUrl) {
      savedSettings = await settings('profile_progress_settings_get');
      await settings('profile_progress_settings_save', {settings:{delivery:'window', position:'bottom-right'}});
    }
    await page.evaluate(`(async () => {
      const {observeGameDiscovery} = await import('/src/lib/game-read-models/shared.ts');
      const probe = window.__liveProgress = {events:[]};
      probe.stopDiscovery = observeGameDiscovery(state => { if (state.status === 'ready') probe.discovery = state.value; });
    })()`);
    const discovery = () => page.evaluate('window.__liveProgress.discovery');
    if ((await until(discovery)).status === 'idle') await request({command:'game_discovery_start'});
    const review = await until(discovery, value => value?.status === 'ready', 75_000);
    const candidate = review.entries.find(entry => entry.action === 'add' && entry.name === gameName
      && entry.executableCandidates.some(path => normalized(path) === normalized(executable)));
    assert.ok(candidate, 'The disposable Steam candidate was not discovered');
    await request({command:'game_discovery_apply', scanId:review.scanId, actions:review.entries.map(entry =>
      entry.id === candidate.id ? {id:entry.id, selected:true, executable} : {id:entry.id, selected:false})});
    const game = (await request({command:'game_get_state'})).games.catalogue.find(entry => entry.id === candidate.gameId);
    assert.ok(game);
    gameId = game.id;
    await page.evaluate('window.__liveProgress.stopDiscovery()');
    assert.equal(game.name, gameName);
    assert.equal(game.steamAppId, appId);
    assert.equal(normalized(game.steamExecutable), normalized(launcher));
    assert.equal(normalized(game.executable), normalized(executable));

    profile = (await request({command:'profile_create', name:profileName})).profiles.document;
    for (const field of ['resources','virtualOutputs','desktopOutputs','bindings','companionAssignments']) {
      assert.equal(profile[field]?.length ?? 0, 0, 'Use an output-free fixture profile');
    }
    profile.eligibility = {gameIds:[gameId]};
    profile = (await request({command:'profile_editor_save', document:profile, assignments:[],
      expectedRevision:profile.revision})).profiles.document;
    await page.evaluate(`(async () => {
      const {observeProfileActivation} = await import('/src/lib/profile-activation-observation.ts');
      const probe = window.__liveProgress;
      probe.stop = observeProfileActivation(entry => { if (entry) probe.events.push(entry); });
    })()`);
    await page.call('Network.enable', {maxPostDataSize:65536});
    stopTrace = page.listen(({method, params}) => {
      if (method !== 'Network.requestWillBeSent' || !params.request.postData) return;
      const path = new URL(params.request.url).pathname;
      if (!['/gateway_request','/api/gateway/'].includes(path)) return;
      const payload = JSON.parse(params.request.postData);
      const command = (payload.request ?? payload).command;
      if (['profile_activation_current','profile_activation_status'].includes(command)) refreshes.push(command);
    });
    await request({command:'profile_activation_current'});
    assert.deepEqual(refreshes.splice(0), ['profile_activation_current'], 'The trace must see the control read');

    if (browserUrl) await documentHidden(true);
    launch = external({command:'profile_game_launch', gameId, profileId:profile.id,
      expectedActiveOwner:null, rememberChoice:false});
    void launch.catch(() => {});
    const waiting = await progress(`entry.profileId === ${JSON.stringify(profile.id)}
      && entry.origin === 'game-launch' && entry.phase === 'launching'
      && entry.message === ${JSON.stringify(waitMessage)}`);
    operationId = waiting.activationId;
    assert.equal(await until(() => readFile(marker, 'utf8').catch(() => '')), `-applaunch ${appId}`);

    if (browserUrl) {
      await documentHidden(false);
      await until(modalMessage, value => value === waitMessage);
      await documentHidden(true);
    } else {
      child = await nativePage(endpoint.origin, (await until(popupTarget)).webSocketDebuggerUrl);
      const handle = await popupVisibility('hide');
      assert.equal(await popupVisibility('reveal'), handle);
      await until(() => child.evaluate(`document.querySelector('.progress-message')?.textContent.trim()`),
        value => value === waitMessage);
      assert.equal(await child.evaluate(`document.querySelector('.profile-name')?.textContent.trim()`), profileName);
      assert.equal(await popupVisibility('hide'), handle);
    }

    const receipt = await external({command:'profile_activation_cancel', activationId:operationId});
    assert.equal(receipt.ok, true, receipt.error);
    assert.equal(receipt.state.profileActivation.cancelRequested, true);
    const terminal = await progress(`entry.activationId === ${JSON.stringify(operationId)} && entry.phase === 'cancelled'`);
    assert.equal(terminal.outcome, 'inactive');
    const result = await launch;
    launch = undefined;
    assert.equal(result.ok, false, 'The cancelled launch must not report success');
    if (browserUrl) {
      await until(modalMessage, value => value === undefined);
      await documentHidden(false);
      assert.equal(await modalMessage(), undefined);
    } else {
      await until(popupTarget, value => value === undefined);
    }
    assert.equal((await status()).state, 'stopped');
    assert.equal((await status()).owner, null);
    assert.deepEqual(refreshes, []);
    console.log(JSON.stringify({transport, realLaunch:true, shownOnReveal:true,
      cancelledWhileHidden:true, presenterRetired:true, owner:null, legacyProgressReads:refreshes.length}));
  } finally {
    stopTrace();
    try {
      if (launch) {
        if (!operationId) operationId = await page.evaluate('window.__liveProgress?.events.at(-1)?.activationId');
        if (operationId) await external({command:'profile_activation_cancel', activationId:operationId}).catch(() => {});
        await launch.catch(() => {});
      }
      if (browserUrl) await documentHidden(false);
      await page.evaluate(`(() => {
        const probe = window.__liveProgress;
        probe?.stopDiscovery?.();
        probe?.stop?.();
        delete window.__liveProgress;
      })()`);
      if (profile) await request({command:'profile_delete', id:profile.id, expectedRevision:profile.revision});
      if (gameId) await request({command:'game_remove', gameId});
      if (savedSettings) await settings('profile_progress_settings_save', {settings:savedSettings});
    } finally {
      child?.close();
      await page.close();
    }
  }
});
