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

export async function verifyLiveDcsSetupWithdrawal({game, until, script, child, absent, passive, biosId, inspect, biosTarget, setTraffic}) {
  const exactCatalogue = process.env.OSG_DCS_EXACT_CATALOGUE_WATCH === '1'
    ? await verifyExactDcsCatalogueWatch({game, until, inspect, passive, biosTarget, setTraffic}) : undefined;
  const catalogue = process.env.OSG_DCS_CATALOGUE_CONTENT_WAKE === '1'
    ? await verifyDcsCatalogueContent({game, until, inspect, biosId, biosTarget}) : undefined;
  if (process.env.OSG_DCS_VEHICLE_SCOPE_WAKE !== '1') return (catalogue || exactCatalogue) && {catalogue, exactCatalogue};
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
  return {withdrawn: true, recovered: true, unchangedRevision: true, catalogue, exactCatalogue};
}

async function verifyExactDcsCatalogueWatch({game, until, inspect, passive, biosTarget, setTraffic}) {
  const channel = 'AAP_CDUPWR:0';
  const root = resolve(biosTarget, 'doc/json');
  const unrelated = resolve(root, 'F-16C_50.json');
  const selected = resolve(root, 'A-10C.json');
  const otherBytes = await readFile(unrelated);
  const selectedBytes = await readFile(selected);
  const assertWithdrawn = async () => {
    const row = (await inspect(channel)).channels.find(row => row.id === channel);
    assert.ok(!row || (row.status !== 'known' && row.value === null), 'A retired aircraft must expose no captured value');
  };
  await passive(channel, 1);
  const before = await game();
  assert.equal(before.currentVehicle?.id, 'A-10C_2');
  const waitForBroadGeneration = async previous => {
    const result = await until(async () => {
      const now = await game();
      if (now.currentVehicle?.id !== 'A-10C_2') return {live:false};
      const generation = now.integrationSetup.catalogueGeneration;
      if (generation === previous) return false;
      const row = (await inspect(channel)).channels.find(row => row.id === channel);
      return row?.status === 'known' && row.value === 1 && {live:true, generation};
    }, 'The provider catalogue generation did not advance');
    assert.ok(result.live, 'An unrelated aircraft edit retired the selected reader');
    return result.generation;
  };
  let changed;
  try {
    await writeFile(unrelated, Buffer.concat([otherBytes, Buffer.from('\n')]));
    changed = await waitForBroadGeneration(before.integrationSetup.catalogueGeneration);
    for (let sample = 0; sample < 5; sample++) {
      assert.equal((await game()).currentVehicle?.id, 'A-10C_2', 'Unselected edit interrupted the reader');
      await delay(80);
    }
  } finally {
    await writeFile(unrelated, otherBytes);
  }
  const restored = await waitForBroadGeneration(changed);
  let withdrawalMs;
  setTraffic(false);
  try {
    await delay(140);
    assert.equal((await game()).currentVehicle?.id, 'A-10C_2', 'The quiet stream must still have a live capture before the edit');
    const start = performance.now();
    try {
      await writeFile(selected, Buffer.concat([selectedBytes, Buffer.from('\n')]));
      await until(async () => {
        const now = await game();
        return !now.currentVehicle && now.vehicleStatus?.message?.includes('selected DCS aircraft');
      }, 'The selected file edit did not wake and retire the passive reader without UDP traffic');
      withdrawalMs = performance.now() - start;
      await assertWithdrawn();
    } finally {
      await writeFile(selected, selectedBytes);
    }
    await delay(2300);
    await assertWithdrawn();
    assert.equal((await game()).currentVehicle, null);
  } finally {
    setTraffic(true);
  }
  await passive(channel, 1);
  assert.equal((await game()).currentVehicle?.id, 'A-10C_2');
  return {unrelatedGenerationChanged:changed, unrelatedGenerationRestored:restored,
    withdrawalMs, recoveredWithFreshPackets:true};
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
