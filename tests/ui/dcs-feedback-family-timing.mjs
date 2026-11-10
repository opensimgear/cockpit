import assert from 'node:assert/strict';
import {randomUUID} from 'node:crypto';
import {until} from '../../scripts/reactive/cdp.mjs';
import {attachBiosFeedback, biosFeedbackPortId} from './dcs-feedback-family-publication.mjs';

export async function assignDcsPanel({request, companion, document, assignments, publicationHash, panel, nodeId, panelNumber, biosPortId}) {
  const {page, peer} = panel;
  const resource = await until(async () => (await request({command:'profile_resources_list', limit:128}))
    .profileResources.resources.find(row => row.attributes?.companionId === peer.companionId
      && row.attributes.componentKind === 'paired-companion'));
  const {ports} = (await request({command:'profile_companion_inspect', resourceId:resource.id,
    publicationHash, offset:0, limit:64})).profileCompanionAuthoring;
  const ids = ['flight-input-0', 'feedback:flight-channel-0:flight-output-0'];
  if (biosPortId) ids.push(biosFeedbackPortId);
  const selected = ids.map(id => {
    const port = ports.find(row => row.descriptor.id === id);
    assert.ok(port, `Missing panel port ${id}`);
    return port;
  });
  await companion('companion_assign', {companionId:peer.companionId, instanceId:'combined-' + randomUUID(), publicationHash});
  await page.getByText('Connected · panel active', {exact:true}).waitFor();
  await until(async () => {
    const current = (await request({command:'profile_resources_inspect', resourceId:resource.id, limit:64}))
      .profileResources.ports;
    return selected.every(port => current.some(live => live.current && live.descriptor.id === port.descriptor.id
      && live.contractHash === port.contractHash));
  });
  const mapped = selected.map((port, index) => ({...port.descriptor, id:['press', 'lamp', 'bios-lamp'][index]}));
  document.resources.push({id:nodeId, name:'Combined browser panel ' + panelNumber, kind:'companion', required:false, ports:mapped});
  document.companionAssignments.push({resourceId:nodeId, publicationHash});
  document.bindings.push({id:nodeId + '-lamp', from:{nodeId:'telemetry', portId:'flight.ownship.inAir'},
    to:{nodeId, portId:'lamp'}});
  if (biosPortId) attachBiosFeedback(document, nodeId, biosPortId, mapped[2].contract);
  assignments.push({resourceId:nodeId, sourceId:resource.id, ports:selected.map((port, index) => ({
    profilePortId:mapped[index].id, observedPortId:port.descriptor.id, expectedContractHash:port.contractHash,
  }))});
}

export async function installFeedbackFamilyProbe(page) {
  await page.addInitScript(({biosChannel, biosOutput}) => {
    const Worker = window.SharedWorker;
    let held;
    const probe = window.__feedbackFamilies = {
      hold:false, replies:0, samples:{}, history:[],
      release() {
        this.hold = false;
        const send = held;
        held = undefined;
        send?.();
      },
      read() {
        const lit = id => {
          const canvas = document.querySelector(`[data-widget-id="${id}"] canvas`);
          if (!canvas) return null;
          const [r, g, b, a] = canvas.getContext('2d').getImageData(0, 0, 1, 1).data;
          return r > 240 && g < 50 && b < 50 && a === 255;
        };
        return {held:!!held, replies:this.replies, samples:this.samples, history:this.history,
          identity:this.identity, ownshipLit:lit('probe'), biosLit:lit('bios-indicator')};
      },
    };
    window.SharedWorker = class extends Worker {
      constructor(...args) {
        super(...args);
        const port = this.port;
        const post = port.postMessage.bind(port);
        port.postMessage = (message, ...rest) => {
          if (!probe.hold || message?.type !== 'feedback-ack') return post(message, ...rest);
          held ??= () => post(message, ...rest);
        };
        port.addEventListener('message', ({data}) => {
          const feedback = data?.type === 'feedback' && data.reply?.feedback;
          if (!feedback) return;
          probe.identity = {sessionId:feedback.sessionId, generation:feedback.generation,
            publicationHash:feedback.publicationHash};
          if (feedback.mode === 'snapshot') probe.samples = {};
          for (const row of feedback.values ?? []) {
            if (row.channelId === 'flight-channel-0' && row.outputId === 'flight-output-0') probe.samples.ownship = row;
            if (row.channelId === biosChannel && row.outputId === biosOutput) probe.samples.bios = row;
          }
          probe.history.push({index:++probe.replies, mode:feedback.mode, samples:{...probe.samples}});
          if (probe.history.length > 16) probe.history.shift();
        });
      }
    };
  }, {biosChannel:biosFeedbackPortId.split(':')[1], biosOutput:biosFeedbackPortId.split(':')[2]});
}

