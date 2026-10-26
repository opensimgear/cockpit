import {discoveredGames} from './game-test-fixtures.mjs';
import assert from 'node:assert/strict';
import test from 'node:test';
import {loadModules} from './test-module-loader.mjs';

const modules=await loadModules(['./games','./game-artwork','./profile-game-context','./profile-editor','./profile-runtime','./profile-automatic','./game-profile-context','./game-settings-navigation','./profile-desktop']);
modules.get('./games').updateGameCatalogue(discoveredGames);
const games=modules.get('./games'),{resolveGameArtwork}=modules.get('./game-artwork');
const {profileGameContext,profileGameName,profileSetGameContext,profileGameContextNeedsSync}=modules.get('./profile-game-context');
const {blankProfile}=modules.get('./profile-editor'),{nativeProfileStatus}=modules.get('./profile-runtime');
const {captureAutomaticStatus}=modules.get('./profile-automatic'),{captureGameProfileCandidates}=modules.get('./game-profile-context');
const {gameSettingsHref,gameSettingsFromHash}=modules.get('./game-settings-navigation');
const id='custom-11111111-1111-4111-8111-111111111111',profileId='22222222-2222-4222-8222-222222222222';
const custom=()=>({id,name:'Custom flight game',provider:null,custom:true,installed:true,executable:'C:\\Games\\Custom\\game.exe',launchSupported:true});
const plain=value=>JSON.parse(JSON.stringify(value));

test('custom identity survives registry loading and removal while selectable membership tracks native registration',()=>{
  const observed=[];const stop=games.gameCatalogue.subscribe(value=>observed.push(value.map(game=>game.id)));
  assert.equal(games.isGameId(id),true);assert.equal(games.isRegisteredGameId(id),false);
  for(const invalid of ['invalid/path','custom-../game','custom-11111111-1111-4111-8111-11111111111Z','CUSTOM-11111111-1111-4111-8111-111111111111','steam:123'])assert.equal(games.isGameId(invalid),false);
  games.updateGameCatalogue([...discoveredGames,custom()]);
  assert.equal(games.isRegisteredGameId(id),true);assert.equal(profileGameName(id),'Custom flight game');
  games.updateGameCatalogue(discoveredGames);
  assert.equal(games.isGameId(id),true);assert.equal(games.isRegisteredGameId(id),false);assert.equal(profileGameName(id),'Custom flight game');
  assert.equal(observed.length,3);stop();games.updateGameCatalogue(discoveredGames);assert.equal(observed.length,3);
});

test('custom context saves eligibility without telemetry resources and preserves bindings across integrated context changes',()=>{
  games.updateGameCatalogue([...discoveredGames,custom()]);
  let draft={document:blankProfile(profileId,'Custom bindings'),assignments:[]};
  draft.document.processingNodes=[{id:'constant',name:'Released',inputs:[],outputs:[{id:'out',name:'Released',direction:'output',contract:{delivery:'state',schema:{type:'boolean'},freshnessMs:1000,fallback:false}}],operation:{kind:'constant',value:false}}];
  draft=profileSetGameContext(draft,'dcs',[]);assert.equal(draft.document.resources.length,1);
  draft=profileSetGameContext(draft,id,[]);
  assert.deepEqual(plain(draft.document.eligibility.gameIds),[id]);assert.equal(draft.document.resources.length,0);assert.equal(draft.document.processingNodes.length,1);assert.equal(draft.assignments.length,0);
  assert.equal(profileGameContextNeedsSync(draft,[]),false);assert.equal(profileGameContext(draft.document),id);
  const endpoint=modules.get('./profile-desktop').desktopPreset('key-hold','keyboard').output;endpoint.scope={kind:'selected-game'};
  draft.document.desktopOutputs=[endpoint];draft.document.bindings.push({id:'scoped-key',from:{nodeId:'constant',portId:'out'},to:{nodeId:'keyboard',portId:'action-1'}});
  const {parseProfileDocument,exportProfileDocument}=modules.get('./profiles'),portable=parseProfileDocument(exportProfileDocument(draft.document));
  assert.deepEqual(plain(portable.eligibility.gameIds),[id]);assert.deepEqual(plain(portable.desktopOutputs[0].scope),{kind:'selected-game'});assert.equal(portable.bindings[0].to.nodeId,'keyboard');
  games.updateGameCatalogue(discoveredGames);
  assert.equal(profileGameContext(draft.document),id,'missing games must not silently become hardware-only profiles');
  assert.throws(()=>profileSetGameContext(draft,id,[]));
  assert.deepEqual(plain(profileSetGameContext(draft,'',[]).document.eligibility.gameIds),[]);
});

