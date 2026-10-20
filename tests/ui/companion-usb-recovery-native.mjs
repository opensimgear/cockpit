// Real-device recovery acceptance. The only mutation is a bounded external pause
// of the exact native dev process, which makes the physical USB peer miss its
// heartbeat. No Cockpit reconnect, profile, assignment or panel command is sent.
import test from 'node:test';
import assert from 'node:assert/strict';
import { createRequire } from 'node:module';
import { mkdir, writeFile } from 'node:fs/promises';
import { execFile } from 'node:child_process';
import { promisify } from 'node:util';
import { resolve } from 'node:path';

const require = createRequire(import.meta.url);
const { ws: RawSocket } = createRequire(require.resolve(process.env.OSG_PLAYWRIGHT_MODULE ?? 'playwright'))('playwright-core/lib/utilsBundle');
const run = promisify(execFile);
const sleep = ms => new Promise(resolve => setTimeout(resolve, ms));
const artifacts = resolve('.research/companion-usb-recovery-native');

async function connectNative() {
  const targets = await (await fetch(`${process.env.OSG_NATIVE_CDP_URL ?? 'http://127.0.0.1:9222'}/json/list`, { signal: AbortSignal.timeout(5000) })).json();
  const endpoint = process.env.OSG_NATIVE_PAGE_WS ?? targets.find(target => target.type === 'page' && /127\.0\.0\.1:1420|localhost:1420|tauri\.localhost/.test(target.url) && !/profile-(?:progress|activation-window)/.test(target.url))?.webSocketDebuggerUrl;
  assert.ok(endpoint, 'Start the actual native dev Cockpit with CDP enabled.');
  const socket = new RawSocket(endpoint, { handshakeTimeout: 8000 });
  await new Promise((resolve, reject) => { socket.once('open', resolve); socket.once('error', reject); });
  let sequence = 0;
  const pending = new Map();
  socket.on('error', () => {});
  socket.on('message', bytes => {
    const message = JSON.parse(bytes.toString()), operation = pending.get(message.id);
    if (!operation) return;
    clearTimeout(operation.timer); pending.delete(message.id);
    if (message.error) operation.reject(new Error(message.error.message)); else operation.resolve(message.result);
  });
  socket.on('close', () => {
    for (const operation of pending.values()) { clearTimeout(operation.timer); operation.reject(new Error('Dev Cockpit disconnected.')); }
    pending.clear();
  });
  return {
    async evaluate(expression) {
      const result = await new Promise((resolve, reject) => {
        const id = ++sequence, timer = setTimeout(() => { pending.delete(id); reject(new Error('Native request timed out.')); }, 10000);
        pending.set(id, { resolve, reject, timer });
        socket.send(JSON.stringify({ id, method: 'Runtime.evaluate', params: { expression, returnByValue: true, awaitPromise: true } }));
      });
      if (result.exceptionDetails) throw new Error(result.exceptionDetails.exception?.description ?? result.exceptionDetails.text);
      return result.result.value;
    },
    close() { socket.close(); }
  };
}

