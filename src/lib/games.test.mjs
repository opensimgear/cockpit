import test from 'node:test';
import assert from 'node:assert/strict';
import {readFile} from 'node:fs/promises';
import {loadModules} from './test-module-loader.mjs';
const modules=await loadModules(['./games']);
const games=modules.get('./games');
const {supportedGames,gameExecutableName,visibleGames,gameIntegrationInstalled,gameIntegrationLabel,gameIntegrationLiveLabel,gameIntegrationNativeVerified,gameIntegrationReady,gameIntegrationVerificationLabel,gameIntegrationSetupRequest,gameIntegrationTargetError,gameIntegrationUserFolder,gameIntegrationFolderError,gameIntegrationSetupBlockedReason,gameCleanupLabel}=games;
const plain=value=>JSON.parse(JSON.stringify(value));
const dcs=()=>({...supportedGames[0],installed:true,integrationSetup:{state:'setup-required',message:'Choose current game-owned settings.'}});

test('metadata library lists actual supported games and never constructs a profile or activation request',async()=>{
 assert.deepEqual(plain(supportedGames.map(game=>game.id)),['dcs','assetto-corsa-rally','x-plane-11','x-plane-12']);
 const catalogue=plain(supportedGames);catalogue[0].installed=true;assert.equal(visibleGames(catalogue,false).length,1);assert.equal(visibleGames(catalogue,true).length,4);assert.equal(supportedGames[0].installed,false);
 for(const game of supportedGames)assert.equal(gameExecutableName(game.id),game.id.startsWith('x-plane-')?'X-Plane.exe':game.id==='dcs'?'DCS.exe':'acr.exe');
 for(const name of ['newGameProfile','cloneGameProfile','captureGameCompanions','gameProfileError','defaultGameProfile'])assert.equal(games[name],undefined);
 const source=await readFile(new URL('./games.ts',import.meta.url),'utf8');for(const retired of ['game_profile_save','game_activate','game_launch','game_deactivate','CapturedMapping','IntegrationConfig','GameSession'])assert.ok(!source.includes(retired),retired);
});

test('provider setup, native verification and telemetry freshness remain separate facts',()=>{
 const game=dcs();delete game.integrationSetup;assert.equal(gameIntegrationLabel(game),'Integration not checked');assert.equal(gameIntegrationReady(game),false);
 for(const state of ['not-installed','setup-required','setting-up','ready','needs-attention','unsupported-platform'])assert.ok(gameIntegrationLabel({...game,integrationSetup:{state}}).length>5);
 game.integrationSetup={state:'ready',live:'waiting'};assert.equal(gameIntegrationReady(game),true);assert.equal(gameIntegrationLabel(game),'Integration ready');assert.equal(gameIntegrationNativeVerified(game),false);
 for(const [live,label]of [['waiting',''],['receiving','Receiving game data'],['stale','Game data is stale']]){game.integrationSetup.live=live;assert.equal(gameIntegrationLiveLabel(game),label);assert.equal(gameIntegrationReady(game),true);}
 game.installed=false;assert.equal(gameIntegrationLabel(game),'Game not installed');assert.equal(gameIntegrationReady(game),false);assert.equal(gameIntegrationLiveLabel({}),'');
 for(const id of ['x-plane-11','x-plane-12'])assert.equal(gameIntegrationReady({...supportedGames.find(game=>game.id===id),installed:true,integrationSetup:{state:'ready',verified:false}}),true);
 assert.equal(gameIntegrationInstalled({...dcs(),integrationSetup:{state:'ready',installed:false}}),false);
 assert.equal(gameIntegrationInstalled({...dcs(),integrationSetup:{state:'needs-attention',installed:true}}),true);
 assert.equal(gameIntegrationReady({...dcs(),integrationSetup:{state:'needs-attention',installed:true}}),false);
});

test('ACR verification requires its actual native provider and survives a closed game without inventing fresh data',()=>{
 const game={...supportedGames[1],installed:true,integrationSetup:{state:'needs-attention',connection:{source:'shared-memory'},verified:false,live:'waiting'}};
 assert.equal(gameIntegrationReady(game),false);assert.equal(gameIntegrationLabel(game),'Telemetry configured · verification needed');assert.equal(gameIntegrationVerificationLabel(game),'Native telemetry not verified');
 game.integrationSetup.verified=true;game.integrationSetup.state='ready';assert.equal(gameIntegrationReady(game),true);assert.equal(gameIntegrationNativeVerified(game),true);assert.equal(gameIntegrationVerificationLabel(game),'Native telemetry verified');assert.equal(gameIntegrationLiveLabel(game),'');
 game.integrationSetup.live='stale';assert.equal(gameIntegrationReady(game),true);assert.equal(gameIntegrationLiveLabel(game),'Game data is stale');
 for(const live of ['waiting','receiving','stale']){game.integrationSetup.connection.source='synthetic';game.integrationSetup.live=live;assert.equal(gameIntegrationReady(game),false);assert.equal(gameIntegrationNativeVerified(game),false);assert.ok(!gameIntegrationLiveLabel(game).includes('Receiving game data'));}
 delete game.integrationSetup.connection;assert.equal(gameIntegrationNativeVerified(game),false);assert.equal(gameIntegrationLabel(game),'Native telemetry not verified');
});

