import test from 'node:test';
import assert from 'node:assert/strict';
import {readFile} from 'node:fs/promises';
import {loadModules} from './test-module-loader.mjs';
const modules=await loadModules(['./games']);
const games=modules.get('./games');
const {supportedGames,gameExecutableName,visibleGames,gameIntegrationInstalled,gameIntegrationLabel,gameIntegrationLiveLabel,gameIntegrationNativeVerified,gameIntegrationReady,gameIntegrationVerificationLabel,gameIntegrationSetupRequest,gameIntegrationTargetError,gameIntegrationUserFolder,gameIntegrationFolderError,gameIntegrationSetupBlockedReason}=games;
const plain=value=>JSON.parse(JSON.stringify(value));
const dcs=()=>({...supportedGames[0],installed:true,integrationSetup:{state:'setup-required',message:'Choose current game-owned settings.'}});

test('native custom registry keeps selectable membership distinct from persistent IDs and survives missing executables',()=>{
 const id='custom-11111111-1111-4111-8111-111111111111',custom={id,name:'A custom game',provider:null,installed:false,launchSupported:false};
 const seen=[];const stop=games.gameCatalogue.subscribe(value=>seen.push(value.length));
 assert.equal(games.isGameId(id),true);assert.equal(games.isRegisteredGameId(id),false);assert.equal(games.isGameId('custom-invalid'),false);
 games.updateGameCatalogue([...supportedGames,custom]);assert.equal(games.getGameCatalogue().length,8);assert.equal(games.isRegisteredGameId(id),true);
 assert.equal(games.gameName(id),'A custom game');assert.equal(games.gameName('missing'),'missing');assert.equal(games.gameHasIntegration(custom),false);
 assert.equal(gameExecutableName(id),'game executable');assert.equal(visibleGames([custom],false).length,1);assert.equal(gameIntegrationLabel(custom),'Bindings and profile activation');
 assert.deepEqual(plain(games.gameInstallationRequest(custom,'C:\\Games\\Moved.exe')),{command:'game_custom_save',gameId:id,name:'A custom game',executable:'C:\\Games\\Moved.exe'});
 assert.deepEqual(plain(games.gameInstallationRequest(supportedGames[0],null)),{command:'game_installation',gameId:'dcs',executable:null});
 assert.equal(games.customGameError('Game','C:\\Games\\game.exe'),'');assert.equal(games.customGameError('Game',''),'');assert.ok(games.customGameError('',''));assert.ok(games.customGameError('Game','relative.exe'));assert.ok(games.customGameError('Game','C:\\game\n.exe'));
 games.updateGameCatalogue(supportedGames);assert.equal(games.isRegisteredGameId(id),false);assert.equal(games.isGameId(id),true);assert.equal(games.gameName(id),'A custom game');assert.deepEqual(seen,[7,8,7]);stop();
});

test('game paths display Windows drive and UNC installations without changing Unix or device paths',()=>{
 for(const [input,expected] of [
  [undefined,''],[null,''],['',''],
  ['C:/Games/DCS/bin/DCS.exe',String.raw`C:\Games\DCS\bin\DCS.exe`],
  [String.raw`\\?\C:\Games\acs.exe`,String.raw`C:\Games\acs.exe`],
  [String.raw`\\?\UNC\server\Games\acs.exe`,String.raw`\\server\Games\acs.exe`],
  [String.raw`\\?\unc\server\Games\acs.exe`,String.raw`\\server\Games\acs.exe`],
  [String.raw`\\server/Games/acs.exe`,String.raw`\\server\Games\acs.exe`],
  ['/home/pilot/X-Plane/X-Plane','/home/pilot/X-Plane/X-Plane'],
  [String.raw`\\?\Volume{fixture}\game.exe`,String.raw`\\?\Volume{fixture}\game.exe`],
 ])assert.equal(games.gameDisplayPath(input),expected);
});

test('metadata library lists actual supported games and never constructs a profile or activation request',async()=>{
 assert.deepEqual(plain(supportedGames.map(game=>game.id)),['dcs','assetto-corsa-rally','x-plane-11','x-plane-12','assetto-corsa','assetto-corsa-competizione','assetto-corsa-evo']);
 const catalogue=plain(supportedGames);catalogue[0].installed=true;assert.equal(visibleGames(catalogue,false).length,1);assert.equal(visibleGames(catalogue,true).length,7);assert.equal(supportedGames[0].installed,false);
 const executables={'dcs':'DCS.exe','assetto-corsa-rally':'acr.exe','x-plane-11':'X-Plane.exe','x-plane-12':'X-Plane.exe','assetto-corsa':'acs.exe','assetto-corsa-competizione':'AC2-Win64-Shipping.exe','assetto-corsa-evo':'AssettoCorsaEVO.exe'};
 for(const game of supportedGames)assert.equal(gameExecutableName(game.id),executables[game.id]);
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

test('current provider setup and native profile ownership guard integration without legacy cleanup state',()=>{
 const game=dcs();assert.equal(gameIntegrationSetupBlockedReason(game,''),'');
 assert.equal(gameIntegrationSetupBlockedReason(game,'Exact native profile recovery is pending.'),'Exact native profile recovery is pending.');
 assert.match(gameIntegrationSetupBlockedReason({...game,installed:false},''),/Install/);
 assert.equal(gameIntegrationSetupBlockedReason({...game,integrationSetup:{state:'unsupported-platform',message:'Native setup requires Windows.'}},''),'Native setup requires Windows.');
 assert.match(gameIntegrationSetupBlockedReason({...game,integrationSetup:{state:'setting-up'}},''),/in progress/);
 assert.match(gameIntegrationSetupBlockedReason({...game,integrationSetup:{recoveryNeeded:true}},''),/Recover/);
 assert.equal(gameIntegrationSetupBlockedReason({...game,integrationSetup:{repairable:false,message:'Unrecognized DCS module.'}},''),'Unrecognized DCS module.');
});



test('category filtering includes built-in and custom games and distinguishes uncategorized',()=>{
  const id='custom-11111111-1111-4111-8111-111111111111',catalogue=[{id,name:'Custom',categoryId:'racing'},games.supportedGames[0],{...games.supportedGames[1],categoryId:'racing'}];
  assert.deepEqual(plain(games.filterGamesByCategory(catalogue,'')).map(game=>game.id),catalogue.map(game=>game.id));
  assert.deepEqual(plain(games.filterGamesByCategory(catalogue,'racing')).map(game=>game.id),[id,'assetto-corsa-rally']);
  assert.deepEqual(plain(games.filterGamesByCategory(catalogue,'uncategorized')).map(game=>game.id),['dcs']);
  assert.equal(games.filterGamesByCategory(catalogue,'deleted').length,0);
  for(const categories of [{},{a:'Racing',b:'Flight simulation'}])assert.equal(games.categoryError(categories),'');
  for(const categories of [{a:''},{a:'Racing',b:' racing '},{a:'x'.repeat(49)},{a:'Bad\nname'},Object.fromEntries(Array.from({length:65},(_,i)=>[i,`Category ${i}`]))])assert.ok(games.categoryError(categories));
});
