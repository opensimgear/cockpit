import {discoveredGames} from './game-test-fixtures.mjs';
import assert from 'node:assert/strict';import test from 'node:test';import {loadModules} from './test-module-loader.mjs';
const modules=await loadModules(['./games','./game-profile-context']),{captureGameProfileCandidates,GameProfileContextClient}=modules.get('./game-profile-context');
modules.get('./games').updateGameCatalogue(discoveredGames);
const id='11111111-1111-4111-8111-111111111111',other='22222222-2222-4222-8222-222222222222',hash=`sha256:${'1'.repeat(64)}`,plain=value=>JSON.parse(JSON.stringify(value));
const summary=()=>({id,name:'DCS rig',revision:1,isDefault:false,semanticHash:hash,gameIds:['dcs']}),candidates=()=>({gameId:'dcs',profiles:[summary()],preferredProfileId:null});
test('candidate captures accept zero matches and freeze matching summaries without requiring Default',()=>{
 const value=captureGameProfileCandidates(candidates(),'dcs');assert.ok(Object.isFrozen(value));assert.ok(Object.isFrozen(value.profiles));assert.ok(Object.isFrozen(value.profiles[0].gameIds));assert.equal(value.profiles[0].name,'DCS rig');
 assert.equal(captureGameProfileCandidates({...candidates(),profiles:[]},'dcs').profiles.length,0);
 const legacy=candidates();delete legacy.profiles[0].gameIds;assert.equal(captureGameProfileCandidates(legacy,'dcs').profiles[0].id,id);
});
test('candidate parsing rejects wrong game, mismatched context, duplicate identities and stale preferences',()=>{
 for(const mutate of [value=>value.gameId='x-plane-12',value=>value.extra=true,value=>value.profiles[0].gameIds=['x-plane-12'],value=>value.profiles.push(summary()),value=>value.preferredProfileId=other,value=>value.profiles[0].name=' bad ',value=>value.profiles[0].revision=0,value=>value.profiles[0].isDefault=true,value=>value.profiles[0].gameIds=['dcs','dcs']]){const value=candidates();mutate(value);assert.throws(()=>captureGameProfileCandidates(value,'dcs'),/could not be verified/);}
});
test('preference writes validate exact saved acknowledgment and never take executable or process authority',async()=>{
 const calls=[],client=new GameProfileContextClient(async request=>{calls.push(plain(request));return{...candidates(),preferredProfileId:request.command==='profile_game_preference_save'?request.profileId:null};});
 await client.candidates('dcs');await client.savePreference('dcs',id);await client.removePreference('dcs');assert.deepEqual(calls,[{command:'profile_game_candidates',gameId:'dcs'},{command:'profile_game_preference_save',gameId:'dcs',profileId:id},{command:'profile_game_preference_remove',gameId:'dcs'}]);
 await assert.rejects(client.savePreference('dcs','invalid'),/matching saved/);await assert.rejects(client.candidates('invalid/path'),/valid game ID/);assert.equal(calls.length,3);
 const missing=new GameProfileContextClient(async()=>candidates());await assert.rejects(missing.savePreference('dcs',id),/not confirmed/);
 const retained=new GameProfileContextClient(async()=>({...candidates(),preferredProfileId:id}));await assert.rejects(retained.removePreference('dcs'),/not confirmed/);
});
