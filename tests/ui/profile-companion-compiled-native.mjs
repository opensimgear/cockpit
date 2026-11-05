// Use an isolated running Cockpit with its companion listener enabled on loopback.
import test from 'node:test';
import assert from 'node:assert/strict';
import {createRequire} from 'node:module';
import {randomUUID,createHash,X509Certificate} from 'node:crypto';
import {connect} from 'node:tls';
import {readFile} from 'node:fs/promises';
import {nativePage,until} from '../../scripts/reactive/cdp.mjs';
test('compiled browser input reaches the profile and returns host feedback', {timeout:90000}, async()=>{
  const cockpit=await nativePage();
  const clientId='compiled-companion-qualification';
  const request=async(command,params={})=>{
   const reply=await cockpit.evaluate(`window.__TAURI_INTERNALS__.invoke('gateway_request',{request:${JSON.stringify({command,clientId,...params})}})`);
   assert.equal(reply.ok,true,reply.error??command);
   return reply.state;
  };
  const companion=async(command,params={})=>(await request(command,{companion:params})).companions;
  const id='reactive-'+randomUUID();
  const publication=JSON.parse(await readFile(new URL('../../scripts/fixtures/compiled-flight-publication.json',import.meta.url)));
  const {chromium}=createRequire(import.meta.url)(process.env.OSG_PLAYWRIGHT_MODULE??'C:/Users/ivang/.cache/codex-runtimes/codex-primary-runtime/dependencies/node/node_modules/playwright');
  let browser, peer, profile;
  try {
   const initial=await companion('companion_status');
   assert.equal(initial.listener?.enabled,true,'Enable the isolated companion listener first');
   assert.equal((await request('profile_status')).profiles.profileSession.state,'stopped');
   const existing=new Set(initial.host.clients.map(row=>row.companionId));
   const offer=await companion('companion_pair_offer',{kind:'browser'});
   const url=new URL(offer.pairing.browserUrl);
   assert.equal(url.hostname,'127.0.0.1');
   // Pin only this test listener in the disposable browser process.
   const pin=await new Promise((resolve,reject)=>{
    const socket=connect({host:url.hostname,port:Number(url.port),rejectUnauthorized:false},()=>{
     try{resolve(createHash('sha256').update(new X509Certificate(socket.getPeerCertificate().raw).publicKey.export({type:"spki",format:"der"})).digest('base64'));}catch(error){reject(error);}finally{socket.destroy();}
    });socket.on('error',reject);
   });
   browser=await chromium.launch({channel:process.env.OSG_BROWSER_CHANNEL??'msedge',headless:true,args:['--ignore-certificate-errors-spki-list='+pin]});
   const page=await browser.newPage();
   await page.addInitScript(()=>{
    window.__feedbackProbe=[];
    const Original=window.SharedWorker;
    window.SharedWorker=class extends Original {
     constructor(...args){
      super(...args);
      this.port.addEventListener('message',({data})=>{
       const frame=data?.reply?.feedback;
       if(frame){
        window.__feedbackProbe.push(frame);
        if(window.__feedbackProbe.length>64)window.__feedbackProbe.shift();
       }
      });
     }
    };
   });
   await page.goto(url.href,{waitUntil:'domcontentloaded'});
   peer=await until(async()=>(await companion('companion_status')).host.clients.find(row=>!existing.has(row.companionId)),Boolean,15000)
    .catch(async error=>{throw new Error('Browser pairing did not finish: '+await page.locator('body').innerText(),{cause:error});});
   const bytes=Buffer.from(JSON.stringify(publication));
   const {uploadId}=await companion('companion_publish_begin',{size:bytes.length});
   for(let offset=0;offset<bytes.length;offset+=8192)await companion('companion_publish_chunk',{uploadId,offset,bytes:[...bytes.subarray(offset,offset+8192)]});
   await companion('companion_publish_commit',{uploadId,saveAsLatest:false});
   await companion('companion_assign',{companionId:peer.companionId,instanceId:id,publicationHash:publication.publicationHash});
   await page.getByText('Connected · panel active',{exact:true}).waitFor({timeout:15000});
   const row=await until(async()=>(await request('profile_resources_list')).profileResources.resources.find(row=>row.attributes?.companionId===peer.companionId),Boolean,10000);
   const details=await request('profile_resources_inspect',{resourceId:row.id});
   const selected=details.profileResources.ports.filter(p=>['flight-input-0','feedback:flight-channel-0:flight-output-0'].includes(p.descriptor.id));
   assert.equal(selected.length,2);
   const document=(await request('profile_create',{name:'Compiled companion activation '+Date.now()})).profiles.document;
   profile=document;
   const ports=selected.map(p=>({...p.descriptor,id:p.descriptor.direction==='output'?'button':'lamp'}));
   document.resources.push({id:'browser-panel',name:'Compiled browser panel',kind:'companion',required:true,ports});
   document.companionAssignments.push({resourceId:'browser-panel',publicationHash:publication.publicationHash});
   document.bindings.push({id:'button-lamp',from:{nodeId:'browser-panel',portId:'button'},to:{nodeId:'browser-panel',portId:'lamp'}});
   const assignments=[{resourceId:'browser-panel',sourceId:row.id,ports:selected.map((p,i)=>({profilePortId:ports[i].id,observedPortId:p.descriptor.id,expectedContractHash:p.contractHash}))}];
   profile=(await request('profile_editor_save',{document,assignments,expectedRevision:document.revision})).profiles.document;
   const applied=await request('profile_apply',{id:profile.id,expectedRevision:profile.revision,expectedActiveOwner:null,activationId:randomUUID()});
   assert.equal(applied.profiles.profileSession.state,'running');
   await page.getByText('Connected · panel active',{exact:true}).waitFor({timeout:15000});
   const owner=applied.profiles.profileSession.owner;
   await page.getByRole('button',{name:'probe: press',exact:true}).click();
   await until(()=>page.evaluate(()=>window.__feedbackProbe.some(frame=>frame.values.some(value=>
    value.channelId==='flight-channel-0'&&value.outputId==='flight-output-0'
    &&value.value===true&&value.status==='fresh'&&value.provenance==='local'&&value.sourceKind==='host'))),Boolean,7000);
   const inspection=(await request('profile_runtime_inspect',{owner,nodeId:'browser-panel'})).profileInspection;
   assert.deepEqual(inspection.ports.map(port=>port.sample.value.value),[true,true]);
   const readInstance=async()=>(await companion('companion_status')).host.instances.find(row=>row.instanceId===id);
   const before=await readInstance();
   assert.equal(before.values['flight-input-0'],true);
   assert.equal(before.profileOwned,true);
   await page.reload({waitUntil:'domcontentloaded'});
   await page.getByText('Connected · panel active',{exact:true}).waitFor({timeout:15000});
   await until(readInstance,row=>row?.connected&&row.armed&&row.values['flight-input-0']===false,15000);
   await until(async()=>{
    const {profileInspection}=await request('profile_runtime_inspect',{owner,nodeId:'browser-panel'});
    return profileInspection.ports.every(port=>port.sample.value?.value===false);
   },Boolean,10000);
   const stopped=await request('profile_stop');
   assert.equal(stopped.profiles.profileSession.state,'stopped');
   assert.equal(stopped.profiles.profileSession.owner,null);
  } finally {
   if(profile){await request('profile_stop').catch(()=>{});await request('profile_delete',{id:profile.id,expectedRevision:profile.revision}).catch(()=>{});}
   if(peer)await companion('companion_unassign',{instanceId:id}).catch(()=>{});
   if(peer)await companion('companion_revoke',{companionId:peer.companionId}).catch(()=>{});
   await browser?.close();cockpit.close();
  }

});
