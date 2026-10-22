// Running dev Cockpit + its real authenticated WSS service + one unchanged C
// firmware/LVGL software client binary. This is software acceptance only.
import test from 'node:test';
import assert from 'node:assert/strict';
import {createRequire} from 'node:module';
import {spawn} from 'node:child_process';
import {createInterface} from 'node:readline';
import {createServer} from 'node:net';
import {randomUUID,createHash} from 'node:crypto';
import {mkdir,readFile,writeFile} from 'node:fs/promises';
import {resolve,join} from 'node:path';
import {deflateSync} from 'node:zlib';
import {TransferDecoder} from '../../apps/browser-companion/src/transport.ts';
import {newDocument,createDesignerHistory,applyDesignerCommand,createWidgetCatalog,publishDocument,importBundle,normalizeToPortableUi} from '../../packages/companion-designer/src/index.ts';
import {fallbackValue} from '../../packages/companion-ui/src/controller.ts';
import {sendPublication} from '../../src/lib/companions.ts';

const pause=ms=>new Promise(resolve=>setTimeout(resolve,ms));
async function native(){
  const targets=await (await fetch((process.env.OSG_WIDGETS_CDP??'http://127.0.0.1:9232')+'/json/list',{signal:AbortSignal.timeout(5000)})).json();
  const origin=process.env.OSG_WIDGETS_ORIGIN??'http://127.0.0.1:1422';
  const target=targets.find(target=>target.type==='page'&&new URL(target.url).origin===origin);assert.ok(target,'Start the isolated widget dev Cockpit.');
  const socket=new WebSocket(target.webSocketDebuggerUrl),pending=new Map();let sequence=0;
  await new Promise((resolve,reject)=>{socket.addEventListener('open',resolve,{once:true});socket.addEventListener('error',reject,{once:true});});
  socket.addEventListener('message',event=>{const reply=JSON.parse(event.data),operation=pending.get(reply.id);if(!operation)return;pending.delete(reply.id);clearTimeout(operation.timer);reply.error?operation.reject(Error(reply.error.message)):operation.resolve(reply.result);});
  const send=(method,params={})=>new Promise((resolve,reject)=>{const id=++sequence,timer=setTimeout(()=>{pending.delete(id);reject(Error(method+' timed out'));},15000);pending.set(id,{resolve,reject,timer});socket.send(JSON.stringify({id,method,params}));});
  const evaluate=async expression=>{const reply=await send('Runtime.evaluate',{expression,returnByValue:true,awaitPromise:true});if(reply.exceptionDetails)throw Error(reply.exceptionDetails.exception?.description??reply.exceptionDetails.text);return reply.result.value;};
  return {send,evaluate,close:()=>socket.close(),request:async(command,companion={})=>{
    const reply=await evaluate(`window.__TAURI_INTERNALS__.invoke('gateway_request',{request:${JSON.stringify({command,companion,clientId:'widget-software-native'})}})`);
    assert.equal(reply.ok,true,reply.error??command);return reply.state.companions;
  }};
}
async function wss(RawSocket,endpoint,certificate){
  const socket=new RawSocket(endpoint,'opensimgear-companion-v1',{ca:certificate,rejectUnauthorized:true,handshakeTimeout:5000,maxPayload:16384});
  await new Promise((resolve,reject)=>{socket.once('open',resolve);socket.once('error',reject);});
  const decoder=new TransferDecoder();let pending,tail=Promise.resolve();
  const fail=error=>{if(pending){clearTimeout(pending.timer);pending.reject(error);pending=undefined;}};
  socket.on('error',fail);socket.on('close',()=>fail(Error('WSS connection closed')));
  socket.on('message',bytes=>{tail=tail.then(async()=>{
    assert.ok(bytes.length<=16384);const reply=await decoder.accept(JSON.parse(bytes.toString()));if(!reply)return;
    assert.ok(pending,'Unexpected host reply');const operation=pending;pending=undefined;clearTimeout(operation.timer);operation.resolve(reply);
  }).catch(fail);});
  return {request:frame=>new Promise((resolve,reject)=>{assert.ok(!pending);const timer=setTimeout(()=>{pending=undefined;reject(Error('WSS request timed out'));},4000);pending={resolve,reject,timer};socket.send(JSON.stringify(frame));}),close:async()=>{
    if(socket.readyState===RawSocket.CLOSED)return;
    const closed=new Promise(resolve=>socket.once('close',resolve));socket.close();await Promise.race([closed,pause(1000)]);if(socket.readyState!==RawSocket.CLOSED)socket.terminate();
  }};
}
async function freePort(){const server=createServer();await new Promise(resolve=>server.listen(0,'127.0.0.1',resolve));const port=server.address().port;await new Promise(resolve=>server.close(resolve));return port;}
function icon(){
  const crc=bytes=>{let value=0xFFFFFFFF;for(const byte of bytes){value^=byte;for(let bit=0;bit<8;bit++)value=(value>>>1)^((value&1)?0xEDB88320:0);}return (value^0xFFFFFFFF)>>>0;};
  const chunk=(name,data)=>{const body=Buffer.concat([Buffer.from(name),data]),size=Buffer.alloc(4),sum=Buffer.alloc(4);size.writeUInt32BE(data.length);sum.writeUInt32BE(crc(body));return Buffer.concat([size,body,sum]);};
  const header=Buffer.alloc(13);header.writeUInt32BE(8);header.writeUInt32BE(8,4);header[8]=8;header[9]=6;
  const rows=Buffer.alloc(8*(1+8*4));for(let y=0;y<8;y++)for(let x=0;x<8;x++){const at=y*33+1+x*4;rows[at]=x===y||x+y===7?255:25;rows[at+1]=190;rows[at+2]=160;rows[at+3]=255;}
  return Buffer.concat([Buffer.from([137,80,78,71,13,10,26,10]),chunk('IHDR',header),chunk('IDAT',deflateSync(rows)),chunk('IEND',Buffer.alloc(0))]);
}
const momentary=panel=>panel.document.presentation.widgets.findLast(widget=>widget.kind==='component'&&panel.document.presentation.definitions.find(definition=>definition.id===widget.config.definitionId&&definition.version===widget.config.definitionVersion).interaction.kind==='momentary');
const touchPoint=widget=>({x:Math.round((widget.bounds.x+widget.bounds.width/2)*800),y:Math.round((widget.bounds.y+widget.bounds.height/2)*480)});
async function exportedCompositions(file){
  const incoming=await importBundle(await readFile(file,'utf8'));
  const first=await publishDocument(incoming.document,incoming.assets),original=momentary(first);
  assert.ok(original,'The exported acceptance panel needs a reusable momentary control.');
  const bytes=icon(),assetId='software-acceptance-'+randomUUID(),document=structuredClone(first.document);
  document.ui.assets.push({id:assetId,sha256:createHash('sha256').update(bytes).digest('hex'),sizeBytes:bytes.length,mime:'image/png'});
  let history=createDesignerHistory(document);
  const definition=structuredClone(first.document.presentation.definitions.find(definition=>definition.id===original.config.definitionId&&definition.version===original.config.definitionVersion));
  definition.version=Math.max(...first.document.presentation.definitions.filter(entry=>entry.id===definition.id).map(entry=>entry.version))+1;
  const label=definition.layers.find(layer=>layer.kind==='text'&&layer.text?.property==='label');assert.ok(label);label.font='mono';label.fontSize=20;
  definition.layers.push({id:'new-composition-lamp',label:'New decoration',kind:'ellipse',bounds:{x:.08,y:.72,width:.08,height:.12},fill:{property:'accent'}});
  definition.layers.push({id:'new-composition-artwork',label:'New artwork',kind:'image',bounds:{x:.82,y:.72,width:.12,height:.18},assetId});
  history=applyDesignerCommand(history,{type:'update-component',id:original.id,definition,properties:{...original.config.properties,label:'ENGINE B',accent:'#ff765e'}});
  history=applyDesignerCommand(history,{type:'move-widget',id:original.id,bounds:{x:.745,y:.375,width:.22,height:.21}});
  const second=await publishDocument(history.present,[...incoming.assets,{assetId,base64:bytes.toString('base64')}],{optimizeAssets:false});
  assert.equal(first.capabilityFingerprint,second.capabilityFingerprint,'Font, layout, new decoration and artwork changes preserve input/feedback contracts');
  assert.deepEqual(momentary(first).controlIds,momentary(second).controlIds);
  return [first,second];
}
async function compositions(){
  if(process.env.OSG_WIDGETS_SOFTWARE_BUNDLE)return exportedCompositions(process.env.OSG_WIDGETS_SOFTWARE_BUNDLE);
  let history=createDesignerHistory(newDocument('Software shared controls'));
  history=applyDesignerCommand(history,{type:'delete-widget',id:history.present.presentation.widgets[0].id});
  const pageId=history.present.ui.pages[0].id;
  history=applyDesignerCommand(history,{type:'set-viewport',pageId,width:800,height:480});
  history=applyDesignerCommand(history,{type:'add-component',definition:createWidgetCatalog()[0],properties:{label:'ENGINE'},bounds:{x:.1,y:.15,width:.35,height:.5}});
  const first=await publishDocument(history.present,[]);
  history=applyDesignerCommand(history,{type:'duplicate-widget',id:history.present.presentation.widgets[0].id});
  const widget=history.present.presentation.widgets.at(-1);
  history=applyDesignerCommand(history,{type:'update-widget',id:widget.id,patch:{bounds:{x:.55,y:.15,width:.35,height:.5},config:{...widget.config,properties:{label:'AUX',accent:'#ff765e'}}}});
  const bytes=icon(),assetId='acceptance-icon';
  const document=structuredClone(history.present);document.ui.assets.push({id:assetId,sha256:createHash('sha256').update(bytes).digest('hex'),sizeBytes:bytes.length,mime:'image/png'});
  history=applyDesignerCommand(history,{type:'replace',document});
  history=applyDesignerCommand(history,{type:'add-widget',kind:'image',bounds:{x:.82,y:.75,width:.1,height:.15}});
  history=applyDesignerCommand(history,{type:'update-widget',id:history.present.presentation.widgets.at(-1).id,patch:{config:{assetId}}});
  const second=await publishDocument(history.present,[{assetId,base64:bytes.toString('base64')}],{optimizeAssets:false});
  assert.equal(first.document.presentation.widgets[0].id,second.document.presentation.widgets[0].id);
  assert.notEqual(second.document.presentation.widgets[0].controlIds[0],second.document.presentation.widgets[1].controlIds[0]);
  return [first,second];
}

