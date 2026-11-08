// Provision the service and simulator with tests/e2e/run.py --prepare-only.
// Use the same private OSG_SIMULATION_NAMESPACE and OSG_LOCAL_GATEWAYS_ONLY=1
// for Cockpit and this fixture. Point OSG_WORKSPACE_SIMULATOR at the
// simulator binary and OSG_WORKSPACE_SIM_STATE at a private state directory.
// On Linux native, OSG_WORKSPACE_SIM_WEBKIT_URL identifies the private
// WebKitGTK HTTP inspector instead of the browser-dev portal URL.
import assert from 'node:assert/strict';
import {execFile, spawn} from 'node:child_process';
import {randomBytes} from 'node:crypto';
import {createInterface} from 'node:readline';
import test from 'node:test';
import {promisify} from 'node:util';
import {browserPage, until} from '../../scripts/reactive/cdp.mjs';
import {webKitPage} from '../../scripts/reactive/webkit.mjs';

const url = process.env.OSG_WORKSPACE_SIM_BROWSER_URL;
const nativeInspector = process.env.OSG_WORKSPACE_SIM_WEBKIT_URL;
const probeModule = process.env.OSG_WORKSPACE_SIM_PROBE_MODULE ?? '/src/lib/workspace-read-models/shared.ts';
const transport = nativeInspector ? 'native-linux' : 'browser-dev';
const simulator = process.env.OSG_WORKSPACE_SIMULATOR;
const namespace = process.env.OSG_SIMULATION_NAMESPACE;
const stateRoot = process.env.OSG_WORKSPACE_SIM_STATE;
const distro = process.env.OSG_WORKSPACE_SIM_WSL;
const execute = promisify(execFile);

async function startGateway(uid) {
  const args = [simulator, '--identity', uid, '--state-dir', `${stateRoot}/${uid}`];
  const wslArgs = ['-d', distro, '--exec'];
  const child = distro
    ? spawn('wsl.exe', [...wslArgs, 'sh', '-c', 'printf "%s\\n" "$$"; exec "$@"',
      'workspace-sim', 'env', `OSG_SIMULATION_NAMESPACE=${namespace}`, ...args], {windowsHide:true})
    : spawn(simulator, args.slice(1));
  let failure, stderr = '', linuxPid;
  child.stderr.on('data', bytes => { stderr = (stderr + bytes).slice(-4096); });
  child.once('error', error => { failure = error; });
  const exited = new Promise(resolve => child.once('close', (code, signal) => resolve({code, signal})));
  const lines = createInterface({input:child.stdout});
  lines.once('line', value => { if (/^[1-9][0-9]*$/.test(value)) linuxPid = value; });
  await until(() => {
    if (failure || child.exitCode !== null) throw failure ?? new Error(`Gateway simulator exited: ${stderr}`);
    return distro ? linuxPid : child.pid;
  });
  return {
    id: `stm32:${uid}`,
    async stop() {
      if (child.exitCode === null && child.signalCode === null) {
        if (distro) {
          await execute('wsl.exe', [...wslArgs, 'sh', '-c',
            'test "$(readlink -f "/proc/$1/exe")" = "$(readlink -f "$2")" && tr "\\000" "\\n" < "/proc/$1/cmdline" | grep -Fxq -- "$3" && kill -TERM "$1"',
            'workspace-sim', linuxPid, simulator, uid], {windowsHide:true, timeout:5000});
        } else child.kill('SIGTERM');
      }
      const result = await exited;
      assert.equal(result.code, 0, stderr || `Gateway simulator exited on ${result.signal}`);
    },
  };
}

