// Explicit actual-game acceptance. The existing native Cockpit owns launch and
// injection; the helper only supplies authenticated WSS input and observations.
import test from 'node:test';
import assert from 'node:assert/strict';
import {createRequire} from 'node:module';
import {createServer} from 'node:http';
import {spawn} from 'node:child_process';
import {mkdir, readFile, writeFile, rm} from 'node:fs/promises';
import {resolve} from 'node:path';
import {randomUUID} from 'node:crypto';

const enabled = process.env.OSG_DESKTOP_GAME_NATIVE === '1' && process.env.OSG_DESKTOP_NATIVE_INJECT === '1';
const root = resolve('.'), artifacts = resolve('.research/desktop-game-native');
const allowed = new Set(['companion_status', 'companion_enable', 'companion_disable', 'companion_pair_offer',
 'companion_approve', 'companion_revoke', 'companion_assign', 'companion_publish_begin', 'companion_publish_chunk',
 'companion_publish_commit', 'profile_status', 'profile_resources_list', 'profile_resources_inspect', 'profile_editor_save',
 'profile_compile', 'profile_assignments_get', 'profile_stop', 'profile_pause', 'profile_runtime_inspect',
 'profile_desktop_status', 'profile_desktop_configure', 'profile_select', 'profile_selection_mode',
 'profile_automatic_status', 'profile_game_launch', 'game_get_state']);

