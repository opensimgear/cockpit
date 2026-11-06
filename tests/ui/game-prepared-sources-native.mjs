// Actual native source notifications; mutates only a uniquely owned fixture asset.
import test from 'node:test';
import assert from 'node:assert/strict';
import {mkdir,readFile,writeFile,unlink} from 'node:fs/promises';
import {createHash,randomBytes} from 'node:crypto';
import {deflateSync} from 'node:zlib';
import {dirname,resolve,sep} from 'node:path';
import {browserPage,nativeMainTarget,nativePage,until} from '../../scripts/reactive/cdp.mjs';

const browserUrl=process.env.OSG_GAME_PREPARED_SOURCES_BROWSER_URL;
const transport=browserUrl?'browser-dev':'native';

function chunk(type,bytes){
  const data=Buffer.concat([Buffer.from(type),bytes]);let crc=0xffffffff;
  for(const byte of data){crc^=byte;for(let bit=0;bit<8;bit++)crc=(crc>>>1)^((crc&1)?0xedb88320:0);}
  const size=Buffer.alloc(4),checksum=Buffer.alloc(4);size.writeUInt32BE(bytes.length);checksum.writeUInt32BE((crc^0xffffffff)>>>0);
  return Buffer.concat([size,data,checksum]);
}
function fixturePng(){
  const header=Buffer.alloc(13);header.writeUInt32BE(16);header.writeUInt32BE(9,4);header[8]=8;header[9]=2;
  const color=randomBytes(3),rows=Buffer.alloc(9*(1+16*3));
  for(let y=0;y<9;y++)for(let x=0;x<16;x++)color.copy(rows,y*49+1+x*3);
  return Buffer.concat([Buffer.from([137,80,78,71,13,10,26,10]),chunk('IHDR',header),chunk('IDAT',deflateSync(rows)),chunk('IEND',Buffer.alloc(0))]);
}

