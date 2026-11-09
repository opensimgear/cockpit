import assert from 'node:assert/strict';
import {execFile} from 'node:child_process';
import {mkdir, writeFile} from 'node:fs/promises';
import {resolve} from 'node:path';
import {setTimeout as delay} from 'node:timers/promises';
import {promisify} from 'node:util';
import {until} from '../../scripts/reactive/cdp.mjs';
import {readSoakPanel} from './dcs-soak-panel.mjs';
import {traceSoakPresentation, traceSoakRequests} from './dcs-soak-metrics.mjs';

const fullSeconds = 30 * 60;
const mib = 1024 ** 2;

export function soakSeconds(raw) {
  if (!raw) return 0;
  const seconds = Number(raw);
  assert.ok(Number.isInteger(seconds) && seconds >= 15 && seconds <= fullSeconds,
    'OSG_DCS_COMBINED_SOAK_SECONDS must be an integer between 15 and 1800');
  return seconds;
}

async function nativeMemory() {
  const pid = Number(process.env.OSG_DCS_SOAK_COCKPIT_PID);
  assert.ok(Number.isSafeInteger(pid) && pid > 0 && pid !== process.pid,
    'Set OSG_DCS_SOAK_COCKPIT_PID to the disposable Cockpit process ID');
  const {stdout} = await promisify(execFile)('powershell.exe', ['-NoProfile', '-NonInteractive',
    '-Command', `$p=Get-Process -Id ${pid} -ErrorAction Stop; $p.PrivateMemorySize64`],
  {windowsHide:true,timeout:10000});
  const bytes = Number(stdout.trim());
  assert.ok(Number.isSafeInteger(bytes) && bytes > 0, stdout);
  return bytes;
}

async function startReaders(cdp, owner, nodeId, biosId) {
  await cdp.evaluate(`(async()=>{
    const {createNativeObservationTransport}=await import('/src/lib/streams/native.ts');
    const {SharedObservations}=await import('/src/lib/streams/shared.ts');
    const {profileNodeSamplesTopic}=await import('/src/lib/profile-samples/models.ts');
    const transport=createNativeObservationTransport(), cancel=new AbortController();
    const observations=new SharedObservations(transport);
    const trace=window.__dcsSoakReaders={slow:0,fast:0,runtime:0,error:null,ownerError:null,
      telemetry:[],telemetryStatus:null,
      close(){cancel.abort();observations.close();}};
    const owner=${JSON.stringify(owner)};
    const topic=await profileNodeSamplesTopic(owner,${JSON.stringify(nodeId)});
    const health=state=>{
      if(state.status==='error'||state.status==='lost')trace.error=state.error??state.status;
      return state.status==='ready';
    };
    transport.open(topic,{
      frame(_envelope,ack){if(++trace.slow===1)trace.release=ack;else void ack();},
      error(error){trace.error=error;}
    },cancel.signal);
    observations.subscribeReadonly(topic,state=>{
      if(!health(state))return;
      const sample=state.frame.payload.ports.find(port=>port.endpoint.portId==='button-1')?.sample;
      if(sample?.status==='known'){trace.fast++;trace.value=sample.value?.value;}
    });
    const telemetryTopic=await profileNodeSamplesTopic(owner,'telemetry');
    const ports=['flight.ownship.inAir',${JSON.stringify(biosId)}];
    observations.subscribeReadonly(telemetryTopic,state=>{
      if(!health(state))return;
      const samples=ports.map(id=>{
        const port=state.frame.payload.ports.find(row=>row.endpoint.portId===id);
        return {id,status:port?.sample?.status??'missing',observedUs:port?.observedUs};
      });
      const signature=samples.map(row=>row.status).join('/');
      if(signature===trace.telemetryStatus)return;
      trace.telemetryStatus=signature;
      trace.telemetry.push({at:performance.now(),samples,revision:state.frame.revision});
      if(trace.telemetry.length>16)trace.telemetry.shift();
    });
    observations.subscribeReadonly('profiles.runtime',state=>{
      if(!health(state))return;
      const {profileSession:session}=state.frame.payload;
      if(!session)return;
      trace.runtime++;
      if(session.state!=='running'||session.fault||!session.owner
        ||Object.entries(owner).some(([key,value])=>session.owner[key]!==value)){
        trace.ownerError??={state:session.state,fault:session.fault,owner:session.owner,
          degradation:session.degradation,at:performance.now()};
      }
    });
  })()`);
}

