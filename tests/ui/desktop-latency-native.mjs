// Opt-in real WSS -> existing native Cockpit workload; no native activity on discovery.
import test from 'node:test';
import assert from 'node:assert/strict';
import {createRequire} from 'node:module';
import {createServer} from 'node:http';
import {spawn} from 'node:child_process';
import {mkdir, readFile, writeFile, rm, readdir, open, stat} from 'node:fs/promises';
import {resolve} from 'node:path';
import {createHash, randomUUID} from 'node:crypto';

const enabled = process.env.OSG_DESKTOP_LATENCY_NATIVE === '1' && process.env.OSG_DESKTOP_NATIVE_INJECT === '1';
const root = resolve('.'), artifacts = resolve('.research/desktop-latency-native');
const allowed = new Set(['companion_status', 'companion_enable', 'companion_disable', 'companion_pair_offer',
 'companion_approve', 'companion_revoke', 'companion_assign', 'companion_unassign', 'companion_publish_begin',
 'companion_publish_chunk', 'companion_publish_commit', 'profile_status', 'profile_list', 'profile_delete',
 'profile_resources_list', 'profile_resources_inspect', 'profile_editor_save', 'profile_compile', 'profile_apply',
 'profile_stop', 'profile_select', 'profile_desktop_status', 'profile_desktop_configure',
 'profile_desktop_capture_application', 'profile_desktop_audit_checkpoint', 'profile_desktop_audit_marker']);

async function invoke(page, request) {
 return page.evaluate(request => {
  if (!window.__TAURI_INTERNALS__?.invoke) throw Error('The existing page is not native Cockpit');
  return window.__TAURI_INTERNALS__.invoke('gateway_request', {request: {...request, clientId: 'desktop-latency-native-check'}});
 }, request);
}

async function powershell(script, env = {}) {
 const child = spawn('powershell.exe', ['-NoProfile', '-NonInteractive', '-Command', script], {
  windowsHide: true, timeout: 15_000, env: {...process.env, ...env}, stdio: ['ignore', 'pipe', 'pipe']});
 let output = '', errors = '';
 child.stdout.on('data', bytes => { output += bytes; }); child.stderr.on('data', bytes => { errors += bytes; });
 const code = await new Promise((resolve, reject) => { child.once('error', reject); child.once('close', resolve); });
 assert.equal(code, 0, errors);
 return JSON.parse(output);
}

async function nativeWorkers() {
 return powershell("$ErrorActionPreference='Stop'; $rows=@(Get-CimInstance Win32_Process -Filter \"Name = 'osg-cockpit.exe'\" | Where-Object {$_.CommandLine -like '*--osg-desktop-input-worker*'} | ForEach-Object {[pscustomobject]@{pid=$_.ProcessId;name=$_.Name}}); ConvertTo-Json -InputObject $rows -Compress");
}

async function stopped(page, applied) {
 const profile = await invoke(page, {command: 'profile_status'}), desktop = await invoke(page, {command: 'profile_desktop_status'});
 assert.equal(profile.ok, true); assert.equal(desktop.ok, true);
 assert.equal(profile.state.profiles.profileSession.state, 'stopped'); assert.equal(profile.state.profiles.profileSession.owner, null);
 const runtime = desktop.state.desktop.runtime;
 if (runtime == null) assert.equal(applied, false, 'No native ledger proves cleanup after attempted Apply');
 else {
  const state = runtime.backend;
  assert.equal(state.cleanupComplete, true); assert.equal(state.uncertain, false); assert.equal(state.inFlight, null);
  for (const key of ['held', 'possibleHeld', 'owners', 'runs']) assert.deepEqual(state[key], []);
 }
 const workers = await nativeWorkers(); assert.deepEqual(workers, []);
 return {profileSession: profile.state.profiles.profileSession, desktop: desktop.state.desktop, nativeInputWorkers: workers};
}

async function launchReceiver(file) {
 for (const path of [file, file.replace(/\.json$/, '.focus'), file.replace(/\.json$/, '.stop')]) await rm(path, {force: true});
 const child = spawn(process.env.OSG_DESKTOP_RECEIVER ?? resolve('crates/desktop-output/target/debug/examples/native_receiver.exe'),
  ['--receiver-only', file], {cwd: root, windowsHide: true, stdio: ['ignore', 'pipe', 'pipe']});
 let errors = '';
 child.stderr.on('data', chunk => { errors += chunk; });
 let ready;
 try {
  ready = await new Promise((resolve, reject) => {
   let output = '';
   const timer = setTimeout(() => reject(Error('Receiver readiness timed out: ' + errors)), 10_000);
   child.once('error', error => { clearTimeout(timer); reject(error); });
   child.once('close', code => { clearTimeout(timer); reject(Error(`Receiver exited ${code}: ${errors}`)); });
   child.stdout.on('data', chunk => {
    output += chunk;
    if (output.includes('\n')) {
     try { const value = JSON.parse(output.split('\n')[0]); assert.ok(value.identity); clearTimeout(timer); resolve(value); }
     catch (error) { clearTimeout(timer); reject(error); }
    }
   });
  });
 } catch (error) { child.kill(); throw error; }
 return {child, file, identity: ready.identity};
}

