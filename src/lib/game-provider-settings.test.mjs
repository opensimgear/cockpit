import assert from 'node:assert/strict';
import test from 'node:test';
import { loadModules } from './test-module-loader.mjs';
const modules = await loadModules(['./runtime-config','./panel-input-metadata','./games','./game-provider-settings']);
const { defaultProviderConnection, providerSettingsDraft, providerSettingsError, providerSettingsEqual, prepareProviderSettingsSave, rebaseProviderSettingsDraft, settleProviderSettingsSave, gameContextLabel } = modules.get('./game-provider-settings');
const plain = value => JSON.parse(JSON.stringify(value));
const rev = n => `sha256:${String(n).repeat(64)}`;
const game = (id='dcs', revision=rev(1)) => ({id,name:id,provider:id==='dcs'?'dcs-bios':id.startsWith('x-plane')?'x-plane':'assetto-corsa-rally',installed:false,launchSupported:false,integrationSetup:{state:'setup-required',message:'Setup required',providerSetupRevision:revision,aircraftModule:'A-10C',userFolder:'C:\\Users\\Pilot\\Saved Games\\DCS',connection:{...defaultProviderConnection(id),...(id==='dcs'?{cataloguePath:'C:\\DCS-BIOS\\doc\\json'}:{})}}});
const draft = id => providerSettingsDraft(game(id));

test('closed game settings project exact provider keys without native process or binding authority', () => {
  for (const id of ['dcs','x-plane-11','x-plane-12','assetto-corsa-rally']) {
    const g=game(id);g.integrationSetup.connection.processId=42;g.integrationSetup.connection.processIdentity={pid:42,epoch:999};g.integrationSetup.connection.assignments=[{}];
    const value=providerSettingsDraft(g);assert.equal(providerSettingsError(value),'');
    assert.equal(value.connection.processId,undefined);assert.equal(value.connection.processIdentity,undefined);assert.equal(value.connection.assignments,undefined);
    const request=plain(prepareProviderSettingsSave(value).request);
    assert.equal(request.command,'game_integration_settings_save');assert.equal(request.expectedSetupRevision,rev(1));
    assert.equal(request.gameProfile,undefined);assert.equal(request.profileId,undefined);assert.equal(request.contextSession,undefined);
    assert.equal('userFolder' in request,id==='dcs');assert.equal('aircraftModule' in request,false);
    if(id==='dcs')assert.equal(request.aircraftModule,undefined);
    if(id==='x-plane-12')assert.equal(request.connection.version,12);
    if(id.startsWith('x-plane'))assert.equal(request.connection.source,'rref');
    request.connection.freshnessMs=800;assert.equal(value.connection.freshnessMs,1500);
  }
});
test('missing and malformed setup hashes block save; default folders do not fabricate setup readiness', () => {
  for(const revision of [null,undefined,'1',`sha256:${'A'.repeat(64)}`,'sha256:short']){
    const g=game();g.integrationSetup.providerSetupRevision=revision;const d=providerSettingsDraft(g);assert.equal(d.baseRevision,null);assert.match(providerSettingsError(d),/revision/);assert.throws(()=>prepareProviderSettingsSave(d),/revision/);
  }
  const g={...game(),integrationSetup:undefined};const d=providerSettingsDraft(g);assert.equal(d.aircraftModule,'');assert.equal(d.userFolder,'');assert.equal(d.connection.cataloguePath,'');
});
test('DCS saves require bounded absolute folders and actual network settings without aircraft selection', () => {
  const mutations=[d=>d.connection.cataloguePath='',d=>d.connection.cataloguePath='relative/path',d=>d.userFolder='',d=>d.userFolder='relative',d=>d.userFolder='C:\\bad\u0000',d=>d.userFolder='C:\\'+ 'a'.repeat(4096),d=>d.connection.commandHost='999.0.0.1',d=>d.connection.listenAddress='host',d=>d.connection.multicastInterface='127.0.0',d=>d.connection.multicastGroup='127.0.0.1',d=>d.connection.multicastGroup='240.1.1.1',d=>d.connection.commandPort=0,d=>d.connection.listenPort=65536,d=>d.connection.freshnessMs=99,d=>d.connection.freshnessMs=10001,d=>d.connection.freshnessMs=NaN,d=>d.connection.processId=1];
  for(const mutate of mutations){const d=draft();mutate(d);assert.notEqual(providerSettingsError(d),'');assert.throws(()=>prepareProviderSettingsSave(d));}
  const d=draft();d.aircraftModule='F-86F Sabre';d.connection.multicastGroup='';assert.equal(providerSettingsError(d),'');assert.equal(prepareProviderSettingsSave(d).request.aircraftModule,undefined);
});
test('telemetry-only providers reject synthetic/process settings, wrong versions and impossible freshness', () => {
  for (const [id,mutate] of [['assetto-corsa-rally',d=>d.connection.source='synthetic'],['assetto-corsa-rally',d=>d.connection.processId=42],['x-plane-11',d=>d.connection.version=12],['x-plane-12',d=>d.connection.source='synthetic'],['x-plane-12',d=>d.connection.processIdentity={}],['x-plane-12',d=>d.connection.host='300.0.0.1'],['x-plane-12',d=>d.connection.port=0],['x-plane-12',d=>d.connection.listenPort=-1],['x-plane-12',d=>d.connection.updateHz=21],['x-plane-12',d=>{d.connection.updateHz=1;d.connection.freshnessMs=1500;}]]){const d=draft(id);mutate(d);assert.notEqual(providerSettingsError(d),'');}
});
test('network addresses match native local-process and unicast constraints', () => {
  for(const host of ['10.0.0.1','0.0.0.0','localhost','127.00.0.1','127.0.0.256']){const d=draft('x-plane-12');d.connection.host=host;assert.match(providerSettingsError(d),/loopback/);}
  const x=draft('x-plane-11');x.connection.host='127.5.6.7';assert.equal(providerSettingsError(x),'');
  for(const [key,values] of [['commandHost',['0.0.0.0','224.0.0.1','255.255.255.255','127.00.0.1']],['listenAddress',['239.0.0.1','255.255.255.255']],['multicastInterface',['224.0.0.1','255.255.255.255']]])for(const value of values){const d=draft();d.connection[key]=value;assert.match(providerSettingsError(d),/unicast/);}
  const d=draft();d.connection.listenAddress='0.0.0.0';d.connection.multicastInterface='0.0.0.0';assert.equal(providerSettingsError(d),'');
});
test('save captures independent data and rebases newer edits without reverting to submitted fields', () => {
  const d=draft();d.connection.commandPort=7779;const save=prepareProviderSettingsSave(d);
  d.connection.commandPort=7780;d.aircraftModule='F-16C';
  assert.equal(save.request.connection.commandPort,7779);assert.equal(save.submitted.aircraftModule,'A-10C');
  const returned=game('dcs',rev(2));returned.integrationSetup.connection.commandPort=7779;
  const result=settleProviderSettingsSave(d,save,returned);
  assert.equal(result.connection.commandPort,7780);assert.equal(result.aircraftModule,'F-16C');assert.equal(result.baseRevision,rev(2));assert.equal(providerSettingsEqual(result,providerSettingsDraft(returned)),false);
  assert.equal(prepareProviderSettingsSave(result).request.expectedSetupRevision,rev(2));
});
test('save response updates a clean editor but never changes a switched/rebased editor', () => {
  const d=draft();const save=prepareProviderSettingsSave(d);const returned=game('dcs',rev(2));
  const clean=settleProviderSettingsSave(d,save,returned);assert.equal(clean.baseRevision,rev(2));assert.equal(providerSettingsEqual(clean,providerSettingsDraft(returned)),true);
  const switched=draft('x-plane-12');assert.equal(settleProviderSettingsSave(switched,save,returned),switched);
  assert.equal(settleProviderSettingsSave(d,save,game('x-plane-12')),d);
  const rebased={...d,baseRevision:rev(3)};assert.equal(settleProviderSettingsSave(rebased,save,returned),rebased);
  assert.throws(()=>settleProviderSettingsSave(d,save,game('dcs',null)),/revision/);
});

