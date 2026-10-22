import assert from 'node:assert/strict';
import test from 'node:test';
import {loadModules} from './test-module-loader.mjs';
const {guardUiSettings}=(await loadModules(['./ui-settings-guard'])).get('./ui-settings-guard');
const plain=value=>JSON.parse(JSON.stringify(value));
const hash=value=>`sha256:${String(value).padStart(64,'0')}`;
const owner={session:'11111111-1111-4111-8111-111111111111',generation:1,profileId:'22222222-2222-4222-8222-222222222222',profileRevision:1,gameId:null};
function native(){
  const state={revision:1,owner:null,enabled:false,mode:'manual',preference:null},calls=[];
  const snapshot=request=>({scope:request.scope,gameId:request.gameId??null,revision:hash(state.revision),settings:{},profileSession:{owner:state.owner}});
  const send=async request=>{
    calls.push(plain(request));
    if(request.command==='profile_local_settings_get')return{localSettings:snapshot(request)};
    if(request.command==='profile_local_settings_save'){
      if(request.expectedSettingsRevision!==hash(state.revision))throw Error('Local settings revision conflict.');
      if(JSON.stringify(request.expectedActiveOwner)!==JSON.stringify(state.owner))throw Error('Active owner conflict.');
      if(request.scope==='desktop')state.enabled=request.settings.enabled;
      if(request.scope==='selection-mode')state.mode=request.settings.mode;
      if(request.scope==='game-preference')state.preference=request.settings.remove?null:request.settings.profileId;
      state.revision++;return{localSettings:snapshot(request)};
    }
    if(request.command==='profile_desktop_status')return{desktop:{enabled:state.enabled}};
    if(request.command==='profile_automatic_status')return{profiles:{automatic:{mode:state.mode}}};
    if(request.command==='profile_game_candidates')return{profiles:{preferredProfileId:state.preference}};
    if(request.command==='profile_stop')return{profiles:{stopped:true}};
    throw Error(`Unsafe legacy write reached native: ${request.command}`);
  };
  return{state,calls,send};
}
test('a displayed desktop snapshot cannot overwrite a later MCP edit; conflict requires a refresh',async()=>{
  const h=native(),send=guardUiSettings(h.send);
  assert.equal((await send({command:'profile_desktop_status'})).desktop.enabled,false);
  h.state.enabled=true;h.state.revision++; // An independent MCP commit.
  await assert.rejects(send({command:'profile_desktop_configure',enabled:false,scope:{kind:'application'}}),/revision conflict/);
  assert.equal(h.state.enabled,true);
  await assert.rejects(send({command:'profile_desktop_configure',enabled:false,scope:{kind:'application'}}),/Refresh/);
  await send({command:'profile_desktop_status'});
  assert.equal((await send({command:'profile_desktop_configure',enabled:false,scope:{kind:'application'}})).desktop.enabled,false);
  assert.equal(h.calls.filter(call=>call.command==='profile_local_settings_save').length,2);
});
test('two UI views share native compare-and-save and carry the exact observed owner',async()=>{
  const h=native();h.state.owner=owner;const first=guardUiSettings(h.send),second=guardUiSettings(h.send);
  await first({command:'profile_automatic_status'});await second({command:'profile_automatic_status'});
  const outcomes=await Promise.allSettled([first({command:'profile_selection_mode',mode:'automatic'}),second({command:'profile_selection_mode',mode:'manual'})]);
  assert.equal(outcomes.filter(result=>result.status==='fulfilled').length,1);assert.equal(h.state.mode,'automatic');assert.equal(h.state.revision,2);
  assert.deepEqual(h.calls.filter(call=>call.command==='profile_local_settings_save')[0].expectedActiveOwner,owner);
});
test('owner changes reject UI writes even with the same settings revision',async()=>{
  const h=native(),send=guardUiSettings(h.send);await send({command:'profile_automatic_status'});h.state.owner=owner;
  await assert.rejects(send({command:'profile_selection_mode',mode:'automatic'}),/owner conflict/);
  assert.equal(h.state.mode,'manual');assert.equal(h.state.revision,1);
});
test('rules and remembered choices use typed guarded edits and independent per-game snapshots',async()=>{
  const h=native(),send=guardUiSettings(h.send);
  await send({command:'profile_automatic_status'});
  await send({command:'profile_game_rule_save',gameId:'dcs',profileId:owner.profileId,priority:3,enabled:true});
  await send({command:'profile_game_rule_remove',gameId:'dcs'});
  await send({command:'profile_game_candidates',gameId:'dcs'});
  await send({command:'profile_game_preference_save',gameId:'dcs',profileId:owner.profileId});
  await send({command:'profile_game_preference_remove',gameId:'dcs'});
  const writes=h.calls.filter(call=>call.command==='profile_local_settings_save');
  assert.deepEqual(writes.map(call=>[call.scope,call.settings]),[
    ['game-rule',{profileId:owner.profileId,priority:3,enabled:true}],['game-rule',{remove:true}],
    ['game-preference',{profileId:owner.profileId}],['game-preference',{remove:true}]
  ]);
  await assert.rejects(send({command:'profile_game_preference_remove',gameId:'x-plane-11'}),/Refresh/);
});
test('a late superseded status read cannot replace a newer displayed snapshot',async()=>{
  const h=native();let release,delay=false;
  const send=guardUiSettings(async request=>{
    const value=await h.send(request);
    if(delay&&request.command==='profile_desktop_status'){delay=false;await new Promise(resolve=>release=resolve);}
    return value;
  });
  await send({command:'profile_desktop_status'});delay=true;
  const older=send({command:'profile_desktop_status'});while(!release)await new Promise(resolve=>setImmediate(resolve));
  h.state.enabled=true;h.state.revision++;
  await send({command:'profile_desktop_status'});release();await older;
  await send({command:'profile_desktop_configure',enabled:false,scope:{kind:'application'}});
  assert.equal(h.state.enabled,false);
});
test('malformed snapshots fail before display/write and unrelated Stop remains direct',async()=>{
  for(const mutate of [value=>delete value.profileSession.owner,value=>value.revision='bad',value=>value.scope='desktop',value=>value.gameId='dcs',value=>value.profileSession.owner={},value=>value.settings=null]){
    const h=native();const send=guardUiSettings(async request=>{const result=await h.send(request);if(result.localSettings)mutate(result.localSettings);return result;});
    await assert.rejects(send({command:'profile_automatic_status'}),/Refresh/);
    assert.equal(h.calls.length,1);
  }
  const h=native(),send=guardUiSettings(h.send);assert.equal((await send({command:'profile_stop'})).profiles.stopped,true);
  await assert.rejects(send({command:'profile_selection_mode',mode:'automatic'}),/Refresh/);assert.equal(h.calls.length,1);
});
