import assert from 'node:assert/strict';
import {intervals,summarizeMilliseconds} from './vjoy-rate-transport.mjs';

export const extendedNativeRates = process.env.OSG_DCS_COMBINED_EXTENDED_RATES === '1';
const durationMs = extendedNativeRates ? 4800 : 192;

export function createNativeRateBurst({cdp,observation,lanes,delay,paintOffsets,litCanvases,panels,samples,waitLamp,probeStart,commandBurst}) {
  const milliseconds = (value) => Number(BigInt(value)-probeStart)/1e6;
  return async function run(owner,setAirborne,showAirborne) {
    const offsets=await paintOffsets(), startNs=process.hrtime.bigint()+150_000_000n;
    const request={command:'qualification_debug_burst',owner,nodes:lanes.map(lane=>lane.nodeId),startNs:String(startNs),durationMs};
    const invoke=cdp.evaluate(`window.__TAURI_INTERNALS__.invoke('gateway_request',{request:${JSON.stringify(request)}})`)
      .then(reply=>{
        assert.equal(reply.ok,true,`qualification_debug_burst: ${reply.error}`);
        return reply.state.qualificationBurst;
      });
    const settled=await Promise.allSettled([invoke,
      (async()=>{await delay(Number(startNs-process.hrtime.bigint())/1e6+10);return showAirborne();})(),
      observation.samplePorts([{portId:'button-1',value:true},{portId:'button-2',value:true}]).then(samples=>{
        observation.drainFast();
        return samples;
      }),litCanvases(offsets),commandBurst?.(startNs,durationMs)]);
    const [rows,inspectorPresentation,mcpSamples,canvases,dcsCommands]=settled.map(result=>{
      if(result.status==='rejected')throw result.reason;
      return result.value;
    });
    if(extendedNativeRates)assert.ok(observation.fastUpdates()>0,'The fast MCP reader must progress under both rates');
    assert.equal(rows.length,lanes.length);
    const submissions=rows.map((lane,index)=>{
      assert.equal(lane.length,durationMs/lanes[index].intervalMs+1);
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
    assert.ok(overlapEnd-overlapStart>=durationMs*0.94,'Both native source schedules must overlap');
    if(inspectorPresentation){
      const probeMs=Number(probeStart)/1e6;
      assert.ok(inspectorPresentation.samples[0].submittedMs-probeMs>=overlapStart
        &&inspectorPresentation.renderedThroughMs-probeMs<=overlapEnd,
      'Every measured inspector transition must render during the native-rate overlap');
    }
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
      return {...lanes[index],count:rows.length,sourceToDispatchMs:summarizeMilliseconds(rows.map(row=>row.dispatchAtMs-row.offerAtMs)),
        ingressWaitMs:summarizeMilliseconds(batches.map(row=>row.ingressWaitMs)),
        ingressWorkMs:summarizeMilliseconds(rows.map(row=>row.ingressWorkMs)),
        batchCount:batches.length,batchSizePeak:1+Math.max(...rows.map(row=>row.batchIndex)),
        dispatchHz:rate(dispatch),completionHz:rate(admitted),
        dispatchIntervalsMs:summarizeMilliseconds(intervals(dispatch)),
        completionIntervalsMs:summarizeMilliseconds(intervals(admitted))};
    });
    return {submissions,evidence,mcpSamples,overlapMs:overlapEnd-overlapStart,overlapStart,overlapEnd,canvases,dcsCommands,inspectorPresentation};
  };
}