const readerStatus = cdp => cdp.evaluate(`(({slow,fast,value,runtime,error,ownerError})=>
  ({slow,fast,value,runtime,error,ownerError}))(window.__dcsSoakReaders)`);

const recentTelemetry = cdp => cdp.evaluate('window.__dcsSoakReaders.telemetry');

async function navigate(cdp, name) {
  const selector = JSON.stringify(`nav[aria-label="Main navigation"] button[title="${name}"]`);
  await cdp.evaluate(`document.querySelector(${selector}).click()`);
  await until(() => cdp.evaluate(`document.querySelector(${selector}).classList.contains('current')`));
}

function checkMemory(samples) {
  if (samples.length < 3) return;
  for (const [index, name] of ['Cockpit private bytes', 'panel 1 JS heap', 'panel 2 JS heap'].entries()) {
    const values = samples.map(sample => sample.bytes[index]);
    assert.ok(values.every(bytes => Number.isFinite(bytes) && bytes > 0), name);
    // Allow GC and cache warming. Compare the last five-minute peak against the
    // first five-minute peak after the first two minutes of the measured run.
    const first = Math.max(...values.slice(4,14));
    const last = Math.max(...values.slice(-10));
    assert.ok(last - first <= 64 * mib, `${name} grew from ${first} to ${last} bytes`);
  }
}

