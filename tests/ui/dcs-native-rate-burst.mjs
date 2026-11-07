import assert from 'node:assert/strict';

export function createNativeRateBurst({cdp,observation,lanes,delay,paintOffsets,litCanvases,panels,samples,waitLamp,probeStart}) {
  return async function run(owner,setAirborne) {
    const offsets=await paintOffsets(), startNs=process.hrtime.bigint()+150_000_000n;
    const request={command:'qualification_debug_burst',owner,nodes:lanes.map(lane=>lane.nodeId),startNs:String(startNs)};
    const invoke=cdp.evaluate(`window.__TAURI_INTERNALS__.invoke('gateway_request',{request:${JSON.stringify(request)}})`)
      .then(reply=>{
        assert.equal(reply.ok,true,`qualification_debug_burst: ${reply.error}`);
        return reply.state.qualificationBurst;
      });
    const [rows,,mcpSamples,canvases]=await Promise.all([invoke,
      (async()=>{await delay(Number(startNs-process.hrtime.bigint())/1e6+10);setAirborne(true);})(),
      observation.samplePorts([{portId:'button-1',value:true},{portId:'button-2',value:true}]),litCanvases(offsets)]);
    assert.equal(rows.length,lanes.length);
    const submissions=rows.map((lane,index)=>{
      assert.equal(lane.length,lanes[index].count);
      assert.ok(lane.every((sample,sampleIndex)=>sample.value===(sampleIndex%2===0)
        &&Number.isFinite(sample.roundTripMs)&&sample.roundTripMs>=0
        &&Number.isInteger(sample.pendingFrames)&&sample.pendingFrames>0&&sample.pendingFrames<=256));
      return lane.map(sample=>({...sample,at:Number(BigInt(sample.atNs)-probeStart)/1e6}));
    });
    const overlapStart=Math.max(...submissions.map(rows=>rows[0].at));
    const overlapEnd=Math.min(...submissions.map(rows=>rows.at(-1).at));
    for(const panel of canvases)assert.ok(panel.earliestMs>=overlapStart&&panel.latestMs<=overlapEnd,
      `Panel ${panel.panel} must paint during native-rate overlap`);
    const after=await Promise.all(panels.map(({page})=>samples(page).then(rows=>rows.length)));
    setAirborne(false);
    await Promise.all(panels.map(({page},index)=>waitLamp(after[index],
      sample=>sample.status==='fresh'&&sample.value===false,page)));
    return {submissions,mcpSamples,overlapMs:overlapEnd-overlapStart,overlapStart,overlapEnd,canvases};
  };
}
