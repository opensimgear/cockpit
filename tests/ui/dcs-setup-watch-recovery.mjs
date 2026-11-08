import assert from 'node:assert/strict';
import {readFile, rename, unlink, writeFile} from 'node:fs/promises';
import {resolve} from 'node:path';
import {setTimeout as delay} from 'node:timers/promises';

export async function verifyDcsSetupWatchRecovery(game, until, userFolder, biosTarget) {
  if (process.env.OSG_DCS_VEHICLE_CATALOGUE_WAKE === '1') await verifyDcsAliasCatalogue(game, until, biosTarget);
  if (process.env.OSG_DCS_SETUP_WATCH_RECOVERY !== '1') return;
  const before = (await game()).integrationSetup;
  assert.equal(before.state, 'ready');
  const blocker = resolve(userFolder, 'Mods');
  await writeFile(blocker, 'Fixture file blocks descendant watch registration.', {flag: 'wx'});
  let blocked;
  try {
    blocked = await until(async () => {
      const {integrationSetup: setup} = await game();
      return setup.message?.includes('Setup file notifications are unavailable') && setup;
    }, 'DCS setup readiness remained advertised without its file watch');
    assert.equal(blocked.state, 'needs-attention');
    assert.equal(blocked.verified, false);
    await delay(2200);
    assert.equal((await game()).integrationSetup.message, blocked.message);
  } finally {
    await unlink(blocker);
  }
  const recovered = await until(async () => {
    const {integrationSetup: setup} = await game();
    return setup.state === 'ready' && setup;
  }, 'DCS setup did not recover its baseline after watch registration succeeded');
  assert.equal(recovered.providerSetupRevision, before.providerSetupRevision);
  console.log(JSON.stringify({dcsSetupWatchRecovered:true}));
  return {before, blocked, recovered};
}

export async function verifyLiveDcsSetupWithdrawal({game, until, script, child, absent, passive, biosId, inspect, biosTarget}) {
  const catalogue = process.env.OSG_DCS_CATALOGUE_CONTENT_WAKE === '1'
    ? await verifyDcsCatalogueContent({game, until, inspect, biosId, biosTarget}) : undefined;
  if (process.env.OSG_DCS_VEHICLE_SCOPE_WAKE !== '1') return catalogue && {catalogue};
  const before = (await game()).integrationSetup;
  assert.equal(before.state, 'ready');
  const retired = script + '.fixture-held';
  await rename(script, retired);
  try {
    await until(async () => (await game()).integrationSetup.state !== 'ready',
      'Removing the managed exporter did not invalidate the live scope');
    await absent('flight.airspeed.true');
    await absent(biosId);
    assert.equal(child.exitCode, null, 'The process must stay live through setup withdrawal');
  } finally {
    await rename(retired, script);
  }
  const recovered = await until(async () => {
    const setup = (await game()).integrationSetup;
    return setup.state === 'ready' && setup;
  }, 'Restoring the managed exporter did not recover the live scope');
  assert.equal(recovered.providerSetupRevision, before.providerSetupRevision);
  await passive('flight.airspeed.true', 210.25);
  await passive(biosId, 1000);
  console.log(JSON.stringify({liveDcsSetupScopeWithdrawnAndRecovered: true}));
  return {withdrawn: true, recovered: true, unchangedRevision: true, catalogue};
}

async function verifyDcsCatalogueContent({game, until, inspect, biosId, biosTarget}) {
  const path = resolve(biosTarget, 'doc/json/CommonData.json');
  const original = await readFile(path);
  const document = JSON.parse(original);
  const control = Object.values(document).flatMap(Object.values).find(item => item.identifier === 'ALT_MSL_FT');
  assert.ok(control);
  const label = async () => (await inspect(biosId)).channels.find(row => row.id === biosId)?.label;
  const generation = async () => (await game()).integrationSetup.catalogueGeneration;
  const before = await generation();
  assert.notEqual(before, undefined, 'The private game must expose its catalogue generation');
  const originalLabel = await label();
  assert.ok(originalLabel);
  const expected = 'Authored DCS altitude catalogue change';
  control.description = expected;
  const expectedLabel = `CommonData reported · ${expected} (measurement validity unavailable)`;
  const started = performance.now();
  let changed, changeMs, last;
  try {
    await writeFile(path, JSON.stringify(document));
    changed = await until(async () => {
      const current = await generation();
      last = {generation:current, label:await label()};
      return current !== before && last.label === expectedLabel && current;
    }, 'The watched catalogue edit did not replace the telemetry descriptor');
    changeMs = performance.now() - started;
  } catch (error) {
    throw new Error(`Expected generation after ${before} and label "${expectedLabel}", last read ${JSON.stringify(last)}`, {cause:error});
  } finally {
    await writeFile(path, original);
  }
  const restored = await until(async () => {
    const current = await generation();
    return current !== changed && await label() === originalLabel && current;
  }, 'The watched catalogue restore did not replace the telemetry descriptor');
  return {before, changed, restored, changeMs};
}

async function verifyDcsAliasCatalogue(game, until, biosTarget) {
  const file = resolve(biosTarget, 'doc/json/AircraftAliases.json');
  const before = await readFile(file);
  const aliases = JSON.parse(before);
  const id = 'OSG-Authored-Catalogue-Aircraft';
  assert.ok(!Object.hasOwn(aliases, id));
  const contains = async () => {
    const state = await game();
    assert.equal(state.currentVehicle, null, 'Installed aliases must not publish a live aircraft');
    return state.vehicleSupport.vehicles.some(row => row.id === id);
  };
  assert.equal(await contains(), false);
  aliases[id] = ['A-10C'];
  try {
    await writeFile(file, JSON.stringify(aliases));
    await until(contains, 'The DCS alias creation did not reach the closed game vehicle list');
  } finally {
    await writeFile(file, before);
  }
  await until(async () => !(await contains()), 'The removed DCS alias remained in the closed game vehicle list');
  console.log(JSON.stringify({closedDcsAliasCatalogueCreationAndRemoval: true}));
}