test('remote settings rebase untouched fields while preserving committed and unfinished edits',()=>{
  const base=draft(),local=plain(base);local.connection.commandPort=7779;local.userFolder='C:\\Saved Games\\DCS';
  const remote=game('dcs',rev(2));remote.integrationSetup.connection.listenPort=5011;remote.integrationSetup.aircraftModule='F-16C';
  const saved=providerSettingsDraft(remote),rebased=rebaseProviderSettingsDraft(local,base,saved);
  assert.equal(rebased.baseRevision,rev(2));assert.equal(rebased.connection.commandPort,7779);assert.equal(rebased.connection.listenPort,5011);assert.equal(rebased.aircraftModule,'F-16C');assert.equal(rebased.userFolder,local.userFolder);
  rebased.connection.commandPort=7780;assert.equal(local.connection.commandPort,7779);assert.equal(saved.connection.commandPort,7778);
  const switched=providerSettingsDraft(game('x-plane-12'));assert.deepEqual(plain(rebaseProviderSettingsDraft(local,base,switched)),plain(switched));
});
test('setup, process proof, profile readiness and freshness remain distinct', () => {
  const g=game();g.installed=true;g.integrationSetup.state='ready';g.integrationSetup.live='receiving';assert.equal(gameContextLabel(g),'Game context not verified');
  for(const [status,label] of [['waiting-profile','Waiting for an active profile'],['waiting-game','Waiting for the matching game'],['setup-required','Provider setup required'],['unsupported-platform','Game context unavailable on this platform'],['error','Game context needs attention'],['ready','Game context not verified']]){g.integrationContext={status,processVerified:false};assert.equal(gameContextLabel(g),label);}
  g.integrationContext={status:'waiting-profile',processVerified:true};assert.equal(gameContextLabel(g),'Waiting for an active profile');
  g.integrationContext.status='ready';assert.equal(gameContextLabel(g),'Matching game context verified');
});
