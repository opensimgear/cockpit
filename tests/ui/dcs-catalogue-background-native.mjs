// From cockpit/, launch a disposable dev instance with OSG_SQLITE_DATABASE and
// OSG_DCS_BIOS_CONFIG set to absolute paths under .research/dcs-catalogue-background/.
// Desktop: set WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS=--remote-debugging-port=9222,
// run pnpm dev, and set OSG_DCS_CATALOGUE_BACKGROUND_NATIVE=1 in the test shell.
// Browser: run pnpm web:dev with a current OSG_GATEWAY_SERVICE and set
// OSG_DCS_CATALOGUE_BACKGROUND_BROWSER_URL=http://127.0.0.1:5173/.
// Set OSG_SQLITE_DATABASE to the same path in the test shell, then run
// node --test tests/ui/dcs-catalogue-background-native.mjs
import assert from 'node:assert/strict';
import {randomUUID} from 'node:crypto';
import {copyFile, mkdir, mkdtemp, readFile, realpath, rm, writeFile} from 'node:fs/promises';
import {isAbsolute, join, relative, resolve, sep} from 'node:path';
import {fileURLToPath} from 'node:url';
import test from 'node:test';
import {browserPage, nativePage, until} from '../../scripts/reactive/cdp.mjs';

const browserUrl = process.env.OSG_DCS_CATALOGUE_BACKGROUND_BROWSER_URL;
const transport = browserUrl ? 'browser-dev' : 'native';
const isolatedRoot = fileURLToPath(new URL('../../.research/dcs-catalogue-background/', import.meta.url));
const fixtures = fileURLToPath(new URL('../fixtures/dcs-bios/', import.meta.url));
const aircraft = 'A-10C_2';
const originalModule = 'A-10C';
const aliasModule = 'FixtureJet';
const aircraftBlock = 'fixture-aircraft';

function assertPrivatePath(root, path) {
  const child = relative(root, path);
  assert.ok(child && child !== '..' && !child.startsWith(`..${sep}`) && !isAbsolute(child),
    'Keep acceptance test paths under .research/dcs-catalogue-background/.');
}

async function verifyDatabase(request, expectedDatabase) {
  const databasePath = await realpath(expectedDatabase);
  assertPrivatePath(await realpath(isolatedRoot), databasePath);
  const {DatabaseSync} = await import('node:sqlite');
  const database = new DatabaseSync(databasePath, {readOnly: true, timeout: 5_000});
  try {
    const {epoch} = database.prepare('SELECT epoch FROM incarnation WHERE singleton = 1').get();
    const {storage} = await request({command: 'storage_status'});
    assert.equal(storage.epoch, epoch, 'Refuse to edit an unexpected Cockpit database.');
  } finally {
    database.close();
  }
}

const writeJson = (folder, name, value) => writeFile(join(folder, `${name}.json`), JSON.stringify(value));

function moduleWithControls(original, ...identifiers) {
  const document = structuredClone(original);
  for (const identifier of identifiers) {
    document.UFC[identifier] = {...document.UFC.UFC_MASTER_CAUTION, identifier, description: identifier, outputs: []};
  }
  return document;
}

async function prepareCatalogue(folder, module) {
  await mkdir(folder);
  await Promise.all([
    ...['AircraftAliases.json', 'MetadataStart.json', 'MetadataEnd.json'].map(name =>
      copyFile(join(fixtures, name), join(folder, name))),
    copyFile(join(fixtures, 'CommonData-altitude.json'), join(folder, 'CommonData.json')),
    writeJson(folder, originalModule, module),
  ]);
}

async function seedProfile(request, name, sourceId) {
  const {profiles: {document}} = await request({command: 'profile_create', name});
  document.eligibility.gameIds = ['dcs'];
  document.resources.push(
    {id: 'fixture-base', name: 'Fixture Base', kind: 'game', required: false, ports: []},
    {id: aircraftBlock, name: 'Authored aircraft integration', kind: 'game', required: false, ports: []},
  );
  document.vehicleTabs = [{id: 'vehicle-1', gameId: 'dcs', vehicleId: aircraft, vehicleName: 'Fixture aircraft', nodeIds: [aircraftBlock]}];
  document.editor.positions.push({nodeId: 'fixture-base', x: 24, y: 30}, {nodeId: aircraftBlock, x: 336, y: 94});
  return (await request({
    command: 'profile_editor_save', document, expectedRevision: document.revision,
    assignments: [{resourceId: aircraftBlock, sourceId, ports: []}],
  })).profiles.document;
}

