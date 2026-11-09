// Opt-in passive measurement of the actual profile inspector.
// Open its physical controller block and set OSG_CONTROLLER_SOURCE_ID explicitly.
// Set OSG_DESKTOP_NATIVE_APP when Cockpit is running from a copied executable.
import {nativePage} from './cdp.mjs';
import {mkdir,readFile,writeFile} from 'node:fs/promises';
import {createHash} from 'node:crypto';
const sourceId=process.env.OSG_CONTROLLER_SOURCE_ID;
const preflight=process.argv.includes('--preflight');
if(!sourceId)throw Error('Set OSG_CONTROLLER_SOURCE_ID to the controller shown in the profile inspector.');
const page=await nativePage();
try{
 const result=await page.evaluate(`(async()=>{
  const sourceId=${JSON.stringify(sourceId)};
  const preflight=${preflight};
  const {gatewayRequest}=await import('/src/lib/gateway.ts');
  const {createNativeObservationTransport}=await import('/src/lib/streams/native.ts');
  const inspector=document.querySelector('[aria-label="Selected block inspector"] .controller-inputs');
  if(!inspector?.querySelector('[data-control-id] .axis'))throw Error('Open the physical controller profile inspector with ready axes.');
  if(inspector.dataset.controllerSourceId!==sourceId)throw Error('The selected profile inspector belongs to a different controller.');
  const calibrate=async()=>{
   const rows=[];
   for(let i=0;i<20;i++){
    const before=performance.now(),response=await gatewayRequest({command:'controller_list',offset:0}),after=performance.now();
    const clock=response.controllers?.captureClock;
    if(!clock)throw Error('Native capture clock unavailable.');
    rows.push({before,after,native:Number(BigInt(clock.nowUs))/1000,id:clock.id,rtt:after-before});
   }
   const best=rows.reduce((a,b)=>a.rtt<b.rtt?a:b);
   return {rows,offset:(best.before+best.after)/2-best.native,uncertainty:best.rtt/2,id:best.id};
  };
  const before=await calibrate(),samples=[],dom=[],errors=[],controller=new AbortController();
  let snapshotCount=0,changeCount=0;
  let axes=[],generation,backend,previousWire,previousDom,previousFrame,subscriptionId,delivery,nativeSequence,limited=false;
  let reportCount=0,coalescedPreviewReports=0,previousRead;
  const nativeReadIntervals=[],readToJournal=[];
  const axisValues=new Map(), axisLabels=new Map();
  const wireSignature=()=>axes.map(id=>String(axisValues.get(id))).join('|');
  const domSignature=()=>axes.map(id=>{
   let label=axisLabels.get(id);
   if(!label||!inspector.contains(label)){
    label=inspector.querySelector('[data-control-id="'+CSS.escape(id)+'"] .heading .value');
    if(label)axisLabels.set(id,label);
   }
   return label?.textContent;
  }).join('|');
  const captureDom=()=>{
   if(!inspector.isConnected||inspector.dataset.controllerSourceId!==sourceId){errors.push('Visible inspector was replaced.');return;}
   if(!axes.length)return;
   const signature=domSignature();if(signature===previousDom)return;previousDom=signature;
   const row={signature,dom:performance.now()};dom.push(row);
   requestAnimationFrame(()=>{row.frameOpportunity=performance.now();});
  };
  const mutation=new MutationObserver(captureDom);
  mutation.observe(inspector,{subtree:true,childList:true,characterData:true,attributes:true});
  const started=performance.now(),visibility=document.visibilityState,focused=document.hasFocus();
  if(visibility!=='visible'||!focused)errors.push('The preview must remain visible and focused.');
  const changedVisibility=()=>{if(document.visibilityState!=='visible')errors.push('Preview became hidden.');};
  const blurred=()=>errors.push('Preview lost focus.');
  document.addEventListener('visibilitychange',changedVisibility);window.addEventListener('blur',blurred);
  try{
   createNativeObservationTransport().open('controllers.input.'+sourceId,{
    frame:(raw,ack)=>{
     const received=performance.now(),frame=raw.frame,payload=frame.payload;
     if(frame.clockId!==before.id||frame.kind==='loss')errors.push('Clock or continuity changed.');
     if(subscriptionId!==undefined&&raw.subscriptionId!==subscriptionId)errors.push('Subscription identity changed.');
     if(delivery!==undefined&&BigInt(raw.delivery)!==delivery+1n)errors.push('Delivery continuity changed.');
     subscriptionId=raw.subscriptionId;delivery=BigInt(raw.delivery);
     if(frame.topic!=='controllers.input.'+sourceId||frame.source!=='controller-input')errors.push('Wrong native topic or producer.');
     const snapshot=frame.kind==='snapshot';
     if(previousFrame){
      const advance=BigInt(frame.revision)-BigInt(previousFrame.revision);
      const same=frame.session===previousFrame.session&&frame.generation===previousFrame.generation;
      const sequenceOk=previousFrame.sequence===undefined||(frame.sequence!==undefined&&
       (snapshot?BigInt(frame.sequence)>BigInt(previousFrame.sequence):BigInt(frame.sequence)===BigInt(previousFrame.sequence)+1n));
      if(!same||advance<=0n||(!snapshot&&advance!==1n)||!sequenceOk)errors.push('Native stream revision or sequence continuity changed.');
     }
     if(snapshot)snapshotCount++;
     if(frame.kind==='change')changeCount++;
     const controls=snapshot?(payload?.controls??[]):[];
     const snapshotAxes=controls.filter(c=>c.capability.kind==='axis').map(c=>c.capability.id).sort();
     if(snapshot){
      axisValues.clear();
      for(const control of controls)if(control.capability.kind==='axis')axisValues.set(control.capability.id,control.value?.normalized);
     }else{
      for(const id of axes)if(Object.hasOwn(payload?.values??{},id))axisValues.set(id,payload.values[id]?.normalized);
     }
     if(generation===undefined&&snapshot){
      generation=payload?.source?.generation;
      backend=payload?.source?.backend;
      nativeSequence=BigInt(payload.source.sequence);
      axes=snapshotAxes;
      if(payload?.source?.id!==sourceId||!payload.source.baselineReady||!axes.length)errors.push('Wrong controller, no ready baseline or no axes.');
      previousWire=wireSignature();
      // Never timestamp pre-existing DOM as a new report transition.
      previousDom=domSignature();
     }else if(generation!==undefined&&(snapshot||frame.kind==='change')){
      if(payload?.source?.generation!==generation||payload?.source?.id!==sourceId||!payload?.source?.baselineReady)errors.push('Source identity, generation or readiness changed.');
      if(snapshot&&snapshotAxes.join('|')!==axes.join('|'))errors.push('Axis set changed during measurement.');
      const sequence=BigInt(payload.source.sequence),advance=sequence-nativeSequence;
      const capturedUs=BigInt(frame.capturedUs);
      const isReport=payload.reason!=='metadata';
      // Complete same-generation preview snapshots may replace unread state.
      // Routing reads the separate ordered journal. Deltas still require adjacency.
      if(isReport&&snapshot&&advance>1n)coalescedPreviewReports+=Number(advance-1n);
      else if(advance!==(isReport?1n:0n))errors.push('Native controller report continuity changed.');
      nativeSequence=sequence;
      const readUs=isReport&&payload.source.nativeReadUs!==undefined?BigInt(payload.source.nativeReadUs):null;
      const validRead=readUs!==null&&readUs>0n&&readUs<=capturedUs;
      if(isReport){
       reportCount++;
       if(validRead){
        if(previousRead!==undefined){
         if(readUs<previousRead)errors.push('Physical read timestamps regressed.');
         else if(advance===1n)nativeReadIntervals.push(Number(readUs-previousRead)/1000);
        }
        previousRead=readUs;
        readToJournal.push(Number(capturedUs-readUs)/1000);
       }
       else if(backend==='windows-raw-input')errors.push('A physical report is missing its valid native read timestamp.');
      }
      const signature=wireSignature();
      // Equal consecutive values do not establish a new visible transition.
      if(signature!==previousWire){
       if(samples.length<5000)samples.push({
        signature,sequence:payload.source.sequence,captured:Number(capturedUs)/1000+before.offset,
        nativeRead:validRead?Number(readUs)/1000+before.offset:undefined,received,
       });
       else if(!limited){limited=true;errors.push('Timing sample limit exceeded.');}
       previousWire=signature;
      }
     }else if(generation===undefined)errors.push('Expected an initial controller snapshot.');
     previousFrame=frame;
     void ack().catch(error=>errors.push(String(error)));
    },error:message=>errors.push(message)
   },controller.signal);
   await new Promise(resolve=>setTimeout(resolve,preflight?3000:15000));
  }finally{controller.abort();mutation.disconnect();document.removeEventListener('visibilitychange',changedVisibility);window.removeEventListener('blur',blurred);}
  const after=await calibrate();
  if(!inspector.isConnected||inspector.dataset.controllerSourceId!==sourceId)errors.push('Visible inspector was replaced.');
  if(before.id!==after.id||Math.abs(after.offset-before.offset)>before.uncertainty+after.uncertainty)errors.push('Capture-clock correlation changed beyond its brackets.');
  const counts=rows=>{const map=new Map();for(const row of rows)map.set(row.signature,(map.get(row.signature)??0)+1);return map;};
  const wireCounts=counts(samples),domCounts=counts(dom),bySignature=new Map(dom.map(row=>[row.signature,row]));
  // Returning to an earlier value is ambiguous; exclude it instead of attributing
  // an old DOM timestamp to a later report. Missing/coalesced transitions remain counted.
  const rows=samples.map(row=>({...row,...(wireCounts.get(row.signature)===1&&domCounts.get(row.signature)===1?bySignature.get(row.signature):undefined)}));
  const percentiles=values=>{
   values.sort((a,b)=>a-b);
   const p=f=>values.length?values[Math.ceil(values.length*f)-1]:null;
   return {count:values.length,p50:p(.5),p95:p(.95),p99:p(.99),max:p(1)};
  };
  const summarize=(key,from='captured')=>percentiles(
   rows.filter(row=>Number.isFinite(row[key])&&Number.isFinite(row[from])).map(row=>row[key]-row[from]));
  const matched=rows.filter(row=>Number.isFinite(row.dom));
  if(!snapshotCount)errors.push('No native controller snapshot arrived.');
  if(!reportCount)errors.push('No continuing controller reports arrived.');
  if(!preflight&&matched.length<20)errors.push('Fewer than 20 unambiguous visible axis transitions; this run cannot qualify timing.');
  if(matched.some(row=>row.dom-row.captured < -before.uncertainty))errors.push('DOM predates correlated capture beyond clock uncertainty.');
  return {at:new Date().toISOString(),durationMs:performance.now()-started,scope:'profile-controller-inspector',
   sourceId,generation,backend,axes,visibility,focused,preflight,reportCount,coalescedPreviewReports,nativeReadCount:readToJournal.length,
   clockUncertaintyMs:before.uncertainty,
   clockOffsetDriftMs:after.offset-before.offset,snapshotCount,changeCount,changedReportCount:rows.length,domTransitionCount:dom.length,
   journalToObserverMs:summarize('received'),journalToDomMs:summarize('dom'),journalToFrameOpportunityMs:summarize('frameOpportunity'),
   nativeReportIntervalMs:percentiles(nativeReadIntervals),nativeReadToJournalMs:percentiles(readToJournal),
   nativeReadToDomMs:summarize('dom','nativeRead'),
   nativeReadToFrameOpportunityMs:summarize('frameOpportunity','nativeRead'),
   nativeReadToPreviewMeasured:!preflight&&errors.length===0&&matched.filter(row=>Number.isFinite(row.nativeRead)).length>=20,
   qualification:'Passive timing to DOM and next frame opportunity. Native-read metrics start at Raw Input read completion. Physical actuation, display scan-out and virtual output need separate measurements.',
   rawInputLatencyQualified:false,virtualLatencyQualified:false,errors,before,after,rows};
 })()`, preflight?15_000:23_000);
 const directory='.research/reactive/profile-preview-latency';await mkdir(directory,{recursive:true});
 result.executableSha256=createHash('sha256').update(await readFile(process.env.OSG_DESKTOP_NATIVE_APP??'src-tauri/target/debug/osg-cockpit.exe')).digest('hex');
 const path=directory+'/'+Date.now()+'.json';await writeFile(path,JSON.stringify(result,null,2)+'\n');
 const {rows,before,after,...summary}=result;console.log(JSON.stringify({path,...summary},null,2));
 if(result.errors.length)throw Error('Invalid timing run: '+result.errors.join('; '));
}finally{page.close();}
