import assert from 'node:assert/strict';
import {mkdir, mkdtemp, writeFile} from 'node:fs/promises';
import {resolve} from 'node:path';
import test from 'node:test';
import {setTimeout as delay} from 'node:timers/promises';
import {browserPage, nativePage, until} from '../../scripts/reactive/cdp.mjs';

const browserUrl = process.env.OSG_GAME_RULE_BROWSER_URL;
const enabled = process.platform === 'win32' && (browserUrl || process.env.OSG_GAME_RULE_NATIVE === '1');
const rule = `document.querySelector('[aria-label="Matching profiles"] select')`;

test('matching profiles reads the current rule after document and route reopen', {
  skip: !enabled, timeout: 120_000,
}, async () => {
  const root = resolve('.research/game-rule-reopen');
  await mkdir(root, {recursive:true});
  const installPath = await mkdtemp(resolve(root, 'installation-'));
  const executable = resolve(installPath, 'Fixture.exe');
  await writeFile(executable, 'Inert file for a game that is never launched.');
  const page = browserUrl ? await browserPage(browserUrl) : await nativePage();
  const request = value => page.evaluate(
    `import('/src/lib/gateway.ts').then(({gatewayRequest}) => gatewayRequest(${JSON.stringify(value)}))`,
  );
  const setHidden = hidden => page.evaluate(`(() => {
    if (${hidden}) Object.defineProperty(document, 'hidden', {configurable:true, value:true});
    else delete document.hidden;
    document.dispatchEvent(new Event('visibilitychange'));
  })()`);
  const readRule = () => page.evaluate(`${rule}?.disabled === false ? ${rule}.value : null`);
  const gatewayPath = browserUrl ? '/api/gateway/' : '/gateway_request';
  let gameId, candidateReads = 0, heldRead, traceError;
  const profiles = [];
  const stopTrace = page.listen(({method, params}) => {
    if (!params.request || new URL(params.request.url).pathname !== gatewayPath) return;
    const body = params.request.postData ? JSON.parse(params.request.postData) : {};
    const command = body.request ?? body;
    const candidate = command.command === 'profile_game_candidates' && command.gameId === gameId;
    if (method === 'Network.requestWillBeSent' && candidate) candidateReads++;
    if (method === 'Fetch.requestPaused') {
      if (candidate && !heldRead) heldRead = params.requestId;
      else void page.call('Fetch.continueRequest', {requestId:params.requestId}).catch(error => { traceError = error; });
    }
  });
  try {
    await until(() => page.evaluate(`!!document.querySelector('nav[aria-label="Main navigation"]')`));
    ({games:{addedGameId:gameId}} = await request({command:'game_add', name:`Rule reopen ${Date.now()}`, installPath, executable}));
    assert.ok(gameId);
    for (const label of ['First', 'Second']) {
      const {profiles:{document}} = await request({command:'profile_create', name:`${label} ${gameId}`});
      profiles.push(document);
      const {profiles:{assignments}} = await request({command:'profile_assignments_get', id:document.id});
      const saved = await request({command:'profile_editor_save', document:{...document, eligibility:{gameIds:[gameId]}},
        assignments, expectedRevision:document.revision});
      profiles[profiles.length - 1] = saved.profiles.document;
    }
    const [first, second] = profiles;
    const prefer = profile => request({command:'profile_game_preference_save', gameId, profileId:profile.id});
    const waitForCommit = profile => page.evaluate(`(async () => {
      const {observeProfileRuntime} = await import('/src/lib/profile-read-models/shared.ts');
      let stop;
      await new Promise(resolve => {
        stop = observeProfileRuntime(state => {
          if (state.status === 'ready' && state.value.preferredProfiles[${JSON.stringify(gameId)}] === ${JSON.stringify(profile.id)}) resolve();
        });
      });
      stop();
    })()`);
    const open = () => page.evaluate(`window.dispatchEvent(new CustomEvent('osg:open-game-settings', {
      detail:{gameId:${JSON.stringify(gameId)}}
    }))`);
    await page.call('Network.enable', {maxPostDataSize:65536});
    await prefer(first);
    await open();
    await until(readRule, value => value === first.id);
    await prefer(second);
    await until(readRule, value => value === second.id);

    const beforeHide = candidateReads;
    await setHidden(true);
    await prefer(first);
    await waitForCommit(first);
    await delay(250);
    assert.equal(candidateReads, beforeHide, 'A rule commit while hidden must not fetch matching profiles');
    assert.equal(await readRule(), null);
    await page.call('Fetch.enable', {patterns:[{urlPattern:'*' + gatewayPath}]});
    try {
      await setHidden(false);
      await until(() => {
        if (traceError) throw traceError;
        return heldRead;
      });
      assert.equal(await readRule(), null, 'The old choice must remain disabled until the new snapshot arrives');
      await page.call('Fetch.continueRequest', {requestId:heldRead});
      await until(readRule, value => value === first.id);
    } finally {
      await page.call('Fetch.disable');
    }
    assert.equal(candidateReads, beforeHide + 1);

    await page.evaluate(`document.querySelector('nav[aria-label="Main navigation"] button[title="Overview"]').click()`);
    await until(() => page.evaluate(`${rule} === null`));
    const beforeClose = candidateReads;
    await prefer(second);
    await waitForCommit(second);
    assert.equal(candidateReads, beforeClose);
    await open();
    await until(readRule, value => value === second.id);
    assert.equal(candidateReads, beforeClose + 1);
    console.log(JSON.stringify({transport:browserUrl ? 'browser-dev' : 'native', visibleCommit:true,
      hiddenReads:0, revealReads:1, routeReopenReads:1}));
  } finally {
    stopTrace();
    try {
      await setHidden(false);
      for (const profile of profiles) await request({command:'profile_delete', id:profile.id, expectedRevision:profile.revision});
      if (gameId) await request({command:'game_remove', gameId});
    } finally {
      await page.close();
    }
  }
});
