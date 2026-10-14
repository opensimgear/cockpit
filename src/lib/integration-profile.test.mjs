import test from 'node:test';
import assert from 'node:assert/strict';
import { loadModules } from './test-module-loader.mjs';
const modules=await loadModules(['./integration-profile']);
const api=modules.get('./integration-profile');
const plain=value=>JSON.parse(JSON.stringify(value));
const {newIntegrationConfig,integrationError,integrationFormatError,exportIntegrationDocument,importIntegrationDocument,
  integrationFingerprint,integrationEqual,normalizeIntegrationCatalogue,integrationOutputActions,integrationChannels,
  roleTarget,assignIntegrationRole,renameIntegrationRole,integrationPhysicalControls,integrationStartError,nextIntegrationId,revLightOutput,syntheticTelemetryFrame}=api;
const moduleId='00112233445566778899aabbccddeeff',gatewayId='pico:OSG-PICO-0011223344556677';
const boolean={id:'led.set',endpoint:'led',label:'LED',readable:true,semantics:'setter',parameters:[{type:'boolean'}]};
const rgb={...boolean,id:'rgb.set',endpoint:'rgb',parameters:[{type:'rgb'}]};
const display={...boolean,id:'display.set',endpoint:'display',parameters:[{type:'string',maxBytes:16}]};
const mode={...boolean,id:'generic.mode',endpoint:'mode',parameters:[{type:'enum',options:['off','on']}]};
const actions={status:'ready',boot:9,actions:[boolean,rgb,display,mode]};
const events={boot:9,buttonCount:5,contactButtonCount:3,maintainedButtons:[2],encoderButtons:[3,4],axisCount:1};
const target={gatewayId,module:{id:moduleId,online:true,boot:9,enabled:true,published:true},label:'Lights',state:{connected:true,moduleActions:{[moduleId]:actions},moduleEvents:{[moduleId]:events}}};
const catalogue={provider:'assetto-corsa-rally',revision:1,commands:[],channels:[{id:'rpm',label:'RPM',type:'integer',maximum:100000},{id:'rev_ratio',label:'Rev ratio',type:'number',derived:true},{id:'gear',label:'Gear',type:'integer'},{id:'label',label:'Label',type:'string'},{id:'warning',label:'Warning',type:'boolean'}]};
function config(){const c=plain(newIntegrationConfig('assetto-corsa-rally'));c.connection.source='synthetic';c.assignments=[{role:'panel',gatewayId,moduleId}];c.document.outputs=[{id:'rev',role:'panel',endpoint:'rgb',actionId:'rgb.set',channel:'rev_ratio',output:0,format:{kind:'rgb',bands:[{minimum:0.7,color:'00FF00'},{minimum:0.9,color:'FF0000'}]}}];return c;}
function dcs(){const c=config();c.document.provider='dcs-bios';c.document.context={aircraftModule:'F-16C_50'};c.connection=plain(newIntegrationConfig().connection);c.connection.cataloguePath='/installed/doc/json';return c;}
test('portable export roundtrips semantic IDs and omits every local field',()=>{
  const c=config(),source=exportIntegrationDocument(c),d=JSON.parse(source);
  assert.equal(d.provider,'assetto-corsa-rally');assert.equal(d.outputs[0].endpoint,'rgb');
  assert.equal(source.includes(gatewayId),false);assert.equal(source.includes('freshnessMs'),false);
  const imported=importIntegrationDocument(source,c);assert.equal(imported.error,undefined);assert.deepEqual(plain(imported.config.document),c.document);
  assert.deepEqual(plain(imported.config.assignments),[]);assert.equal(imported.config.connection.source,'synthetic');
  assert.equal(c.assignments.length,1);assert.equal(integrationError(imported.config),'');
});
test('malformed, oversized and local-data imports leave current settings untouched',()=>{
  const c=config(),before=JSON.stringify(c);
  for(const source of ['{','null','[]',JSON.stringify({...c.document,schemaVersion:2}),JSON.stringify({...c.document,provider:'unknown'}),JSON.stringify({...c.document,assignments:c.assignments}),' '.repeat(65537)]) assert.ok(importIntegrationDocument(source,c).error);
  assert.equal(JSON.stringify(c),before);
  const imported=importIntegrationDocument(exportIntegrationDocument(dcs()),c).config;assert.equal(imported.connection.cataloguePath,'');assert.equal(imported.document.context.aircraftModule,'F-16C_50');
  assert.throws(()=>exportIntegrationDocument({...c,document:{...c.document,cataloguePath:'private'}}));
});
test('profile checks bound identities, roles, assignment scope and output-only profiles',()=>{
  const c=config();assert.equal(integrationError(c),'');assert.equal(integrationError(newIntegrationConfig()),'Add one to eight roles and up to 16 inputs and 16 outputs.');
  for(const change of [{schemaVersion:2},{id:'bad id'},{name:'\n'},{provider:'other'},{context:null},{roles:[]},{roles:Array(9).fill(c.document.roles[0])},{inputs:[],outputs:[]},{outputs:Array(17).fill(c.document.outputs[0])}]) assert.ok(integrationError({...c,document:{...c.document,...change}}));
  for(const change of [{id:'bad'+'x'.repeat(31)},{role:'missing'},{channel:'bad\nname'},{actionId:''},{endpoint:''},{output:64}]) assert.ok(integrationError({...c,document:{...c.document,outputs:[{...c.document.outputs[0],...change}]}}));
  assert.ok(integrationError({...c,document:{...c.document,roles:[...c.document.roles,c.document.roles[0]]}}));
  assert.ok(integrationError({...c,document:{...c.document,outputs:[...c.document.outputs,{...c.document.outputs[0],id:'another'}]}}));
  for(const a of [{role:'missing',gatewayId,moduleId},{role:'panel',gatewayId:'bad',moduleId},{role:'panel',gatewayId,moduleId:'0'.repeat(32)}]) assert.ok(integrationError({...c,assignments:[a]}));
  assert.ok(integrationError({...c,assignments:[c.assignments[0],c.assignments[0]]}));
  assert.ok(integrationError({...c,connection:{source:'shared-memory',processId:0,freshnessMs:1500}}));
  assert.equal(integrationError({...c,connection:{source:'shared-memory',processId:7,freshnessMs:10000}}),'');
  assert.ok(integrationError({...c,connection:{source:'synthetic',freshnessMs:99}}));
});
test('DCS local connection and typed input bindings retain their bounded semantics',()=>{
  const c=dcs();c.document.inputs=[{id:'input',role:'panel',kind:'button',control:0,command:'MASTER',interface:'set_state'}];assert.equal(integrationError(c),'');
  for(const change of [{aircraftModule:''},{aircraftModule:'bad\n'}]) assert.ok(integrationError({...c,document:{...c.document,context:change}}));
  for(const change of [{cataloguePath:''},{commandHost:'bad host'},{listenAddress:''},{multicastGroup:' '},{commandPort:0},{listenPort:65536}]) assert.ok(integrationError({...c,connection:{...c.connection,...change}}));
  for(const change of [{kind:'hat'},{control:128},{command:'bad command'},{interface:'fixed_step'},{pressedValue:65536},{step:0}]) assert.ok(integrationError({...c,document:{...c.document,inputs:[{...c.document.inputs[0],...change}]}}));
  assert.equal(integrationError({...c,document:{...c.document,inputs:[{...c.document.inputs[0],kind:'encoder',interface:'variable_step',step:65535}]}}),'');
  assert.ok(integrationError({...c,document:{...c.document,inputs:[c.document.inputs[0],{...c.document.inputs[0],id:'other',control:1}]}}));
  assert.ok(integrationError({...config(),document:{...config().document,inputs:c.document.inputs}}));
});
test('different roles cannot alias a physical endpoint and gateways remain distinct',()=>{
  const c=config();c.document.roles.push({id:'other',label:'Other'});c.document.outputs.push({...c.document.outputs[0],id:'other',role:'other'});c.assignments.push({...c.assignments[0],role:'other'});assert.match(integrationError(c),/Different roles/);
  c.assignments[1].gatewayId='stm32:00112233445566778899aabb';assert.equal(integrationError(c),'');
  assert.equal(roleTarget(c,'other',[target]),undefined);assert.equal(roleTarget(c,'panel',[target]).module.id,moduleId);
  const next=assignIntegrationRole(c,'other',target);assert.equal(next.assignments.length,2);assert.equal(next.assignments[1].gatewayId,gatewayId);assert.notEqual(c.assignments[1].gatewayId,gatewayId);
  assert.equal(assignIntegrationRole(next,'other').assignments.length,1);
  const many=config();many.document.roles=[];many.document.outputs=[];many.assignments=[];for(let i=0;i<5;i++){many.document.roles.push({id:`r${i}`,label:'Role'});many.document.outputs.push({...c.document.outputs[0],id:`o${i}`,role:`r${i}`});many.assignments.push({role:`r${i}`,gatewayId:`gateway:${i}`,moduleId});}assert.match(integrationError(many),/four/);
});
test('format validation rejects unsafe values and unsorted RGB rules',()=>{
  for(const f of [null,{kind:'code'},{kind:'fixed',scale:Infinity},{kind:'integer',decimals:7},{kind:'text',prefix:'é'.repeat(9)},{kind:'text',staleText:'\u0080'},{kind:'text',align:'center'},{kind:'text',overflow:'wrap'},{kind:'threshold'},{kind:'threshold',threshold:1,comparison:'eq'},{kind:'rgb',bands:[]},{kind:'rgb',bands:[{minimum:1,color:'FFFFFF'},{minimum:1,color:'000000'}]},{kind:'rgb',bands:[{minimum:0,color:'bad'}]},{kind:'rgb',bands:[{minimum:0,color:'FFFFFF'}],staleColor:'bad'}]) assert.ok(integrationFormatError(f));
  assert.equal(integrationFormatError({kind:'threshold',threshold:0,comparison:'lte'}),'');assert.equal(integrationFormatError({kind:'fixed',scale:-1e6,decimals:6,prefix:'é'.repeat(8)}),'');
});
test('fingerprints distinguish edited local or portable data but ignore object-key order',()=>{
  const c=config();assert.equal(integrationEqual(c,JSON.parse(JSON.stringify(c))),'true'==='true');assert.equal(integrationEqual(c),false);
  assert.equal(integrationFingerprint(c),integrationFingerprint({assignments:c.assignments,connection:c.connection,document:c.document}));
  assert.equal(integrationEqual(c,{...c,connection:{...c.connection,freshnessMs:100}}),false);
  assert.equal(nextIntegrationId(c,'rev'),'rev-1');c.document.outputs.push({...c.document.outputs[0],id:'rev-1'});assert.equal(nextIntegrationId(c,'rev'),'rev-2');
});
test('nested unknown portable fields cannot leak machine data through export',()=>{
  const c=config();
  for(const mutate of [d=>{d.roles[0].gatewayId=gatewayId;},d=>{d.outputs[0].cataloguePath='/private';},d=>{d.outputs[0].format.secret='/private';},d=>{d.outputs[0].format.bands[0].gatewayId=gatewayId;},d=>{d.context.aircraftModule={cataloguePath:'/private'};},d=>{d.outputs[0].format={kind:'boolean',bands:[{minimum:0,color:'FFFFFF',secret:'/private'}]};}]) {const bad=structuredClone(c);mutate(bad.document);assert.ok(integrationError(bad,true));assert.throws(()=>exportIntegrationDocument(bad));assert.ok(importIntegrationDocument(JSON.stringify(bad.document)).error);}
});
test('explicit role rename preserves existing bindings and exact local assignments',()=>{
  const c=dcs();c.document.inputs=[{id:'button',role:'panel',kind:'button',control:0,command:'MASTER',interface:'set_state'}];
  const renamed=renameIntegrationRole(c,'panel','radio');assert.equal(renamed.document.roles[0].id,'radio');assert.equal(renamed.document.inputs[0].role,'radio');assert.equal(renamed.document.outputs[0].role,'radio');assert.equal(renamed.assignments[0].gatewayId,gatewayId);assert.equal(renamed.assignments[0].role,'radio');assert.equal(c.document.roles[0].id,'panel');assert.equal(integrationError(renamed),'');
  assert.throws(()=>renameIntegrationRole(c,'panel','bad id'));c.document.roles.push({id:'other',label:'Other'});assert.throws(()=>renameIntegrationRole(c,'panel','other'));
});
test('catalogue normalization keeps original DCS output ordinals and typed channel choices',()=>{
  const page={controls:[{id:'RADIO',label:'Radio',inputs:[{interface:'set_state',maxValue:3}],outputs:[{index:2,type:'string',maxLength:7},{index:4,type:'integer',maxValue:1}]}]};const n=normalizeIntegrationCatalogue(page);
  assert.deepEqual(plain(n.channels).map(c=>c.output),[2,4]);assert.equal(n.commands[0].interfaces[0].maximum,3);
  assert.deepEqual(plain(integrationChannels('text',page)).map(c=>c.output),[2]);assert.deepEqual(plain(integrationChannels('boolean',page)).map(c=>c.output),[4]);
  assert.deepEqual(plain(integrationChannels('rgb',catalogue)).map(c=>c.id),['rpm','rev_ratio','gear']);assert.deepEqual(plain(integrationChannels('boolean',catalogue)).map(c=>c.id),['warning']);
  assert.equal(normalizeIntegrationCatalogue().channels.length,0);assert.equal(normalizeIntegrationCatalogue({channels:catalogue.channels}).commands.length,0);
});
test('advertised action selection accepts generic enum and rejects wrong semantics or type',()=>{
  assert.deepEqual(plain(integrationOutputActions('threshold',actions)).map(a=>a.id),['led.set','generic.mode']);assert.deepEqual(plain(integrationOutputActions('rgb',actions)).map(a=>a.id),['rgb.set']);assert.deepEqual(plain(integrationOutputActions('text',actions)).map(a=>a.id),['display.set']);
  assert.equal(integrationOutputActions('boolean',{...actions,status:'unknown'}).length,0);assert.equal(integrationOutputActions('boolean').length,0);
  assert.equal(integrationOutputActions('boolean',{...actions,actions:[{...mode,parameters:[{type:'enum',options:['off']}]},{...boolean,readable:false},{...boolean,semantics:'oneShot'},{...boolean,parameters:[]}]}).length,0);
});
test('physical capabilities and Start guards support output-only targets with enabled HID',()=>{
  assert.deepEqual(plain(integrationPhysicalControls('button',events)).map(c=>c.value),[0,1]);assert.deepEqual(plain(integrationPhysicalControls('switch',events)).map(c=>c.value),[2]);assert.equal(integrationPhysicalControls('axis',events).length,1);assert.equal(integrationPhysicalControls('encoder',events).length,1);assert.equal(integrationPhysicalControls('button').length,0);
  const c=config();assert.equal(integrationStartError(c,[target]),'');assert.match(integrationStartError(c,[]),/Assign and reconnect/);
  assert.match(integrationStartError(c,[{...target,busy:true}]),/Wait/);assert.match(integrationStartError(c,[{...target,state:{...target.state,moduleActions:{}}}]),/Refresh/);
  const input=dcs();input.document.inputs=[{id:'input',role:'panel',kind:'button',control:0,command:'MASTER',interface:'set_state'}];assert.match(integrationStartError(input,[target]),/Disable/);
  const disabled={...target,module:{...target.module,enabled:false,published:false,directUsb:false}};assert.equal(integrationStartError(input,[disabled]),'');assert.match(integrationStartError(input,[{...disabled,state:{...disabled.state,moduleEvents:{}}}]),/advertised/);
  assert.match(integrationStartError(newIntegrationConfig(),[]),/roles/);
});
test('rev preset uses real ratio metadata and prefers advertised RGB with boolean fallback',()=>{
  const c=config();const preset=revLightOutput(c,'panel',target,catalogue);assert.equal(preset.channel,'rev_ratio');assert.equal(preset.format.kind,'rgb');assert.equal(preset.endpoint,'rgb');assert.equal(preset.format.bands.length,3);
  const boolTarget={...target,state:{...target.state,moduleActions:{[moduleId]:{...actions,actions:[mode]}}}};assert.equal(revLightOutput(c,'panel',boolTarget,catalogue).format.kind,'threshold');assert.equal(revLightOutput(c,'panel',target,{}),undefined);assert.equal(revLightOutput(c,'panel',{...target,state:{...target.state,moduleActions:{}}},catalogue),undefined);
});
test('synthetic byteframes preserve packet, epoch, raw gear, status and optional max RPM',()=>{
  const f=syntheticTelemetryFrame({packetId:4294967295,epoch:3,rpm:6300,gear:4,speed:124.25,maxRpm:7500,status:'live'});
  const p=new DataView(Uint8Array.from(f.physics).buffer),g=new DataView(Uint8Array.from(f.graphics).buffer),s=new DataView(Uint8Array.from(f.staticPage).buffer);
  assert.equal(f.sourceEpoch,3);assert.equal(p.getUint32(0,true),4294967295);assert.equal(p.getInt32(16,true),5);assert.equal(p.getInt32(20,true),6300);assert.equal(p.getFloat32(28,true),124.25);assert.equal(g.getInt32(4,true),2);assert.equal(s.getInt32(412,true),7500);
  for(const status of ['paused','replay','off']) assert.equal(syntheticTelemetryFrame({packetId:0,epoch:1,rpm:0,gear:-1,speed:0,maxRpm:0,status}).staticPage.length,416);
  for(const change of [{packetId:-1},{epoch:0},{rpm:100001},{gear:32},{speed:NaN},{speed:-1},{maxRpm:-1},{status:'other'}]) assert.throws(()=>syntheticTelemetryFrame({packetId:1,epoch:1,rpm:1,gear:0,speed:0,maxRpm:0,status:'live',...change}));
});
