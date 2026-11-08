// Real native helper lifetime. Uses a private Cockpit database and a neutral output.
import assert from 'node:assert/strict';
import {execFile} from 'node:child_process';
import {randomUUID} from 'node:crypto';
import {readFile} from 'node:fs/promises';
import test from 'node:test';
import {promisify} from 'node:util';
import {nativePage, until} from '../../scripts/reactive/cdp.mjs';

const appPid = Number(process.env.OSG_CHILD_WAIT_APP_PID);
const enabled = process.env.OSG_DESKTOP_WORKER_STOP_NATIVE === '1';
const contract = {delivery: 'state', schema: {type: 'boolean'}, freshnessMs: 1000, fallback: false};

async function helpers() {
  const {stdout} = await promisify(execFile)('powershell.exe', [
    '-NoProfile', '-NonInteractive', '-Command',
    "$ErrorActionPreference='Stop'; Get-CimInstance Win32_Process -Filter \"ParentProcessId=$env:OSG_CHILD_WAIT_APP_PID\" | " +
      "Where-Object { $_.CommandLine -match '(?:^|\\s)--osg-desktop-input-worker$' } | " +
      "ForEach-Object { '{0}|{1:o}' -f $_.ProcessId,$_.CreationDate }",
  ], {windowsHide: true, timeout: 5000});
  return stdout.trim().split(/\r?\n/).filter(Boolean);
}

test('the running Cockpit stops its exact desktop worker after neutral profile Stop', {
  skip: !enabled,
  timeout: 45_000,
}, async () => {
  assert.ok(Number.isSafeInteger(appPid) && appPid > 0 && process.env.OSG_SQLITE_DATABASE,
    'Launch a dedicated native Cockpit with a disposable database and supply its process ID');
  const page = await nativePage();
  const request = body => page.evaluate(
    "import('/src/lib/gateway.ts').then(({gatewayRequest}) => gatewayRequest(" + JSON.stringify(body) + '))',
  );
  const status = async () => (await request({command: 'profile_status'})).profiles.profileSession;
  const desktop = async () => (await request({command: 'profile_desktop_status'})).desktop;
  const preferences = await desktop();
  const document = {
    ...JSON.parse(await readFile(new URL('../fixtures/profiles/blank.json', import.meta.url), 'utf8')),
    id: randomUUID(), name: 'Neutral desktop worker shutdown ' + Date.now(),
    processingNodes: [{
      id: 'neutral', name: 'Neutral', operation: {kind: 'constant', value: false}, inputs: [],
      outputs: [{id: 'released', name: 'Released', direction: 'output', contract}],
    }],
    desktopOutputs: [{
      id: 'keyboard', name: 'Keyboard', required: true, scope: {kind: 'desktop-wide'},
      ports: [{id: 'hold', name: 'Hold', direction: 'input', contract}],
      actions: [{portId: 'hold', action: {kind: 'key-hold', key: 44}}],
    }],
    bindings: [{id: 'neutral-binding', from: {nodeId: 'neutral', portId: 'released'}, to: {nodeId: 'keyboard', portId: 'hold'}}],
  };
  let saved;
  try {
    assert.equal((await status()).owner, null);
    assert.deepEqual(await helpers(), []);
    assert.equal(preferences.qualified, true);
    await request({command: 'profile_desktop_configure', enabled: true, scope: preferences.scope});
    saved = (await request({command: 'profile_editor_save', document, assignments: [], expectedRevision: null})).profiles.document;
    await request({command: 'profile_apply', id: saved.id, expectedRevision: saved.revision});
    await until(status, session => session.owner?.profileId === saved.id && ['running', 'degraded'].includes(session.state));
    const active = await until(desktop, state => state.runtime?.helperScope?.live && state.runtime.backend?.runs?.length === 1);
    assert.deepEqual(active.runtime.backend.held, []);
    assert.deepEqual(active.runtime.backend.possibleHeld, []);
    const running = await helpers();
    assert.equal(running.length, 1, 'The dedicated Cockpit owns one exact desktop helper');

    await request({command: 'profile_stop'});
    await until(status, session => session.owner === null && session.state === 'stopped');
    const stopped = await until(desktop, state => state.runtime?.state === 'stopped' && state.runtime.backend?.cleanupComplete);
    assert.deepEqual(stopped.runtime.backend.held, []);
    assert.deepEqual(stopped.runtime.backend.possibleHeld, []);
    assert.deepEqual(await helpers(), [], 'Stop completed with no desktop helper left under the live Cockpit process');
    process.kill(appPid, 0);
    console.log(JSON.stringify({appPid, workerIdentity: running[0], workerExited: true,
      appStillRunning: true, cleanupComplete: true, authoredNonNeutralInput: false}));
  } finally {
    try {
      if (saved) {
        const current = await status();
        if (current.owner?.profileId === saved.id) await request({command: 'profile_stop'});
        await until(status, state => state.owner === null);
        await request({command: 'profile_delete', id: saved.id, expectedRevision: saved.revision});
      }
      await request({command: 'profile_desktop_configure', enabled: preferences.enabled, scope: preferences.scope});
    } finally {
      await page.close();
    }
  }
});
