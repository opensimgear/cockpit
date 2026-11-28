// Opt-in passive capture from the actual running native development Cockpit.
// Uses only controller inventory/input observers and profile_status; no writes.
import {nativePage} from '../../scripts/reactive/cdp.mjs';
import {writeFile,mkdir} from 'node:fs/promises';
import {createHash} from 'node:crypto';
const page=await nativePage();
const hash=value=>createHash('sha256').update(value).digest('hex').slice(0,16);
try {
  const data=await page.evaluate(`(async()=>{
    const {gatewayRequest}=await import('/src/lib/gateway.ts');
    const {observeControllerInputs}=await import('/src/lib/controller-observations.ts');
    const before=(await gatewayRequest({command:'profile_status'})).profiles.profileSession;
    if(before.owner!==null)throw Error('Capture requires no applied profile; this script will not stop outputs.');
    const inventory=(await gatewayRequest({command:'controller_list',offset:0,limit:64})).controllers;
    const physical=(inventory.sources??[]).filter(s=>s.online&&!(s.vendorId===0x1234&&s.productId===0xbead));
    if(!physical.length)throw Error('No actual attached non-vJoy game controllers.');
    if(physical.length>8)throw Error('Bounded capture allows at most eight physical sources.');
    const samples={},states={},stops=[],started=new Date().toISOString();
    try{
      for(const source of physical){samples[source.id]=[];
        stops.push(observeControllerInputs(source.id,state=>{
          states[source.id]={status:state.status,error:state.error};
          if(state.status==='ready'&&state.detail?.source?.baselineReady){
            const rows=samples[source.id],detail=structuredClone(state.detail);
            if(rows.length<128&&rows.at(-1)?.detail?.source?.sequence!==detail.source.sequence)
              rows.push({observedMs:performance.now(),detail});
          }
        }));
      }
      await new Promise(resolve=>setTimeout(resolve,6000));
      const after=(await gatewayRequest({command:'profile_status'})).profiles.profileSession;
      if(after.owner!==null)throw Error('Profile owner changed externally during capture.');
      return {started,ended:new Date().toISOString(),durationMs:6000,
        title:document.title,origin:location.origin,inventory:physical,samples,states,
        profileOwnerBefore:before.owner,profileOwnerAfter:after.owner};
    }finally{stops.forEach(stop=>stop());}
  })()`);
  // Device paths/serials and machine-specific stable IDs never enter the corpus.
  const redactDetail=detail=>{
    if(detail.source)detail.source.id=`source-${hash(detail.source.id)}`;
    if(detail.identity)detail.identity={...detail.identity,path:'[redacted device path]',
      serial:detail.identity.serial===null?null:'[redacted serial]',fingerprint:'[redacted fingerprint]'};
    return detail;
  };
  data.inventory=data.inventory.map(source=>({...source,id:`source-${hash(source.id)}`}));
  data.samples=Object.fromEntries(Object.entries(data.samples).map(([id,rows])=>[
    `source-${hash(id)}`,rows.map(row=>({...row,detail:redactDetail(row.detail)}))]));
  data.states=Object.fromEntries(Object.entries(data.states).map(([id,state])=>[`source-${hash(id)}`,state]));
  data.evidenceKind='OS-parsed native Cockpit controller snapshots; not raw descriptor bytes';
  await mkdir(new URL('.generated/',import.meta.url),{recursive:true});
  await writeFile(new URL('.generated/cockpit-parsed.json',import.meta.url),JSON.stringify(data,null,2)+'\n');
  console.log(JSON.stringify({inventory:data.inventory.map(s=>({name:s.name,vendorId:s.vendorId,productId:s.productId,backend:s.backend})),samples:Object.fromEntries(Object.entries(data.samples).map(([id,rows])=>[id,rows.length])),states:data.states}));
}finally{page.close();}