const read = page => page.evaluate(() => window.__feedbackFamilies.read());
const sampleMatches = (sample, value) => value === null
  ? ['stale', 'unknown'].includes(sample?.status) && sample.value == null
  : sample?.status === 'fresh' && sample.value === value;
const matches = (row, ownship, bios) => sampleMatches(row.samples.ownship, ownship)
  && sampleMatches(row.samples.bios, bios) && row.ownshipLit === (ownship === true)
  && row.biosLit === (bios === true);

async function waitPanel(page, ownship, bios) {
  try {
    return await until(() => read(page), row => matches(row, ownship, bios), 3500);
  } catch (cause) {
    const {history, ...last} = await read(page);
    throw new Error(`Panel did not paint ownship ${ownship} and BIOS ${bios}: ${JSON.stringify(last)}`, {cause});
  }
}

export async function verifyDcsFeedbackFamilies(panels, submitOwnship, {setBios, setPhysics, setAirborne}) {
  // Allow the R0 panel presentation budget plus one 50 ms test observation.
  const presentationAllowanceMs = 150 + 50;
  assert.equal(panels.length, 2);
  const [slow, fast] = panels.map(({page}) => page);
  const baseline = await Promise.all([slow, fast].map(page => waitPanel(page, false, true)));
  await slow.evaluate(() => { window.__feedbackFamilies.hold = true; });
  try {
    await submitOwnship(true);
    const held = await until(() => read(slow), row => row.held, 3500);
    await waitPanel(fast, true, true);

    const biosOff = performance.now();
    setBios('off');
    const biosWithdrawn = await waitPanel(fast, true, null);
    const biosWithdrawalMs = performance.now() - biosOff;
    assert.ok(biosWithdrawalMs <= 600 + presentationAllowanceMs,
      `BIOS withdrawal exceeded its 600 ms freshness plus presentation allowance: ${biosWithdrawalMs} ms`);
    await submitOwnship(false);
    const peerAdvanced = await waitPanel(fast, false, null);
    assert.equal((await read(slow)).replies, held.replies, 'Only one worker feedback delivery may be in flight');

    await slow.evaluate(() => window.__feedbackFamilies.release());
    const resumed = await waitPanel(slow, false, null);
    const first = resumed.history.find(row => row.index === held.replies + 1);
    assert.equal(first?.mode, 'snapshot');
    assert.ok(sampleMatches(first.samples.bios, null), 'A recovered panel must skip pre-loss BIOS feedback');
    assert.ok(sampleMatches(first.samples.ownship, false), 'A recovered panel must receive current ownship feedback');

    setBios('full', 0);
    const biosRecovered = await Promise.all([slow, fast].map(page => waitPanel(page, false, false)));
    await submitOwnship(true);
    await Promise.all([slow, fast].map(page => waitPanel(page, true, false)));
    const ownshipOff = performance.now();
    setPhysics(false);
    const ownshipWithdrawn = await waitPanel(fast, null, false);
    const ownshipWithdrawalMs = performance.now() - ownshipOff;
    assert.ok(ownshipWithdrawalMs <= 1000 + presentationAllowanceMs,
      `Ownship withdrawal exceeded its 1,000 ms freshness plus presentation allowance: ${ownshipWithdrawalMs} ms`);
    setBios('full', 1000);
    await Promise.all([slow, fast].map(page => waitPanel(page, null, true)));

    setAirborne(false);
    setPhysics(true);
    const rearmed = await Promise.all([slow, fast].map(page => waitPanel(page, false, true)));
    for (const [index, row] of rearmed.entries()) assert.deepEqual(row.identity, baseline[index].identity);
    return {biosWithdrawalMs, ownshipWithdrawalMs, heldReply:held.replies, resumedSnapshot:first,
      biosWithdrawn:biosWithdrawn.samples, peerAdvanced:peerAdvanced.samples,
      biosRecovered:biosRecovered.map(row => row.samples), ownshipWithdrawn:ownshipWithdrawn.samples,
      rearmed:rearmed.map(row => row.samples)};
  } finally {
    setAirborne(false);
    setPhysics(true);
    setBios('counter-only');
    await slow.evaluate(() => window.__feedbackFamilies.release());
  }
}
