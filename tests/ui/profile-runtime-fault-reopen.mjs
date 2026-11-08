import assert from 'node:assert/strict';
import {execFile} from 'node:child_process';
import {randomUUID} from 'node:crypto';
import {resolve} from 'node:path';
import test from 'node:test';
import {promisify} from 'node:util';
import {browserPage, nativePage, until} from '../../scripts/reactive/cdp.mjs';

// The private launcher supplies the exact Cockpit PID (native) or Vite PID
// (browser-dev) and the executable of the gateway that will own the feeder.
const browserUrl = process.env.OSG_PROFILE_FAULT_BROWSER_URL;
const rootPid = process.env.OSG_PROFILE_FAULT_ROOT_PID;
const executable = process.env.OSG_PROFILE_FAULT_EXECUTABLE;
const transport = browserUrl ? 'browser-dev' : 'native';

async function stopFixtureFeeder() {
  await promisify(execFile)('powershell.exe', ['-NoProfile', '-NonInteractive', '-Command', `
    $ErrorActionPreference = 'Stop'
    $root = Get-CimInstance Win32_Process -Filter "ProcessId = $env:OSG_PROFILE_FAULT_ROOT_PID"
    if (!$root) { throw 'The isolated launcher process exited' }
    $path = $env:OSG_PROFILE_FAULT_EXECUTABLE
    $owners = @($root | Where-Object ExecutablePath -eq $path)
    if ($env:OSG_PROFILE_FAULT_BROWSER_URL) {
      $owners = @(Get-CimInstance Win32_Process -Filter "ParentProcessId = $($root.ProcessId)" |
        Where-Object ExecutablePath -eq $path)
    }
    if ($owners.Count -ne 1) { throw 'Expected exactly one fixture gateway owner' }
    $feeders = @(Get-CimInstance Win32_Process -Filter "ParentProcessId = $($owners[0].ProcessId)" |
      Where-Object { $_.ExecutablePath -eq $path -and $_.CommandLine -match '\\s--osg-vjoy-feeder\\s*$' })
    if ($feeders.Count -ne 1) { throw 'Expected exactly one feeder owned by the fixture' }
    $feeder = Get-Process -Id $feeders[0].ProcessId
    if ($feeder.Path -ne $path -or [Math]::Abs(($feeder.StartTime - $feeders[0].CreationDate).TotalSeconds) -gt 1) {
      throw 'Feeder identity changed'
    }
    Stop-Process -InputObject $feeder
  `], {
    windowsHide:true, timeout:10_000, env:{...process.env, OSG_PROFILE_FAULT_EXECUTABLE:resolve(executable)},
  });
}