test(`${transport} refreshes an open DCS aircraft integration after background catalogue changes`, {
  skip: !browserUrl && process.env.OSG_DCS_CATALOGUE_BACKGROUND_NATIVE !== '1',
  timeout: 120_000,
}, async () => {
  const expectedDatabase = process.env.OSG_SQLITE_DATABASE;
  assert.ok(expectedDatabase && isAbsolute(expectedDatabase), 'Set OSG_SQLITE_DATABASE to the isolated dev database.');
  assertPrivatePath(isolatedRoot, resolve(expectedDatabase));
  const page = browserUrl ? await browserPage(browserUrl) : await nativePage();
  const request = async value => {
    if (browserUrl) return page.evaluate(
      `import(${JSON.stringify(new URL('/src/lib/gateway.ts', browserUrl).href)}).then(({gatewayRequest}) => gatewayRequest(${JSON.stringify(value)}))`,
    );
    const reply = await page.evaluate(
      `window.__TAURI_INTERNALS__.invoke('gateway_request', {request:${JSON.stringify({...value, clientId: 'dcs-catalogue-fixture'})}})`,
    );
    assert.equal(reply.ok, true, `${value.command}: ${reply.error}`);
    return reply.state;
  };
  const setup = async () => (await request({command: 'game_get_state'})).games.catalogue
    .find(game => game.id === 'dcs').integrationSetup;
  const savePath = async (userFolder, cataloguePath, expectedSetupRevision) => {
    await request({
      command: 'game_integration_settings_save', gameId: 'dcs', userFolder, expectedSetupRevision,
      connection: {cataloguePath, freshnessMs: 1500, commandHost: '127.0.0.1', commandPort: 7778,
        listenAddress: '0.0.0.0', listenPort: 5010, multicastGroup: '239.255.50.10', multicastInterface: '0.0.0.0'},
    });
  };
  const click = selector => until(() => page.evaluate(`(() => {
    const button = document.querySelector(${JSON.stringify(selector)});
    if (!button || button.disabled) return false;
    button.click();
    return true;
  })()`));
  const titleButton = '.profiles-workspace button[aria-label="Edit profile name"]';
  const readCanvas = () => page.evaluate(`(() => {
    const node = document.querySelector('.profiles-workspace [data-node-id="${aircraftBlock}"]');
    if (!node) return null;
    const label = node.querySelector('[data-connection-target]')?.getAttribute('aria-label') ?? '';
    return {
      inputs: Number(label.match(/\\d+ \\/ (\\d+)\\)$/)?.[1] ?? -1),
      name: node.querySelector('.block-heading strong')?.textContent,
      position: [node.style.left, node.style.top],
      discovering: node.textContent.includes('Discovering inputs and outputs'),
      error: node.querySelector('.block-discovery-error')?.textContent ?? '',
      profileName: document.querySelector(${JSON.stringify(`${titleButton} span`)})?.textContent,
      origin: performance.timeOrigin,
    };
  })()`);
  async function waitForInputs(expected, step) {
    let last;
    try {
      return await until(async () => (last = await readCanvas()), value =>
        value && !value.discovering && !value.error && value.inputs === expected, 20_000);
    } catch (error) {
      throw new Error(`${step}: expected ${expected} aircraft inputs, last canvas state ${JSON.stringify(last)}`, {cause: error});
    }
  }

  let scratch;
  try {
    await until(() => page.evaluate(`!!document.querySelector('nav[aria-label="Main navigation"]')`));
    await verifyDatabase(request, expectedDatabase);
    scratch = await mkdtemp(join(isolatedRoot, 'fixtures-'));
    const userFolder = join(scratch, 'Saved Games');
    const gameRoot = join(scratch, 'DCS');
    const executable = join(gameRoot, 'bin', 'DCS.exe');
    const first = join(scratch, 'first'), second = join(scratch, 'second');
    const module = JSON.parse(await readFile(join(fixtures, `${originalModule}.json`), 'utf8'));
    const aliases = JSON.parse(await readFile(join(fixtures, 'AircraftAliases.json'), 'utf8'));
    await mkdir(userFolder);
    await mkdir(join(gameRoot, 'bin'), {recursive: true});
    await writeFile(executable, 'Catalogue test installation. No game process is started.');
    await request({command: 'game_add', name: 'DCS catalogue fixture', installPath: gameRoot, executable});
    await prepareCatalogue(first, module);
    await prepareCatalogue(second, moduleWithControls(module, 'PATH_CONTROL'));
    await writeJson(second, aliasModule, moduleWithControls(module, 'ALIAS_ONE', 'ALIAS_TWO', 'ALIAS_THREE'));

    await savePath(userFolder, first, (await setup()).providerSetupRevision);
    const firstSetup = await setup();
    const {profileResources: {resource}} = await request({command: 'profile_dcs_vehicle_resource', vehicleId: aircraft});
    assert.equal(resource.attributes.aircraftModule, originalModule);
    const profileName = `DCS background ${randomUUID()}`;
    const profile = await seedProfile(request, profileName, resource.id);
    const storedProfile = async () => {
      const {profiles: {document, assignments}} = await request({command: 'profile_assignments_get', id: profile.id});
      return {document, assignments};
    };
    const before = await storedProfile();
    await click('nav[aria-label="Main navigation"] button[title="Profiles"]');
    await click(`.profiles-workspace button[aria-label="Edit ${profileName}"]`);
    const draftName = `${profileName} draft`;
    await click(titleButton);
    await until(() => page.evaluate(`(() => {
      const input = document.querySelector('[aria-label="Profile name"]');
      if (!input) return false;
      input.value = ${JSON.stringify(draftName)};
      input.dispatchEvent(new Event('input', {bubbles: true}));
      input.dispatchEvent(new KeyboardEvent('keydown', {key: 'Enter', bubbles: true}));
      return true;
    })()`));
    const original = await waitForInputs(1, 'Initial fixture');
    assert.equal(original.profileName, draftName);
    assert.equal(original.name, 'Authored aircraft integration');
    assert.deepEqual(original.position, ['336px', '94px']);

    // After each change, read only the DOM until the active editor has updated.
    await savePath(userFolder, second, firstSetup.providerSetupRevision);
    const changedPath = await waitForInputs(2, 'Changed provider path');
    const secondSetup = await setup();
    assert.notEqual(secondSetup.providerSetupRevision, firstSetup.providerSetupRevision);

    await writeJson(second, originalModule, moduleWithControls(module, 'PATH_CONTROL', 'EDIT_CONTROL'));
    const changedModule = await waitForInputs(3, 'Edited active module file');
    const editedSetup = await setup();
    assert.equal(editedSetup.providerSetupRevision, secondSetup.providerSetupRevision);
    assert.notEqual(editedSetup.catalogueGeneration, secondSetup.catalogueGeneration);

    await writeJson(second, 'AircraftAliases', {...aliases, [aircraft]: [aliasModule]});
    const changedAlias = await waitForInputs(4, 'Remapped aircraft alias');
    const remappedSetup = await setup();
    assert.equal(remappedSetup.providerSetupRevision, secondSetup.providerSetupRevision);
    assert.notEqual(remappedSetup.catalogueGeneration, editedSetup.catalogueGeneration);
    for (const current of [changedPath, changedModule, changedAlias]) {
      assert.equal(current.origin, original.origin);
      assert.equal(current.profileName, draftName, 'Keep the open editor’s unsaved change.');
      assert.equal(current.name, original.name);
      assert.deepEqual(current.position, original.position);
    }
    await click('.profiles-workspace button.header-back');
    await click('dialog[open][aria-labelledby="leave-profile-title"] button:nth-of-type(2)');
    const editButton = `.profiles-workspace button[aria-label="Edit ${profileName}"]`;
    await until(() => page.evaluate(`!!document.querySelector(${JSON.stringify(editButton)})`));
    await writeJson(second, aliasModule, moduleWithControls(module, 'ALIAS_ONE', 'ALIAS_TWO', 'ALIAS_THREE', 'REOPEN_CONTROL'));
    await until(setup, value => value.catalogueGeneration !== remappedSetup.catalogueGeneration);
    await click(editButton);
    const reopened = await waitForInputs(5, 'Reopened aircraft editor');
    assert.equal(reopened.profileName, profileName);
    assert.equal(reopened.origin, original.origin);
    assert.deepEqual(reopened.position, original.position);
    assert.deepEqual(await storedProfile(), before, 'Catalogue refresh must preserve the saved profile and assignments.');
  } finally {
    try {
      await page.close();
    } finally {
      if (scratch) {
        const resolvedScratch = await realpath(scratch);
        assertPrivatePath(await realpath(isolatedRoot), resolvedScratch);
        await rm(resolvedScratch, {recursive: true, force: true});
      }
    }
  }
});