async function missPhysicalHeartbeats() {
  // The child process owns the complete suspend/resume transaction. Its finally
  // resumes Cockpit before any JavaScript assertion or status poll can fail.
  const processId = Number(process.env.OSG_USB_RECOVERY_PROCESS_ID);
  assert.ok(Number.isSafeInteger(processId) && processId > 0, 'Set the exact native debug Cockpit process ID.');
  const script = String.raw`
$ErrorActionPreference = 'Stop'
$taskProcessId = [int]$env:OSG_USB_RECOVERY_PROCESS_ID
$taskExpectedPath = [IO.Path]::GetFullPath($env:OSG_USB_RECOVERY_EXPECTED_PATH)
$taskProcess = Get-Process -Id $taskProcessId -ErrorAction Stop
if (![string]::Equals($taskProcess.Path, $taskExpectedPath, [StringComparison]::OrdinalIgnoreCase)) {
  throw 'The selected process is not this workspace native debug Cockpit.'
}
Add-Type -TypeDefinition @'
using System;
using System.Runtime.InteropServices;
public static class CockpitUsbHeartbeatPause {
  [DllImport("kernel32.dll", SetLastError=true)] public static extern IntPtr OpenProcess(uint access, bool inherit, int processId);
  [DllImport("ntdll.dll")] public static extern int NtSuspendProcess(IntPtr process);
  [DllImport("ntdll.dll")] public static extern int NtResumeProcess(IntPtr process);
  [DllImport("kernel32.dll")] public static extern bool CloseHandle(IntPtr handle);
}
'@
$taskHandle = [CockpitUsbHeartbeatPause]::OpenProcess(0x0800, $false, $taskProcessId)
if ($taskHandle -eq [IntPtr]::Zero) { throw 'Cannot open the native Cockpit process for the bounded heartbeat interruption.' }
$taskSuspended = $false
$taskStarted = [DateTime]::UtcNow
try {
  if ([CockpitUsbHeartbeatPause]::NtSuspendProcess($taskHandle) -ne 0) { throw 'Cannot pause the native Cockpit process.' }
  $taskSuspended = $true
  Start-Sleep -Milliseconds 3000
} finally {
  try {
    if ($taskSuspended -and [CockpitUsbHeartbeatPause]::NtResumeProcess($taskHandle) -ne 0) { throw 'Could not resume the native Cockpit process.' }
  } finally {
    [void][CockpitUsbHeartbeatPause]::CloseHandle($taskHandle)
  }
}
[ordered]@{ processId = $taskProcessId; executablePath = $taskExpectedPath; pausedMs = ([DateTime]::UtcNow - $taskStarted).TotalMilliseconds; resumed = $true } | ConvertTo-Json -Compress
`;
  const result = await run('powershell.exe', ['-NoProfile', '-NonInteractive', '-Command', script], {
    env: { ...process.env, OSG_USB_RECOVERY_EXPECTED_PATH: resolve('src-tauri/target/debug/osg-cockpit.exe') },
    windowsHide: true,
    timeout: 15000,
    maxBuffer: 64 * 1024
  });
  const interruption = JSON.parse(result.stdout.trim());
  assert.equal(interruption.resumed, true);
  assert.ok(interruption.pausedMs >= 3000 && interruption.pausedMs < 5000, 'The bounded interruption must outlast the physical peer heartbeat request.');
  return interruption;
}