test(`${transport} Debug reopens on a real profile fault and on recovery`, {
  skip:process.platform !== 'win32' || !executable, timeout:100_000,
}, async () => {
  assert.match(rootPid ?? '', /^[1-9][0-9]*$/);
  assert.ok(process.env.OSG_SQLITE_DATABASE, 'Use a private Cockpit database');
  if (browserUrl) assert.equal(new URL(browserUrl).hostname, '127.0.0.1');
  const page = browserUrl ? await browserPage(browserUrl) : await nativePage();
  const request = value => page.evaluate(`import('/src/lib/gateway.ts').then(({gatewayRequest}) =>
    gatewayRequest(${JSON.stringify(value)}))`);
  const runtime = () => page.evaluate('window.__runtimeFault?.value');
  const inventory = async () => (await request({command:'system_inventory'})).systemInventory.virtualDevices;
  const navigate = title => until(() => page.evaluate(`(() => {
    const button = document.querySelector('nav[aria-label="Main navigation"] button[title="${title}"]');
    if (!button) return false;
    if (!button.classList.contains('current')) { button.click(); return false; }
    return true;
  })()`));
  const openRecovery = async () => {
    await navigate('Debug');
    await until(() => page.evaluate(`(() => {
      const tab = [...document.querySelectorAll('[aria-label="Debug sections"] [role="tab"]')]
        .find(node => node.textContent.trim() === 'Runtime & recovery');
      if (!tab) return false;
      if (tab.getAttribute('aria-selected') !== 'true') { tab.click(); return false; }
      return true;
    })()`));
  };
  const banner = () => page.evaluate(`(() => {
    const panel = document.querySelector('[role="tabpanel"]:not([hidden]) [aria-label="Runtime profile status"]');
    if (!panel?.checkVisibility()) return null;
    const alerts = [...panel.querySelectorAll('[role="alert"]')];
    return {
      heading:panel.querySelector('.runtime-heading')?.textContent.trim(),
      alerts:alerts.map(node => [...node.querySelectorAll('p')].map(line => line.textContent.trim())),
      recover:alerts.some(node => node.querySelector('button:not([disabled])')?.textContent.trim() === 'Recover'),
    };
  })()`);
  const reads = [];
  let profile, stopTrace = () => {}, faultInduced = false;
  try {
    await page.evaluate(`(async () => {
      const {observeProfileRuntime} = await import('/src/lib/profile-read-models/shared.ts');
      const probe = window.__runtimeFault = {};
      probe.stop = observeProfileRuntime(state => {
        if (state.status === 'ready') probe.value = state.value.profileSession;
      });
    })()`);
    await until(runtime, state => state?.state === 'stopped' && !state.owner && !state.fault);
    const slot = await until(async () => (await inventory()).find(device =>
      device.status === 'free' && device.buttons > 0 && Array.isArray(device.axes)
      && !device.discreteHats && !device.forceFeedback
      && device.buttons + device.axes.length + device.continuousHats <= 128));
    profile = (await request({command:'profile_create', name:'Temporary recovery fixture'})).profiles.document;
    const layout = {
      buttons:Array.from({length:slot.buttons}, (_, index) => 'button-' + (index + 1)),
      axes:slot.axes.map((_, index) => ({portId:'axis-' + (index + 1), range:'bipolar', fallback:0})),
      hats:Array.from({length:slot.continuousHats}, (_, index) => 'hat-' + (index + 1)),
    };
    const document = await page.evaluate(`(async () => {
      const {graphConnect, graphSetVirtual} = await import('/src/lib/profile-graph.ts');
      const document = ${JSON.stringify(profile)};
      const contract = {delivery:'state', schema:{type:'boolean'}, freshnessMs:1000, fallback:false};
      document.resources = [{id:'debug', name:'Debug input', kind:'debug-controller', required:false,
        ports:[{id:'button-1', name:'Button', direction:'output', contract}]}];
      const draft = graphSetVirtual({document, assignments:[]},
        {id:'fixture-vjoy', name:'Recovery fixture', required:true, layout:${JSON.stringify(layout)}});
      return graphConnect(draft, {nodeId:'debug', portId:'button-1'},
        {nodeId:'fixture-vjoy', portId:'button-1'}).document;
    })()`);
    profile = (await request({command:'profile_editor_save', document, assignments:[],
      expectedRevision:profile.revision})).profiles.document;
    await request({command:'profile_virtual_slot_set', id:profile.id, expectedRevision:profile.revision,
      nodeId:'fixture-vjoy', slot:slot.slot});
    await request({command:'profile_apply', id:profile.id, expectedRevision:profile.revision,
      expectedActiveOwner:null});
    await until(runtime, state => ['running', 'degraded'].includes(state?.state) && state.owner?.profileId === profile.id);
    await until(inventory, devices => devices.find(device => device.slot === slot.slot)?.status === 'busy');
    await openRecovery();
    await until(banner, value => value?.heading.includes(profile.name) && value.alerts.length === 0);
    await navigate('Overview');
    assert.equal(await banner(), null);

    await page.call('Network.enable', {maxPostDataSize:65536});
    stopTrace = page.listen(({method, params}) => {
      if (method !== 'Network.requestWillBeSent' || !params.request.postData) return;
      if (!['/gateway_request', '/api/gateway/'].includes(new URL(params.request.url).pathname)) return;
      const payload = JSON.parse(params.request.postData);
      const command = (payload.request ?? payload).command;
      if (['profile_status', 'profile_activation_status'].includes(command)) reads.push(command);
    });
    await request({command:'profile_status'});
    assert.deepEqual(reads.splice(0), ['profile_status'], 'The trace must see the control read');
    await stopFixtureFeeder();
    faultInduced = true;
    const fault = await until(runtime, state => state?.state === 'faulted' && !!state.fault);
    await until(inventory, devices => devices.find(device => device.slot === slot.slot)?.status === 'free');
    await openRecovery();
    const faultBanner = await until(banner, value => value?.recover && value.alerts.flat().includes(fault.fault));
    assert.deepEqual(faultBanner.alerts, [[fault.fault]]);

    await navigate('Overview');
    assert.equal(await banner(), null);
    const recovery = {command:'profile_recover', activationId:randomUUID(), clientId:randomUUID()};
    const reply = await page.evaluate(browserUrl
      ? `fetch('/api/gateway/', {method:'POST', headers:{'Content-Type':'application/json', 'X-OSG-Client':'1'},
          body:${JSON.stringify(JSON.stringify(recovery))}}).then(response => response.json())`
      : `window.__TAURI_INTERNALS__.invoke('gateway_request', {request:${JSON.stringify(recovery)}})`);
    assert.equal(reply.ok, true, reply.error);
    await until(runtime, state => state?.state === 'stopped' && !state.fault && !state.owner);
    await openRecovery();
    await until(banner, value => value?.heading === 'No profile active' && value.alerts.length === 0);
    assert.deepEqual(reads, []);
    console.log(JSON.stringify({transport, fault:fault.fault, slot:slot.slot, reopenedFault:true,
      reopenedRecovered:true, legacyStatusReads:reads.length}));
  } finally {
    stopTrace();
    try {
      if (profile) {
        const current = await runtime();
        if (current?.owner?.profileId === profile.id) await request({command:'profile_stop'});
        if (faultInduced && ['faulted', 'recovery-needed'].includes(current?.state)) {
          await request({command:'profile_recover'});
        }
        await request({command:'profile_delete', id:profile.id, expectedRevision:profile.revision});
      }
    } finally {
      await page.evaluate('window.__runtimeFault?.stop();delete window.__runtimeFault').catch(() => {});
      await page.close();
    }
  }
});
