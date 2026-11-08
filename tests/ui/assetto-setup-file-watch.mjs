import assert from 'node:assert/strict';
import {mkdtemp, mkdir, rename, unlink, writeFile} from 'node:fs/promises';
import {dirname, resolve} from 'node:path';
import test from 'node:test';
import {browserPage, nativePage, until} from '../../scripts/reactive/cdp.mjs';

const browserUrl = process.env.OSG_ASSETTO_FILE_WATCH_BROWSER_URL;
const enabled = browserUrl || process.env.OSG_ASSETTO_FILE_WATCH_NATIVE === '1';
const gameId = 'assetto-corsa-rally';
const unverified = 'needs its first native verification';
const changed = 'executable/build changed';
const unavailable = 'Setup file notifications are unavailable';

test('the Assetto setup reader follows sibling runtime creation, replacement and removal', {
  skip: !enabled || process.platform !== 'win32', timeout: 60000,
}, async () => {
  const fixtures = resolve('.research/reactive/assetto-setup-file-watch');
  await mkdir(fixtures, {recursive: true});
  const installPath = await mkdtemp(resolve(fixtures, 'installation-'));
  const executable = resolve(installPath, 'acr.exe');
  const runtime = resolve(installPath, 'acr/Binaries/Win64/acr.exe');
  const blocker = resolve(installPath, 'acr');
  const replacement = resolve(installPath, 'new.exe');
  await writeFile(executable, 'Fixture launcher. Never executed.');
  await writeFile(replacement, 'Different fixture build. Never executed.');

  const page = browserUrl ? await browserPage(browserUrl) : await nativePage();
  const request = value => page.evaluate(
    `import('/src/lib/gateway.ts').then(({gatewayRequest})=>gatewayRequest(${JSON.stringify(value)}))`,
  );
  const gatewayPath = browserUrl ? '/api/gateway/' : '/gateway_request';
  const stateQueries = [];
  const stopTrace = page.listen(({method, params}) => {
    if (method !== 'Network.requestWillBeSent' || !params.request.postData
      || new URL(params.request.url).pathname !== gatewayPath) return;
    const body = JSON.parse(params.request.postData);
    const command = (body.request ?? body).command;
    if (command === 'game_get_state' || command === 'game_integration_check') stateQueries.push(command);
  });
  const setup = () => page.evaluate('window.__assettoSetupWatch?.status');
  const waitFor = async text => {
    try {
      return await until(setup, status => status?.message?.includes(text));
    } catch (cause) {
      throw new Error(`Expected ${text}, got ${JSON.stringify(await setup())}`, {cause});
    }
  };
  const writeLauncherCopy = () => writeFile(runtime, 'Fixture launcher. Never executed.');
  const timings = [];
  let registered = false, configured = false;
  try {
    await until(() => page.evaluate(`!!document.querySelector('nav[aria-label="Main navigation"]')`));
    const {games} = await request({command:'game_add', name:'Assetto Corsa Rally', executable, installPath});
    assert.equal(games.addedGameId, gameId);
    registered = true;
    await page.call('Network.enable', {maxPostDataSize: 65536});
    await page.evaluate(`import('/src/lib/game-read-models/shared.ts').then(({observeGameDetails})=>{
      const trace=window.__assettoSetupWatch={};
      trace.stop=observeGameDetails(${JSON.stringify(gameId)}, model => {
        if (model.status==='ready') trace.status=model.value.value.integrationSetup;
      });
    })`);
    await until(setup, status => status?.state === 'setup-required');
    await request({command:'game_integration_setup', gameId});
    configured = true;
    const baseline = await waitFor('configured for native shared memory');
    assert.equal(baseline.verified, false);

    await writeFile(blocker, 'A regular file prevents native registration below this path.');
    const blocked = await waitFor(unavailable);
    assert.equal(blocked.state, 'needs-attention');
    assert.equal(blocked.verified, false);
    // Leave the blocker in place across a failed registration retry.
    await new Promise(resolve => setTimeout(resolve, 2200));
    assert.deepEqual(await setup(), blocked);
    await unlink(blocker);
    const recovered = await waitFor(unverified);
    assert.equal(recovered.providerSetupRevision, baseline.providerSetupRevision);

    const publish = async (step, operation, text) => {
      const start = performance.now();
      await operation();
      const status = await waitFor(text);
      assert.equal(status.verified, false);
      timings.push({step, ms: performance.now() - start});
    };
    await publish('nested runtime appears', async () => {
      await mkdir(dirname(runtime), {recursive: true});
      await rename(replacement, runtime);
    }, changed);
    await publish('nested runtime removed', () => rename(runtime, replacement), unverified);
    // The fingerprint includes the selected runtime path, even when its bytes match the launcher.
    await publish('identical bytes at a different runtime path', writeLauncherCopy, changed);
    await publish('launcher restored as selected runtime', () => rename(runtime, resolve(installPath, 'retired.exe')), unverified);
    assert.ok(timings.reduce((total, {ms}) => total + ms, 0) < 4000,
      'Four sequential changes cannot come from the two-second fallback');
    assert.deepEqual(stateQueries, []);
    console.log(JSON.stringify({transport: browserUrl ? 'browser-dev' : 'native', watchRecovered: true, timings, stateQueries}));
  } finally {
    stopTrace();
    try {
      await page.evaluate('window.__assettoSetupWatch?.stop()');
      if (configured) await request({command:'game_integration_remove', gameId});
      if (registered) await request({command:'game_remove', gameId});
    } finally {
      await page.close();
    }
  }
});