test('Sunton automatically restores the active panel and real game feedback after failed USB heartbeats', { skip: !process.env.OSG_USB_RECOVERY_PROCESS_ID, timeout: 120000 }, async () => {
  assert.equal(process.platform, 'win32', 'This fault injection targets the real Windows native dev process.');
  await mkdir(artifacts, { recursive: true });
  const native = await connectNative();
  const evidence = { actualNativeCockpit: true, physicalUsbCompanion: true, syntheticTelemetry: false, automaticRecovery: true, samples: [], commands: [], startedAt: new Date().toISOString() };
  const readOnly = new Set(['profile_status', 'companion_status', 'companion_usb_status', 'profile_assignments_get', 'profile_resources_list', 'profile_runtime_inspect']);
  const request = async value => {
    assert.ok(readOnly.has(value.command), 'Recovery acceptance must never issue an app mutation that could implement recovery.');
    if (!evidence.commands.includes(value.command)) evidence.commands.push(value.command);
    const reply = await native.evaluate(`window.__TAURI_INTERNALS__.invoke('gateway_request',{request:${JSON.stringify({ ...value, clientId: 'companion-usb-auto-recovery-native' })}})`);
    assert.equal(reply.ok, true, reply.error ?? value.command);
    return reply.state;
  };
  const status = async () => (await request({ command: 'profile_status' })).profiles.profileSession;
  const links = usb => usb.devices ?? [usb];
  const assignmentShape = companions => companions.host.instances.map(({ instanceId, companionId, publicationHash, profileOwned }) => ({ instanceId, companionId, publicationHash, profileOwned })).sort((a, b) => a.instanceId.localeCompare(b.instanceId));
  const savedShape = ({ document, assignments }) => ({ document, assignments });
  const endpoint = value => JSON.stringify([value.nodeId, value.portId]);
  let started, owner, companionId, desiredHash, baselinePeer, baselineAssignments, binding, last;
  try {
    const before = await status();
    assert.equal(before.state, 'running', 'Leave the actual game profile active before testing.');
    assert.equal(before.fault, null);
    assert.deepEqual(before.degradation, []);
    assert.ok(before.owner?.profileId);
    owner = before.owner;
    if (process.env.OSG_USB_RECOVERY_PROFILE_ID) assert.equal(owner.profileId, process.env.OSG_USB_RECOVERY_PROFILE_ID);
    const [companionState, usbState, savedState, inventoryState] = await Promise.all([
      request({ command: 'companion_status' }),
      request({ command: 'companion_usb_status' }),
      request({ command: 'profile_assignments_get', id: owner.profileId }),
      request({ command: 'profile_resources_list', offset: 0, limit: 128 })
    ]);
    const sunton = links(usbState.companions.usb).find(row => row.device?.boardProfile === 'sunton-3248s035r' && (!process.env.OSG_USB_RECOVERY_COMPANION_ID || row.device?.companionId === process.env.OSG_USB_RECOVERY_COMPANION_ID));
    assert.equal(sunton?.connected, true, 'The actual paired Sunton must start healthy.');
    assert.equal(sunton.ready, true);
    assert.equal(sunton.device.status, 'live-armed');
    assert.equal(sunton.device.operation?.phase, 'ready');
    companionId = sunton.device.companionId;
    baselinePeer = sunton.device;
    baselineAssignments = assignmentShape(companionState.companions);
    const initialInstance = companionState.companions.host.instances.find(row => row.companionId === companionId);
    assert.equal(initialInstance?.profileOwned, true);
    assert.equal(initialInstance.armed, true);
    assert.equal(initialInstance.connected, true);
    desiredHash = initialInstance.publicationHash;
    const source = inventoryState.profileResources.resources.find(row => row.resourceType === 'companion' && row.attributes.companionId === companionId && row.attributes.componentKind === 'paired-companion');
    assert.ok(source);
    const local = savedState.profiles.assignments.find(row => row.sourceId === source.id);
    assert.ok(local);
    assert.equal(savedState.profiles.document.companionAssignments.find(row => row.resourceId === local.resourceId)?.publicationHash, desiredHash);
    binding = savedState.profiles.document.bindings.find(row => row.to.nodeId === local.resourceId && savedState.profiles.document.resources.some(resource => resource.id === row.from.nodeId && resource.kind === 'game'));
    assert.ok(binding, 'Keep the actual integration-speed to companion binding active for recovery acceptance.');
    evidence.owner = owner;
    evidence.before = { sunton, assignments: baselineAssignments, binding };

    evidence.interruption = await missPhysicalHeartbeats();
    started = Date.now();
    let healthySince, healthySession, healthyWrites, healthyWriteFailures, feedbackPairs = 0, lastRecorded = -1000, observedFailure = false, recovered = false;
    while (Date.now() - started < 90000) {
      const [profileState, companionState, usbState, inspectionState] = await Promise.all([
        request({ command: 'profile_status' }),
        request({ command: 'companion_status' }),
        request({ command: 'companion_usb_status' }),
        request({ command: 'profile_runtime_inspect', owner, offset: 0, limit: 64 })
      ]);
      const profile = profileState.profiles.profileSession,
        companions = companionState.companions,
        usb = links(usbState.companions.usb).find(row => row.device?.companionId === companionId),
        peer = usb?.device,
        current = companions.host.instances.find(row => row.companionId === companionId),
        elapsedMs = Date.now() - started,
        inspected = inspectionState.profileInspection,
        from = inspected.ports.find(port => endpoint(port.endpoint) === endpoint(binding.from)),
        to = inspected.ports.find(port => endpoint(port.endpoint) === endpoint(binding.to));
      last = { elapsedMs, profile, usb, instance: current, source: from, destination: to };
      assert.deepEqual(profile.owner, owner, 'The exact native profile owner must survive; no deactivate/reactivate is permitted.');
      assert.ok(['running', 'degraded'].includes(profile.state), 'A USB outage may degrade the branch while preserving the active profile.');
      assert.equal(profile.fault, null);
      assert.ok(!(profile.degradation ?? []).some(message => message.includes('Start again explicitly')), 'The active USB branch must describe automatic recovery instead of instructing profile reactivation.');
      assert.deepEqual(assignmentShape(companions), baselineAssignments, 'Every companion assignment, including the other screen, must remain unchanged.');
      assert.deepEqual(inspected.owner, owner);
      if ((usb?.recovery && usb.recovery.phase !== 'recovered') || !usb?.connected || !current?.connected || !current?.armed || peer?.status !== 'live-armed' || peer?.operation?.phase !== 'ready' || profile.degradation?.length) observedFailure = true;
      const healthy = usb?.connected && usb.ready && usb.statusAgeMs < 1500 && peer?.status === 'live-armed' && peer?.operation?.phase === 'ready'
        && peer.sessionId && peer.sessionId !== baselinePeer.sessionId && peer.publicationHash === desiredHash
        && current?.connected && current.armed && current.publicationHash === desiredHash && current.sessionId === peer.sessionId
        && current.generation === peer.generation && profile.state === 'running' && !(profile.degradation?.length);
      if (elapsedMs - lastRecorded >= 1000 || !healthy) {
        evidence.samples.push(last);
        lastRecorded = elapsedMs;
      }
      if (healthy) {
        assert.equal(current.profileOwned, true);
        assert.equal(current.transport, 'usb');
        assert.equal(peer.operation.instanceId, current.instanceId);
        assert.equal(peer.operation.publicationHash, desiredHash);
        assert.equal(peer.operation.sessionId, peer.sessionId);
        assert.equal(peer.operation.generation, peer.generation);
        assert.ok(!peer.operation.errorCode);
        assert.ok(!usb.lastHostError, 'Recovered USB must clear the old host error.');
        assert.ok(Object.values(current.values ?? {}).every(value => value === false), 'Recovered controls must have a new released neutral baseline.');
        if (healthySession !== peer.sessionId) {
          healthySession = peer.sessionId;
          healthySince = undefined;
          healthyWrites = usb.serialIo?.writesCompleted;
          healthyWriteFailures = usb.serialIo?.writeFailures;
          feedbackPairs = 0;
        }
        healthySince ??= Date.now();
        evidence.recoveryMs ??= elapsedMs;
        if (from?.sample.status === 'known' && to?.sample.status === 'known') {
          assert.deepEqual(to.sample, from.sample, 'Same-capture real game value must resume at the companion destination.');
          assert.equal(to.observedUs, from.observedUs);
          feedbackPairs++;
        }
        if (Date.now() - healthySince >= 15000 && feedbackPairs >= 15) {
          assert.ok(Number.isFinite(healthyWrites) && usb.serialIo?.writesCompleted > healthyWrites, 'Real USB writes must continue after restoring the panel.');
          assert.equal(usb.serialIo?.writeFailures, healthyWriteFailures, 'No new write error may be concealed during the recovered session.');
          assert.equal(usb.serialIo?.lastWriteError, null);
          recovered = true;
          break;
        }
      } else { healthySince = undefined; feedbackPairs = 0; }
      await sleep(200);
    }
    assert.ok(recovered, 'The active Sunton panel must recover and retain real feedback for fifteen seconds without explicit recovery.');
    assert.ok(observedFailure, 'Observe an actual failed or recovering USB branch before accepting its restoration.');
    assert.notEqual(last.usb.device.sessionId, baselinePeer.sessionId, 'The physical peer must establish a fresh session after missed heartbeats.');
    evidence.observedFailure = observedFailure;
    evidence.feedbackPairs = feedbackPairs;
    evidence.final = last;
    assert.deepEqual(savedShape((await request({ command: 'profile_assignments_get', id: owner.profileId })).profiles), savedShape(savedState.profiles), 'The saved profile and local assignments must stay byte-for-byte equivalent.');
    evidence.result = 'passed';
    console.log(JSON.stringify({ result: evidence.result, observedFailure, recoveryMs: evidence.recoveryMs, feedbackPairs, previousSession: baselinePeer.sessionId, recoveredSession: last.usb.device.sessionId, owner, artifacts }));
  } catch (error) {
    evidence.error = error.stack;
    evidence.final = last;
    throw error;
  } finally {
    evidence.observedMs = started ? Date.now() - started : 0;
    await writeFile(resolve(artifacts, 'evidence.json'), JSON.stringify(evidence, null, 2) + '\n');
    native.close();
  }
});
