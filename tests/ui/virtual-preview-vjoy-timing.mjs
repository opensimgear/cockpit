import assert from 'node:assert/strict';
import {spawn} from 'node:child_process';
import {once} from 'node:events';
import {createInterface} from 'node:readline';
import {summarize} from './dcs-panel-presentation-timing.mjs';

export async function startPreviewVjoyProbe(executable) {
  assert.equal(process.platform,'win32');
  const launch=process.hrtime.bigint();
  const probe=spawn(executable,['--vjoy-buttons','9000'],{windowsHide:true});
  const stdout=createInterface({input:probe.stdout});
  const exited=once(probe,'close');
  const reports=[];
  let ready;
  const registered=new Promise(resolve=>ready=resolve);
  stdout.on('line',line=>{
    if(line==='ready')ready(process.hrtime.bigint());
    else reports.push(line);
  });
  let stderr='';
  probe.stderr.on('data',data=>stderr=(stderr+data).slice(-4096));
  const close=async()=>{
    if(probe.exitCode===null&&probe.signalCode===null)probe.kill();
    await exited.catch(()=>{});
    stdout.close();
  };
  try {
    const registeredAt=await Promise.race([
      registered,
      exited.then(([code])=>{throw Error(`Raw Input probe exited before registration (${code}) ${stderr}`);}),
    ]);
    return {
      async finish(slot,samples,clockBefore,clockAfter) {
        assert.equal((await exited)[0],0,stderr);
        assert.equal(reports.length,1);
        return compareReceipts(JSON.parse(reports[0]),slot,samples,clockBefore,clockAfter,launch,registeredAt);
      },
      close,
    };
  }catch(error){
    await close();
    throw error;
  }
}

function compareReceipts(report,slot,samples,before,after,launch,registered) {
  assert.equal(report.captureOverflow,false);
  assert.ok(Number.isSafeInteger(report.receiverTickHz)&&report.receiverTickHz>0);
  const tickNs=ticks=>ticks*1_000_000_000n/BigInt(report.receiverTickHz);
  const started=BigInt(report.receiverStartedTicks);
  assert.ok(tickNs(started)>=launch&&tickNs(started)<=registered,'Receiver and Node must share Windows QPC');
  // Allow for browser timer quantization as in the Windows dual-rate fixture.
  const low=Math.max(before.low,after.low)-0.25;
  const high=Math.min(before.high,after.high)+0.25;
  assert.ok(low<=high&&high-low<=2,'The browser clock must stay within a 2 ms QPC bracket');
  const asMs=ns=>Number(ns)/1e6;
  const ended=asMs(tickNs(BigInt(report.receiverEndedTicks)));
  assert.ok(ended+low>=samples.at(-1).frame,'The receiver must stay active through presentation');
  const events=report.buttonEvents.filter(event=>event.report===slot);
  const device=events[0]?.device;
  assert.ok(report.devices.some(row=>row.id===device&&row.virtualVjoy===true));
  const timed=events.map(event=>{
    assert.equal(event.device,device,'The selected slot must resolve to one native device');
    assert.equal(event.status,0x00110000,'HID button decoding must succeed');
    assert.equal(event.peerPressed,false,'The profile owns only button 1');
    assert.ok(Number.isSafeInteger(event.receivedTicks)&&event.receivedTicks>=0);
    const ns=tickNs(started+BigInt(event.receivedTicks));
    return {pressed:event.pressed,ns:ns.toString(),at:asMs(ns)};
  });
  if(timed[0]?.pressed===false){
    assert.ok(timed[0].at+high<samples[0].submitted,'An initial neutral report must precede the workload');
    timed.shift();
  }
  assert.deepEqual(timed.map(event=>event.pressed),samples.map(sample=>sample.value));
  const edges=samples.map((sample,index)=>{
    const event=timed[index];
    const lower=event.at+low-sample.submitted, upper=event.at+high-sample.submitted;
    assert.ok(upper>=0&&(!index||event.at>timed[index-1].at),'The receipt must follow its input in order');
    if(index+1<samples.length)assert.ok(event.at+high<samples[index+1].submitted,
      'The receipt must precede the next input throughout the clock bracket');
    return {sequence:sample.sequence,receivedQpcNs:event.ns,lowerMs:Math.max(0,lower),upperMs:upper};
  });
  return {
    device,clockBefore:before,clockAfter:after,clockBracketMs:high-low,
    submitToRawLowerMs:summarize(edges.map(edge=>edge.lowerMs)),
    submitToRawUpperMs:summarize(edges.map(edge=>edge.upperMs)),
    edges,
  };
}
