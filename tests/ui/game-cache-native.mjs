// Opt-in actual native library/artwork regression; never launches a game.
import test from 'node:test';
import assert from 'node:assert/strict';
import {mkdir,writeFile} from 'node:fs/promises';
import {resolve} from 'node:path';
import {nativePage} from '../../scripts/reactive/cdp.mjs';

test('native committed artwork stays stable through passive reads, replacement and removal',{
  skip:process.env.OSG_GAME_CACHE_NATIVE!=='1',timeout:120000,
},async()=>{
  const directory=resolve(`.research/reactive/game-cache-native/${Date.now()}`);
  const installation=resolve(directory,'Owned cache fixture');
  await mkdir(installation,{recursive:true});
  const executable=resolve(installation,'cache-fixture.exe');
  await writeFile(executable,'Harmless metadata fixture; never executed.');
  const page=await nativePage();let gameId;
  const request=async value=>{
    const result=await page.evaluate(`window.__TAURI_INTERNALS__.invoke('gateway_request',{request:${JSON.stringify({...value,clientId:'game-cache-native-acceptance'})}})`);
    assert.equal(result.ok,true,`${value.command}: ${result.error}`);return result.state;
  };
  const read=async()=>(await request({command:'game_get_state'})).games;
  const find=state=>state.catalogue.find(game=>game.id===gameId);
  try{
    // The same connection must recover from protocol/evaluation errors; no
    // application action or permission change is involved in these probes.
    const events=[];const stopEvents=page.listen(event=>events.push(event.method));
    await page.call('Runtime.enable');
    await assert.rejects(page.call('OpenSimGear.invalidProbe'),/-32601/);
    await assert.rejects(page.evaluate('Promise.reject(new Error("Owned harness probe"))'),/Owned harness probe/);
    assert.equal(await page.evaluate('2+2'),4);stopEvents();
    assert.ok(events.includes('Runtime.executionContextCreated'));
    assert.equal((await request({command:'profile_status'})).profiles.profileSession.owner,null);
    const saved=(await request({command:'game_add',name:`Cache fixture ${Date.now()}`,
      installPath:installation,executable,imagePath:resolve('tests/fixtures/artwork/landscape.jpg')})).games;
    gameId=saved.addedGameId;assert.ok(gameId);
    const baseline=find(saved);assert.ok(baseline);
    assert.match(baseline.background.image,/^data:image\/jpeg;base64,/);
    assert.equal(baseline.background.positionX,50);assert.equal(baseline.background.positionY,50);
    for(let count=0;count<12;count++)assert.deepEqual(find(await read()).background,baseline.background);
    const changed=(await request({command:'game_background_save',gameId,positionX:25,positionY:75})).games;
    const replacement=find(changed).background;
    assert.equal(replacement.image,baseline.background.image);
    assert.equal(replacement.positionX,25);assert.equal(replacement.positionY,75);
    assert.deepEqual(find(await read()).background,replacement);
    await request({command:'game_background_save',gameId,path:null});
    assert.equal(find(await read()).background??null,null);
    await request({command:'game_remove',gameId});
    assert.equal(find(await read()),undefined);gameId=undefined;
    assert.equal((await request({command:'profile_status'})).profiles.profileSession.owner,null);
    await writeFile(resolve(directory,'evidence.json'),JSON.stringify({at:new Date().toISOString(),
      actualNativeCockpit:true,gameLaunches:false,noExternalOutputs:true,
      baseline,changedBackground:replacement,stableReadCount:12,resetAndRemoval:true,
      profileRemainsStopped:true,steamSchedulingQualifiedByLocalUnitTests:true,
      physicalLatencyQualified:false,fullGamesReadModelQualified:false},null,2)+'\n');
  }finally{
    try{if(gameId)await request({command:'game_remove',gameId});}finally{page.close();}
  }
});
