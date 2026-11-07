import assert from 'node:assert/strict';
import {intervals,summarizeMilliseconds} from './vjoy-rate-transport.mjs';

export function createNativeRateBurst({cdp,observation,lanes,delay,paintOffsets,litCanvases,panels,samples,waitLamp,probeStart,commandBurst}) {
  const milliseconds = (value) => Number(BigInt(value)-probeStart)/1e6;
  return async function run(owner,setAirborne) {
    const offsets=await paintOffsets(), startNs=process.hrtime.bigint()+150_000_000n;
    const request={command:'qualification_debug_burst',owner,nodes:lanes.map(lane=>lane.nodeId),startNs:String(startNs)};
    const invoke=cdp.evaluate(`window.__TAURI_INTERNALS__.invoke('gateway_request',{request:${JSON.stringify(request)}})`)
      .then(reply=>{
        assert.equal(reply.ok,true,`qualification_debug_burst: ${reply.error}`);
        return reply.state.qualificationBurst;
      });
    const [rows,,mcpSamples,canvases,dcsCommands]=await Promise.all([invoke,
      (async()=>{await delay(Number(startNs-process.hrtime.bigint())/1e6+10);setAirborne(true);})(),
      observation.samplePorts([{portId:'button-1',value:true},{portId:'button-2',value:true}]),litCanvases(offsets),commandBurst?.(startNs)]);
    assert.equal(rows.length,lanes.length);
    const submissions=rows.map((lane,index)=>{
      assert.equal(lane.length,lanes[index].count);
      assert.ok(lane.every((sample,sampleIndex)=>sample.value===(sampleIndex%2===0)
        &&Number.isFinite(sample.roundTripMs)&&sample.roundTripMs>=0
        &&Number.isInteger(sample.pendingFrames)&&sample.pendingFrames>0&&sample.pendingFrames<=256
        &&Number.isInteger(sample.batchIndex)&&sample.batchIndex>=0));
      return lane.map(sample=>({...sample,offerAtMs:milliseconds(sample.atNs),
        dispatchAtMs:milliseconds(sample.dispatchAtNs),admittedAtMs:milliseconds(sample.admittedAtNs),
        at:milliseconds(sample.atNs)}));
    });
    const overlapStart=Math.max(...submissions.map(rows=>rows[0].offerAtMs));
    const overlapEnd=Math.min(...submissions.map(rows=>rows.at(-1).offerAtMs));
    for(const panel of canvases)assert.ok(panel.earliestMs>=overlapStart&&panel.latestMs<=overlapEnd,
      `Panel ${panel.panel} must paint during native-rate overlap`);
    for(const packet of dcsCommands?.datagrams??[]){
      const at=milliseconds(packet.atNs);
      assert.ok(at>=overlapStart&&at<=overlapEnd,`DCS command ${packet.text.trim()} must arrive during native-rate overlap`);
    }
    const after=await Promise.all(panels.map(({page})=>samples(page).then(rows=>rows.length)));
    setAirborne(false);
    await Promise.all(panels.map(({page},index)=>waitLamp(after[index],
      sample=>sample.status==='fresh'&&sample.value===false,page)));
    const evidence=submissions.map((rows,index)=>{
      const dispatch=rows.map(row=>row.dispatchAtMs), admitted=rows.map(row=>row.admittedAtMs);
      const batches=rows.filter(row=>row.batchIndex===0);
      const rate=(times)=>(times.length-1)*1000/(times.at(-1)-times[0]);
      return {...lanes[index],sourceToDispatchMs:summarizeMilliseconds(rows.map(row=>row.dispatchAtMs-row.offerAtMs)),
        coordinatorWaitMs:summarizeMilliseconds(batches.map(row=>row.coordinatorWaitMs)),
        coordinatorWorkMs:summarizeMilliseconds(rows.map(row=>row.coordinatorWorkMs)),
        batchCount:batches.length,batchSizePeak:1+Math.max(...rows.map(row=>row.batchIndex)),
        dispatchHz:rate(dispatch),completionHz:rate(admitted),
        dispatchIntervalsMs:summarizeMilliseconds(intervals(dispatch)),
        completionIntervalsMs:summarizeMilliseconds(intervals(admitted))};
    });
    return {submissions,evidence,mcpSamples,overlapMs:overlapEnd-overlapStart,overlapStart,overlapEnd,canvases,dcsCommands};
  };
}
