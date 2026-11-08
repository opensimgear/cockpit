import assert from 'node:assert/strict';
import {unlink, writeFile} from 'node:fs/promises';
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
