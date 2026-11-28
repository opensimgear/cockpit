// Opt-in Linux WebKitGTK/native gateway qualification. No process/app/device auto-selection.
import assert from 'node:assert/strict';
import {execFile,execFileSync} from 'node:child_process';
import {promisify} from 'node:util';
import {readFile,mkdir,writeFile,stat} from 'node:fs/promises';
import {createHash,randomUUID} from 'node:crypto';
import {fileURLToPath,pathToFileURL} from 'node:url';
import {resolve,join,isAbsolute} from 'node:path';
import {webKitPage} from '../../scripts/reactive/webkit.mjs';
import {compareEvidence,framingFor,validateProbe,inventoryEvidence} from './verify-linux-core.mjs';

const root=fileURLToPath(new URL('../../',import.meta.url));
const execute=promisify(execFile);
export function options(argv) {
  const result={durationMs:2000,python:'python3',decoder:join(root,'crates/hid-report/target/debug/examples/qualify')};
  for(let index=0;index<argv.length;index++) {
    const argument=argv[index];
    if(argument==='--allow-hardware-read')result.allow=true;
    else if(argument==='--inventory-only')result.inventoryOnly=true;
    else if(['--device','--webkit','--output','--python','--decoder','--duration-ms'].includes(argument)) {
      const value=argv[++index];assert.ok(value&&!value.startsWith('--'),`missing ${argument}`);
      result[{'--duration-ms':'durationMs'}[argument]??argument.slice(2)]=value;
    } else throw Error(`Unknown argument ${argument}`);
  }
  if(result.inventoryOnly) {
    assert.ok(!result.allow&&!result.device,'inventory-only mode does not accept a device or hardware-read flag');
  } else {
    assert.equal(result.allow,true,'Explicit --allow-hardware-read is required');
    assert.match(result.device??'',/^\/dev\/hidraw[0-9]+$/,'select one exact current hidraw node');
  }
  const endpoint=new URL(result.webkit);
  assert.equal(endpoint.protocol,'http:');assert.equal(endpoint.hostname,'127.0.0.1','private WebKit inspector only');
  result.durationMs=Number(result.durationMs);
  assert.ok(Number.isInteger(result.durationMs)&&result.durationMs>=100&&result.durationMs<=10000);
  assert.ok(result.output&&isAbsolute(result.output),'--output must be an explicit absolute private candidate directory');
  assert.ok(isAbsolute(result.decoder),'--decoder must be an absolute maintained verification executable');
  return result;
}
function portable(decoder,descriptorHex,records,framing) {
  const reports=records.map(record=>({kind:'input',framing,wireHex:record.wireHex}));
  const input=JSON.stringify({descriptorHex,reports});
  assert.ok(Buffer.byteLength(input)<=20*1024*1024);
  return JSON.parse(execFileSync(decoder,[],{input,encoding:'utf8',timeout:30000,maxBuffer:34*1024*1024}));
}
async function profileStopped(page,clientId) {
  const value=await page.evaluate(`window.__TAURI_INTERNALS__.invoke('gateway_request',{request:${JSON.stringify({command:'profile_status',clientId})}})`);
  assert.equal(value.ok,true,value.error);
  assert.equal(value.state?.profiles?.profileSession?.owner,null,'qualification requires stopped profiles; runner never stops them');
}
async function saveCandidate(directory,name,value) {
  const text=JSON.stringify(value,null,2)+'\n';
  assert.ok(Buffer.byteLength(text)<=64*1024*1024,'candidate recording exceeds the64MiB bound');
  await mkdir(directory,{recursive:true});
  const path=join(directory,name);
  await writeFile(path,text,{flag:'wx',mode:0o600});
  return path;
}
async function qualify(args) {
  assert.equal(process.platform,'linux','Real Linux acceptance cannot run on a fixture/build host');
  let decoderHash;
  if(!args.inventoryOnly) {
    const decoderMetadata=await stat(args.decoder);assert.ok(decoderMetadata.isFile()&&decoderMetadata.size<=512*1024*1024);
    decoderHash=createHash('sha256').update(await readFile(args.decoder)).digest('hex');
  }
  const page=await webKitPage(args.webkit),clientId=`hidraw-qualification-${randomUUID()}`;
  const request=async(command,fields={})=>{
    const payload={command:`controller_hid_${command}`,clientId,...fields};
    const value=await page.evaluate(`window.__TAURI_INTERNALS__.invoke('gateway_request',{request:${JSON.stringify(payload)}})`);
    assert.equal(value.ok,true,value.error);return value.state.controllers;
  };
  let opened,probeTask,probeFinished=false;
  try {
    const inventory=await request('list');
    if(args.inventoryOnly) {
      const receipt={...inventoryEvidence(inventory),generatedAt:new Date().toISOString(),
        runningBinarySourceRevision:'not-established-by-this-runner'};
      const evidence=await saveCandidate(args.output,'linux-hidraw-inventory.json',receipt);
      console.log(JSON.stringify({interfaces:inventory.interfaces.length,truncated:inventory.truncated,
        failures:inventory.failures.length,hardwareAcceptance:'not-run',deviceOpenedByRunner:false,evidence}));
      return;
    }
    await profileStopped(page,clientId);
    const matches=inventory.interfaces.filter(row=>row.path===args.device);
    assert.equal(matches.length,1,'selected exact hidraw path must resolve once; no VID/PID or product guessing');
    const interfaceId=matches[0].interfaceId;
    const detail=await request('inspect',{interfaceId});
    assert.equal(detail.interfaceId,interfaceId);assert.ok(detail.descriptorHex);
    const framing=framingFor(detail.descriptor);
    // Validate portable binary before opening either capture reader.
    assert.deepEqual(portable(args.decoder,detail.descriptorHex,[],framing).descriptor,detail.descriptor);
    opened=await request('open',{interfaceId});
    assert.equal(opened.interfaceId,interfaceId);assert.ok(typeof opened.generation==='string'&&opened.generation.length>0);
    probeTask=execute(args.python,[fileURLToPath(new URL('./verify-linux-probe.py',import.meta.url)),
      '--device',args.device,'--allow-hardware-read','--duration-ms',String(args.durationMs)],
      {encoding:'utf8',timeout:args.durationMs+5000,killSignal:'SIGKILL',maxBuffer:24*1024*1024});
    // Observe rejection immediately even if native polling fails before awaiting the probe.
    probeTask.finally(()=>{probeFinished=true;}).catch(()=>{});
    const records=[];let retained=0,after='0';
    const deadline=Date.now()+args.durationMs+250;
    while(Date.now()<deadline) {
      const response=await request('reports',{interfaceId,generation:opened.generation,after});
      const current=response.result;
      assert.equal(current.gap,false,'native history lost reports during this acceptance window');
      assert.equal(current.terminal,null,'native capture ended before acceptance finished');
      for(const record of current.reports) {
        retained+=Buffer.byteLength(JSON.stringify(record));
        assert.ok(records.length<4096&&retained<=8*1024*1024,'qualification evidence retention bound exceeded');
        records.push(record);after=record.sequence;
      }
      await new Promise(resolve=>setTimeout(resolve,20));
    }
    const probe=JSON.parse((await probeTask).stdout);validateProbe(probe);
    const final=await request('inspect',{interfaceId});
    assert.equal(final.descriptorHex,detail.descriptorHex,'descriptor changed while running');
    await profileStopped(page,clientId);
    // Bounded cross-entry interpretation sample; complete raw capture count remains explicit.
    const independent=new Set(probe.reports.map(report=>report.wireHex));
    const compared=records.filter(record=>independent.has(record.wireHex)).slice(0,64);
    const oracle=portable(args.decoder,probe.descriptorHex,compared,framing);
    const checks=compareEvidence(probe,detail,compared,oracle);
    const generation=opened.generation;
    const closed=await request('close',{interfaceId,generation});
    assert.equal(closed.result?.closed,true,'native receiver must acknowledge closure');opened=undefined;
    const receipt={origin:'real-linux-hidraw-and-native-webkit-gateway',fixtureEvidence:false,
      releaseQualified:false,hardwareScope:'One explicitly selected interface and observed input reports only',
      generatedAt:new Date().toISOString(),decoderSha256:decoderHash,
      runningBinarySourceRevision:'not-established-by-this-runner',device:args.device,
      checks,interfaceId,generation,inventoryTruncated:inventory.truncated,inventoryFailures:inventory.failures,
      nativeCaptureClosed:true,nativeRetainedReports:records.length,nativeRetainedBytes:retained,probe,nativeReports:records,
      comparisonSelection:'First at most64 native reports whose exact bytes also occurred in the independent window',
      comparedReportSequences:compared.map(record=>record.sequence),portable:oracle,
      limits:{durationMs:args.durationMs,reportsPerReader:4096,bytesPerReader:8*1024*1024,portableComparedReports:64}};
    const evidence=await saveCandidate(args.output,'linux-hidraw-receiver.json',receipt);
    console.log(JSON.stringify({...checks,evidence,releaseQualified:false}));
  } finally {
    if(probeTask&&!probeFinished)probeTask.child?.kill('SIGKILL');
    try {if(opened)await request('close',{interfaceId:opened.interfaceId,generation:opened.generation});}
    finally {page.close();}
  }
}
if(process.argv[1]&&import.meta.url===pathToFileURL(resolve(process.argv[1])).href) {
  qualify(options(process.argv.slice(2))).catch(error=>{console.error(error.message);process.exitCode=1;});
}