async function closeReceiver(target) {
 if (target.child.exitCode !== null || target.child.signalCode !== null) return;
 const exited = new Promise(resolve => target.child.once('close', resolve));
 await writeFile(target.file.replace(/\.json$/, '.stop'), 'stop');
 await Promise.race([exited, new Promise(resolve => setTimeout(resolve, 2000))]);
 if (target.child.exitCode === null && target.child.signalCode === null) { target.child.kill(); await exited; }
 assert.equal(target.child.exitCode, 0);
}

async function profilesUi(page) {
 await page.evaluate(() => {
  const button = [...document.querySelectorAll('button')].find(node => node.textContent.trim() === 'Profiles');
  button?.click(); // DOM handler only: never bring the WebView to OS foreground.
  document.querySelector('button.back-profiles')?.click();
 });
 await page.waitForFunction(() => !!document.querySelector('.profiles-workspace .list-tools'), undefined, {timeout: 5000});
}

async function uiLoad(page, start, profileName) {
 if (start) {
  await profilesUi(page);
  await page.evaluate(() => window.dispatchEvent(new Event('focus')));
  await page.waitForFunction(name => [...document.querySelectorAll('.profile-list .profile-copy strong')].some(node => node.textContent.trim() === name), profileName, {timeout: 5000});
 }
 return page.evaluate(({start, profileName}) => {
  const previous = window.__osgDesktopLatencyUi;
  if (previous) previous.cleanup();
  const snapshot = state => state ? {mode: 'real-svelte-profiles-auto-refresh', refreshEvents: state.events, refreshEventsDelivered: state.delivered,
   domMutations: state.mutations, elapsedMs: performance.now() - state.began,
   profileName: state.profileName, realProfilesView: !!document.querySelector('.profiles-workspace .profile-list'), nativeCockpit: !!window.__TAURI_INTERNALS__,
   targetProfileVisible: [...document.querySelectorAll('.profile-list .profile-copy strong')].some(node => node.textContent.trim() === state.profileName),
   visibleRuntimeStatus: document.querySelector('[aria-label="Runtime profile status"]')?.textContent?.trim() ?? ''}
   : {mode: 'idle', refreshEvents: 0, refreshEventsDelivered: 0, domMutations: 0, elapsedMs: 0};
  if (!start) { delete window.__osgDesktopLatencyUi; return snapshot(previous); }
  const section = document.querySelector('.profiles-workspace');
  if (!section?.querySelector('.profile-list') || !window.__TAURI_INTERNALS__) throw Error('The actual native Svelte Profiles list is unavailable');
  const state = {events: 0, delivered: 0, currentEvent: null, mutations: 0, began: performance.now(), profileName};
  // Exercise the real focus-refresh handler without making the WebView foreground.
  // Count event delivery, not native completions or UI busy transitions: passive
  // refresh deliberately coalesces pending reads and keeps the controls usable.
  const observedFocus = event => { if (event === state.currentEvent) state.delivered++; };
  window.addEventListener('focus', observedFocus);
  state.observer = new MutationObserver(records => {
   state.mutations += records.length;
  });
  state.observer.observe(section, {subtree: true, attributes: true, childList: true, characterData: true});
  state.timer = setInterval(() => { if (!document.hidden && section.isConnected) { state.events++; state.currentEvent = new Event('focus'); window.dispatchEvent(state.currentEvent); state.currentEvent = null; } }, 40);
  state.cleanup = () => { clearInterval(state.timer); state.observer.disconnect(); window.removeEventListener('focus', observedFocus); };
  window.__osgDesktopLatencyUi = state;
  return snapshot(state);
 }, {start, profileName});
}

