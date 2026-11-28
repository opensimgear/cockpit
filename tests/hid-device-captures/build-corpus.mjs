// Normalize local captures into compact, redacted, deterministic fixture records.
import {readFile,writeFile,mkdir} from 'node:fs/promises';
import {createHash} from 'node:crypto';
const json=async name=>JSON.parse((await readFile(new URL(name,import.meta.url),'utf8')).replace(/^\uFEFF/,''));
const raw=await json('.generated/raw-input.json'),parsed=await json('.generated/cockpit-parsed.json');
const sha=value=>createHash('sha256').update(value).digest('hex');
if(raw.physicalOverflow)throw Error('Raw collector overflowed; do not publish a partial corpus.');
if(raw.durationMs<5000||raw.durationMs>15000||raw.physicalReports.length>512)throw Error('Raw capture exceeds validated bounds.');
await mkdir(new URL('.generated/candidates/',import.meta.url),{recursive:true});
// One compact row per array item keeps representative capabilities/samples legible
// and fixtures under the repository 500-line limit without changing their values.
function format(value,depth=0){
  const pad='  '.repeat(depth),next='  '.repeat(depth+1);
  if(Array.isArray(value))return value.length?'[\n'+value.map(row=>next+JSON.stringify(row)).join(',\n')+'\n'+pad+']':'[]';
  if(value&&typeof value==='object')return '{\n'+Object.entries(value).map(([key,row])=>next+JSON.stringify(key)+': '+format(row,depth+1)).join(',\n')+'\n'+pad+'}';
  return JSON.stringify(value);
}
const start=Math.min(...raw.physicalReports.map(row=>row.receivedTicks));
for(const source of parsed.inventory){
  const candidates=raw.devices.filter(d=>!d.virtualVjoy&&d.vendor===source.vendorId&&d.product===source.productId);
  if(candidates.length!==1)throw Error('Ambiguous VID/PID match; require explicit per-interface mapping.');
  const device=candidates[0],reports=raw.physicalReports.filter(row=>row.device===device.id),snapshots=parsed.samples[source.id];
  if(!reports.length||!snapshots?.length)throw Error('Capture requires both actual raw reports and parsed snapshots.');
  const hex=value=>value.toString(16).padStart(4,'0');
  const fixtureId=`${hex(source.vendorId)}-${hex(source.productId)}-idle`;
  const rawStates=[],parsedStates=[];
  const intern=(values,list)=>{const key=JSON.stringify(values);let index=list.findIndex(row=>JSON.stringify(row.values)===key);
    if(index<0){index=list.length;list.push({id:index,values,sha256:sha(key)});}return index;};
  const rawSamples=reports.map(row=>({ordinal:row.reportOrdinal,elapsedMs:(row.receivedTicks-start)*1000/raw.tickHz,
    stateId:intern(row.reportHex,rawStates)}));
  const first=snapshots[0];
  const parsedSamples=snapshots.map(row=>({sequence:row.detail.source.sequence,
    elapsedMs:row.observedMs-first.observedMs,nativeReadUs:row.detail.source.nativeReadUs,
    stateId:intern(row.detail.controls.map(control=>control.value),parsedStates)}));
  const transitions=samples=>samples.slice(1).filter((row,index)=>row.stateId!==samples[index].stateId).length;
  const unchanged=transitions(rawSamples)===0&&transitions(parsedSamples)===0;
  const fixture={fixtureId,device:{productName:source.name.trim(),vendorId:source.vendorId,productId:source.productId,
    usagePage:1,usage:device.usage,transport:'Windows Raw Input HID game-controller collection',virtual:false},
    provenance:{platform:'Windows x64',backend:source.backend,rawStarted:raw.started,rawEnded:raw.ended,
      parsedStarted:parsed.started,parsedEnded:parsed.ended,
      collectionPolicy:'Passive, no operator-requested actuation; '+(unchanged?'report content unchanged during each captured window.':'content changed; inspect observedTransitions before promoting this candidate.'),
      keyboardMouseCaptured:false,outputWrites:false,featureReads:false,deviceFileOpenAttempted:false,
      rawSource:'WM_INPUT RAWHID input-report payload copied from GetRawInputData; not a USB bus capture.',
      capabilitiesSource:'Actual Cockpit Windows HidP/OS-parsed capabilities; not raw HID report descriptor bytes.',
      independentWindows:true,correlation:'Separate raw and parsed capture windows; no per-report synchronization or decoder equivalence claim.',
      redactions:['OS device paths','serial/fingerprint','machine stable controller IDs','foreground process','DirectInput instance IDs']},
    descriptor:{rawReportDescriptorAvailable:false,osParsedCapabilities:first.detail.controls.map(control=>control.capability)},
    rawInput:{durationMs:raw.durationMs,tickHz:raw.tickHz,reportCount:reports.length,reportBytes:[...new Set(reports.map(row=>row.reportBytes))],
      uniqueReportCount:rawStates.length,observedTransitions:transitions(rawSamples),states:rawStates.map(row=>({id:row.id,
        reportHex:row.values,sha256:sha(Buffer.from(row.values,'hex'))})),samples:rawSamples},
    cockpitParsed:{durationMs:parsed.durationMs,snapshotCount:snapshots.length,controlCount:first.detail.controls.length,
      baselineReady:first.detail.source.baselineReady,diagnostics:first.detail.diagnostics??[],
      observedTransitions:transitions(parsedSamples),states:parsedStates,samples:parsedSamples},
    qualification:{idleReports:true,physicalTransitions:false,fullTravel:false,hatDirections:false,
      reconnect:false,multiReportIdCoverage:false,rawDescriptorParser:false,outputBehavior:false}};
  const output=format(fixture)+'\n';
  if(output.split('\n').length>=500)throw Error('Candidate exceeds repository line limit; review capture selection.');
  await writeFile(new URL(`.generated/candidates/${fixtureId}.json`,import.meta.url),output);
  console.log(JSON.stringify({fixtureId,raw:reports.length,parsed:snapshots.length,rawTransitions:fixture.rawInput.observedTransitions,
    parsedTransitions:fixture.cockpitParsed.observedTransitions,controls:fixture.cockpitParsed.controlCount}));
}
