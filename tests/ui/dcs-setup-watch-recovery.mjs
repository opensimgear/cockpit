import assert from 'node:assert/strict';
import {rename, unlink, writeFile} from 'node:fs/promises';
import {resolve} from 'node:path';
import {setTimeout as delay} from 'node:timers/promises';

export async function verifyDcsSetupWatchRecovery(game, until, userFolder) {
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

export async function verifyLiveDcsSetupWithdrawal({game, until, script, child, absent, passive, biosId}) {
  if (process.env.OSG_DCS_VEHICLE_SCOPE_WAKE !== '1') return;
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
  return {withdrawn: true, recovered: true, unchangedRevision: true};
}
