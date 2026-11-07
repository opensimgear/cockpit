import assert from 'node:assert/strict';

// Timestamp the production canvas write. Display scan-out is outside this probe.
export async function tracePanelCanvas(page) {
  await page.addInitScript(()=>{
    window.__panelPaints=[];
    const paint=CanvasRenderingContext2D.prototype.putImageData;
    CanvasRenderingContext2D.prototype.putImageData=function(image,...args){
      const result=paint.call(this,image,...args);
      if(this.canvas.matches('[data-widget-id="probe"] canvas') && window.__panelPaints.length<128){
        const [red,green,blue,alpha]=image.data;
        window.__panelPaints.push({at:performance.now(),pixel:[red,green,blue,alpha],
          lit:red>240&&green<50&&blue<50&&alpha===255});
      }
      return result;
    };
  });
}

async function alignClock(page) {
  const brackets=[];
  for(let attempt=0;attempt<16;attempt++){
    const before=Number(process.hrtime.bigint())/1e6;
    const browser=await page.evaluate(()=>performance.now());
    const after=Number(process.hrtime.bigint())/1e6;
    brackets.push({low:browser-after,high:browser-before});
  }
  return brackets.reduce((a,b)=>a.high-a.low<=b.high-b.low?a:b);
}

function summarize(values) {
  const sorted=[...values].sort((a,b)=>a-b);
  const percentile=p=>sorted[Math.ceil(p*sorted.length)-1];
  return {count:values.length,p50:percentile(.5),p95:percentile(.95),p99:percentile(.99),max:sorted.at(-1)};
}

export async function measurePanelPresentation(page, submitOwnship) {
  assert.equal(await page.evaluate(()=>document.visibilityState),'visible');
  const initial=await alignClock(page), samples=[];
  for(let index=0;index<24;index++){
    const lit=index%2===0;
    const offsets=await page.evaluate(()=>({feedback:window.__combinedLamp.length,paint:window.__panelPaints.length}));
    const submission=await submitOwnship(lit);
    const match=await page.waitForFunction(({lit,offsets})=>{
      const feedback=window.__combinedLamp.slice(offsets.feedback).find(row=>row.status==='fresh'&&row.value===lit);
      const paint=feedback&&window.__panelPaints.slice(offsets.paint).find(row=>row.lit===lit&&row.at>=feedback.receivedAt);
      return paint?{feedback,paint}:null;
    },{lit,offsets},{timeout:3500});
    try {samples.push({lit,...submission,...await match.jsonValue()});} finally {await match.dispose();}
  }
  const final=await alignClock(page);
  const low=Math.max(initial.low,final.low),high=Math.min(initial.high,final.high);
  assert.ok(high>=low,'Node and browser clock alignment changed during the sample');
  const offset=(low+high)/2,uncertaintyMs=(high-low)/2;
  for(const [index,sample] of samples.entries()){
    if(index)assert.ok(sample.packetSequence>samples[index-1].packetSequence);
    assert.equal(sample.feedback.sourceKind,'host');
    assert.ok(sample.feedback.receivedAt>=sample.submittedMs+low,'Feedback precedes its UDP submission');
    const [red,green,blue,alpha]=sample.paint.pixel;
    assert.ok(sample.lit?red>240&&green<50&&blue<50&&alpha===255:
      red>100&&red<130&&green>210&&blue>180&&alpha===255,'The production widget must paint the expected color');
  }
  const udpToBrowserMs=summarize(samples.map(row=>row.feedback.receivedAt-row.submittedMs-offset));
  const udpToCanvasMs=summarize(samples.map(row=>row.paint.at-row.submittedMs-offset));
  const browserToCanvasMs=summarize(samples.map(row=>row.paint.at-row.feedback.receivedAt));
  assert.ok(udpToCanvasMs.p95+uncertaintyMs<=150,'Loopback authored UDP to browser canvas exceeded 150 ms');
  return {initial,final,uncertaintyMs,udpToBrowserMs,udpToCanvasMs,browserToCanvasMs,samples};
}
