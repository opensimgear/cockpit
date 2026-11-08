// Opt-in check against a disposable running Cockpit database on Windows.
import assert from 'node:assert/strict';
import {mkdir, mkdtemp, rmdir, unlink, writeFile} from 'node:fs/promises';
import {join, resolve} from 'node:path';
import test from 'node:test';
import {browserPage, nativePage, until} from '../../scripts/reactive/cdp.mjs';

const url = process.env.OSG_DISCOVERY_REOPEN_BROWSER_URL;
const enabled = url || process.env.OSG_DISCOVERY_REOPEN_NATIVE === '1';
const gateway = url ? '/api/gateway/' : '/gateway_request';

test('Games opens the discovery review committed by a background scan', {
  skip: !enabled || process.platform !== 'win32', timeout: 180_000,
}, async () => {
  const page = url ? await browserPage(url) : await nativePage();
  const parent = resolve('.research/game-discovery-reopen');
  await mkdir(parent, {recursive:true});
  const root = await mkdtemp(join(parent, 'review-'));
  const fixtures = ['FirstReview', 'SecondReview'].map(name => {
    const scanRoot = join(root, name), installation = join(scanRoot, name);
    return {scanRoot, installation, executable:join(installation, name + '.exe'), name};
  });
  const request = payload => page.evaluate(`import('/src/lib/gateway.ts')
    .then(({gatewayRequest}) => gatewayRequest(${JSON.stringify(payload)}))`);
  const click = (selector, label) => until(() => page.evaluate(`(() => {
    const button = [...document.querySelectorAll(${JSON.stringify(selector)})]
      .find(node => node.checkVisibility() && !node.disabled
        && (node.title === ${JSON.stringify(label)} || node.textContent.trim() === ${JSON.stringify(label)}));
    if (!button) return false;
    button.click();
    return true;
  })()`));
  const navigate = label => click('nav[aria-label="Main navigation"] button', label);
  const current = () => page.evaluate('window.__discoveryReopen?.value');
  const ready = scanId => until(current, review => review?.status === 'ready' && review.scanId === scanId, 75_000);
  const scan = async fixture => {
    const {games} = await request({command:'game_discovery_start', paths:[fixture.scanRoot]});
    const review = await ready(games.discovery.scanId);
    assert.equal(review.firstStart, false);
    const entry = review.entries.find(row => row.executable?.toLowerCase().endsWith('\\' + fixture.name.toLowerCase() + '.exe'));
    assert.ok(entry, `No discovery entry for ${fixture.name}`);
    return entry;
  };
  const openReview = async entry => {
    await click('.initial-discovery button', 'View results');
    await until(() => page.evaluate(`[...document.querySelectorAll('.discovery-entry')].some(
      entry => entry.dataset.entryId === ${JSON.stringify(entry.id)})`));
  };
  const commands = [];
  const stopTrace = page.listen(({method,params}) => {
    if (method !== 'Network.requestWillBeSent' || !params.request.postData
      || new URL(params.request.url).pathname !== gateway) return;
    const body = JSON.parse(params.request.postData);
    commands.push((body.request ?? body).command);
  });
  try {
    for (const fixture of fixtures) {
      await mkdir(fixture.installation, {recursive:true});
      await writeFile(fixture.executable, 'Discovery fixture. Never launched.');
    }
    await navigate('Overview');
    await page.evaluate(`(async () => {
      const {observeGameDiscovery} = await import('/src/lib/game-read-models/shared.ts');
      window.__stopDiscoveryReopen = observeGameDiscovery(state => window.__discoveryReopen = state);
    })()`);
    const initial = await until(current, Boolean);
    const initialId = initial.scanId || (await request({command:'game_discovery_start'})).games.discovery.scanId;
    const firstStart = await ready(initialId);
    assert.equal(firstStart.firstStart, true, 'Use a fresh disposable database');
    await request({command:'game_discovery_apply', scanId:initialId,
      actions:firstStart.entries.map(({id}) => ({id,selected:false}))});
    await until(current, review => review?.status === 'applied');
    await page.call('Network.enable', {maxPostDataSize:65536});

    await navigate('Games');
    const first = await scan(fixtures[0]);
    await openReview(first);
    await navigate('Overview');
    const second = await scan(fixtures[1]);
    assert.notEqual(first.id, second.id);
    await navigate('Games');
    await openReview(second);
    assert.equal(await page.evaluate(`[...document.querySelectorAll('.discovery-entry')].some(
      entry => entry.dataset.entryId === ${JSON.stringify(first.id)})`), false);
    assert.equal(commands.filter(command => command === 'game_discovery_start').length, 2);
    assert.ok(!commands.includes('game_get_state'));
    console.log(JSON.stringify({transport:url ? 'browser-dev' : 'native', visibleReview:true,
      reopenedCurrentReview:true, retainedOldEntry:false, backgroundScans:2, refreshReads:0}));
  } finally {
    stopTrace();
    try {
      await page.evaluate('window.__stopDiscoveryReopen?.()').catch(() => {});
      await page.close();
    } finally {
      for (const fixture of fixtures) {
        await unlink(fixture.executable);
        await rmdir(fixture.installation);
        await rmdir(fixture.scanRoot);
      }
      await rmdir(root);
    }
  }
});