async function receiver(file) {
 for (const path of [file, file.replace(/\.json$/, '.focus'), file.replace(/\.json$/, '.stop')]) await rm(path, {force: true});
 const executable = process.env.OSG_DESKTOP_RECEIVER ?? resolve('crates/desktop-output/target/debug/examples/native_receiver.exe');
 const child = spawn(executable, ['--receiver-only', file], {cwd: root, windowsHide: true, stdio: ['ignore', 'pipe', 'pipe']});
 let stderr = '';
 child.stderr.on('data', chunk => { stderr += chunk.toString(); });
 try {
  await new Promise((resolve, reject) => {
   let output = '';
   const timer = setTimeout(() => reject(Error('Receiver readiness timed out: ' + stderr)), 10_000);
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

async function invoke(page, request) {
 return page.evaluate(request => {
  if (!window.__TAURI_INTERNALS__?.invoke) throw Error('Existing page is not native Cockpit');
  return window.__TAURI_INTERNALS__.invoke('gateway_request', {request: {...request, clientId: 'desktop-game-native-check'}});
 }, request);
}

async function processInventory() {
 const script = "$ErrorActionPreference='Stop'; $items=@(Get-CimInstance Win32_Process -Filter \"Name = 'acr.exe' OR Name = 'osg-cockpit.exe'\" | ForEach-Object { [pscustomobject]@{pid=$_.ProcessId;name=$_.Name;inputWorker=($_.CommandLine -like '*--osg-desktop-input-worker*')} }); [pscustomobject]@{gameProcesses=@($items | Where-Object {$_.name -eq 'acr.exe'});nativeInputWorkers=@($items | Where-Object {$_.name -eq 'osg-cockpit.exe' -and $_.inputWorker})} | ConvertTo-Json -Depth 4 -Compress";
 const child = spawn('powershell.exe', ['-NoProfile', '-NonInteractive', '-Command', script], {windowsHide: true, stdio: ['ignore', 'pipe', 'pipe']});
 let output = '', errors = '';
 child.stdout.on('data', chunk => { output += chunk.toString(); }); child.stderr.on('data', chunk => { errors += chunk.toString(); });
 const code = await new Promise((resolve, reject) => { child.once('error', reject); child.once('close', resolve); });
 assert.equal(code, 0, errors);
 return JSON.parse(output);
}

test('actual ACR launch binds game scope and cancels native desktop holds', {skip: !enabled, timeout: 300_000}, async () => {
 assert.equal(process.platform, 'win32');
 const require = createRequire(import.meta.url);
 const {chromium} = require(process.env.OSG_PLAYWRIGHT_MODULE ?? 'playwright');
 await mkdir(artifacts, {recursive: true});
 const browser = await chromium.connectOverCDP(process.env.OSG_NATIVE_CDP_URL ?? 'http://127.0.0.1:9222');
 const pages = browser.contexts().flatMap(context => context.pages());
 const page = pages.find(page => /127\.0\.0\.1:1420|localhost:1420|tauri\.localhost/.test(page.url())) ?? pages[0];
 assert.ok(page);
 const token = randomUUID(), commands = [], receiverFile = resolve(artifacts, 'receiver.json');
 const settingsFile = process.env.OSG_DESKTOP_SETTINGS_FILE ?? resolve(process.env.APPDATA, 'OpenSimGear/Cockpit/desktop-input-v1.json');
 const preferences = await invoke(page, {command: 'profile_desktop_status'});
 assert.equal(preferences.ok, true);
 const desktopBefore = preferences.state.desktop;
 let settingsBefore;
 try { settingsBefore = await readFile(settingsFile); } catch (error) { if (error.code !== 'ENOENT') throw error; }
 let target, server;
 try {
  target = await receiver(receiverFile);
  server = createServer(async (req, res) => {
   try {
    if (req.method !== 'POST' || req.url !== '/native' || req.headers.authorization !== `Bearer ${token}`) throw Error('Unauthorized acceptance bridge request');
    let bytes = 0;
    const chunks = [];
    for await (const chunk of req) { bytes += chunk.length; if (bytes > 2 * 1024 * 1024) throw Error('Acceptance request exceeds its bound'); chunks.push(chunk); }
    const request = JSON.parse(Buffer.concat(chunks).toString('utf8'));
    if (!allowed.has(request.command)) throw Error('Command is outside game acceptance');
    if (request.command === 'profile_game_launch' && request.gameId !== 'assetto-corsa-rally') throw Error('Only the explicitly selected game may launch');
    commands.push({command: request.command, at: new Date().toISOString()});
    const result = await invoke(page, request);
    res.writeHead(200, {'content-type': 'application/json'}); res.end(JSON.stringify(result));
   } catch (error) { res.writeHead(400, {'content-type': 'application/json'}); res.end(JSON.stringify({ok: false, error: String(error)})); }
  });
  server.requestTimeout = 180_000;
  await new Promise(resolve => server.listen(0, '127.0.0.1', resolve));
  const python = process.env.OSG_PYTHON ?? 'python';
  const child = spawn(python, ['scripts/desktop-game-native.py', '--launch', '--bridge', `http://127.0.0.1:${server.address().port}/native`,
   '--output', resolve(artifacts, 'evidence.json'), '--receiver', receiverFile, '--desktop-settings-file',
   settingsFile], {cwd: root, windowsHide: true, timeout: 240_000, env: {...process.env, OSG_NATIVE_BRIDGE_TOKEN: token}, stdio: ['ignore', 'pipe', 'pipe']});
  let output = '';
  child.stdout.on('data', chunk => { output += chunk.toString(); }); child.stderr.on('data', chunk => { output += chunk.toString(); });
  const code = await new Promise((resolve, reject) => { child.once('error', reject); child.once('close', resolve); });
  await writeFile(resolve(artifacts, 'run.log'), output);
  assert.equal(code, 0, output);
  const evidence = JSON.parse(await readFile(resolve(artifacts, 'evidence.json'), 'utf8'));
  assert.equal(evidence.actualNativeCockpit, true); assert.equal(evidence.actualLaunchRequested, true);
  assert.equal(evidence.actualGame, 'assetto-corsa-rally'); assert.equal(evidence.injected, true);
  assert.equal(evidence.observedGameBehavior, false); assert.equal(evidence.productionQualified, false);
  assert.equal(evidence.virtualControllers, 0); assert.ok(evidence.checks.length >= 5);
  assert.ok(evidence.actualProcess.created); assert.ok(evidence.focusReturn); assert.deepEqual(evidence.cleanupErrors, []);
 } finally {
  await writeFile(resolve(artifacts, 'commands.json'), JSON.stringify(commands, null, 2) + '\n');
  // Parent cancellation must still revoke the native owner if Python hit its
  // bounded child timeout before its own finally could execute.
  const cleanup = {errors: []};
  try {
   cleanup.stop = await invoke(page, {command: 'profile_stop'});
   assert.equal(cleanup.stop.ok, true);
   const restored = await invoke(page, {command: 'profile_desktop_configure', enabled: desktopBefore.enabled, scope: desktopBefore.scope});
   assert.equal(restored.ok, true);
   if (settingsBefore === undefined) await rm(settingsFile, {force: true}); else await writeFile(settingsFile, settingsBefore);
   cleanup.desktop = await invoke(page, {command: 'profile_desktop_status'});
   assert.equal(cleanup.desktop.ok, true);
   const final = cleanup.desktop.state.desktop;
   assert.equal(cleanup.stop.state.profiles.profileSession.state, 'stopped');
   assert.equal(cleanup.stop.state.profiles.profileSession.owner, null);
   assert.equal(final.enabled, desktopBefore.enabled); assert.deepEqual(final.scope, desktopBefore.scope); assert.deepEqual(final.application, desktopBefore.application);
   assert.equal(final.runtime.backend.cleanupComplete, true); assert.equal(final.runtime.backend.uncertain, false);
   assert.deepEqual(final.runtime.backend.held, []); assert.deepEqual(final.runtime.backend.possibleHeld, []);
  } catch (error) { cleanup.errors.push(String(error)); }
  try { if (target) await closeReceiver(target); } catch (error) { cleanup.errors.push(String(error)); }
  cleanup.receiver = target ? {pid: target.child.pid, exitCode: target.child.exitCode, signalCode: target.child.signalCode} : null;
  try { cleanup.processes = await processInventory(); assert.deepEqual(cleanup.processes.nativeInputWorkers, [], 'Native desktop worker survived Stop'); }
  catch (error) { cleanup.errors.push(String(error)); }
  await writeFile(resolve(artifacts, 'wrapper-cleanup.json'), JSON.stringify(cleanup, null, 2) + '\n');
  if (server) await new Promise(resolve => server.close(resolve));
  await browser.close();
  assert.deepEqual(cleanup.errors, [], 'Native wrapper cleanup failed; inspect wrapper-cleanup.json');
 }
});
