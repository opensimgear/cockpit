import {discoveredGames} from './game-test-fixtures.mjs';
import assert from 'node:assert/strict';
import test from 'node:test';
import {loadModules} from './test-module-loader.mjs';

const modules=await loadModules(['./games','./game-provider-settings','./profile-runtime','./profile-automatic','./game-profile-context','./profile-inspection','./profile-canvas']);
modules.get('./games').updateGameCatalogue(discoveredGames);
const games=modules.get('./games'),settings=modules.get('./game-provider-settings');
const added=['assetto-corsa','assetto-corsa-competizione','assetto-corsa-evo'];
const profileId='11111111-1111-4111-8111-111111111111',session='22222222-2222-4222-8222-222222222222',hash=`sha256:${'1'.repeat(64)}`;

test('each Assetto title uses its own verified native shared-memory setup and bounded timing settings',()=>{
  for(const id of added){
    const game={...discoveredGames.find(game=>game.id===id),installed:true,integrationSetup:{state:'ready',verified:false,message:'Verification needed',providerSetupRevision:hash,connection:{source:'shared-memory',freshnessMs:1500}}};
    assert.equal(game.provider,id);
    assert.equal(games.gameIntegrationReady(game),false);
    assert.equal(games.gameIntegrationLabel(game),'Telemetry configured · verification needed');
    const draft=settings.providerSettingsDraft(game),save=settings.prepareProviderSettingsSave(draft);
    assert.equal(save.request.gameId,id);
    assert.deepEqual(Object.keys(save.request.connection).sort(),['freshnessMs','source']);
    draft.connection.source='synthetic';assert.match(settings.providerSettingsError(draft),/shared-memory/);
    draft.connection.source='shared-memory';draft.connection.processId=42;assert.match(settings.providerSettingsError(draft),/shared-memory/);
    game.integrationSetup.verified=true;assert.equal(games.gameIntegrationReady(game),true);
    game.integrationSetup.live='stale';assert.equal(games.gameIntegrationLiveLabel(game),'Game data is stale');
    game.integrationSetup.connection.source='synthetic';assert.equal(games.gameIntegrationNativeVerified(game),false);
  }
});

test('new game identities survive runtime ownership, inspection, canvas, profile choice and automatic rules',()=>{
  for(const gameId of added){
    const owner={session,generation:1,profileId,profileRevision:1,gameId};
    const status={state:'running',owner,appliedSnapshot:{profileId,revision:1,semanticHash:hash},degradation:[],fault:null};
    assert.equal(modules.get('./profile-runtime').nativeProfileStatus(status).owner.gameId,gameId);
    assert.equal(modules.get('./profile-inspection').captureProfileOwner(owner).gameId,gameId);
    assert.equal(modules.get('./profile-canvas').canvasGameId({observation:{attributes:{gameId}}}),gameId);
    const summary={id:profileId,name:'Racing rig',revision:1,isDefault:false,semanticHash:hash,gameIds:[gameId]};
    assert.equal(modules.get('./game-profile-context').captureGameProfileCandidates({gameId,profiles:[summary],preferredProfileId:profileId},gameId).gameId,gameId);
    const automatic={schemaVersion:1,mode:'automatic',manualProfileId:profileId,rules:[{identity:{gameId,executable:`C:\\Games\\${gameId}\\game.exe`},profileId,priority:1,enabled:true}],status:'waiting',message:'Waiting for game.',conflicts:[],lastAttempt:null,target:null,readOnly:false};
    assert.equal(modules.get('./profile-automatic').captureAutomaticStatus(automatic).rules[0].identity.gameId,gameId);
  }
});

test('automatic status accepts one rule for every registered game and rejects overflow',()=>{
  const ids=discoveredGames.map(game=>game.id);
  const rules=ids.map(gameId=>({identity:{gameId,executable:`C:\\Games\\${gameId}\\game.exe`},profileId,priority:1,enabled:true}));
  const status={schemaVersion:1,mode:'automatic',manualProfileId:profileId,rules,status:'conflict',message:'Choose one running game.',conflicts:ids,lastAttempt:null,target:null,readOnly:false};
  const capture=modules.get('./profile-automatic').captureAutomaticStatus;
  assert.equal(capture(status).rules.length,7);
  assert.equal(capture(status).conflicts.length,7);
  assert.throws(()=>capture({...status,rules:[...rules,rules[0]]}),/malformed/);
});
