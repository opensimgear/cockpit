// Opt-in actual Cockpit subscription/page integration; never launches a game.
import test from 'node:test';
import assert from 'node:assert/strict';
import {mkdir,writeFile} from 'node:fs/promises';
import {resolve} from 'node:path';
import {nativePage,until} from '../../scripts/reactive/cdp.mjs';

test('native game summaries, exact details and lazy artwork follow committed changes',{
  skip:process.env.OSG_GAME_READ_MODELS_NATIVE!=='1',timeout:120000,
},async()=>{
  const directory=resolve(`.research/reactive/game-read-models-native/${Date.now()}`);
  await mkdir(directory,{recursive:true});
  const executable=resolve(directory,'owned-fixture.exe');
  await writeFile(executable,'Harmless metadata fixture; never executed.');
  const page=await nativePage();let gameId;
  const request=async value=>{
    const reply=await page.evaluate(`window.__TAURI_INTERNALS__.invoke('gateway_request',{request:${JSON.stringify({...value,clientId:'game-stream-native-acceptance'})}})`);
    assert.equal(reply.ok,true,`${value.command}: ${reply.error}`);return reply.state;
  };
  const read=()=>page.evaluate(`(()=>{const p=window.__gameStreamProbe;return {a:p.a,b:p.b,detail:p.detail,jobs:p.jobs};})()`);
  const row=(state,id)=>state?.value?.catalogue?.find(value=>value.id===id);
  try{
    assert.equal((await request({command:'profile_status'})).profiles.profileSession.owner,null);
    await page.evaluate(`(async()=>{
      const m=await import('/src/lib/game-read-models/shared.ts');
      const p=window.__gameStreamProbe={stops:[]};
      p.stops.push(m.observeGamesState(s=>p.a=structuredClone(s)));
      p.stops.push(m.observeGamesState(s=>p.b=structuredClone(s)));
      p.stops.push(m.observeGameJobs(s=>p.jobs=structuredClone(s)));
    })()`);
    await until(read,s=>s.a?.status==='ready'&&s.b?.status==='ready'&&s.jobs?.status==='ready');
    const saved=(await request({command:'game_add',name:`Stream fixture ${Date.now()}`,
      installPath:directory,executable,imagePath:resolve('tests/fixtures/artwork/landscape.jpg')})).games;
    gameId=saved.addedGameId;assert.ok(gameId);
    const baseline=await until(read,s=>s.a?.status==='ready'&&s.b?.status==='ready'&&row(s.a,gameId)&&row(s.b,gameId));
    assert.deepEqual(row(baseline.a,gameId),row(baseline.b,gameId));
    await page.evaluate(`(async()=>{
      const m=await import('/src/lib/game-read-models/shared.ts'),p=window.__gameStreamProbe;
      p.stops.push(m.observeGameDetails(${JSON.stringify(gameId)},s=>p.detail=structuredClone(s)));
    })()`);
    const exact=await until(read,s=>s.a?.status==='ready'&&s.detail?.status==='ready'
      &&row(s.a,gameId)?.detailsRevision===s.detail.value.scope);
    assert.equal(exact.detail.value.gameId,gameId);
    assert.match(exact.detail.value.value.background.image,/^game-asset:[a-f0-9]{64}:\d+$/);
    const image=await page.evaluate(`(async()=>{
      const {resolveGameImage}=await import('/src/lib/game-read-models/image.ts');
      const data=await resolveGameImage(window.__gameStreamProbe.detail.value.value.background.image);
      return {prefix:data.slice(0,23),length:data.length};
    })()`);
    assert.equal(image.prefix,'data:image/jpeg;base64,');assert.ok(image.length>100);
    await request({command:'game_background_save',gameId,positionX:23,positionY:71});
    const changed=await until(read,s=>s.a?.status==='ready'&&s.b?.status==='ready'&&s.detail?.status==='ready'
      &&row(s.a,gameId)?.detailsRevision===s.detail.value.scope
      &&row(s.b,gameId)?.background?.positionX===23&&s.detail.value.value.background?.positionY===71);
    assert.notEqual(changed.detail.value.scope,exact.detail.value.scope);
    await request({command:'game_remove',gameId});
    const removedId=gameId;gameId=undefined;
    await until(read,s=>s.a?.status==='ready'&&s.b?.status==='ready'&&!row(s.a,removedId)&&!row(s.b,removedId)
      &&s.detail?.status!=='ready');
    assert.equal((await request({command:'profile_status'})).profiles.profileSession.owner,null);
    await writeFile(resolve(directory,'evidence.json'),JSON.stringify({at:new Date().toISOString(),
      actualNativeCockpit:true,twoSummaryConsumers:true,exactDetailScope:true,lazyArtwork:image,
      changedDetailScope:true,removalWithdraws:true,gameLaunches:false,profileRemainsStopped:true,
      physicalLatencyQualified:false},null,2)+'\n');
  }finally{
    try{
      await page.evaluate('window.__gameStreamProbe?.stops.forEach(stop=>stop());delete window.__gameStreamProbe;');
      if(gameId)await request({command:'game_remove',gameId});
    }finally{page.close();}
  }
});
