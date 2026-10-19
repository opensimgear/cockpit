// Opt-in actual WSS -> running native Cockpit -> receive-only window acceptance.
// Default CI discovery performs no browser connection, subprocess or OS input.
import test from 'node:test';
import assert from 'node:assert/strict';
import {createRequire} from 'node:module';
import {createServer} from 'node:http';
import {spawn} from 'node:child_process';
import {mkdir, readFile, writeFile, rm} from 'node:fs/promises';
import {resolve} from 'node:path';
import {randomUUID} from 'node:crypto';

const enabled = process.env.OSG_DESKTOP_OWNERSHIP_NATIVE === '1' && process.env.OSG_DESKTOP_NATIVE_INJECT === '1';
const root = resolve('.'), artifacts = resolve('.research/desktop-ownership-native');
const allowed = new Set(['companion_status', 'companion_enable', 'companion_disable', 'companion_pair_offer',
 'companion_approve', 'companion_revoke', 'companion_assign', 'companion_unassign', 'companion_publish_begin',
 'companion_publish_chunk', 'companion_publish_commit', 'profile_status', 'profile_list', 'profile_delete',
 'profile_resources_list', 'profile_resources_inspect', 'profile_editor_save', 'profile_compile', 'profile_apply',
 'profile_stop', 'profile_pause', 'profile_select', 'profile_desktop_status', 'profile_desktop_configure',
 'profile_desktop_capture_application']);

async function invoke(page, request) {
 return page.evaluate(request => {
  if (!window.__TAURI_INTERNALS__?.invoke) throw Error('Existing page is not native Cockpit');
  return window.__TAURI_INTERNALS__.invoke('gateway_request', {request: {...request, clientId: 'desktop-ownership-native-check'}});
 }, request);
}

async function targetReceiver(file) {
 for (const path of [file, file.replace(/\.json$/, '.focus'), file.replace(/\.json$/, '.stop')]) await rm(path, {force: true});
 const executable = process.env.OSG_DESKTOP_RECEIVER ?? resolve('crates/desktop-output/target/debug/examples/native_receiver.exe');
 const child = spawn(executable, ['--receiver-only', file], {cwd: root, windowsHide: true, stdio: ['ignore', 'pipe', 'pipe']});
 let stderr = '';
 child.stderr.on('data', chunk => { stderr += chunk.toString(); });
 try {
  await new Promise((resolve, reject) => {
   let output = '';
   const timer = setTimeout(() => reject(Error('Receive-only window readiness timed out: ' + stderr)), 10_000);
   child.once('error', error => { clearTimeout(timer); reject(error); });
   child.once('close', code => { clearTimeout(timer); reject(Error(`Receiver exited ${code}: ${stderr}`)); });
   child.stdout.on('data', chunk => {
    output += chunk.toString();
    if (output.includes('\n')) {
     try { const ready = JSON.parse(output.split('\n')[0]); assert.ok(ready.identity); clearTimeout(timer); resolve(); }
     catch (error) { clearTimeout(timer); reject(error); }
    }
   });
  });
 } catch (error) { child.kill(); throw error; }
 return {child, file};
}

async function closeReceiver({child, file}) {
 if (child.exitCode !== null || child.signalCode !== null) return;
 const exited = new Promise(resolve => child.once('close', resolve));
 await writeFile(file.replace(/\.json$/, '.stop'), 'stop');
 await Promise.race([exited, new Promise(resolve => setTimeout(resolve, 2000))]);
 if (child.exitCode === null && child.signalCode === null) { child.kill(); await exited; }
}

async function nativeWorkers() {
 const script = "$ErrorActionPreference='Stop'; $workers=@(Get-CimInstance Win32_Process -Filter \"Name = 'osg-cockpit.exe'\" | Where-Object {$_.CommandLine -like '*--osg-desktop-input-worker*'} | ForEach-Object {[pscustomobject]@{pid=$_.ProcessId;name=$_.Name}}); ConvertTo-Json -InputObject $workers -Compress";
 const child = spawn('powershell.exe', ['-NoProfile', '-NonInteractive', '-Command', script], {windowsHide: true, stdio: ['ignore', 'pipe', 'pipe'], timeout: 15_000});
 let output = '', errors = '';
 child.stdout.on('data', chunk => { output += chunk.toString(); });
 child.stderr.on('data', chunk => { errors += chunk.toString(); });
 const code = await new Promise((resolve, reject) => { child.once('error', reject); child.once('close', resolve); });
 assert.equal(code, 0, errors);
 return JSON.parse(output);
}