test('unavailable custom entries stay visible and artwork resolves custom/default/bundled/fallback in order',()=>{
  const game=custom();game.installed=false;
  assert.equal(games.visibleGames([game],false).length,1);assert.equal(games.gameIntegrationLabel(game),'Bindings and profile activation');assert.equal(games.gameIntegrationReady(game),false);
  assert.equal(resolveGameArtwork(game).wordmark,game.name);assert.equal(resolveGameArtwork(game).image,'');
  game.defaultBackground={image:'data:image/jpeg;base64,default',positionX:20,positionY:80};
  assert.equal(resolveGameArtwork(game).image,game.defaultBackground.image);
  game.background={image:'data:image/jpeg;base64,override',positionX:70,positionY:10};
  assert.equal(resolveGameArtwork(game).image,game.background.image);assert.equal(resolveGameArtwork(game).positionX,70);
  const builtin={...discoveredGames[0],background:game.background};
  assert.equal(resolveGameArtwork(builtin).image,game.background.image);assert.equal(resolveGameArtwork({...builtin,background:null}).image,'/game-art/dcs.jpg');
});

test('runtime owners, automatic selection, matching profiles and navigation retain valid custom IDs without registry authority',()=>{
  games.updateGameCatalogue(discoveredGames);
  const hash=`sha256:${'1'.repeat(64)}`,summary={id:profileId,name:'Custom bindings',revision:1,isDefault:false,semanticHash:hash,gameIds:[id]};
  const owner={session:'33333333-3333-4333-8333-333333333333',generation:1,profileId,profileRevision:1,gameId:id};
  assert.equal(nativeProfileStatus({state:'running',owner,appliedSnapshot:{profileId,revision:1,semanticHash:hash},degradation:[],fault:null}).owner.gameId,id);
  const automatic={schemaVersion:1,mode:'automatic',manualProfileId:profileId,rules:[],status:'waiting',message:'Waiting for game',conflicts:[],lastAttempt:null,target:{profileId,gameId:id,processSession:'native-process-session'},readOnly:false};
  assert.equal(captureAutomaticStatus(automatic).target.gameId,id);
  assert.equal(captureGameProfileCandidates({gameId:id,profiles:[summary],preferredProfileId:profileId},id).profiles[0].gameIds[0],id);
  assert.equal(gameSettingsFromHash(gameSettingsHref(id)),id);
});

test('manual metadata rejects malformed names and relative or control-character executable paths',()=>{
  assert.equal(games.gameMetadataError('My game','C:\\Games\\game.exe'),'');assert.equal(games.gameMetadataError('My game',''),'');
  for(const [name,path]of [['',''],['A'.repeat(81),''],['My\nGame',''],['Game','relative.exe'],['Game','C:\\game\n.exe']])assert.ok(games.gameMetadataError(name,path));
});

test('automatic status accepts native retained-rule capacity after more than 256 registrations have been removed',()=>{
  const status={schemaVersion:1,mode:'manual',manualProfileId:profileId,rules:[],status:'manual',message:'Manual profile selection',conflicts:[],lastAttempt:null,target:null,readOnly:false};
  const rules=Array.from({length:513},(_,index)=>({identity:{gameId:`custom-11111111-1111-4111-8111-${(index+1).toString(16).padStart(12,'0')}`,executable:`C:\\Games\\Game${index}.exe`},profileId,priority:0,enabled:true}));
  for(const count of [257,512])assert.equal(captureAutomaticStatus({...status,rules:rules.slice(0,count)}).rules.length,count);
  assert.throws(()=>captureAutomaticStatus({...status,rules}),/malformed/);
});