test('game-owned setup targets use only explicit bounded installation metadata',()=>{
 const first=String.raw`C:\Users\Pilot\Saved Games\DCS`,second=String.raw`D:\Profiles\DCS.openbeta`;
 for(const setup of [undefined,{candidates:[]},{candidates:[first,second]}])assert.equal(gameIntegrationUserFolder({integrationSetup:setup}),'');
 assert.equal(gameIntegrationUserFolder({integrationSetup:{candidates:[first]}}),first);assert.equal(gameIntegrationUserFolder({integrationSetup:{candidates:[first,second],userFolder:second}}),second);
 for(const path of ['',' ','DCS','..\\DCS','C:DCS',String.raw`\Saved Games\DCS`])assert.ok(gameIntegrationFolderError(path));
 for(const path of [first,'D:/Profiles/DCS',String.raw`\\server\Profiles\DCS`,'/home/pilot/Saved Games/DCS',' /home/pilot/DCS '])assert.equal(gameIntegrationFolderError(path),'');
 for(const path of ['C:/Users/pilot/\nDCS','C:/Users/pilot/\u0000DCS'])assert.match(gameIntegrationFolderError(path),/control/);
 for(const game of supportedGames){assert.equal(gameIntegrationTargetError(game,''),game.id==='dcs'?'Choose the DCS Saved Games folder.':'');for(const command of ['game_integration_check','game_integration_setup','game_integration_cancel','game_integration_recover'])assert.deepEqual(plain(gameIntegrationSetupRequest(game,command,'  C:/Pilot/DCS  ')),{command,gameId:game.id,...(game.id==='dcs'?{userFolder:'C:/Pilot/DCS'}:{})});}
 assert.deepEqual(plain(gameIntegrationSetupRequest(dcs(),'game_integration_check','')),{command:'game_integration_check',gameId:'dcs'});
});

test('missing cleanup metadata is unavailable rather than a manufactured idle session; native profile ownership blocks setup separately',()=>{
 const game=dcs();assert.match(gameIntegrationSetupBlockedReason(game,undefined,''),/cleanup metadata is unavailable/);
 for(const phase of ['idle','failed'])assert.equal(gameIntegrationSetupBlockedReason(game,{phase},''),'');
 for(const phase of ['restoring','recovery-needed'])assert.match(gameIntegrationSetupBlockedReason(game,{phase},''),/previous owned setup/);
 assert.equal(gameIntegrationSetupBlockedReason(game,{phase:'idle'},'Exact native profile recovery is pending.'),'Exact native profile recovery is pending.');
 assert.match(gameIntegrationSetupBlockedReason({...game,installed:false},undefined,''),/Install/);
 assert.equal(gameIntegrationSetupBlockedReason({...game,integrationSetup:{state:'unsupported-platform',message:'Native setup requires Windows.'}},{phase:'idle'},''),'Native setup requires Windows.');
 assert.match(gameIntegrationSetupBlockedReason({...game,integrationSetup:{state:'setting-up'}},{phase:'idle'},''),/in progress/);
 assert.match(gameIntegrationSetupBlockedReason({...game,integrationSetup:{recoveryNeeded:true}},{phase:'idle'},''),/Recover/);
 assert.equal(gameIntegrationSetupBlockedReason({...game,integrationSetup:{repairable:false,message:'Unrecognized DCS module.'}},{phase:'idle'},''),'Unrecognized DCS module.');
});

test('cleanup labels cannot represent running profiles, process launches or applying a graph',()=>{
 for(const [phase,label]of [['idle','Nothing to restore'],['restoring','Restoring previous owned setup'],['recovery-needed','Recovery needed'],['failed','Cleanup failed']])assert.equal(gameCleanupLabel({phase}),label);
 assert.equal(gameCleanupLabel({phase:'running'}),undefined);assert.equal(gameCleanupLabel({phase:'launching'}),undefined);
});
