// Opt-in real-gateway acceptance for installed X-Plane aircraft catalogue
// updates. A private inert installation is observed through the Games detail
// stream; no simulator process or provider is started.
import assert from 'node:assert/strict';
import {mkdtemp, mkdir, rename, rm, writeFile} from 'node:fs/promises';
import {resolve, sep} from 'node:path';
import test from 'node:test';
import {browserPage, nativePage, until} from '../../scripts/reactive/cdp.mjs';

const browserUrl = process.env.OSG_XPLANE_VEHICLE_CATALOGUE_BROWSER_URL;
const enabled = browserUrl || process.env.OSG_XPLANE_VEHICLE_CATALOGUE_NATIVE === '1';
const gameId = 'x-plane-12';

test('the X-Plane Games detail stream follows nested aircraft catalogue changes', {
  skip: !enabled || process.platform !== 'win32', timeout: 60000,
}, async () => {
  const fixtures = resolve('.research/reactive/xplane-vehicle-catalogue');
  await mkdir(fixtures, {recursive: true});
  const installPath = await mkdtemp(resolve(fixtures, 'installation-'));
  const executable = resolve(installPath, 'X-Plane.exe');
  const aircraft = resolve(installPath, 'Aircraft/Authored');
  const first = resolve(aircraft, 'Training/First_Plane.acf');
  const second = resolve(aircraft, 'Experimental/Nested/Second_Plane.acf');
  const renamed = resolve(aircraft, 'Training/Renamed_Plane.acf');
  await mkdir(resolve(installPath, 'Resources/plugins'), {recursive: true});
  await writeFile(resolve(installPath, 'Resources/plugins/DataRefs.txt'), '2 1200 Authored catalogue fixture\n');
  await writeFile(executable, 'Harmless fixture metadata; never executed.');

  const page = browserUrl ? await browserPage(browserUrl) : await nativePage();
  const request = value => page.evaluate(
    `import('/src/lib/gateway.ts').then(({gatewayRequest})=>gatewayRequest(${JSON.stringify(value)}))`,
  );
  const game = () => page.evaluate('window.__xplaneVehicleCatalogue?.game');
  const normalizedPath = value => value?.replace(/^\\\\\?\\/, '').replaceAll('/', '\\').toLowerCase().replace(/\\+$/, '');
  const rows = value => (value?.vehicleSupport?.vehicles ?? [])
    .filter(vehicle => vehicle.id.startsWith('Aircraft/Authored/'))
    .map(({id, name}) => ({id, name}))
    .sort((a, b) => a.id.localeCompare(b.id));
  const expectedRows = (...values) => values.map(([id, name]) => ({id, name}))
    .sort((a, b) => a.id.localeCompare(b.id));
  const waitForRows = expected => until(game, value => value
    && normalizedPath(value.installPath) === normalizedPath(installPath)
    && JSON.stringify(rows(value)) === JSON.stringify(expected)).catch(cause => {
      throw new Error('The aircraft file change did not reach the Games detail stream', {cause});
    });
  let installed = false;
  let beforeGame;
  let fixtureInstallation;
  try {
    await until(() => page.evaluate(`!!document.querySelector('nav[aria-label="Main navigation"]')`));
    const initial = (await request({command: 'game_get_state'})).games;
    assert.ok(!initial.detectedGames?.some(value => value.gameId.startsWith('x-plane-')),
      'Close real X-Plane processes before running the private catalogue fixture.');
    beforeGame = initial.catalogue.find(value => value.id === gameId);
    const added = (await request({command: 'game_add', name: beforeGame?.name ?? 'X-Plane 12',
      installPath, executable})).games;
    installed = true;
    assert.equal(added.addedGameId, gameId, 'The authored installation must bind to X-Plane 12.');
    fixtureInstallation = added.catalogue.find(value => value.id === gameId)?.installations
      ?.find(value => normalizedPath(value.installPath) === normalizedPath(installPath));
    assert.ok(fixtureInstallation, 'Cockpit must register the private X-Plane installation.');
    await request({command: 'game_installation_select', gameId, installationId: fixtureInstallation.id});
    await page.evaluate(`import('/src/lib/game-read-models/shared.ts').then(({observeGameDetails})=>{
      const trace=window.__xplaneVehicleCatalogue={game:null,updates:0};
      trace.stop=observeGameDetails(${JSON.stringify(gameId)},state=>{
        if(state.status==='ready'){
          trace.game=structuredClone(state.value.value);
          trace.updates++;
        }
      });
    })`);
    const baseline = await until(game, value => normalizedPath(value?.installPath) === normalizedPath(installPath)
      && value.vehicleSupport?.supported === true
      && value.vehicleSupport?.kind === 'aircraft');
    assert.deepEqual(rows(baseline), []);
    assert.equal(baseline.currentVehicle, null);
    assert.equal(baseline.vehicleStatus.status, 'waiting');

    await mkdir(resolve(aircraft, 'Training'), {recursive: true});
    await mkdir(resolve(aircraft, 'Experimental/Nested'), {recursive: true});
    await writeFile(first, 'I\n1200 Version\nACF');
    const one = expectedRows(['Aircraft/Authored/Training/First_Plane.acf', 'First Plane']);
    const firstUpdate = await waitForRows(one);
    assert.deepEqual(rows(firstUpdate), one);
    assert.equal(firstUpdate.currentVehicle, null, 'Installed catalogue discovery must not claim live aircraft.');

    await writeFile(second, 'I\n1200 Version\nACF');
    const two = expectedRows(
      ['Aircraft/Authored/Training/First_Plane.acf', 'First Plane'],
      ['Aircraft/Authored/Experimental/Nested/Second_Plane.acf', 'Second Plane'],
    );
    assert.deepEqual(rows(await waitForRows(two)), two, 'Nested .acf creation must publish the exact relative IDs.');

    await rename(first, renamed);
    const afterRename = expectedRows(
      ['Aircraft/Authored/Training/Renamed_Plane.acf', 'Renamed Plane'],
      ['Aircraft/Authored/Experimental/Nested/Second_Plane.acf', 'Second Plane'],
    );
    assert.deepEqual(rows(await waitForRows(afterRename)), afterRename,
      'A rename must withdraw the old ID and publish the new relative ID.');

    await rm(second);
    const afterRemoval = expectedRows(['Aircraft/Authored/Training/Renamed_Plane.acf', 'Renamed Plane']);
    const final = await waitForRows(afterRemoval);
    assert.deepEqual(rows(final), afterRemoval, 'Removing a nested .acf must withdraw its catalogue row.');
    assert.equal(final.currentVehicle, null);
    assert.equal(final.vehicleStatus.status, 'waiting');

    const root = resolve(installPath, 'Aircraft');
    const held = resolve(installPath, 'Held-Aircraft');
    await rename(root, held);
    try {
      assert.deepEqual(rows(await waitForRows([])), [], 'Removing the selected Aircraft tree must withdraw its rows.');
    } finally {
      await rename(held, root);
    }
    assert.deepEqual(rows(await waitForRows(afterRemoval)), afterRemoval, 'Restoring the tree must rearm its watcher.');
    console.log(JSON.stringify({
      transport: browserUrl ? 'browser-dev' : 'native',
      gameId,
      installation: installPath,
      detailUpdates: await page.evaluate('window.__xplaneVehicleCatalogue.updates'),
      checks: ['nested creation', 'rename identity replacement', 'nested removal', 'root removal and rearm', 'closed process stays unselected'],
    }));
  } finally {
    try {
      await page.evaluate('window.__xplaneVehicleCatalogue?.stop(); delete window.__xplaneVehicleCatalogue');
      if (installed && fixtureInstallation && !beforeGame) {
        const state = (await request({command: 'game_get_state'})).games.catalogue.find(value => value.id === gameId);
        assert.deepEqual(state?.installations?.map(item => item.id), [fixtureInstallation.id]);
        await request({command: 'game_remove', gameId});
        assert.ok(installPath.startsWith(fixtures + sep), 'Fixture cleanup must stay inside its private directory.');
        await rm(installPath, {recursive: true, force: true, maxRetries: 8, retryDelay: 100});
      } else if (installed && fixtureInstallation) {
        if (beforeGame.selectedInstallationId) {
          await request({command: 'game_installation_select', gameId,
            installationId: beforeGame.selectedInstallationId});
        }
        assert.ok(installPath.startsWith(fixtures + sep), 'Fixture cleanup must stay inside its private directory.');
        await rm(installPath, {recursive: true, force: true});
        await until(async () => (await request({command: 'game_get_state'})).games.discovery?.status !== 'scanning',
          Boolean, 90000);
        const scan = async paths => {
          await request({command: 'game_discovery_start', paths});
          return until(async () => {
            const value = (await request({command: 'game_get_state'})).games.discovery;
            return value?.status === 'ready' ? value : false;
          }, Boolean, 90000);
        };
        let discovery = await scan([]);
        if (!discovery.libraries.some(value => value.id === 'disk')) {
          discovery = await scan([fixtures]);
        }
        const removal = discovery.entries.find(value => value.action === 'remove'
          && value.gameId === gameId && value.installationId === fixtureInstallation.id);
        assert.ok(removal, 'Discovery must offer removal of exactly the deleted private installation.');
        assert.equal(normalizedPath(removal.installPath), normalizedPath(installPath));
        await request({command: 'game_discovery_apply', scanId: discovery.scanId,
          actions: discovery.entries.map(value => ({id: value.id, selected: value.id === removal.id}))});
        const afterCleanup = (await request({command: 'game_get_state'})).games.catalogue.find(value => value.id === gameId);
        if (beforeGame) {
          assert.equal(afterCleanup?.selectedInstallationId, beforeGame.selectedInstallationId);
          assert.deepEqual(afterCleanup?.installations, beforeGame.installations);
        } else {
          assert.equal(afterCleanup, undefined);
        }
      }
    } finally {
      await page.close();
      if (!installed || !fixtureInstallation) {
        assert.ok(installPath.startsWith(fixtures + sep), 'Fixture cleanup must stay inside its private directory.');
        await rm(installPath, {recursive: true, force: true});
      }
    }
  }
});
