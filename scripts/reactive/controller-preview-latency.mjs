// Opt-in live measurement. Open the native Devices controller inspector first.
// Measures journal commit -> JS/DOM/frame opportunities, not hardware or photon latency.
import {nativePage} from './cdp.mjs';
import {mkdir,writeFile} from 'node:fs/promises';
const page=await nativePage();
try{
  const result=await page.evaluate(`(async()=>{
    const {gatewayRequest}=await import('/src/lib/gateway.ts');
    const {createNativeObservationTransport}=await import('/src/lib/streams/native.ts');
    const detail=document.querySelector('.host-detail');
    if(!detail)throw Error('Open the native controller inspector before measuring.');
    const heading=document.querySelector('[aria-label="Component details"] h1')?.textContent?.trim();
    let clocks=[],selected;
    for(let i=0;i<20;i++){
      const before=performance.now();
      const response=await gatewayRequest({command:'controller_list',offset:0});
      const after=performance.now(),data=response.controllers;
      selected??=data?.sources?.find(source=>source.online&&(!heading||source.name.trim()===heading));
      if(!data?.captureClock)throw Error('Rebuild the native service with captureClock metadata.');
      const native=Number(BigInt(data.captureClock.nowUs))/1000;
      clocks.push({before,after,native,id:data.captureClock.id,rtt:after-before});
    }
    if(!selected)throw Error('The visible inspector needs an observed physical controller.');
    const best=clocks.reduce((best,item)=>item.rtt<best.rtt?item:best);
    const offset=(best.before+best.after)/2-best.native;
    const samples=[],dom=new Map(),errors=[],controller=new AbortController();
    const sequence=()=>{
      const parts=detail.querySelector('.controller-status')?.textContent?.match(/Connection ([0-9]+).*report ([0-9]+)/);
      return parts?parts[1]+':'+parts[2]:undefined;
    };
    const mutation=new MutationObserver(()=>{
      const id=sequence();if(!id||dom.has(id))return;
      const entry={dom:performance.now()};dom.set(id,entry);
      requestAnimationFrame(()=>{entry.frame=performance.now();requestAnimationFrame(()=>{entry.nextFrame=performance.now();});});
      if(dom.size>5000)dom.delete(dom.keys().next().value);
    });
    mutation.observe(detail,{subtree:true,childList:true,characterData:true});
    const started=performance.now(),visibility=document.visibilityState;
    createNativeObservationTransport().open('controllers.input.'+selected.id,{
      frame:(raw,ack)=>{
        const received=performance.now(),frame=raw.frame;
        if(frame.clockId!==best.id || frame.kind==='loss' || frame.payload?.source?.generation!==selected.generation){
          errors.push('Source clock/generation/continuity changed; discard this timing run.');
          controller.abort();return;
        }
        if(frame.kind==='change'){
          const captured=Number(BigInt(frame.capturedUs))/1000+offset;
          if(samples.length<5000)samples.push({sequence:frame.payload.source.generation+':'+frame.payload.source.sequence,captured,received});
        }
        void ack().catch(error=>errors.push(String(error)));
      },error:message=>errors.push(message)
    },controller.signal);
    await new Promise(resolve=>setTimeout(resolve,10000));
    controller.abort();mutation.disconnect();
    const rows=samples.map(row=>({...row,...dom.get(row.sequence)}));
    const summary=key=>{
      const data=rows.filter(row=>Number.isFinite(row[key])).map(row=>row[key]-row.captured).sort((a,b)=>a-b);
      const p=f=>data.length?data[Math.min(data.length-1,Math.floor((data.length-1)*f))]:null;
      return {count:data.length,p50:p(.5),p95:p(.95),p99:p(.99),max:p(1)};
    };
    return {at:new Date().toISOString(),durationMs:performance.now()-started,visibility,
      source:{name:selected.name,backend:selected.backend,controls:selected.controlCount},
      clockUncertaintyMs:best.rtt/2,clockRoundTripMs:best.rtt,
      journalToObserverMs:summary('received'),journalToDomMs:summary('dom'),
      journalToFrameOpportunityMs:summary('frame'),journalToFollowingFrameMs:summary('nextFrame'),
      errors,clocks,samples:rows};
  })()`);
  await mkdir('.research/reactive',{recursive:true});
  await writeFile('.research/reactive/controller-preview-latency.json',JSON.stringify(result,null,2));
  if(result.errors.length)throw Error('Timing run invalid: '+result.errors.join('; '));
  const {samples,clocks,...summary}=result;
  console.log(JSON.stringify(summary,null,2));
}finally{page.close();}