export async function runDcsSoak({cdp, request, panels, owner, seconds, directory, status, live,
  setAirborne, submitOwnship, sendDebug, openMcp, nodeId, fixtureClientId, biosId, beginDualRates}) {
  assert.equal(panels.length, 2);
  const root = resolve(directory, 'discovery-root');
  await mkdir(root);
  const evidence = {requestedSeconds:seconds, qualifiedDuration:false, scans:0, inventory:0,
    samples:0, mcpSamples:0, nav:[], memory:[], owner};
  const panelSnapshots = () => Promise.all(panels.map(panel => readSoakPanel(panel.page)));
  const instances = async () => {
    const {host} = (await request({command:'companion_status',companion:{}})).companions;
    return panels.map(({peer}) => {
      const instance = host.instances.find(row => row.companionId === peer.companionId && row.connected);
      assert.ok(instance?.armed && instance.sessionId, `Panel ${peer.companionId} has no armed session`);
      const {instanceId,sessionId,generation,publicationHash} = instance;
      return {instanceId,sessionId,generation,publicationHash};
    });
  };
  const ready = async value => {
    let rows;
    try {
      return await until(async () => {
        rows = await panelSnapshots();
        return rows.every(row => row.panelReady && row.documentVisible && !row.panelErrorText
          && row.latest?.status === 'fresh' && row.latest.value === value
          && row.latestPaint?.lit === value) && rows;
      }, Boolean, 5000);
    } catch (error) {
      throw new Error(`Expected both panel lamps to paint ${value}: ${JSON.stringify(rows)}`, {cause:error});
    }
  };
  setAirborne(true);
  await live();
  await ready(true);
  setAirborne(false);
  await live();
  await until(async () => (await request({command:'game_get_state'})).games.discovery?.status !== 'scanning',
    Boolean, 90000).catch(error => {throw new Error('Initial discovery did not finish before the soak', {cause:error});});
  const baseline = await ready(false);
  evidence.instances = await instances();
  if (!beginDualRates) await sendDebug(owner, false);
  await startReaders(cdp, owner, nodeId, biosId);
  let traffic, presentation;
  try {
    const firstReader = await until(async () => {
      const state = await readerStatus(cdp);
      assert.equal(state.error, null);
      return state.slow === 1 && state.fast > 0 && state.runtime > 0 && state;
    });
    const mcp = await openMcp();
    let lastMcp;
    const readMcp = async value => {
      const sample = await Promise.race([mcp.sample(value), delay(5000, null, {ref:false})]);
      assert.ok(sample, `MCP sample did not report ${value} within five seconds`);
      if (lastMcp) {
        assert.equal(sample.generation, lastMcp.generation);
        assert.ok(BigInt(sample.revision) > BigInt(lastMcp.revision), 'MCP revision did not advance');
      }
      lastMcp = sample;
    };
    presentation = await traceSoakPresentation(panels);
    traffic = await traceSoakRequests(cdp, fixtureClientId);
    const rates = await beginDualRates?.();
    if (rates) {
      mcp.drainFast();
      await delay(Math.max(0, Number(rates.startNs-process.hrtime.bigint())/1e6));
    }
    const start = performance.now();
    let nextActivity = 0, nextMemory = 0, round = 0, scan, value = false, lastFast = firstReader.fast, lastMcpCount = 0;
    const views = ['Devices', 'Games', 'Debug', 'Overview'];
    while (performance.now() - start < seconds * 1000 || scan) {
      const elapsed = performance.now() - start;
      const finishing = elapsed >= seconds * 1000;
      if (elapsed >= nextActivity && !finishing) {
        for (const view of views) {
          traffic.setView(view);
          await navigate(cdp, view);
          if (!evidence.nav.includes(view)) evidence.nav.push(view);
          if (view === 'Devices') {
            const {systemInventory} = await request({command:'system_inventory',refresh:true,limit:1});
            assert.ok(Array.isArray(systemInventory.virtualDevices));
            evidence.inventory++;
          }
          if (view === 'Games' && !scan) {
            const {games} = await request({command:'game_discovery_start',paths:[root]});
            assert.ok(games.discovery.scanId);
            scan = {id:games.discovery.scanId,deadline:performance.now()+90000};
          }
        }
        nextActivity += 30000;
      }
      if (scan) {
        const {games} = await request({command:'game_get_state'});
        assert.equal(games.discovery.scanId, scan.id);
        assert.ok(performance.now() < scan.deadline, 'Game discovery exceeded 90 seconds');
        assert.ok(['scanning','ready'].includes(games.discovery.status), JSON.stringify(games.discovery));
        if (games.discovery.status === 'ready') {
          evidence.scans++;
          scan = undefined;
        }
      }
      value = !value;
      rates?.check();
      const [submission] = await Promise.all([submitOwnship(value), !rates && sendDebug(owner, value)]);
      const rows = await ready(value);
      presentation.record(submission, value, rows);
      if (!rates) {
        await readMcp(value);
        evidence.mcpSamples++;
      }
      rows.forEach((row, index) => {
        for (const key of ['nonFreshAfterFirstFresh','identityChanges','bindingGenerationAdvances',
          'bindingGenerationRegressions','sequenceRegressions','feedbackClosures','workerErrors','invalidCounters']) {
          assert.equal(row[key], baseline[index][key], `${key}: ${JSON.stringify(row)}`);
        }
      });
      await live();
      const session = (await status()).profileSession;
      assert.equal(session.state, 'running', JSON.stringify(session));
      assert.ok(!session.fault, JSON.stringify(session));
      assert.deepEqual(session.owner, owner);
      assert.deepEqual(await instances(), evidence.instances);
      const checkReaders = async () => {
        const state = await readerStatus(cdp);
        assert.equal(state.error, null);
        assert.equal(state.ownerError, null, JSON.stringify(state.ownerError));
        assert.equal(state.slow, 1);
        if (rates) {
          const updates = mcp.fastUpdates();
          if (process.hrtime.bigint() < rates.endNs && (state.fast <= lastFast || updates <= lastMcpCount)) return null;
          lastMcpCount = updates;
          evidence.mcpSamples = updates;
          return state;
        }
        return state.fast > lastFast && state.value === value && state;
      };
      const readers = await until(checkReaders, Boolean, 3000).catch(error => {
        throw new Error(`Fast vJoy sample reader stalled at ${lastFast}`, {cause:error});
      });
      lastFast = readers.fast;
      evidence.samples++;
      if (elapsed >= nextMemory) {
        evidence.memory.push({elapsedMs:Math.round(elapsed),
          bytes:[await nativeMemory(),...rows.map(row => row.jsHeap?.usedBytes)]});
        nextMemory += 30000;
      }
      if (elapsed >= round * 60000) {
        console.log(`DCS soak ${Math.floor(elapsed/1000)}s, ${evidence.samples} health checks, ${evidence.scans} discovery scans`);
        round++;
      }
      if (!finishing) await delay(Math.min(1800, Math.max(0, seconds * 1000 - (performance.now() - start))));
    }
    if (rates) {
      await rates.offered;
      assert.ok(evidence.mcpSamples > 0 && lastFast > firstReader.fast);
      const leaseRequest = {
        command: 'qualification_companion_leases', clientId: fixtureClientId, sessions: evidence.instances,
      };
      const inputLeases = await cdp.evaluate(`window.__TAURI_INTERNALS__.invoke('gateway_request',
        {request:${JSON.stringify(leaseRequest)}})`);
      assert.equal(inputLeases.length, evidence.instances.length);
      inputLeases.forEach((lease, index) => {
        for (const [key, value] of Object.entries(evidence.instances[index])) assert.equal(lease[key], value);
        assert.ok(Number.isSafeInteger(lease.acceptedRenewals) && lease.acceptedRenewals > 0);
        assert.ok(Number.isSafeInteger(lease.minimumMarginMs) && lease.minimumMarginMs > 0);
        assert.equal(lease.minimumMarginMs, lease.leaseMs - lease.maxGapMs);
        assert.ok(lease.gapAtReadMs >= 0 && lease.gapAtReadMs <= lease.maxGapMs);
      });
      evidence.inputLeases = inputLeases;
    }
    evidence.elapsedMs = Math.round(performance.now() - start);
    traffic.stop();
    evidence.gatewayRequests = traffic.counts;
    assert.ok(evidence.gatewayRequests.fixture.profile_status >= evidence.samples,
      'The traffic collector must capture each explicit fixture status check');
    assert.equal(evidence.gatewayRequests.app.unparsed, undefined, 'An unparsed gateway request prevents attribution');
    evidence.panelPresentation = await presentation.finish();
    evidence.budgetMisses = evidence.panelPresentation.panels
      .filter(panel => panel.udpToCanvasUpperMs.p95 > 150)
      .map(panel => `Panel ${panel.panel} soak UDP-to-canvas upper p95 was ${panel.udpToCanvasUpperMs.p95.toFixed(2)} ms`);
    assert.ok(evidence.elapsedMs >= seconds * 1000);
    assert.ok(evidence.scans > 0 && evidence.inventory > 0);
    evidence.fastNotifications = lastFast - firstReader.fast;
    evidence.panels = await panelSnapshots();
    await cdp.evaluate('window.__dcsSoakReaders.release()');
    const resumed = await until(async () => {
      const state = await readerStatus(cdp);
      assert.equal(state.error, null);
      assert.equal(state.ownerError, null, JSON.stringify(state.ownerError));
      return state.slow > 1 && state;
    });
    evidence.slowDeliveriesAfterRelease = resumed.slow;
    if (seconds === fullSeconds) {
      assert.ok(evidence.memory.length >= 50);
      checkMemory(evidence.memory);
      evidence.qualifiedDuration = true;
    }
    setAirborne(false);
    if (!rates) {
      await sendDebug(owner, false);
      if (value) await readMcp(false);
    }
    await live();
    await ready(false);
    return evidence;
  } catch (error) {
    const diagnostics = await Promise.allSettled([panelSnapshots(), readerStatus(cdp), recentTelemetry(cdp)]);
    await writeFile(resolve(directory, 'soak-failure.json'), JSON.stringify({
      ...evidence, gatewayRequests:traffic?.counts, partialPresentation:presentation?.partial(),
      diagnostics, failure:String(error),
    }, null, 2) + '\n');
    throw error;
  } finally {
    traffic?.stop();
    await cdp.evaluate('window.__dcsSoakReaders.close(); delete window.__dcsSoakReaders');
  }
}