test(transport+' file notifications replace prepared artwork without read-side collection',{
  skip:!browserUrl&&process.env.OSG_GAME_PREPARED_SOURCES_NATIVE!=='1',timeout:90000,
},async()=>{
  assert.equal(process.platform,'win32');
  const directory=resolve('.research/reactive/game-prepared-sources-'+transport,String(Date.now()));
  await mkdir(directory,{recursive:true});
  const executable=resolve(directory,'fixture.exe'),source=resolve(directory,'unique.png');
  await writeFile(executable,'Harmless metadata fixture; never launched.');await writeFile(source,fixturePng());
  const endpoint=process.env.OSG_NATIVE_CDP_URL??'http://127.0.0.1:9222';
  if(!browserUrl)await until(async()=>{try{return nativeMainTarget(await(await fetch(`${endpoint}/json`)).json());}catch{return false;}},Boolean,30000);
  const page=browserUrl?await browserPage(browserUrl):await nativePage(),cleanupErrors=[];let gameId,asset,original,modified=false;
  if(!browserUrl)await until(()=>page.evaluate('!!window.__TAURI_INTERNALS__'),Boolean,15000);
  const request=async value=>{
    const body=JSON.stringify({...value,clientId:'game-prepared-sources-fixture'});
    const reply=await page.evaluate(browserUrl
      ?`fetch('/api/gateway/',{method:'POST',headers:{'Content-Type':'application/json','x-osg-client':'1'},body:${JSON.stringify(body)}}).then(response=>response.json())`
      :`window.__TAURI_INTERNALS__.invoke('gateway_request',{request:${body}})`);
    assert.equal(reply.ok,true,`${value.command}: ${reply.error}`);return reply.state;
  };
  const read=async()=>(await request({command:'game_get_state'})).games;
  const find=state=>state.catalogue.find(game=>game.id===gameId);
  try{
    assert.equal((await request({command:'profile_status'})).profiles.profileSession.owner,null);
    const added=(await request({command:'game_add',name:`Source fixture ${Date.now()}`,installPath:directory,executable,imagePath:source})).games;
    gameId=added.addedGameId;const image=find(added).background.image;
    assert.match(image,/^data:image\/jpeg;base64,/);original=Buffer.from(image.split(',')[1],'base64');
    assert.ok(!added.catalogue.some(game=>game.id!==gameId&&(game.background?.image===image||game.defaultBackground?.image===image)),'Mutate only an unshared owned asset.');
    const hash=createHash('sha256').update(original).digest('hex');
    const root=process.env.OSG_GAME_ARTWORK_ROOT??(process.env.OSG_DCS_BIOS_CONFIG?dirname(process.env.OSG_DCS_BIOS_CONFIG):resolve(process.env.APPDATA,'OpenSimGear','Cockpit'));
    const managed=resolve(root,'game-artwork');asset=resolve(managed,`${hash}.jpg`);
    assert.ok(asset.startsWith(managed+sep));assert.deepEqual(await readFile(asset),original);
    for(let count=0;count<8;count++)assert.equal(find(await read()).background.image,image);
    await page.evaluate(`(async()=>{
      const {observeGameDetails,observeGamesState}=await import('/src/lib/game-read-models/shared.ts');
      const gameId=${JSON.stringify(gameId)};
      const probe=window.__preparedArtworkProbe={images:[],summary:undefined};
      probe.open=()=>observeGameDetails(gameId,state=>{
        if(state.status==='ready')probe.images.push(state.value.value.background?.image??null);
      });
      probe.stop=probe.open();
      probe.stopSummary=observeGamesState(state=>{
        if(state.status==='ready'){
          const game=state.value.catalogue.find(game=>game.id===gameId);
          if(game)probe.summary=game.background?.image??null;
        }
      });
    })()`);
    const observedImages=()=>page.evaluate('window.__preparedArtworkProbe.images');
    const reference=(await until(observedImages,images=>Boolean(images.at(-1)))).at(-1);
    assert.match(reference,/^game-asset:[a-f0-9]{64}:\d+$/);
    // DevTools reads the subscriber's memory. No fixture gateway read can
    // trigger the invalidation or supply the new artwork between mutations.
    modified=true;await writeFile(asset,Buffer.from('Owned invalidation fixture'));
    await until(observedImages,images=>images.at(-1)===null,15000);
    await unlink(asset);await writeFile(asset,original);
    await until(observedImages,images=>images.at(-1)===reference,15000);
    await unlink(asset);
    await until(observedImages,images=>images.at(-1)===null,15000);
    await writeFile(asset,original);modified=false;
    await until(observedImages,images=>images.at(-1)===reference,15000);
    const history=await observedImages();
    const transitions=history.slice(history.indexOf(reference)).filter((value,index,images)=>index===0||value!==images[index-1]);
    assert.deepEqual(transitions,[reference,null,reference,null,reference]);
    await until(()=>page.evaluate('window.__preparedArtworkProbe.summary'),value=>value===reference);
    const closed=await page.evaluate(`(() => {
      const probe=window.__preparedArtworkProbe;
      probe.stop();
      return probe.images;
    })()`);
    modified=true;await unlink(asset);
    await until(()=>page.evaluate('window.__preparedArtworkProbe.summary === null'));
    assert.deepEqual(await observedImages(),closed,'A closed detail observer must receive no file updates');
    await page.evaluate('window.__preparedArtworkProbe.stop=window.__preparedArtworkProbe.open()');
    const reopened=await until(observedImages,images=>images.length>closed.length);
    assert.equal(reopened[closed.length],null,'Reopen must start with the committed artwork withdrawal');
    await writeFile(asset,original);modified=false;
    await until(observedImages,images=>images.at(-1)===reference);
    assert.equal((await request({command:'profile_status'})).profiles.profileSession.owner,null);
    await writeFile(resolve(directory,'evidence.json'),JSON.stringify({at:new Date().toISOString(),transport,actualNativeGateway:true,
      noGameLaunch:true,noPhysicalOutputs:true,asset,hash,stableReadCount:8,
      nativeTamperWithdrawal:true,deleteAndRestore:true,transitions,reopenedAfterBackgroundWithdrawal:true,fixtureReadsDuringMutations:0,
      profileRemainsStopped:true,physicalLatencyQualified:false},null,2)+'\n');
  }finally{
    try{
      try{await page.evaluate('window.__preparedArtworkProbe?.stop();window.__preparedArtworkProbe?.stopSummary();delete window.__preparedArtworkProbe;');}
      catch(error){cleanupErrors.push(String(error));}
      if(modified&&asset&&original)try{await writeFile(asset,original);}catch(error){cleanupErrors.push(String(error));}
      if(gameId)try{await request({command:'game_remove',gameId});}catch(error){cleanupErrors.push(String(error));}
      await writeFile(resolve(directory,'cleanup.json'),JSON.stringify({cleanupErrors},null,2)+'\n');
    }finally{await page.close();}
    assert.deepEqual(cleanupErrors,[]);
  }
});