test('real native desktop audit under two-source input and Svelte UI load', {skip: !enabled, timeout: 300_000}, async () => {
 assert.equal(process.platform, 'win32');
 const auditDirectory = process.env.OSG_DESKTOP_AUDIT_DIR, auditRun = process.env.OSG_DESKTOP_AUDIT_RUN;
 assert.ok(auditDirectory && auditRun && /^[A-Za-z0-9_-]{1,64}$/.test(auditRun), 'Use the audit directory/run supplied to the running app at startup');
 const executable = resolve(process.env.OSG_DESKTOP_NATIVE_APP ?? 'src-tauri/target/debug/osg-cockpit.exe');
 const appHash = createHash('sha256').update(await readFile(executable)).digest('hex');
 const appRows = await powershell("$ErrorActionPreference='Stop'; $expected=[IO.Path]::GetFullPath($env:OSG_LATENCY_EXPECTED_EXE); $rows=@(Get-CimInstance Win32_Process -Filter \"Name = 'osg-cockpit.exe'\" | Where-Object {$_.ExecutablePath -eq $expected -and $_.CommandLine -notlike '*--osg-desktop-input-worker*'} | ForEach-Object {[pscustomobject]@{pid=$_.ProcessId;executable=$_.ExecutablePath;created=$_.CreationDate.ToUniversalTime().ToString('o')}}); ConvertTo-Json -InputObject $rows -Compress", {OSG_LATENCY_EXPECTED_EXE: executable});
 assert.equal(appRows.length, 1, 'The hash must belong to the exact existing native app process');
 const require = createRequire(import.meta.url), {chromium} = require(process.env.OSG_PLAYWRIGHT_MODULE ?? 'playwright');
 await mkdir(artifacts, {recursive: true});
 await rm(resolve(artifacts, 'parent-checkpoint.json'), {force: true});
 const browser = await chromium.connectOverCDP(process.env.OSG_NATIVE_CDP_URL ?? 'http://127.0.0.1:9222');
 try {
  const page = browser.contexts().flatMap(context => context.pages()).find(page => /127\.0\.0\.1:1420|localhost:1420|tauri\.localhost/.test(page.url()));
  assert.ok(page, 'An already-running native development Cockpit is required');
  const before = await stopped(page, false), preferences = before.desktop;
  const profileBefore = (await invoke(page, {command: 'profile_status'})).state.profiles;
  assert.equal(profileBefore.session.selectionMode, 'manual');
  const listenerBefore = (await invoke(page, {command: 'companion_status'})).state.companions.listener?.enabled ?? false;
  const settingsFile = process.env.OSG_DESKTOP_SETTINGS_FILE ?? resolve(process.env.APPDATA, 'OpenSimGear/Cockpit/desktop-input-v1.json');
  const snapshots = [];
  for (const path of [settingsFile, resolve(process.env.APPDATA, 'OpenSimGear/Cockpit/profiles-activation-v1.json')]) {
   try { snapshots.push({path, data: await readFile(path)}); }
   catch (error) { if (error.code !== 'ENOENT') throw error; snapshots.push({path, data: undefined}); }
  }
  await profilesUi(page);
  const token = randomUUID(), commands = [], profiles = new Map(), paired = new Set(), assigned = new Set(), receivers = [];
  let server, pythonChild, target;
  try {
   server = createServer(async (req, res) => {
    try {
     if (req.method !== 'POST' || req.url !== '/native' || req.headers.authorization !== `Bearer ${token}`) throw Error('Unauthorized acceptance bridge request');
     let bytes = 0; const chunks = [];
     for await (const chunk of req) { bytes += chunk.length; if (bytes > 2 * 1024 * 1024) throw Error('Acceptance request exceeds bound'); chunks.push(chunk); }
     const request = JSON.parse(Buffer.concat(chunks).toString('utf8'));
     let result;
     if (request.command === 'latency_receiver_begin') {
      assert.ok(['idle', 'ui-load'].includes(request.phase));
      await stopped(page, commands.some(item => item.command === 'profile_apply'));
      if (target) await closeReceiver(target);
      target = await launchReceiver(resolve(artifacts, `receiver-${request.phase}.json`)); receivers.push(target);
      result = {ok: true, state: {latency: {file: target.file, identity: target.identity}}};
     } else if (request.command === 'latency_ui_load') {
      assert.equal(typeof request.enabled, 'boolean');
      result = {ok: true, state: {latency: await uiLoad(page, request.enabled, request.profileName)}};
     } else {
      if (!allowed.has(request.command)) throw Error('Command is outside native latency acceptance');
      if (request.command === 'profile_editor_save' && profiles.size && !profiles.has(request.document.id)) throw Error('Only the exact latency test profile may be revised');
      if (request.command === 'profile_delete' && profiles.get(request.id) !== request.expectedRevision) throw Error('Only the exact latency test revision may be deleted');
      commands.push({command: request.command, at: new Date().toISOString()});
      result = await invoke(page, request);
      if (result.ok && request.command === 'profile_editor_save') { const document = result.state.profiles.document; profiles.set(document.id, document.revision); }
      if (result.ok && request.command === 'companion_approve') paired.add(request.companion.companionId);
      if (result.ok && request.command === 'companion_assign') assigned.add(request.companion.instanceId);
      if (result.ok && request.command === 'companion_unassign') assigned.delete(request.companion.instanceId);
      if (result.ok && request.command === 'companion_revoke') paired.delete(request.companion.companionId);
     }
     res.writeHead(200, {'content-type': 'application/json'}); res.end(JSON.stringify(result));
    } catch (error) { res.writeHead(400, {'content-type': 'application/json'}); res.end(JSON.stringify({ok: false, error: String(error)})); }
   });
   server.requestTimeout = 30_000;
   await new Promise(resolve => server.listen(0, '127.0.0.1', resolve));
   pythonChild = spawn(process.env.OSG_PYTHON ?? 'python', ['scripts/desktop-latency-native.py', '--inject',
    '--bridge', `http://127.0.0.1:${server.address().port}/native`, '--output', resolve(artifacts, 'evidence.json'),
    '--desktop-settings-file', settingsFile, '--app-sha256', appHash, '--audit-directory', resolve(auditDirectory), '--audit-run', auditRun], {
     cwd: root, windowsHide: true, timeout: 180_000, env: {...process.env, OSG_NATIVE_BRIDGE_TOKEN: token}, stdio: ['ignore', 'pipe', 'pipe']});
   let output = '';
   pythonChild.stdout.on('data', bytes => { output += bytes; }); pythonChild.stderr.on('data', bytes => { output += bytes; });
   const code = await new Promise((resolve, reject) => { pythonChild.once('error', reject); pythonChild.once('close', resolve); });
   await writeFile(resolve(artifacts, 'run.log'), output); assert.equal(code, 0, output);
   const evidence = JSON.parse(await readFile(resolve(artifacts, 'evidence.json'), 'utf8'));
   assert.equal(evidence.nativeExecutableSha256, appHash); assert.equal(evidence.productionQualified, false);
   assert.equal(evidence.phases.length, 2); assert.equal(evidence.injected, true); assert.deepEqual(evidence.cleanupErrors, []);
   const files = [];
   for (const name of await readdir(auditDirectory)) if (name.startsWith(`audit-${auditRun}-`) && name.endsWith('.jsonl')) {
    const path = resolve(auditDirectory, name), file = await open(path, 'r');
    let header;
    try { const buffer = Buffer.alloc(16_384), {bytesRead} = await file.read(buffer, 0, buffer.length, 0); header = JSON.parse(buffer.subarray(0, bytesRead).toString('utf8').split('\n')[0]); }
    finally { await file.close(); }
    assert.equal(header.run, auditRun); assert.equal(header.type, 'header');
    const size = (await stat(path)).size; assert.ok(size <= 32 * 1024 * 1024);
    files.push({file: name, header, bytes: size});
   }
   assert.ok(files.some(item => item.header.role === 'parent' && item.header.processPid === String(appRows[0].pid)), 'Audit parent header must match the exact hashed app');
   assert.ok(files.filter(item => item.header.role === 'worker').length >= 2, 'Both native workers must publish audit context');
   await writeFile(resolve(artifacts, 'run-manifest.json'), JSON.stringify({nativeProcess: appRows[0], nativeExecutableSha256: appHash,
    sourceRevision: process.env.OSG_DESKTOP_APP_REVISION ?? null, auditRun, auditDirectory: resolve(auditDirectory), files,
    receiverProcesses: receivers.map(item => ({pid: item.child.pid, identity: item.identity, file: item.file}))}, null, 2) + '\n');
  } finally {
   if (pythonChild && pythonChild.exitCode === null && pythonChild.signalCode === null) {
    const exited = new Promise(resolve => pythonChild.once('close', resolve)); pythonChild.kill(); await exited;
   }
   const cleanup = {errors: []};
   const attempt = async (label, operation) => { try { return await operation(); } catch (error) { cleanup.errors.push(`${label}: ${String(error)}`); } };
   await attempt('Stop actual UI refresh', () => uiLoad(page, false, ''));
   await attempt('Stop native profile', async () => { const result = await invoke(page, {command: 'profile_stop'}); assert.equal(result.ok, true); });
   const clear = await attempt('Prove clean native ownership', async () => { cleanup.ownershipProof = await stopped(page, commands.some(item => item.command === 'profile_apply')); return true; }) === true;
   cleanup.ownershipCleanupConfirmed = clear; cleanup.needsRootCleanup = !clear;
   if (clear) {
    await attempt('Restore desktop enablement/scope', async () => { const result = await invoke(page, {command: 'profile_desktop_configure', enabled: preferences.enabled, scope: preferences.scope}); assert.equal(result.ok, true); });
    await attempt('Restore prior Manual selection', async () => { const result = await invoke(page, {command: 'profile_select', id: profileBefore.session.selectedProfileId, selectionSource: 'manual'}); assert.equal(result.ok, true); });
    for (const instanceId of assigned) await attempt('Unassign exact test instance', async () => { const result = await invoke(page, {command: 'companion_unassign', companion: {instanceId}}); assert.equal(result.ok, true); });
    for (const companionId of paired) await attempt('Revoke exact test peer', async () => { const result = await invoke(page, {command: 'companion_revoke', companion: {companionId}}); assert.equal(result.ok, true); });
    if (!listenerBefore) await attempt('Restore disabled listener', async () => { const result = await invoke(page, {command: 'companion_disable'}); assert.equal(result.ok, true); });
    await attempt('Remove only owned remaining test revisions', async () => {
     const library = await invoke(page, {command: 'profile_list'}); assert.equal(library.ok, true);
     for (const item of library.state.profiles.profiles) if (profiles.has(item.id)) {
      assert.equal(item.revision, profiles.get(item.id)); const result = await invoke(page, {command: 'profile_delete', id: item.id, expectedRevision: item.revision}); assert.equal(result.ok, true);
     }
    });
    for (const {path, data} of snapshots) await attempt('Restore exact saved preference bytes', async () => {
     if (data === undefined) await rm(path, {force: true}); else await writeFile(path, data);
    });
    await attempt('Verify restored native state', async () => {
     cleanup.final = await stopped(page, commands.some(item => item.command === 'profile_apply'));
     assert.equal(cleanup.final.desktop.enabled, preferences.enabled); assert.deepEqual(cleanup.final.desktop.scope, preferences.scope);
     assert.deepEqual(cleanup.final.desktop.application, preferences.application);
     const current = (await invoke(page, {command: 'profile_status'})).state.profiles;
     assert.equal(current.session.selectedProfileId, profileBefore.session.selectedProfileId); assert.equal(current.session.selectionMode, 'manual');
     for (const {path, data} of snapshots) {
      if (data !== undefined) assert.deepEqual(await readFile(path), data);
      else await assert.rejects(readFile(path), error => error.code === 'ENOENT');
     }
    });
    for (const receiver of receivers) await attempt('Close receive-only helper', () => closeReceiver(receiver));
    await attempt('Save the final externally acknowledged parent audit checkpoint', async () => {
     cleanup.afterReceiverClose = await stopped(page, commands.some(item => item.command === 'profile_apply'));
     const result = await invoke(page, {command: 'profile_desktop_audit_checkpoint'}); assert.equal(result.ok, true);
     const report = result.state.desktop.audit;
     for (const key of ['configured', 'enabled', 'complete', 'valid']) assert.equal(report[key], true);
     const decimal = value => typeof value === 'string' && /^(0|[1-9][0-9]*)$/.test(value);
     assert.ok(decimal(report.throughSequence)); assert.ok(Object.values(report.counters).every(decimal));
     assert.ok(BigInt(report.counters.written) < 65536n);
     cleanup.finalAuditCheckpoint = report;
     await writeFile(resolve(artifacts, 'parent-checkpoint.json'), JSON.stringify(report, null, 2) + '\n');
    });
   } else for (const receiver of receivers) if (receiver.child.exitCode === null && receiver.child.signalCode === null) {
    receiver.child.stdout.destroy(); receiver.child.stderr.destroy(); receiver.child.unref();
   }
   cleanup.receivers = receivers.map(item => ({pid: item.child.pid, exitCode: item.child.exitCode, retainedForRootCleanup: !clear}));
   await writeFile(resolve(artifacts, 'commands.json'), JSON.stringify(commands, null, 2) + '\n');
   await writeFile(resolve(artifacts, 'wrapper-cleanup.json'), JSON.stringify(cleanup, null, 2) + '\n');
   if (server) await new Promise(resolve => server.close(resolve));
   assert.deepEqual(cleanup.errors, [], 'Inspect the native latency cleanup evidence');
  }
 } finally { await browser.close(); }
});