test('one actual firmware/LVGL software client loads two source compositions from the running Cockpit',{
  skip:process.env.OSG_WIDGETS_SOFTWARE_NATIVE!=='1',timeout:120000,
},async()=>{
  const require=createRequire(import.meta.url),{ws:RawSocket}=createRequire(require.resolve(process.env.OSG_PLAYWRIGHT_MODULE??'playwright'))('playwright-core/lib/utilsBundle');
  const api=await native(),companionId='software-'+randomUUID(),instanceId='software-'+randomUUID();
  const directory=resolve('.research/widget-software-native',String(Date.now()));await mkdir(directory,{recursive:true});
  const evidence={actualNativeCockpit:true,realAuthenticatedWss:true,actualFirmwareClient:true,actualLvgl:true,physicalHardware:false,compositions:[],wireTypes:[],status:[]};
  let client,child,createdListener=false,ticker,stepBusy=false,stopping=false,engineError,sequence=0,tail=Promise.resolve();
  const pending=new Map(),events=[],testHashes=new Set(),existingHashes=new Set();let last,stderr='';
  const program=process.env.OSG_WIDGETS_SOFTWARE_BIN;
  assert.ok(program,'Build the software-client target and set OSG_WIDGETS_SOFTWARE_BIN.');
  const binary=process.env.OSG_WIDGETS_SOFTWARE_BINARY_FILE;
  if(binary)evidence.binarySha256=createHash('sha256').update(await readFile(binary)).digest('hex');
  const command=(kind,values={})=>{
    const work=tail.then(()=>new Promise((resolve,reject)=>{
      if(engineError){reject(engineError);return;}
      const id=++sequence,timer=setTimeout(()=>{pending.delete(id);reject(Error('Software client '+kind+' timed out'));},12000);
      pending.set(id,{resolve,reject,timer});child.stdin.write(JSON.stringify({command:kind,requestId:id,...values})+'\n');
    }));tail=work.catch(()=>{});return work;
  };
  const until=async(work,message)=>{for(let i=0;i<250;i++){if(engineError)throw engineError;const result=await work();if(result)return result;await pause(25);}throw Error(message+': '+JSON.stringify(last));};
  try{
    let state=await api.request('companion_status');
    for(const publication of state.host.publications)existingHashes.add(publication.publicationHash);
    if(!state.listener?.enabled){state=await api.request('companion_enable',{address:'127.0.0.1',advertisedHost:'127.0.0.1',port:await freePort(),discovery:false});createdListener=true;}
    const certificate=(await api.request('companion_browser_certificate')).certificatePem;
    const offer=await api.request('companion_pair_offer',{kind:'esp32'}),pairing=JSON.parse(offer.pairing.manualPayload);
    client=await wss(RawSocket,pairing.endpoint,certificate);
    const request=await client.request({type:'pair',secret:pairing.secret,companionId,name:'Software LVGL acceptance'});assert.equal(request.ok,true);
    const paired=await client.request({type:'pair-status',requestId:request.requestId,secret:pairing.secret});assert.equal(paired.ok,true);assert.equal(paired.type,'paired');
    await client.close();client=undefined;
    const executable=process.platform==='win32'?'wsl.exe':program;
    const args=process.platform==='win32'?['-d',process.env.OSG_WIDGETS_WSL_DISTRO??'Ubuntu','--exec',program]:[];
    child=spawn(executable,args,{windowsHide:true,stdio:['pipe','pipe','pipe'],env:{...process.env,UBSAN_OPTIONS:'halt_on_error=1:print_stacktrace=1'}});
    const ended=new Promise(resolve=>child.once('close',(code,signal)=>resolve({code,signal})));
    const fail=error=>{engineError=error;for(const operation of pending.values()){clearTimeout(operation.timer);operation.reject(error);}pending.clear();};
    child.once('error',fail);child.stderr.on('data',bytes=>{stderr+=bytes.toString();});child.once('close',code=>{if(!stopping)fail(Error('Software client exited '+code+': '+stderr));});
    createInterface({input:child.stdout}).on('line',line=>{
      events.push((async()=>{
        const value=JSON.parse(line);
        if(value.bridge){
          let reply;
          if(value.bridge==='open'){client=await wss(RawSocket,pairing.endpoint,certificate);reply={ok:true};}
          else if(value.bridge==='close'){await client?.close();client=undefined;reply={ok:true};}
          else{
            evidence.wireTypes.push(value.frame.type);
            reply=await client.request(value.frame);
          }
          child.stdin.write(JSON.stringify(reply)+'\n');return;
        }
        if(value.requestId){const operation=pending.get(value.requestId);assert.ok(operation);pending.delete(value.requestId);clearTimeout(operation.timer);last=value;operation.resolve(value);}
        else evidence.status.push({status:value.status,phase:value.operation?.phase});
      })().catch(fail));
    });
    const initial=await command('status');assert.equal(initial.armed,false);assert.equal(initial.renderProfile.boardProfile,'software-lvgl');
    evidence.profile=initial.renderProfile;evidence.processId=initial.processId;
    await command('install',{hostId:paired.hostId,companionId,token:paired.token});
    ticker=setInterval(()=>{if(stepBusy||stopping)return;stepBusy=true;command('step').catch(error=>{engineError=error;}).finally(()=>{stepBusy=false;});},20);
    await until(async()=>{const current=await api.request('companion_status');return current.host.clients.find(row=>row.companionId===companionId)?.renderProfile;},'Software engine report did not reach Cockpit');
    const panels=await compositions();
    for(const [index,panel] of panels.entries()){
      if(!existingHashes.has(panel.publicationHash))testHashes.add(panel.publicationHash);
      await sendPublication(panel,(command,payload)=>api.request(command,payload),{saveAsLatest:false});
      let baselines;
      if(index===1){
        await command('touch',{down:true,valid:true,...touchPoint(momentary(panels[0]))});
        await until(async()=>{const current=await api.request('companion_status');return current.host.instances.find(row=>row.instanceId===instanceId).values?.[momentary(panels[0]).controlIds[0]]===true;},'Held input did not reach the old composition');
        baselines=evidence.wireTypes.filter(type=>type==='baseline').length;
      }
      await api.request('companion_assign',{companionId,instanceId,publicationHash:panel.publicationHash});
      if(index===1){
        await until(async()=>last?.publicationHash===panel.publicationHash&&!last.armed&&last.operation?.phase==='awaiting-neutral','Replacement did not remain disarmed while touch was held');
        assert.equal(evidence.wireTypes.filter(type=>type==='baseline').length,baselines,'Held touch must not admit a new neutral baseline');
        await command('touch',{down:false,valid:true,...touchPoint(momentary(panels[0]))});evidence.heldSwitchRequiresRelease=true;
      }
      const ready=await until(async()=>last?.publicationHash===panel.publicationHash&&last.armed&&last.operation?.phase==='ready'?last:false,'Source composition did not render and arm');
      const state=await api.request('companion_status'),instance=state.host.instances.find(row=>row.instanceId===instanceId);
      assert.equal(instance.publicationHash,panel.publicationHash);assert.equal(instance.armed,true);assert.deepEqual(instance.values,Object.fromEntries(panel.document.ui.controls.map(control=>[control.id,fallbackValue(control)])));
      assert.equal(ready.processId,evidence.processId);
      evidence.compositions.push({publicationHash:panel.publicationHash,capabilityFingerprint:panel.capabilityFingerprint,widgets:ready.widgets,frames:ready.frames,operation:ready.operation,neutralValues:instance.values,appearance:ready.appearance});
      const original=momentary(panel),controlId=original.controlIds[0];
      const touch=async down=>{const appearance=await command('touch',{down,valid:true,...touchPoint(original)});assert.equal(appearance.appearance.find(item=>item.widget===original.id).state,down?'pressed':'unknown');await until(async()=>{
        const current=await api.request('companion_status'),instance=current.host.instances.find(row=>row.instanceId===instanceId);return instance.values?.[controlId]===down;
      },'Actual software touch input did not arrive in order');};
      await touch(true);await touch(false);
      if(index===0){
        if(process.env.OSG_WIDGETS_SOFTWARE_BUNDLE){
          const document=structuredClone(panel.document),selected=document.presentation.widgets.find(widget=>widget.id===original.id);
          const other=document.presentation.widgets.find(widget=>widget.kind==='component'&&widget.id!==selected.id&&widget.controlIds.length);
          assert.ok(other);selected.bounds=structuredClone(other.bounds);
          document.ui=normalizeToPortableUi(document);
          const overlapping=await publishDocument(document,panel.assets);
          if(!existingHashes.has(overlapping.publicationHash))testHashes.add(overlapping.publicationHash);
          await sendPublication(overlapping,(command,payload)=>api.request(command,payload),{saveAsLatest:false});
          await assert.rejects(()=>api.request('companion_assign',{companionId,instanceId,publicationHash:overlapping.publicationHash}),/hitArea.overlap/);
          const retained=(await api.request('companion_status')).host.instances.find(row=>row.instanceId===instanceId);
          assert.equal(retained.publicationHash,panel.publicationHash);assert.equal(retained.armed,true);
          evidence.overlappingTouchAssignmentPreservesRunningPanel=true;
        }
        const document=structuredClone(panel.document);document.presentation.widgets[0].config.properties.label='😀';
        const incompatible=await publishDocument(document,panel.assets);
        if(!existingHashes.has(incompatible.publicationHash))testHashes.add(incompatible.publicationHash);
        await sendPublication(incompatible,(command,payload)=>api.request(command,payload),{saveAsLatest:false});
        await assert.rejects(()=>api.request('companion_assign',{companionId,instanceId,publicationHash:incompatible.publicationHash}),/U\+1F600/);
        const current=(await api.request('companion_status')).host.instances.find(row=>row.instanceId===instanceId);
        assert.equal(current.publicationHash,panel.publicationHash);assert.equal(current.armed,true);evidence.incompatibleAssignmentPreservesRunningPanel=true;
      }
      if(index===1){
        const values=(await api.request('companion_status')).host.instances.find(row=>row.instanceId===instanceId).values;
        const independent=panel.document.presentation.widgets.find(widget=>widget.kind==='component'&&widget.id!==original.id&&widget.controlIds.some(id=>panel.document.ui.controls.find(control=>control.id===id)?.kind==='button'));
        assert.ok(independent);assert.equal(values[independent.controlIds[0]],false,'Another reusable instance remains independent');
      }
      const path=process.env.OSG_WIDGETS_ENGINE_EVIDENCE_DIR;
      if(path){await command('snapshot',{path:path+`/composition-${index}.ppm`});}
    }
    assert.ok(evidence.wireTypes.filter(type=>type==='open').length>=2);assert.ok(evidence.wireTypes.filter(type=>type==='baseline').length>=2);assert.ok(evidence.wireTypes.filter(type=>type==='input').length>=4);
    const downloaded=evidence.wireTypes.filter(type=>type==='image-chunk').length;assert.ok(downloaded>0,'The actual host prepares and streams portable image pixels');
    await command('reconnect');
    const reloaded=await until(async()=>last?.publicationHash===panels[1].publicationHash&&last.armed&&last.operation?.phase==='ready'?last:false,'Cached composition did not reconnect');
    assert.equal(reloaded.operation.cacheMisses,0);assert.ok(reloaded.operation.cacheHits>0);
    assert.equal(evidence.wireTypes.filter(type=>type==='image-chunk').length,downloaded,'Unchanged prepared pixels reuse the bounded software cache');
    evidence.preparedImageCache={downloadRequests:downloaded,reconnect:reloaded.operation};
    // This binary is installed once; no recompilation or publication-specific C
    // generation occurs between the two assignments.
    if(binary)assert.equal(createHash('sha256').update(await readFile(binary)).digest('hex'),evidence.binarySha256);
    await writeFile(join(directory,'evidence.json'),JSON.stringify(evidence,null,2));
    console.log(JSON.stringify({directory,compositions:evidence.compositions.map(item=>({publicationHash:item.publicationHash,widgets:item.widgets,frames:item.frames})),wireCounts:evidence.wireTypes.reduce((counts,type)=>(counts[type]=(counts[type]??0)+1,counts),{})}));
    stopping=true;clearInterval(ticker);await tail;await command('quit');const exit=await ended;assert.equal(exit.code,0,stderr);await Promise.all(events);
  }catch(error){
    evidence.failure=String(error);await writeFile(join(directory,'evidence.json'),JSON.stringify(evidence,null,2));throw error;
  }finally{
    stopping=true;clearInterval(ticker);
    if(child&&child.exitCode===null){child.stdin.end();await pause(50);if(child.exitCode===null)child.kill();}
    await client?.close().catch(()=>{});
    await api.request('companion_revoke',{companionId}).catch(()=>{});
    if(testHashes.size)await api.request('companion_prune',{panelIds:[],hashes:[...testHashes]}).catch(()=>{});
    if(createdListener)await api.request('companion_disable').catch(()=>{});
    if(engineError||stderr)await writeFile(join(directory,'engine.txt'),String(engineError??'')+'\n'+stderr);
    api.close();
  }
});
