import assert from 'node:assert/strict';
import {execFile, spawn} from 'node:child_process';
import {randomUUID} from 'node:crypto';
import {once} from 'node:events';
import {copyFile, mkdir, mkdtemp, writeFile} from 'node:fs/promises';
import {resolve} from 'node:path';
import {promisify} from 'node:util';
import test from 'node:test';
import {browserPage, nativePage, until} from '../../scripts/reactive/cdp.mjs';

const browserUrl = process.env.OSG_PROFILE_AUTOMATIC_REOPEN_BROWSER_URL;
const enabled = process.platform === 'win32' && (browserUrl || process.env.OSG_PROFILE_AUTOMATIC_REOPEN_NATIVE === '1');
const transport = browserUrl ? 'browser-dev' : 'native';
const execute = promisify(execFile);
const refreshCommands = new Set(['profile_status', 'profile_list', 'profile_automatic_status', 'profile_activation_status']);

test(`${transport} Profiles reopens on the current automatic selection`, {skip:!enabled, timeout:120_000}, async () => {
  const root = resolve('.research/reactive/profile-automatic-reopen/runs');
  await mkdir(root, {recursive:true});
  const directory = await mkdtemp(resolve(root, 'run-'));
  const source = resolve(directory, 'ProcessFixture.cs');
  const binary = resolve(directory, 'AutomaticProfileFixture.exe');
  await writeFile(source, 'public static class ProcessFixture { public static void Main() { System.Threading.Thread.Sleep(System.Threading.Timeout.Infinite); } }');
  await execute('C:\\Windows\\Microsoft.NET\\Framework64\\v4.0.30319\\csc.exe',
    ['/nologo', '/target:winexe', `/out:${binary}`, source], {windowsHide:true});

  const page = browserUrl ? await browserPage(browserUrl) : await nativePage();
  const request = value => page.evaluate(`import('/src/lib/gateway.ts').then(({gatewayRequest}) => gatewayRequest(${JSON.stringify(value)}))`);
  const stopExternally = async activationId => {
    const value = {command:'profile_stop', activationId, clientId:'automatic-reopen-external'};
    const reply = await page.evaluate(browserUrl
      ? `fetch('/api/gateway/',{method:'POST',headers:{'Content-Type':'application/json','X-OSG-Client':'1'},body:${JSON.stringify(JSON.stringify(value))}}).then(response=>response.json())`
      : `window.__TAURI_INTERNALS__.invoke('gateway_request',{request:${JSON.stringify(value)}})`);
    assert.equal(reply.ok, true, reply.error);
  };
  const fixtures = [];
  const reads = [];
  const cleanupErrors = [];
  let child, modeBefore, tracking = false, stopTrace = () => {};
  const evidence = {transport, boundaries:[], reads, cleanupErrors};
  const navigate = async label => {
    const selector = `nav[aria-label="Main navigation"] button[title="${label}"]`;
    await until(() => page.evaluate(`(() => {
      const button = document.querySelector(${JSON.stringify(selector)});
      if (!button) return false;
      if (!button.classList.contains('current')) { button.click(); return false; }
      return true;
    })()`));
  };
  const readRuntime = () => page.evaluate('window.__automaticReopen?.current');
  const noMatch = state => state?.automatic === 'blocked' && state.message.startsWith('Waiting for a running game');
  const selected = fixture => until(readRuntime, current => current?.owner?.profileId === fixture.profile.id
    && ['running', 'degraded'].includes(current.state) && current.automatic === 'active', 20_000);
  const completed = (field, value, origin) => until(() => page.evaluate(`window.__automaticReopen?.completed.find(
    entry => entry[${JSON.stringify(field)}] === ${JSON.stringify(value)} && entry.origin === ${JSON.stringify(origin)})`));
  const overview = () => page.evaluate(`document.querySelector('[aria-label="Active profile"] .overview-value')?.textContent.trim()`);
  const detected = fixture => page.evaluate(`[...document.querySelectorAll('.detected-game > strong')]
    .some(row => row.textContent.trim() === ${JSON.stringify(fixture.gameName)})`);
  const profileButton = (fixture, action) => {
    const selector = `button[aria-label="${action} ${fixture.profile.name}"]`;
    return page.evaluate(`document.querySelector(${JSON.stringify(selector)})?.checkVisibility() ?? false`);
  };
  const stop = async () => {
    if (!child) return;
    if (child.exitCode === null && child.signalCode === null) {
      const exited = once(child, 'exit');
      child.kill();
      await exited;
    }
    child = undefined;
  };
  const start = async fixture => {
    child = spawn(fixture.executable, {windowsHide:true, stdio:'ignore'});
    await once(child, 'spawn');
  };
  try {
    await navigate('Overview');
    const initial = (await request({command:'profile_status'})).profiles;
    assert.equal(initial.profileSession.state, 'stopped', 'Use a disposable Cockpit with no applied profile');
    modeBefore = initial.session.selectionMode;
    assert.equal((await request({command:'ui_preferences_get', key:'game-process-events'})).uiPreferences.value, false);
    for (const label of ['A', 'B']) {
      const installPath = resolve(directory, label);
      const executable = resolve(installPath, `AutomaticProfile${label}.exe`);
      await mkdir(installPath);
      await copyFile(binary, executable);
      const fixture = {executable, gameName:`Automatic fixture ${label}`};
      fixtures.push(fixture);
      const {games} = await request({command:'game_add', name:fixture.gameName, installPath, executable});
      fixture.gameId = games.addedGameId;
      assert.ok(fixture.gameId);
      const {profiles:{document}} = await request({command:'profile_create', name:`Automatic profile ${label}`});
      fixture.profile = document;
      document.eligibility = {gameIds:[fixture.gameId]};
      assert.deepEqual(document.resources ?? [], []);
      assert.deepEqual(document.desktopOutputs ?? [], []);
      assert.deepEqual(document.virtualOutputs ?? [], []);
      fixture.profile = (await request({command:'profile_editor_save', document, assignments:[],
        expectedRevision:document.revision})).profiles.document;
    }
    assert.notEqual(fixtures[0].gameId, fixtures[1].gameId);
    await request({command:'profile_selection_mode', mode:'automatic'});
    await page.evaluate(`(async () => {
      const {observeProfileRuntime} = await import('/src/lib/profile-read-models/shared.ts');
      const {observeProfileActivation} = await import('/src/lib/profile-activation-observation.ts');
      const trace = window.__automaticReopen = {completed:[]};
      const stopRuntime = observeProfileRuntime(state => {
        if (state.status !== 'ready') return;
        const {profileSession, automatic} = state.value;
        trace.current = {state:profileSession.state, owner:profileSession.owner, automatic:automatic.status,
          message:automatic.message, target:automatic.target};
      });
      const stopActivation = observeProfileActivation(progress => {
        if (progress?.phase === 'succeeded' && !trace.completed.some(item => item.activationId === progress.activationId)) {
          trace.completed.push({activationId:progress.activationId, profileId:progress.profileId, origin:progress.origin});
        }
      });
      trace.close = () => { stopRuntime(); stopActivation(); };
    })()`);
    await until(readRuntime, state => state?.state === 'stopped' && noMatch(state));
    await page.call('Network.enable', {maxPostDataSize:65536});
    stopTrace = page.listen(({method, params}) => {
      if (!tracking || method !== 'Network.requestWillBeSent' || !params.request.postData) return;
      if (!['/gateway_request', '/api/gateway/'].includes(new URL(params.request.url).pathname)) return;
      const payload = JSON.parse(params.request.postData);
      const command = (payload.request ?? payload).command;
      if (refreshCommands.has(command)) reads.push(command);
    });
    tracking = true;
    await request({command:'profile_status'});
    assert.deepEqual(reads.splice(0), ['profile_status'], 'The CDP trace must see the control read');
    let previousOwner;
    for (const fixture of fixtures) {
      await navigate('Overview');
      const started = performance.now();
      await start(fixture);
      const active = await selected(fixture);
      const selectionMs = performance.now() - started;
      assert.equal(active.owner.gameId, fixture.gameId);
      assert.equal(active.target?.profileId, fixture.profile.id);
      const progress = await completed('profileId', fixture.profile.id, 'automatic');
      if (previousOwner) assert.notDeepEqual(active.owner, previousOwner);
      await until(overview, value => value?.includes(fixture.profile.name));
      await until(() => detected(fixture));
      assert.equal(await profileButton(fixture, 'Deactivate'), false, 'Profiles must be closed during selection');
      await navigate('Profiles');
      await until(() => profileButton(fixture, 'Deactivate'));
      await navigate('Overview');
      await stop();
      await until(() => detected(fixture), value => !value);
      const waiting = await until(readRuntime, noMatch, 15_000);
      assert.deepEqual(waiting.owner, active.owner, 'Automatic selection retains the active profile when no game matches');
      await navigate('Profiles');
      await until(() => profileButton(fixture, 'Deactivate'));
      previousOwner = active.owner;
      evidence.boundaries.push({profile:fixture.profile.name, gameId:fixture.gameId, selectionMs,
        activationId:progress.activationId, retainedOnExit:true});
    }
    await navigate('Overview');
    const stopId = randomUUID();
    await stopExternally(stopId);
    await completed('activationId', stopId, 'deactivation');
    await until(readRuntime, state => state?.state === 'stopped' && !state.owner);
    await until(overview, value => value === 'No profile active');
    await navigate('Profiles');
    for (const fixture of fixtures) await until(() => profileButton(fixture, 'Activate'));
    assert.deepEqual(reads, [], 'Automatic selection and reopening must not query legacy profile status');
    evidence.stoppedOnReopen = true;
  } catch (error) {
    evidence.failure = String(error);
    evidence.current = await readRuntime().catch(() => null);
    throw error;
  } finally {
    tracking = false;
    stopTrace();
    const cleanup = async (label, action) => { try { await action(); } catch (error) { cleanupErrors.push(`${label}: ${error}`); } };
    await cleanup('fixture process', stop);
    await cleanup('runtime reader', () => page.evaluate('window.__automaticReopen?.close(); delete window.__automaticReopen'));
    if (modeBefore) {
      await cleanup('profile stop', () => request({command:'profile_stop'}));
      await cleanup('selection mode', () => request({command:'profile_selection_mode', mode:modeBefore}));
    }
    for (const fixture of fixtures.reverse()) {
      if (fixture.profile) await cleanup('profile removal', () => request({command:'profile_delete',
        id:fixture.profile.id, expectedRevision:fixture.profile.revision}));
      if (fixture.gameId) await cleanup('game removal', () => request({command:'game_remove', gameId:fixture.gameId}));
    }
    await cleanup('Cockpit page', () => page.close());
    await writeFile(resolve(directory, 'evidence.json'), JSON.stringify(evidence, null, 2) + '\n');
    console.log(JSON.stringify(evidence));
  }
  assert.deepEqual(cleanupErrors, []);
});