test('two actual WSS sources independently own one native MouseLeft', {skip: !enabled, timeout: 300_000}, async () => {
 assert.equal(process.platform, 'win32');
 const require = createRequire(import.meta.url);
 const {chromium} = require(process.env.OSG_PLAYWRIGHT_MODULE ?? 'playwright');
 await mkdir(artifacts, {recursive: true});
 const browser = await chromium.connectOverCDP(process.env.OSG_NATIVE_CDP_URL ?? 'http://127.0.0.1:9222');
 try {
 const page = browser.contexts().flatMap(context => context.pages()).find(page => /127\.0\.0\.1:1420|localhost:1420|tauri\.localhost/.test(page.url()));
 assert.ok(page, 'An already-running native development Cockpit page is required');
 const profileBefore = await invoke(page, {command: 'profile_status'});
 const preferences = await invoke(page, {command: 'profile_desktop_status'});
 const companionBefore = await invoke(page, {command: 'companion_status'});
 assert.equal(profileBefore.ok, true); assert.equal(preferences.ok, true); assert.equal(companionBefore.ok, true);
 assert.equal(profileBefore.state.profiles.profileSession.state, 'stopped');
 assert.equal(profileBefore.state.profiles.profileSession.owner, null);
 assert.equal(profileBefore.state.profiles.session.selectionMode, 'manual', 'Automatic activation must not be authorized by this isolated acceptance');
 assert.deepEqual(await nativeWorkers(), [], 'Another native input worker is active');
 const desktopBefore = preferences.state.desktop, selectedBefore = profileBefore.state.profiles.session.selectedProfileId;
 const listenerEnabled = companionBefore.state.companions.listener?.enabled ?? false;
 const settingsFile = process.env.OSG_DESKTOP_SETTINGS_FILE ?? resolve(process.env.APPDATA, 'OpenSimGear/Cockpit/desktop-input-v1.json');
 const activationFile = resolve(process.env.APPDATA, 'OpenSimGear/Cockpit/profiles-activation-v1.json');
 const snapshots = [];
 for (const path of [settingsFile, activationFile]) {
  try { snapshots.push({path, data: await readFile(path)}); }
  catch (error) { if (error.code !== 'ENOENT') throw error; snapshots.push({path, data: undefined}); }
 }
 const token = randomUUID(), commands = [], ownedProfiles = new Map(), paired = new Set(), assigned = new Set();
 let target, server, pythonChild;
 try {
  target = await targetReceiver(resolve(artifacts, 'receiver.json'));
  server = createServer(async (req, res) => {
   try {
    if (req.method !== 'POST' || req.url !== '/native' || req.headers.authorization !== `Bearer ${token}`) throw Error('Unauthorized acceptance bridge request');
    let bytes = 0;
    const chunks = [];
    for await (const chunk of req) { bytes += chunk.length; if (bytes > 2 * 1024 * 1024) throw Error('Acceptance request exceeds its bound'); chunks.push(chunk); }
    const request = JSON.parse(Buffer.concat(chunks).toString('utf8'));
    if (!allowed.has(request.command)) throw Error('Command is outside ownership acceptance');
    if (request.command === 'profile_editor_save' && ownedProfiles.size && !ownedProfiles.has(request.document.id)) throw Error('Only this acceptance profile may be revised');
    if (request.command === 'profile_delete' && ownedProfiles.get(request.id) !== request.expectedRevision) throw Error('Only the exact acceptance profile revision may be deleted');
    commands.push({command: request.command, at: new Date().toISOString()});
    const result = await invoke(page, request);
    if (result.ok && request.command === 'profile_editor_save') {
     const document = result.state.profiles.document;
     ownedProfiles.set(document.id, document.revision);
    }
    if (result.ok && request.command === 'companion_approve') paired.add(request.companion.companionId);
    if (result.ok && request.command === 'companion_assign') assigned.add(request.companion.instanceId);
    if (result.ok && request.command === 'companion_unassign') assigned.delete(request.companion.instanceId);
    if (result.ok && request.command === 'companion_revoke') paired.delete(request.companion.companionId);
    res.writeHead(200, {'content-type': 'application/json'}); res.end(JSON.stringify(result));
   } catch (error) { res.writeHead(400, {'content-type': 'application/json'}); res.end(JSON.stringify({ok: false, error: String(error)})); }
  });
  server.requestTimeout = 30_000;
  await new Promise(resolve => server.listen(0, '127.0.0.1', resolve));
  pythonChild = spawn(process.env.OSG_PYTHON ?? 'python', ['scripts/desktop-ownership-native.py', '--inject',
   '--bridge', `http://127.0.0.1:${server.address().port}/native`, '--output', resolve(artifacts, 'evidence.json'),
   '--receiver', target.file, '--desktop-settings-file', settingsFile], {cwd: root, windowsHide: true, timeout: 150_000,
   env: {...process.env, OSG_NATIVE_BRIDGE_TOKEN: token}, stdio: ['ignore', 'pipe', 'pipe']});
  let output = '';
  pythonChild.stdout.on('data', chunk => { output += chunk.toString(); });
  pythonChild.stderr.on('data', chunk => { output += chunk.toString(); });
  const code = await new Promise((resolve, reject) => { pythonChild.once('error', reject); pythonChild.once('close', resolve); });
  await writeFile(resolve(artifacts, 'run.log'), output);
  assert.equal(code, 0, output);
  const evidence = JSON.parse(await readFile(resolve(artifacts, 'evidence.json'), 'utf8'));
  assert.equal(evidence.actualNativeCockpit, true); assert.equal(evidence.actualWssPeers, 2); assert.equal(evidence.injected, true);
  assert.equal(evidence.virtualControllers, 0); assert.equal(evidence.productionQualified, false);
  assert.deepEqual(evidence.mouseMessages, [513, 514, 513, 514, 513, 514]);
  assert.equal(evidence.checks.length, 4); assert.deepEqual(evidence.cleanupErrors, []);
  assert.equal(evidence.stages.independentLoss.blockedPublicationWrite.ok, false);
  assert.equal(evidence.stages.sessionReplacement.protocol.inputBeforeBaseline.ok, false);
  assert.equal(evidence.stages.sessionReplacement.protocol.heldBaseline.ok, false);
  assert.equal(evidence.stages.sessionReplacement.protocol.neutralBaseline.ok, true);
 } finally {
  // A terminated Python process cannot prevent the independent wrapper from
  // revoking native ownership and restoring its exact local preference bytes.
  if (pythonChild && pythonChild.exitCode === null && pythonChild.signalCode === null) {
   const exited = new Promise(resolve => pythonChild.once('close', resolve)); pythonChild.kill(); await exited;
  }
  const cleanup = {errors: [], deletedTestProfiles: []};
  const attempt = async (label, operation) => {
   try { return await operation(); } catch (error) { cleanup.errors.push(`${label}: ${String(error)}`); }
  };
  await attempt('Stop', async () => { cleanup.stop = await invoke(page, {command: 'profile_stop'}); assert.equal(cleanup.stop.ok, true); });
  const ownershipClear = await attempt('Prove native ownership cleanup before restoring settings', async () => {
   const status = await invoke(page, {command: 'profile_desktop_status'}), profile = await invoke(page, {command: 'profile_status'});
   assert.equal(status.ok, true); assert.equal(profile.ok, true);
   assert.equal(profile.state.profiles.profileSession.state, 'stopped'); assert.equal(profile.state.profiles.profileSession.owner, null);
   const runtime = status.state.desktop.runtime;
   if (runtime == null) assert.ok(!commands.some(item => item.command === 'profile_apply'), 'No native ledger proves cleanup after attempted Apply');
   else {
    const state = runtime.backend;
    assert.equal(state.cleanupComplete, true); assert.equal(state.uncertain, false); assert.equal(state.inFlight, null);
    assert.deepEqual(state.held, []); assert.deepEqual(state.possibleHeld, []); assert.deepEqual(state.owners, []);
   }
   const workers = await nativeWorkers(); assert.deepEqual(workers, [], 'A native input worker still owns the session');
   cleanup.ownershipProof = {profileSession: profile.state.profiles.profileSession, desktop: status.state.desktop, nativeInputWorkers: workers};
   return true;
  }) === true;
  cleanup.ownershipCleanupConfirmed = ownershipClear;
  cleanup.needsRootCleanup = !ownershipClear;
  if (ownershipClear) {
  await attempt('Restore desktop scope/enablement', async () => {
   const restored = await invoke(page, {command: 'profile_desktop_configure', enabled: desktopBefore.enabled, scope: desktopBefore.scope}); assert.equal(restored.ok, true);
  });
  await attempt('Restore selection without activation', async () => {
   const restored = await invoke(page, {command: 'profile_select', id: selectedBefore, selectionSource: 'manual'}); assert.equal(restored.ok, true);
  });
  await attempt('Delete only owned profile revisions', async () => {
   const library = await invoke(page, {command: 'profile_list'}); assert.equal(library.ok, true);
   for (const profile of library.state.profiles.profiles) if (ownedProfiles.has(profile.id)) {
    assert.equal(profile.revision, ownedProfiles.get(profile.id), 'Test profile was externally changed; preserve it');
    const removed = await invoke(page, {command: 'profile_delete', id: profile.id, expectedRevision: profile.revision});
    assert.equal(removed.ok, true); cleanup.deletedTestProfiles.push(profile.id);
   }
  });
  for (const instanceId of assigned) await attempt('Unassign exact test instance', async () => {
   const result = await invoke(page, {command: 'companion_unassign', companion: {instanceId}}); assert.equal(result.ok, true);
  });
  for (const companionId of paired) await attempt('Revoke exact test peer', async () => {
   const result = await invoke(page, {command: 'companion_revoke', companion: {companionId}}); assert.equal(result.ok, true);
  });
  if (!listenerEnabled) await attempt('Restore disabled listener', async () => {
   const result = await invoke(page, {command: 'companion_disable'}); assert.equal(result.ok, true);
  });
  for (const {path, data} of snapshots) await attempt('Restore exact preferences', async () => {
   if (data === undefined) await rm(path, {force: true}); else await writeFile(path, data);
  });
  await attempt('Verify stopped native owner and restored state', async () => {
   cleanup.desktop = await invoke(page, {command: 'profile_desktop_status'});
   cleanup.profile = await invoke(page, {command: 'profile_status'});
   cleanup.companion = await invoke(page, {command: 'companion_status'});
   for (const result of [cleanup.desktop, cleanup.profile, cleanup.companion]) assert.equal(result.ok, true);
   const final = cleanup.desktop.state.desktop, profile = cleanup.profile.state.profiles;
   assert.equal(profile.profileSession.state, 'stopped'); assert.equal(profile.profileSession.owner, null);
   assert.equal(profile.session.selectedProfileId, selectedBefore); assert.equal(profile.session.selectionMode, 'manual');
   assert.equal(final.enabled, desktopBefore.enabled); assert.deepEqual(final.scope, desktopBefore.scope); assert.deepEqual(final.application, desktopBefore.application);
   assert.equal(final.runtime.backend.cleanupComplete, true); assert.equal(final.runtime.backend.uncertain, false);
   assert.deepEqual(final.runtime.backend.held, []); assert.deepEqual(final.runtime.backend.possibleHeld, []); assert.deepEqual(final.runtime.backend.owners, []);
   assert.equal(cleanup.companion.state.companions.listener?.enabled ?? false, listenerEnabled);
  });
  }
  if (ownershipClear) await attempt('Close receive-only helper', async () => { if (target) await closeReceiver(target); });
  else if (target && target.child.exitCode === null && target.child.signalCode === null) {
   // Preserve the exact target and enabled recovery settings for coordination.
   // No successful Stop/ledger proof means closing it could hide a held control.
   target.child.stdout.destroy(); target.child.stderr.destroy(); target.child.unref();
  }
  cleanup.receiver = target ? {pid: target.child.pid, exitCode: target.child.exitCode, signalCode: target.child.signalCode, retainedForRootCleanup: !ownershipClear} : null;
  await attempt('Verify native worker absence', async () => {
   cleanup.nativeInputWorkers = await nativeWorkers();
   if (ownershipClear) assert.deepEqual(cleanup.nativeInputWorkers, [], 'Native desktop worker survived Stop');
  });
  await writeFile(resolve(artifacts, 'commands.json'), JSON.stringify(commands, null, 2) + '\n');
  await writeFile(resolve(artifacts, 'wrapper-cleanup.json'), JSON.stringify(cleanup, null, 2) + '\n');
  if (server) await new Promise(resolve => server.close(resolve));
  assert.deepEqual(cleanup.errors, [], 'Native ownership wrapper cleanup failed; inspect wrapper-cleanup.json');
 }
 } finally { await browser.close(); } // Disconnect CDP; keep the native app open.
});
