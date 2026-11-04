// Opt-in live measurement. Open the native Devices controller inspector first.
// Measures journal commit -> JS/DOM/frame opportunities, not hardware or photon latency.
import {nativePage} from './cdp.mjs';
import {mkdir,writeFile} from 'node:fs/promises';
const page=await nativePage();
try{
  const result=await page.evaluate(`(async()=>{
    const {gatewayRequest}=await import('/src/lib/gateway.ts');
    const {observeControllerInputs}=await import('/src/lib/controller-observations.ts');
    const detail=document.querySelector('.host-detail');
    if(!detail)throw Error('Open the native controller inspector before measuring.');
    const sourceId=detail.querySelector('.controller-identity code')?.textContent?.trim();
    if(!sourceId)throw Error('The inspector must identify its selected controller.');
    let clocks=[],selected;
    for(let i=0;i<20;i++){
      const before=performance.now();
      const response=await gatewayRequest({command:'controller_list',offset:0});
      const after=performance.now(),data=response.controllers;
      selected??=data?.sources?.find(source=>source.online&&source.id===sourceId);
      if(!data?.captureClock)throw Error('Rebuild the native service with captureClock metadata.');
      const native=Number(BigInt(data.captureClock.nowUs))/1000;
      clocks.push({before,after,native,id:data.captureClock.id,rtt:after-before});
    }
    if(!selected)throw Error('The visible inspector needs an observed physical controller.');
    const best=clocks.reduce((best,item)=>item.rtt<best.rtt?item:best);
    const offset=(best.before+best.after)/2-best.native;
    const samples=[],dom=new Map(),errors=[];
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
    let lastSequence;
    const stop=observeControllerInputs(sourceId,state=>{
      const received=performance.now(),current=state.detail;
      if(state.status!=='ready'||!current?.source?.baselineReady||
          'controllers:'+current.session!==best.id||current.source.generation!==selected.generation){
        errors.push('The inspector lost its existing baseline or changed identity.');
        return;
      }
      const sequence=current.source.generation+':'+current.source.sequence;
      if(lastSequence&&sequence!==lastSequence&&samples.length<5000){
        const captured=Number(BigInt(current.capturedUs))/1000+offset;
        samples.push({sequence,captured,received});
      }
      lastSequence=sequence;
    });
    try{await new Promise(resolve=>setTimeout(resolve,10000));}
    finally{stop();mutation.disconnect();}
    const rows=samples.map(row=>({...row,...dom.get(row.sequence)}));
    if(!detail.isConnected||!rows.some(row=>Number.isFinite(row.dom))){
      errors.push('No matching reports were rendered by the selected inspector.');
    }
    const summary=(key,from='captured')=>{
      const data=rows.filter(row=>Number.isFinite(row[key])).map(row=>row[key]-row[from]).sort((a,b)=>a-b);
      const p=f=>data.length?data[Math.min(data.length-1,Math.floor((data.length-1)*f))]:null;
      return {count:data.length,p50:p(.5),p95:p(.95),p99:p(.99),max:p(1)};
    };
    return {at:new Date().toISOString(),durationMs:performance.now()-started,visibility,
      observer:'shared-inspector',
      source:{name:selected.name,backend:selected.backend,controls:selected.controlCount},
      clockUncertaintyMs:best.rtt/2,clockRoundTripMs:best.rtt,
      journalToObserverMs:summary('received'),journalToDomMs:summary('dom'),
      observerToDomMs:summary('dom','received'),
      journalToFrameOpportunityMs:summary('frame'),journalToFollowingFrameMs:summary('nextFrame'),
      errors,clocks,samples:rows};
  })()`);
  await mkdir('.research/reactive',{recursive:true});
  await writeFile('.research/reactive/controller-preview-latency.json',JSON.stringify(result,null,2));
  if(result.errors.length)throw Error('Timing run invalid: '+result.errors.join('; '));
  const {samples,clocks,...summary}=result;
  console.log(JSON.stringify(summary,null,2));
}finally{page.close();}