test(`${transport} shell and Devices follow actual simulated gateway arrival and replacement`, {
  skip: !url && !nativeInspector, timeout: 120_000,
}, async () => {
  assert.ok(simulator && namespace && stateRoot, 'Use an isolated simulator namespace and state directory');
  assert.match(namespace, /^[A-Za-z0-9._-]{1,31}$/);
  assert.equal(process.env.OSG_LOCAL_GATEWAYS_ONLY, '1');
  assert.ok(!url || !nativeInspector, 'Choose one Cockpit transport');
  const page = nativeInspector ? await webKitPage(nativeInspector) : await browserPage(url);
  const forbidden = ['discover', 'ports', 'status', 'connect', 'start', 'config_read', 'action_describe'];
  const unexpected = [];
  const stopTrace = page.listen?.(({method, params}) => {
    if (method !== 'Network.requestWillBeSent' || !params.request.postData) return;
    if (new URL(params.request.url).pathname !== '/api/gateway/') return;
    const {command} = JSON.parse(params.request.postData);
    if (forbidden.includes(command)) unexpected.push(command);
  }) ?? (() => {});
  const navigate = label => until(() => page.evaluate(`(() => {
    const button = document.querySelector('nav[aria-label="Main navigation"] button[title=${JSON.stringify(label)}]');
    if (!button) return false;
    if (!button.classList.contains('current')) { button.click(); return false; }
    return true;
  })()`));
  const overviewCount = (target = page) => target.evaluate(`document.querySelector('.hardware-counts strong')?.textContent`);
  const tile = id => page.evaluate(`document.querySelector('[data-component-id="gateway:${id}"]')?.getAttribute('aria-label')`);
  const connected = label => label?.includes(' · Connected');
  const ready = (id, present) => page.evaluate(`(() => {
    const state = window.__workspaceProbe?.state;
    if (state?.status !== 'ready') return false;
    const entry = state.value.entries.find(entry => entry.id === ${JSON.stringify(id)});
    return Boolean(entry?.present && entry.state?.connected) === ${present};
  })()`);
  const gateways = [];
  try {
    if (url) await page.call('Network.enable', {maxPostDataSize:65536});
    await page.evaluate(`(async () => {
      const {observeWorkspaceState} = await import(${JSON.stringify(probeModule)});
      const probe = window.__workspaceProbe = {};
      probe.stop = observeWorkspaceState(state => probe.state = state);
    })()`);
    await navigate('Overview');
    await until(() => page.evaluate(`(() => {
      const state = window.__workspaceProbe.state;
      return state?.status === 'ready' && state.value.ports.length === 0;
    })()`));
    await until(overviewCount, count => count === '0');
    if (nativeInspector) await page.evaluate(`(async () => {
      const probe = window.__workspaceProbe;
      const api = window.__TAURI_INTERNALS__;
      const forbidden = ${JSON.stringify(forbidden)};
      const gatewayEndpoint = api.convertFileSrc('gateway_request', 'ipc');
      const fetchOriginal = window.fetch;
      const postOriginal = window.ipc.postMessage;
      probe.requests = [];
      const record = payload => {
        const command = payload?.request?.command;
        if (forbidden.includes(command)) probe.requests.push(command);
      };
      const fetchTraced = function(url, options) {
        if (url === gatewayEndpoint) record(JSON.parse(options.body));
        return Reflect.apply(fetchOriginal, this, arguments);
      };
      const postTraced = function(data) {
        const {cmd, payload} = JSON.parse(data);
        if (cmd === 'gateway_request') record(payload);
        return Reflect.apply(postOriginal, this, arguments);
      };
      window.fetch = fetchTraced;
      window.ipc.postMessage = postTraced;
      probe.restore = () => {
        if (window.fetch === fetchTraced) window.fetch = fetchOriginal;
        if (window.ipc.postMessage === postTraced) window.ipc.postMessage = postOriginal;
      };
      const result = await api.invoke('gateway_request', {request:{command:'ports', clientId:crypto.randomUUID()}});
      if (!result.ok || !probe.requests.includes('ports')) throw Error('Native IPC trace did not capture the control request.');
      probe.requests.length = 0;
    })()`);
    await navigate('Devices');
    const first = await startGateway(randomBytes(12).toString('hex'));
    gateways.push(first);
    await until(() => ready(first.id, true));
    await until(() => tile(first.id), connected);
    await navigate('Overview');
    await until(overviewCount, count => count === '1');

    await navigate('Games');
    await first.stop();
    await until(() => ready(first.id, false));
    const replacement = await startGateway(randomBytes(12).toString('hex'));
    gateways.push(replacement);
    await until(() => ready(replacement.id, true));
    await navigate('Overview');
    await until(overviewCount, count => count === '1');
    await navigate('Devices');
    await until(() => tile(replacement.id), connected);
    assert.ok(!connected(await tile(first.id)), 'A retired gateway must not reopen as connected');
    if (url) {
      const peer = await page.openPeer();
      await until(() => overviewCount(peer), count => count === '1');
    }

    await replacement.stop();
    await until(() => ready(replacement.id, false));
    await until(() => tile(replacement.id), label => !connected(label));
    await navigate('Overview');
    await until(overviewCount, count => count === '0');
    const polling = nativeInspector ? await page.evaluate('window.__workspaceProbe.requests') : unexpected;
    assert.deepEqual(polling, []);
    console.log(JSON.stringify({transport, arrival:true, replacementWhileClosed:true, reopenedCurrent:true, freshShell:!!url,
      visibleRemoval:true, pollingRequests:polling.length}));
  } catch (error) {
    const state = await page.evaluate(`window.__workspaceProbe?.state`).catch(() => null);
    throw new Error(`Workspace state at failure: ${JSON.stringify(state)}`, {cause:error});
  } finally {
    stopTrace();
    try {
      await page.evaluate(`(() => { window.__workspaceProbe?.restore?.(); window.__workspaceProbe?.stop(); delete window.__workspaceProbe; })()`);
    } finally {
      const cleanup = await Promise.allSettled([page.close(), ...gateways.map(gateway => gateway.stop())]);
      for (const result of cleanup) if (result.status === 'rejected') throw result.reason;
    }
  }
});
