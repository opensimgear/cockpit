// Opt-in live measurement. Open the native Devices controller inspector first.
// Measures native read completion -> journal/DOM and journal -> JS/DOM/frame opportunities.
// OSG_CONTROLLER_TRACE_IPC=1 also measures large WebView channel fetches.
// OSG_CONTROLLER_TRACE_DIRECTORY can separate comparison runs.
import {nativePage} from './cdp.mjs';
import {mkdir,writeFile} from 'node:fs/promises';
import {join} from 'node:path';
const page=await nativePage();
const traceIpc=process.env.OSG_CONTROLLER_TRACE_IPC==='1';
const pending=new Map(),reads=new Set(),fetches=[];
let unreadableFetches=0;
const stopTrace=traceIpc?page.listen(({method,params})=>{
  const id=params.requestId;
  if(method==='Network.requestWillBeSent'&&/plugin(?::|%3a)__TAURI_CHANNEL__(?:\||%7c)fetch/i.test(params.request.url)){
    pending.set(id,params.timestamp);
  }else if(method==='Network.loadingFailed'){
    pending.delete(id);
  }else if(method==='Network.loadingFinished'&&pending.has(id)){
    const started=pending.get(id);
    pending.delete(id);
    const task=page.call('Network.getResponseBody',{requestId:id}).then(({body})=>{
      const {frame}=JSON.parse(body);
      if(frame?.source!=='controller-input'||fetches.length>=5000)return;
      fetches.push({kind:frame.kind,capturedUs:frame.capturedUs,
        sequence:frame.payload?.source?.sequence,bytes:Buffer.byteLength(body),
        fetchMs:(params.timestamp-started)*1000});
    }).catch(()=>{unreadableFetches++;});
    reads.add(task);
    void task.finally(()=>reads.delete(task));
  }
}):()=>{};
try{
  if(traceIpc)await page.call('Network.enable');
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
    const visibility=document.visibilityState;
    let started;
    let lastSequence;
    const stop=observeControllerInputs(sourceId,state=>{
      const received=performance.now(),current=state.detail;
      if(state.status==='connecting'&&started===undefined)return;
      if(state.status!=='ready'||!current?.source?.baselineReady){
        if(started!==undefined||state.status==='error'||state.status==='lost'){
          errors.push('Controller observation was not ready: '+(state.error??state.status));
        }
        return;
      }
      if('controllers:'+current.session!==best.id||current.source.id!==sourceId||current.source.generation!==selected.generation){
        errors.push('Controller observation changed capture clock or source identity.');
        return;
      }
      started??=received;
      const sequence=current.source.generation+':'+current.source.sequence;
      if(lastSequence&&sequence!==lastSequence&&samples.length<5000){
        const captured=Number(BigInt(current.capturedUs))/1000+offset;
        const nativeRead=current.source.nativeReadUs===undefined?undefined:Number(BigInt(current.source.nativeReadUs))/1000+offset;
        samples.push({sequence,captured,received,nativeRead});
      }
      lastSequence=sequence;
    });
    try{await new Promise(resolve=>setTimeout(resolve,10000));}
    finally{stop();mutation.disconnect();}
    const rows=samples.map(row=>({...row,...dom.get(row.sequence)}));
    if(!detail.isConnected||!rows.some(row=>Number.isFinite(row.dom))){
      errors.push('No matching reports were rendered by the selected inspector.');
    }
    if(selected.backend==='windows-raw-input'&&rows.some(row=>!Number.isFinite(row.nativeRead))){
      errors.push('A Windows Raw Input report is missing its native read timestamp.');
    }
    const summary=(key,from='captured')=>{
      const data=rows.filter(row=>Number.isFinite(row[key])&&Number.isFinite(row[from]))
        .map(row=>row[key]-row[from]).sort((a,b)=>a-b);
      const p=f=>data.length?data[Math.min(data.length-1,Math.floor((data.length-1)*f))]:null;
      return {count:data.length,p50:p(.5),p95:p(.95),p99:p(.99),max:p(1)};
    };
    return {at:new Date().toISOString(),durationMs:started===undefined?0:performance.now()-started,visibility,
      observer:'shared-inspector',
      source:{name:selected.name,backend:selected.backend,controls:selected.controlCount},
      clockUncertaintyMs:best.rtt/2,clockRoundTripMs:best.rtt,
      journalToObserverMs:summary('received'),journalToDomMs:summary('dom'),
      nativeReadToJournalMs:summary('captured','nativeRead'),nativeReadToDomMs:summary('dom','nativeRead'),
      // Separate readers can receive the same report in either order.
      diagnosticObserverToDomOffsetMs:summary('dom','received'),
      journalToFrameOpportunityMs:summary('frame'),journalToFollowingFrameMs:summary('nextFrame'),
      errors,clocks,samples:rows};
  })()`);
  stopTrace();
  await Promise.all(reads);
  if(traceIpc){
    const p95=key=>{
      const values=fetches.map(packet=>packet[key]).sort((a,b)=>a-b);
      return values.length?values[Math.floor((values.length-1)*.95)]:null;
    };
    result.channelFetch={count:fetches.length,p95Bytes:p95('bytes'),p95Ms:p95('fetchMs'),
      unreadableFetches,kinds:[...new Set(fetches.map(packet=>packet.kind))]};
    result.channelFetches=fetches;
  }
  const directory=process.env.OSG_CONTROLLER_TRACE_DIRECTORY??'.research/reactive';
  await mkdir(directory,{recursive:true});
  const artifact=traceIpc?'controller-preview-latency-ipc':'controller-preview-latency';
  await writeFile(join(directory,`${artifact}.json`),JSON.stringify(result,null,2));
  if(result.errors.length)throw Error('Timing run invalid: '+result.errors.join('; '));
  const {samples,clocks,channelFetches,...summary}=result;
  console.log(JSON.stringify(summary,null,2));
}finally{stopTrace();page.close();}
